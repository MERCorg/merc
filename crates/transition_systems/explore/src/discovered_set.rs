use merc_unsafety::ConcurrentIndexedSet;

use crate::SequenceForest;
use crate::SequenceForestContext;
use crate::Slot;
use crate::Tree;

/// A stable handle to a state stored in a [`DiscoveredSet`].
///
/// Handles are stable and unique per state, but the underlying indices may be
/// sparse rather than a contiguous `0..n`.
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct StateRef(usize);

impl StateRef {
    /// Returns the underlying index as a `usize`.
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// A set of `T` state vectors that deduplicates equal vectors and assigns each
/// a stable [`StateRef`].
///
/// The set is thread-safe: states can be inserted and looked up concurrently
/// through `&self` from multiple threads.
pub(crate) struct DiscoveredSet<T = usize>
where
    T: Copy,
{
    /// Hash-consed forest backing every stored state sequence.
    forest: SequenceForest<T, 2>,
    /// Deduplicates canonical roots and assigns each a stable index into the
    /// forest.
    states: ConcurrentIndexedSet<Tree>,
}

impl<T> DiscoveredSet<T>
where
    T: Slot,
{
    /// Creates a new empty discovered set.
    pub(crate) fn new() -> DiscoveredSet<T> {
        DiscoveredSet {
            forest: SequenceForest::new(),
            states: ConcurrentIndexedSet::new(),
        }
    }

    /// Creates a new empty discovered set with room for at least `capacity`
    /// states before reallocating.
    #[allow(dead_code)]
    pub(crate) fn with_capacity(capacity: usize) -> DiscoveredSet<T> {
        DiscoveredSet {
            forest: SequenceForest::new(),
            states: ConcurrentIndexedSet::with_capacity(capacity),
        }
    }

    /// Inserts `state` and returns its handle together with a boolean that is
    /// true when the state was newly inserted and false when it was already
    /// present.
    pub(crate) fn insert(&self, state: &[T]) -> (StateRef, bool) {
        self.insert_with(state, &mut SequenceForestContext::new())
    }

    /// Inserts `state` like [`DiscoveredSet::insert`], reusing the scratch
    /// buffers in `context`.
    pub(crate) fn insert_with(&self, state: &[T], context: &mut SequenceForestContext) -> (StateRef, bool) {
        let root = self.forest.insert_with(state, context);
        let (index, is_new) = self.states.insert(root);
        (StateRef(index), is_new)
    }

    /// Returns the handle of `state` if it is present, or `None` otherwise.
    ///
    /// Note this is *not* a read-only operation. To obtain the canonical handle
    /// to compare against, `state` is interned into the backing forest, which
    /// is append-only. As a result, failures also grow the forest.
    #[allow(dead_code)]
    pub(crate) fn index(&self, state: &[T]) -> Option<StateRef> {
        self.index_with(state, &mut SequenceForestContext::new())
    }

    /// Looks up `state` like [`DiscoveredSet::index`], reusing the scratch buffers in `context`.
    #[allow(dead_code)]
    pub(crate) fn index_with(&self, state: &[T], context: &mut SequenceForestContext) -> Option<StateRef> {
        let root = self.forest.insert_with(state, context);
        self.states.index(&root).map(StateRef)
    }

    /// Returns true if `state` is present in the set. Like
    /// [`DiscoveredSet::index`], this interns `state` into the forest.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, state: &[T]) -> bool {
        self.index(state).is_some()
    }

    /// Reconstructs the state vector for `reference` into the freshly cleared
    /// `out` buffer, returning whether `reference` was present.
    #[allow(dead_code)]
    pub(crate) fn get_into(&self, reference: StateRef, out: &mut Vec<T>) -> bool {
        out.clear();
        let Some(&tree) = self.states.get_by_index(reference.index()) else {
            return false;
        };
        out.extend(self.forest.iter(tree));
        true
    }

    /// Returns the state vector for `reference`, allocating a fresh [`Vec`], or
    /// `None` if `reference` is not present.
    ///
    /// Prefer [`DiscoveredSet::get_into`] on hot paths to reuse a buffer.
    #[allow(dead_code)]
    pub(crate) fn get(&self, reference: StateRef) -> Option<Vec<T>> {
        let &tree = self.states.get_by_index(reference.index())?;
        Some(self.forest.iter(tree).collect())
    }

    /// Removes all states, invalidating every previously returned
    /// [`StateRef`].
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.forest.clear();
        self.states.clear();
    }
}

impl<T> DiscoveredSet<T>
where
    T: Copy,
{
    /// Returns the number of distinct states in the set.
    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    /// Returns true if the set is empty.
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

impl<T> Default for DiscoveredSet<T>
where
    T: Slot,
{
    fn default() -> DiscoveredSet<T> {
        DiscoveredSet::new()
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

    use super::DiscoveredSet;
    use super::StateRef;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_insert_dedup() {
        random_test(100, |rng| {
            // Inserts random states and checks deduplication, handle stability and
            // reconstruction against a HashMap reference.
            let set: DiscoveredSet<usize> = DiscoveredSet::new();
            let mut seen: HashMap<Vec<usize>, StateRef> = HashMap::new();
            let mut out = Vec::new();

            for _ in 0..200 {
                let len = rng.random_range(0..20);
                let state: Vec<usize> = (0..len).map(|_| rng.random_range(0..10)).collect();

                let (reference, is_new) = set.insert(&state);
                assert_eq!(is_new, !seen.contains_key(&state), "states are new exactly once");
                seen.entry(state.clone()).or_insert(reference);

                assert_eq!(seen[&state], reference, "equal states share a handle");
                assert!(reference.index() < seen.len(), "indices stay dense");
                assert_eq!(set.len(), seen.len());

                assert!(set.get_into(reference, &mut out));
                assert_eq!(out, state);
            }
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_concurrent_insert() {
        // Inserts random states concurrently and checks that equal states share a
        // handle across threads.
        let set: Arc<DiscoveredSet<usize>> = Arc::new(DiscoveredSet::new());
        let seen: Arc<Mutex<HashMap<Vec<usize>, StateRef>>> = Arc::new(Mutex::new(HashMap::new()));

        random_test_threads(
            200,
            8,
            || (Arc::clone(&set), Arc::clone(&seen)),
            move |rng, (set, seen)| {
                let len = rng.random_range(0..20);
                let state: Vec<usize> = (0..len).map(|_| rng.random_range(0..30)).collect();

                let (reference, _) = set.insert(&state);
                let mut out = Vec::new();
                assert!(set.get_into(reference, &mut out));
                assert_eq!(out, state);

                let mut seen = seen.lock().unwrap();
                match seen.get(&state) {
                    Some(&prev) => assert_eq!(prev, reference, "equal states interned to different handles"),
                    None => {
                        seen.insert(state, reference);
                    }
                }
            },
        );
    }
}
