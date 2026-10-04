//! Copying the engine's structures into owned ones.
//!
//! The one place this crate reads memory the engine owns. Everything is copied out
//! before the engine's result or project is freed; nothing returned borrows from it.
//!
//! Every structure is taken apart by naming each of its fields, never with `..`, so a
//! field the interface gains cannot be dropped here unnoticed: the bindings change,
//! and this stops compiling until the field is carried or deliberately set aside.

use std::ffi::{CStr, c_char};

use pdx_engine_sys as sys;

use crate::error::EngineError;
use crate::model::{
    Call, Channel, ChannelDirection, Definition, DefinitionKind, Diagnostic, EnvAccess,
    FileExtract, FileStatus, Import, LexicalFacts, ReadWrite, SourceDigest, Span, Surface, Throw,
    TypeRef, Usage, Visibility,
};

/// A string the engine owns, copied, or `None` for NULL.
///
/// The interface promises UTF-8, but some strings are the source's own text, and a
/// source need not be valid UTF-8; such bytes are replaced, the same way every time.
///
/// # Safety
///
/// `p` is NULL or a NUL-terminated string that is alive for the call.
pub(crate) unsafe fn optional(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a live NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// As [`optional`], for a string the contract says is never NULL.
///
/// # Safety
///
/// As for [`optional`].
pub(crate) unsafe fn required(p: *const c_char, what: &str) -> Result<String, EngineError> {
    // SAFETY: as the caller guarantees.
    unsafe { optional(p) }.ok_or_else(|| EngineError::Contract(format!("{what} is missing")))
}

/// The `n` elements at `p`.
///
/// # Safety
///
/// When `n` is non-zero, `p` points to `n` initialised elements alive for `'a`.
pub(crate) unsafe fn slice<'a, T>(p: *const T, n: u32, what: &str) -> Result<&'a [T], EngineError> {
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() {
        return Err(EngineError::Contract(format!(
            "{what} has {n} elements and no array"
        )));
    }
    // SAFETY: as the caller guarantees; a u32 count always fits a usize here.
    Ok(unsafe { std::slice::from_raw_parts(p, n as usize) })
}

/// A position, or `None` for the all-zero span that means unknown.
pub(crate) fn span(s: sys::pdxe_span) -> Option<Span> {
    let sys::pdxe_span {
        start_byte,
        end_byte,
        start_line,
        start_col,
        end_line,
        end_col,
    } = s;
    let unknown = [
        start_byte, end_byte, start_line, start_col, end_line, end_col,
    ]
    .iter()
    .all(|&v| v == 0);
    (!unknown).then_some(Span {
        start_byte,
        end_byte,
        start_line,
        start_col,
        end_line,
        end_col,
    })
}

/// An index into the file's definitions, or `None` for the no-parent sentinel.
fn index(i: u32, n_defs: u32, what: &str) -> Result<Option<u32>, EngineError> {
    if i == sys::PDXE_NO_PARENT {
        Ok(None)
    } else if i < n_defs {
        Ok(Some(i))
    } else {
        Err(EngineError::Contract(format!(
            "{what} names definition {i} of {n_defs}"
        )))
    }
}

/// The definitions of a file.
///
/// # Safety
///
/// Every string in `d` is NULL or alive for the call.
unsafe fn definition(d: &sys::pdxe_definition, n_defs: u32) -> Result<Definition, EngineError> {
    let sys::pdxe_definition {
        name,
        qualified_name,
        kind,
        engine_kind,
        signature,
        doc,
        span: whole,
        body_span,
        parent_index,
        visibility,
        is_test,
        is_entry_point,
        cyclomatic,
        cognitive,
        loop_depth,
        base_classes,
        n_base_classes,
    } = *d;
    // SAFETY: (all string reads below) the caller guarantees the strings are alive.
    let kind_name = unsafe { required(kind, "a definition's kind") }?;
    let kind = DefinitionKind::parse(&kind_name).ok_or_else(|| {
        EngineError::Contract(format!("`{kind_name}` is not a normalised definition kind"))
    })?;
    let visibility = match u32::from(visibility) {
        sys::PDXE_VIS_UNKNOWN => Visibility::Unknown,
        sys::PDXE_VIS_PUBLIC => Visibility::Public,
        sys::PDXE_VIS_NON_PUBLIC => Visibility::NonPublic,
        other => {
            return Err(EngineError::Contract(format!(
                "{other} is not a visibility"
            )));
        }
    };
    Ok(Definition {
        name: unsafe { required(name, "a definition's name") }?,
        qualified_name: unsafe { required(qualified_name, "a definition's qualified name") }?,
        kind,
        engine_kind: unsafe { required(engine_kind, "a definition's engine kind") }?,
        signature: unsafe { optional(signature) },
        doc: unsafe { optional(doc) },
        span: span(whole),
        body_span: span(body_span),
        parent: index(parent_index, n_defs, "a definition's parent")?,
        visibility,
        is_test: is_test != 0,
        is_entry_point: is_entry_point != 0,
        cyclomatic,
        cognitive,
        loop_depth,
        // SAFETY: the array, when there is one, holds `n_base_classes` strings alive
        // for the call.
        base_classes: unsafe {
            slice(
                base_classes.cast_const(),
                n_base_classes,
                "a definition's bases",
            )
        }?
        .iter()
        .map(|&b| unsafe { required(b, "a definition's base") })
        .collect::<Result<_, _>>()?,
    })
}

/// A call site, as extraction or a resolution describes it.
///
/// # Safety
///
/// Every string in `c` is NULL or alive for the call.
pub(crate) unsafe fn call(c: &sys::pdxe_call, n_defs: Option<u32>) -> Result<Call, EngineError> {
    let sys::pdxe_call {
        callee_text,
        receiver_text,
        caller_index,
        span: site,
        is_reference,
        typed_only,
        lexical,
    } = *c;
    // A resolution's site may belong to another file's result, whose definition count
    // is not at hand; only the bound the file's own result gives is checked.
    let caller = match n_defs {
        Some(n) => index(caller_index, n, "a call's caller")?,
        None => (caller_index != sys::PDXE_NO_PARENT).then_some(caller_index),
    };
    Ok(Call {
        // SAFETY: the caller guarantees the strings are alive.
        callee_text: unsafe { required(callee_text, "a call's callee") }?,
        receiver_text: unsafe { optional(receiver_text) },
        caller,
        span: span(site),
        is_reference: is_reference != 0,
        typed_only: typed_only != 0,
        lexical: LexicalFacts::from_bits(lexical),
    })
}

/// The elements of an array the engine owns, each copied by `f`.
///
/// # Safety
///
/// As for [`slice`].
unsafe fn each<T, U>(
    p: *const T,
    n: u32,
    what: &str,
    f: impl FnMut(&T) -> Result<U, EngineError>,
) -> Result<Vec<U>, EngineError> {
    // SAFETY: as the caller guarantees.
    unsafe { slice(p, n, what) }?.iter().map(f).collect()
}

/// # Safety
///
/// The strings in `i` are NULL or alive for the call.
unsafe fn import(i: &sys::pdxe_import) -> Result<Import, EngineError> {
    let sys::pdxe_import {
        module_text,
        imported_name,
        alias,
        span: at,
    } = *i;
    // SAFETY: as the caller guarantees.
    unsafe {
        Ok(Import {
            module_text: required(module_text, "an import's module")?,
            imported_name: optional(imported_name),
            alias: optional(alias),
            span: span(at),
        })
    }
}

/// # Safety
///
/// The strings in `u` are NULL or alive for the call.
unsafe fn usage(u: &sys::pdxe_usage, n_defs: u32) -> Result<Usage, EngineError> {
    let sys::pdxe_usage {
        name,
        scope_index,
        span: at,
        lexical,
    } = *u;
    Ok(Usage {
        // SAFETY: as the caller guarantees.
        name: unsafe { required(name, "a usage's name") }?,
        scope: index(scope_index, n_defs, "a usage's scope")?,
        span: span(at),
        lexical: LexicalFacts::from_bits(lexical),
    })
}

/// # Safety
///
/// The strings in `t` are NULL or alive for the call.
unsafe fn type_ref(t: &sys::pdxe_type_ref, n_defs: u32) -> Result<TypeRef, EngineError> {
    let sys::pdxe_type_ref {
        type_text,
        scope_index,
        span: at,
    } = *t;
    Ok(TypeRef {
        // SAFETY: as the caller guarantees.
        type_text: unsafe { required(type_text, "a type reference's type") }?,
        scope: index(scope_index, n_defs, "a type reference's scope")?,
        span: span(at),
    })
}

/// # Safety
///
/// The strings in `w` are NULL or alive for the call.
unsafe fn read_write(w: &sys::pdxe_rw, n_defs: u32) -> Result<ReadWrite, EngineError> {
    let sys::pdxe_rw {
        field_text,
        scope_index,
        is_write,
        span: at,
    } = *w;
    Ok(ReadWrite {
        // SAFETY: as the caller guarantees.
        field_text: unsafe { required(field_text, "an access's field") }?,
        scope: index(scope_index, n_defs, "an access's scope")?,
        is_write: is_write != 0,
        span: span(at),
    })
}

/// # Safety
///
/// The strings in `c` are NULL or alive for the call.
unsafe fn channel(c: &sys::pdxe_channel) -> Result<Channel, EngineError> {
    let sys::pdxe_channel {
        channel_text,
        is_listen,
        span: at,
    } = *c;
    Ok(Channel {
        // SAFETY: as the caller guarantees.
        channel_text: unsafe { required(channel_text, "a channel's name") }?,
        direction: if is_listen == 0 {
            ChannelDirection::Emit
        } else {
            ChannelDirection::Listen
        },
        span: span(at),
    })
}

/// # Safety
///
/// The strings in `e` are NULL or alive for the call.
unsafe fn env_access(e: &sys::pdxe_env_access) -> Result<EnvAccess, EngineError> {
    let sys::pdxe_env_access { key, span: at } = *e;
    Ok(EnvAccess {
        // SAFETY: as the caller guarantees.
        key: unsafe { required(key, "a configuration read's key") }?,
        span: span(at),
    })
}

/// # Safety
///
/// The strings in `d` are NULL or alive for the call.
unsafe fn diagnostic(d: &sys::pdxe_diag) -> Result<Diagnostic, EngineError> {
    let sys::pdxe_diag { message, span: at } = *d;
    Ok(Diagnostic {
        // SAFETY: as the caller guarantees.
        message: unsafe { required(message, "a diagnostic's message") }?,
        span: span(at),
    })
}

/// # Safety
///
/// The strings in `t` are NULL or alive for the call.
unsafe fn throw(t: &sys::pdxe_throw, n_defs: u32) -> Result<Throw, EngineError> {
    let sys::pdxe_throw {
        exception_text,
        scope_index,
        span: at,
    } = *t;
    Ok(Throw {
        // SAFETY: as the caller guarantees.
        exception_text: unsafe { required(exception_text, "a throw's exception") }?,
        scope: index(scope_index, n_defs, "a throw's scope")?,
        span: span(at),
    })
}

/// Everything an extraction result holds, copied, with the surface taken from it.
///
/// # Safety
///
/// `r` is a result the engine returned and has not freed, alive for the call.
pub(crate) unsafe fn file_extract(
    r: &sys::pdxe_file_result,
    language: &str,
    rel_path: &str,
    source: &[u8],
    surface: Surface,
) -> Result<FileExtract, EngineError> {
    let sys::pdxe_file_result {
        status,
        defs,
        n_defs,
        calls,
        n_calls,
        imports,
        n_imports,
        usages,
        n_usages,
        types,
        n_types,
        rws,
        n_rws,
        channels,
        n_channels,
        envs,
        n_envs,
        diags,
        n_diags,
        throws,
        n_throws,
        truncated,
        extraction_lost,
        declared_namespace,
    } = *r;

    let status = match u32::try_from(status) {
        Ok(sys::PDXE_FILE_PARSED) => FileStatus::Parsed,
        Ok(sys::PDXE_FILE_PARTIAL) => FileStatus::Partial,
        Ok(sys::PDXE_FILE_FAILED) => FileStatus::Failed,
        _ => {
            return Err(EngineError::Contract(format!(
                "{status} is not a file status"
            )));
        }
    };

    // SAFETY (every read below): the result is alive, each array holds its count, and
    // every string is alive with the result.
    unsafe {
        Ok(FileExtract {
            language: language.to_owned(),
            rel_path: rel_path.to_owned(),
            source_len: source.len() as u64,
            source_digest: SourceDigest::of(source),
            status,
            truncated: truncated != 0,
            extraction_lost,
            declared_namespace: optional(declared_namespace),
            definitions: each(defs, n_defs, "the definitions", |d| definition(d, n_defs))?,
            calls: each(calls, n_calls, "the calls", |c| call(c, Some(n_defs)))?,
            imports: each(imports, n_imports, "the imports", |i| import(i))?,
            usages: each(usages, n_usages, "the usages", |u| usage(u, n_defs))?,
            type_refs: each(types, n_types, "the type references", |t| {
                type_ref(t, n_defs)
            })?,
            throws: each(throws, n_throws, "the throws", |t| throw(t, n_defs))?,
            read_writes: each(rws, n_rws, "the reads and writes", |w| {
                read_write(w, n_defs)
            })?,
            channels: each(channels, n_channels, "the channels", |c| channel(c))?,
            env_accesses: each(envs, n_envs, "the configuration reads", |e| env_access(e))?,
            diagnostics: each(diags, n_diags, "the diagnostics", |d| diagnostic(d))?,
            surface,
        })
    }
}
