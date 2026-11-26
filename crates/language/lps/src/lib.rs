#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod cfg_lps;
mod control_flow;
mod explore_explicit;
mod explore_symbolic;
mod io_lps;
mod linear_process;
mod lps_terms;
pub mod transformations;

pub use cfg_lps::CfgLinearProcessSpecification;
pub use cfg_lps::CfgMetrics;
pub use cfg_lps::SummandCfgMetrics;
pub use control_flow::CfgEdge;
pub use control_flow::CfgSummand;
pub use control_flow::CfgTarget;
pub use control_flow::ControlFlowGraph;
pub use explore_explicit::ExplicitContext;
pub use explore_explicit::ExplicitLinearProcessSpecification;
pub use explore_explicit::ExplicitSummand;
pub use explore_explicit::LpsMultiActionLabel;
pub use explore_explicit::LtsMultiActionAdapter;
pub use explore_explicit::explore_lps_explicit;
pub use explore_explicit::explore_lps_explicit_parallel;
pub use explore_explicit::lps_progress;
pub use explore_symbolic::explore_lps_symbolic;
pub use explore_symbolic::explore_lps_symbolic_to_sym;

pub use io_lps::read_lps;
pub use io_lps::write_lps;
pub use linear_process::Action;
pub use linear_process::ActionLabel;
pub use linear_process::ActionSummand;
pub use linear_process::DeadlockSummand;
pub use linear_process::LinearProcessSpecification;
pub use linear_process::MultiAction;
pub use merc_sabre::DataRewriter;
pub use merc_sabre::Rewriter;
pub use transformations::DEFAULT_PREPROCESSING;
pub use transformations::LpsTransformation;
pub use transformations::TransformedLps;
pub use transformations::preprocess;

/// A symbolic LTS with multi-action transition labels, as produced by
/// [`explore_lps_symbolic_to_sym`] and consumed by [`merc_symbolic::write_symbolic_lts`].
pub type LpsSymbolicLts = merc_symbolic::SymbolicLts<merc_lts::LtsMultiAction<merc_lts::LtsAction>>;
