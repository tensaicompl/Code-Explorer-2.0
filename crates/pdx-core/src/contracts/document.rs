//! Structured documents (YAML, JSON) as one plain tree, read safely: nothing a document
//! says is fetched, included, executed or expanded.
//!
//! YAML is read with an event parser and the tree is built here, so the rules are
//! ours: an alias is kept as an opaque [`Doc::Alias`] and never expanded (no document
//! can grow by reference), tags are ignored, and nesting deeper than
//! [`CONTRACT_DOCUMENT_MAX_DEPTH`] refuses the document. A scalar is its text as
//! written; only YAML's null spellings are [`Doc::Null`]. JSON is read with the JSON
//! parser and converted to the same tree.

use saphyr_parser::{Event, Parser, ScalarStyle};

use crate::consts::CONTRACT_DOCUMENT_MAX_DEPTH;

/// A node of a structured document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doc {
    /// A scalar, as its text.
    Scalar(String),
    /// A sequence.
    Seq(Vec<Doc>),
    /// A mapping, its entries in document order.
    Map(Vec<(Doc, Doc)>),
    /// A null.
    Null,
    /// A YAML alias, never expanded.
    Alias,
}

/// Why a document could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocError {
    /// Its syntax is not one the parser reads.
    Syntax,
    /// It nests deeper than the limit.
    TooDeep,
}

impl Doc {
    /// The scalar's text.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Scalar(s) => Some(s),
            _ => None,
        }
    }

    /// The value of a mapping's key, if the mapping has exactly one entry with it.
    pub fn get(&self, key: &str) -> Option<&Doc> {
        let Self::Map(entries) = self else {
            return None;
        };
        let mut found = entries
            .iter()
            .filter(|(k, _)| k.as_str() == Some(key))
            .map(|(_, v)| v);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// A mapping's entries with a scalar key, in document order; none for anything else.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Doc)> {
        let entries: &[(Doc, Doc)] = match self {
            Self::Map(entries) => entries,
            _ => &[],
        };
        entries
            .iter()
            .filter_map(|(k, v)| k.as_str().map(|k| (k, v)))
    }

    /// A sequence's items; none for anything else.
    pub fn items(&self) -> &[Doc] {
        match self {
            Self::Seq(items) => items,
            _ => &[],
        }
    }
}

/// Every document of a YAML stream.
///
/// # Errors
///
/// [`DocError::Syntax`] for a stream the parser cannot read, [`DocError::TooDeep`] for
/// one nesting past the limit.
pub fn parse_yaml(text: &str) -> Result<Vec<Doc>, DocError> {
    enum Open {
        Seq(Vec<Doc>),
        Map(Vec<(Doc, Doc)>, Option<Doc>),
    }
    fn place(stack: &mut [Open], docs: &mut Vec<Doc>, node: Doc) {
        match stack.last_mut() {
            Some(Open::Seq(items)) => items.push(node),
            Some(Open::Map(entries, key)) => match key.take() {
                Some(k) => entries.push((k, node)),
                None => *key = Some(node),
            },
            None => docs.push(node),
        }
    }
    let mut docs = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    for event in Parser::new_from_str(text) {
        let (event, _) = event.map_err(|_| DocError::Syntax)?;
        match event {
            Event::Scalar(value, style, _, _) => {
                let node = if style == ScalarStyle::Plain
                    && matches!(value.as_ref(), "" | "~" | "null" | "Null" | "NULL")
                {
                    Doc::Null
                } else {
                    Doc::Scalar(value.into_owned())
                };
                place(&mut stack, &mut docs, node);
            }
            Event::Alias(_) => place(&mut stack, &mut docs, Doc::Alias),
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                if stack.len() >= CONTRACT_DOCUMENT_MAX_DEPTH {
                    return Err(DocError::TooDeep);
                }
                stack.push(if matches!(event, Event::SequenceStart(..)) {
                    Open::Seq(Vec::new())
                } else {
                    Open::Map(Vec::new(), None)
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let node = match stack.pop() {
                    Some(Open::Seq(items)) => Doc::Seq(items),
                    Some(Open::Map(entries, None)) => Doc::Map(entries),
                    // A key with no value, or an end with nothing open.
                    _ => return Err(DocError::Syntax),
                };
                place(&mut stack, &mut docs, node);
            }
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd => {}
        }
    }
    if stack.is_empty() {
        Ok(docs)
    } else {
        Err(DocError::Syntax)
    }
}

/// A JSON document.
///
/// # Errors
///
/// [`DocError::Syntax`] for text that is not JSON (the parser's own nesting limit
/// included), [`DocError::TooDeep`] for one nesting past ours.
pub fn parse_json(text: &str) -> Result<Doc, DocError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| DocError::Syntax)?;
    from_json(&value, 0)
}

fn from_json(value: &serde_json::Value, depth: usize) -> Result<Doc, DocError> {
    use serde_json::Value;
    Ok(match value {
        Value::Null => Doc::Null,
        Value::Bool(b) => Doc::Scalar(b.to_string()),
        Value::Number(n) => Doc::Scalar(n.to_string()),
        Value::String(s) => Doc::Scalar(s.clone()),
        Value::Array(items) => {
            if depth >= CONTRACT_DOCUMENT_MAX_DEPTH {
                return Err(DocError::TooDeep);
            }
            Doc::Seq(
                items
                    .iter()
                    .map(|v| from_json(v, depth + 1))
                    .collect::<Result<_, _>>()?,
            )
        }
        Value::Object(map) => {
            if depth >= CONTRACT_DOCUMENT_MAX_DEPTH {
                return Err(DocError::TooDeep);
            }
            Doc::Map(
                map.iter()
                    .map(|(k, v)| Ok((Doc::Scalar(k.clone()), from_json(v, depth + 1)?)))
                    .collect::<Result<_, _>>()?,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_reads_mappings_sequences_and_nulls() {
        let docs = parse_yaml("a:\n  b: x\n  c: [1, two]\nn: ~\n---\nz: 1\n").expect("yaml");
        assert_eq!(docs.len(), 2);
        let a = docs[0].get("a").expect("a");
        assert_eq!(a.get("b").and_then(Doc::as_str), Some("x"));
        assert_eq!(a.get("c").map(Doc::items).map(<[Doc]>::len), Some(2));
        assert_eq!(docs[0].get("n"), Some(&Doc::Null));
    }

    #[test]
    fn yaml_aliases_are_never_expanded() {
        // A doubling bomb: each level refers to the one before twice.
        use std::fmt::Write as _;
        let mut text = String::from("a0: &a0 [x, x]\n");
        for i in 1..40 {
            let _ = writeln!(text, "a{i}: &a{i} [*a{}, *a{}]", i - 1, i - 1);
        }
        let docs = parse_yaml(&text).expect("yaml");
        assert_eq!(
            docs[0].get("a39").map(Doc::items),
            Some(&[Doc::Alias, Doc::Alias][..])
        );
    }

    #[test]
    fn deep_documents_are_refused() {
        let deep = "[".repeat(CONTRACT_DOCUMENT_MAX_DEPTH + 1);
        assert_eq!(parse_yaml(&deep), Err(DocError::TooDeep));
        let deep_json = format!("{}{}", "[".repeat(100), "]".repeat(100));
        assert_eq!(parse_json(&deep_json), Err(DocError::TooDeep));
    }

    #[test]
    fn malformed_documents_are_errors() {
        assert_eq!(parse_yaml("a: [1, 2\n"), Err(DocError::Syntax));
        assert_eq!(parse_json("{\"a\": "), Err(DocError::Syntax));
    }

    #[test]
    fn duplicate_keys_answer_nothing() {
        let docs = parse_yaml("k: 1\nk: 2\n").expect("yaml");
        assert_eq!(docs[0].get("k"), None);
    }
}
