//! The benchmark's own working area, `bench/.cache/bench/` (git-ignored, disposable):
//!
//! ```text
//! bench/.cache/bench/determinism/<lang>/<repo>/   cache-a/ cache-b/ a.db b.db warm.db build-*/
//! bench/.cache/bench/perf/<lang|scale>/<name>/sample-<n>/   cache/ segment.db build/
//! bench/.cache/bench/results/<version>/           result files staged before promotion
//! ```
//!
//! Every directory is accepted only as a real directory, never through a symlink (the
//! golden harness's [`ensure_dir`]). A build's directory is emptied before it is used:
//! only a directory below this area, and only by removing what the benchmark itself
//! writes there, regular files and directories, walked without following anything; a
//! symlink or any other kind of entry stops the run instead of being removed or
//! followed.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::plan::BenchEntry;
use crate::golden::cache::{CacheError, ensure_dir};

/// The deepest a build directory nests: the extraction cache is three levels below
/// it, so anything deeper is not the benchmark's.
const MAX_DEPTH: usize = 8;

/// The benchmark's area.
#[derive(Clone, Debug)]
pub struct Area {
    root: PathBuf,
}

impl Area {
    /// The area at `root` (`bench/.cache/bench`), made; its parent must be a real
    /// directory.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn open(root: &Path) -> Result<Self, CacheError> {
        ensure_dir(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The area's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, parts: &[&str]) -> Result<PathBuf, CacheError> {
        let mut path = self.root.clone();
        for part in parts {
            path.push(part);
            ensure_dir(&path)?;
        }
        Ok(path)
    }

    /// An entry's determinism directory, emptied.
    ///
    /// # Errors
    ///
    /// [`CacheError`] for an unsafe path or anything the benchmark did not write.
    pub fn fresh_determinism(&self, entry: &BenchEntry) -> Result<PathBuf, CacheError> {
        let [first, second] = entry.cache_components();
        let dir = self.dir(&["determinism", first, second])?;
        self.empty(&dir)?;
        Ok(dir)
    }

    /// An entry's directory for performance sample `n`, emptied.
    ///
    /// # Errors
    ///
    /// [`CacheError`] for an unsafe path or anything the benchmark did not write.
    pub fn fresh_sample(&self, entry: &BenchEntry, n: usize) -> Result<PathBuf, CacheError> {
        let [first, second] = entry.cache_components();
        let dir = self.dir(&["perf", first, second, &format!("sample-{n}")])?;
        self.empty(&dir)?;
        Ok(dir)
    }

    /// The directory result files are staged in for `version`, emptied.
    ///
    /// # Errors
    ///
    /// [`CacheError`] for an unsafe path or anything the benchmark did not write.
    pub fn fresh_staging(&self, version: &str) -> Result<PathBuf, CacheError> {
        let dir = self.dir(&["results", version])?;
        self.empty(&dir)?;
        Ok(dir)
    }

    /// A new directory `name` in `parent`, a directory of this area.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn subdirectory(parent: &Path, name: &str) -> Result<PathBuf, CacheError> {
        let dir = parent.join(name);
        ensure_dir(&dir)?;
        Ok(dir)
    }

    /// Removes everything in `dir`, which must lie below the area's root.
    fn empty(&self, dir: &Path) -> Result<(), CacheError> {
        let below = dir.strip_prefix(&self.root).ok().filter(|rest| {
            rest.components().count() > 0
                && rest.components().all(|c| matches!(c, Component::Normal(_)))
        });
        if below.is_none() {
            return Err(CacheError::Outside(dir.to_path_buf()));
        }
        for child in read_dir(dir)? {
            remove_owned(&child, 0)?;
        }
        Ok(())
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> CacheError + '_ {
    move |source| CacheError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn read_dir(dir: &Path) -> Result<Vec<PathBuf>, CacheError> {
    let mut children = Vec::new();
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        children.push(entry.map_err(io_error(dir))?.path());
    }
    Ok(children)
}

/// Removes `path`: a regular file, or a real directory and what it holds, never
/// followed through a link. Anything else stops.
fn remove_owned(path: &Path, depth: usize) -> Result<(), CacheError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error(path))?;
    let kind = metadata.file_type();
    if kind.is_file() {
        fs::remove_file(path).map_err(io_error(path))
    } else if kind.is_dir() && depth < MAX_DEPTH {
        for child in read_dir(path)? {
            remove_owned(&child, depth + 1)?;
        }
        fs::remove_dir(path).map_err(io_error(path))
    } else {
        Err(CacheError::Refused(path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::plan::Kind;

    fn entry() -> BenchEntry {
        BenchEntry {
            kind: Kind::Golden {
                language: "python".into(),
            },
            name: "flask".into(),
            url: "git.example/acme/flask".into(),
            commit: "a".repeat(40),
        }
    }

    #[test]
    fn a_build_directory_is_emptied_of_what_the_benchmark_wrote() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let area = Area::open(&tmp.path().join("bench")).expect("area");
        let dir = area.fresh_determinism(&entry()).expect("dir");
        assert!(dir.ends_with("determinism/python/flask"));
        fs::create_dir_all(dir.join("cache-a/v5/ab")).expect("mkdir");
        fs::write(dir.join("cache-a/v5/ab/object"), b"x").expect("write");
        fs::write(dir.join("a.db"), b"segment").expect("write");
        let again = area.fresh_determinism(&entry()).expect("emptied");
        assert_eq!(again, dir);
        assert_eq!(fs::read_dir(&dir).expect("read").count(), 0);
        let sample = area.fresh_sample(&entry(), 2).expect("sample");
        assert!(sample.ends_with("perf/python/flask/sample-2"));
        let staging = area.fresh_staging("0.1.0").expect("staging");
        assert!(staging.ends_with("results/0.1.0"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_never_followed_or_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let outside = tmp.path().join("precious");
        fs::create_dir(&outside).expect("mkdir");
        fs::write(outside.join("keep"), b"keep").expect("write");
        let area = Area::open(&tmp.path().join("bench")).expect("area");
        let dir = area.fresh_determinism(&entry()).expect("dir");
        std::os::unix::fs::symlink(&outside, dir.join("cache-a")).expect("symlink");
        assert!(matches!(
            area.fresh_determinism(&entry()),
            Err(CacheError::Refused(_))
        ));
        assert!(outside.join("keep").is_file());
        assert!(dir.join("cache-a").symlink_metadata().is_ok());

        // A planted link in place of an area directory is refused before anything below.
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join("real")).expect("mkdir");
        std::os::unix::fs::symlink(tmp.path().join("real"), tmp.path().join("bench"))
            .expect("symlink");
        assert!(matches!(
            Area::open(&tmp.path().join("bench")),
            Err(CacheError::Refused(_))
        ));
    }

    #[test]
    fn nothing_outside_the_area_is_emptied() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let area = Area::open(&tmp.path().join("bench")).expect("area");
        assert!(matches!(
            area.empty(tmp.path()),
            Err(CacheError::Outside(_))
        ));
        assert!(matches!(
            area.empty(&area.root().join("..").join("x")),
            Err(CacheError::Outside(_))
        ));
        assert!(matches!(
            area.empty(area.root()),
            Err(CacheError::Outside(_))
        ));
    }
}
