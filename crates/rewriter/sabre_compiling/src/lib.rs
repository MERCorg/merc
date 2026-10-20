//! Compiling variant of the Sabre rewrite engine: generates Rust source for a
//! rewrite specification, compiles it into a dynamic library, and loads it
//! back in.
mod indenter;
mod innermost_codegen;
mod library;
mod sabre_compiling;

pub(crate) use innermost_codegen::*;

pub use sabre_compiling::SabreCompilingRewriter;
