//! `Module` nodes (Appendix B.3; issue 48).
//!
//! A module node stands for a module of the registry ([`ModuleKey`]): its language, the
//! root its name is relative to, and its name. Its identity is
//! `(Module, path = scope, qualified_name = "<language>:<name>", disambiguator = "")`,
//! the language before the first `:` (a matrix id never contains one), so it depends on
//! nothing that varies with the files in it: not which file was seen first, not how
//! many there are.
//!
//! A module in one file is parented to that file, and its `file_id` is that file. A
//! module spanning files has no `file_id`, lists its member paths, sorted, in
//! `props.files`, and is parented to the nearest folder containing them all, or to the
//! repository. Symbols stay where they physically are: a symbol's semantic module is
//! `props.module`, never its parent, so a file joining a package moves nothing.
//!
//! A language whose module is the file itself (shell, SQL, configuration, Markdown)
//! has no module node: its file node is the module.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::ids::{NodeKey, RepoId};
use crate::kinds::NodeKind;
use crate::languages::{self, ModuleRule};
use crate::model::Node;
use crate::resolve::registry::ModuleKey;

use super::{Builder, DeriveError, DeriveInput};

/// A module's node key: `path` its scope, `qualified_name` `"<language>:<name>"`.
pub fn module_node_key(repo: &RepoId, module: &ModuleKey) -> NodeKey {
    NodeKey::definition(
        repo,
        NodeKind::Module,
        &module.scope,
        &format!("{}:{}", module.language, module.name),
        "",
    )
}

/// Whether a language's module is its file, which the file's node already is.
fn module_is_the_file(language: &str) -> bool {
    languages::by_id(language).is_some_and(|l| {
        matches!(
            l.module_rule,
            ModuleRule::File | ModuleRule::FileSectionsAsDocs | ModuleRule::FileKeysOnly
        )
    })
}

/// The nearest folder containing every file, as a path; `None` for the repository.
fn common_folder(files: &[&str]) -> Option<String> {
    let dirs: Vec<Vec<&str>> = files
        .iter()
        .map(|f| {
            let mut parts: Vec<&str> = f.split('/').collect();
            parts.pop();
            parts
        })
        .collect();
    let first = dirs.first()?;
    let mut common = first.len();
    for dir in &dirs[1..] {
        common = common.min(first.iter().zip(dir).take_while(|(a, b)| a == b).count());
    }
    (common > 0).then(|| first[..common].join("/"))
}

/// Adds every module's node, and `props.module` to the symbols in it.
pub(crate) fn build(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let registry = input.registry;
    for module in registry.modules() {
        if module_is_the_file(module.language) {
            continue;
        }
        let files: Vec<&str> = registry
            .files_in_module(module)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let key = module_node_key(input.repo, module);
        let mut node = Node::new(&key, &module.name)?;
        if let [only] = files.as_slice() {
            let file = graph.file_node(only)?.clone();
            node.file_id = Some(file.clone());
            node.parent_id = Some(file);
        } else {
            node.parent_id = Some(match common_folder(&files) {
                Some(folder) => {
                    graph
                        .folder_nodes
                        .get(&folder)
                        .cloned()
                        .ok_or(DeriveError::Missing {
                            path: folder,
                            detail: "a module's folder with no node",
                        })?
                }
                None => graph.repo_node.clone(),
            });
            node.props.insert(
                "files",
                Value::from(files.iter().map(|f| Value::from(*f)).collect::<Vec<_>>()),
            );
        }
        node.props.insert("language", Value::from(module.language));
        graph
            .module_nodes
            .insert(module.clone(), node.node_id.clone());
        graph.add_node(node)?;
    }

    // Each symbol's semantic module, by reference.
    let memberships: Vec<_> = graph
        .definition_nodes
        .iter()
        .filter(|(r, _)| !graph.file_definitions.contains(r))
        .filter_map(|(r, id)| {
            let module = registry.module_of_definition(r)?;
            Some((id.clone(), graph.module_nodes.get(module)?.clone()))
        })
        .collect();
    for (symbol, module) in memberships {
        graph.set_prop(&symbol, "module", Value::from(module.as_str()));
    }
    Ok(())
}
