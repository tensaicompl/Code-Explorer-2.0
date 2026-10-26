//! A file's path below the repository root, shared (P2-17, issue 82).
//!
//! Resolution refers to files constantly: every site, every definition a site may
//! target, every candidate (up to 32 a site), each in the registry's indexes several
//! times. Each reference held its own copy of the path: on Kubernetes about two
//! gigabytes of one file's name written again and again. A `RelPath` is one shared,
//! immutable string: cloning it copies a pointer. It compares, orders, hashes and
//! serialises exactly as the string it holds, so nothing that sorts, keys or writes
//! paths changes.

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A file's path below the repository root, shared by every reference to the file.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelPath(Arc<str>);

impl RelPath {
    /// The path, as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether two paths share one allocation.
    pub fn shares(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for RelPath {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for RelPath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RelPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for RelPath {
    fn from(path: &str) -> Self {
        Self(Arc::from(path))
    }
}

impl From<String> for RelPath {
    fn from(path: String) -> Self {
        Self(Arc::from(path))
    }
}

impl From<&String> for RelPath {
    fn from(path: &String) -> Self {
        Self(Arc::from(path.as_str()))
    }
}

impl From<RelPath> for String {
    fn from(path: RelPath) -> String {
        path.0.to_string()
    }
}

impl PartialEq<str> for RelPath {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for RelPath {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

impl PartialEq<String> for RelPath {
    fn eq(&self, other: &String) -> bool {
        *self.0 == **other
    }
}

impl PartialEq<RelPath> for str {
    fn eq(&self, other: &RelPath) -> bool {
        self == &*other.0
    }
}

impl PartialEq<RelPath> for &str {
    fn eq(&self, other: &RelPath) -> bool {
        *self == &*other.0
    }
}

impl PartialEq<RelPath> for String {
    fn eq(&self, other: &RelPath) -> bool {
        **self == *other.0
    }
}

/// As the string prints.
impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for RelPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_its_string_in_every_comparison() {
        let a = RelPath::from("src/a.rs");
        let shared = a.clone();
        assert!(a.shares(&shared));
        assert_eq!(a, "src/a.rs");
        assert_eq!(a, "src/a.rs".to_owned());
        let (first, second) = (RelPath::from("src/a.rs"), RelPath::from("src/b.rs"));
        assert!(first < second);
        assert_eq!(format!("{a:?}"), format!("{:?}", "src/a.rs"));
        assert_eq!(a.to_string(), "src/a.rs");
        let mut set = std::collections::BTreeSet::new();
        set.insert(a.clone());
        assert!(set.contains("src/a.rs"));
        let encoded = postcard::to_stdvec(&a).expect("encodes");
        assert_eq!(encoded, postcard::to_stdvec("src/a.rs").expect("encodes"));
        let back: RelPath = postcard::from_bytes(&encoded).expect("decodes");
        assert_eq!(back, a);
    }
}
