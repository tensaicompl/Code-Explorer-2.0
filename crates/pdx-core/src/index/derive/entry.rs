//! Entry points (Appendix B.2), as `props.is_entry_point`, wherever the facts prove one.
//!
//! An entry point is a graph semantic, decided here from evidence of Appendix B.2's
//! categories; the engine's own flag, which it sets on every exported JavaScript or
//! TypeScript declaration among others, is kept as `props.engine_entry_point` and never
//! makes one (issue 55). The categories:
//!
//! - a program's `main`, by its language's convention ([`is_main`]): Java's and Groovy's
//!   `main(String[])`, Scala's `main(Array[String])`, Kotlin's `main()` or
//!   `main(Array<String>)`, C#'s `Main()` or `Main(string[])`, C's `main` at file scope,
//!   C++'s `main` in the global namespace, Go's `func main()` at file scope, Rust's
//!   `fn main()` at file scope in a binary's root (`main.rs`, or a file under a `bin`
//!   directory);
//! - a class annotated `@SpringBootApplication`;
//! - a module-level variable a `FastAPI` or Flask application is constructed into, in a
//!   file that imports `fastapi` or `flask`;
//! - the definition (or file, at file scope) that calls `listen` on a receiver in a
//!   file that imports `express`;
//! - the definition that calls `NestFactory.create` in a file that imports
//!   `@nestjs/core`;
//! - an ASP.NET `Program.cs` with top-level statements, as its file;
//! - every route handler ([`super::routes`]) and every test ([`super::tests`]).
//!
//! Go's package clause is not among the extracted facts, so a `func main()` in a package
//! other than `main` (legal, and never run) is taken for one too; that is the one
//! approximation. Command-line subcommand handlers (clap, argparse, cobra) are not
//! recognised: the facts here do not tie a handler to its registration. Nothing is an
//! entry point by its name alone outside its language's convention (`start`, `run`,
//! `execute`, a Python `main`).

use std::collections::BTreeSet;

use pdx_engine::{Call, Definition, DefinitionKind};
use serde_json::Value;

use crate::ids::NodeId;
use crate::resolve::registry::{DefinitionRef, SymbolRegistry};
use crate::resolve::stages::split_callee;

use super::containment::{is_file_definition, normalise_type};
use super::routes::imports_module;
use super::{Builder, DeriveError, DeriveInput};

/// The node a call is made from: its definition's, or the file's at file scope.
fn caller_node(
    graph: &Builder,
    registry: &SymbolRegistry,
    path: &str,
    call: &Call,
) -> Result<NodeId, DeriveError> {
    let enclosing = call.caller.and_then(|index| {
        let definition = registry.extract(path)?.definitions.get(index as usize)?;
        (!is_file_definition(definition, path)).then(|| DefinitionRef {
            path: path.to_owned(),
            index,
        })
    });
    match enclosing {
        Some(r) => graph.definition_node(&r).cloned(),
        None => graph.file_node(path).cloned(),
    }
}

/// Whether a definition is its language's conventional program entry point.
pub fn is_main(
    registry: &SymbolRegistry,
    language: &str,
    path: &str,
    r: &DefinitionRef,
    definition: &Definition,
) -> bool {
    let types: Vec<String> = definition
        .signature_param_types
        .iter()
        .map(|t| normalise_type(language, t))
        .collect();
    let types: Vec<&str> = types.iter().map(String::as_str).collect();
    let function = definition.kind == DefinitionKind::Function;
    let method = definition.kind == DefinitionKind::Method;
    let top = definition.parent.is_none();
    let named = |n: &str| definition.name == n;
    match language {
        "java" | "groovy" => {
            method && named("main") && matches!(types.as_slice(), ["String[]" | "String..."])
        }
        "scala" => method && named("main") && types == ["Array[String]"],
        "kotlin" => {
            named("main")
                && matches!(types.as_slice(), [] | ["Array"])
                && ((function && top) || method)
        }
        "csharp" => method && named("Main") && matches!(types.as_slice(), [] | ["string[]"]),
        "c" => function && top && named("main"),
        "cpp" => {
            function
                && top
                && named("main")
                && registry
                    .module_of_definition(r)
                    .is_none_or(|m| m.name.is_empty())
        }
        "go" => function && top && named("main") && types.is_empty(),
        "rust" => {
            let mut parts = path.rsplit('/');
            let file = parts.next().unwrap_or(path);
            function
                && top
                && named("main")
                && types.is_empty()
                && (file == "main.rs" || parts.any(|d| d == "bin"))
        }
        _ => false,
    }
}

/// Marks every entry point the facts prove.
pub(crate) fn mark(
    graph: &mut Builder,
    input: &DeriveInput<'_>,
    tests: &BTreeSet<DefinitionRef>,
) -> Result<(), DeriveError> {
    let registry = input.registry;
    let mut entries: BTreeSet<NodeId> = BTreeSet::new();
    for test in tests {
        entries.insert(graph.definition_node(test)?.clone());
    }
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index,
            };
            let spring_boot = definition.decorators.iter().any(|d| {
                d.trim_start_matches('@')
                    .split(['(', ' '])
                    .next()
                    .is_some_and(|n| n.rsplit('.').next() == Some("SpringBootApplication"))
            });
            if is_main(registry, language.id, path, &r, definition) || spring_boot {
                entries.insert(graph.definition_node(&r)?.clone());
            }
        }

        // Python application objects.
        if language.id == "python"
            && (imports_module(registry, path, "fastapi")
                || imports_module(registry, path, "flask"))
        {
            for call in &extract.calls {
                let (_, name) = split_callee(&call.callee_text);
                let (false, "FastAPI" | "Flask", Some(span)) = (call.is_reference, name, call.span)
                else {
                    continue;
                };
                let line = span.start_line;
                for (index, definition) in extract.definitions.iter().enumerate() {
                    let holds = definition.kind == DefinitionKind::Variable
                        && definition.parent.is_none()
                        && definition
                            .span
                            .is_some_and(|s| s.start_line <= line && line <= s.end_line);
                    if holds && let Ok(index) = u32::try_from(index) {
                        let r = DefinitionRef {
                            path: path.to_owned(),
                            index,
                        };
                        entries.insert(graph.definition_node(&r)?.clone());
                    }
                }
            }
        }

        // Servers started by a call.
        let express = matches!(language.id, "javascript" | "typescript")
            && imports_module(registry, path, "express");
        let nest = matches!(language.id, "javascript" | "typescript")
            && imports_module(registry, path, "@nestjs/core");
        for call in &extract.calls {
            let (receiver, name) = split_callee(&call.callee_text);
            let listens = express && receiver.is_some() && name == "listen";
            let bootstraps = nest && receiver == Some("NestFactory") && name == "create";
            if !call.is_reference && (listens || bootstraps) {
                entries.insert(caller_node(graph, registry, path, call)?);
            }
        }

        // ASP.NET top-level statements.
        if language.id == "csharp"
            && path.rsplit('/').next() == Some("Program.cs")
            && extract.calls.iter().any(|c| {
                !c.is_reference
                    && c.caller.is_none_or(|i| {
                        extract
                            .definitions
                            .get(i as usize)
                            .is_some_and(|d| is_file_definition(d, path))
                    })
            })
        {
            entries.insert(graph.file_node(path)?.clone());
        }
    }
    for entry in entries {
        graph.set_prop(&entry, "is_entry_point", Value::from(true));
    }
    Ok(())
}
