#![forbid(unsafe_code)]

use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;
use merc_utilities::Timing;

use crate::branching_bisim_sigref;
use crate::branching_bisim_sigref_naive;
use crate::quotient_lts_block;
use crate::quotient_lts_naive;
use crate::quotient_lts_weak;
use crate::strong_bisim_sigref;
use crate::strong_bisim_sigref_naive;
use crate::weak_bisim_sigref_inductive_naive;
use crate::weak_bisim_sigref_naive;
use crate::weak_bisimulation;
use crate::weak_bisimulation_parallel;

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Equivalence {
    /// An O(|Act|mn) algorithm for weak bisimulation equivalence.
    WeakBisim,
    /// An O(|Act|mn) algorithm for weak bisimulation equivalence that computes |Act| in parallel.
    WeakBisimParallel,
    /// A variant of the O(|Act|mn) algorithm for weak bisimulation equivalence that preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-weak-bisim"))]
    WeakBisimDivergencePreserving,
    /// A variant of the parallel O(|Act|mn) algorithm for weak bisimulation equivalence that preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-weak-bisim-parallel"))]
    WeakBisimParallelDivergencePreserving,
    /// A signature based weak bisimulation algorithm that uses inductive signatures.
    WeakBisimSigref,
    /// A naive signature based weak bisimulation algorithm.
    WeakBisimSigrefNaive,
    /// A signature based weak bisimulation algorithm that uses inductive signatures and preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-weak-bisim-sigref"))]
    WeakBisimSigrefDivergencePreserving,
    /// A naive signature based weak bisimulation algorithm that preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-weak-bisim-sigref-naive"))]
    WeakBisimSigrefNaiveDivergencePreserving,
    /// A signature based strong bisimulation algorithm.
    StrongBisim,
    /// A naive signature based strong bisimulation algorithm.
    StrongBisimNaive,
    /// A signature based branching bisimulation algorithm that uses inductive signatures.
    BranchingBisim,
    /// A naive signature based branching bisimulation algorithm.
    BranchingBisimNaive,
    /// A signature based branching bisimulation algorithm that uses inductive signatures and preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-branching-bisim"))]
    BranchingBisimDivergencePreserving,
    /// A naive signature based branching bisimulation algorithm that preserves divergence.
    #[cfg_attr(feature = "clap", clap(alias = "dp-branching-bisim-naive"))]
    BranchingBisimDivergencePreservingNaive,
}

/// Reduces `lts` modulo the given equivalence, returning the quotient LTS.
///
/// The algorithm used depends on `equivalence`; see [`Equivalence`] for the
/// available variants and their complexity.
pub fn reduce_lts<L: LTS>(
    lts: L,
    equivalence: Equivalence,
    preprocess: bool,
    timing: &Timing,
) -> LabelledTransitionSystem<L::Label> {
    let state = lts.initial_state_index();
    match equivalence {
        Equivalence::WeakBisim => {
            let (lts, _, partition) = weak_bisimulation(lts, state, preprocess, false, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, true))
        }
        Equivalence::WeakBisimDivergencePreserving => {
            let (lts, _, partition) = weak_bisimulation(lts, state, preprocess, true, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, false))
        }
        Equivalence::WeakBisimParallel => {
            let (lts, _, partition) = weak_bisimulation_parallel(lts, state, preprocess, false, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, true))
        }
        Equivalence::WeakBisimParallelDivergencePreserving => {
            let (lts, _, partition) = weak_bisimulation_parallel(lts, state, preprocess, true, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, false))
        }
        Equivalence::WeakBisimSigref => {
            let (lts, _, partition) = weak_bisim_sigref_inductive_naive(lts, state, preprocess, false, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, true))
        }
        Equivalence::WeakBisimSigrefNaive => {
            let (lts, _, partition) = weak_bisim_sigref_naive(lts, state, preprocess, false, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, true))
        }
        Equivalence::WeakBisimSigrefDivergencePreserving => {
            let (lts, _, partition) = weak_bisim_sigref_inductive_naive(lts, state, preprocess, true, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, false))
        }
        Equivalence::WeakBisimSigrefNaiveDivergencePreserving => {
            let (lts, _, partition) = weak_bisim_sigref_naive(lts, state, preprocess, true, timing);
            timing.measure("quotient", || quotient_lts_weak(&lts, &partition, false))
        }
        Equivalence::StrongBisim => {
            let (lts, partition) = strong_bisim_sigref(lts, timing);
            timing.measure("quotient", || quotient_lts_block::<_, false>(&lts, &partition, false))
        }
        Equivalence::StrongBisimNaive => {
            let (lts, partition) = strong_bisim_sigref_naive(lts, timing);
            timing.measure("quotient", || quotient_lts_naive(&lts, &partition, false, false))
        }
        Equivalence::BranchingBisim => {
            let (lts, _, partition) = branching_bisim_sigref(lts, state, false, timing);
            timing.measure("quotient", || quotient_lts_block::<_, true>(&lts, &partition, true))
        }
        Equivalence::BranchingBisimNaive => {
            let (lts, _, partition) = branching_bisim_sigref_naive(lts, state, false, timing);
            timing.measure("quotient", || quotient_lts_naive(&lts, &partition, true, true))
        }
        Equivalence::BranchingBisimDivergencePreserving => {
            let (lts, _, partition) = branching_bisim_sigref(lts, state, true, timing);
            timing.measure("quotient", || quotient_lts_block::<_, true>(&lts, &partition, false))
        }
        Equivalence::BranchingBisimDivergencePreservingNaive => {
            let (lts, _, partition) = branching_bisim_sigref_naive(lts, state, true, timing);
            timing.measure("quotient", || quotient_lts_naive(&lts, &partition, true, false))
        }
    }
}
