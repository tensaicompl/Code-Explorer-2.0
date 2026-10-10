//! Table and column contracts (4.7.1).
//!
//! **Providers**, each a `Table` (and, where the source lists them, its `Column`s):
//!
//! - `CREATE TABLE` in a `.sql` file (DDL, Flyway migrations), its columns from the
//!   column list; a temporary table is no contract. Owned by the file.
//! - Liquibase changelogs (a file whose name or directory says `changelog`): XML,
//!   YAML or JSON `createTable`, with its `column`s. Owned by the file. A document type
//!   declaration is refused, so no entity is ever expanded or fetched.
//! - JPA: a class annotated `@Entity` and `@Table(name = …)`, in a file importing
//!   `jakarta.persistence` or `javax.persistence`. Owned by the class.
//! - Entity Framework: a class with `[Table("…")]`, in a file using
//!   `System.ComponentModel.DataAnnotations.Schema`. Owned by the class.
//! - `SQLAlchemy`: `__tablename__ = "…"` directly in a class body, in a file importing
//!   `sqlalchemy`. Owned by the class.
//! - Django: `db_table = "…"` in a model's `Meta`, in a file importing `django`. Owned
//!   by the model class.
//! - Prisma: each `model`, its table its `@@map("…")` or its name. Owned by the file.
//!
//! **Consumers**: a call's string argument that is SQL (it starts with `SELECT`,
//! `INSERT`, `UPDATE`, `DELETE`, `WITH` or `MERGE`) consumes every table it reads
//! (`FROM`, `JOIN`) or writes (`INSERT INTO`, `UPDATE`, `DELETE FROM`, `MERGE INTO`),
//! with that access in `props.access_modes`. A name that is a placeholder or a
//! common-table expression is not a table. A `FROM` list is read up to its first
//! join; an item listed after a join's condition is not (a table missed, never one
//! invented). Repositories bound to entities
//! (`JpaRepository<User, …>`) are not read: the facts keep no base class's type
//! arguments.
//!
//! Keys are [`super::identity::table_key`] and [`super::identity::column_key`]. The
//! namespace is the data source: a literal JDBC URL in the configuration, `exact`, and
//! the repository's declared `datasources`.

use std::collections::BTreeSet;

use pdx_engine::{Definition, DefinitionKind, SiteRef};

use crate::index::derive::DeriveError;
use crate::index::derive::calls::{call_site, ordinals};
use crate::index::derive::routes::imports_module;
use crate::kinds::{ContractKind, SiteKind};
use crate::model::ContractDirection;
use crate::resolve::registry::DefinitionRef;

use super::annotation::{self, string_literal};
use super::document::{self, Doc, DocError};
use super::identity::{column_key, table_key};
use super::source::{self, Read};
use super::{Context, ContractObservation, ContractProblem, DeclaredIdentities, Found, extension};

/// A table a source declares: its name as written (`schema.name`, quoted or not) and
/// its columns as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredTable {
    /// The table's name, as written.
    pub name: String,
    /// Its columns, as written.
    pub columns: Vec<String>,
}

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let exact: BTreeSet<String> = context.config.datasource().into_iter().collect();
    let identities = DeclaredIdentities::with_exact(&exact, &context.declared.datasources);
    let mut out = Vec::new();
    documents(context, found, &mut out)?;
    entities(context, &mut out)?;
    python_models(context, &mut out)?;
    consumers(context, &mut out)?;
    for observation in out {
        found.observe(observation.with_identities(identities.clone()));
    }
    Ok(())
}

/// Tables and columns as observations, owned by `owner` when given.
fn provided(
    table: &DeclaredTable,
    path: &str,
    framework: &'static str,
    raw_form: &str,
    owner: Option<&crate::ids::NodeId>,
    out: &mut Vec<ContractObservation>,
) {
    let Some(key) = table_key(&table.name) else {
        return;
    };
    let finish = |o: ContractObservation| {
        let o = o.with_evidence("frameworks", framework);
        match owner {
            Some(owner) => o.with_owner(owner.clone()),
            None => o,
        }
    };
    out.push(finish(ContractObservation::new(
        ContractKind::Table,
        key,
        ContractDirection::Provides,
        path,
        raw_form,
    )));
    for column in &table.columns {
        if let Some(key) = column_key(&table.name, column) {
            out.push(finish(ContractObservation::new(
                ContractKind::Column,
                key,
                ContractDirection::Provides,
                path,
                format!("{raw_form} {column}"),
            )));
        }
    }
}

/// Whether a file is a Liquibase changelog by its name or directory.
fn is_changelog(path: &str) -> bool {
    path.to_ascii_lowercase().contains("changelog")
        && matches!(extension(path), Some("xml" | "yaml" | "yml" | "json"))
}

fn documents(
    context: &Context<'_>,
    found: &mut Found,
    out: &mut Vec<ContractObservation>,
) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let sql = extension(path) == Some("sql");
        let prisma = extension(path) == Some("prisma");
        let changelog = is_changelog(path);
        if !(sql || prisma || changelog) {
            continue;
        }
        let text = match source::read(context.root, registry, path)? {
            Read::Text(text) => text,
            Read::NotUtf8 => {
                found.diagnose(path, ContractProblem::NotUtf8);
                continue;
            }
            Read::Unread => continue,
        };
        let owner = context.graph.file_node(path)?.clone();
        let (tables, framework) = if sql {
            let Some(tables) = create_tables(&text) else {
                found.diagnose(path, ContractProblem::Unparseable { format: "sql" });
                continue;
            };
            (tables, "sql-ddl")
        } else if prisma {
            let Some(tables) = prisma_models(&text) else {
                found.diagnose(path, ContractProblem::Unparseable { format: "prisma" });
                continue;
            };
            (tables, "prisma")
        } else {
            match liquibase(&text, path) {
                Ok(tables) => (tables, "liquibase"),
                Err(problem) => {
                    found.diagnose(path, problem);
                    continue;
                }
            }
        };
        for table in &tables {
            let raw = format!("CREATE TABLE {}", table.name);
            provided(table, path, framework, &raw, Some(&owner), out);
        }
    }
    Ok(())
}

// --- SQL --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Sql {
    /// A word: a keyword or an unquoted identifier.
    Word(String),
    /// A quoted identifier, with its quotes.
    Quoted(String),
    /// A string literal or anything else that is no name (a number, a parameter).
    Value,
    Punct(char),
}

/// SQL's tokens: comments, string literals and dollar-quoted bodies skipped; `None`
/// for one left unterminated.
fn sql_tokens(text: &str) -> Option<Vec<Sql>> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                loop {
                    if i + 1 >= chars.len() {
                        return None;
                    }
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            '\'' => {
                i += 1;
                loop {
                    match chars.get(i)? {
                        '\'' if chars.get(i + 1) == Some(&'\'') => i += 2,
                        '\'' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                out.push(Sql::Value);
            }
            '"' | '`' | '[' => {
                let close = if c == '[' { ']' } else { c };
                let start = i;
                i += 1;
                while *chars.get(i)? != close {
                    i += 1;
                }
                i += 1;
                out.push(Sql::Quoted(chars[start..i].iter().collect()));
            }
            '$' => {
                // A dollar-quoted body ($$ … $$, $tag$ … $tag$), or a parameter.
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if chars.get(j) == Some(&'$') {
                    let tag: String = chars[i..=j].iter().collect();
                    let rest: String = chars[j + 1..].iter().collect();
                    let end = rest.find(&tag)?;
                    i = j + 1 + rest[..end].chars().count() + tag.chars().count();
                } else {
                    i = j;
                }
                out.push(Sql::Value);
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$' | '#'))
                {
                    i += 1;
                }
                out.push(Sql::Word(chars[start..i].iter().collect()));
            }
            c if c.is_ascii_digit() => {
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                    i += 1;
                }
                out.push(Sql::Value);
            }
            c => {
                out.push(Sql::Punct(c));
                i += 1;
            }
        }
    }
    Some(out)
}

fn keyword(token: Option<&Sql>, word: &str) -> bool {
    matches!(token, Some(Sql::Word(w)) if w.eq_ignore_ascii_case(word))
}

/// A dotted name at `at`, as written, and the index after it.
fn sql_name(tokens: &[Sql], mut at: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    loop {
        match tokens.get(at)? {
            Sql::Word(w) | Sql::Quoted(w) => name.push_str(w),
            _ => return None,
        }
        at += 1;
        if tokens.get(at) == Some(&Sql::Punct('.')) {
            name.push('.');
            at += 1;
        } else {
            return Some((name, at));
        }
    }
}

/// Words that open a column list's constraint, not a column.
const CONSTRAINTS: [&str; 11] = [
    "CONSTRAINT",
    "PRIMARY",
    "UNIQUE",
    "FOREIGN",
    "CHECK",
    "KEY",
    "INDEX",
    "EXCLUDE",
    "LIKE",
    "FULLTEXT",
    "SPATIAL",
];

/// Every non-temporary `CREATE TABLE` of a SQL text, with its columns; `None` when the
/// text cannot be tokenised or a `CREATE TABLE` names nothing.
pub fn create_tables(text: &str) -> Option<Vec<DeclaredTable>> {
    let tokens = sql_tokens(text)?;
    let mut out = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if !keyword(tokens.get(at), "CREATE") {
            at += 1;
            continue;
        }
        let mut i = at + 1;
        let mut temporary = false;
        while let Some(Sql::Word(w)) = tokens.get(i) {
            let upper = w.to_ascii_uppercase();
            match upper.as_str() {
                "OR" | "REPLACE" | "GLOBAL" | "LOCAL" | "UNLOGGED" => i += 1,
                "TEMP" | "TEMPORARY" => {
                    temporary = true;
                    i += 1;
                }
                _ => break,
            }
        }
        if !keyword(tokens.get(i), "TABLE") {
            at = i;
            continue;
        }
        i += 1;
        if keyword(tokens.get(i), "IF") {
            if !(keyword(tokens.get(i + 1), "NOT") && keyword(tokens.get(i + 2), "EXISTS")) {
                return None;
            }
            i += 3;
        }
        let (name, after) = sql_name(&tokens, i)?;
        let (columns, after) = column_list(&tokens, after);
        if !temporary {
            out.push(DeclaredTable { name, columns });
        }
        at = after;
    }
    Some(out)
}

/// The column names of the list that opens at `at`, if one does, and the index after.
fn column_list(tokens: &[Sql], at: usize) -> (Vec<String>, usize) {
    if tokens.get(at) != Some(&Sql::Punct('(')) {
        return (Vec::new(), at);
    }
    let mut columns = Vec::new();
    let mut depth = 0usize;
    let mut expect_column = true;
    let mut i = at;
    while let Some(token) = tokens.get(i) {
        match token {
            Sql::Punct('(') => {
                depth += 1;
                i += 1;
                continue;
            }
            Sql::Punct(')') => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                continue;
            }
            Sql::Punct(',') if depth == 1 => expect_column = true,
            Sql::Word(w) | Sql::Quoted(w) if depth == 1 && expect_column => {
                if !CONSTRAINTS.iter().any(|c| w.eq_ignore_ascii_case(c)) {
                    columns.push(w.clone());
                }
                expect_column = false;
            }
            _ => {}
        }
        i += 1;
    }
    (columns, i)
}

/// What a SQL statement does to each table it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    /// It reads it.
    Read,
    /// It writes it.
    Write,
}

impl Access {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// The tables a SQL string reads and writes; empty for text that is not SQL.
pub fn sql_access(text: &str) -> BTreeSet<(String, Access)> {
    let mut out = BTreeSet::new();
    let Some(tokens) = sql_tokens(text) else {
        return out;
    };
    let starts = ["SELECT", "INSERT", "UPDATE", "DELETE", "WITH", "MERGE"];
    if !starts.iter().any(|s| keyword(tokens.first(), s)) {
        return out;
    }
    let mut ctes = BTreeSet::new();
    if keyword(tokens.first(), "WITH") {
        let mut i = 1;
        if keyword(tokens.get(i), "RECURSIVE") {
            i += 1;
        }
        while let Some(Sql::Word(w)) = tokens.get(i) {
            ctes.insert(w.to_ascii_lowercase());
            // Past the CTE's body.
            let Some(open) = tokens[i..].iter().position(|t| *t == Sql::Punct('(')) else {
                break;
            };
            let mut depth = 0usize;
            let mut j = i + open;
            while let Some(t) = tokens.get(j) {
                match t {
                    Sql::Punct('(') => depth += 1,
                    Sql::Punct(')') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
            if tokens.get(i) == Some(&Sql::Punct(',')) {
                i += 1;
            } else {
                break;
            }
        }
    }
    let mut deleting = false;
    for i in 0..tokens.len() {
        let access = if keyword(tokens.get(i), "DELETE") {
            deleting = true;
            continue;
        } else if keyword(tokens.get(i), "FROM") {
            if deleting {
                deleting = false;
                Access::Write
            } else {
                Access::Read
            }
        } else if keyword(tokens.get(i), "JOIN") {
            Access::Read
        } else if keyword(tokens.get(i), "INTO") || keyword(tokens.get(i), "UPDATE") {
            Access::Write
        } else {
            continue;
        };
        let mut at = i + 1;
        while let Some((name, after)) = sql_name(&tokens, at) {
            if (name.contains('.') || !ctes.contains(&name.to_ascii_lowercase()))
                && let Some(key) = table_key(&name).filter(|k| !is_reserved(k))
            {
                out.insert((key, access));
            }
            // A comma-separated `FROM a x, b y` list.
            let mut next = after;
            if keyword(tokens.get(next), "AS") {
                next += 1;
            }
            if matches!(tokens.get(next), Some(Sql::Word(w)) if !is_clause(w)) {
                next += 1;
            }
            if access == Access::Read && tokens.get(next) == Some(&Sql::Punct(',')) {
                at = next + 1;
            } else {
                break;
            }
        }
    }
    out
}

/// Words that begin a clause, never an alias.
fn is_clause(word: &str) -> bool {
    [
        "WHERE",
        "JOIN",
        "INNER",
        "LEFT",
        "RIGHT",
        "FULL",
        "CROSS",
        "ON",
        "GROUP",
        "ORDER",
        "HAVING",
        "LIMIT",
        "UNION",
        "SET",
        "VALUES",
        "SELECT",
        "USING",
        "WHEN",
        "RETURNING",
        "OFFSET",
        "FETCH",
        "WINDOW",
        "NATURAL",
        "OUTER",
        "LATERAL",
    ]
    .iter()
    .any(|c| word.eq_ignore_ascii_case(c))
}

/// Keys that are SQL words where a name was expected: a subquery's or function's.
fn is_reserved(key: &str) -> bool {
    matches!(
        key,
        "select" | "lateral" | "unnest" | "dual" | "values" | "set"
    )
}

// --- Liquibase and Prisma -----------------------------------------------------------

fn liquibase(text: &str, path: &str) -> Result<Vec<DeclaredTable>, ContractProblem> {
    match extension(path) {
        Some("xml") => liquibase_xml(text),
        Some("json") => document::parse_json(text)
            .map(|d| vec![d])
            .map_err(doc_problem("json"))
            .map(|docs| liquibase_doc(&docs)),
        _ => document::parse_yaml(text)
            .map_err(doc_problem("yaml"))
            .map(|docs| liquibase_doc(&docs)),
    }
}

fn doc_problem(format: &'static str) -> impl Fn(DocError) -> ContractProblem {
    move |e| match e {
        DocError::TooDeep => ContractProblem::TooDeep,
        DocError::Syntax => ContractProblem::Unparseable { format },
    }
}

fn liquibase_xml(text: &str) -> Result<Vec<DeclaredTable>, ContractProblem> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..roxmltree::ParsingOptions::default()
    };
    let document = roxmltree::Document::parse_with_options(text, options)
        .map_err(|_| ContractProblem::Unparseable { format: "xml" })?;
    if document.root_element().tag_name().name() != "databaseChangeLog" {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for node in document.descendants() {
        if node.tag_name().name() != "createTable" {
            continue;
        }
        let Some(table) = node.attribute("tableName") else {
            continue;
        };
        let name = match node.attribute("schemaName") {
            Some(schema) => format!("{schema}.{table}"),
            None => table.to_owned(),
        };
        let columns = node
            .children()
            .filter(|c| c.tag_name().name() == "column")
            .filter_map(|c| c.attribute("name").map(str::to_owned))
            .collect();
        out.push(DeclaredTable { name, columns });
    }
    Ok(out)
}

fn liquibase_doc(docs: &[Doc]) -> Vec<DeclaredTable> {
    let mut out = Vec::new();
    for doc in docs {
        for entry in doc
            .get("databaseChangeLog")
            .map(Doc::items)
            .unwrap_or_default()
        {
            let Some(changes) = entry.get("changeSet").and_then(|c| c.get("changes")) else {
                continue;
            };
            for change in changes.items() {
                let Some(create) = change.get("createTable") else {
                    continue;
                };
                let Some(table) = create.get("tableName").and_then(Doc::as_str) else {
                    continue;
                };
                let name = match create.get("schemaName").and_then(Doc::as_str) {
                    Some(schema) => format!("{schema}.{table}"),
                    None => table.to_owned(),
                };
                let columns = create
                    .get("columns")
                    .map(Doc::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|c| c.get("column")?.get("name")?.as_str().map(str::to_owned))
                    .collect();
                out.push(DeclaredTable { name, columns });
            }
        }
    }
    out
}

/// Prisma's models, each its `@@map` name or its own, in its `@@schema` when given;
/// `None` for a model block left open.
pub fn prisma_models(text: &str) -> Option<Vec<DeclaredTable>> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix("model ") else {
            continue;
        };
        let model = rest.trim_end_matches('{').trim();
        if model.is_empty() || !model.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let (mut map, mut schema) = (None, None);
        loop {
            let body = lines.next()?;
            let body = body.split("//").next().unwrap_or("").trim();
            if body == "}" {
                break;
            }
            let argument = |attribute: &str| {
                body.strip_prefix(attribute)
                    .and_then(|r| r.trim().strip_prefix('('))
                    .and_then(|r| r.trim().strip_suffix(')'))
                    .and_then(string_literal)
            };
            if let Some(name) = argument("@@map") {
                map = Some(name);
            } else if let Some(name) = argument("@@schema") {
                schema = Some(name);
            }
        }
        let table = map.unwrap_or_else(|| model.to_owned());
        let name = match schema {
            Some(schema) => format!("\"{schema}\".\"{table}\""),
            None => format!("\"{table}\""),
        };
        out.push(DeclaredTable {
            name,
            columns: Vec::new(),
        });
    }
    Some(out)
}

// --- ORM entities -------------------------------------------------------------------

fn entities(context: &Context<'_>, out: &mut Vec<ContractObservation>) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        let framework = match language.id {
            "java" | "kotlin"
                if imports_module(registry, path, "jakarta.persistence")
                    || imports_module(registry, path, "javax.persistence") =>
            {
                "jpa"
            }
            "csharp"
                if imports_module(
                    registry,
                    path,
                    "System.ComponentModel.DataAnnotations.Schema",
                ) =>
            {
                "entity-framework"
            }
            _ => continue,
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            if definition.kind != DefinitionKind::Class {
                continue;
            }
            let interpolates = language.id == "kotlin";
            let Some(name) = entity_table(definition, framework == "jpa", interpolates) else {
                continue;
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            let owner = context.graph.definition_node(&r)?.clone();
            let table = DeclaredTable {
                name,
                columns: Vec::new(),
            };
            let raw = definition
                .decorators
                .iter()
                .find(|d| annotation::parse(d).is_some_and(|(n, _)| n == "Table"))
                .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            provided(&table, path, framework, &raw, Some(&owner), out);
        }
    }
    Ok(())
}

/// The table a class's annotations map it to: JPA's `@Table(name, schema)` on an
/// `@Entity`, or Entity Framework's `[Table("name", Schema = …)]`; literals only.
fn entity_table(definition: &Definition, jpa: bool, interpolates: bool) -> Option<String> {
    let parsed: Vec<(String, annotation::Args)> = definition
        .decorators
        .iter()
        .filter_map(|d| annotation::parse_in(d, interpolates))
        .collect();
    if jpa && !parsed.iter().any(|(n, _)| n == "Entity") {
        return None;
    }
    let (_, args) = parsed.iter().find(|(n, _)| n == "Table")?;
    let literal = |keys: &[&str], positional| match annotation::named(args, keys, positional) {
        Some(annotation::Arg::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let (name, schema) = if jpa {
        (literal(&["name"], false)?, literal(&["schema"], false))
    } else {
        (literal(&["Name"], true)?, literal(&["Schema"], false))
    };
    Some(match schema {
        Some(schema) => format!("\"{schema}\".\"{name}\""),
        None => format!("\"{name}\""),
    })
}

/// `SQLAlchemy`'s `__tablename__` and Django's `Meta.db_table`, read from the class's
/// own body in the source.
fn python_models(
    context: &Context<'_>,
    out: &mut Vec<ContractObservation>,
) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        if language.id != "python" {
            continue;
        }
        let sqlalchemy = imports_module(registry, path, "sqlalchemy");
        let django = imports_module(registry, path, "django");
        if !(sqlalchemy || django) {
            continue;
        }
        let Read::Text(text) = source::read(context.root, registry, path)? else {
            continue;
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            if definition.kind != DefinitionKind::Class {
                continue;
            }
            let Some(body) = definition
                .span
                .and_then(|s| text.get(s.start_byte as usize..s.end_byte as usize))
            else {
                continue;
            };
            let (attribute, framework, owner) = if sqlalchemy && definition.name != "Meta" {
                ("__tablename__", "sqlalchemy", index)
            } else if django && definition.name == "Meta" {
                let Some(model) = definition.qualified_name.strip_suffix(".Meta") else {
                    continue;
                };
                let models: Vec<_> = registry
                    .by_qualified_name(model)
                    .iter()
                    .filter(|r| r.path == path)
                    .collect();
                let [model] = models.as_slice() else {
                    continue;
                };
                ("db_table", "django", model.index as usize)
            } else {
                continue;
            };
            let Some(name) = class_attribute(body, attribute) else {
                continue;
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index: u32::try_from(owner).unwrap_or(u32::MAX),
            };
            let owner = context.graph.definition_node(&r)?.clone();
            let table = DeclaredTable {
                name: format!("\"{name}\""),
                columns: Vec::new(),
            };
            let raw = format!("{attribute} = {name:?}");
            provided(&table, path, framework, &raw, Some(&owner), out);
        }
    }
    Ok(())
}

/// A string literal assigned to `attribute` directly in a Python class body (at the
/// body's own indentation, never a nested block's).
fn class_attribute(class: &str, attribute: &str) -> Option<String> {
    let mut lines = class.lines().skip(1);
    let mut indent = None;
    let mut found = None;
    for line in lines.by_ref() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let depth = line.len() - trimmed.len();
        let body = *indent.get_or_insert(depth);
        if depth != body {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix(attribute) else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.split('#').next().unwrap_or("").trim();
        if found.is_some() {
            // Assigned twice: not one table.
            return None;
        }
        found = Some(string_literal(value)?);
    }
    found
}

// --- Consumers ----------------------------------------------------------------------

fn consumers(context: &Context<'_>, out: &mut Vec<ContractObservation>) -> Result<(), DeriveError> {
    let registry = context.registry;
    let ordinals = ordinals(registry);
    for path in registry.files() {
        let Some(extract) = registry.extract(path) else {
            continue;
        };
        for (index, call) in extract.calls.iter().enumerate() {
            if call.is_reference {
                continue;
            }
            for argument in &call.args {
                let Some(text) = argument
                    .value
                    .clone()
                    .or_else(|| string_literal(&argument.expr))
                else {
                    continue;
                };
                let access = sql_access(&text);
                if access.is_empty() {
                    continue;
                }
                let site_ref = SiteRef {
                    rel_path: path.to_owned(),
                    call_index: u32::try_from(index).unwrap_or(u32::MAX),
                };
                let site = match call_site(
                    context.graph,
                    registry,
                    &ordinals,
                    &site_ref,
                    call,
                    SiteKind::Call,
                )? {
                    Ok((site, _)) if context.graph.sites.contains_key(&site.site_id) => {
                        Some(site.site_id.as_str().to_owned())
                    }
                    _ => None,
                };
                let statement = text
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase();
                for (key, mode) in access {
                    let mut observation = ContractObservation::new(
                        ContractKind::Table,
                        key.clone(),
                        ContractDirection::Consumes,
                        path,
                        format!("{} {statement} {key}", call.callee_text),
                    )
                    .with_evidence("access_modes", mode.as_str());
                    if let Some(site) = &site {
                        observation = observation.with_evidence("site_ids", site.clone());
                    }
                    out.push(observation);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_table_with_columns_and_constraints() {
        let sql = "-- CREATE TABLE commented (x int);\nCREATE TABLE IF NOT EXISTS public.\"Users\" (\n  id bigint PRIMARY KEY,\n  \"Name\" text,\n  CONSTRAINT u UNIQUE (id)\n);\nCREATE TEMP TABLE scratch (x int);\nCREATE FUNCTION f() RETURNS void AS $$ CREATE TABLE hidden (x int); $$ LANGUAGE sql;\nCREATE TABLE sales.orders AS SELECT 1;\n";
        let tables = create_tables(sql).expect("parsed");
        assert_eq!(
            tables,
            [
                DeclaredTable {
                    name: "public.\"Users\"".into(),
                    columns: vec!["id".into(), "\"Name\"".into()],
                },
                DeclaredTable {
                    name: "sales.orders".into(),
                    columns: vec![],
                },
            ]
        );
        assert_eq!(create_tables("CREATE TABLE x (a int); /* open"), None);
    }

    #[test]
    fn sql_literals_read_and_write() {
        let access = sql_access(
            "SELECT * FROM users u, items i JOIN sales.Orders o ON o.u = u.id WHERE x IN (SELECT y FROM z)",
        );
        let found: Vec<(&str, Access)> = access.iter().map(|(k, a)| (k.as_str(), *a)).collect();
        assert_eq!(
            found,
            [
                ("items", Access::Read),
                ("sales.orders", Access::Read),
                ("users", Access::Read),
                ("z", Access::Read),
            ]
        );
        let found: Vec<(String, Access)> = sql_access("INSERT INTO audit (a) SELECT a FROM src")
            .into_iter()
            .collect();
        assert_eq!(
            found,
            [
                ("audit".to_owned(), Access::Write),
                ("src".to_owned(), Access::Read)
            ]
        );
        let found: Vec<(String, Access)> = sql_access("DELETE FROM old WHERE id = ?")
            .into_iter()
            .collect();
        assert_eq!(found, [("old".to_owned(), Access::Write)]);
        let found: Vec<(String, Access)> =
            sql_access("WITH recent AS (SELECT * FROM events) SELECT * FROM recent")
                .into_iter()
                .collect();
        assert_eq!(found, [("events".to_owned(), Access::Read)]);
        assert!(sql_access("not sql FROM x").is_empty());
    }

    #[test]
    fn python_class_attributes_at_body_level_only() {
        let class = "class User(Base):\n    __tablename__ = \"users\"  # the table\n    class Inner:\n        __tablename__ = \"nested\"\n";
        assert_eq!(
            class_attribute(class, "__tablename__"),
            Some("users".into())
        );
        assert_eq!(
            class_attribute("class A:\n    __tablename__ = NAME\n", "__tablename__"),
            None
        );
    }

    #[test]
    fn prisma_maps() {
        let models = prisma_models(
            "model User {\n  id Int @id\n  @@map(\"users\")\n}\nmodel Post {\n  id Int @id\n}\n",
        )
        .expect("parsed");
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["\"users\"", "\"Post\""]);
    }
}
