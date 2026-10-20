use std::hash::BuildHasher;
use std::hash::Hash;

use rustc_hash::FxBuildHasher;

use crate::ConcurrentAppendVec;
use crate::ShardedHashMap;

/// An append-only set mapping each distinct value of type `T` to a stable
/// `usize` index.
///
/// The set is thread-safe: values can be inserted and looked up concurrently
/// through `&self` from multiple threads. To keep concurrent insertion
/// contention-free the indices are unique and stable but **not dense**, so walk
/// [`ConcurrentIndexedSet::iter`] rather than `0..len`. There is no removal; an
/// index stays valid for the lifetime of the set (until
/// [`ConcurrentIndexedSet::clear`], which requires `&mut self`).
pub struct ConcurrentIndexedSet<T, S = FxBuildHasher> {
    /// Stores each distinct value at the index handed out for it. Indices are
    /// stable but may be sparse.
    values: ConcurrentAppendVec<T>,
    /// Hash index from a value's hash to its index in `values`. Stores only the
    /// index; the hash and equality closures dereference `values`, so the
    /// payload is not duplicated.
    table: ShardedHashMap<usize, S>,
}

impl<T, S: Default> ConcurrentIndexedSet<T, S> {
    /// Creates a new empty set with the default hasher.
    pub fn new() -> ConcurrentIndexedSet<T, S> {
        ConcurrentIndexedSet {
            values: ConcurrentAppendVec::new(),
            table: ShardedHashMap::with_hasher(S::default()),
        }
    }

    /// Creates a new empty set whose hash index has room for at least
    /// `capacity` values before reallocating.
    pub fn with_capacity(capacity: usize) -> ConcurrentIndexedSet<T, S> {
        ConcurrentIndexedSet {
            values: ConcurrentAppendVec::new(),
            table: ShardedHashMap::with_capacity_and_hasher(capacity, S::default()),
        }
    }
}

impl<T, S> ConcurrentIndexedSet<T, S>
where
    T: Hash + Eq + Clone,
    S: BuildHasher,
{
    /// Inserts `value` and returns its index together with a boolean that is
    /// true when the value was newly inserted and false when it was already
    /// present.
    pub fn insert(&self, value: T) -> (usize, bool) {
        let hash = self.table.hash(&value);
        // SAFETY (this and the hash closure below): the table only ever holds
        // indices returned by `push`, and every read here runs under the shard
        // lock that ordered that push's publish, so the slot is initialized and
        // visible — `get_unchecked` is sound.
        let eq = |&index: &usize| unsafe { self.values.get_unchecked(index) } == &value;

        // Fast path: the value is already present, so no write lock is needed.
        if let Some(index) = self.table.find(hash, eq) {
            return (index, false);
        }

        // The vacant branch runs while the shard for `hash` is write-locked, so
        // two threads inserting the same new value cannot both push and hand
        // out duplicate indices. `push` publishes the slot before returning, so
        // recording its index in the table afterwards guarantees a returned
        // index always resolves in `get_by_index`.
        self.table.find_or_insert_with(
            hash,
            eq,
            |&index| self.table.hash(unsafe { self.values.get_unchecked(index) }),
            || self.values.push(value.clone()),
        )
    }

    /// Returns the index of `value` if it is present, or `None` otherwise.
    pub fn index(&self, value: &T) -> Option<usize> {
        let hash = self.table.hash(value);
        // SAFETY: indices in the table were returned by `push` and published
        // under the shard lock this read takes, so `get_unchecked` is sound.
        self.table
            .find(hash, |&index| unsafe { self.values.get_unchecked(index) } == value)
    }

    /// Returns true if `value` is present in the set.
    pub fn contains(&self, value: &T) -> bool {
        self.index(value).is_some()
    }
}

impl<T, S> ConcurrentIndexedSet<T, S> {
    /// Returns a reference to the value at `index`, or `None` if the index was
    /// never handed out (out of range or a sparse gap).
    pub fn get_by_index(&self, index: usize) -> Option<&T> {
        self.values.get(index)
    }

    /// Returns an iterator over the resident values. Order is unspecified, and
    /// indices are not exposed because they may be sparse.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.values.iter()
    }

    /// Returns the number of distinct values in the set.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Removes all values, invalidating every previously returned index.
    pub fn clear(&mut self) {
        self.values.clear();
        self.table.clear();
    }
}

impl<T, S: Default> Default for ConcurrentIndexedSet<T, S> {
    fn default() -> ConcurrentIndexedSet<T, S> {
        ConcurrentIndexedSet::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use rand::RngExt;

    use merc_utilities::random_test;
    use merc_utilities::random_test_threads;

    use super::ConcurrentIndexedSet;

    /// Inserts random values from a small space and checks the set against a
    /// `HashMap<value, index>` oracle after every step: the fresh-insert flag,
    /// the index assigned in insertion order (dense on a single thread), and
    /// that every read path (`index`, `contains`, `get_by_index`) round-trips
    /// against the model.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_insert_dedup_and_roundtrip() {
        random_test(50, |rng| {
            let set: ConcurrentIndexedSet<u64> = ConcurrentIndexedSet::new();
            let mut model: HashMap<u64, usize> = HashMap::new();

            for _ in 0..500 {
                let value = rng.random_range(0..32u64);
                let next = model.len();
                let expected_new = !model.contains_key(&value);

                let (index, is_new) = set.insert(value);
                assert_eq!(is_new, expected_new, "fresh-insert flag agrees with the model");
                assert_eq!(
                    index,
                    *model.entry(value).or_insert(next),
                    "equal values share the index assigned in insertion order"
                );

                assert_eq!(set.get_by_index(index), Some(&value), "the index resolves to the value");
                assert_eq!(set.index(&value), Some(index), "lookup is the inverse of get_by_index");
                assert!(set.contains(&value));
            }

            assert_eq!(set.len(), model.len());

            // Values from outside the inserted space are absent.
            assert_eq!(set.index(&64), None);
            assert!(!set.contains(&64));
            assert_eq!(set.get_by_index(model.len()), None);
        });
    }

    /// Concurrently inserts random values from a fixed value space across
    /// several threads. Across all threads each value yields exactly one fresh
    /// insertion, so the count of fresh insertions must equal the set size, and
    /// every returned index resolves to the value that was inserted even under
    /// contention.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_concurrent_insert_counts_each_once() {
        let set: Arc<ConcurrentIndexedSet<u64>> = Arc::new(ConcurrentIndexedSet::new());
        let inserted = Arc::new(AtomicUsize::new(0));
        let values = 256u64;

        random_test_threads(
            2000,
            8,
            || (Arc::clone(&set), Arc::clone(&inserted)),
            move |rng, (set, inserted)| {
                let value = rng.random_range(0..values);
                let (index, is_new) = set.insert(value);
                assert_eq!(
                    set.get_by_index(index),
                    Some(&value),
                    "the index resolves under contention"
                );
                if is_new {
                    inserted.fetch_add(1, Ordering::Relaxed);
                }
            },
        );

        assert_eq!(
            inserted.load(Ordering::Relaxed),
            set.len(),
            "every fresh insertion is a distinct resident value"
        );

        // Indices may be sparse, so iterate the resident values rather than
        // `0..len`. Each must be in range and round-trip through `index` and
        // `get_by_index`.
        let mut residents = HashSet::new();
        for &value in set.iter() {
            assert!(value < values, "only inserted values are resident");
            assert!(residents.insert(value), "each value is resident once");
            let index = set.index(&value).expect("a resident value has an index");
            assert_eq!(set.get_by_index(index), Some(&value), "index is the inverse of lookup");
        }
        assert_eq!(residents.len(), set.len(), "iter visits every resident value once");
    }
}
