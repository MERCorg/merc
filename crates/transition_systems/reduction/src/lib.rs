#![doc = include_str!("../README.md")]

mod block_partition;
mod compare;
mod distinguishing_formula;
mod divergence_preserving;
mod diverges;
mod longest_tau_path;
mod partition;
mod partition_tree;
mod quotient;
mod reduce;
mod scc_decomposition;
mod signature_refinement;
mod signatures;
mod simple_block_partition;
mod sort_topological;
mod weak_bisimulation;

pub(crate) use block_partition::*;
pub(crate) use divergence_preserving::*;
pub(crate) use longest_tau_path::*;
pub(crate) use partition_tree::*;
pub(crate) use quotient::*;
pub(crate) use signature_refinement::*;
pub(crate) use signatures::*;
pub(crate) use simple_block_partition::*;
pub(crate) use sort_topological::*;
pub(crate) use weak_bisimulation::*;

pub use block_partition::BlockPartition;
pub use compare::compare_lts;
pub use distinguishing_formula::DistinguishingFormula;
pub use diverges::diverges;
pub use partition::Partition;
pub use quotient::quotient_lts_block;
pub use quotient::quotient_lts_naive;
pub use reduce::Equivalence;
pub use reduce::reduce_lts;
pub use scc_decomposition::tau_scc_decomposition;
pub use scc_decomposition::tau_scc_decomposition_iterative;
pub use signature_refinement::branching_bisim_sigref;
pub use signature_refinement::strong_bisim_sigref;
