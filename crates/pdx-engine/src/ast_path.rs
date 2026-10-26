//! A site's node-type path (4.2.1, issue 46), held compactly (P2-17, issue 82).
//!
//! Every call site carries the syntax node types from its enclosing definition down to
//! it, which its identity hashes. Held as strings, they were the largest part of an
//! extraction after its surface: on Kubernetes's packages 6.9 million strings, 261 MB
//! for 45 MB of source. The names are a grammar's node types, a few thousand across
//! every language, so each is interned once per process; and the sites of one file
//! share their paths, so each distinct path of an extraction is held once
//! ([`ExtractRecord`](crate::ExtractRecord) shares them when it is bound).
//!
//! On the wire a path is the sequence of its names, exactly as a `Vec<String>` was, so
//! the extraction cache's format does not change.

use std::collections::HashSet;
use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, OnceLock, RwLock};

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The most distinct names the process interns. Grammars have a few thousand node types
/// between them; a source of names past this (a damaged cache entry) is refused rather
/// than grown without bound.
pub const MAX_INTERNED_NODE_TYPES: usize = 1 << 16;

/// The node types from a site's enclosing definition down to the site, in order; empty
/// when the engine could not record them.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstPath(Arc<[&'static str]>);

impl Default for AstPath {
    fn default() -> Self {
        Self(Arc::from(Vec::new()))
    }
}

impl AstPath {
    /// The path of these names, each interned.
    ///
    /// # Panics
    ///
    /// Past [`MAX_INTERNED_NODE_TYPES`] distinct names in the process, which no grammar
    /// reaches; [`AstPath::try_from_names`] returns the refusal instead.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self::try_from_names(names).expect("node types within the interning bound")
    }

    /// The path of these names, each interned, or `None` past the interning bound.
    pub fn try_from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let names: Option<Vec<&'static str>> = names.into_iter().map(intern).collect();
        Some(Self(Arc::from(names?)))
    }

    /// The names, in order.
    pub fn as_slice(&self) -> &[&'static str] {
        &self.0
    }

    /// Whether two paths share one allocation: what [`ExtractRecord`](crate::ExtractRecord)
    /// arranges for equal paths of one extraction.
    pub fn shares(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for AstPath {
    type Target = [&'static str];

    fn deref(&self) -> &[&'static str] {
        &self.0
    }
}

impl From<Vec<String>> for AstPath {
    fn from(names: Vec<String>) -> Self {
        Self::from_names(names.iter().map(String::as_str))
    }
}

impl From<&[&str]> for AstPath {
    fn from(names: &[&str]) -> Self {
        Self::from_names(names.iter().copied())
    }
}

impl<const N: usize> From<[&str; N]> for AstPath {
    fn from(names: [&str; N]) -> Self {
        Self::from_names(names)
    }
}

impl PartialEq<Vec<String>> for AstPath {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.0.len() == other.len() && self.0.iter().zip(other).all(|(a, b)| a == b)
    }
}

impl<const N: usize> PartialEq<[&str; N]> for AstPath {
    fn eq(&self, other: &[&str; N]) -> bool {
        self.0.len() == N && self.0.iter().zip(other).all(|(a, b)| a == b)
    }
}

/// As a list of names, as `Vec<String>` prints.
impl fmt::Debug for AstPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

impl Serialize for AstPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

impl<'de> Deserialize<'de> for AstPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Names;

        impl<'de> Visitor<'de> for Names {
            type Value = AstPath;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a sequence of node type names")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<AstPath, A::Error> {
                let mut names = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
                while let Some(name) = seq.next_element::<std::borrow::Cow<'de, str>>()? {
                    names.push(intern(&name).ok_or_else(|| {
                        serde::de::Error::custom("more distinct node types than any grammar has")
                    })?);
                }
                Ok(AstPath(Arc::from(names)))
            }
        }

        deserializer.deserialize_seq(Names)
    }
}

/// `name`, interned for the life of the process; `None` once
/// [`MAX_INTERNED_NODE_TYPES`] distinct names are.
fn intern(name: &str) -> Option<&'static str> {
    static NAMES: OnceLock<RwLock<HashSet<&'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(RwLock::default);
    if let Some(&known) = names
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
    {
        return Some(known);
    }
    let mut names = names
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&known) = names.get(name) {
        return Some(known);
    }
    if names.len() >= MAX_INTERNED_NODE_TYPES {
        return None;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    Some(leaked)
}

/// Gives equal paths one allocation, as an extraction's sites are bound.
#[derive(Default)]
pub(crate) struct PathSharing(HashSet<AstPath>);

impl PathSharing {
    /// `path`, or the equal path seen first.
    pub(crate) fn share(&mut self, path: &mut AstPath) {
        if path.is_empty() {
            return;
        }
        match self.0.get(path) {
            Some(first) => *path = first.clone(),
            None => {
                self.0.insert(path.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_its_names_on_the_wire() {
        let path = AstPath::from(["function_definition", "block", "call"]);
        let names = vec![
            "function_definition".to_owned(),
            "block".to_owned(),
            "call".to_owned(),
        ];
        let encoded = postcard::to_stdvec(&path).expect("encodes");
        assert_eq!(encoded, postcard::to_stdvec(&names).expect("encodes"));
        let decoded: AstPath = postcard::from_bytes(&encoded).expect("decodes");
        assert_eq!(decoded, path);
        assert_eq!(decoded, names);
        let back: Vec<String> = postcard::from_bytes(&encoded).expect("decodes");
        assert_eq!(back, names);
    }

    #[test]
    fn names_are_interned_and_paths_ordered_as_strings() {
        let a = AstPath::from(["block", "call"]);
        let b = AstPath::from(["block", "call"]);
        assert!(std::ptr::eq(a[0], b[0]));
        assert!(!a.shares(&b));
        let (ab, ac) = (AstPath::from(["a", "b"]), AstPath::from(["a", "c"]));
        let (a, aa) = (AstPath::from(["a"]), AstPath::from(["a", "a"]));
        assert!(ab < ac);
        assert!(a < aa);
        assert_eq!(
            format!("{a:?}"),
            format!("{:?}", vec!["block".to_owned(), "call".to_owned()])
        );
        assert!(AstPath::default().is_empty());
    }

    #[test]
    fn equal_paths_share_one_allocation() {
        let mut sharing = PathSharing::default();
        let mut first = AstPath::from(["x", "y"]);
        let mut second = AstPath::from(["x", "y"]);
        let mut other = AstPath::from(["x"]);
        sharing.share(&mut first);
        sharing.share(&mut second);
        sharing.share(&mut other);
        assert!(first.shares(&second));
        assert!(!first.shares(&other));
    }
}
