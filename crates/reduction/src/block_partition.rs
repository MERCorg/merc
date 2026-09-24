#![forbid(unsafe_code)]

use std::fmt;

use itertools::Itertools;

use merc_collections::BlockIndex;
use merc_lts::IncomingTransitions;
use merc_lts::StateIndex;

use super::Partition;

/// A partition that explicitly stores a list of blocks and their indexing into
/// the list of elements.
#[derive(Debug)]
pub struct BlockPartition {
    elements: Vec<StateIndex>,
    blocks: Vec<Block>,

    // These are only used to provide O(1) marking of elements.
    /// Stores the block index for each element.
    element_to_block: Vec<BlockIndex>,

    /// Stores the offset within the block for every element.
    element_offset: Vec<usize>,
}

impl BlockPartition {
    /// Create an initial partition where all the states are in a single block
    /// 0. And all the elements in the block are marked.
    pub(crate) fn new(num_of_elements: usize) -> BlockPartition {
        debug_assert!(num_of_elements > 0, "Cannot partition the empty set");

        let blocks = Vec::from([Block::new(0, num_of_elements)]);

        // Manual loops instead of `.map(StateIndex::new).collect()` because
        // Aeneas's Iterator model has no `map`/`collect`.
        let mut elements = Vec::with_capacity(num_of_elements);
        let mut element_to_block = Vec::with_capacity(num_of_elements);
        let mut element_to_block_offset = Vec::with_capacity(num_of_elements);
        for i in 0..num_of_elements {
            elements.push(StateIndex::new(i));
            element_to_block.push(BlockIndex::new(0));
            element_to_block_offset.push(i);
        }

        BlockPartition {
            elements,
            element_to_block,
            element_offset: element_to_block_offset,
            blocks,
        }
    }

    /// Partition the elements of the given block into multiple new blocks based
    /// on the given partitioner; which returns a number for each marked
    /// element. Elements with the same number belong to the same block, and the
    /// returned numbers should be dense.
    ///
    /// Returns an iterator over the new block indices, where the first element
    /// is the index of the block that was partitioned. And that block is the
    /// largest block.
    pub(crate) fn partition_marked_with<F>(
        &mut self,
        block_index: BlockIndex,
        builder: &mut BlockPartitionBuilder,
        mut partitioner: F,
    ) -> Vec<BlockIndex>
    where
        F: FnMut(StateIndex, &BlockPartition) -> BlockIndex,
    {
        debug_assert!(
            self.blocks[block_index].has_marked(),
            "Cannot partition marked elements of a block without marked elements"
        );

        // Thin wrapper around `is_trivially_partitioned`/`marked_elements_sorted`/
        // `finish_partition_marked` (kept for the tests below); `signature_refinement`
        // calls those directly instead, since Aeneas cannot translate a closure
        // that itself invokes another (generic, captured) closure like `partitioner` here.
        if self.is_trivially_partitioned(block_index) {
            return self.trivial_partition_marked(block_index);
        }

        self.marked_elements_sorted(block_index, builder);
        for element_index in 0..builder.old_elements.len() {
            let element = builder.old_elements[element_index];
            let number = partitioner(element, self);

            builder.index_to_block[element_index] = number;
            if number.value() + 1 > builder.block_sizes.len() {
                builder.block_sizes.resize(number.value() + 1, 0);
            }

            builder.block_sizes[number] += 1;
        }

        self.finish_partition_marked(block_index, builder)
    }

    /// Returns whether `block_index` is trivially partitioned: a block with a
    /// single (marked) element never needs to be split.
    pub(crate) fn is_trivially_partitioned(&self, block_index: BlockIndex) -> bool {
        self.blocks[block_index].len() == 1
    }

    /// Handles the trivial case where `block_index` has a single (marked)
    /// element: unmarks it and returns a singleton "new blocks" iterator.
    ///
    /// Only call this when [`Self::is_trivially_partitioned`] holds for `block_index`.
    pub(crate) fn trivial_partition_marked(&mut self, block_index: BlockIndex) -> Vec<BlockIndex> {
        self.blocks[block_index].unmark_all();
        Vec::from([block_index])
    }

    /// Computes the sorted marked elements of `block_index` into
    /// `builder.old_elements`, and grows `builder.index_to_block` to fit -
    /// the caller (e.g. `signature_refinement`) should then fill in one
    /// `BlockIndex` per `old_elements` entry (into `index_to_block`, growing
    /// `block_sizes` to match, mirroring `partition_marked_with`'s loop
    /// below) before calling [`Self::finish_partition_marked`].
    ///
    /// Only call this when [`Self::is_trivially_partitioned`] does not hold
    /// for `block_index`.
    pub(crate) fn marked_elements_sorted(&self, block_index: BlockIndex, builder: &mut BlockPartitionBuilder) {
        let block = self.blocks[block_index];

        // Keeps track of the block index for every element in this block by index.
        builder.index_to_block.clear();
        builder.block_sizes.clear();
        builder.old_elements.clear();

        builder.index_to_block.resize(block.len_marked(), BlockIndex::new(0));

        // O(n log n) Loop through the marked elements in order (to maintain topological sorting)
        builder.old_elements.extend(block.iter_marked(&self.elements));
        builder.old_elements.sort_unstable();
    }

    /// Finishes partitioning a non-trivial block, given `builder.old_elements`
    /// (from [`Self::marked_elements_sorted`]) with `builder.index_to_block`
    /// fully populated (one `BlockIndex` per `old_elements` entry) and
    /// `builder.block_sizes` grown to fit.
    pub(crate) fn finish_partition_marked(&mut self, block_index: BlockIndex, builder: &mut BlockPartitionBuilder) -> Vec<BlockIndex> {
        let block = self.blocks[block_index];

        // Convert block sizes into block offsets.
        let end_of_blocks = self.blocks.len();
        let new_block_index = if block.has_unmarked() {
            self.blocks.len()
        } else {
            self.blocks.len() - 1
        };

        // A plain loop with a local accumulator instead of `.iter_mut().fold(closure)`
        // because the closure captures `self` by mutable borrow, which Aeneas
        // cannot translate.
        let mut current = 0usize;
        for size in builder.block_sizes.iter_mut() {
            debug_assert!(*size > 0, "Partition is not dense, there are empty blocks");

            let new_current = if current == 0 {
                if block.has_unmarked() {
                    // Adapt the offsets of the current block to only include the unmarked elements.
                    self.blocks[block_index] = Block::new_unmarked(block.begin, block.marked_split);

                    // Introduce a new block for the zero block.
                    self.blocks
                        .push(Block::new_unmarked(block.marked_split, block.marked_split + *size));
                    block.marked_split
                } else {
                    // Use this as the zero block.
                    self.blocks[block_index] = Block::new_unmarked(block.begin, block.begin + *size);
                    block.begin
                }
            } else {
                // Introduce a new block for every other non-empty block.
                self.blocks.push(Block::new_unmarked(current, current + *size));
                current
            };

            let offset = new_current + *size;
            *size = new_current;
            current = offset;
        }
        let block_offsets = &mut builder.block_sizes;

        for (index, offset_block_index) in builder.index_to_block.iter().enumerate() {
            // Swap the element to the correct position.
            let element = builder.old_elements[index];
            self.elements[block_offsets[*offset_block_index]] = builder.old_elements[index];
            self.element_offset[element] = block_offsets[*offset_block_index];
            self.element_to_block[element] = if *offset_block_index == 0 && !block.has_unmarked() {
                block_index
            } else {
                BlockIndex::new(new_block_index + offset_block_index.value())
            };

            // Update the offset for this block.
            block_offsets[*offset_block_index] += 1;
        }

        // The new block indices: `block_index` itself, plus any block newly
        // pushed above. A manual loop instead of `.chain(..).map(BlockIndex::new)`
        // because Aeneas doesn't model `chain`.
        let mut new_block_indices = Vec::with_capacity(1 + (self.blocks.len() - end_of_blocks));
        new_block_indices.push(block_index);
        for i in end_of_blocks..self.blocks.len() {
            new_block_indices.push(BlockIndex::new(i));
        }

        // Swap the first block and the maximum sized block.
        //
        // A manual loop instead of `.max_by_key(..)` because Aeneas doesn't
        // model it either. Mirrors its "last element wins on ties" tie-breaking
        // with `>=`.
        let mut max_block_index = new_block_indices[0];
        let mut max_len = self.block(max_block_index).len();
        for &candidate in new_block_indices.iter().skip(1) {
            let candidate_len = self.block(candidate).len();
            if candidate_len >= max_len {
                max_len = candidate_len;
                max_block_index = candidate;
            }
        }
        self.swap_blocks(block_index, max_block_index);

        self.assert_consistent();

        new_block_indices
    }

    /// Split the given block into two separate block based on the splitter
    /// predicate.
    #[allow(dead_code)]
    pub(crate) fn split_marked<F>(&mut self, block_index: usize, mut splitter: F)
    where
        F: FnMut(StateIndex) -> bool,
    {
        let mut updated_block = self.blocks[block_index];
        let mut new_block: Option<Block> = None;

        // Loop over all elements, we use a while loop since the index stays the
        // same when a swap takes place.
        let mut element_index = updated_block.marked_split;
        while element_index < updated_block.end {
            let element = self.elements[element_index];
            if splitter(element) {
                match &mut new_block {
                    None => {
                        new_block = Some(Block::new_unmarked(updated_block.end - 1, updated_block.end));

                        // Swap the current element to the last place
                        self.swap_elements(element_index, updated_block.end - 1);
                        updated_block.end -= 1;
                    }
                    Some(new_block_index) => {
                        // Swap the current element to the beginning of the new block.
                        new_block_index.begin -= 1;
                        updated_block.end -= 1;

                        self.swap_elements(element_index, new_block_index.begin);
                    }
                }
            } else {
                // If no swap takes place consider the next index.
                element_index += 1;
            }
        }

        if let Some(new_block) = new_block
            && (updated_block.end - updated_block.begin) != 0
        {
            // A new block was introduced, so we need to update the current
            // block. Unless the current block is empty in which case
            // nothing changes.
            updated_block.unmark_all();
            self.blocks[block_index] = updated_block;

            // Introduce a new block for the split, containing only the new element.
            self.blocks.push(new_block);

            // Update the elements for the new block
            for element in new_block.iter(&self.elements) {
                self.element_to_block[element] = BlockIndex::new(self.blocks.len() - 1);
            }
        }

        self.assert_consistent();
    }

    /// Makes the marked elements closed under the silent closure of incoming
    /// tau-transitions within the current block.
    pub(crate) fn mark_backward_closure(
        &mut self,
        block_index: BlockIndex,
        incoming_transitions: &IncomingTransitions,
    ) {
        let block = self.blocks[block_index];
        let mut it = block.end - 1;

        // First compute backwards silent transitive closure.
        while it >= self.blocks[block_index].marked_split && self.blocks[block_index].has_unmarked() {
            for transition in incoming_transitions.incoming_silent_transitions(self.elements[it]) {
                if self.block_number(transition.from) == block_index {
                    self.mark_element(transition.from);
                }
            }

            if it == 0 {
                break;
            }

            it -= 1;
        }

        for element in block.iter_marked(&self.elements) {
            debug_assert!(
                incoming_transitions
                    .incoming_silent_transitions(element)
                    .all(|transition| self.block_number(transition.from) != block_index
                        || self.is_element_marked(transition.from)),
                "All silent transitions from marked elements should be marked"
            );
        }
    }

    /// Swaps the given blocks given by the indices.
    pub(crate) fn swap_blocks(&mut self, left_index: BlockIndex, right_index: BlockIndex) {
        if left_index == right_index {
            // Nothing to do.
            return;
        }

        // Manual swap instead of `Vec::swap` - Aeneas's modeled `Vec`/`Slice`
        // method set does not include `swap`.
        let left_block = self.blocks[left_index];
        self.blocks[left_index] = self.blocks[right_index];
        self.blocks[right_index] = left_block;

        // Explicit index ranges instead of `Block::iter` - Aeneas fails to
        // translate the second of two structurally-identical loops in this
        // shape ("Unimplemented"), so avoid the custom-iterator adaptor here.
        let left_block = self.blocks[left_index];
        for i in left_block.begin..left_block.end {
            self.element_to_block[self.elements[i]] = left_index;
        }

        let right_block = self.blocks[right_index];
        for i in right_block.begin..right_block.end {
            self.element_to_block[self.elements[i]] = right_index;
        }

        self.assert_consistent();
    }

    /// Marks the given element, such that it is returned by iter_marked.
    pub(crate) fn mark_element(&mut self, element: StateIndex) {
        let block_index = self.element_to_block[element];
        let offset = self.element_offset[element];
        let marked_split = self.blocks[block_index].marked_split;

        if offset < marked_split {
            // Element was not already marked.
            self.swap_elements(offset, marked_split - 1);
            self.blocks[block_index].marked_split -= 1;
        }

        self.blocks[block_index].assert_consistent();
    }

    /// Returns true iff the given element has already been marked.
    pub fn is_element_marked(&self, element: StateIndex) -> bool {
        let block_index = self.element_to_block[element];
        let offset = self.element_offset[element];
        let marked_split = self.blocks[block_index].marked_split;

        offset >= marked_split
    }

    /// Return a reference to the given block.
    pub fn block(&self, block_index: BlockIndex) -> &Block {
        &self.blocks[block_index]
    }

    /// Returns the number of blocks in the partition.
    pub fn num_of_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// Returns an iterator over the elements of a given block.
    pub fn iter_block(&self, block_index: BlockIndex) -> BlockIter<'_> {
        BlockIter {
            elements: &self.elements,
            index: self.blocks[block_index].begin,
            end: self.blocks[block_index].end,
        }
    }

    /// Swaps the elements at the given indices and updates the element_to_block
    fn swap_elements(&mut self, left_index: usize, right_index: usize) {
        self.elements.swap(left_index, right_index);
        self.element_offset[self.elements[left_index]] = left_index;
        self.element_offset[self.elements[right_index]] = right_index;
    }

    /// Returns true iff the invariants of a partition hold
    fn assert_consistent(&self) -> bool {
        if cfg!(debug_assertions) {
            let mut marked = vec![false; self.elements.len()];

            for block in &self.blocks {
                for element in block.iter(&self.elements) {
                    debug_assert!(
                        !marked[element],
                        "Partition {self:?}, element {element} belongs to multiple blocks"
                    );
                    marked[element] = true;
                }

                block.assert_consistent();
            }

            // Check that every element belongs to a block.
            debug_assert!(
                !marked.contains(&false),
                "Partition {self:?} contains elements that do not belong to a block"
            );

            // Check that it belongs to the block indicated by element_to_block.
            //
            // A plain index loop instead of `self.element_to_block.iter().enumerate()`
            // because Aeneas can't reconcile borrowing one field (`element_to_block`)
            // via an iterator while indexing others (`blocks`, `elements`) in the body.
            for current_element in 0..self.element_to_block.len() {
                let block_index = self.element_to_block[current_element];
                debug_assert!(
                    self.blocks[block_index.value()]
                        .iter(&self.elements)
                        .any(|element| element == current_element),
                    "Partition {self:?}, element {current_element} does not belong to block {block_index} as indicated by element_to_block"
                );

                let index = self.element_offset[current_element];
                debug_assert_eq!(
                    self.elements[index], current_element,
                    "Partition {self:?}, element {current_element} does not have the correct offset in the block"
                );
            }
        }

        true
    }
}

#[derive(Default)]
pub(crate) struct BlockPartitionBuilder {
    // Keeps track of the block index for every element in this block by index.
    //
    // Fields are `pub(crate)` because `signature_refinement` fills
    // `index_to_block`/`block_sizes` directly in its own loop, between
    // calling `BlockPartition::marked_elements_sorted` and
    // `BlockPartition::finish_partition_marked`.
    pub(crate) index_to_block: Vec<BlockIndex>,

    /// Keeps track of the size of each block.
    pub(crate) block_sizes: Vec<usize>,

    /// Stores the old elements to perform the swaps safely.
    pub(crate) old_elements: Vec<StateIndex>,
}

impl Partition for BlockPartition {
    fn block_number(&self, element: StateIndex) -> BlockIndex {
        self.element_to_block[element.value()]
    }

    fn num_of_blocks(&self) -> usize {
        self.blocks.len()
    }

    fn len(&self) -> usize {
        self.elements.len()
    }
}

impl fmt::Display for BlockPartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let blocks_str = self.blocks.iter().format_with(", ", |block, f| {
            let elements = block
                .iter_unmarked(&self.elements)
                .map(|e| (e, false))
                .chain(block.iter_marked(&self.elements).map(|e| (e, true)))
                .format_with(", ", |(e, marked), f| {
                    if marked {
                        f(&format_args!("{}*", e))
                    } else {
                        f(&format_args!("{}", e))
                    }
                });

            f(&format_args!("{{{}}}", elements))
        });

        write!(f, "{{{}}}", blocks_str)
    }
}

/// A block stores a subset of the elements in a partition.
///
/// # Details
///
/// A block uses `start`, `middle` and `end` indices to indicate a range
/// `start`..`end` of elements in the partition. The middle is used such that
/// `marked_split`..`end` are the marked elements. This is useful to be able to
/// split off new blocks cheaply.
///
/// Invariant: `start` <= `middle` <= `end` && `start` < `end`.
#[derive(Clone, Copy, Debug)]
pub struct Block {
    begin: usize,
    marked_split: usize,
    end: usize,
}

impl Block {
    /// Creates a new block where every element is marked.
    pub(crate) fn new(begin: usize, end: usize) -> Block {
        debug_assert!(begin < end, "The range of this block is incorrect");

        Block {
            begin,
            marked_split: begin,
            end,
        }
    }

    pub(crate) fn new_unmarked(begin: usize, end: usize) -> Block {
        debug_assert!(begin < end, "The range {begin} to {end} of this block is incorrect");

        Block {
            begin,
            marked_split: end,
            end,
        }
    }

    /// Returns an iterator over the elements in this block.
    pub(crate) fn iter<'a>(&self, elements: &'a [StateIndex]) -> BlockIter<'a> {
        BlockIter {
            elements,
            index: self.begin,
            end: self.end,
        }
    }

    /// Returns an iterator over the marked elements in this block.
    pub(crate) fn iter_marked<'a>(&self, elements: &'a [StateIndex]) -> BlockIter<'a> {
        BlockIter {
            elements,
            index: self.marked_split,
            end: self.end,
        }
    }

    /// Returns an iterator over the unmarked elements in this block.
    pub(crate) fn iter_unmarked<'a>(&self, elements: &'a [StateIndex]) -> BlockIter<'a> {
        BlockIter {
            elements,
            index: self.begin,
            end: self.marked_split,
        }
    }

    /// Returns true iff the block has marked elements.
    pub(crate) fn has_marked(&self) -> bool {
        self.assert_consistent();

        self.marked_split < self.end
    }

    /// Returns true iff the block has unmarked elements.
    pub(crate) fn has_unmarked(&self) -> bool {
        self.assert_consistent();

        self.begin < self.marked_split
    }

    /// Returns the number of elements in the block.
    ///
    /// A block always satisfies `begin < end`, so it is never empty; there is
    /// deliberately no `is_empty`.
    #[allow(clippy::len_without_is_empty)]
    pub(crate) fn len(&self) -> usize {
        self.assert_consistent();

        self.end - self.begin
    }

    /// Returns the number of marked elements in the block.
    pub(crate) fn len_marked(&self) -> usize {
        self.assert_consistent();

        self.end - self.marked_split
    }

    /// Unmark all elements in the block.
    fn unmark_all(&mut self) {
        self.marked_split = self.end;
    }

    /// Returns true iff the block is consistent.
    fn assert_consistent(&self) {
        debug_assert!(self.begin < self.end, "The range of block {self:?} is incorrect",);

        debug_assert!(
            self.begin <= self.marked_split,
            "The marked_split lies before the beginning of the block {self:?}"
        );

        debug_assert!(
            self.marked_split <= self.end,
            "The marked_split lies after the beginning of the block {self:?}"
        );
    }
}

pub struct BlockIter<'a> {
    elements: &'a [StateIndex],
    index: usize,
    end: usize,
}

impl Iterator for BlockIter<'_> {
    type Item = StateIndex;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.end {
            let element = self.elements[self.index];
            self.index += 1;
            Some(element)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use merc_lts::StateIndex;
    use test_log::test;

    use merc_collections::BlockIndex;

    use crate::BlockPartition;
    use crate::BlockPartitionBuilder;

    #[test]
    fn test_block_partition_split() {
        let mut partition = BlockPartition::new(10);

        partition.split_marked(0, |element| element < 3);

        // The new block only has elements that satisfy the predicate.
        for element in partition.iter_block(BlockIndex::new(1)) {
            assert!(element < 3);
        }

        for element in partition.iter_block(BlockIndex::new(0)) {
            assert!(element >= 3);
        }

        for i in (0..10).map(StateIndex::new) {
            partition.mark_element(i);
        }

        partition.split_marked(0, |element| element < 7);
        for element in partition.iter_block(BlockIndex::new(2)) {
            assert!((3..7).contains(&element.value()));
        }

        for element in partition.iter_block(BlockIndex::new(0)) {
            assert!(element >= 7);
        }

        // Test the case where all elements belong to the split block.
        partition.split_marked(1, |element| element < 7);
    }

    #[test]
    fn test_block_partition_partitioning() {
        // Test the partitioning function for a random assignment of elements
        let mut partition = BlockPartition::new(10);
        let mut builder = BlockPartitionBuilder::default();

        let _ = partition.partition_marked_with(BlockIndex::new(0), &mut builder, |element, _| match element.value() {
            0..=1 => BlockIndex::new(0),
            2..=6 => BlockIndex::new(1),
            _ => BlockIndex::new(2),
        });

        partition.mark_element(StateIndex::new(7));
        partition.mark_element(StateIndex::new(8));
        let _ = partition.partition_marked_with(BlockIndex::new(2), &mut builder, |element, _| match element.value() {
            7 => BlockIndex::new(0),
            8 => BlockIndex::new(1),
            _ => BlockIndex::new(2),
        });
    }
}
