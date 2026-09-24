use std::mem::swap;

use bumpalo::Bump;
use log::debug;
use log::info;
use log::log_enabled;
use log::trace;
use merc_io::TimeProgress;
use merc_lts::IncomingTransitions;
use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::LabelledTransitionSystem;
use merc_lts::StateIndex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use merc_collections::BlockIndex;
use merc_collections::IndexedPartition;
use merc_utilities::Timing;

use crate::BlockPartition;
use crate::BlockPartitionBuilder;
use crate::DivergencePreservingLts;
use crate::Partition;
use crate::PartitionTree;
use crate::RefinementForest;
use crate::Signature;
use crate::SignatureBuilder;
use crate::branching_bisim_signature;
use crate::branching_bisim_signature_inductive;
use crate::branching_bisim_signature_sorted;
use crate::is_tau_hat;
use crate::longest_tau_path;
use crate::quotient_lts_block;
use crate::strong_bisim_signature;
use crate::tau_cycle_elimination_and_reorder;
use crate::weak_bisim_presignature_sorted;
use crate::weak_bisim_signature_sorted;
use crate::weak_bisim_signature_sorted_full;
use crate::weak_bisim_signature_sorted_taus;

/// Computes a strong bisimulation partitioning using signature refinement
pub fn strong_bisim_sigref<L: LTS>(lts: L, timing: &Timing) -> (L, BlockPartition) {
    let incoming = IncomingTransitions::new(&lts);

    let partition = timing.measure("reduction", || {
        signature_refinement::<_, _, _, false>(
            &lts,
            &incoming,
            |state_index, partition, _, mut builder| {
                strong_bisim_signature(state_index, &lts, partition, &mut builder);
                builder
            },
            |_, _| None,
        )
    });

    (lts, partition)
}

/// Computes a strong bisimulation partitioning using signature refinement
pub(crate) fn strong_bisim_sigref_naive<L: LTS>(lts: L, timing: &Timing) -> (L, IndexedPartition) {
    let partition = strong_bisim_sigref_naive_impl(&lts, &mut (), timing);
    (lts, partition)
}

/// Computes a strong bisimulation partitioning using signature refinement,
/// additionally returning the [`PartitionTree`] recorded during refinement.
///
/// The tree can be used to reconstruct a minimal-depth distinguishing formula
/// for any two states found in different blocks, via
/// [`PartitionTree::distinguish`].
pub(crate) fn strong_bisim_sigref_naive_with_counterexample<L: LTS>(
    lts: L,
    timing: &Timing,
) -> (L, IndexedPartition, PartitionTree) {
    let mut tree = PartitionTree::new(lts.num_of_states());
    let partition = strong_bisim_sigref_naive_impl(&lts, &mut tree, timing);
    (lts, partition, tree)
}

/// Shared implementation of the actual naive strong bisimulation refinement.
fn strong_bisim_sigref_naive_impl<L: LTS, R: RefinementForest>(
    lts: &L,
    forest: &mut R,
    timing: &Timing,
) -> IndexedPartition {
    timing.measure("reduction", || {
        signature_refinement_naive::<_, _, _, false>(
            lts,
            |state_index, partition, _, builder| {
                strong_bisim_signature(state_index, lts, partition, builder);
            },
            forest,
        )
    })
}

/// Computes a branching bisimulation partitioning using signature refinement.
///
/// The `state` is any state for which we return the equivalent state in the
/// preprocessed LTS. And if `divergence_preserving` is true, we compute
/// divergence preserving branching bisimulation instead.
pub fn branching_bisim_sigref<L: LTS>(
    lts: L,
    state: StateIndex,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, BlockPartition) {
    let (preprocessed_lts, mapped_state) = timing.measure("preprocess", || {
        tau_cycle_elimination_and_reorder(lts, state, !divergence_preserving)
    });

    let partition = if divergence_preserving {
        branching_bisim_sigref_impl(&DivergencePreservingLts::new(&preprocessed_lts), timing)
    } else {
        branching_bisim_sigref_impl(&preprocessed_lts, timing)
    };

    (preprocessed_lts, mapped_state, partition)
}

/// Implementation of [branching_bisim_sigref].
fn branching_bisim_sigref_impl<L: LTS>(preprocessed_lts: &L, timing: &Timing) -> BlockPartition {
    let incoming = timing.measure("preprocess", || IncomingTransitions::new(preprocessed_lts));

    if log_enabled!(log::Level::Debug) {
        let path = longest_tau_path(preprocessed_lts);
        debug!("longest_tau_path" = path.len(); "The longest tau path is {:?}", path);
    }

    let mut expected_builder = SignatureBuilder::default();
    let mut visited = FxHashSet::default();
    let mut stack = Vec::new();

    timing.measure("reduction", || {
        signature_refinement::<_, _, _, true>(
            preprocessed_lts,
            &incoming,
            |state_index, partition, state_to_key, mut builder| {
                branching_bisim_signature_inductive(state_index, preprocessed_lts, partition, state_to_key, &mut builder);

                // Compute the expected signature, only used in debugging.
                if cfg!(debug_assertions) {
                    branching_bisim_signature(
                        state_index,
                        preprocessed_lts,
                        partition,
                        &mut expected_builder,
                        &mut visited,
                        &mut stack,
                    );
                    let expected_result = builder.clone();

                    let signature = Signature::new(&builder);
                    debug_assert_eq!(
                        signature.as_slice(),
                        expected_result,
                        "The sorted and expected signature should be the same"
                    );
                }

                builder
            },
            |signature, key_to_signature| {
                // Inductive signatures.
                for (label, key) in signature.iter().rev() {
                    if is_tau_hat(*label, preprocessed_lts)
                        && key_to_signature[*key].is_subset_of(signature, (*label, *key))
                    {
                        return Some(*key);
                    }

                    if !is_tau_hat(*label, preprocessed_lts) {
                        return None;
                    }
                }

                None
            },
        )
    })
}

/// Computes a branching bisimulation partitioning using signature refinement
/// without dirty blocks.
///
/// The `state` is any state for which we return the equivalent state in the
/// preprocessed LTS. And if `divergence_preserving` is true, we compute
/// divergence preserving branching bisimulation instead.
pub(crate) fn branching_bisim_sigref_naive<L: LTS>(
    lts: L,
    state: StateIndex,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, IndexedPartition) {
    let (preprocessed_lts, mapped_state) = timing.measure("preprocess", || {
        tau_cycle_elimination_and_reorder(lts, state, !divergence_preserving)
    });

    let partition = if divergence_preserving {
        branching_bisim_sigref_naive_impl(&DivergencePreservingLts::new(&preprocessed_lts), timing)
    } else {
        branching_bisim_sigref_naive_impl(&preprocessed_lts, timing)
    };

    (preprocessed_lts, mapped_state, partition)
}

/// Implementation of [branching_bisim_sigref_naive].
fn branching_bisim_sigref_naive_impl<L: LTS>(preprocessed_lts: &L, timing: &Timing) -> IndexedPartition {
    timing.measure("reduction", || {
        let mut expected_builder = SignatureBuilder::default();
        let mut visited = FxHashSet::default();
        let mut stack = Vec::new();

        signature_refinement_naive::<_, _, _, false>(
            preprocessed_lts,
            |state_index, partition, state_to_signature, builder| {
                branching_bisim_signature_sorted(state_index, preprocessed_lts, partition, state_to_signature, builder);

                // Compute the expected signature, only used in debugging.
                if cfg!(debug_assertions) {
                    branching_bisim_signature(
                        state_index,
                        preprocessed_lts,
                        partition,
                        &mut expected_builder,
                        &mut visited,
                        &mut stack,
                    );
                    let expected_result = builder.clone();

                    let signature = Signature::new(builder);
                    debug_assert_eq!(
                        signature.as_slice(),
                        expected_result,
                        "The sorted and expected signature should be the same"
                    );
                }
            },
            &mut (),
        )
    })
}

/// Computes a branching bisimulation partitioning using signature refinement without dirty blocks.
///
/// The `state` is any state for which we return the equivalent state in the preprocessed LTS.
pub(crate) fn weak_bisim_sigref_inductive_naive<L: LTS>(
    lts: L,
    state: StateIndex,
    preprocess: bool,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, IndexedPartition) {
    // Preprocess the LTS if desired.
    if preprocess {
        let (preprocessed_lts, mapped_state, partition) =
            branching_bisim_sigref(lts, state, divergence_preserving, timing);
        let quotiented_state = StateIndex::new(*partition.block_number(mapped_state));
        let lts = timing.measure("quotient", || {
            quotient_lts_block::<_, true>(&preprocessed_lts, &partition, !divergence_preserving)
        });
        weak_bisim_sigref_inductive_naive_impl(lts, quotiented_state, divergence_preserving, timing)
    } else {
        weak_bisim_sigref_inductive_naive_impl(lts, state, divergence_preserving, timing)
    }
}

/// Implementation of [weak_bisim_sigref_inductive_naive] that deals with both preprocessed and regular LTSs.
///
/// The `state` is any state for which we return the equivalent state in the preprocessed LTS.
pub(crate) fn weak_bisim_sigref_inductive_naive_impl<L: LTS>(
    lts: L,
    state: StateIndex,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, IndexedPartition) {
    let (preprocessed_lts, mapped_state) = timing.measure("preprocess", || {
        tau_cycle_elimination_and_reorder(lts, state, !divergence_preserving)
    });
    let partition = timing.measure("reduction", || {
        if divergence_preserving {
            signature_refinement_weak(&DivergencePreservingLts::new(&preprocessed_lts))
        } else {
            signature_refinement_weak(&preprocessed_lts)
        }
    });
    (preprocessed_lts, mapped_state, partition)
}

/// Computes a branching bisimulation partitioning using signature refinement without dirty blocks.
///
/// The `state` is any state for which we return the equivalent state in the preprocessed LTS.
pub(crate) fn weak_bisim_sigref_naive<L: LTS>(
    lts: L,
    state: StateIndex,
    preprocess: bool,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, IndexedPartition) {
    // Preprocess the LTS if desired.
    if preprocess {
        let (preprocessed_lts, mapped_state, partition) =
            branching_bisim_sigref(lts, state, divergence_preserving, timing);
        let quotiented_state = StateIndex::new(*partition.block_number(mapped_state));
        let lts = timing.measure("quotient", || {
            quotient_lts_block::<_, true>(&preprocessed_lts, &partition, !divergence_preserving)
        });
        weak_bisim_sigref_naive_impl(lts, quotiented_state, divergence_preserving, timing)
    } else {
        weak_bisim_sigref_naive_impl(lts, state, divergence_preserving, timing)
    }
}

/// Implementation of [weak_bisim_sigref_naive] that deals with both
/// preprocessed and regular LTSs.
///
/// The `state` is any state for which we return the equivalent state in the
/// preprocessed LTS.
fn weak_bisim_sigref_naive_impl<L: LTS>(
    lts: L,
    state: StateIndex,
    divergence_preserving: bool,
    timing: &Timing,
) -> (LabelledTransitionSystem<L::Label>, StateIndex, IndexedPartition) {
    let (preprocessed_lts, mapped_state) = timing.measure("preprocess", || {
        tau_cycle_elimination_and_reorder(lts, state, !divergence_preserving)
    });
    let partition = timing.measure("reduction", || {
        if divergence_preserving {
            let divergence_preserving_lts = DivergencePreservingLts::new(&preprocessed_lts);
            signature_refinement_naive::<_, _, _, true>(
                &divergence_preserving_lts,
                |state_index, partition, state_to_signature, builder| {
                    weak_bisim_signature_sorted(
                        state_index,
                        &divergence_preserving_lts,
                        partition,
                        state_to_signature,
                        builder,
                    )
                },
                &mut (),
            )
        } else {
            signature_refinement_naive::<_, _, _, true>(
                &preprocessed_lts,
                |state_index, partition, state_to_signature, builder| {
                    weak_bisim_signature_sorted(state_index, &preprocessed_lts, partition, state_to_signature, builder)
                },
                &mut (),
            )
        }
    });

    (preprocessed_lts, mapped_state, partition)
}

/// Interns `slice` into `arena`, returning `&[]` directly if it's empty (a
/// workaround for a data race in bumpalo with zero-sized slices).
fn intern_slice<'a>(arena: &'a Bump, slice: &[(LabelIndex, BlockIndex)]) -> &'a [(LabelIndex, BlockIndex)] {
    if slice.is_empty() { &[] } else { arena.alloc_slice_copy(slice) }
}

/// Looks up `builder`'s signature in the (per-worklist-iteration) interning
/// table `id`, or interns it (via `intern_slice`/`arena`) and assigns it a
/// fresh `BlockIndex` if not already present.
///
/// Pulled out because Aeneas needs the `FxHashMap` `get_key_value`/`insert`
/// calls to stay in an external function.
fn intern_signature<'a>(
    arena: &'a Bump,
    id: &mut FxHashMap<Signature<'a>, BlockIndex>,
    key_to_signature: &mut Vec<Signature<'a>>,
    builder: &SignatureBuilder,
) -> BlockIndex {
    if let Some((_, index)) = id.get_key_value(&Signature::new(builder)) {
        *index
    } else {
        let slice = intern_slice(arena, builder);
        let number = BlockIndex::new(key_to_signature.len());
        id.insert(Signature::new(slice), number);
        key_to_signature.push(Signature::new(slice));
        number
    }
}

/// Computes the `BlockIndex` for a single marked state's signature: uses
/// `renumber`'s inductive renumbering if it applies, otherwise looks up (or
/// interns) the flat signature in `builder` via [`intern_signature`].
///
/// Pulled out because Aeneas cannot translate the closure that would result
/// from merging the `renumber` branch back into the caller.
fn compute_signature_index<'a, G>(
    arena: &'a Bump,
    id: &mut FxHashMap<Signature<'a>, BlockIndex>,
    key_to_signature: &mut Vec<Signature<'a>>,
    builder: &SignatureBuilder,
    renumber: &mut G,
) -> BlockIndex
where
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
{
    if let Some(key) = renumber(builder, key_to_signature) {
        key
    } else {
        intern_signature(arena, id, key_to_signature, builder)
    }
}

/// Registers a new occurrence of block `index` in `block_sizes`, growing it
/// first if `index` is new.
///
/// Pulled out because Aeneas fails to translate the merged context after
/// conditionally resizing `block_sizes` when inlined directly in
/// `signature_refinement`.
fn count_block_occurrence(block_sizes: &mut Vec<usize>, index: BlockIndex) {
    if index.value() + 1 > block_sizes.len() {
        block_sizes.resize(index.value() + 1, 0);
    }
    block_sizes[index] += 1;
}

/// Computes the new block indices from partitioning the marked elements of
/// `block_index`: the trivial (single marked element) case, or the general
/// case (which needs signature computation via [`process_marked_elements`]).
///
/// Pulled out because Aeneas fails to merge the translated context of the
/// trivial and general branches when inlined in `signature_refinement`.
#[allow(clippy::too_many_arguments)]
fn partition_marked<'a, F, G>(
    partition: &mut BlockPartition,
    block_index: BlockIndex,
    arena: &'a Bump,
    id: &mut FxHashMap<Signature<'a>, BlockIndex>,
    key_to_signature: &mut Vec<Signature<'a>>,
    signature_builder: &mut SignatureBuilder,
    split_builder: &mut BlockPartitionBuilder,
    state_to_key: &mut [BlockIndex],
    signature: &mut F,
    renumber: &mut G,
) -> Vec<BlockIndex>
where
    F: FnMut(StateIndex, &BlockPartition, &[BlockIndex], SignatureBuilder) -> SignatureBuilder,
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
{
    if partition.is_trivially_partitioned(block_index) {
        return partition.trivial_partition_marked(block_index);
    }

    partition.marked_elements_sorted(block_index, split_builder);

    process_marked_elements(
        partition,
        arena,
        id,
        key_to_signature,
        signature_builder,
        split_builder,
        state_to_key,
        signature,
        renumber,
    );

    partition.finish_partition_marked(block_index, split_builder)
}

/// Computes and records the target `BlockIndex` for every marked element in
/// `split_builder.old_elements`.
///
/// Pulled out because Aeneas cannot translate a `while` loop nested inside
/// `signature_refinement`'s own outer `while` loop, regardless of what the
/// inner loop's body contains.
#[allow(clippy::too_many_arguments)]
fn process_marked_elements<'a, F, G>(
    lts_partition: &BlockPartition,
    arena: &'a Bump,
    id: &mut FxHashMap<Signature<'a>, BlockIndex>,
    key_to_signature: &mut Vec<Signature<'a>>,
    signature_builder: &mut SignatureBuilder,
    split_builder: &mut BlockPartitionBuilder,
    state_to_key: &mut [BlockIndex],
    signature: &mut F,
    renumber: &mut G,
) where
    F: FnMut(StateIndex, &BlockPartition, &[BlockIndex], SignatureBuilder) -> SignatureBuilder,
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
{
    let mut element_index = 0;
    while element_index < split_builder.old_elements.len() {
        let state_index = split_builder.old_elements[element_index];

        // `mem::take` + call-by-value + write-back instead of passing
        // `signature_builder` as `&mut` directly to `signature` - Aeneas's
        // Lean model of `FnMut`/`FnOnce` cannot account for a `&mut`
        // reference nested inside a generically-called closure's argument.
        let builder = std::mem::take(signature_builder);
        *signature_builder = signature(state_index, lts_partition, state_to_key, builder);

        let index = compute_signature_index(arena, id, key_to_signature, signature_builder, renumber);

        split_builder.index_to_block[element_index] = index;
        count_block_occurrence(&mut split_builder.block_sizes, index);

        // (branching) Keep track of the signature for every block in the next partition.
        state_to_key[state_index] = index;

        element_index += 1;
    }
}

/// Marks the states with incoming transitions into `new_block_index` as
/// dirty (queuing their blocks on `worklist` if not already marked).
///
/// Pulled out because Aeneas fails to translate this nested-loop
/// dirty-marking logic when inlined directly in `signature_refinement`.
fn mark_dirty_states<L: LTS, const BRANCHING: bool>(
    lts: &L,
    partition: &mut BlockPartition,
    incoming: &IncomingTransitions,
    worklist: &mut Vec<BlockIndex>,
    states: &mut Vec<StateIndex>,
    new_block_index: BlockIndex,
    num_blocks: usize,
) {
    states.clear();
    states.extend(partition.iter_block(new_block_index));

    for &state_index in states.iter() {
        for transition in incoming.incoming_transitions(state_index) {
            if BRANCHING {
                // Mark incoming states into old blocks, or visible actions.
                if !lts.is_hidden_label(transition.label) || partition.block_number(transition.from) < num_blocks {
                    let other_block = partition.block_number(transition.from);
                    if !partition.block(other_block).has_marked() {
                        // If block was not already marked then add it to the worklist.
                        worklist.push(other_block);
                    }
                    partition.mark_element(transition.from);
                }
            } else {
                // In this case mark all incoming states.
                let other_block = partition.block_number(transition.from);
                if !partition.block(other_block).has_marked() {
                    // If block was not already marked then add it to the worklist.
                    worklist.push(other_block);
                }
                partition.mark_element(transition.from);
            }
        }
    }
}

/// Marks the incoming states of every newly-created block (any
/// `new_block_indices` entry other than `block_index` itself) as dirty.
///
/// Pulled out because Aeneas fails to translate this loop over
/// [`mark_dirty_states`] when inlined directly in `signature_refinement`.
fn mark_dirty_new_blocks<L: LTS, const BRANCHING: bool>(
    lts: &L,
    partition: &mut BlockPartition,
    incoming: &IncomingTransitions,
    worklist: &mut Vec<BlockIndex>,
    states: &mut Vec<StateIndex>,
    block_index: BlockIndex,
    new_block_indices: Vec<BlockIndex>,
    num_blocks: usize,
) {
    for new_block_index in new_block_indices {
        if block_index != new_block_index {
            mark_dirty_states::<L, BRANCHING>(lts, partition, incoming, worklist, states, new_block_index, num_blocks);
        }
    }
}

/// Bundles the state [`process_worklist_block`] threads across
/// worklist-loop iterations into a single mutable borrow, because Aeneas
/// fails to translate the loop when each piece of state is threaded through
/// as a separate parameter.
struct WorklistContext<F, G> {
    partition: BlockPartition,
    worklist: Vec<BlockIndex>,
    states: Vec<StateIndex>,
    builder: SignatureBuilder,
    split_builder: BlockPartitionBuilder,
    state_to_key: Vec<BlockIndex>,
    signature: F,
    renumber: G,
}

/// Processes one dirty block popped from the worklist: computes signatures
/// for its marked elements, splits it accordingly, and marks the incoming
/// states of any newly-created blocks as dirty.
///
/// Pulled out because Aeneas fails to translate the worklist loop with this
/// body inlined directly in `signature_refinement`.
fn process_worklist_block<F, G, L, const BRANCHING: bool>(
    lts: &L,
    incoming: &IncomingTransitions,
    ctx: &mut WorklistContext<F, G>,
    block_index: BlockIndex,
) where
    F: FnMut(StateIndex, &BlockPartition, &[BlockIndex], SignatureBuilder) -> SignatureBuilder,
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
    L: LTS,
{
    // A fresh arena and signature index for this iteration only - instead of
    // resetting and reusing the previous iteration's arena (which needs
    // relaxing the borrow's lifetime to convince the borrow checker the old,
    // now-dangling slices are gone), the old arena and its interned
    // signatures are simply dropped wholesale and a new one allocated. This
    // avoids reallocations *within* an iteration, at the cost of a fresh
    // allocation *per* iteration.
    let arena = Bump::new();
    let mut id: FxHashMap<Signature<'_>, BlockIndex> = FxHashMap::default();
    let mut key_to_signature: Vec<Signature<'_>> = Vec::new();

    debug_assert!(
        ctx.partition.block(block_index).has_marked(),
        "Every block in the worklist should have at least one marked state"
    );

    maybe_mark_backward_closure::<BRANCHING>(&mut ctx.partition, block_index, incoming);

    // Blocks above this number are new in this iteration.
    let num_blocks = ctx.partition.num_of_blocks();

    // Delegated to `partition_marked` because Aeneas cannot translate a
    // closure that itself invokes another (generic, captured) closure.
    let new_block_indices: Vec<BlockIndex> = partition_marked(
        &mut ctx.partition,
        block_index,
        &arena,
        &mut id,
        &mut key_to_signature,
        &mut ctx.builder,
        &mut ctx.split_builder,
        &mut ctx.state_to_key,
        &mut ctx.signature,
        &mut ctx.renumber,
    );

    // If this is a new block, mark the incoming states as dirty.
    mark_dirty_new_blocks::<L, BRANCHING>(
        lts,
        &mut ctx.partition,
        incoming,
        &mut ctx.worklist,
        &mut ctx.states,
        block_index,
        new_block_indices,
        num_blocks,
    );
}

/// Aeneas cannot translate the mixed mutable-borrow if/else here, so this is
/// pulled out into its own function.
fn maybe_mark_backward_closure<const BRANCHING: bool>(
    partition: &mut BlockPartition,
    block_index: BlockIndex,
    incoming: &IncomingTransitions,
) {
    if BRANCHING {
        partition.mark_backward_closure(block_index, incoming);
    }
}

/// Logs the progress of [signature_refinement]'s worklist loop.
///
/// Pulled out (instead of an inline closure) because Aeneas cannot translate
/// the `log` crate's macro expansion.
fn log_signature_refinement_progress((iteration, blocks): (usize, usize)) {
    info!("Iteration {iteration}, found {blocks} blocks...");
}

/// Signature refinement algorithm that accepts an arbitrary signature and uses
/// process-the-smaller-half optimisation by marking dirty states.
///
/// The `signature` function is called for each state and should fill the
/// signature builder with the signature of the state.
///
/// The `renumber` function can be used to renumber the signatures, which is
/// used in inductive signatures.
///
/// If `BRANCHING` then incoming tau-paths are considered for marking the
/// incoming blocks. Furthermore, the signature function receives the
/// `state_to_key` mapping that contains the signature index for every state,
/// required for inductive signatures. And the signatures are computed in the
/// order of the given `lts`.
fn signature_refinement<F, G, L, const BRANCHING: bool>(
    lts: &L,
    incoming: &IncomingTransitions,
    signature: F,
    renumber: G,
) -> BlockPartition
where
    F: FnMut(StateIndex, &BlockPartition, &[BlockIndex], SignatureBuilder) -> SignatureBuilder,
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
    L: LTS,
{
    let mut state_to_key: Vec<BlockIndex> = Vec::new();
    state_to_key.resize_with(lts.num_of_states(), || BlockIndex::new(0));

    // Used to keep track of dirty blocks.
    // `Vec::from([..])` instead of `vec![..]` because Aeneas cannot
    // translate the `vec!` macro expansion.
    let mut ctx = WorklistContext {
        partition: BlockPartition::new(lts.num_of_states()),
        worklist: Vec::from([BlockIndex::new(0)]),
        states: Vec::new(),
        builder: SignatureBuilder::default(),
        split_builder: BlockPartitionBuilder::default(),
        state_to_key,
        signature,
        renumber,
    };

    // Delegated to `run_worklist_loop` because Aeneas cannot translate this
    // `while` loop inlined here.
    run_worklist_loop::<F, G, L, BRANCHING>(lts, incoming, &mut ctx);

    ctx.partition
}

/// Runs [`process_worklist_block`] until the worklist is empty, logging
/// progress every few seconds.
///
/// Pulled out of [`signature_refinement`]'s own body - see the comment at
/// its call site.
fn run_worklist_loop<F, G, L, const BRANCHING: bool>(lts: &L, incoming: &IncomingTransitions, ctx: &mut WorklistContext<F, G>)
where
    F: FnMut(StateIndex, &BlockPartition, &[BlockIndex], SignatureBuilder) -> SignatureBuilder,
    G: FnMut(&[(LabelIndex, BlockIndex)], &Vec<Signature>) -> Option<BlockIndex>,
    L: LTS,
{
    let mut iteration = 0usize;
    let progress = TimeProgress::new(log_signature_refinement_progress, 5);

    while let Some(block_index) = ctx.worklist.pop() {
        process_worklist_block::<F, G, L, BRANCHING>(lts, incoming, ctx, block_index);

        iteration += 1;

        progress.print((iteration, ctx.partition.num_of_blocks()));
    }
}

/// Weak signature refinement algorithm, doing inductive signatures naively.
///
/// The signature function is called for each state and should fill the
/// signature builder with the pre_signature of the state.
fn signature_refinement_weak<L: LTS>(lts: &L) -> IndexedPartition {
    // Avoids reallocations when computing the signature.
    let mut arena = Bump::new();
    let mut builder = SignatureBuilder::default();

    // Put all the states in the initial partition { S }.
    let mut id: FxHashMap<Signature<'_>, BlockIndex> = FxHashMap::default();

    // Assigns the signature to each state.
    let mut partition = IndexedPartition::new(lts.num_of_states());
    let mut next_partition = IndexedPartition::new(lts.num_of_states());
    let mut state_to_signature: Vec<Option<usize>> = Vec::new();
    let mut key_to_signature: Vec<Signature> = Vec::new();
    let mut state_to_taus: Vec<Signature> = Vec::new();

    state_to_signature.resize_with(lts.num_of_states(), || None);
    state_to_taus.resize_with(lts.num_of_states(), Signature::default);

    let mut old_count = 1;
    let mut iteration = 0;

    let progress = TimeProgress::new(
        |(iteration, blocks)| {
            debug!("Iteration {iteration}, found {blocks} blocks...",);
        },
        5,
    );

    // This is a workaround for a data race in bumpalo for zero-sized slices.
    let empty_slice: &[(LabelIndex, BlockIndex)] = &[];
    // Refine partitions until stable.

    while old_count != id.len() {
        old_count = id.len();
        progress.print((iteration, old_count));
        swap(&mut partition, &mut next_partition);

        // Clear every collection that borrows from the arena *before* resetting
        // it, so that no `Signature` borrow can outlive the storage it points
        // into.
        id.clear();
        state_to_signature.clear();
        key_to_signature.clear();
        state_to_taus.clear();

        // Remove the current signatures; safe now that every borrowing collection
        // has been cleared above.
        arena.reset();

        state_to_signature.resize_with(lts.num_of_states(), || None);

        // SAFETY: `id` was cleared above, so it holds no `Signature` borrowing
        // from the arena; the transmute only relaxes that borrow's lifetime so the
        // map can be refilled with slices allocated from the freshly reset arena
        // during this iteration.
        let id: &'_ mut FxHashMap<Signature<'_>, BlockIndex> = unsafe { std::mem::transmute(&mut id) };
        // SAFETY: see above; `key_to_signature` was cleared.
        let key_to_signature: &'_ mut Vec<Signature<'_>> = unsafe { std::mem::transmute(&mut key_to_signature) };
        // SAFETY: see above; `state_to_taus` was cleared.
        let state_to_taus: &'_ mut Vec<Signature<'_>> = unsafe { std::mem::transmute(&mut state_to_taus) };

        // Compute for each state its tau signature. This seems inefficient, but for now it works.
        state_to_taus.resize_with(lts.num_of_states(), Signature::default);
        for state in lts.iter_states() {
            weak_bisim_signature_sorted_taus(state, lts, &partition, state_to_taus, &mut builder);

            let slice = if builder.is_empty() {
                empty_slice
            } else {
                arena.alloc_slice_copy(&builder)
            };
            state_to_taus[state] = Signature::new(slice);
        }

        for state_index in lts.iter_states() {
            // Compute the Presignature of a single state
            weak_bisim_presignature_sorted(
                state_index,
                lts,
                &partition,
                state_to_taus,
                &state_to_signature,
                &mut builder,
            );

            // Inductive step see if presig is a subset of a tau reachable state.
            let mut inductive_key = None;
            for keyvalue in builder.as_slice() {
                if is_tau_hat(keyvalue.0, lts) {
                    let tau_sig = &key_to_signature[keyvalue.1.value()];
                    let presig = Signature::new(&builder);

                    if tau_sig.is_subset_of(presig.as_slice(), *keyvalue) {
                        inductive_key = Some(*keyvalue.1);
                        break;
                    }
                }
            }
            if let Some(inductive_key) = inductive_key {
                trace!(
                    "State {state_index} with pre {:?} uses inductive key {inductive_key}:{:?}",
                    builder.as_slice(),
                    key_to_signature[inductive_key].as_slice()
                );
                state_to_signature[state_index] = Some(inductive_key);
                next_partition.set_block(*state_index, BlockIndex::new(inductive_key));
            } else {
                // If not: expand the signature completely.
                weak_bisim_signature_sorted_full(
                    state_index,
                    lts,
                    &partition,
                    state_to_taus,
                    &state_to_signature,
                    key_to_signature,
                    &mut builder,
                );
                trace!("State {state_index} final signature {:?}", builder.as_slice());

                // Keep track of the index for every state
                let mut new_id = BlockIndex::new(key_to_signature.len());
                if let Some((_signature, index)) = id.get_key_value(&Signature::new(&builder)) {
                    state_to_signature[state_index] = Some(index.value());
                    new_id = *index;
                } else {
                    let slice = if builder.is_empty() {
                        empty_slice
                    } else {
                        arena.alloc_slice_copy(&builder)
                    };
                    id.insert(Signature::new(slice), new_id);
                    key_to_signature.push(Signature::new(slice));

                    state_to_signature[state_index] = Some(new_id.value());
                }

                next_partition.set_block(*state_index, new_id);
            };
        }

        iteration += 1;

        debug_assert!(
            iteration <= lts.num_of_states().max(2),
            "There can never be more splits than number of states, but at least two iterations for stability"
        );
    }

    trace!("Refinement partition {partition}");
    partition
}

/// General signature refinement algorithm that accepts an arbitrary signature
///
/// The signature function is called for each state and should fill the
/// signature builder with the signature of the state. It consists of the
/// current partition, the signatures per state for the next partition.
///
/// The `forest` records the block-history of the refinement, which can later
/// be used to reconstruct a distinguishing formula for any two states in
/// different final blocks. Pass `&mut ()` when this is not needed; the no-op
/// implementation is free.
fn signature_refinement_naive<F, L: LTS, R: RefinementForest, const WEAK: bool>(
    lts: &L,
    mut signature: F,
    forest: &mut R,
) -> IndexedPartition
where
    F: FnMut(StateIndex, &IndexedPartition, &Vec<Signature<'_>>, &mut SignatureBuilder),
{
    // Avoids reallocations when computing the signature.
    let mut arena = Bump::new();
    let mut builder = SignatureBuilder::default();

    // Put all the states in the initial partition { S }.
    let mut id: FxHashMap<Signature<'_>, BlockIndex> = FxHashMap::default();

    // Assigns the signature to each state.
    let mut partition = IndexedPartition::new(lts.num_of_states());
    let mut next_partition = IndexedPartition::new(lts.num_of_states());
    let mut state_to_signature: Vec<Signature<'_>> = Vec::new();
    state_to_signature.resize_with(lts.num_of_states(), Signature::default);

    // Refine partitions until stable.
    let mut old_count = 1;
    let mut iteration = 0;

    let progress = TimeProgress::new(
        |(iteration, blocks)| {
            debug!("Iteration {iteration}, found {blocks} blocks...",);
        },
        5,
    );

    // This is a workaround for a data race in bumpalo for zero-sized slices.
    let empty_slice: &[(LabelIndex, BlockIndex)] = &[];

    while old_count != id.len() {
        trace!("Iteration {} ({} blocks)", iteration, id.len());

        old_count = id.len();
        progress.print((iteration, old_count));
        swap(&mut partition, &mut next_partition);

        // Clear the current partition to start the next blocks.
        id.clear();

        state_to_signature.clear();
        state_to_signature.resize_with(lts.num_of_states(), Signature::default);

        // SAFETY: `id` was cleared above, so it holds no `Signature` borrowing
        // from the arena; the transmute only relaxes that borrow's lifetime so it
        // can be refilled with slices allocated after the `arena.reset()` below.
        let id: &'_ mut FxHashMap<Signature<'_>, BlockIndex> = unsafe { std::mem::transmute(&mut id) };
        // SAFETY: see above; `state_to_signature` was cleared (it now holds only
        // the empty default signatures, which borrow no arena storage).
        let state_to_signature: &mut Vec<Signature<'_>> = unsafe { std::mem::transmute(&mut state_to_signature) };

        // Remove the current signatures.
        arena.reset();

        if WEAK {
            for state_index in lts.iter_states() {
                weak_bisim_signature_sorted_taus(state_index, lts, &partition, state_to_signature, &mut builder);

                trace!("State {state_index} weak signature {:?}", builder);

                // Keep track of the index for every state, either use the arena to allocate space or simply borrow the value.
                let slice = if builder.is_empty() {
                    empty_slice
                } else {
                    arena.alloc_slice_copy(&builder)
                };
                state_to_signature[state_index] = Signature::new(slice);
            }
        }

        for state_index in lts.iter_states() {
            // Compute the signature of a single state
            signature(state_index, &partition, state_to_signature, &mut builder);

            trace!("State {state_index} signature {builder:?}");

            // Keep track of the index for every state, either use the arena to allocate space or simply borrow the value.
            let mut new_id = BlockIndex::new(id.len());
            if let Some((signature, index)) = id.get_key_value(&Signature::new(&builder)) {
                // SAFETY: `signature` borrows from the arena, which outlives this
                // iteration; the transmute only re-labels that borrow with the
                // lifetime expected by `state_to_signature`.
                state_to_signature[state_index] = unsafe {
                    std::mem::transmute::<Signature<'_>, Signature<'_>>(Signature::new(signature.as_slice()))
                };
                new_id = *index;
            } else {
                let slice = if builder.is_empty() {
                    empty_slice
                } else {
                    arena.alloc_slice_copy(&builder)
                };
                id.insert(Signature::new(slice), new_id);

                // (branching) Keep track of the signature for every block in the next partition.
                state_to_signature[state_index] = Signature::new(slice);
            }

            next_partition.set_block(*state_index, new_id);
        }

        forest.record_level(&partition, &next_partition);

        iteration += 1;

        debug_assert!(
            iteration <= lts.num_of_states().max(2),
            "There can never be more splits than number of states, but at least two iterations for stability"
        );
    }

    forest.finalize(lts, &partition);

    trace!("Refinement partition {partition}");
    debug_assert!(
        is_valid_refinement(lts, &partition, |state_index, partition, builder| signature(
            state_index,
            partition,
            &state_to_signature,
            builder
        )),
        "The resulting partition is not a valid partition."
    );
    partition
}

/// Returns true iff the given partition is a strong bisimulation partition
pub(crate) fn is_valid_refinement<F, P, L>(lts: &L, partition: &P, mut compute_signature: F) -> bool
where
    F: FnMut(StateIndex, &P, &mut SignatureBuilder),
    P: Partition,
    L: LTS,
{
    // Check that the partition is indeed stable and as such is a quotient of strong bisimulation
    let mut block_to_signature: Vec<Option<SignatureBuilder>> = vec![None; partition.num_of_blocks()];

    // Avoids reallocations when computing the signature.
    let mut builder = SignatureBuilder::default();

    for state_index in lts.iter_states() {
        let block = partition.block_number(state_index);

        // Compute the flat signature, which has Hash and is more compact.
        compute_signature(state_index, partition, &mut builder);
        let signature: Vec<(LabelIndex, BlockIndex)> = builder.clone();

        if let Some(block_signature) = &block_to_signature[block] {
            if signature != *block_signature {
                trace!(
                    "State {state_index} has a different signature {signature:?} then the block {block} which has signature {block_signature:?}"
                );
                return false;
            }
        } else {
            block_to_signature[block] = Some(signature);
        };
    }

    // Check if there are two blocks with the same signature
    let mut signature_to_block: FxHashMap<Signature, usize> = FxHashMap::default();

    for (block_index, signature) in block_to_signature
        .iter()
        .map(|signature: &Option<SignatureBuilder>| signature.as_ref().expect("Signature should be defined"))
        .enumerate()
    {
        if let Some(other_block_index) = signature_to_block.get(&Signature::new(signature)) {
            if block_index != *other_block_index {
                trace!("Block {block_index} and {other_block_index} have the same signature {signature:?}");
                return false;
            }
        } else {
            signature_to_block.insert(Signature::new(signature), block_index);
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use merc_io::DumpFiles;
    use merc_lts::random_lts;
    use merc_lts::write_aut;
    use merc_utilities::Timing;
    use merc_utilities::random_test;

    use super::BlockIndex;
    use super::LTS;
    use super::Partition;
    use super::StateIndex;
    use super::branching_bisim_sigref;
    use super::branching_bisim_sigref_naive;
    use super::strong_bisim_sigref;
    use super::strong_bisim_sigref_naive;
    use super::weak_bisim_sigref_inductive_naive;
    use super::weak_bisim_sigref_naive;

    use merc_lts::LabelIndex;
    use merc_lts::LabelledTransitionSystem;
    use merc_lts::TransitionLabel;

    /// A tiny deterministic instance that exercises the `unsafe` arena-reuse path
    /// in [`signature_refinement`] under miri; the randomized tests below build
    /// 1000-state systems and are skipped under miri because they are too slow.
    #[test]
    fn test_strong_bisim_sigref_small() {
        // 0 -a-> 1, 0 -a-> 2, 1 -a-> 3, 2 -a-> 3 (label index 1 is "a", 0 is tau).
        let transitions = [(0, 1, 1), (0, 1, 2), (1, 1, 3), (2, 1, 3)]
            .map(|(from, label, to)| (StateIndex::new(from), LabelIndex::new(label), StateIndex::new(to)));

        let lts = LabelledTransitionSystem::new(
            StateIndex::new(0),
            None,
            || transitions.iter().cloned(),
            vec![String::tau_label(), "a".to_string()],
        );

        let timing = Timing::new();
        let (_, partition) = strong_bisim_sigref(lts, &timing);

        // States 1 and 2 are strongly bisimilar (both only do a -> 3), so the four
        // states collapse into the three blocks {0}, {1, 2}, {3}.
        assert_eq!(partition.num_of_blocks(), 3);
        assert_eq!(
            partition.block_number(StateIndex::new(1)),
            partition.block_number(StateIndex::new(2))
        );
    }

    /// Exercises the weak signature-refinement arena path
    /// ([`signature_refinement_weak`], where `arena.reset()` is interleaved with
    /// clearing the borrowing collections) under miri, for the same reason as
    /// [`test_strong_bisim_sigref_small`].
    #[test]
    fn test_weak_bisim_sigref_small() {
        // 0 -tau-> 1, 1 -a-> 2, 0 -a-> 2 (label index 1 is "a", 0 is tau).
        let transitions = [(0, 0, 1), (1, 1, 2), (0, 1, 2)]
            .map(|(from, label, to)| (StateIndex::new(from), LabelIndex::new(label), StateIndex::new(to)));

        let lts = LabelledTransitionSystem::new(
            StateIndex::new(0),
            None,
            || transitions.iter().cloned(),
            vec![String::tau_label(), "a".to_string()],
        );

        let timing = Timing::new();
        let (_, _, partition) = weak_bisim_sigref_inductive_naive(lts, StateIndex::new(0), false, false, &timing);

        // State 2 is a deadlock while state 0 can still perform a, so they must end
        // up in different blocks.
        assert_ne!(
            partition.block_number(StateIndex::new(0)),
            partition.block_number(StateIndex::new(2))
        );
    }

    /// Returns true iff the partitions are equal, runs in O(n^2).
    fn equal_partitions<P: Partition, Q: Partition>(left: &P, right: &Q) -> bool {
        // Check that states in the same block, have a single (unique) number in
        // the other partition.
        for block_index in (0..left.num_of_blocks()).map(BlockIndex::new) {
            let mut other_block_index = None;

            for state_index in (0..left.len())
                .map(StateIndex::new)
                .filter(|&state_index| left.block_number(state_index) == block_index)
            {
                match other_block_index {
                    None => other_block_index = Some(right.block_number(state_index)),
                    Some(other_block_index) => {
                        if right.block_number(state_index) != other_block_index {
                            return false;
                        }
                    }
                }
            }
        }

        for block_index in (0..right.num_of_blocks()).map(BlockIndex::new) {
            let mut other_block_index = None;

            for state_index in (0..left.len())
                .map(StateIndex::new)
                .filter(|&state_index| right.block_number(state_index) == block_index)
            {
                match other_block_index {
                    None => other_block_index = Some(left.block_number(state_index)),
                    Some(other_block_index) => {
                        if left.block_number(state_index) != other_block_index {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }

    /// Checks that the strong bisimulation partition is a refinement of the branching bisimulation partition.
    fn is_refinement<L: LTS, P: Partition, Q: Partition>(lts: &L, strong_partition: &P, branching_partition: &Q) {
        for state_index in lts.iter_states() {
            for other_state_index in lts.iter_states() {
                if strong_partition.block_number(state_index) == strong_partition.block_number(other_state_index) {
                    // If the states are together according to strong bisimilarity, then they should also be together according to branching bisimilarity.
                    assert_eq!(
                        branching_partition.block_number(state_index),
                        branching_partition.block_number(other_state_index),
                        "The strong partition should be a refinement of the branching partition, but states {state_index} and {other_state_index} are in different strong blocks"
                    );
                }
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_random_strong_bisim_sigref() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_strong_bisim_sigref");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |writer| write_aut(writer, &lts)).unwrap();

            let timing = Timing::new();

            let (result_lts, result_partition) = strong_bisim_sigref(lts.clone(), &timing);
            let (expected_lts, expected_partition) = strong_bisim_sigref_naive(lts, &timing);

            files
                .dump("result.aut", |writer| write_aut(writer, &result_lts))
                .unwrap();
            files
                .dump("expected.aut", |writer| write_aut(writer, &expected_lts))
                .unwrap();

            // There is no preprocessing so this works.
            assert!(equal_partitions(&result_partition, &expected_partition));
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_random_branching_bisim_sigref() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_branching_bisim_sigref");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |writer| write_aut(writer, &lts)).unwrap();

            let timing = Timing::new();

            let (result_lts, _, result_partition) =
                branching_bisim_sigref(lts.clone(), StateIndex::new(0), false, &timing);
            let (expected_lts, _, expected_partition) =
                branching_bisim_sigref_naive(lts, StateIndex::new(0), false, &timing);

            files
                .dump("result.aut", |writer| write_aut(writer, &result_lts))
                .unwrap();
            files
                .dump("expected.aut", |writer| write_aut(writer, &expected_lts))
                .unwrap();

            // There is no preprocessing so this works.
            assert!(equal_partitions(&result_partition, &expected_partition));
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_random_weak_bisim_sigref() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_weak_bisim_sigref");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |writer| write_aut(writer, &lts)).unwrap();

            let timing = Timing::new();

            let (result_lts, _, result_partition) =
                weak_bisim_sigref_naive(lts.clone(), StateIndex::new(0), false, false, &timing);
            let (expected_lts, _, expected_partition) =
                weak_bisim_sigref_inductive_naive(lts, StateIndex::new(0), false, false, &timing);

            files
                .dump("result.aut", |writer| write_aut(writer, &result_lts))
                .unwrap();
            files
                .dump("expected.aut", |writer| write_aut(writer, &expected_lts))
                .unwrap();

            // There is no preprocessing so this works.
            assert!(equal_partitions(&result_partition, &expected_partition));
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_random_branching_bisim_sigref_naive() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_branching_bisim_sigref_naive");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |writer| write_aut(writer, &lts)).unwrap();

            let timing = Timing::new();

            let (preprocessed_lts, _, branching_partition) =
                branching_bisim_sigref_naive(lts, StateIndex::new(0), false, &timing);
            files
                .dump("preprocessed.aut", |writer| write_aut(writer, &preprocessed_lts))
                .unwrap();

            let strong_partition = strong_bisim_sigref_naive(preprocessed_lts.clone(), &timing).1;
            is_refinement(&preprocessed_lts, &strong_partition, &branching_partition);
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_random_weak_bisim_sigref_naive() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_weak_bisim_sigref_naive");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |writer| write_aut(writer, &lts)).unwrap();

            let timing = Timing::new();

            let (preprocessed_lts, _, weak_partition) =
                weak_bisim_sigref_naive(lts, StateIndex::new(0), false, false, &timing);
            files
                .dump("preprocessed.aut", |writer| write_aut(writer, &preprocessed_lts))
                .unwrap();

            let (_, _, branching_partition) =
                branching_bisim_sigref_naive(preprocessed_lts.clone(), StateIndex::new(0), false, &timing);
            is_refinement(&preprocessed_lts, &branching_partition, &weak_partition);
        });
    }

    /// Exercises the `unsafe` arena/lifetime-reuse paths in the signature
    /// refinement implementations on small inputs.
    #[test]
    fn test_miri_sigref_unsafe_paths() {
        random_test(3, |rng| {
            let lts = random_lts::<String, _>(rng, 6, 3);
            let timing = Timing::new();

            // signature_refinement (strong + branching with inductive renumbering).
            let _ = strong_bisim_sigref(lts.clone(), &timing);
            let _ = branching_bisim_sigref(lts.clone(), StateIndex::new(0), false, &timing);
            let _ = branching_bisim_sigref(lts.clone(), StateIndex::new(0), true, &timing);

            // signature_refinement_naive (strong, branching and weak signatures).
            let _ = strong_bisim_sigref_naive(lts.clone(), &timing);
            let _ = branching_bisim_sigref_naive(lts.clone(), StateIndex::new(0), false, &timing);
            let _ = weak_bisim_sigref_naive(lts.clone(), StateIndex::new(0), false, false, &timing);

            // signature_refinement_weak (inductive weak signatures).
            let _ = weak_bisim_sigref_inductive_naive(lts, StateIndex::new(0), false, false, &timing);
        });
    }
}
