//! Physical containment (Appendix B.3): `Repo → Folder* → File → definitions`, and
//! one file record per discovered file.
//!
//! Every discovered file gets a `File` node and a record, whatever became of it, so
//! coverage can account for the whole repository. Every extracted definition gets one
//! node, parented to the definition that encloses it, or else to its file; the engine's
//! file-level module definition is the file itself, never a second node. A test
//! callable is a `Test` node rather than a `Function` or `Method` (Appendix B.5), its
//! declared kind kept in `props.declared_kind`, so one definition is always one node.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::{Definition, DefinitionKind, FileStatus as EngineStatus, Visibility};
use serde_json::Value;

use crate::ids::{NodeKey, overload_disambiguators};
use crate::index::extract::FileOutcome;
use crate::kinds::NodeKind;
use crate::model::{FileRecord, FileStatus, Node};
use crate::resolve::registry::DefinitionRef;

use super::{Builder, DeriveError, DeriveInput, Diagnostics};

/// The node kind of a definition kind. A module definition other than the engine's
/// file-level one (a Rust inline `mod`, say) is a `Module` node of its own.
pub fn node_kind(kind: DefinitionKind) -> NodeKind {
    match kind {
        DefinitionKind::Class => NodeKind::Class,
        DefinitionKind::Interface => NodeKind::Interface,
        DefinitionKind::Enum => NodeKind::Enum,
        DefinitionKind::Struct => NodeKind::Struct,
        DefinitionKind::Trait => NodeKind::Trait,
        DefinitionKind::TypeAlias => NodeKind::TypeAlias,
        DefinitionKind::Function => NodeKind::Function,
        DefinitionKind::Method => NodeKind::Method,
        DefinitionKind::Constructor => NodeKind::Constructor,
        DefinitionKind::Field => NodeKind::Field,
        DefinitionKind::Variable => NodeKind::Variable,
        DefinitionKind::Macro => NodeKind::Macro,
        DefinitionKind::Module => NodeKind::Module,
    }
}

/// Whether a definition is the engine's file-level module: the file itself.
pub(crate) fn is_file_definition(definition: &Definition, path: &str) -> bool {
    definition.kind == DefinitionKind::Module && definition.name == path
}

/// A parameter type as 4.2.1 normalises it for an overload's signature: whitespace
/// removed, generic arguments erased to the raw type (`List<String>` is `List`, and in
/// Python `list[int]` is `list`). No default alias is expanded: none is defined, so a
/// type stays as declared. An unknown type (`?`) stays `?`.
pub fn normalise_type(language: &str, declared: &str) -> String {
    let compact: String = declared.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = String::with_capacity(compact.len());
    let mut depth = 0u32;
    let mut python_depth = 0u32;
    for c in compact.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            '[' if language == "python" && depth == 0 && out.ends_with(is_name_char) => {
                python_depth += 1;
            }
            '[' if python_depth > 0 => python_depth += 1,
            ']' if python_depth > 0 => python_depth -= 1,
            _ if depth == 0 && python_depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A definition's normalised signature: its parameter types, normalised, joined by
/// `,` (4.2.1's reference vector `String,int`).
pub fn normalised_signature(language: &str, definition: &Definition) -> String {
    definition
        .signature_param_types
        .iter()
        .map(|t| normalise_type(language, t))
        .collect::<Vec<_>>()
        .join(",")
}

/// A file's status and its reason, from what Stage 2 did with it. A file the engine
/// failed or crashed on is `failed`, never `parsed`; a withheld one is `redacted`,
/// never `skipped`.
pub fn file_status(outcome: &FileOutcome) -> (FileStatus, Option<&'static str>) {
    match outcome {
        FileOutcome::Extracted { extract, .. } => match extract.status {
            EngineStatus::Parsed => (FileStatus::Parsed, None),
            EngineStatus::Partial => (FileStatus::Partial, None),
            EngineStatus::Failed => (FileStatus::Failed, Some("parse")),
        },
        FileOutcome::EngineFailed { .. } => (FileStatus::Failed, Some("engine_error")),
        FileOutcome::EngineCrashed { .. } => (FileStatus::Failed, Some("engine_crash")),
        FileOutcome::SkippedMemory => (FileStatus::Skipped, Some("memory")),
        FileOutcome::SkippedSize => (FileStatus::Skipped, Some("size")),
        FileOutcome::UnknownLanguage => (FileStatus::Skipped, Some("unknown_language")),
        FileOutcome::Binary => (FileStatus::Binary, None),
        FileOutcome::Redacted => (FileStatus::Redacted, None),
    }
}

/// Whether a path is repository-relative and POSIX (4.2.1), as every identity needs:
/// names separated by `/`, none of them empty, `.` or `..`, no `\` and no drive
/// letter. A host-native path (`src\a.py`, `C:/src/a.py`, `/home/x/a.py`) is not one.
pub fn is_repository_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    !drive
        && !path.contains('\\')
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

fn last_component(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(dir, _)| dir)
}

/// Builds the containment tree, the file records and the definition nodes.
pub(crate) fn build(
    input: &DeriveInput<'_>,
    tests: &BTreeSet<DefinitionRef>,
) -> Result<Builder, DeriveError> {
    let repo = input.repo;
    let registry = input.registry;
    let repo_node = Node::new(&NodeKey::repo(repo), input.repo_name)?;
    let mut graph = Builder {
        files: BTreeMap::new(),
        nodes: BTreeMap::new(),
        sites: BTreeMap::new(),
        edges: BTreeMap::new(),
        candidates: BTreeMap::new(),
        repo_node: repo_node.node_id.clone(),
        file_nodes: BTreeMap::new(),
        folder_nodes: BTreeMap::new(),
        definition_nodes: BTreeMap::new(),
        file_definitions: BTreeSet::new(),
        module_nodes: BTreeMap::new(),
        diagnostics: Diagnostics::default(),
    };
    graph.add_node(repo_node)?;

    folders_and_files(&mut graph, input)?;
    for path in registry.files() {
        definitions(&mut graph, input, path, tests)?;
    }
    Ok(graph)
}

/// The folders every discovered path implies, parents first, and each file's node and
/// record.
fn folders_and_files(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let repo = input.repo;
    let registry = input.registry;
    // Folders: every directory a discovered path implies, parents first.
    let mut folders = BTreeSet::new();
    for path in registry.files() {
        if !is_repository_path(path) {
            return Err(DeriveError::NotRepositoryPath(path.to_owned()));
        }
        let mut dir = parent_dir(path);
        while let Some(d) = dir {
            folders.insert(d.to_owned());
            dir = parent_dir(d);
        }
    }
    for folder in &folders {
        let mut node = Node::new(&NodeKey::folder(repo, folder), last_component(folder))?;
        node.parent_id = Some(match parent_dir(folder) {
            Some(p) => graph.folder_nodes[p].clone(),
            None => graph.repo_node.clone(),
        });
        graph
            .folder_nodes
            .insert(folder.clone(), node.node_id.clone());
        graph.add_node(node)?;
    }

    // Files and their records.
    for path in registry.files() {
        let discovered = registry.discovered(path).ok_or(DeriveError::Missing {
            path: path.to_owned(),
            detail: "a file the registry did not discover",
        })?;
        let outcome = registry.outcome(path).ok_or(DeriveError::Missing {
            path: path.to_owned(),
            detail: "a file with no outcome",
        })?;
        let mut node = Node::new(&NodeKey::file(repo, path), last_component(path))?;
        node.file_id = Some(node.node_id.clone());
        node.parent_id = Some(match parent_dir(path) {
            Some(dir) => graph.folder_nodes[dir].clone(),
            None => graph.repo_node.clone(),
        });
        node.props
            .insert("language", Value::from(discovered.language_id()));
        let (status, reason) = file_status(outcome);
        graph.files.insert(
            path.to_owned(),
            FileRecord {
                file_id: node.node_id.clone(),
                path: path.to_owned(),
                language: discovered.language_id().to_owned(),
                status,
                status_reason: reason.map(str::to_owned),
                blob_sha: outcome.blob_sha().map(|b| b.to_string()),
                size_bytes: discovered.size_bytes,
                line_count: registry.line_count(path),
            },
        );
        graph
            .file_nodes
            .insert(path.to_owned(), node.node_id.clone());
        graph.add_node(node)?;
    }

    Ok(())
}

/// Each definition's node kind (none for the engine's file-level module, which is the
/// file) and disambiguator: definitions that share a kind and qualified name in one
/// file are told apart by their normalised signatures, in file order (4.2.1). Two
/// definitions cannot share an id, so a non-callable repeated under one name is
/// numbered the same way.
fn identities(
    extract: &pdx_engine::FileExtract,
    path: &str,
    language: &str,
    tests: &BTreeSet<DefinitionRef>,
) -> Result<(Vec<Option<NodeKind>>, Vec<String>), DeriveError> {
    let reference = |index: usize| DefinitionRef {
        path: path.to_owned(),
        index: u32::try_from(index).unwrap_or(u32::MAX),
    };
    let kinds: Vec<Option<NodeKind>> = extract
        .definitions
        .iter()
        .enumerate()
        .map(|(i, d)| {
            if is_file_definition(d, path) {
                None
            } else if tests.contains(&reference(i)) {
                Some(NodeKind::Test)
            } else {
                Some(node_kind(d.kind))
            }
        })
        .collect();
    let mut groups: BTreeMap<(&'static str, &str), Vec<usize>> = BTreeMap::new();
    for (i, d) in extract.definitions.iter().enumerate() {
        if let Some(kind) = kinds[i] {
            groups
                .entry((kind.as_str(), d.qualified_name.as_str()))
                .or_default()
                .push(i);
        }
    }
    let mut disambiguators = vec![String::new(); extract.definitions.len()];
    for members in groups.values() {
        let signatures: Vec<String> = members
            .iter()
            .map(|&i| normalised_signature(language, &extract.definitions[i]))
            .collect();
        let refs: Vec<&str> = signatures.iter().map(String::as_str).collect();
        for (&i, d) in members.iter().zip(overload_disambiguators(&refs)?) {
            disambiguators[i] = d;
        }
    }

    Ok((kinds, disambiguators))
}

/// The nodes of one file's definitions.
fn definitions(
    graph: &mut Builder,
    input: &DeriveInput<'_>,
    path: &str,
    tests: &BTreeSet<DefinitionRef>,
) -> Result<(), DeriveError> {
    let registry = input.registry;
    let Some(extract) = registry.extract(path) else {
        return Ok(());
    };
    let language = registry.language(path).map_or("unknown", |l| l.id);
    let file_node = graph.file_node(path)?.clone();
    let reference = |index: usize| DefinitionRef {
        path: path.to_owned(),
        index: u32::try_from(index).unwrap_or(u32::MAX),
    };

    let (kinds, disambiguators) = identities(extract, path, language, tests)?;

    // Ids first, so a parent is known whatever order definitions come in.
    let mut ids = Vec::with_capacity(extract.definitions.len());
    for (i, d) in extract.definitions.iter().enumerate() {
        let r = reference(i);
        let id = match kinds[i] {
            None => {
                graph.file_definitions.insert(r.clone());
                file_node.clone()
            }
            Some(kind) => NodeKey::definition(
                input.repo,
                kind,
                path,
                &d.qualified_name,
                &disambiguators[i],
            )
            .node_id()?,
        };
        graph.definition_nodes.insert(r, id.clone());
        ids.push(id);
    }

    for (i, d) in extract.definitions.iter().enumerate() {
        let Some(kind) = kinds[i] else {
            continue;
        };
        let key = NodeKey::definition(
            input.repo,
            kind,
            path,
            &d.qualified_name,
            &disambiguators[i],
        );
        let mut node = Node::new(&key, &d.name)?;
        node.file_id = Some(file_node.clone());
        node.parent_id = Some(match d.parent.and_then(|p| ids.get(p as usize)) {
            Some(parent) => parent.clone(),
            None => file_node.clone(),
        });
        node.start_line = d.span.map(|s| s.start_line);
        node.end_line = d.span.map(|s| s.end_line);
        node.signature.clone_from(&d.signature);
        node.doc.clone_from(&d.doc);
        node.props
            .insert("engine_kind", Value::from(d.engine_kind.as_str()));
        match d.visibility {
            Visibility::Public => node.props.insert("visibility", Value::from("public")),
            Visibility::NonPublic => node.props.insert("visibility", Value::from("non_public")),
            Visibility::Unknown => {}
        }
        let is_test = kind == NodeKind::Test;
        node.props.insert("is_test", Value::from(is_test));
        // A test is an entry point; every other category is the entry stage's to prove
        // (Appendix B.2). The engine's own flag, which it sets on far more (every
        // exported JavaScript or TypeScript declaration), is kept as the engine's
        // evidence only and never makes an entry point (issue 55).
        node.props.insert("is_entry_point", Value::from(is_test));
        node.props
            .insert("engine_entry_point", Value::from(d.is_entry_point));
        if is_test {
            node.props
                .insert("declared_kind", Value::from(node_kind(d.kind).as_str()));
        }
        graph.add_node(node)?;
    }
    Ok(())
}
