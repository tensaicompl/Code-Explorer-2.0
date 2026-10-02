//! Stage 1 of the indexing pipeline: discover (specification 4.5).
//!
//! Walks a checkout and returns every regular file the index will account for, sorted
//! by path, each with the language Appendix A (and the repository's `[languages]`)
//! assigns it and what discovery found: a candidate for extraction, too large, binary,
//! or a secret. Nothing here extracts, parses or hashes; a candidate is a file that
//! may be extracted, not one that has been.
//!
//! The walk depends on the checkout and its configuration only: never on the working
//! directory, the environment, the user's Git settings or the order the filesystem
//! lists entries in. Symlinks are never followed, directories that are excluded are
//! never entered, and a file's content is read only when it may be indexed, never more
//! than the size limit of it.
//!
//! What excludes a path, any one of which is enough:
//!
//! - a directory named `.git`, `node_modules`, `target`, `build`, `dist` or `vendor`, at
//!   any depth (`vendor` not when `[discover] include_vendor` is set), and `.git` as a
//!   file too;
//! - the `.gitignore` files of the directories down to it, with Git's precedence
//!   among them;
//! - the root `.pdxignore`;
//! - `[discover] extra_excludes`.
//!
//! Each source is applied on its own, so a negation in one never re-includes what
//! another excludes. Hidden files are not excluded, and no other ignore file is read
//! (`.ignore`, `.git/info/exclude` and the user's global one included).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, FileType};
use std::io::Read;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::config::PdxConfig;
use crate::consts::MAX_FILE_BYTES;
use crate::languages::Language;

/// Directory names never indexed, at any depth (4.5). `vendor` is indexed when
/// `[discover] include_vendor` is set.
pub const HARD_EXCLUDES: [&str; 6] = [".git", "node_modules", "target", "build", "dist", "vendor"];

/// The repository's own ignore file, at its root, in gitignore syntax.
pub const PDXIGNORE: &str = ".pdxignore";

const GITIGNORE: &str = ".gitignore";

/// What discovery found a file to be. None of these says it was extracted: that is
/// the next stage's to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// A text file the next stage may extract.
    Candidate,
    /// Larger than `[discover] max_file_bytes`: recorded as skipped, with the reason
    /// `size`, and never read.
    SkippedSize,
    /// It holds a NUL byte.
    Binary,
    /// Its path matches a secret pattern: indexed as `redacted`, and never read.
    Redacted,
}

impl Disposition {
    /// The reason a skipped file is recorded with: `size` for one too large.
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::SkippedSize => Some("size"),
            _ => None,
        }
    }
}

/// One file of the checkout, as discovery found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// Its path below the root: `/` between names, no leading `./`, never absolute.
    pub path: String,
    /// Its language, or `None` for one no rule assigns. A file of no language is
    /// still a file of the repository, and is discovered.
    pub language: Option<&'static Language>,
    /// What discovery found it to be.
    pub disposition: Disposition,
    /// Its size, from its metadata.
    pub size_bytes: u64,
}

impl DiscoveredFile {
    /// The language's id, or `unknown`, as `files.language` stores it.
    pub fn language_id(&self) -> &'static str {
        self.language.map_or("unknown", |l| l.id)
    }
}

/// Why a checkout could not be discovered.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// The root does not exist.
    #[error("{}: no such directory", .0.display())]
    RootMissing(PathBuf),
    /// The root is not a directory.
    #[error("{}: not a directory", .0.display())]
    RootNotADirectory(PathBuf),
    /// The root is a symlink; discovery is given the real directory.
    #[error("{}: a symlink; discovery walks a real directory only", .0.display())]
    RootIsSymlink(PathBuf),
    /// A path in the checkout is not UTF-8, so it has no path in the index.
    #[error("{}: not UTF-8, and a repository path must be", .0.display())]
    NonUtf8Path(PathBuf),
    /// An ignore file could not be used.
    #[error("{}: {reason}", path.display())]
    IgnoreFile {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// A directory or file could not be read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The directory or file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: std::io::Error,
    },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> DiscoverError + '_ {
    move |source| DiscoverError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Discovers the checkout at `root` under `config`.
///
/// Returns every regular file that no rule excludes, sorted by path, classified in
/// this order: a path matching a secret pattern is [`Disposition::Redacted`] and a
/// file larger than `max_file_bytes` [`Disposition::SkippedSize`], neither read; any
/// other file is read, at most `max_file_bytes` of it, and is
/// [`Disposition::Binary`] if it holds a NUL byte and a [`Disposition::Candidate`]
/// otherwise. Its language comes from its path and, for a file that was read, its
/// content.
///
/// # Errors
///
/// [`DiscoverError`] for a root that is missing, not a directory or a symlink; a path
/// that is not UTF-8; an ignore file that is a symlink, too large or malformed; or a
/// directory or file that cannot be read.
pub fn discover(root: &Path, config: &PdxConfig) -> Result<Vec<DiscoveredFile>, DiscoverError> {
    let metadata = match root.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(DiscoverError::RootMissing(root.to_path_buf()));
        }
        Err(source) => {
            return Err(DiscoverError::Io {
                path: root.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(DiscoverError::RootIsSymlink(root.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(DiscoverError::RootNotADirectory(root.to_path_buf()));
    }

    let rules = Rules::new(root, config)?;
    let mut found = Vec::new();
    let mut gitignores = Vec::new();
    walk(root, "", &rules, &mut gitignores, &mut found)?;
    found.sort_by(|a, b| a.path.cmp(&b.path));

    let paths: BTreeSet<&str> = found.iter().map(|f| f.path.as_str()).collect();
    let max = config.discover.max_file_bytes;
    let mut files = Vec::with_capacity(found.len());
    for file in &found {
        let directory = file.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let sibling_exists = |name: &str| {
            let sibling = if directory.is_empty() {
                name.to_owned()
            } else {
                format!("{directory}/{name}")
            };
            paths.contains(sibling.as_str())
        };
        let (disposition, content) = if rules.is_secret(&file.path) {
            (Disposition::Redacted, None)
        } else if file.size > max {
            (Disposition::SkippedSize, None)
        } else {
            let content = read_bounded(&file.absolute, max)?;
            match content {
                None => (Disposition::SkippedSize, None),
                Some(bytes) if bytes.contains(&0) => (Disposition::Binary, Some(bytes)),
                Some(bytes) => (Disposition::Candidate, Some(bytes)),
            }
        };
        // A file that was not read is classified from its path alone: no shebang or
        // header content is claimed for it.
        let language = config.languages.detect(
            &file.path,
            content.as_deref().unwrap_or(&[]),
            sibling_exists,
        );
        files.push(DiscoveredFile {
            path: file.path.clone(),
            language,
            disposition,
            size_bytes: file.size,
        });
    }
    Ok(files)
}

/// A regular file the walk found and no rule excluded.
struct Found {
    path: String,
    absolute: PathBuf,
    size: u64,
}

/// The exclusion sources other than the per-directory `.gitignore` files, and the
/// secret patterns.
struct Rules {
    include_vendor: bool,
    pdxignore: Gitignore,
    extra_excludes: Gitignore,
    secrets: Gitignore,
}

impl Rules {
    fn new(root: &Path, config: &PdxConfig) -> Result<Self, DiscoverError> {
        let pattern_error = |path: &Path| {
            let path = path.to_path_buf();
            move |e: ignore::Error| DiscoverError::IgnoreFile {
                path,
                reason: e.to_string(),
            }
        };
        let config_path = root.join(crate::config::CONFIG_FILE);
        let lines = |patterns: &[String]| -> Result<Gitignore, DiscoverError> {
            let mut builder = GitignoreBuilder::new(root);
            for pattern in patterns {
                builder
                    .add_line(None, pattern)
                    .map_err(pattern_error(&config_path))?;
            }
            builder.build().map_err(pattern_error(&config_path))
        };
        let pdxignore_path = root.join(PDXIGNORE);
        let pdxignore = match read_ignore_file(&pdxignore_path)? {
            None => Gitignore::empty(),
            Some(text) => {
                let mut builder = GitignoreBuilder::new(root);
                for line in text.lines() {
                    builder
                        .add_line(Some(pdxignore_path.clone()), line)
                        .map_err(pattern_error(&pdxignore_path))?;
                }
                builder.build().map_err(pattern_error(&pdxignore_path))?
            }
        };
        Ok(Self {
            include_vendor: config.discover.include_vendor,
            pdxignore,
            extra_excludes: lines(&config.discover.extra_excludes)?,
            secrets: lines(&config.secrets.patterns)?,
        })
    }

    fn hard_excluded(&self, name: &str, is_dir: bool) -> bool {
        match name {
            ".git" => true,
            "vendor" => is_dir && !self.include_vendor,
            _ => is_dir && HARD_EXCLUDES.contains(&name),
        }
    }

    /// Whether the root `.pdxignore` or the configured excludes exclude `path`.
    fn pdx_excluded(&self, absolute: &Path, is_dir: bool) -> bool {
        self.pdxignore.matched(absolute, is_dir).is_ignore()
            || self.extra_excludes.matched(absolute, is_dir).is_ignore()
    }

    fn is_secret(&self, path: &str) -> bool {
        self.secrets
            .matched_path_or_any_parents(path, false)
            .is_ignore()
    }
}

/// An ignore file's text: `None` when there is none. One that is a symlink, larger than
/// any repository file may be, or not UTF-8 is an error rather than silently unused.
fn read_ignore_file(path: &Path) -> Result<Option<String>, DiscoverError> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(DiscoverError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let refuse = |reason: &str| DiscoverError::IgnoreFile {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if metadata.file_type().is_symlink() {
        return Err(refuse("a symlink, which discovery does not follow"));
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    let bytes = read_bounded(path, MAX_FILE_BYTES)?
        .ok_or_else(|| refuse("larger than any repository file may be"))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| refuse("not UTF-8"))
}

/// A file's bytes, or `None` if it holds more than `limit`: never more than `limit + 1`
/// bytes are read, whatever its size has become.
fn read_bounded(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, DiscoverError> {
    let file = File::open(path).map_err(io(path))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io(path))?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

/// The entries of a directory, by name, with their types and sizes, which are never a
/// link's target's: the entry's own metadata is read, without opening the file.
fn entries(dir: &Path) -> Result<Vec<(OsString, FileType, u64)>, DiscoverError> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(io(dir))? {
        let entry = entry.map_err(io(dir))?;
        let metadata = entry.metadata().map_err(io(&entry.path()))?;
        out.push((entry.file_name(), metadata.file_type(), metadata.len()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Whether the `.gitignore` files in force exclude a path: the deepest one that has
/// a rule for it decides, as in Git.
fn git_excluded(gitignores: &[Gitignore], absolute: &Path, is_dir: bool) -> bool {
    for gitignore in gitignores.iter().rev() {
        match gitignore.matched(absolute, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

fn walk(
    dir: &Path,
    relative: &str,
    rules: &Rules,
    gitignores: &mut Vec<Gitignore>,
    found: &mut Vec<Found>,
) -> Result<(), DiscoverError> {
    let gitignore_path = dir.join(GITIGNORE);
    let pushed = match read_ignore_file(&gitignore_path)? {
        None => false,
        Some(text) => {
            let mut builder = GitignoreBuilder::new(dir);
            for line in text.lines() {
                builder
                    .add_line(Some(gitignore_path.clone()), line)
                    .map_err(|e| DiscoverError::IgnoreFile {
                        path: gitignore_path.clone(),
                        reason: e.to_string(),
                    })?;
            }
            let gitignore = builder.build().map_err(|e| DiscoverError::IgnoreFile {
                path: gitignore_path.clone(),
                reason: e.to_string(),
            })?;
            gitignores.push(gitignore);
            true
        }
    };

    for (name, file_type, size) in entries(dir)? {
        let absolute = dir.join(&name);
        let Some(name) = name.to_str() else {
            return Err(DiscoverError::NonUtf8Path(absolute));
        };
        let path = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        // Never followed: a link to a file, a directory, outside the root or to itself.
        if file_type.is_symlink() {
            continue;
        }
        let is_dir = file_type.is_dir();
        if !is_dir && !file_type.is_file() {
            continue; // a pipe, socket or device is not a file of the repository
        }
        if rules.hard_excluded(name, is_dir)
            || git_excluded(gitignores, &absolute, is_dir)
            || rules.pdx_excluded(&absolute, is_dir)
        {
            continue;
        }
        if is_dir {
            walk(&absolute, &path, rules, gitignores, found)?;
        } else {
            found.push(Found {
                path,
                absolute,
                size,
            });
        }
    }

    if pushed {
        gitignores.pop();
    }
    Ok(())
}
