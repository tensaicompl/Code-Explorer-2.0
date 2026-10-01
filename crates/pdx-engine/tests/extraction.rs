//! Extraction through the safe interface: every structure the engine reports arrives,
//! owned, and outlives the engine.

mod common;

use pdx_engine::{
    ChannelDirection, DefinitionKind, Engine, EngineError, FileExtract, FileStatus, Visibility,
};

fn extract(engine: &Engine, language: &str, rel_path: &str, source: &[u8]) -> FileExtract {
    engine
        .extract(language, rel_path, source)
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

#[test]
fn every_array_of_the_interface_crosses_the_boundary() {
    // Each of these sources holds at least one fact of each kind the engine reports;
    // between them, every array of the interface must arrive non-empty. The structures
    // are taken apart field by field in the wrapper, so a field the interface gains
    // cannot be dropped there without the build failing; this shows the data arrives.
    let engine = Engine::new().unwrap();
    let events = extract(
        &engine,
        "javascript",
        "events.js",
        br"const EventEmitter = require('events');
const emitter = new EventEmitter();

function subscribe() {
    emitter.on('user.created', (user) => console.log(user));
}

function publish(user) {
    if (!user) {
        throw new TypeError('no user');
    }
    emitter.emit('user.created', user);
    return process.env.API_KEY;
}
",
    );
    let settings = extract(
        &engine,
        "python",
        "settings.py",
        br#"import os
from typing import Optional


class Settings:
    def __init__(self) -> None:
        self.home: Optional[str] = os.environ.get("HOME")
        self.count = 0

    def bump(self) -> int:
        total = self.count + 1
        self.count = total
        return total
"#,
    );
    // Nesting past the depth the engine parses Perl to: skipped, with a diagnostic.
    let deep = format!("{}1{}", "f(".repeat(200), ")".repeat(200));
    let nested = extract(&engine, "perl", "deep.pl", deep.as_bytes());

    let all = [&events, &settings, &nested];
    let any = |f: fn(&FileExtract) -> bool| all.iter().any(|e| f(e));
    assert!(any(|e| !e.definitions.is_empty()), "no definitions");
    assert!(any(|e| !e.calls.is_empty()), "no calls");
    assert!(any(|e| !e.imports.is_empty()), "no imports");
    assert!(any(|e| !e.usages.is_empty()), "no usages");
    assert!(any(|e| !e.type_refs.is_empty()), "no type references");
    assert!(any(|e| !e.throws.is_empty()), "no throws");
    assert!(any(|e| !e.read_writes.is_empty()), "no reads or writes");
    assert!(any(|e| !e.channels.is_empty()), "no channels");
    assert!(
        any(|e| !e.env_accesses.is_empty()),
        "no configuration reads"
    );
    assert!(any(|e| !e.diagnostics.is_empty()), "no diagnostics");
    assert!(any(|e| !e.surface.as_bytes().is_empty()), "no surface");

    assert!(
        events
            .channels
            .iter()
            .any(|c| c.channel_text == "user.created" && c.direction == ChannelDirection::Listen)
    );
    assert!(
        events
            .channels
            .iter()
            .any(|c| c.channel_text == "user.created" && c.direction == ChannelDirection::Emit)
    );
    assert!(events.env_accesses.iter().any(|e| e.key == "API_KEY"));
    assert_eq!(nested.status, FileStatus::Failed);
    assert_eq!(nested.diagnostics.len(), 1);
}

#[test]
fn throws_name_their_exception_and_scope() {
    let engine = Engine::new().unwrap();
    let dir = common::fixtures().join("throws");
    for (file, language, exception, scope) in [
        ("Thrower.java", "java", "IllegalArgumentException", "read"),
        ("raiser.py", "python", "ValueError", "parse"),
        ("thrower.ts", "typescript", "RangeError", "check"),
    ] {
        let source = std::fs::read(dir.join(file)).unwrap();
        let e = extract(&engine, language, file, &source);
        let found = e.throws.iter().any(|t| {
            t.exception_text == exception
                && t.scope
                    .is_some_and(|i| e.definitions[i as usize].name == scope)
                && t.span.is_none()
        });
        assert!(
            found,
            "{file}: {exception} in {scope} not reported: {:?}",
            e.throws
        );
        assert!(!e.truncated, "{file}: truncated with no budget");
    }
}

#[test]
fn definitions_keep_every_field() {
    let engine = Engine::new().unwrap();
    let source =
        std::fs::read(common::fixtures().join("resolve/python_cross_file_calls/app/models.py"))
            .unwrap();
    let e = extract(&engine, "python", "app/models.py", &source);
    let class = e
        .definitions
        .iter()
        .find(|d| d.name == "User")
        .expect("the class");
    assert_eq!(class.kind, DefinitionKind::Class);
    assert_eq!(class.qualified_name, "app.models.User");
    assert_eq!(class.parent, None);
    let method = e
        .definitions
        .iter()
        .find(|d| d.name == "display_name")
        .expect("the method");
    assert_eq!(method.kind, DefinitionKind::Method);
    assert_eq!(
        method
            .parent
            .map(|i| e.definitions[i as usize].name.as_str()),
        Some("User")
    );
    let span = method.span.expect("a positioned method");
    assert_eq!(span.start_line, 2);
    assert!(span.start_byte < span.end_byte);
    assert_ne!(method.visibility, Visibility::Unknown);
    assert!(!method.engine_kind.is_empty());
}

#[test]
fn an_extraction_owns_everything_and_outlives_its_engine() {
    let source = b"def greet(name):\n    return 'hello ' + name\n";
    let e = {
        let engine = Engine::new().unwrap();
        extract(&engine, "python", "pkg/greet.py", source)
        // The engine shuts down here, freeing everything it allocated.
    };
    assert_eq!(e.language, "python");
    assert_eq!(e.rel_path, "pkg/greet.py");
    assert_eq!(e.source_len, source.len() as u64);
    assert_eq!(e.status, FileStatus::Parsed);
    let greet = e
        .definitions
        .iter()
        .find(|d| d.name == "greet")
        .expect("the function");
    assert_eq!(greet.qualified_name, "pkg.greet.greet");
    assert_eq!(greet.kind, DefinitionKind::Function);
    // A new engine on the same thread gives the same answer.
    let engine = Engine::new().unwrap();
    assert_eq!(extract(&engine, "python", "pkg/greet.py", source), e);
}

#[test]
fn an_extraction_survives_serialisation_whole() {
    let engine = Engine::new().unwrap();
    for (rel, language, source) in
        common::source_files(&common::fixtures().join("resolve/python_cross_file_calls"))
    {
        let e = extract(&engine, language, &rel, &source);
        let bytes = postcard::to_stdvec(&e).unwrap();
        let back: FileExtract = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, e);
        assert_eq!(postcard::to_stdvec(&back).unwrap(), bytes);
    }
}

#[test]
fn the_engines_refusals_are_typed() {
    let engine = Engine::new().unwrap();
    assert_eq!(
        engine.extract("no-such-language", "a.x", b""),
        Err(EngineError::UnknownLanguage("no-such-language".into()))
    );
    assert!(matches!(
        engine.extract("python", "a\0b.py", b""),
        Err(EngineError::InvalidArgument(_))
    ));
    assert!(Engine::knows_language("python"));
    assert!(!Engine::knows_language("no-such-language"));
    assert!(!Engine::version().is_empty());
    assert_eq!(engine.live_handles(), 0);
}
