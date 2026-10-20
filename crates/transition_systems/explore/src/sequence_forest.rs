use std::hash::BuildHasher;
use std::hash::Hash;
use std::mem;

use merc_unsafety::ConcurrentAppendVec;
use merc_unsafety::ShardedHashMap;
use rustc_hash::FxBuildHasher;

/// The default branching factor, matches a standard cache line of words.
const DEFAULT_BRANCHING: usize = 8;

/// Shards in the per-height interning tables.
const FOREST_SHARDS: usize = 16;

/// A value that can be stored in a `SequenceForest` node slot.
///
/// Nodes are untyped `[V; N]` arrays with neither a length nor a leaf/inner
/// discriminant, so the slot type itself must represent three things: a real
/// leaf value, an interior child reference, and an unused (empty) slot.
///
/// Implementations must keep these disjoint: [`Slot::EMPTY`] must never equal a
/// real value, and a slot produced by [`Slot::from_child`] is only ever decoded
/// with [`Slot::as_child`] when the traversal already knows (from the node's
/// height) that the slot holds a child reference.
pub trait Slot: Copy + Eq + Hash {
    /// Sentinel stored in unused trailing slots, in place of a per-node length.
    /// It must be a value the producer of real values never emits.
    const EMPTY: Self;

    /// Encodes a child node index into a slot value.
    fn from_child(index: usize) -> Self;

    /// Decodes a slot that the caller already knows (from the node's height)
    /// holds a child index.
    fn as_child(self) -> usize;
}

impl Slot for usize {
    const EMPTY: usize = usize::MAX;

    fn from_child(index: usize) -> usize {
        index
    }

    fn as_child(self) -> usize {
        self
    }
}

impl Slot for u32 {
    const EMPTY: u32 = u32::MAX;

    fn from_child(index: usize) -> u32 {
        // A `u32` slot caps the node pool at `u32::MAX` exclusive, since that
        // value is reserved for `EMPTY`. Encode defensively so exhausting that
        // cap is caught in debug builds instead of silently truncating a child
        // reference (or colliding with the empty-slot sentinel).
        debug_assert!(index < u32::MAX as usize, "node pool exceeded u32 slot capacity");
        index as u32
    }

    fn as_child(self) -> usize {
        self as usize
    }
}

/// Number of bits reserved for the height in the packed `Tree` field. Six bits
/// suffice because the tallest possible tree (branching factor two) has height
/// `max_height(2)`, which is representable in six bits (checked below).
const MAX_HEIGHT_BITS: u32 = 6;
const HEIGHT_MASK: u64 = (1 << MAX_HEIGHT_BITS) - 1;

/// The packed height field must be wide enough for the tallest tree any
/// supported branching factor can produce; two is the worst case.
const _: () = assert!(
    max_height(2) < (1 << MAX_HEIGHT_BITS),
    "MAX_HEIGHT_BITS too small for the tallest representable tree"
);

/// All-ones in the 58-bit root field; used as the empty-tree sentinel.
const ROOT_EMPTY: u64 = (1u64 << (64 - MAX_HEIGHT_BITS)) - 1;

/// A handle to a single tree stored in a [`SequenceForest`].
///
/// Handles are only meaningful for the forest that produced them and are
/// invalidated by [`SequenceForest::clear`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Tree {
    /// Bits \[63:6\] hold the root node index; bits \[5:0\] hold the height.
    /// When bits \[63:6\] equal `ROOT_EMPTY` the tree is empty.
    packed: u64,
}

impl Tree {
    /// The empty tree, holding no entries.
    pub(crate) const EMPTY: Tree = Tree {
        packed: ROOT_EMPTY << MAX_HEIGHT_BITS,
    };

    /// Creates a new tree with the given root and height.
    fn new(root: usize, height: u8) -> Tree {
        debug_assert!((root as u64) < ROOT_EMPTY, "node pool exhausted");
        Tree {
            packed: ((root as u64) << MAX_HEIGHT_BITS) | (height as u64),
        }
    }

    /// Returns the root node index of this tree.
    fn root(self) -> usize {
        (self.packed >> MAX_HEIGHT_BITS) as usize
    }

    /// Returns the height of this tree.
    fn height(self) -> u8 {
        (self.packed & HEIGHT_MASK) as u8
    }

    /// Returns true if this tree has no entries.
    pub(crate) fn is_empty(self) -> bool {
        self.packed >> MAX_HEIGHT_BITS == ROOT_EMPTY
    }
}

impl Default for Tree {
    fn default() -> Tree {
        Tree::EMPTY
    }
}

/// A thread-safe forest of hash-consed sequences of `V` with branching factor `N`.
///
/// # Details
///
/// Inspired by the `cranelift_bforest` crate. The crucial difference is that
/// this implementation uses hash consing to maximally share (immutable) nodes
/// across trees. This makes it suitable to compactly represent large sets of
/// similar sequences.
///
/// Values are expected to be small `Copy` types, matching the
/// `cranelift_bforest` design tradeoffs. The branching factor `N` can be
/// overridden to experiment with node sizes.
///
/// Because a value and a child index could share a bit pattern, interning is
/// partitioned by height: nodes are only ever deduplicated against other nodes
/// at the same height, so `[5, 7]` interpreted as values never collides with
/// `[5, 7]` interpreted as child references.
///
/// Nodes are never freed individually; the forest is append-only and reclaimed
/// all at once with [`SequenceForest::clear`].
pub(crate) struct SequenceForest<V, const N: usize = DEFAULT_BRANCHING, S = FxBuildHasher>
where
    V: Copy,
{
    /// The shared node pool. Each push hands out the index a node is stored at,
    /// which serves as its interned identity. Indices are unique but not dense.
    nodes: ConcurrentAppendVec<[V; N]>,
    /// One interning index per height, mapping a node's content to its index in
    /// `nodes`. Avoids duplicating the `nodes` information by passing the hash
    /// and equality functions as closures.
    tables: Vec<ShardedHashMap<usize, S>>,
}

/// Reusable scratch buffers so repeated inserts on a hot path do not
/// reallocate.
///
/// The buffers only hold node indices, so a single context works for any
/// [`SequenceForest`] regardless of its value type or branching factor.
#[derive(Debug, Default, Clone)]
pub(crate) struct SequenceForestContext {
    /// Child indices of the level just built.
    src: Vec<usize>,
    /// Child indices of the level currently being built.
    dst: Vec<usize>,
}

impl SequenceForestContext {
    /// Creates an empty context.
    pub(crate) fn new() -> SequenceForestContext {
        SequenceForestContext::default()
    }
}

impl<V, const N: usize, S> SequenceForest<V, N, S>
where
    V: Slot,
    S: Default,
{
    /// Creates a new empty forest.
    ///
    /// `N` must be at least two; a smaller branching factor fails to compile.
    pub(crate) fn new() -> SequenceForest<V, N, S> {
        const { assert!(N >= 2, "branching factor must be at least two") };
        // One interning table per representable height. The height field is
        // `MAX_HEIGHT_BITS` wide, so heights range over `0..(1 << MAX_HEIGHT_BITS)`.
        let tables = (0..(1usize << MAX_HEIGHT_BITS))
            .map(|_| ShardedHashMap::with_shards_and_hasher(FOREST_SHARDS, S::default()))
            .collect();
        SequenceForest {
            nodes: ConcurrentAppendVec::new(),
            tables,
        }
    }
}

impl<V, const N: usize, S> SequenceForest<V, N, S>
where
    V: Slot,
    S: BuildHasher,
{
    /// Interns the sequence `values` and returns a handle to its tree, using a
    /// throwaway [`SequenceForestContext`]. Prefer [`SequenceForest::insert_with`] on
    /// hot paths to reuse the scratch buffers.
    #[allow(dead_code)]
    pub(crate) fn insert(&self, values: &[V]) -> Tree {
        self.insert_with(values, &mut SequenceForestContext::new())
    }

    /// Interns the sequence `values` and returns a handle to its tree, reusing
    /// any already-interned nodes. Equal sequences always yield the same
    /// handle.
    ///
    /// # Details
    ///
    /// The tree is assembled bottom-up: contiguous chunks of `values` become
    /// leaf-level nodes, then those are grouped into interior nodes, and so on
    /// until a single root remains.
    pub(crate) fn insert_with(&self, values: &[V], context: &mut SequenceForestContext) -> Tree {
        if values.is_empty() {
            return Tree::EMPTY;
        }

        // `src` is the level just built, `dst` the one being built. The buffers
        // live in `context` so repeated inserts do not reallocate, but they are
        // not shared between threads.
        let SequenceForestContext { src, dst } = context;

        // Each chunk of `values` is copied straight into a node, padded with
        // `EMPTY`.
        src.clear();
        for chunk in values.chunks(N) {
            let mut slots = [V::EMPTY; N];
            for (offset, &value) in chunk.iter().enumerate() {
                debug_assert!(value != V::EMPTY, "value collides with the empty-slot sentinel");
                slots[offset] = value;
            }
            src.push(self.intern(0, slots));
        }

        // Group the previous level into nodes of child indices until one node
        // remains.
        let mut height = 0u8;
        while src.len() > 1 {
            height += 1;
            dst.clear();
            for chunk in src.chunks(N) {
                let mut slots = [V::EMPTY; N];
                for (offset, &child) in chunk.iter().enumerate() {
                    slots[offset] = V::from_child(child);
                }
                dst.push(self.intern(height, slots));
            }
            mem::swap(src, dst);
        }

        Tree::new(src[0], height)
    }

    /// Interns `data` at `height`, returning the index of the existing identical
    /// node if one is present, or of a freshly allocated node otherwise.
    fn intern(&self, height: u8, data: [V; N]) -> usize {
        debug_assert!(
            (height as usize) <= max_height(N),
            "tree height exceeds max_height for this branching factor"
        );
        // The table is specific to this height, so entries are compared and
        // hashed purely by their node content.
        let table = &self.tables[height as usize];
        let hash = table.hash(&data);
        let eq = |&index: &usize| self.node(index) == data;

        // Fast path: an existing node is found while only read-locking the shard.
        if let Some(index) = table.find(hash, eq) {
            return index;
        }

        // `push` appends the node and returns the index it landed at. The slot is
        // published before `push` returns, so any thread that later reads the
        // index from `tables` can resolve it in `nodes`. `make` runs only on a
        // miss and while the shard is write-locked, so the index is unique.
        table
            .find_or_insert_with(
                hash,
                eq,
                |&index| table.hash(&self.node(index)),
                || {
                    let index = self.nodes.push(data);
                    debug_assert!((index as u64) < ROOT_EMPTY, "node pool exhausted");
                    index
                },
            )
            .0
    }

    /// Returns a copy of the interned node at `index`, which must exist.
    fn node(&self, index: usize) -> [V; N] {
        // SAFETY: `index` came from `tables`, which only holds indices returned
        // by `push`; the interning shard lock published that write before this
        // read, so `get_unchecked` is sound.
        *unsafe { self.nodes.get_unchecked(index) }
    }
}

/// Returns the maximum useful tree depth for a branching factor of `n`.
///
/// A tree of height `h` can hold at most `n^(h+1)` leaf values, so once
/// `h+1 > usize::BITS / floor(log2(n))` the tree would need more nodes than
/// a `usize` can index. This ceiling is therefore a tight upper bound on any
/// reachable depth.
pub(crate) const fn max_depth(n: usize) -> usize {
    // floor(log2(n))
    let log2_n = (usize::BITS - n.leading_zeros() - 1) as usize;
    // ceil(usize::BITS / log2_n)
    (usize::BITS as usize).div_ceil(log2_n)
}

/// Returns the maximum height any tree with branching factor `n` can reach.
///
/// Leaves sit at height zero, so a tree spanning `max_depth(n)` levels tops out
/// at one less than that. This is the largest value the packed height field of
/// a [`Tree`] ever has to hold.
pub(crate) const fn max_height(n: usize) -> usize {
    max_depth(n) - 1
}

/// Upper bound on the height of any tree.
const MAX_DEPTH: usize = max_depth(2);

impl<V, const N: usize, S> SequenceForest<V, N, S>
where
    V: Slot,
{
    /// Returns an iterator over the values of `tree` in sequence order.
    pub(crate) fn iter(&self, tree: Tree) -> Iter<'_, V, N> {
        let mut stack = [([V::EMPTY; N], 0u32, 0u8); MAX_DEPTH];
        let mut depth = 0;
        if !tree.is_empty() {
            // SAFETY: a `Tree` is only produced by interning its nodes, so the
            // root index was returned by `push` and is published to any thread
            // that holds the handle.
            let root = *unsafe { self.nodes.get_unchecked(tree.root()) };
            stack[0] = (root, 0, tree.height());
            depth = 1;
        }
        Iter {
            nodes: &self.nodes,
            stack,
            depth,
        }
    }
}

impl<V, const N: usize, S> SequenceForest<V, N, S>
where
    V: Slot,
{
    /// Returns the number of distinct nodes currently held by the forest. This
    /// is the deduplicated node count, not the total number of entries across
    /// all trees.
    #[allow(dead_code)]
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Removes every tree from the forest, invalidating all outstanding
    /// [`Tree`] handles.
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.nodes.clear();
        for table in &self.tables {
            table.clear();
        }
    }
}

impl<V, const N: usize, S> Default for SequenceForest<V, N, S>
where
    V: Slot,
    S: Default,
{
    fn default() -> SequenceForest<V, N, S> {
        SequenceForest::new()
    }
}

/// In-order iterator over the values of a single tree.
///
/// The descent stack is a fixed-size array, so no heap allocation is performed,
/// which keeps reconstruction cheap on hot paths. Each frame caches a copy of
/// its node's slot array, so a node is fetched from the shared pool at most once
/// per descent rather than on every value.
pub(crate) struct Iter<'a, V, const N: usize>
where
    V: Copy,
{
    nodes: &'a ConcurrentAppendVec<[V; N]>,
    /// Frames on the path from the root, each holding the node's slot array, the
    /// index of the next slot to visit in it, and the node's height.
    stack: [([V; N], u32, u8); MAX_DEPTH],
    depth: usize,
}

impl<V, const N: usize> Iterator for Iter<'_, V, N>
where
    V: Slot,
{
    type Item = V;

    fn next(&mut self) -> Option<V> {
        while self.depth > 0 {
            let top = self.depth - 1;
            let slot = self.stack[top].1 as usize;
            let height = self.stack[top].2;

            // A slot past the end or holding the empty sentinel exhausts this
            // node; pop back to its parent.
            if slot >= N || self.stack[top].0[slot] == V::EMPTY {
                self.depth -= 1;
                continue;
            }

            let value = self.stack[top].0[slot];
            self.stack[top].1 += 1;

            if height == 0 {
                // Height zero means the slot holds a leaf value.
                return Some(value);
            }

            // Otherwise it is a child reference; descend into it.
            assert!(self.depth < max_depth(N), "tree deeper than max_depth({N})");
            // SAFETY: child indices are stored in interned nodes, so they were
            // returned by `push` and published when the tree was built.
            let child = *unsafe { self.nodes.get_unchecked(value.as_child()) };
            self.stack[self.depth] = (child, 0, height - 1);
            self.depth += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    use rand::RngExt;

    use merc_utilities::random_test;
    use merc_utilities::random_test_threads;

    use super::SequenceForest;
    use super::SequenceForestContext;
    use super::Tree;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_insert_iter_dedup() {
        random_test(100, |rng| {
            // Interns random sequences and checks every forest invariant against a
            // HashMap implementation.
            let forest: SequenceForest<usize, 2> = SequenceForest::new();
            let mut context = SequenceForestContext::new();
            let mut seen: HashMap<Vec<usize>, Tree> = HashMap::new();

            for _ in 0..200 {
                let len = rng.random_range(0..20);
                let seq: Vec<usize> = (0..len).map(|_| rng.random_range(0..10)).collect();

                let tree = forest.insert_with(&seq, &mut context);
                assert_eq!(forest.insert(&seq), tree, "context path matches the allocating path");

                let got: Vec<usize> = forest.iter(tree).collect();
                assert_eq!(got, seq);
                assert_eq!(
                    seq.is_empty(),
                    tree.is_empty(),
                    "only the empty sequence is the empty tree"
                );

                match seen.get(&seq) {
                    Some(&prev) => {
                        let before = forest.node_count();
                        assert_eq!(forest.insert(&seq), prev, "equal sequences share a handle");
                        assert_eq!(forest.node_count(), before, "re-interning must not allocate nodes");
                    }
                    None => {
                        seen.insert(seq, tree);
                    }
                }
            }
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_concurrent_intern() {
        // Interns random sequences concurrently.
        let forest: Arc<SequenceForest<usize, 4>> = Arc::new(SequenceForest::new());
        let seen: Arc<Mutex<HashMap<Vec<usize>, Tree>>> = Arc::new(Mutex::new(HashMap::new()));

        random_test_threads(
            200,
            8,
            || (Arc::clone(&forest), Arc::clone(&seen)),
            move |rng, (forest, seen)| {
                let len = rng.random_range(0..20);
                let seq: Vec<usize> = (0..len).map(|_| rng.random_range(0..30)).collect();

                let tree = forest.insert(&seq);
                let got: Vec<usize> = forest.iter(tree).collect();
                assert_eq!(got, seq);

                let mut seen = seen.lock().unwrap();
                match seen.get(&seq) {
                    Some(&prev) => assert_eq!(prev, tree, "equal sequences interned to different trees"),
                    None => {
                        seen.insert(seq, tree);
                    }
                }
            },
        );
    }
}
