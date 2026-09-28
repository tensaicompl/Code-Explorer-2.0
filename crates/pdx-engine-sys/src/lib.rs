//! Raw bindings to the extraction engine, and its build.
//!
//! Everything here is the C interface of `engine/include/pdxe.h`, as bindgen reads
//! it, with no translation: the functions are `unsafe` to call and the structures
//! borrow memory the engine owns. The header is the contract; its comments are
//! carried over as the documentation of each item. The safe interface is the
//! `pdx-engine` crate.
//!
//! Building this crate builds the engine (`engine/`) with `CMake` and links it
//! statically, with the C++ runtime its macro preprocessor needs.

#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    missing_docs,
    clippy::all,
    clippy::pedantic
)]
mod bindings;

pub use bindings::*;
