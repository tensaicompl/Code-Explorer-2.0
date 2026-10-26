//! The local extraction cache's retention (P2-18, decision 85, issue 133): least recently
//! used entries go first, retired formats are removed, and nothing a running build has
//! read or written is evicted.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use pdx_core::config::PdxConfig;
use pdx_core::consts::EXTRACT_CACHE_FORMAT_VERSION;
use pdx_core::index::extract::{BlobSha, CacheKey, ExtractCache, FsCache, Pruned};
use pdx_core::secrets::SecretPolicyDigest;
use pdx_engine::{Engine, ExtractRecord};

fn key(path: &str, source: &[u8]) -> CacheKey {
    CacheKey::new(
        SecretPolicyDigest::of(&PdxConfig::default().secrets),
        "python",
        path,
        BlobSha::of(source),
    )
}

fn record(path: &str, source: &[u8]) -> ExtractRecord {
    Engine::new()
        .expect("an engine")
        .extract("python", path, source)
        .expect("an extraction")
        .record()
        .clone()
}

/// Stores an entry and dates its last use `age` ago.
fn stored(cache: &FsCache, name: &str, age: Duration) -> (CacheKey, PathBuf) {
    let source = format!("def {name}():\n    return 1\n");
    let key = key(&format!("{name}.py"), source.as_bytes());
    cache
        .store(&key, &record(&format!("{name}.py"), source.as_bytes()))
        .expect("stored");
    let path = cache.object_path(&key);
    date(&path, age);
    (key, path)
}

fn date(path: &Path, age: Duration) {
    fs::File::options()
        .write(true)
        .open(path)
        .expect("an entry")
        .set_modified(SystemTime::now() - age)
        .expect("dated");
}

fn size(path: &Path) -> u64 {
    fs::metadata(path).expect("an entry").len()
}

const HOUR: Duration = Duration::from_hours(1);

#[test]
fn cache_evicts_least_recent_first() {
    let dir = tempfile::tempdir().expect("a directory");
    let writer = FsCache::at(dir.path());
    let (_, oldest) = stored(&writer, "oldest", 3 * HOUR);
    let (_, middle) = stored(&writer, "middle", 2 * HOUR);
    let (_, newest) = stored(&writer, "newest", HOUR);
    let sizes = [size(&oldest), size(&middle), size(&newest)];
    let total: u64 = sizes.iter().sum();

    // Another build, later: none of the three is its own.
    let build = FsCache::at(dir.path());
    assert_eq!(build.stats().expect("stats").bytes, total);
    let pruned = build.prune(total - 1).expect("pruned");
    assert_eq!(
        pruned,
        Pruned {
            entries: 1,
            bytes: sizes[0],
            kept_bytes: sizes[1] + sizes[2],
        }
    );
    assert!(!oldest.exists() && middle.exists() && newest.exists());
    let pruned = build.prune(0).expect("pruned");
    assert_eq!((pruned.entries, pruned.kept_bytes), (2, 0));
    assert!(!middle.exists() && !newest.exists());
}

#[test]
fn a_hit_is_a_use() {
    let dir = tempfile::tempdir().expect("a directory");
    let writer = FsCache::at(dir.path());
    let (old_key, old) = stored(&writer, "old", 3 * HOUR);
    let (_, newer) = stored(&writer, "newer", 2 * HOUR);
    // A build reads the old entry: it is now the most recently used.
    let reader = FsCache::at(dir.path());
    assert!(reader.load(&old_key).expect("a hit").is_some());
    std::thread::sleep(Duration::from_millis(20));
    let next = FsCache::at(dir.path());
    let pruned = next.prune(size(&old)).expect("pruned");
    assert_eq!(pruned.entries, 1);
    assert!(
        old.exists() && !newer.exists(),
        "the entry not read goes first"
    );
}

#[test]
fn cache_never_evicts_the_running_build() {
    let dir = tempfile::tempdir().expect("a directory");
    let before = FsCache::at(dir.path());
    let (_, unused) = stored(&before, "unused", 2 * HOUR);
    let (read_key, read) = stored(&before, "read", 3 * HOUR);

    // The running build reads one old entry, writes one, and prunes to nothing.
    let build = FsCache::at(dir.path());
    assert!(build.load(&read_key).expect("a hit").is_some());
    date(&read, 3 * HOUR); // its use is what protects it, not its date
    let (_, written) = stored(&build, "written", 4 * HOUR);
    // Another build touched this one after the running build began.
    let (_, touched) = stored(&before, "touched", Duration::ZERO);
    date(&touched, Duration::ZERO);
    let pruned = build.prune(0).expect("pruned");
    assert_eq!(pruned.entries, 1);
    assert!(!unused.exists());
    assert!(read.exists() && written.exists() && touched.exists());
}

#[test]
fn cache_removes_retired_formats() {
    let dir = tempfile::tempdir().expect("a directory");
    let root = dir.path();
    let file = |path: &str, bytes: &[u8]| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(path, bytes).expect("a file");
    };
    file("v5/ab/one", b"12345");
    file("v8/cd/two", b"123");
    file(&format!("v{EXTRACT_CACHE_FORMAT_VERSION}/ef/current"), b"1");
    file("notes/kept", b"1");
    file("vx/kept", b"1");
    file("v/kept", b"1");
    file("v7", b"a file named like a format");
    let cache = FsCache::at(root);
    let stats = cache.stats().expect("stats");
    assert_eq!(stats.retired, [("v5".to_owned(), 5), ("v8".to_owned(), 3)]);
    let removed = cache.remove_retired_formats().expect("removed");
    assert_eq!(removed, [("v5".to_owned(), 5), ("v8".to_owned(), 3)]);
    assert!(!root.join("v5").exists() && !root.join("v8").exists());
    for kept in [
        format!("v{EXTRACT_CACHE_FORMAT_VERSION}/ef/current"),
        "notes/kept".to_owned(),
        "vx/kept".to_owned(),
        "v/kept".to_owned(),
        "v7".to_owned(),
    ] {
        assert!(root.join(&kept).exists(), "{kept}");
    }
    assert!(cache.remove_retired_formats().expect("again").is_empty());
}
