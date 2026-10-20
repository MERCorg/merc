//! Global and thread-local storage for terms.
//!
//! A term has the form `t := c | f(t1, ..., tn) | u64`, where `f` is a function symbol with a
//! unique name and arity `n > 0`, `c` is a constant, and `u64` is a numerical term. Terms are
//! stored maximally shared in the global pool — structurally equal subterms are the same
//! allocation — and are immutable. The global pool performs garbage collection to reclaim terms
//! that are no longer reachable, tracked by each thread's local pool.
//!
//! This module uses `unsafe` for its lower-level parts; submodules that only use safe Rust are
//! marked `#![forbid(unsafe_code)]`.

mod aterm_storage;
mod gc_mutex;
mod global_aterm_pool;
mod shared_term;
mod symbol_pool;
mod thread_aterm_pool;

pub(crate) use aterm_storage::*;
pub(crate) use gc_mutex::*;
pub(crate) use global_aterm_pool::*;
pub(crate) use shared_term::*;
pub(crate) use symbol_pool::*;

// Public API re-exports.
pub use global_aterm_pool::Marker;
pub use shared_term::SharedTerm;
pub use symbol_pool::SharedSymbol;
pub use thread_aterm_pool::THREAD_TERM_POOL;
pub use thread_aterm_pool::ThreadTermPool;
