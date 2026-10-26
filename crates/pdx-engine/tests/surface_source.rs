//! A project that asks for each file's surface only when it resolves the file (P2-17):
//! the answers are the ones every surface decoded whole gives, and a surface the
//! caller cannot give leaves its file unresolved, counted, never a silent gap.

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pdx_engine::{Engine, FileExtract, ProjectResolution, ProjectResolver, RunStatus};

const FILES: [(&str, &str); 3] = [
    (
        "app/models.py",
        "class User:\n    def save(self):\n        return 1\n\n\ndef make_user():\n    return User()\n",
    ),
    (
        "app/service.py",
        "from app.models import make_user\n\n\ndef create():\n    user = make_user()\n    user.save()\n    return user\n",
    ),
    (
        "app/main.py",
        "from app.service import create\n\n\ndef main():\n    return create()\n",
    ),
];

/// Every file extracted and taken through a cache, as a build holds them.
fn cached(engine: &Engine) -> Vec<FileExtract> {
    FILES
        .iter()
        .map(|(path, source)| {
            common::round_trip(&engine.extract("python", path, source.as_bytes()).unwrap())
        })
        .collect()
}

fn eager(engine: &Engine, files: &[FileExtract]) -> ProjectResolution {
    let mut project = ProjectResolver::new(engine).unwrap();
    for (extract, (_, source)) in files.iter().zip(FILES) {
        project.add_file(extract, source.as_bytes()).unwrap();
    }
    project.run().unwrap()
}

/// Every file added light, its surface given again by `give` when it resolves; which
/// paths were asked for, in order.
fn lazy(
    engine: &Engine,
    files: &[FileExtract],
    give: impl Fn(&str) -> Option<Vec<u8>> + 'static,
) -> (ProjectResolution, Vec<String>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&asked);
    let mut project = ProjectResolver::new(engine).unwrap();
    project
        .set_surface_source(move |path| {
            seen.borrow_mut().push(path.to_owned());
            give(path)
        })
        .unwrap();
    for (extract, (_, source)) in files.iter().zip(FILES) {
        project
            .add_file_with_surface(extract, source.as_bytes(), extract.surface.as_bytes())
            .unwrap();
    }
    let resolution = project.run().unwrap();
    let asked = asked.borrow().clone();
    (resolution, asked)
}

fn surfaces(files: &[FileExtract]) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|f| (f.rel_path.clone(), f.surface.as_bytes().to_vec()))
        .collect()
}

#[test]
fn surfaces_given_file_by_file_resolve_the_same() {
    let engine = Engine::new().unwrap();
    let files = cached(&engine);
    let whole = eager(&engine, &files);
    assert!(
        !whole.resolutions.is_empty(),
        "the fixture has typed answers"
    );
    let kept = surfaces(&files);
    let (lazily, asked) = lazy(&engine, &files, move |path| kept.get(path).cloned());
    assert_eq!(lazily, whole);
    let mut once: Vec<&str> = asked.iter().map(String::as_str).collect();
    once.sort_unstable();
    once.dedup();
    assert_eq!(
        once.len(),
        asked.len(),
        "each file is asked for once: {asked:?}"
    );
    assert_eq!(once, ["app/main.py", "app/models.py", "app/service.py"]);
}

#[test]
fn a_surface_not_given_leaves_its_file_unresolved_and_counted() {
    let engine = Engine::new().unwrap();
    let files = cached(&engine);
    let kept = surfaces(&files);
    let (resolution, _) = lazy(&engine, &files, move |path| {
        (path != "app/service.py")
            .then(|| kept.get(path).cloned())
            .flatten()
    });
    assert_eq!(resolution.health.status, RunStatus::Degraded);
    assert!(resolution.health.pass_failures > 0);
    assert!(
        resolution
            .resolutions
            .iter()
            .all(|r| r.site_ref.rel_path != "app/service.py"),
        "no answer for the file without its surface"
    );
}

#[test]
fn a_surface_source_comes_before_any_file() {
    let engine = Engine::new().unwrap();
    let files = cached(&engine);
    let mut project = ProjectResolver::new(&engine).unwrap();
    project.add_file(&files[0], FILES[0].1.as_bytes()).unwrap();
    assert!(project.set_surface_source(|_| None).is_err());
}
