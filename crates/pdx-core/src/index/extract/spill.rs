//! Where a build keeps its extractions' surfaces between Stage 2 and typed resolution
//! (P2-17, issue 82).
//!
//! A surface is the engine's own encoding of everything typed resolution reads from a
//! file, and the largest part of an extraction: on Kubernetes's packages about 20 times
//! the source, three fifths of what Stage 2 held. Only typed resolution reads it, once a
//! file, so Stage 2 moves each surface out of its extraction into one unnamed file
//! beside the build as the extraction completes, and typed resolution reads it back
//! when it adds the file. Nothing else changes: the extraction cache still stores the
//! whole record, surface included, before the surface is moved.
//!
//! The file is removed by the operating system when the spill is dropped, and is never
//! read by anything else. Where each surface is depends on the order workers finish in;
//! what is read back never does.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

/// The surfaces of one build's extractions, by path, in a file of their own.
pub struct SurfaceSpill {
    inner: Mutex<Inner>,
}

struct Inner {
    file: File,
    end: u64,
    at: HashMap<String, (u64, u64)>,
}

impl SurfaceSpill {
    /// A spill in an unnamed file in `dir`, which should be on the disk the build writes
    /// to rather than a memory-backed temporary directory.
    ///
    /// # Errors
    ///
    /// When the file cannot be created.
    pub fn create_in(dir: &Path) -> io::Result<Self> {
        Ok(Self::with_file(tempfile::tempfile_in(dir)?))
    }

    /// A spill in an unnamed file in the system's temporary directory.
    ///
    /// # Errors
    ///
    /// When the file cannot be created.
    pub fn create() -> io::Result<Self> {
        Ok(Self::with_file(tempfile::tempfile()?))
    }

    /// The spill's state. A user that panicked holding it left nothing half done that
    /// a later reader could misread: a surface is recorded only once it is written.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn with_file(file: File) -> Self {
        Self {
            inner: Mutex::new(Inner {
                file,
                end: 0,
                at: HashMap::new(),
            }),
        }
    }

    /// Keeps `bytes` as the surface of `path`. A path is kept once.
    ///
    /// # Errors
    ///
    /// When the file cannot be written, or `path` already has a surface.
    pub fn put(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let mut inner = self.lock();
        if inner.at.contains_key(path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path}: a surface is already spilled"),
            ));
        }
        let start = inner.end;
        inner.file.seek(SeekFrom::Start(start))?;
        inner.file.write_all(bytes)?;
        let len = bytes.len() as u64;
        inner.end = start + len;
        inner.at.insert(path.to_owned(), (start, len));
        Ok(())
    }

    /// The surface kept for `path`, or `None` when none was.
    ///
    /// # Errors
    ///
    /// When the file cannot be read.
    pub fn get(&self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let mut inner = self.lock();
        let Some(&(start, len)) = inner.at.get(path) else {
            return Ok(None);
        };
        let len = usize::try_from(len).map_err(io::Error::other)?;
        let mut bytes = vec![0; len];
        inner.file.seek(SeekFrom::Start(start))?;
        inner.file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }

    /// How many surfaces are kept.
    pub fn len(&self) -> usize {
        self.lock().at.len()
    }

    /// Whether none is.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many bytes are kept.
    pub fn bytes(&self) -> u64 {
        self.lock().end
    }
}

/// Spills are compared by identity: two reports are equal in this only when they kept
/// their surfaces in the same spill.
impl PartialEq for SurfaceSpill {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for SurfaceSpill {}

impl std::fmt::Debug for SurfaceSpill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SurfaceSpill({} surfaces)", self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_reads_back_as_it_was_kept() {
        let spill = SurfaceSpill::create().expect("a spill");
        spill.put("a.py", b"first").expect("kept");
        spill.put("b.py", b"").expect("kept");
        spill.put("c.py", b"third surface").expect("kept");
        assert_eq!(
            spill.get("c.py").expect("read").as_deref(),
            Some(&b"third surface"[..])
        );
        assert_eq!(
            spill.get("a.py").expect("read").as_deref(),
            Some(&b"first"[..])
        );
        assert_eq!(spill.get("b.py").expect("read").as_deref(), Some(&b""[..]));
        assert_eq!(spill.get("d.py").expect("read"), None);
        assert_eq!((spill.len(), spill.bytes()), (3, 18));
    }

    #[test]
    fn a_path_is_kept_once() {
        let spill = SurfaceSpill::create().expect("a spill");
        spill.put("a.py", b"x").expect("kept");
        assert!(spill.put("a.py", b"y").is_err());
        assert_eq!(spill.get("a.py").expect("read").as_deref(), Some(&b"x"[..]));
    }
}
