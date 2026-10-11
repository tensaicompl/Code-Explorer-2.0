//! The harness's disposable working area, `bench/.cache/` (git-ignored), and the only
//! file operations the harness performs there.
//!
//! ```text
//! bench/.cache/repos/<lang>/<repo>/                 pinned checkouts
//! bench/.cache/segments/<lang>/<repo>/segment.db    what pdx index wrote
//! bench/.cache/generated/<lang>/<repo>.expected.json  goldens staged before promotion
//! bench/.cache/no-hooks/                            an empty hooks directory for Git
//! bench/.cache/git-global                           an empty global Git configuration
//! ```
//!
//! Every directory is created or accepted only as a real directory, never through a
//! symlink, a file or anything else: a planted link is refused before anything below
//! it is read, reset, cleaned or written. Nothing is deleted but a segment file below
//! `segments/`, and nothing is ever deleted recursively.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::plan::GoldenEntry;

/// Why a cache path cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The path is a symlink, a file or other non-directory where a directory belongs,
    /// or a symlink or directory where a file belongs.
    #[error(
        "{0} is not what the harness expects there (a symlink, or the wrong kind of entry); refusing to use it"
    )]
    Refused(PathBuf),
    /// The path is not below the area it must be in.
    #[error("{0} is outside the harness's own area; refusing to touch it")]
    Outside(PathBuf),
    /// The file system refused.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: io::Error,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> CacheError + '_ {
    move |source| CacheError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `path` as a real directory: kept when it is one, created when absent, refused
/// when it is a symlink or anything else. Its parent must already be one.
///
/// # Errors
///
/// [`CacheError::Refused`] or [`CacheError::Io`].
pub fn ensure_dir(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(CacheError::Refused(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(io_error(path))
        }
        Err(e) => Err(io_error(path)(e)),
    }
}

/// Whether `path` is a real directory (not a symlink to one).
///
/// # Errors
///
/// [`CacheError::Refused`] when it exists as anything else; [`CacheError::Io`].
pub fn is_real_dir(path: &Path) -> Result<bool, CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_) => Err(CacheError::Refused(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(path)(e)),
    }
}

/// Writes `bytes` to `path`, a regular file or nothing: never through a symlink.
///
/// # Errors
///
/// [`CacheError::Refused`] for a symlink or directory there; [`CacheError::Io`].
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => return Err(CacheError::Refused(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(path)(e)),
    }
    fs::write(path, bytes).map_err(io_error(path))
}

/// Removes the regular file `path`, which must lie below `root` (lexically, with no
/// `..`): absent is fine, a symlink or directory is refused. Nothing else is removed.
///
/// # Errors
///
/// [`CacheError::Outside`], [`CacheError::Refused`] or [`CacheError::Io`].
pub fn remove_owned_file(root: &Path, path: &Path) -> Result<(), CacheError> {
    let below = path.strip_prefix(root).ok().filter(|rest| {
        rest.components().count() > 0
            && rest.components().all(|c| matches!(c, Component::Normal(_)))
    });
    if below.is_none() {
        return Err(CacheError::Outside(path.to_path_buf()));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(path).map_err(io_error(path))
        }
        Ok(_) => Err(CacheError::Refused(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(path)(e)),
    }
}

/// The harness's area, `bench/.cache/`, checked as a real directory.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// The cache at `root`, made when absent; its parent must exist.
    ///
    /// # Errors
    ///
    /// [`CacheError::Refused`] when `root` is a symlink, a file or anything but a
    /// directory; [`CacheError::Io`].
    pub fn open(root: &Path) -> Result<Self, CacheError> {
        ensure_dir(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The cache's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory `parts` below the root, every level a real directory, made when
    /// absent.
    fn dir(&self, parts: &[&str]) -> Result<PathBuf, CacheError> {
        let mut path = self.root.clone();
        for part in parts {
            path.push(part);
            ensure_dir(&path)?;
        }
        Ok(path)
    }

    /// The parent of an entry's checkout, made; the checkout itself is the caller's.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn checkout(&self, entry: &GoldenEntry) -> Result<PathBuf, CacheError> {
        Ok(self.dir(&["repos", &entry.language])?.join(&entry.repo))
    }

    /// The root every segment lies below.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn segments_root(&self) -> Result<PathBuf, CacheError> {
        self.dir(&["segments"])
    }

    /// Where an entry's segment is written: its directory made, the file the
    /// caller's.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn segment(&self, entry: &GoldenEntry) -> Result<PathBuf, CacheError> {
        Ok(self
            .dir(&["segments", &entry.language, &entry.repo])?
            .join("segment.db"))
    }

    /// Where an entry's golden is staged before it is compared or promoted.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn staged(&self, entry: &GoldenEntry) -> Result<PathBuf, CacheError> {
        Ok(self
            .dir(&["generated", &entry.language])?
            .join(entry.file_name()))
    }

    /// An empty directory Git is pointed at for hooks, so none ever runs.
    ///
    /// # Errors
    ///
    /// As [`ensure_dir`].
    pub fn no_hooks(&self) -> Result<PathBuf, CacheError> {
        self.dir(&["no-hooks"])
    }

    /// An empty file Git reads as its global configuration, so none of the user's
    /// settings (line endings, filters, aliases, hooks, credential helpers) applies.
    /// Emptied on every call.
    ///
    /// # Errors
    ///
    /// As [`write_file`].
    pub fn git_global(&self) -> Result<PathBuf, CacheError> {
        let path = self.root.join("git-global");
        write_file(&path, b"")?;
        Ok(path)
    }
}
