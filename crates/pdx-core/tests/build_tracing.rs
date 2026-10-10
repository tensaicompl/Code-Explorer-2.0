//! `build_segment`'s `tracing` spans (P2-10): one for the build and one per stage,
//! observational only. A test binary of its own, with one test, because a subscriber
//! installed for one thread races other threads' builds for `tracing`'s per-callsite
//! interest, which is process-wide.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use pdx_core::ids::RepoId;
use pdx_core::index::extract::ExtractLimits;
use pdx_core::index::{IndexRequest, build_segment};

/// A subscriber that records every span's name and every field's value.
#[derive(Default)]
struct Recorder {
    next: AtomicU64,
    spans: Mutex<Vec<String>>,
    values: Mutex<Vec<String>>,
}

struct Values<'a>(&'a Mutex<Vec<String>>);

impl tracing::field::Visit for Values<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .unwrap()
            .push(format!("{}={value:?}", field.name()));
    }
}

impl tracing::Subscriber for &'static Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        self.spans
            .lock()
            .unwrap()
            .push(span.metadata().name().to_owned());
        span.record(&mut Values(&self.values));
        tracing::span::Id::from_u64(self.next.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut Values(&self.values));
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        event.record(&mut Values(&self.values));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn request<'a>(root: &'a Path, out: &Path, name: &str) -> IndexRequest<'a> {
    IndexRequest {
        root,
        repo_id: RepoId::parse("ce63551447285fd4").expect("a repo id"),
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

#[test]
fn tracing_spans_cover_every_stage_and_change_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("pdx-trace-test-")
        .tempdir()
        .expect("a directory");
    let out = tempfile::tempdir().expect("an output directory");
    for (path, content) in [
        (
            "app/service.py",
            "def helper():\n    return 1\n\n\ndef run():\n    return helper()\n",
        ),
        ("config/.env.production", "MODE=1\n"),
        ("big/data.py", "x = 1\n"),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }

    // Traced: a span for the build and each stage, and nothing of the source or the
    // checkout's location in any field.
    let recorder: &'static Recorder = Box::leak(Box::default());
    let traced = tracing::subscriber::with_default(recorder, || {
        build_segment(request(dir.path(), out.path(), "traced.db")).expect("the build succeeds")
    });
    let spans = recorder.spans.lock().unwrap().clone();
    assert_eq!(
        spans,
        [
            "build_segment",
            "discover",
            "extract",
            "resolve",
            "derive",
            "write"
        ]
    );
    let values = recorder.values.lock().unwrap().join("\n");
    let root = dir.path().to_string_lossy().into_owned();
    for forbidden in [
        root.as_str(),
        "pdx-trace-test-",
        "def helper",
        "return helper",
        "MODE=1",
    ] {
        assert!(!values.contains(forbidden), "{forbidden} in {values}");
    }
    assert!(values.contains("files=3"), "{values}");

    // Untraced: the same bytes.
    let plain =
        build_segment(request(dir.path(), out.path(), "plain.db")).expect("the build succeeds");
    assert_eq!(traced.segment.content_sha256, plain.segment.content_sha256);
}
