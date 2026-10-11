//! `build_segment`'s `tracing` spans (P2-10, decision 35): one for the build and one per
//! stage, carrying stage names and aggregate counts only, observational only. A test
//! binary of its own, with one test, because a subscriber installed for one thread
//! races other threads' builds for `tracing`'s per-callsite interest, which is
//! process-wide.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use pdx_core::ids::RepoId;
use pdx_core::index::extract::ExtractLimits;
use pdx_core::index::{IndexRequest, SegmentStatus, build_segment};
use pdx_core::segment::RepoIdSource;

/// What `pdx-core` emitted: each span's name and the fields its callsite declares, the
/// fields recorded on each span, each event's fields, and every value, as written.
#[derive(Default)]
struct Recorder {
    next: AtomicU64,
    spans: Mutex<Vec<(String, Vec<String>)>>,
    names: Mutex<BTreeMap<u64, String>>,
    recorded: Mutex<BTreeMap<String, BTreeSet<String>>>,
    events: Mutex<Vec<Vec<String>>>,
    values: Mutex<Vec<String>>,
}

/// Collects the names and values of the fields a span or event records.
struct Fields<'a> {
    names: BTreeSet<String>,
    values: &'a Mutex<Vec<String>>,
}

impl tracing::field::Visit for Fields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.names.insert(field.name().to_owned());
        self.values
            .lock()
            .unwrap()
            .push(format!("{}={value:?}", field.name()));
    }
}

fn ours(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.target().starts_with("pdx_core")
}

impl tracing::Subscriber for &'static Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let metadata = span.metadata();
        if ours(metadata) {
            let name = metadata.name().to_owned();
            let declared = metadata
                .fields()
                .iter()
                .map(|f| f.name().to_owned())
                .collect();
            self.spans.lock().unwrap().push((name.clone(), declared));
            self.names.lock().unwrap().insert(id, name.clone());
            let mut fields = Fields {
                names: BTreeSet::new(),
                values: &self.values,
            };
            span.record(&mut fields);
            self.recorded
                .lock()
                .unwrap()
                .entry(name)
                .or_default()
                .extend(fields.names);
        }
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let Some(name) = self.names.lock().unwrap().get(&id.into_u64()).cloned() else {
            return;
        };
        let mut fields = Fields {
            names: BTreeSet::new(),
            values: &self.values,
        };
        values.record(&mut fields);
        self.recorded
            .lock()
            .unwrap()
            .entry(name)
            .or_default()
            .extend(fields.names);
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !ours(event.metadata()) {
            return;
        }
        let mut fields = Fields {
            names: BTreeSet::new(),
            values: &self.values,
        };
        event.record(&mut fields);
        self.events
            .lock()
            .unwrap()
            .push(fields.names.into_iter().collect());
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn request<'a>(root: &'a Path, out: &Path, name: &str) -> IndexRequest<'a> {
    IndexRequest {
        root,
        repo_id: RepoId::parse("ce63551447285fd4").expect("a repo id"),
        repo_id_source: RepoIdSource::Registered,
        repo_url: "https://git.example/acme/shop.git".to_owned(),
        repo_name: "shop".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        limits: ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        },
        cache: None,
        backend: None,
        destination: out.join(name),
        build_dir: None,
        progress: None,
    }
}

/// Field names no P2-10 span or event may carry: the repository's identity, URL, name
/// or commit, a location, a file, a source or a secret.
const FORBIDDEN: [&str; 15] = [
    "repo",
    "repo_id",
    "repo_url",
    "repo_name",
    "commit",
    "commit_sha",
    "root",
    "checkout",
    "destination",
    "path",
    "paths",
    "file",
    "source",
    "secret",
    "content",
];

#[test]
fn tracing_spans_cover_every_stage_and_change_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("pdx-trace-test-")
        .tempdir()
        .expect("a directory");
    let out = tempfile::tempdir().expect("an output directory");
    // Four files, one of them over `max_file_bytes`: 25% skipped for size, so the
    // build is degraded and its warning is emitted too.
    for (path, content) in [
        ("pdx.toml", "[discover]\nmax_file_bytes = 64\n"),
        (
            "app/service.py",
            "def helper():\n    return 1\n\n\ndef run():\n    return helper()\n",
        ),
        ("config/.env.production", "MODE=1\n"),
        ("big/data.py", "x = 1\n".repeat(20).as_str()),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }

    let recorder: &'static Recorder = Box::leak(Box::default());
    let traced = tracing::subscriber::with_default(recorder, || {
        build_segment(request(dir.path(), out.path(), "traced.db")).expect("the build succeeds")
    });
    assert_eq!(traced.status, SegmentStatus::Degraded);

    // The spans, in order, each with exactly the fields decision 35 allows: none on
    // the build, one aggregate count on each stage.
    let spans = recorder.spans.lock().unwrap().clone();
    let expected: Vec<(String, Vec<String>)> = [
        ("build_segment", &[][..]),
        ("discover", &["files"][..]),
        ("extract", &["files"][..]),
        ("resolve", &["call_sites"][..]),
        ("derive", &["nodes"][..]),
        ("write", &["bytes"][..]),
    ]
    .iter()
    .map(|(name, fields)| {
        (
            (*name).to_owned(),
            fields.iter().map(|f| (*f).to_owned()).collect(),
        )
    })
    .collect();
    assert_eq!(spans, expected);
    // Each stage records its count, and nothing else is recorded on any span.
    let written = recorder.recorded.lock().unwrap().clone();
    for (name, fields) in &expected {
        let got = written.get(name).cloned().unwrap_or_default();
        let want: BTreeSet<String> = fields.iter().cloned().collect();
        assert_eq!(got, want, "{name}");
    }
    // One event: the degraded warning, with its message and the number of reasons.
    let events = recorder.events.lock().unwrap().clone();
    assert_eq!(events, [vec!["message".to_owned(), "reasons".to_owned()]]);
    // No field anywhere is named for an identity, a location or content.
    let names: BTreeSet<&str> = spans
        .iter()
        .flat_map(|(_, fields)| fields.iter())
        .chain(events.iter().flatten())
        .map(String::as_str)
        .collect();
    for forbidden in FORBIDDEN {
        assert!(!names.contains(forbidden), "{forbidden} in {names:?}");
    }
    // And no value holds the checkout's location, the repository's identity or
    // source text: the counts are 4 files, then what each stage produced.
    let values = recorder.values.lock().unwrap().join("\n");
    let root = dir.path().to_string_lossy().into_owned();
    for forbidden in [
        root.as_str(),
        "pdx-trace-test-",
        "ce63551447285fd4",
        "0123456789abcdef",
        "git.example",
        "def helper",
        "return helper",
        "MODE=1",
    ] {
        assert!(!values.contains(forbidden), "{forbidden} in {values}");
    }
    assert!(values.contains("files=4"), "{values}");

    // Untraced: the same bytes, coverage and status.
    let plain =
        build_segment(request(dir.path(), out.path(), "plain.db")).expect("the build succeeds");
    assert_eq!(traced.segment.content_sha256, plain.segment.content_sha256);
    assert_eq!(traced.coverage, plain.coverage);
    assert_eq!(traced.status, plain.status);
}
