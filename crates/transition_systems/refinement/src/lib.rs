#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod antichain;
mod counterexample_constructor;
mod failures_refinement;
mod impossible_futures;
mod refinement;

pub(crate) use antichain::*;
pub(crate) use counterexample_constructor::*;
pub(crate) use failures_refinement::*;
pub(crate) use impossible_futures::*;

pub use counterexample_constructor::CounterExample;
pub use refinement::ExplorationStrategy;
pub use refinement::RefinementType;
pub use refinement::refines;
