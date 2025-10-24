use log::debug;
use mcrl3_lts::LabelledTransitionSystem;
use mcrl3_lts::LtsBuilder;
use mcrl3_lts::StateIndex;
use mcrl3_utilities::TagIndex;

/// A zero sized tag for the block.
pub struct BlockTag {}

/// The index for blocks.
pub type BlockIndex = TagIndex<usize, BlockTag>;

/// A trait for partition refinement algorithms that expose the block number for
/// every state. Can be used to compute the quotient labelled transition system.
///
/// The invariants are that the union of all blocks is the original set, and
/// that each block contains distinct elements
pub trait Partition {
    /// Returns the block number for the given state.
    fn block_number(&self, state_index: StateIndex) -> BlockIndex;

    /// Returns the number of blocks in the partition.
    fn num_of_blocks(&self) -> usize;

    /// Returns the number of elements in the partition.
    fn len(&self) -> usize;

    /// Returns whether the partition is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true iff the partitions are equal, runs in O(n^2)
    fn equal(&self, other: &impl Partition) -> bool {
        // Check that states in the same block, have a single (unique) number in
        // the other partition.
        for block_index in (0..self.num_of_blocks()).map(BlockIndex::new) {
            let mut other_block_index = None;

            for state_index in (0..self.len())
                .map(StateIndex::new)
                .filter(|&state_index| self.block_number(state_index) == block_index)
            {
                match other_block_index {
                    None => other_block_index = Some(other.block_number(state_index)),
                    Some(other_block_index) => {
                        if other.block_number(state_index) != other_block_index {
                            return false;
                        }
                    }
                }
            }
        }

        for block_index in (0..other.num_of_blocks()).map(BlockIndex::new) {
            let mut other_block_index = None;

            for state_index in (0..self.len())
                .map(StateIndex::new)
                .filter(|&state_index| other.block_number(state_index) == block_index)
            {
                match other_block_index {
                    None => other_block_index = Some(self.block_number(state_index)),
                    Some(other_block_index) => {
                        if self.block_number(state_index) != other_block_index {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }
}

/// Returns a new LTS based on the given partition.
///
/// All states in a single block are replaced by a single representative state.
pub fn quotient_lts(
    lts: &LabelledTransitionSystem,
    partition: &impl Partition,
    eliminate_tau_loops: bool,
) -> LabelledTransitionSystem {
    let start = std::time::Instant::now();
    // Introduce the transitions based on the block numbers, the number of blocks is a decent approximation for the number of transitions.
    let mut transitions = LtsBuilder::with_capacity(partition.num_of_blocks(), lts.num_of_labels(), partition.num_of_blocks());

    for state_index in lts.iter_states() {
        for transition in lts.outgoing_transitions(state_index) {
            let block = partition.block_number(state_index);
            let to_block = partition.block_number(transition.to);

            // If we eliminate tau loops then check if the 'to' and 'from' end up in the same block
            if !(eliminate_tau_loops && lts.is_hidden_label(transition.label) && block == to_block) {
                debug_assert!(
                    partition.block_number(state_index) < partition.num_of_blocks(),
                    "Quotienting assumes that the block numbers do not exceed the number of blocks"
                );

                // Make sure to keep the outgoing transitions sorted.
                transitions.add_transition(
                    StateIndex::new(block.value()),
                    transition.label,
                    StateIndex::new(to_block.value()),
                );
            }
        }
    }

    // Remove duplicates.
    let start2 = std::time::Instant::now();
    transitions.remove_duplicates();
    debug!("Time remove duplications: {:.3}s", start2.elapsed().as_secs_f64());

    let result = LabelledTransitionSystem::new(
        StateIndex::new(partition.block_number(lts.initial_state_index()).value()),
        Some(partition.num_of_blocks()),
        || transitions.iter(),
        lts.labels().into(),
        lts.hidden_labels().into(),
    );
    debug!("Time quotient: {:.3}s", start.elapsed().as_secs_f64());
    result
}
