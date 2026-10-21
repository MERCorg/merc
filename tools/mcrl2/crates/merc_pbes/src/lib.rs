//!
//! Exploration and symmetry reduction for mCRL2 parameterised boolean equation
//! systems (PBESs).
//!
//! The [`merc-pbes`] binary is a thin command line wrapper around this crate: it
//! parses the arguments, reads the PBES and then calls into the algorithms
//! defined here.
//!
//! - [`explore_pbes()`] and [`explore_pbes_parallel`] instantiate a PBES into a
//!   parity game directly via its structure graph, while [`explore_srf_pbes`]
//!   and [`explore_srf_pbes_parallel`] first convert it to standard recursive
//!   form. [`explore_pbes_symbolic`] performs LDD-based symbolic reachability.
//! - [`CfgPbesSrfLps`] wraps a PBES in SRF form with a control flow analysis
//!   ([`mcrl2::ControlFlowGraph`], shared with `merc_lps`'s explicit explorer)
//!   that prunes summands whose source-value condition cannot hold in the
//!   current state, on top of the equation-index pruning [`PbesSrfLps`] always
//!   applies.
//! - [`graph_symmetries`] detects symmetries by building the symmetry detection
//!   graph and handing it to GAP, while [`SymmetryAlgorithm`] checks candidate
//!   parameter permutations directly on the equations.
//! - [`Bsgs`] turns a set of generators into a stabilizer chain, which
//!   [`QuotientLps`] uses to canonicalize every next-state to its orbit
//!   representative during exploration.
//!
//! [`merc-pbes`]: https://mercorg.github.io/merc-website/

pub mod bsgs;
pub mod cfg_srf;
pub mod clone_iterator;
pub mod debruijn;
pub mod explore_common;
pub mod explore_pbes;
pub mod explore_srf;
pub mod explore_symbolic_srf;
pub mod graph_symmetry;
pub mod permutation;
pub mod quotient_lps;
pub mod symmetry;

pub use bsgs::Bsgs;
pub use cfg_srf::CfgMetrics;
pub use cfg_srf::CfgPbesSrfLps;
pub use cfg_srf::SummandCfgMetrics;
pub use clone_iterator::CloneIterator;
pub use explore_common::ParameterLayoutLPS;
pub use explore_common::PbesVertex;
pub use explore_common::check_parameter_basis;
pub use explore_common::explore_pbes_impl;
pub use explore_common::explore_pbes_parallel_impl;
pub use explore_common::symmetry_parameter_basis;
pub use explore_common::symmetry_unified_pbes;
pub use explore_pbes::PbesLps;
pub use explore_pbes::explore_pbes;
pub use explore_pbes::explore_pbes_parallel;
pub use explore_srf::PbesSrfLps;
pub use explore_srf::explore_srf_pbes;
pub use explore_srf::explore_srf_pbes_parallel;
pub use explore_symbolic_srf::SymbolicPbes;
pub use explore_symbolic_srf::explore_pbes_symbolic;
pub use explore_symbolic_srf::explore_pbes_symbolic_game;
pub use graph_symmetry::GapConfig;
pub use graph_symmetry::GraphSymmetryResult;
pub use graph_symmetry::Sdg;
pub use graph_symmetry::graph_symmetries;
pub use graph_symmetry::write_dot;
pub use permutation::Permutation;
pub use quotient_lps::Canonicaliser;
pub use quotient_lps::QuotientLps;
pub use symmetry::SymmetryAlgorithm;
