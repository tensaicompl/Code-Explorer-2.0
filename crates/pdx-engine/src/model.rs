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
//!
//! Text a fact takes from the file is a [`TextRef`], a range of the source rather than
//! a copy, and a definition's qualified name a [`QualifiedName`], given by its relation
//! to an earlier definition's (issue 90, decision 61): evidence that nests would
//! otherwise repeat every byte it overlaps. What is stored and sent is an
//! [`ExtractRecord`], which holds no source; a [`FileExtract`] is a record bound to the
//! exact source it was taken from, checked when it is bound, and is what every
//! consumer reads text through.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::Arc;

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

/// The engine's exportability or visibility fact for a definition. It is declaration
/// visibility only where the engine applies the language's own rule (Go's exported
/// identifiers, Python's leading underscore); elsewhere it does not read declaration
/// modifiers (issue 68).
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

/// A piece of text a fact carries (issue 90, decision 61).
///
/// Text that is exactly a run of the file's source is held as the run's byte range,
/// never copied: it costs the same whatever its length, so a member chain each of whose
/// links is a call, or an argument holding a call holding an argument, does not repeat
/// the text it overlaps. Text the engine assembled or rewrote, and text it read in
/// C-family source after macro expansion, which the caller never sees, is owned.
///
/// A range means something only with its extraction's source: equality and hashing
/// compare how the text is held, which is the text itself only within one extraction.
/// Compare text across extractions by what [`FileExtract::text`] gives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextRef {
    /// The source's bytes `start..end`, exactly: nothing trimmed or rewritten.
    Source {
        /// Offset of the first byte.
        start: u32,
        /// Offset one past the last byte.
        end: u32,
    },
    /// Text of the engine's own.
    Owned(String),
}

/// A definition's qualified name, by its relation to an earlier definition's (issue 90,
/// decision 61): nested definitions would otherwise each repeat the names of everything
/// around them. Every form spells exactly the engine's name; [`FileExtract::qualified_name`]
/// and [`QualifiedNames`] spell it out, and [`FileExtract::qualified_name_parts`] gives
/// its pieces in order, for hashing it without spelling it out.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QualifiedName {
    /// The whole name.
    Whole(String),
    /// The qualified name of the definition at `base`, an earlier one, then `.`, then
    /// this definition's own [`Definition::name`].
    Member {
        /// The earlier definition.
        base: u32,
    },
    /// The qualified name of the definition at `base`, an earlier one, then `rest`,
    /// which is empty or begins with an ASCII character.
    Extended {
        /// The earlier definition.
        base: u32,
        /// What follows its name.
        rest: String,
    },
    /// The first `keep` bytes of the qualified name of the definition at `base`, an
    /// earlier one, then `rest`, which is empty or begins with `.`: the prefix ends
    /// before a `.` that definition adds itself, after its own base's name. Names that
    /// share a prefix none of their definitions is (C++ functions in nested namespaces,
    /// which are no definitions) hold only what they add to it.
    Prefix {
        /// The earlier definition.
        base: u32,
        /// How many bytes of its name begin this one.
        keep: u32,
        /// What follows them.
        rest: String,
    },
}

/// Anything the graph gives a node of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    /// The name as declared.
    pub name: String,
    /// The engine's qualified name, held by its relation to an earlier definition's.
    pub qualified_name: QualifiedName,
    /// The normalised kind.
    pub kind: DefinitionKind,
    /// The engine's own kind, which the normalised kind may have generalised.
    pub engine_kind: String,
    /// The parameter list as the source writes it, where the language has one: a range
    /// of the source, since parameters can hold whole definitions (a JavaScript function
    /// as a default) whose own parameter lists would repeat them (issue 90).
    pub signature: Option<TextRef>,
    /// The documentation comment, if any.
    pub doc: Option<String>,
    /// The whole definition.
    pub span: Option<Span>,
    /// Its body.
    pub body_span: Option<Span>,
    /// The definition this one is inside, as an index into the same file's
    /// definitions; `None` at file scope.
    pub parent: Option<u32>,
    /// The engine's exportability or visibility fact. Reliable as declaration
    /// visibility only for Go and Python; a consumer that needs declaration visibility
    /// must treat every other language's value as unknown (issue 68).
    pub visibility: Visibility,
    /// A test.
    pub is_test: bool,
    /// An entry point of the program.
    pub is_entry_point: bool,
    /// The engine's raw count of branching nodes, by its language's table of branching
    /// node types. Stage 4's product metric named `cyclomatic` is 1 + this value
    /// (issue 67).
    pub cyclomatic: u32,
    /// The engine's cognitive count by its language's tables: for each branching node,
    /// one plus the number of branching nodes enclosing it (issue 67).
    pub cognitive: u32,
    /// The engine's deepest nesting of named loop nodes (issue 67).
    pub loop_depth: u32,
    /// The classes, interfaces and traits it names as its bases, exactly as the engine
    /// recorded them: in its order, spelling and case, unresolved. Empty when it names
    /// none, or for a kind that has none.
    pub base_classes: Vec<String>,
    /// The decorators, annotations or attributes it carries, as the engine recorded
    /// each, in its order (issue 47).
    pub decorators: Vec<String>,
    /// Its parameters' types as the engine reads them, in order; `?` for one it cannot
    /// read (issue 47).
    pub signature_param_types: Vec<String>,
    /// Every HTTP route binding its decorators or annotations declare, each with the
    /// node that declares it, in source order (issue 54). One per method and path: an
    /// annotation listing two paths gives two, sharing their node. A binding whose path
    /// or method is not a literal is not reported.
    pub routes: Vec<RouteFact>,
}

/// One HTTP route binding a decorator or annotation declares for a definition, and the
/// node that declares it (issue 54).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteFact {
    /// The method, in upper case; `ANY` when the declaration names none.
    pub method: String,
    /// The route's path, joined to the enclosing class's own mapping.
    pub path: String,
    /// The decorator's callee or the annotation's name, as written (`app.get`,
    /// `GetMapping`).
    pub callee_text: String,
    /// The whole declaring decorator call or annotation, as written.
    pub source_text: Option<TextRef>,
    /// The declaring node's position; `None` when the engine does not know it.
    pub span: Option<Span>,
    /// The node types from the lowest node holding both the definition and the
    /// declaring node (the definition itself for an annotation inside it; the decorated
    /// wrapper for a Python decorator) down to the declaring node; empty when not known.
    pub ast_path: Vec<String>,
}

/// One argument the engine captured at a call (issue 47).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallArg {
    /// The expression as written.
    pub expr: TextRef,
    /// The string value it resolves to, where the engine resolved one: a literal, or a
    /// constant it followed.
    pub value: Option<String>,
    /// The keyword, for a keyword argument.
    pub keyword: Option<String>,
    /// Its 0-based position.
    pub index: u32,
}

/// An `impl Trait for Type` block (Rust): which trait a type implements, as the engine
/// records it. Recorded for every such block, an empty one included; a trait's own
/// supertraits are not recorded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImplTrait {
    /// The trait as the source spells it, without type arguments (`From`, `fmt::Display`).
    pub trait_name: String,
    /// The implementing type as the source spells it, without type arguments.
    pub struct_name: String,
    /// The implementing type's qualified name, as extraction builds it for the file.
    pub struct_qn: String,
}

/// A call site, or a callable passed as a value.
///
/// Calls are in the order the engine found them, then references in the order their
/// names appear; a typed resolution refers to a call by its index in that order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// The callee as the source spells it, or as the engine names it where the source
    /// spells no one run of text (an operator, a desugared call).
    pub callee_text: TextRef,
    /// The receiver, where the engine splits it off.
    pub receiver_text: Option<String>,
    /// The definition making the call, as an index into the file's definitions: the
    /// innermost enclosing definition the extractor represents, never a nested callable
    /// it does not (issue 75), joined to it by its declaration occurrence however the
    /// engine names it (issue 76); `None` at file scope. For a call with no position in
    /// the file, never a site, the callable its name names: context for resolution.
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
    /// The syntax node types from the enclosing definition's node (the file's root at
    /// file scope) down to the node the site is recorded at: what a site's identity
    /// hashes (4.2.1, issue 46). Empty when the engine could not record it, and for a
    /// resolution's site.
    pub ast_path: Vec<String>,
    /// The arguments the engine captured, in its order; empty for a reference and a
    /// resolution's site (issue 47).
    pub args: Vec<CallArg>,
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
    pub name: TextRef,
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

/// Where an extraction says which package or namespace a definition belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NamespaceEvidence {
    /// The file's own declaration, which [`FileExtract::declared_namespace`] carries;
    /// a file without one is in the language's unnamed package or global namespace.
    FileDeclaration,
    /// The definitions' qualified names: after the file's module, the namespaces that
    /// enclose a definition, each as written (`a::b` for a nested declaration written
    /// at once), then any enclosing types, then its name.
    QualifiedNames,
    /// Nowhere: the engine records no package or namespace for the language.
    Unrecorded,
}

/// What the engine records of package and namespace declarations in `language`, by
/// the engine's language identifier. Qualified names are otherwise built from the
/// file's path, never from a declaration.
pub fn namespace_evidence(language: &str) -> NamespaceEvidence {
    match language {
        "java" | "kotlin" | "csharp" | "php" => NamespaceEvidence::FileDeclaration,
        "cpp" => NamespaceEvidence::QualifiedNames,
        _ => NamespaceEvidence::Unrecorded,
    }
}

/// Everything extraction found in one file, and its resolution surface, as it is stored
/// and sent: an extraction cache keeps it and the isolated worker returns it.
///
/// It names the file and language it was taken for and identifies the source by length
/// and digest, and holds no source: its text ranges are ranges of that source, which the
/// cache's caller and the worker's parent hold already. It is read only once bound to
/// that source ([`ExtractRecord::bind`]), and serialising it never resolves a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractRecord {
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
    /// The package or namespace the file declares, as written in its first
    /// declaration, or `None` when it declares none. The engine reads it only where
    /// [`namespace_evidence`](crate::namespace_evidence) says
    /// [`NamespaceEvidence::FileDeclaration`](crate::NamespaceEvidence::FileDeclaration);
    /// for every other language it is `None`, whatever the source says.
    pub declared_namespace: Option<String>,
    /// Definitions.
    pub definitions: Vec<Definition>,
    /// The file's `impl Trait for Type` relations, in the engine's order: Rust's only.
    pub impl_traits: Vec<ImplTrait>,
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

/// Why a record cannot be bound to a source (issue 90). A cache treats it as a miss.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindError {
    /// The source is not the length extracted.
    #[error("the source is {given} bytes, not the {extracted} extracted")]
    Length {
        /// What the record was extracted from.
        extracted: u64,
        /// What it was given.
        given: u64,
    },
    /// The source is the length extracted but not the bytes.
    #[error("the source is not the one extracted")]
    Digest,
    /// A text range does not lie within the source.
    #[error("the text range {start}..{end} is not within the {len}-byte source")]
    Range {
        /// The range's start.
        start: u32,
        /// The range's end.
        end: u32,
        /// The source's length.
        len: u64,
    },
    /// A qualified name is given relative to a definition that is not an earlier one.
    #[error(
        "definition {index}'s qualified name is given by definition {base}, not an earlier one"
    )]
    QualifiedName {
        /// The definition.
        index: usize,
        /// The definition it names.
        base: u32,
    },
    /// A qualified name's prefix of an earlier one does not end before a `.` that the
    /// earlier definition adds itself, or its rest does not begin with one.
    #[error(
        "definition {index}'s qualified name keeps {keep} bytes of definition {base}'s, which is not a prefix it adds"
    )]
    Prefix {
        /// The definition.
        index: usize,
        /// The definition it names.
        base: u32,
        /// The bytes it keeps.
        keep: u32,
    },
}

impl ExtractRecord {
    /// The record bound to `source`, which must be exactly the bytes it was extracted
    /// from: the length and the digest are checked, then every text range against the
    /// source and every qualified name's base against the definitions. Nothing is
    /// copied but the reference to `source`.
    ///
    /// # Errors
    ///
    /// [`BindError`] for any other source, and for a record whose ranges or names are
    /// not well formed, which a cache counts as a miss: never resolved, never clamped.
    pub fn bind(self, source: Arc<[u8]>) -> Result<FileExtract, BindError> {
        if source.len() as u64 != self.source_len {
            return Err(BindError::Length {
                extracted: self.source_len,
                given: source.len() as u64,
            });
        }
        if SourceDigest::of(&source) != self.source_digest {
            return Err(BindError::Digest);
        }
        self.check_well_formed()?;
        Ok(FileExtract {
            record: self,
            source,
        })
    }

    /// Every text range within the source, every qualified name's base earlier, every
    /// prefix one its base adds: from each name's length, computed once in order, and
    /// its base's own bytes, so the check is linear however deeply the names nest.
    pub(crate) fn check_well_formed(&self) -> Result<(), BindError> {
        let len = self.source_len;
        let range = |text: &TextRef| match *text {
            TextRef::Source { start, end } if start > end || u64::from(end) > len => {
                Err(BindError::Range { start, end, len })
            }
            _ => Ok(()),
        };
        let mut lens: Vec<usize> = Vec::with_capacity(self.definitions.len());
        for (index, d) in self.definitions.iter().enumerate() {
            let name_len = match &d.qualified_name {
                QualifiedName::Whole(whole) => whole.len(),
                QualifiedName::Member { base }
                | QualifiedName::Extended { base, .. }
                | QualifiedName::Prefix { base, .. }
                    if *base as usize >= index =>
                {
                    return Err(BindError::QualifiedName { index, base: *base });
                }
                QualifiedName::Member { base } => lens[*base as usize] + 1 + d.name.len(),
                QualifiedName::Extended { base, rest } => lens[*base as usize] + rest.len(),
                QualifiedName::Prefix { base, keep, rest } => {
                    if !(rest.is_empty() || rest.starts_with('.'))
                        || !self.adds_dot_at(*base as usize, *keep as usize, &lens)
                    {
                        return Err(BindError::Prefix {
                            index,
                            base: *base,
                            keep: *keep,
                        });
                    }
                    *keep as usize + rest.len()
                }
            };
            lens.push(name_len);
            d.signature.as_ref().map_or(Ok(()), range)?;
            for r in &d.routes {
                r.source_text.as_ref().map_or(Ok(()), range)?;
            }
        }
        for c in &self.calls {
            range(&c.callee_text)?;
            for a in &c.args {
                range(&a.expr)?;
            }
        }
        for u in &self.usages {
            range(&u.name)?;
        }
        Ok(())
    }

    /// Whether byte `keep` of definition `b`'s qualified name, the earlier names being
    /// `lens` long, is a `.` that `b` adds after its own base's name: read in the bytes
    /// `b` holds itself, never by walking its bases.
    fn adds_dot_at(&self, b: usize, keep: usize, lens: &[usize]) -> bool {
        let d = &self.definitions[b];
        // Where `b`'s own bytes begin, and them; a member's are `.` and its name.
        let (own, added, member) = match &d.qualified_name {
            QualifiedName::Whole(whole) => (0, whole.as_bytes(), false),
            QualifiedName::Member { base } => (lens[*base as usize], d.name.as_bytes(), true),
            QualifiedName::Extended { base, rest } => {
                (lens[*base as usize], rest.as_bytes(), false)
            }
            QualifiedName::Prefix { keep: k, rest, .. } => (*k as usize, rest.as_bytes(), false),
        };
        if keep <= own || keep >= lens[b] {
            return false;
        }
        let at = keep - own;
        added.get(if member { at - 1 } else { at }) == Some(&b'.')
    }
}

/// Everything extraction found in one file, bound to the source it was taken from.
///
/// The record's fields are read through it as if they were its own. Its text is read
/// with [`FileExtract::text`], its definitions' qualified names with
/// [`FileExtract::qualified_name`] and [`QualifiedNames`]; nothing is copied until a
/// consumer asks. One source backs the whole extraction, shared, never cloned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExtract {
    record: ExtractRecord,
    source: Arc<[u8]>,
}

impl Deref for FileExtract {
    type Target = ExtractRecord;

    fn deref(&self) -> &ExtractRecord {
        &self.record
    }
}

impl FileExtract {
    /// A record just made from `source` by this crate, bound without checking again.
    pub(crate) fn fresh(record: ExtractRecord, source: Arc<[u8]>) -> Self {
        Self { record, source }
    }

    /// The record, as it is stored and sent.
    pub fn record(&self) -> &ExtractRecord {
        &self.record
    }

    /// The record, unbound.
    pub fn into_record(self) -> ExtractRecord {
        self.record
    }

    /// The exact bytes extracted.
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// The shared source, for binding another record of the same bytes.
    pub fn shared_source(&self) -> Arc<[u8]> {
        Arc::clone(&self.source)
    }

    /// The text: a range read from the source, borrowed, or owned text as it is. Bytes
    /// that are not UTF-8 are replaced as the engine's strings always were, the same
    /// way every time.
    ///
    /// # Panics
    ///
    /// For a range that is not within this extraction's source, which only a text taken
    /// from another extraction can be: every range of this one was checked when it was
    /// bound.
    pub fn text<'a>(&'a self, text: &'a TextRef) -> Cow<'a, str> {
        match text {
            TextRef::Owned(s) => Cow::Borrowed(s),
            TextRef::Source { start, end } => {
                String::from_utf8_lossy(&self.source[*start as usize..*end as usize])
            }
        }
    }

    /// The pieces of definition `index`'s qualified name, in order: written one after
    /// another they are the name, so it can be hashed or compared without being spelled
    /// out. A prefix's base contributes only the pieces, or the parts of them, its first
    /// `keep` bytes hold.
    ///
    /// # Panics
    ///
    /// For an index past the definitions.
    pub fn qualified_name_parts(&self, index: usize) -> Vec<&str> {
        let defs = &self.record.definitions;
        // Last piece first: each step's bytes are [start, whole) of the name at hand, of
        // which the name asked for uses those before `seen`.
        let mut parts = Vec::new();
        let mut whole = self.qualified_name_len(index);
        let mut seen = whole;
        let mut at = index;
        loop {
            let d = &defs[at];
            let (start, next, pieces): (usize, Option<usize>, [&str; 2]) = match &d.qualified_name {
                QualifiedName::Whole(name) => (0, None, ["", name]),
                QualifiedName::Member { base } => (
                    whole - d.name.len() - 1,
                    Some(*base as usize),
                    [".", d.name.as_str()],
                ),
                QualifiedName::Extended { base, rest } => {
                    (whole - rest.len(), Some(*base as usize), ["", rest])
                }
                QualifiedName::Prefix { base, keep, rest } => {
                    (*keep as usize, Some(*base as usize), ["", rest])
                }
            };
            let mut offset = start;
            let mut used = [""; 2];
            for (slot, piece) in used.iter_mut().zip(pieces) {
                if offset < seen {
                    *slot = &piece[..seen.min(offset + piece.len()) - offset];
                }
                offset += piece.len();
            }
            parts.extend(used.into_iter().rev().filter(|p| !p.is_empty()));
            let Some(base) = next else { break };
            whole = if matches!(d.qualified_name, QualifiedName::Prefix { .. }) {
                self.qualified_name_len(base)
            } else {
                start
            };
            seen = seen.min(start);
            at = base;
        }
        parts.reverse();
        parts
    }

    /// The length of definition `index`'s qualified name, walking its bases only until a
    /// whole name or a prefix, whose length its own bytes give.
    fn qualified_name_len(&self, index: usize) -> usize {
        let defs = &self.record.definitions;
        let mut total = 0;
        let mut at = index;
        loop {
            match &defs[at].qualified_name {
                QualifiedName::Whole(name) => return total + name.len(),
                QualifiedName::Prefix { keep, rest, .. } => {
                    return total + *keep as usize + rest.len();
                }
                QualifiedName::Member { base } => {
                    total += 1 + defs[at].name.len();
                    at = *base as usize;
                }
                QualifiedName::Extended { base, rest } => {
                    total += rest.len();
                    at = *base as usize;
                }
            }
        }
    }

    /// Definition `index`'s qualified name, spelled out.
    ///
    /// # Panics
    ///
    /// For an index past the definitions.
    pub fn qualified_name(&self, index: usize) -> String {
        self.qualified_name_parts(index).concat()
    }

    /// Every definition's qualified name, each spelled out once.
    pub fn qualified_names(&self) -> QualifiedNames {
        QualifiedNames::of(self)
    }
}

/// Every definition's qualified name in one file, each spelled out once from its base's,
/// in one buffer: the one place a consumer that needs them all whole gets them, so no
/// name is built twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedNames {
    text: String,
    ends: Vec<usize>,
}

impl QualifiedNames {
    fn of(extract: &FileExtract) -> Self {
        let defs = &extract.record.definitions;
        let mut text: Vec<u8> = Vec::new();
        let mut ends = Vec::with_capacity(defs.len());
        for d in defs {
            match &d.qualified_name {
                QualifiedName::Whole(whole) => text.extend_from_slice(whole.as_bytes()),
                QualifiedName::Member { base } => {
                    let (start, end) = span_of(&ends, *base as usize);
                    text.extend_from_within(start..end);
                    text.push(b'.');
                    text.extend_from_slice(d.name.as_bytes());
                }
                QualifiedName::Extended { base, rest } => {
                    let (start, end) = span_of(&ends, *base as usize);
                    text.extend_from_within(start..end);
                    text.extend_from_slice(rest.as_bytes());
                }
                QualifiedName::Prefix { base, keep, rest } => {
                    let (start, _) = span_of(&ends, *base as usize);
                    text.extend_from_within(start..start + *keep as usize);
                    text.extend_from_slice(rest.as_bytes());
                }
            }
            ends.push(text.len());
        }
        // Every piece is a string, and the pieces join at whole characters: a prefix ends
        // before a `.`.
        let text = String::from_utf8(text)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
        Self { text, ends }
    }

    /// Definition `index`'s qualified name.
    ///
    /// # Panics
    ///
    /// For an index past the definitions.
    pub fn get(&self, index: usize) -> &str {
        let (start, end) = span_of(&self.ends, index);
        &self.text[start..end]
    }

    /// How many there are: one per definition.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// Whether the file has no definitions.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }
}

/// Where the `index`th of the runs ending at `ends` lies.
fn span_of(ends: &[usize], index: usize) -> (usize, usize) {
    let start = if index == 0 { 0 } else { ends[index - 1] };
    (start, ends[index])
}
