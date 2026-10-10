//! The arguments of an annotation or attribute, as the engine records its text
//! (`@Table(name = "users")`, `@KafkaListener(topics = {"a", "b"})`, Kotlin's
//! `topics = ["a"]`, C#'s `Table("Users", Schema = "sales")`), read for literals only.
//!
//! A value is a string only when it is one string literal; anything else (a constant,
//! a concatenation, an expression, a text block) is [`Arg::Other`] and never guessed
//! at.

/// An argument's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// One string literal, unescaped.
    Str(String),
    /// An array of values (`{…}` or `[…]`).
    List(Vec<Arg>),
    /// Anything else, as written.
    Other(String),
}

impl Arg {
    /// Every string the value is: itself, or each item of an array; `None` when any
    /// part is not a literal.
    pub fn strings(&self) -> Option<Vec<&str>> {
        match self {
            Self::Str(s) => Some(vec![s.as_str()]),
            Self::List(items) => items
                .iter()
                .map(|i| match i {
                    Self::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect(),
            Self::Other(_) => None,
        }
    }
}

/// An annotation's arguments, each with its name when it has one.
pub type Args = Vec<(Option<String>, Arg)>;

/// An annotation's name (its last segment, without `@` or brackets) and arguments.
/// `None` for text that is not an annotation.
pub fn parse(text: &str) -> Option<(String, Args)> {
    parse_in(text, false)
}

/// [`parse`], for a language whose double-quoted strings interpolate `$` (Kotlin,
/// Groovy): such a string is a literal only with every `$` escaped.
pub fn parse_in(text: &str, interpolates: bool) -> Option<(String, Args)> {
    let text = text.trim();
    let text = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    let text = text.strip_prefix('@').unwrap_or(text).trim();
    let (name, rest) = match text.find('(') {
        Some(i) => (&text[..i], Some(&text[i..])),
        None => (text, None),
    };
    let name = name.trim().rsplit(['.', ':']).next()?.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let args = match rest {
        None => Vec::new(),
        Some(rest) => {
            let inner = rest.trim().strip_prefix('(')?.strip_suffix(')')?;
            arguments(inner, interpolates)?
        }
    };
    Some((name.to_owned(), args))
}

/// The named argument `key`, or the unnamed first argument when `key` is `value`'s
/// stand-in (`@Table("users")` is `name`, `@KafkaListener("t")` is `topics`).
pub fn named<'a>(
    args: &'a [(Option<String>, Arg)],
    keys: &[&str],
    positional: bool,
) -> Option<&'a Arg> {
    args.iter()
        .find(|(k, _)| k.as_deref().is_some_and(|k| keys.contains(&k)))
        .map(|(_, v)| v)
        .or_else(|| {
            positional
                .then(|| args.first())
                .flatten()
                .filter(|(k, _)| k.is_none())
                .map(|(_, v)| v)
        })
}

/// The comma-separated arguments of an argument list.
fn arguments(inner: &str, interpolates: bool) -> Option<Args> {
    let mut out = Vec::new();
    for part in split_top(inner)? {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = match named_split(part) {
            Some((k, v)) => (Some(k.to_owned()), v),
            None => (None, part),
        };
        out.push((key, value_of(value.trim(), interpolates)?));
    }
    Some(out)
}

/// `key = value` split at its `=`, when the part starts with an identifier and `=`.
fn named_split(part: &str) -> Option<(&str, &str)> {
    let eq = part.find('=')?;
    let key = part[..eq].trim();
    let rest = &part[eq + 1..];
    let identifier = !key.is_empty()
        && key.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !rest.starts_with('=');
    identifier.then_some((key, rest))
}

/// A value: a string literal, an array, or anything else.
fn value_of(text: &str, interpolates: bool) -> Option<Arg> {
    if let Some(inner) = text
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .or_else(|| text.strip_prefix('[').and_then(|t| t.strip_suffix(']')))
    {
        let items = split_top(inner)?
            .into_iter()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| value_of(t, interpolates))
            .collect::<Option<Vec<_>>>()?;
        return Some(Arg::List(items));
    }
    let literal = if interpolates {
        script_literal(text)
    } else {
        string_literal(text)
    };
    Some(literal.map_or_else(|| Arg::Other(text.to_owned()), Arg::Str))
}

/// The text of one double- or single-quoted string literal, unescaped, if `text` is
/// exactly one; `None` for a text block, a prefixed (interpolated, verbatim, raw)
/// string, or anything around the literal. A `$` is a character.
pub fn string_literal(text: &str) -> Option<String> {
    literal(text, false)
}

/// [`string_literal`] in a language whose double-quoted strings interpolate `$`
/// (Groovy, Kotlin): an unescaped `$` there makes the string a template, not a literal.
pub fn script_literal(text: &str) -> Option<String> {
    literal(text, true)
}

fn literal(text: &str, interpolates: bool) -> Option<String> {
    let text = text.trim();
    let quote = text.chars().next()?;
    if !matches!(quote, '"' | '\'') || text.starts_with("\"\"\"") || text.len() < 2 {
        return None;
    }
    let mut out = String::new();
    let mut chars = text[1..].chars();
    loop {
        match chars.next()? {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                c @ ('\\' | '"' | '\'' | '/' | '$') => out.push(c),
                _ => return None,
            },
            c if c == quote => break,
            '$' if interpolates && quote == '"' => return None,
            c => out.push(c),
        }
    }
    chars.as_str().is_empty().then_some(out)
}

/// `text` split at commas outside quotes and brackets; `None` when they do not balance.
pub(crate) fn split_top(text: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    (depth == 0 && quote.is_none()).then(|| {
        parts.push(&text[start..]);
        parts
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_arrays_and_names() {
        let (name, args) =
            parse(r#"@KafkaListener(topics = {"a", "${b}"}, groupId = "g")"#).expect("parsed");
        assert_eq!(name, "KafkaListener");
        assert_eq!(
            named(&args, &["topics"], true).and_then(Arg::strings),
            Some(vec!["a", "${b}"])
        );
        let (name, args) = parse(r#"Table("Users", Schema = "sales")"#).expect("parsed");
        assert_eq!(name, "Table");
        assert_eq!(
            named(&args, &["name"], true),
            Some(&Arg::Str("Users".into()))
        );
        assert_eq!(
            named(&args, &["Schema"], false),
            Some(&Arg::Str("sales".into()))
        );
        let (_, args) = parse(r#"@KafkaListener(topics = ["kt"])"#).expect("kotlin");
        assert_eq!(
            named(&args, &["topics"], true).and_then(Arg::strings),
            Some(vec!["kt"])
        );
    }

    #[test]
    fn non_literals_are_never_strings() {
        let (_, args) = parse("@Table(name = Names.USERS)").expect("parsed");
        assert_eq!(named(&args, &["name"], true).and_then(Arg::strings), None);
        let (_, args) = parse(r#"@Table(name = "a" + "b")"#).expect("parsed");
        assert_eq!(named(&args, &["name"], true).and_then(Arg::strings), None);
        assert_eq!(script_literal(r#""orders-$env""#), None);
        assert_eq!(
            script_literal(r#""orders-\$env""#).as_deref(),
            Some("orders-$env")
        );
        assert_eq!(
            string_literal(r#""${orders.topic}""#).as_deref(),
            Some("${orders.topic}")
        );
        let (_, args) = parse_in(r#"@KafkaListener(topics = ["${t}"])"#, true).expect("kotlin");
        assert_eq!(named(&args, &["topics"], true).and_then(Arg::strings), None);
        assert_eq!(parse("@Table(name = \"x\""), None);
    }
}
