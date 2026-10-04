//! The generic-name blocklist (Appendix B.4).
//!
//! A call whose callee is one of these names is `blocked` before any resolution stage
//! runs, typed resolution included (specification 4.2.2, 4.5 Stage 3): the names are
//! so common that a target found for them by any means is more likely a coincidence
//! of spelling than the definition meant. Names compare exactly, case included:
//! `toString` is blocked, `ToString` is not.
//!
//! The base list is Appendix B.4's, unchanged. A language may add names of its own,
//! each documented here with the reason it is generic in that language; nothing else
//! extends the list.

/// Appendix B.4's base list, in its order. Applies to every language.
pub const BASE: [&str; 57] = [
    "get",
    "set",
    "close",
    "open",
    "run",
    "main",
    "start",
    "stop",
    "to_dict",
    "from_dict",
    "to_json",
    "from_json",
    "serialize",
    "deserialize",
    "handle",
    "execute",
    "process",
    "validate",
    "parse",
    "format",
    "update",
    "create",
    "delete",
    "remove",
    "add",
    "append",
    "clear",
    "read",
    "write",
    "load",
    "save",
    "send",
    "receive",
    "setup",
    "teardown",
    "init",
    "reset",
    "next",
    "value",
    "name",
    "toString",
    "equals",
    "hashCode",
    "length",
    "size",
    "isEmpty",
    "push",
    "pop",
    "apply",
    "call",
    "bind",
    "then",
    "catch",
    "map",
    "filter",
    "reduce",
    "forEach",
];

/// A name blocked in one language only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addition {
    /// The language, by the matrix's identifier.
    pub language: &'static str,
    /// The name.
    pub name: &'static str,
}

/// Python's `__init__`: every class may define one, and it is called through
/// `super().__init__()` and by every subclass's own construction, so a call by this
/// name names a constructor of some class, never one a name lookup can choose.
const PYTHON_INIT: Addition = Addition {
    language: "python",
    name: "__init__",
};

/// Python's `__str__`: the text conversion any class may define, called on values of
/// every type; the method of the same name in the repository is no evidence of which
/// class a value has.
const PYTHON_STR: Addition = Addition {
    language: "python",
    name: "__str__",
};

/// Python's `__repr__`: the representation any class may define, as generic as
/// `__str__` for the same reason.
const PYTHON_REPR: Addition = Addition {
    language: "python",
    name: "__repr__",
};

/// Python's `__enter__`: the context-manager entry every `with` target defines; a call
/// by this name is a protocol call on a value of any type.
const PYTHON_ENTER: Addition = Addition {
    language: "python",
    name: "__enter__",
};

/// Python's `__exit__`: the context-manager exit, generic as `__enter__` is.
const PYTHON_EXIT: Addition = Addition {
    language: "python",
    name: "__exit__",
};

/// Every language-specific addition.
pub const ADDITIONS: [Addition; 5] = [
    PYTHON_INIT,
    PYTHON_STR,
    PYTHON_REPR,
    PYTHON_ENTER,
    PYTHON_EXIT,
];

/// Whether a callee named `name` in a file of `language` (the matrix's identifier) is
/// on the blocklist: the base list, or an addition for that language.
pub fn is_blocked(language: &str, name: &str) -> bool {
    BASE.contains(&name)
        || ADDITIONS
            .iter()
            .any(|a| a.language == language && a.name == name)
}
