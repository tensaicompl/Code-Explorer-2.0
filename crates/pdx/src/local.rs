//! Where a local build writes: its segment, and its extraction cache.
//!
//! - The segment goes to `<repository root>/.pdx/segment.db` unless `--out` names
//!   another place: an absolute path as given, a relative one below the repository root
//!   (never the directory the command was run from). Only the default `.pdx` directory
//!   is created, and only when nothing is there: a `.pdx` that is a symlink, a file or
//!   anything but a directory is refused, never written through. `.pdx` ignores itself
//!   (a `.gitignore` of `*` inside it), so a build leaves the tree clean (issue 99).
//! - The default segment is built beside itself (`.pdx/segment.db.<pid>.partial`) and
//!   then replaces the previous one, so `pdx index` can run again after every commit
//!   (issue 99). A segment `--out` names is never overwritten: the segment writer
//!   refuses an existing destination.
//! - The extraction cache (4.5) is `pdx/extract` below the platform's cache directory:
//!   `$XDG_CACHE_HOME` or `~/.cache` on Linux, `~/Library/Caches` on macOS,
//!   `%LOCALAPPDATA%\pdx\cache` on Windows (4.3's local cache roots). With no usable
//!   cache directory, or one inside the repository (where discovery would find it),
//!   there is no cache: a cache is an optimisation, and changes nothing built.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The default segment, relative to the repository root, as reports show it.
pub const DEFAULT_OUTPUT: &str = ".pdx/segment.db";

/// Where a segment goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The path written.
    pub path: PathBuf,
    /// The path as reports show it: relative to the repository root as the user wrote
    /// it, or absolute as the user wrote it; never the checkout's own location.
    pub display: String,
    /// Whether it is the default `.pdx/segment.db`.
    pub default: bool,
}

/// Why the output cannot be prepared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputError {
    /// `--out` is not UTF-8, which a segment path must be.
    NotUtf8,
    /// `.pdx` exists and is not a plain directory.
    PdxNotADirectory,
    /// `.pdx` could not be created.
    PdxCreate,
    /// The new default segment could not replace the previous one.
    Replace,
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => f.write_str("--out must be a UTF-8 path"),
            Self::PdxNotADirectory => f.write_str(
                ".pdx exists and is not a directory (a symlink or a file is never written \
                 through); remove it or pass --out",
            ),
            Self::PdxCreate => f.write_str(".pdx could not be created in the repository"),
            Self::Replace => f.write_str(
                "the new segment could not replace .pdx/segment.db; the previous segment is \
                 left as it was",
            ),
        }
    }
}

/// The output for `--out` (or its absence), in the repository at `root`.
///
/// # Errors
///
/// [`OutputError::NotUtf8`] for an `--out` that is not UTF-8.
pub fn output(root: &Path, out: Option<&OsStr>) -> Result<Output, OutputError> {
    let Some(out) = out else {
        return Ok(Output {
            path: root.join(".pdx").join("segment.db"),
            display: DEFAULT_OUTPUT.to_owned(),
            default: true,
        });
    };
    let display = out.to_str().ok_or(OutputError::NotUtf8)?.to_owned();
    let given = Path::new(out);
    let path = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    Ok(Output {
        path,
        display,
        default: false,
    })
}

/// Makes the `.pdx` directory when the output is in it (the default, or `--out
/// .pdx/<name>`): kept when it is a plain directory, made when absent, refused
/// otherwise. `.pdx` ignores itself, and a partial file a stopped build left (a day old
/// or more, so never a running build's) is removed.
///
/// # Errors
///
/// [`OutputError::PdxNotADirectory`] or [`OutputError::PdxCreate`].
pub fn prepare(root: &Path, output: &Output) -> Result<(), OutputError> {
    let pdx = root.join(".pdx");
    if !output.default && output.path.parent() != Some(pdx.as_path()) {
        return Ok(());
    }
    match fs::symlink_metadata(&pdx) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&pdx).map_err(|_| OutputError::PdxCreate)?;
        }
        _ => return Err(OutputError::PdxNotADirectory),
    }
    // `.pdx` ignores itself, so what is written there never makes the tree dirty for
    // Git or any other tool. Only made when absent; never written through a symlink.
    let ignore = pdx.join(".gitignore");
    if fs::symlink_metadata(&ignore).is_err() {
        fs::write(&ignore, "*\n").map_err(|_| OutputError::PdxCreate)?;
    }
    let day = std::time::Duration::from_hours(24);
    if let Ok(entries) = fs::read_dir(&pdx) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("segment.db.") && name.ends_with(".partial")) {
                continue;
            }
            // Regular files only (a symlink is never followed), and only old ones.
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let old = metadata
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age >= day);
            if metadata.file_type().is_file() && old {
                let _ = remove_file(&entry.path());
            }
        }
    }
    Ok(())
}

/// Where the build writes: for the default output, a partial file beside the segment
/// it will replace, so the replacement is a rename within `.pdx`; else the output
/// itself.
pub fn staging_path(output: &Output) -> PathBuf {
    if output.default {
        output
            .path
            .with_file_name(format!("segment.db.{}.partial", std::process::id()))
    } else {
        output.path.clone()
    }
}

/// Puts the segment built at `staged` in place of the default output, replacing the
/// previous one; nothing for an explicit `--out`, which was written in place. The
/// previous segment is never removed before the new one is in its place.
///
/// # Errors
///
/// [`OutputError::Replace`] when the rename fails; the partial file is removed and the
/// previous segment is left as it was.
pub fn publish(staged: &Path, output: &Output) -> Result<(), OutputError> {
    if staged == output.path {
        return Ok(());
    }
    let mut replaced = fs::rename(staged, &output.path);
    if cfg!(windows) && replaced.is_err() {
        // Windows will not rename over a read-only file, and a segment is published
        // read-only: clear the attribute on the file itself (never through a link) and
        // try once more.
        if let Ok(metadata) = fs::symlink_metadata(&output.path)
            && metadata.file_type().is_file()
        {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&output.path, permissions);
        }
        replaced = fs::rename(staged, &output.path);
    }
    replaced.map_err(|_| {
        let _ = remove_file(staged);
        OutputError::Replace
    })
}

/// Removes a file the segment writer made read-only: on Unix unlinking needs only the
/// directory's permission; on Windows the file's read-only attribute is cleared first,
/// on the file itself. Never follows a symlink.
fn remove_file(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if cfg!(windows) && metadata.file_type().is_file() && metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
    fs::remove_file(path)
}

/// The platforms whose cache directory conventions differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Linux and other Unix systems: XDG.
    Xdg,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
}

impl Platform {
    /// This build's platform.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Xdg
        }
    }
}

/// The extraction cache's directory on `platform`, from the environment `var` reads;
/// `None` when no absolute cache directory is known.
pub fn extract_cache_dir(
    platform: Platform,
    var: impl Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let absolute = |p: PathBuf| p.is_absolute().then_some(p);
    let root = match platform {
        Platform::Xdg => var("XDG_CACHE_HOME")
            .and_then(absolute)
            .or_else(|| var("HOME").and_then(absolute).map(|h| h.join(".cache")))?
            .join("pdx"),
        Platform::MacOs => var("HOME")
            .and_then(absolute)?
            .join("Library")
            .join("Caches")
            .join("pdx"),
        Platform::Windows => var("LOCALAPPDATA")
            .and_then(absolute)?
            .join("pdx")
            .join("cache"),
    };
    Some(root.join("extract"))
}

/// The extraction cache's directory for a build of the repository at `root`: `None`
/// when there is none, or when it would be inside the repository.
pub fn extract_cache(root: &Path) -> Option<PathBuf> {
    let dir = extract_cache_dir(Platform::current(), |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })?;
    (!dir.starts_with(root)).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_belongs_to_the_repository() {
        let root = Path::new("/repo");
        let default = output(root, None).expect("an output");
        assert_eq!(default.path, Path::new("/repo/.pdx/segment.db"));
        assert_eq!(default.display, ".pdx/segment.db");
        assert!(default.default);
        let relative = output(root, Some(OsStr::new("artifacts/index.db"))).expect("an output");
        assert_eq!(relative.path, Path::new("/repo/artifacts/index.db"));
        assert_eq!(relative.display, "artifacts/index.db");
        assert!(!relative.default);
        #[cfg(unix)]
        {
            let absolute = output(root, Some(OsStr::new("/elsewhere/x.db"))).expect("an output");
            assert_eq!(absolute.path, Path::new("/elsewhere/x.db"));
            assert_eq!(absolute.display, "/elsewhere/x.db");
        }
    }

    #[test]
    fn the_cache_follows_each_platforms_convention() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| PathBuf::from(v))
            }
        };
        #[cfg(unix)]
        {
            assert_eq!(
                extract_cache_dir(
                    Platform::Xdg,
                    env(&[("XDG_CACHE_HOME", "/xdg"), ("HOME", "/h")])
                ),
                Some(PathBuf::from("/xdg/pdx/extract"))
            );
            assert_eq!(
                extract_cache_dir(Platform::Xdg, env(&[("HOME", "/h")])),
                Some(PathBuf::from("/h/.cache/pdx/extract"))
            );
            // A relative XDG_CACHE_HOME is not a cache directory (XDG requires absolute).
            assert_eq!(
                extract_cache_dir(
                    Platform::Xdg,
                    env(&[("XDG_CACHE_HOME", "rel"), ("HOME", "/h")])
                ),
                Some(PathBuf::from("/h/.cache/pdx/extract"))
            );
            assert_eq!(
                extract_cache_dir(Platform::MacOs, env(&[("HOME", "/Users/u")])),
                Some(PathBuf::from("/Users/u/Library/Caches/pdx/extract"))
            );
        }
        #[cfg(windows)]
        assert_eq!(
            extract_cache_dir(
                Platform::Windows,
                env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")])
            ),
            Some(PathBuf::from(r"C:\Users\u\AppData\Local\pdx\cache\extract"))
        );
        // Nothing known: no cache, never a directory in the repository.
        for platform in [Platform::Xdg, Platform::MacOs, Platform::Windows] {
            assert_eq!(extract_cache_dir(platform, env(&[])), None);
            assert_eq!(
                extract_cache_dir(platform, env(&[("HOME", "relative")])),
                None
            );
        }
    }

    #[test]
    fn the_default_pdx_is_made_only_as_a_plain_directory() {
        let root = tempfile::tempdir().expect("a directory");
        let default = output(root.path(), None).expect("an output");
        prepare(root.path(), &default).expect("made");
        assert!(root.path().join(".pdx").is_dir());
        prepare(root.path(), &default).expect("kept");
        let file = tempfile::tempdir().expect("a directory");
        fs::write(file.path().join(".pdx"), "x").expect("a file");
        assert_eq!(
            prepare(file.path(), &output(file.path(), None).expect("an output")),
            Err(OutputError::PdxNotADirectory)
        );
        // An explicit output makes no directory.
        let explicit = tempfile::tempdir().expect("a directory");
        let out = output(explicit.path(), Some(OsStr::new("x.db"))).expect("an output");
        prepare(explicit.path(), &out).expect("nothing to do");
        assert!(!explicit.path().join(".pdx").exists());
    }
}
