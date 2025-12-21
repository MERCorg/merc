//!
//! Implements various (antichain) based preorder checks for labelled transition systems.
//! 

#![forbid(unsafe_code)]

mod failures_refinement;
mod antichain;
mod preorder;

pub use failures_refinement::*;
pub use antichain::*;
pub use preorder::*;