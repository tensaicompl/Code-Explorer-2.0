//! The built engine archive is whole: our symbols only, each defined once, and
//! nothing missing that the system does not supply.
//!
//! These run against the archive `make engine` builds. They are required wherever
//! the archive is expected to exist: under `make check`, which builds it first and
//! sets `PDX_REQUIRE_ENGINE`, and in continuous integration. There a missing archive or
//! a missing tool fails the test. Elsewhere, so that a contributor who has not built
//! the C side is not blocked, they report and pass.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

/// Whether the archive must exist and be checked, rather than skipped when absent.
fn required() -> bool {
    std::env::var_os("PDX_REQUIRE_ENGINE").is_some() || std::env::var_os("CI").is_some()
}

/// Skips the test with a message, or fails it where the check is required.
fn unavailable(why: &str) {
    assert!(!required(), "{why}, and this check is required here");
    eprintln!("{why}; skipping, as this check is not required here");
}

/// The archive, if it has been built.
fn archive() -> Option<PathBuf> {
    let path = repo_root().join("target/engine/libpdxe.a");
    path.is_file().then_some(path)
}

/// One line of `nm` output: its symbol and its type letter.
fn nm_lines(archive: &Path, args: &[&str]) -> Option<Vec<(String, char)>> {
    let out = Command::new("nm").args(args).arg(archive).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                match fields.as_slice() {
                    [.., kind, name] if kind.len() == 1 => {
                        Some(((*name).to_owned(), kind.chars().next()?))
                    }
                    _ => None,
                }
            })
            .collect(),
    )
}

/// A symbol as the platform's C compiler names it, without the leading underscore
/// some platforms add.
fn bare(name: &str) -> &str {
    if cfg!(target_os = "macos") {
        name.strip_prefix('_').unwrap_or(name)
    } else {
        name
    }
}

/// The upstream prefix, assembled so this file does not carry it.
fn upstream_prefix() -> String {
    ['c', 'b', 'm'].iter().collect()
}

#[test]
fn engine_symbols_renamed() {
    let Some(archive) = archive() else {
        return unavailable("the engine archive is not built (make engine)");
    };
    let Some(defined) = nm_lines(&archive, &["-g", "--defined-only"]) else {
        return unavailable("nm is not available");
    };
    assert!(
        defined.len() > 1000,
        "only {} symbols in the archive, which cannot be the whole engine",
        defined.len()
    );
    let prefix = upstream_prefix();
    let offenders: Vec<&String> = defined
        .iter()
        .map(|(name, _)| name)
        .filter(|s| bare(s).to_ascii_lowercase().starts_with(&prefix))
        .collect();
    assert!(
        offenders.is_empty(),
        "{} symbol(s) still carry the upstream prefix, so a build of ours would \
         collide with a build of the original: {:?}",
        offenders.len(),
        offenders.iter().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn every_symbol_is_defined_once() {
    let Some(archive) = archive() else {
        return unavailable("the engine archive is not built (make engine)");
    };
    let Some(defined) = nm_lines(&archive, &["-g", "--defined-only"]) else {
        return unavailable("nm is not available");
    };
    // Strong definitions of code and data. Weak and common symbols may repeat by
    // design; everything else defined twice links whichever the linker meets first.
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, kind) in &defined {
        if matches!(kind, 'T' | 'D' | 'B' | 'R' | 'S') {
            *count.entry(bare(name)).or_default() += 1;
        }
    }
    let twice: Vec<(&str, usize)> = count.into_iter().filter(|(_, n)| *n > 1).collect();
    assert!(
        twice.is_empty(),
        "{} symbol(s) are defined in more than one object: {:?}",
        twice.len(),
        twice.iter().take(10).collect::<Vec<_>>()
    );
}

#[test]
fn nothing_is_missing_that_the_system_does_not_supply() {
    let Some(archive) = archive() else {
        return unavailable("the engine archive is not built (make engine)");
    };
    // Every object in the archive, linked into a program against the system's C,
    // mathematics, thread and C++ runtime libraries and nothing else. A static link
    // normally takes only the objects a program uses; taking all of them is what
    // makes this a check of the whole archive. The link fails on any symbol those
    // libraries do not supply and on any symbol defined twice.
    let dir = std::env::temp_dir().join(format!("pdxe-whole-archive-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    let main = dir.join("main.c");
    std::fs::write(&main, "int main(void) { return 0; }\n").expect("write main.c");
    let program = dir.join("whole");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let mut command = Command::new(&cc);
    command.arg(&main);
    if cfg!(target_os = "macos") {
        command.arg("-Wl,-all_load").arg(&archive).arg("-lc++");
    } else {
        command
            .arg("-Wl,--whole-archive")
            .arg(&archive)
            .arg("-Wl,--no-whole-archive")
            .arg("-lstdc++");
    }
    command.args(["-lm", "-lpthread", "-o"]).arg(&program);
    let Ok(out) = command.output() else {
        let _ = std::fs::remove_dir_all(&dir);
        return unavailable(&format!("the C compiler {cc} is not available"));
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "linking the whole archive against the system libraries alone failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .take(30)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_engine_symbol_is_left_for_someone_else_to_define() {
    let Some(archive) = archive() else {
        return unavailable("the engine archive is not built (make engine)");
    };
    let (Some(defined), Some(undefined)) = (
        nm_lines(&archive, &["-g", "--defined-only"]),
        nm_lines(&archive, &["-u"]),
    ) else {
        return unavailable("nm is not available");
    };
    // nm reports each object's references, including those a sibling object
    // satisfies. What nothing in the archive defines is left to the system; none of
    // it may be in a namespace the engine or its libraries own.
    let defined: std::collections::BTreeSet<&str> =
        defined.iter().map(|(name, _)| bare(name)).collect();
    let ours = [
        "pdxe",
        "ts_",
        "tree_sitter_",
        "yyjson",
        "mi_",
        "LZ4",
        "simplecpp",
    ];
    let gaps: Vec<&str> = undefined
        .iter()
        .map(|(name, _)| bare(name))
        .filter(|s| !defined.contains(s))
        .filter(|s| ours.iter().any(|p| s.starts_with(p)))
        .collect();
    assert!(
        gaps.is_empty(),
        "the archive leaves engine symbols undefined: {gaps:?}"
    );
}

/// Every function the public header declares.
fn interface_functions() -> Vec<String> {
    let header = std::fs::read_to_string(repo_root().join("engine/include/pdxe.h"))
        .expect("the public header reads");
    let mut names: Vec<String> = header
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('#') && !l.starts_with("typedef"))
        .filter_map(|l| {
            let open = l.find("pdxe_")?;
            let rest = &l[open..];
            let end = rest.find('(')?;
            let name = &rest[..end];
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
                .then(|| name.to_owned())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn every_archive_member_is_reachable_from_the_interface() {
    let Some(archive) = archive() else {
        return unavailable("the engine archive is not built (make engine)");
    };
    let functions = interface_functions();
    assert!(
        functions.len() >= 15,
        "only {} functions read from the public header",
        functions.len()
    );
    // A program that refers to every function the interface declares, and nothing
    // else. The linker pulls in exactly the archive members it needs, transitively,
    // and records them in its map; a member it never pulls in is code nothing can
    // reach, which the strip list exists to remove.
    let dir = std::env::temp_dir().join(format!("pdxe-reach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    let mut src = String::from("#include \"pdxe.h\"\nvoid *volatile roots[] = {\n");
    for f in &functions {
        writeln!(src, "    (void *){f},").expect("writing to a string cannot fail");
    }
    src.push_str("};\nint main(void) { return roots[0] == 0; }\n");
    let main = dir.join("roots.c");
    std::fs::write(&main, src).expect("write roots.c");
    let map = dir.join("link.map");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let map_flag = if cfg!(target_os = "macos") {
        format!("-Wl,-map,{}", map.display())
    } else {
        format!("-Wl,-Map,{}", map.display())
    };
    let runtime = if cfg!(target_os = "macos") {
        "-lc++"
    } else {
        "-lstdc++"
    };
    let status = Command::new(&cc)
        .arg(format!(
            "-I{}",
            repo_root().join("engine/include").display()
        ))
        .arg(&main)
        .arg(&archive)
        .args([runtime, "-lm", "-lpthread", &map_flag, "-o"])
        .arg(dir.join("roots"))
        .output();
    let Ok(out) = status else {
        let _ = std::fs::remove_dir_all(&dir);
        return unavailable(&format!("the C compiler {cc} is not available"));
    };
    assert!(
        out.status.success(),
        "linking the interface failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let map_text = std::fs::read_to_string(&map).expect("the link map reads");
    let _ = std::fs::remove_dir_all(&dir);
    let pulled: std::collections::BTreeSet<String> = map_text
        .match_indices("libpdxe.a(")
        .filter_map(|(i, m)| {
            let rest = &map_text[i + m.len()..];
            rest.find(')').map(|end| rest[..end].to_owned())
        })
        .collect();
    let members = Command::new("ar").arg("t").arg(&archive).output();
    let Ok(members) = members else {
        return unavailable("ar is not available");
    };
    let unreached: Vec<String> = String::from_utf8_lossy(&members.stdout)
        .lines()
        .filter(|m| !m.is_empty() && !m.starts_with("__.SYMDEF"))
        .filter(|m| !pulled.contains(*m))
        .map(str::to_owned)
        .collect();
    assert!(
        unreached.is_empty(),
        "archive members nothing in the interface reaches, candidates for \
         scripts/vendor/strip-list.txt: {unreached:?}"
    );
}
