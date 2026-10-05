//! Entry points (Appendix B.2), as `props.is_entry_point`, wherever the facts prove one.
//!
//! The engine's own entry-point flag is kept and never cleared. Beside it:
//!
//! - a JVM `main(String[])` method, and C#'s `Main`;
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
//! Command-line subcommand handlers (clap, argparse, cobra) are not recognised: the
//! facts here do not tie a handler to its registration. Nothing is an entry point by its
//! name alone (`start`, `run`, `execute`).

use std::collections::BTreeSet;

use pdx_engine::{Call, DefinitionKind};
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
            let types: Vec<String> = definition
                .signature_param_types
                .iter()
                .map(|t| normalise_type(language.id, t))
                .collect();
            let jvm_main = matches!(language.id, "java" | "kotlin" | "scala" | "groovy")
                && definition.kind == DefinitionKind::Method
                && definition.name == "main"
                && matches!(types.as_slice(), [t] if t == "String[]" || t == "String..." || t == "Array");
            let dotnet_main = language.id == "csharp"
                && definition.kind == DefinitionKind::Method
                && definition.name == "Main";
            let spring_boot = definition.decorators.iter().any(|d| {
                d.trim_start_matches('@')
                    .split(['(', ' '])
                    .next()
                    .is_some_and(|n| n.rsplit('.').next() == Some("SpringBootApplication"))
            });
            if jvm_main || dotnet_main || spring_boot {
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
