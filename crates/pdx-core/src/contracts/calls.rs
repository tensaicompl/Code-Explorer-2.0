//! Evidence about a call, shared by every source that reads library calls (endpoint
//! registrations, HTTP clients, broker clients): which library the file imports, what a
//! receiver is declared as, and the literal values in the call's own arguments.
//!
//! A library call is recognised only by **provenance and shape** together: the file
//! imports the library, the call is one of its API's operations with the destination
//! where that API takes it, and, where the facts say what the receiver is declared as (a
//! parameter's type in its callable's signature), it is the library's type. A call
//! resolution sends to a definition of the repository is the repository's own, never a
//! library's. A receiver's name is never evidence.

use pdx_engine::{Call, CallArg, SiteRef};

use crate::index::derive::routes::imports_module;
use crate::resolve::registry::SymbolRegistry;
use crate::resolve::stages::split_callee;

use super::Context;
use super::annotation::{split_top, string_literal};

/// Whether the file imports `module` (or a module beneath it), in the language's own
/// path syntax: `.` or `/` (Java, Python, Go, JavaScript), `::` (Rust).
pub fn imports(registry: &SymbolRegistry, path: &str, module: &str) -> bool {
    imports_module(registry, path, module)
        || registry.imports_of(path).iter().any(|i| {
            i.module_text
                .strip_prefix(module)
                .is_some_and(|rest| rest.starts_with("::"))
        })
}

/// Whether the file imports any of `modules`.
pub fn imports_any(registry: &SymbolRegistry, path: &str, modules: &[&str]) -> bool {
    modules.iter().any(|m| imports(registry, path, m))
}

/// Whether a call is the repository's own: resolution drew it to a definition here.
pub(crate) fn is_internal(context: &Context<'_>, path: &str, index: usize) -> bool {
    let site = SiteRef {
        rel_path: path.to_owned(),
        call_index: u32::try_from(index).unwrap_or(u32::MAX),
    };
    context.internal.contains(&site)
}

/// The argument at a position (not a keyword argument).
pub fn positional(call: &Call, index: u32) -> Option<&CallArg> {
    call.args
        .iter()
        .find(|a| a.index == index && a.keyword.is_none())
}

/// The keyword argument `name`, or the positional one at `index`.
pub fn keyword_or<'a>(call: &'a Call, name: &str, index: u32) -> Option<&'a CallArg> {
    call.args
        .iter()
        .find(|a| a.keyword.as_deref() == Some(name))
        .or_else(|| positional(call, index))
}

/// An argument's string value: the engine's, or one plain string literal as written.
pub fn literal(arg: &CallArg) -> Option<String> {
    arg.value.clone().or_else(|| string_literal(&arg.expr))
}

/// The positional arguments of a call written as text (`f(a, b)`), split at top-level
/// commas: what follows the first `(` of `text`, up to its matching `)`, which must end
/// the text.
fn call_arguments(text: &str) -> Option<Vec<&str>> {
    let open = text.find('(')?;
    let inner = text[open + 1..].trim_end().strip_suffix(')')?;
    split_top(inner).map(|parts| parts.into_iter().map(str::trim).collect())
}

/// The literal first argument of a constructor written in an argument
/// (`new ProducerRecord<>("orders", key, value)`), when the constructed type is one of
/// `types`.
pub fn constructed_first(expr: &str, types: &[&str]) -> Option<String> {
    let rest = expr.trim().strip_prefix("new ")?.trim_start();
    let end = rest.find(['<', '('])?;
    if !types.contains(&rest[..end].trim()) {
        return None;
    }
    string_literal(call_arguments(rest)?.first()?)
}

/// The literal argument of a factory call written in an argument
/// (`session.createQueue("orders")`), when the factory is one of `factories`.
pub fn factory_literal(expr: &str, factories: &[&str]) -> Option<String> {
    let expr = expr.trim();
    let callee = &expr[..expr.find('(')?];
    let (_, name) = split_callee(callee);
    if !factories.contains(&name) {
        return None;
    }
    let args = call_arguments(expr)?;
    let [only] = args.as_slice() else {
        return None;
    };
    string_literal(only)
}

/// The literal strings of a list written as an argument: a list literal (`['a', 'b']`),
/// a Java list factory (`List.of(…)`, `Arrays.asList(…)`, `Set.of(…)`,
/// `Collections.singletonList(…)`), or one string literal. `None` when any item is not
/// a literal.
pub fn literal_list(expr: &str) -> Option<Vec<String>> {
    let expr = expr.trim();
    if let Some(one) = string_literal(expr) {
        return Some(vec![one]);
    }
    let items: Vec<&str> =
        if let Some(inner) = expr.strip_prefix('[').and_then(|e| e.strip_suffix(']')) {
            split_top(inner)?.into_iter().map(str::trim).collect()
        } else {
            let callee = &expr[..expr.find('(')?];
            if !matches!(
                callee,
                "List.of"
                    | "Set.of"
                    | "Arrays.asList"
                    | "Collections.singletonList"
                    | "Collections.singleton"
            ) {
                return None;
            }
            call_arguments(expr)?
        };
    let items: Vec<&str> = items.into_iter().filter(|i| !i.is_empty()).collect();
    if items.is_empty() {
        return None;
    }
    items.into_iter().map(string_literal).collect()
}

/// The value of `key` in an object literal written as an argument
/// (`{ topic: 'orders', messages }`); `None` for anything that is not one, or one with a
/// spread that could set the key.
pub fn object_value<'a>(expr: &'a str, key: &str) -> Option<&'a str> {
    let inner = expr.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut found = None;
    for part in split_top(inner)? {
        let part = part.trim();
        if part.starts_with("...") {
            return None;
        }
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        if k.trim().trim_matches(['"', '\'']) == key {
            found = Some(v.trim());
        }
    }
    found
}

/// The declared type (its simple name, generic arguments and package dropped) of a
/// receiver that is a parameter of the callable the call is made in, from the
/// signature the engine recorded (`(KafkaTemplate<String, String> template)`). `None`
/// when the receiver is not a simple parameter name (a field, a local, an expression),
/// or the signature does not declare it.
pub fn parameter_type(registry: &SymbolRegistry, path: &str, call: &Call) -> Option<String> {
    let (receiver, _) = split_callee(&call.callee_text);
    declared_parameter_type(registry, path, call, receiver?)
}

/// The declared type of `name` as a parameter of the callable `call` is made in, as
/// [`parameter_type`] reads it.
pub fn declared_parameter_type(
    registry: &SymbolRegistry,
    path: &str,
    call: &Call,
    name: &str,
) -> Option<String> {
    let receiver = name;
    if !receiver.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let caller = registry
        .extract(path)?
        .definitions
        .get(call.caller? as usize)?;
    let signature = caller.signature.as_deref()?;
    let inner = signature.trim().strip_prefix('(')?.strip_suffix(')')?;
    let go = registry.language(path).is_some_and(|l| l.id == "go");
    for parameter in parameters(inner) {
        let parameter = parameter.trim();
        // `name: Type` (Kotlin, TypeScript, Rust, Python), `name Type` (Go) or
        // `Type name` (Java, C#).
        let (name, declared) = if let Some((name, declared)) = parameter.split_once(':') {
            (name.trim(), declared.trim())
        } else if go {
            match parameter.split_once(char::is_whitespace) {
                Some((name, declared)) => (name.trim(), declared.trim()),
                None => continue,
            }
        } else {
            match parameter.rsplit_once(char::is_whitespace) {
                Some((declared, name)) => (name.trim(), declared.trim()),
                None => continue,
            }
        };
        let name = name.trim_start_matches("mut ").trim();
        if name != receiver {
            continue;
        }
        let declared = declared
            .split_whitespace()
            .filter(|w| !matches!(*w, "final" | "@NonNull" | "@Nullable" | "mut"))
            .collect::<Vec<_>>()
            .join(" ");
        let declared = declared.trim_start_matches(['*', '&']).trim();
        let base = declared.split(['<', '[']).next().unwrap_or(declared).trim();
        let simple = base.rsplit(['.', ':']).next().unwrap_or(base).trim();
        return (!simple.is_empty()).then(|| simple.to_owned());
    }
    None
}

/// A parameter list split at its top-level commas: outside parentheses, brackets,
/// braces and generic angle brackets.
fn parameters(list: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in list.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&list[start..]);
    out
}

/// Whether a call's receiver, when its declared type is known, is one of `types`; a
/// receiver whose type the facts do not give passes on provenance and shape alone.
pub fn receiver_may_be(registry: &SymbolRegistry, path: &str, call: &Call, types: &[&str]) -> bool {
    parameter_type(registry, path, call).is_none_or(|t| types.contains(&t.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_shapes() {
        assert_eq!(
            constructed_first(
                "new ProducerRecord<>(\"orders\", \"k\", v)",
                &["ProducerRecord"]
            )
            .as_deref(),
            Some("orders")
        );
        assert_eq!(
            constructed_first("new Other(\"orders\")", &["ProducerRecord"]),
            None
        );
        assert_eq!(
            factory_literal("session.createQueue(\"q\")", &["createQueue"]).as_deref(),
            Some("q")
        );
        assert_eq!(
            factory_literal("session.createQueue(name)", &["createQueue"]),
            None
        );
        assert_eq!(
            literal_list("List.of(\"a\", \"b\")"),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(literal_list("['a']"), Some(vec!["a".into()]));
        assert_eq!(literal_list("List.of(a)"), None);
        assert_eq!(
            object_value("{ topic: 'orders', messages: [] }", "topic"),
            Some("'orders'")
        );
        assert_eq!(object_value("{ ...base, topic: 'x' }", "topic"), None);
        assert_eq!(
            parameters("KafkaProducer<String, String> producer, Map<K, V> m"),
            ["KafkaProducer<String, String> producer", " Map<K, V> m"]
        );
    }
}
