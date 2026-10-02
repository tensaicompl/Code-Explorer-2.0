//! Stage 1, discover (specification 4.5): which files of a checkout the index accounts
//! for, in what order, and what discovery may and may not read.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use pdx_core::config::PdxConfig;
use pdx_core::consts::MAX_FILE_BYTES;
use pdx_core::index::discover::{DiscoverError, DiscoveredFile, Disposition, discover};

// --- fixtures ----------------------------------------------------------------------

/// A checkout in a temporary directory.
struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new() -> Self {
        Self {
            dir: tempfile::Builder::new()
                .prefix("pdx-discover-test-")
                .tempdir()
                .expect("a directory"),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root().join(relative)
    }

    fn write(&self, relative: &str, content: impl AsRef<[u8]>) -> &Self {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
        self
    }

    fn config(&self) -> PdxConfig {
        PdxConfig::load(self.root()).expect("the configuration loads")
    }

    fn discover(&self) -> Vec<DiscoveredFile> {
        discover(self.root(), &self.config()).expect("discovery succeeds")
    }

    fn paths(&self) -> Vec<String> {
        self.discover().into_iter().map(|f| f.path).collect()
    }
}

fn find<'a>(files: &'a [DiscoveredFile], path: &str) -> &'a DiscoveredFile {
    files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} was not discovered"))
}

/// Holds a file unreadable for as long as it lives, so that discovery succeeding
/// proves it never opened the file: mode 000 on Unix, a handle that shares nothing on
/// Windows. `None` where the platform cannot deny the read (running as root).
struct Unreadable {
    path: PathBuf,
    #[cfg(windows)]
    _handle: File,
}

fn unreadable(path: &Path) -> Option<Unreadable> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("permissions");
        if File::open(path).is_ok() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).expect("permissions");
            eprintln!(
                "running as a user who can read anything: the no-read proof is not possible here"
            );
            return None;
        }
        Some(Unreadable {
            path: path.to_path_buf(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .expect("an exclusive handle");
        assert!(File::open(path).is_err(), "the file is still readable");
        Some(Unreadable {
            path: path.to_path_buf(),
            _handle: handle,
        })
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o644));
        }
        #[cfg(windows)]
        let _ = &self.path;
    }
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

// --- acceptance ------------------------------------------------------------------

#[test]
fn discover_honours_gitignore() {
    let checkout = Checkout::new();
    checkout
        .write("src/main.rs", "fn main() {}\n")
        // .gitignore: a file, a directory, a glob and a negation of it, nested files.
        .write(
            ".gitignore",
            "ignored.txt\nlogs/\n*.log\n!keep.log\n!node_modules/\n!vendor/\n",
        )
        .write("ignored.txt", "x")
        .write("logs/today.txt", "x")
        .write("app.log", "x")
        .write("keep.log", "x")
        .write("src/.gitignore", "local.rs\n")
        .write("src/local.rs", "x")
        .write("src/other.rs", "x")
        .write("docs/local.rs", "not under src, so not ignored")
        // .pdxignore: its own exclusions, and negations that must not re-include
        // anything another source excludes.
        .write(
            ".pdxignore",
            "pdx_only.txt\n!ignored.txt\n!generated/keep.rs\n!node_modules/\n",
        )
        .write("pdx_only.txt", "x")
        // [discover] extra_excludes.
        .write(
            "pdx.toml",
            "[discover]\nextra_excludes = [\"generated/**\"]\n",
        )
        .write("generated/out.rs", "x")
        .write("generated/keep.rs", "x")
        // Hard-coded excludes, at the root and deeper.
        .write("node_modules/left-pad/index.js", "x")
        .write("target/debug/build.rs", "x")
        .write("build/output.o", "x")
        .write("dist/bundle.js", "x")
        .write("vendor/lib/lib.go", "x")
        .write("packages/web/node_modules/react/index.js", "x")
        .write("packages/web/dist/app.js", "x")
        // A file named like an excluded directory is a file.
        .write("tools/build", "#!/bin/bash\necho build\n")
        // `.ignore` is not one of PDX's ignore files: it hides nothing.
        .write(".ignore", "visible.txt\nsrc/\n")
        .write("visible.txt", "x")
        // Hidden files are files.
        .write(".hidden.rs", "x")
        .write(".github/workflows/ci.yml", "on: push\n");

    let paths = checkout.paths();
    let expected = [
        ".github/workflows/ci.yml",
        ".gitignore",
        ".hidden.rs",
        ".ignore",
        ".pdxignore",
        "docs/local.rs",
        "keep.log",
        "pdx.toml",
        "src/.gitignore",
        "src/main.rs",
        "src/other.rs",
        "tools/build",
        "visible.txt",
    ];
    assert_eq!(paths, expected);

    // With `include_vendor`, vendor is indexed; nothing else hard-coded is.
    checkout.write(
        "pdx.toml",
        "[discover]\ninclude_vendor = true\nextra_excludes = [\"generated/**\"]\n",
    );
    let paths = checkout.paths();
    assert!(paths.contains(&"vendor/lib/lib.go".to_owned()), "{paths:?}");
    for excluded in [
        "node_modules/left-pad/index.js",
        "target/debug/build.rs",
        "build/output.o",
        "dist/bundle.js",
        "packages/web/node_modules/react/index.js",
    ] {
        assert!(!paths.contains(&excluded.to_owned()), "{excluded}");
    }
}

#[test]
fn discover_skips_symlinks() {
    let checkout = Checkout::new();
    let outside = tempfile::Builder::new()
        .prefix("pdx-outside-")
        .tempdir()
        .expect("a directory");
    let secret = outside.path().join("outside.rs");
    fs::write(&secret, "fn outside() {}\n").expect("a file");
    checkout
        .write("real.rs", "fn real() {}\n")
        .write("realdir/inner.rs", "fn inner() {}\n");

    let links: [(&Path, &str, bool); 4] = [
        (&checkout.path("real.rs"), "link_to_file.rs", false),
        (&checkout.path("realdir"), "link_to_dir", true),
        (outside.path(), "link_outside", true),
        (checkout.root(), "realdir/loop", true),
    ];
    for (target, link, is_dir) in links {
        let made = if is_dir {
            symlink_dir(target, &checkout.path(link))
        } else {
            symlink_file(target, &checkout.path(link))
        };
        made.unwrap_or_else(|e| {
            panic!(
                "cannot create the symlink {link} on this system ({e}); this test needs symlinks"
            )
        });
    }
    symlink_file(&secret, &checkout.path("link_outside_file.rs")).expect("a symlink");
    let _guard = unreadable(&secret);

    let files = checkout.discover();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    // The real file and directory once each; no link, nothing through a link, nothing
    // outside the root, and no endless descent through the loop.
    assert_eq!(paths, ["real.rs", "realdir/inner.rs"]);
}

#[test]
fn discover_marks_binary_and_large() {
    let checkout = Checkout::new();
    let limit = usize::try_from(MAX_FILE_BYTES).expect("small enough");
    checkout
        .write("small.rs", "fn small() {}\n")
        .write("exact.txt", vec![b'a'; limit])
        .write("over.txt", vec![b'a'; limit + 1])
        .write("image.bin", b"PNG\x00\x01\x02")
        .write("latin1.c", b"/* caf\xe9 */ int x;\n");
    let over = checkout.path("over.txt");
    let guard = unreadable(&over);

    let files = checkout.discover();
    assert_eq!(find(&files, "small.rs").disposition, Disposition::Candidate);
    assert_eq!(
        find(&files, "small.rs").language.map(|l| l.id),
        Some("rust")
    );
    // The limit is inclusive: a file of exactly the limit is read and indexed.
    let exact = find(&files, "exact.txt");
    assert_eq!(
        (exact.disposition, exact.size_bytes),
        (Disposition::Candidate, MAX_FILE_BYTES)
    );
    // One byte over is skipped, for its size, without being read: it is unreadable now.
    let over_file = find(&files, "over.txt");
    assert_eq!(
        (over_file.disposition, over_file.size_bytes),
        (Disposition::SkippedSize, MAX_FILE_BYTES + 1)
    );
    assert_eq!(over_file.disposition.reason(), Some("size"));
    drop(guard);
    // A NUL byte makes a file binary; bytes that are not UTF-8 do not.
    assert_eq!(find(&files, "image.bin").disposition, Disposition::Binary);
    let latin1 = find(&files, "latin1.c");
    assert_eq!(
        (latin1.disposition, latin1.language.map(|l| l.id)),
        (Disposition::Candidate, Some("c"))
    );

    // The limit is the repository's, when it sets one.
    checkout.write("pdx.toml", "[discover]\nmax_file_bytes = 14\n");
    let files = checkout.discover();
    assert_eq!(
        find(&files, "small.rs").disposition,
        Disposition::Candidate,
        "14 bytes, at the limit"
    );
    assert_eq!(
        find(&files, "latin1.c").disposition,
        Disposition::SkippedSize
    );
}

// --- secrets -----------------------------------------------------------------------

#[test]
fn secret_paths_are_redacted_without_being_read() {
    let checkout = Checkout::new();
    let marker = "PDX-SYNTHETIC-SECRET-NOT-A-REAL-CREDENTIAL";
    for path in [
        ".env",
        ".env.production",
        ".env.example",
        "config/server.pem",
        "private.key",
        "home/my_id_rsa_backup",
    ] {
        checkout.write(path, marker);
    }
    checkout
        .write("ordinary.env.example", "KEY=")
        .write("src/app.py", "print('hi')\n");
    let guard = unreadable(&checkout.path(".env"));
    let files = checkout.discover();
    for path in [
        ".env",
        ".env.production",
        ".env.example",
        "config/server.pem",
        "private.key",
        "home/my_id_rsa_backup",
    ] {
        assert_eq!(
            find(&files, path).disposition,
            Disposition::Redacted,
            "{path}"
        );
    }
    drop(guard);
    // `.env*` matches a name beginning `.env`, not one containing it.
    assert_eq!(
        find(&files, "ordinary.env.example").disposition,
        Disposition::Candidate
    );
    assert_eq!(
        find(&files, "src/app.py").disposition,
        Disposition::Candidate
    );
    // A redacted file keeps the language its name gives: `.env*` is `properties`.
    assert_eq!(
        find(&files, ".env").language.map(|l| l.id),
        Some("properties")
    );

    // Configured patterns add to the mandatory ones, and may name a directory.
    checkout
        .write(
            "pdx.toml",
            "[secrets]\npatterns = [\"*.secret\", \"credentials/\"]\n",
        )
        .write("deploy/db.secret", marker)
        .write("credentials/aws.txt", marker);
    let files = checkout.discover();
    for path in [
        "deploy/db.secret",
        "credentials/aws.txt",
        ".env",
        ".env.production",
        "config/server.pem",
        "private.key",
        "home/my_id_rsa_backup",
    ] {
        assert_eq!(
            find(&files, path).disposition,
            Disposition::Redacted,
            "{path}: added patterns extend the mandatory ones"
        );
    }
}

#[test]
fn repository_cannot_disable_default_secret_patterns() {
    // A repository's configuration is untrusted: an empty list, or a list of anything
    // else, removes none of the mandatory patterns.
    let checkout = Checkout::new();
    let marker = "PDX-SYNTHETIC-SECRET-NOT-A-REAL-CREDENTIAL";
    let secrets = [
        ".env",
        "deploy/.env.local",
        "tls/server.pem",
        "tls/server.key",
        "home/my_id_rsa",
    ];
    for path in secrets {
        checkout.write(path, marker);
    }
    for config in [
        "[secrets]\npatterns = []\n",
        "[secrets]\npatterns = [\"*.unrelated\"]\n",
    ] {
        checkout.write("pdx.toml", config);
        let guard = unreadable(&checkout.path(".env"));
        let files = checkout.discover();
        for path in secrets {
            assert_eq!(
                find(&files, path).disposition,
                Disposition::Redacted,
                "{config:?}: {path}"
            );
        }
        drop(guard);
    }
}

// --- languages, paths, order -----------------------------------------------------------

#[test]
fn unknown_languages_are_discovered() {
    let checkout = Checkout::new();
    checkout
        .write("README.weirdlanguage", "plain text\n")
        .write("Makefile", "all:\n");
    let files = checkout.discover();
    for path in ["README.weirdlanguage", "Makefile"] {
        let file = find(&files, path);
        assert_eq!(
            (file.language, file.disposition, file.language_id()),
            (None, Disposition::Candidate, "unknown")
        );
    }
}

#[test]
fn languages_come_from_names_content_and_configuration() {
    let checkout = Checkout::new();
    checkout
        .write("include/buffer.h", "struct buffer;\n")
        .write("include/buffer.cpp", "#include \"buffer.h\"\n")
        .write("include/plain.h", "struct plain;\n")
        .write("include/templated.h", "template<typename T> T id(T);\n")
        .write("bin/deploy", "#!/usr/bin/env bash\necho deploy\n")
        .write("views/index.blade.php", "<?php echo 1;\n")
        .write("ui/widget.component.rs", "x")
        // Another directory: on a case-insensitive filesystem the two names are one file.
        .write("ui/upper/widget.COMPONENT.rs", "x")
        .write("ui/Widget.tsx", "export const W = () => null;\n")
        .write(
            "pdx.toml",
            "[languages]\nextra = { \".blade.php\" = \"php\", \".component.rs\" = \"python\" }\n",
        );
    let files = checkout.discover();
    let language = |path: &str| find(&files, path).language.map(|l| l.id);
    assert_eq!(
        language("include/buffer.h"),
        Some("cpp"),
        "a .cpp sibling with the same basename"
    );
    assert_eq!(language("include/plain.h"), Some("c"));
    assert_eq!(
        language("include/templated.h"),
        Some("cpp"),
        "C++ in the content"
    );
    assert_eq!(language("bin/deploy"), Some("bash"), "the shebang");
    assert_eq!(language("views/index.blade.php"), Some("php"));
    assert_eq!(
        language("ui/widget.component.rs"),
        Some("python"),
        "the longer configured suffix wins"
    );
    assert_eq!(
        language("ui/upper/widget.COMPONENT.rs"),
        Some("rust"),
        "suffixes match case-sensitively"
    );
    assert_eq!(language("ui/Widget.tsx"), Some("typescript"));
}

#[test]
fn paths_are_relative_with_slashes_and_sorted() {
    let checkout = Checkout::new();
    // Written out of order, with nesting, case and non-ASCII names.
    for path in [
        "zeta.rs",
        "b/z.rs",
        "Zed.rs",
        "a/b/c/d.rs",
        "ü.txt",
        "a/a.rs",
        "B.txt",
        "a.rs",
        "a/b.rs",
    ] {
        checkout.write(path, "x");
    }
    let paths = checkout.paths();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(
        paths, sorted,
        "sorted by bytes, not by locale or filesystem order"
    );
    assert_eq!(
        paths,
        [
            "B.txt",
            "Zed.rs",
            "a.rs",
            "a/a.rs",
            "a/b.rs",
            "a/b/c/d.rs",
            "b/z.rs",
            "zeta.rs",
            "ü.txt"
        ]
    );
    for path in &paths {
        assert!(
            !path.starts_with('/') && !path.starts_with("./") && !path.contains('\\'),
            "{path}"
        );
    }
    // The same checkout, the same answer, every time.
    assert_eq!(checkout.discover(), checkout.discover());
}

// --- refusals ----------------------------------------------------------------------

#[test]
fn the_root_must_be_a_real_directory() {
    let checkout = Checkout::new();
    checkout.write("file.rs", "x").write("dir/a.rs", "x");
    let config = PdxConfig::default();
    assert!(matches!(
        discover(&checkout.path("missing"), &config),
        Err(DiscoverError::RootMissing(_))
    ));
    assert!(matches!(
        discover(&checkout.path("file.rs"), &config),
        Err(DiscoverError::RootNotADirectory(_))
    ));
    symlink_dir(&checkout.path("dir"), &checkout.path("linked")).expect("a symlink");
    assert!(matches!(
        discover(&checkout.path("linked"), &config),
        Err(DiscoverError::RootIsSymlink(_))
    ));
}

#[test]
fn ignore_files_are_never_followed() {
    let checkout = Checkout::new();
    let outside = tempfile::Builder::new()
        .prefix("pdx-outside-")
        .tempdir()
        .expect("a directory");
    fs::write(outside.path().join("rules"), "*.rs\n").expect("a file");
    checkout.write("a.rs", "x");
    symlink_file(&outside.path().join("rules"), &checkout.path(".pdxignore")).expect("a symlink");
    assert!(matches!(
        discover(checkout.root(), &PdxConfig::default()),
        Err(DiscoverError::IgnoreFile { .. })
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn a_path_that_is_not_utf8_is_refused() {
    use std::os::unix::ffi::OsStrExt;
    let checkout = Checkout::new();
    checkout.write("ok.rs", "x");
    let name = std::ffi::OsStr::from_bytes(b"bad\xff.rs");
    fs::write(checkout.root().join(name), "x").expect("Linux accepts any bytes in a name");
    assert!(matches!(
        discover(checkout.root(), &PdxConfig::default()),
        Err(DiscoverError::NonUtf8Path(_))
    ));
}
