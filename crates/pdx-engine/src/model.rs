//! What extraction finds in one file, owned by Rust.
//!
//! Every structure of the engine's extraction result has a counterpart here, field for
//! field, with nothing borrowed: a [`FileExtract`] outlives the engine that produced
//! it, crosses processes, and is what the extraction cache keeps. Order is exactly the
//! engine's; nothing here is ever reordered or deduplicated.
//!
//! The interface's encodings become types: an unknown position is `None` rather than
//! a span of zeros, "no enclosing definition" is `None` rather than a sentinel index,
//! and the status, visibility and kind codes are enums.

use serde::{Deserialize, Serialize};

/// A position in a file: byte offsets into its UTF-8 source, and the 1-based lines and
/// columns they fall on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// Offset of the first byte.
    pub start_byte: u32,
    /// Offset one past the last byte.
    pub end_byte: u32,
    /// Line of the first byte.
    pub start_line: u32,
    /// Column of the first byte.
    pub start_col: u32,
    /// Line of the last byte.
    pub end_line: u32,
    /// Column of the last byte.
    pub end_col: u32,
}

/// How far parsing got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileStatus {
    /// Parsed without errors.
    Parsed,
    /// Parsed, but the tree carries errors; what was found is reported.
    Partial,
    /// Not parsed.
    Failed,
}

/// The normalised kind of a definition. The engine's own kind is kept beside it, in
/// [`Definition::engine_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DefinitionKind {
    /// `class`.
    Class,
    /// `interface`.
    Interface,
    /// `enum`.
    Enum,
    /// `struct`.
    Struct,
    /// `trait`.
    Trait,
    /// `type_alias`.
    TypeAlias,
    /// `function`.
    Function,
    /// `method`.
    Method,
    /// `constructor`.
    Constructor,
    /// `field`.
    Field,
    /// `variable`, and every engine kind with no mapping of its own.
    Variable,
    /// `macro`.
    Macro,
    /// `module`.
    Module,
}

impl DefinitionKind {
    /// The interface's name for the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Struct => "struct",
            Self::Trait => "trait",
            Self::TypeAlias => "type_alias",
            Self::Function => "function",
            Self::Method => "method",
            Self::Constructor => "constructor",
            Self::Field => "field",
            Self::Variable => "variable",
            Self::Macro => "macro",
            Self::Module => "module",
        }
    }

    /// The kind the interface names `name`, if it names one.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "class" => Self::Class,
            "interface" => Self::Interface,
            "enum" => Self::Enum,
            "struct" => Self::Struct,
            "trait" => Self::Trait,
            "type_alias" => Self::TypeAlias,
            "function" => Self::Function,
            "method" => Self::Method,
            "constructor" => Self::Constructor,
            "field" => Self::Field,
            "variable" => Self::Variable,
            "macro" => Self::Macro,
            "module" => Self::Module,
            _ => return None,
        })
    }
}

/// Visibility, as the source declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    /// The source does not say.
    Unknown,
    /// Exported or public.
    Public,
    /// Anything narrower than public.
    NonPublic,
}

/// What the extractor saw of a name in its surroundings, for resolving it by name when
/// typed resolution has no answer. A set of independent facts; the bits are the
/// interface's, and bits this version does not name are kept, not dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct LexicalFacts(u16);

impl LexicalFacts {
    /// The source spells a callable reference as such, rather than passing a plain name
    /// a callable might be behind.
    pub const EXPLICIT_REFERENCE: Self = Self(1);
    /// The name is the member half of a selector; its receiver has been removed.
    pub const MEMBER_ACCESS: Self = Self(2);
    /// A binding in the same code means the name is not the module-level definition of
    /// that name; resolving it by name must not reach a callable.
    pub const BLOCKED: Self = Self(4);
    /// That binding is in a local scope; resolving it by name must not reach anything.
    pub const BLOCKED_LOCALLY: Self = Self(8);
    /// A member call whose receiver the extractor could not tie to anything.
    pub const UNRESOLVED_MEMBER: Self = Self(16);
    /// A bare call whose callee name is a parameter of an enclosing function.
    pub const LOCALLY_BOUND: Self = Self(32);
    /// A member call whose receiver is an attribute chain rooted at `self` or `cls`.
    pub const SELF_ROOTED: Self = Self(64);

    /// The facts the interface's bits encode.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// The interface's bits.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether every fact of `other` holds.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no fact holds.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Anything the graph gives a node of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    /// The name as declared.
    pub name: String,
    /// The engine's qualified name.
    pub qualified_name: String,
    /// The normalised kind.
    pub kind: DefinitionKind,
    /// The engine's own kind, which the normalised kind may have generalised.
    pub engine_kind: String,
    /// The signature, where the language has one.
    pub signature: Option<String>,
    /// The documentation comment, if any.
    pub doc: Option<String>,
    /// The whole definition.
    pub span: Option<Span>,
    /// Its body.
    pub body_span: Option<Span>,
    /// The definition this one is inside, as an index into the same file's
    /// definitions; `None` at file scope.
    pub parent: Option<u32>,
    /// Declared visibility.
    pub visibility: Visibility,
    /// A test.
    pub is_test: bool,
    /// An entry point of the program.
    pub is_entry_point: bool,
    /// Cyclomatic complexity.
    pub cyclomatic: u32,
    /// Cognitive complexity.
    pub cognitive: u32,
    /// Deepest loop nesting.
    pub loop_depth: u32,
}

/// A call site, or a callable passed as a value.
///
/// Calls are in the order the engine found them, then references in the order their
/// names appear; a typed resolution refers to a call by its index in that order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// The callee as the source spells it.
    pub callee_text: String,
    /// The receiver, where the engine splits it off.
    pub receiver_text: Option<String>,
    /// The definition making the call, as an index into the file's definitions;
    /// `None` at file scope.
    pub caller: Option<u32>,
    /// Where the site is.
    pub span: Option<Span>,
    /// A callable passed as a value rather than invoked. A call reference only if typed
    /// resolution says so; otherwise an ordinary use of a name, never a call.
    pub is_reference: bool,
    /// A site that exists only as a question for typed resolution. A call only if a
    /// typed resolution names its target; never resolved by name.
    pub typed_only: bool,
    /// What the extractor saw around it.
    pub lexical: LexicalFacts,
}

/// An import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// The module as the source names it.
    pub module_text: String,
    /// The name it binds locally.
    pub imported_name: Option<String>,
    /// An alias, where the engine records one separately.
    pub alias: Option<String>,
    /// Where the import is.
    pub span: Option<Span>,
}

/// A use of a name as a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// The name.
    pub name: String,
    /// The definition it is used in; `None` at file scope.
    pub scope: Option<u32>,
    /// Where the use is.
    pub span: Option<Span>,
    /// What the extractor saw around it.
    pub lexical: LexicalFacts,
}

/// A reference to a type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRef {
    /// The type as the source spells it.
    pub type_text: String,
    /// The definition it appears in; `None` at file scope.
    pub scope: Option<u32>,
    /// Always `None` today: the engine records no position for these.
    pub span: Option<Span>,
}

/// An exception a definition raises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Throw {
    /// The exception's type as the source spells it.
    pub exception_text: String,
    /// The definition that raises it; `None` at file scope.
    pub scope: Option<u32>,
    /// Always `None` today: the engine records no position for these.
    pub span: Option<Span>,
}

/// A read or write of a field or variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWrite {
    /// The field or variable.
    pub field_text: String,
    /// The definition it happens in; `None` at file scope.
    pub scope: Option<u32>,
    /// A write rather than a read.
    pub is_write: bool,
    /// Always `None` today: the engine records no position for these.
    pub span: Option<Span>,
}

/// Which way a file takes part in a publish/subscribe channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelDirection {
    /// It publishes.
    Emit,
    /// It subscribes.
    Listen,
}

/// Publish or subscribe participation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    /// The channel as the source names it.
    pub channel_text: String,
    /// Publish or subscribe.
    pub direction: ChannelDirection,
    /// Always `None` today: the engine records no position for these.
    pub span: Option<Span>,
}

/// An environment or configuration key the file reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvAccess {
    /// The key.
    pub key: String,
    /// Always `None` today: the engine records no position for these.
    pub span: Option<Span>,
}

/// Something the engine reports about the file as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// What it says.
    pub message: String,
    /// Where, if it says.
    pub span: Option<Span>,
}

/// A file's resolution surface: everything typed resolution reads from the file's
/// extraction, in the engine's own encoding.
///
/// Opaque, and not a substitute for the extraction's fields: it is what lets a file
/// whose content has not changed be resolved again without being extracted again.
/// Equal extractions have equal surfaces.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface(#[serde(with = "serde_bytes_compat")] Vec<u8>);

impl Surface {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The engine's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Surface({} bytes)", self.0.len())
    }
}

/// Bytes as one length-prefixed run rather than a sequence of numbers, which is what
/// serde makes of a `Vec<u8>` by default.
pub(crate) mod serde_bytes_compat {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl serde::de::Visitor<'_> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }
        d.deserialize_byte_buf(Bytes)
    }
}

/// The SHA-256 digest of the exact bytes handed to the engine to extract.
///
/// It identifies the source an extraction was taken from, so the extraction is only
/// ever resolved with that source. It is a digest of whatever the caller extracted,
/// after any normalisation the caller applied, and nothing else: it is not a version
/// control identity of the file, which is computed differently and over different
/// bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceDigest([u8; 32]);

impl SourceDigest {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::Digest as _;
        Self(sha2::Sha256::digest(bytes).into())
    }

    /// The digest's bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for SourceDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl std::fmt::Debug for SourceDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SourceDigest({self})")
    }
}

/// Everything extraction found in one file, and its resolution surface.
///
/// Self-contained: it names the file and language it was taken for and identifies the
/// source by length and digest, so it can be cached, sent between processes, and later
/// resolved with that same source, and no other, without the engine that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExtract {
    /// The language it was extracted as, by the language matrix's identifier.
    pub language: String,
    /// The path it was extracted under, relative to the repository. Qualified names
    /// depend on it.
    pub rel_path: String,
    /// The length of the source in bytes.
    pub source_len: u64,
    /// The digest of the source: of exactly the bytes extracted.
    pub source_digest: SourceDigest,
    /// How far parsing got.
    pub status: FileStatus,
    /// The extractor stopped walking the file at its node budget: what it found up to
    /// then is here, the rest of the file is not, and typed resolution skips it.
    pub truncated: bool,
    /// Work extraction lost on the file: allocations that failed and work budgets that
    /// ran out while it was extracted. 0 when the engine reported no loss; otherwise
    /// the extraction is degraded, and is the same count its surface carries into any
    /// project that resolves it.
    pub extraction_lost: u32,
    /// Definitions.
    pub definitions: Vec<Definition>,
    /// Calls, then callables passed as values.
    pub calls: Vec<Call>,
    /// Imports.
    pub imports: Vec<Import>,
    /// Uses of names that are not calls.
    pub usages: Vec<Usage>,
    /// Type references.
    pub type_refs: Vec<TypeRef>,
    /// Exceptions raised.
    pub throws: Vec<Throw>,
    /// Field and variable reads and writes.
    pub read_writes: Vec<ReadWrite>,
    /// Publish/subscribe participation.
    pub channels: Vec<Channel>,
    /// Environment and configuration keys read.
    pub env_accesses: Vec<EnvAccess>,
    /// What the engine reported about the file.
    pub diagnostics: Vec<Diagnostic>,
    /// What typed resolution reads from the file.
    pub surface: Surface,
}
