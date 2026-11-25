//! The bundled mCRL2 specifications of the standard sorts, embedded as source text.
//!
//! These are exported so that other crates can use them without referring to files outside their
//! own package, which `cargo package` does not include.

/// The contents of `spec/bag.mcrl2`.
pub const BAG: &str = include_str!("../spec/bag.mcrl2");
/// The contents of `spec/bag64.mcrl2`.
pub const BAG64: &str = include_str!("../spec/bag64.mcrl2");
/// The contents of `spec/bool.mcrl2`.
pub const BOOL: &str = include_str!("../spec/bool.mcrl2");
/// The contents of `spec/comparison.mcrl2`.
pub const COMPARISON: &str = include_str!("../spec/comparison.mcrl2");
/// The contents of `spec/fbag.mcrl2`.
pub const FBAG: &str = include_str!("../spec/fbag.mcrl2");
/// The contents of `spec/fbag64.mcrl2`.
pub const FBAG64: &str = include_str!("../spec/fbag64.mcrl2");
/// The contents of `spec/fset.mcrl2`.
pub const FSET: &str = include_str!("../spec/fset.mcrl2");
/// The contents of `spec/fset64.mcrl2`.
pub const FSET64: &str = include_str!("../spec/fset64.mcrl2");
/// The contents of `spec/int.mcrl2`.
pub const INT: &str = include_str!("../spec/int.mcrl2");
/// The contents of `spec/int64.mcrl2`.
pub const INT64: &str = include_str!("../spec/int64.mcrl2");
/// The contents of `spec/list.mcrl2`.
pub const LIST: &str = include_str!("../spec/list.mcrl2");
/// The contents of `spec/list64.mcrl2`.
pub const LIST64: &str = include_str!("../spec/list64.mcrl2");
/// The contents of `spec/machine_word.mcrl2`.
pub const MACHINE_WORD: &str = include_str!("../spec/machine_word.mcrl2");
/// The contents of `spec/nat.mcrl2`.
pub const NAT: &str = include_str!("../spec/nat.mcrl2");
/// The contents of `spec/nat64.mcrl2`.
pub const NAT64: &str = include_str!("../spec/nat64.mcrl2");
/// The contents of `spec/pos.mcrl2`.
pub const POS: &str = include_str!("../spec/pos.mcrl2");
/// The contents of `spec/pos64.mcrl2`.
pub const POS64: &str = include_str!("../spec/pos64.mcrl2");
/// The contents of `spec/real.mcrl2`.
pub const REAL: &str = include_str!("../spec/real.mcrl2");
/// The contents of `spec/real64.mcrl2`.
pub const REAL64: &str = include_str!("../spec/real64.mcrl2");
/// The contents of `spec/set.mcrl2`.
pub const SET: &str = include_str!("../spec/set.mcrl2");
/// The contents of `spec/set64.mcrl2`.
pub const SET64: &str = include_str!("../spec/set64.mcrl2");
