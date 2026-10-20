use std::hash::BuildHasher;
use std::hash::Hash;
use std::hash::RandomState;
use std::thread::available_parallelism;

use crossbeam_utils::CachePadded;
use equivalent::Equivalent;
use hashbrown::HashTable;
use hashbrown::hash_table::Entry;
use parking_lot::RwLock;

/// Caps the default shard count so machines with many cores do not allocate an
/// unreasonable number of locks.
const MAX_DEFAULT_SHARDS: usize = 256;

/// Number of high hash bits `hashbrown` reserves for its control byte; the shard
/// is selected from the bits immediately below them.
const CONTROL_BITS: u32 = 7;

/// A concurrent hash table storing bare elements of type `T`, looked up by a
/// caller-supplied hash and equality closure rather than a key type it owns —
/// so `T` can be a compact handle (for example an index) whose hash and
/// identity are derived from data stored elsewhere, without duplicating the
/// interned payload.
///
/// Elements are striped across a fixed, power-of-two number of shards, each
/// guarded by its own [`parking_lot::RwLock`]. The shard is selected from the
/// hash bits just below the top seven that `hashbrown` reserves for its
/// control byte, so shard selection does not degrade in-shard probing.
///
/// The raw API ([`find`](Self::find), [`find_or_insert_with`](Self::find_or_insert_with)
/// and [`remove`](Self::remove)) takes the hash, equality and (for insertion)
/// rehashing closures explicitly. The convenience API ([`insert`](Self::insert),
/// [`get`](Self::get), [`contains`](Self::contains), [`remove_equiv`](Self::remove_equiv))
/// hashes through the table's own [`BuildHasher`] for elements that describe
/// themselves via `Hash`/`Eq`/[`Equivalent`].
pub struct ShardedHashMap<T, S = RandomState> {
    /// Element shards, selected by a slice of the hash. Always a power of two in
    /// length so the shard index is a cheap bit extraction.
    shards: Box<[CachePadded<RwLock<HashTable<T>>>]>,
    /// Builds the hashes the convenience methods use; exposed via
    /// [`ShardedHashMap::hash`] so raw callers hash consistently with the table.
    hasher: S,
    /// `64 - log2(shards.len())`; shifts the hash so the shard index ends up in
    /// the low bits. Unused when there is a single shard.
    shard_shift: u32,
}

/// Returns a sensible default shard count for the running machine.
fn default_shard_count() -> usize {
    let parallelism = available_parallelism().map(|n| n.get()).unwrap_or(1);
    (parallelism * 4).clamp(1, MAX_DEFAULT_SHARDS).next_power_of_two()
}

impl<T, S: Default> ShardedHashMap<T, S> {
    /// Creates a table with a machine-dependent number of shards.
    pub fn new() -> ShardedHashMap<T, S> {
        ShardedHashMap::with_hasher(S::default())
    }

    /// Creates a table with at least `shards` shards (rounded up to a power of
    /// two). Fewer shards reduce memory; more reduce write contention.
    pub fn with_shards(shards: usize) -> ShardedHashMap<T, S> {
        ShardedHashMap::with_shards_and_hasher(shards, S::default())
    }
}

impl<T, S: Default> Default for ShardedHashMap<T, S> {
    fn default() -> ShardedHashMap<T, S> {
        ShardedHashMap::with_hasher(S::default())
    }
}

impl<T, S> ShardedHashMap<T, S> {
    /// Creates a table with a machine-dependent number of shards and the given
    /// hash builder.
    pub fn with_hasher(hasher: S) -> ShardedHashMap<T, S> {
        ShardedHashMap::build(default_shard_count(), 0, hasher)
    }

    /// Creates a table with at least `shards` shards (rounded up to a power of
    /// two) and the given hash builder.
    pub fn with_shards_and_hasher(shards: usize, hasher: S) -> ShardedHashMap<T, S> {
        ShardedHashMap::build(shards, 0, hasher)
    }

    /// Creates a table sized to hold at least `capacity` elements before
    /// reallocating, with the given hash builder.
    pub fn with_capacity_and_hasher(capacity: usize, hasher: S) -> ShardedHashMap<T, S> {
        let shards = default_shard_count();
        ShardedHashMap::build(shards, capacity.div_ceil(shards), hasher)
    }

    /// Builds a table with `shards` (rounded up to a power of two) shards, each
    /// pre-sized for `per_shard_capacity` elements.
    fn build(shards: usize, per_shard_capacity: usize, hasher: S) -> ShardedHashMap<T, S> {
        let shards = shards.max(1).next_power_of_two();
        let shard_shift = u64::BITS - shards.trailing_zeros();
        let shards = (0..shards)
            .map(|_| CachePadded::new(RwLock::new(HashTable::with_capacity(per_shard_capacity))))
            .collect();
        ShardedHashMap {
            shards,
            hasher,
            shard_shift,
        }
    }

    /// Returns the shard a hash maps to.
    fn shard(&self, hash: u64) -> &RwLock<HashTable<T>> {
        // With a single shard the shift would be 64 (undefined for `>>`), so
        // short-circuit. Otherwise drop the top `CONTROL_BITS` (used by
        // `hashbrown`'s control byte) and take the next `log2(shards)` bits.
        let index = if self.shards.len() == 1 {
            0
        } else {
            ((hash << CONTROL_BITS) >> self.shard_shift) as usize
        };
        &self.shards[index]
    }

    /// Returns the number of elements across all shards.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|shard| shard.read().len()).sum()
    }

    /// Returns true if every shard is empty.
    pub fn is_empty(&self) -> bool {
        self.shards.iter().all(|shard| shard.read().is_empty())
    }

    /// Returns the total capacity across all shards.
    pub fn capacity(&self) -> usize {
        self.shards.iter().map(|shard| shard.read().capacity()).sum()
    }

    /// Removes all elements, keeping the allocated shard capacity.
    pub fn clear(&self) {
        for shard in &self.shards {
            shard.write().clear();
        }
    }

    /// Retains only the elements for which `keep` returns true.
    pub fn retain(&self, mut keep: impl FnMut(&T) -> bool) {
        for shard in &self.shards {
            shard.write().retain(|element| keep(element));
        }
    }

    /// Calls `f` on a reference to every element. The shard holding each element
    /// is read-locked while `f` runs against it, so `f` must not call back into
    /// the same table.
    pub fn for_each(&self, mut f: impl FnMut(&T)) {
        for shard in &self.shards {
            let table = shard.read();
            for element in table.iter() {
                f(element);
            }
        }
    }

    /// Returns a clone of the element matching `hash`/`eq`, if present.
    pub fn find(&self, hash: u64, mut eq: impl FnMut(&T) -> bool) -> Option<T>
    where
        T: Clone,
    {
        self.shard(hash).read().find(hash, |element| eq(element)).cloned()
    }

    /// Looks up the element matching `hash`/`eq` and, if present, calls `f` on a
    /// shared reference to it, returning its result; returns `None` otherwise.
    ///
    /// Unlike [`ShardedHashMap::find`], the stored element is not required to be
    /// [`Clone`] and is never copied: `f` runs while the shard read lock is
    /// held. Consequently `f` must not call back into the same table (it would
    /// deadlock) and should be kept short to limit the lock hold time.
    pub fn find_with<R>(&self, hash: u64, mut eq: impl FnMut(&T) -> bool, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.shard(hash).read().find(hash, |element| eq(element)).map(f)
    }

    /// Returns a clone of the element matching `hash`/`eq`, inserting the value
    /// produced by `make` if none is present. `make` runs only on insertion and
    /// while the shard is write-locked; `hash_of` recomputes an element's hash
    /// when the shard grows. Returns the resident element and whether it was
    /// freshly inserted.
    pub fn find_or_insert_with(
        &self,
        hash: u64,
        mut eq: impl FnMut(&T) -> bool,
        hash_of: impl Fn(&T) -> u64,
        make: impl FnOnce() -> T,
    ) -> (T, bool)
    where
        T: Clone,
    {
        let mut table = self.shard(hash).write();
        match table.entry(hash, |element| eq(element), &hash_of) {
            Entry::Occupied(entry) => (entry.get().clone(), false),
            Entry::Vacant(entry) => (entry.insert(make()).get().clone(), true),
        }
    }

    /// Inserts the element produced by `make` unless an element matching
    /// `hash`/`eq` is already present, returning whether a new element was
    /// inserted. `make` runs only on insertion and while the shard is
    /// write-locked; `hash_of` recomputes an element's hash when the shard
    /// grows.
    ///
    /// Unlike [`ShardedHashMap::find_or_insert_with`], the resident element is
    /// neither returned nor cloned, so `T` need not be [`Clone`]. Use this when
    /// only publication matters and the stored element is expensive to clone.
    pub fn insert_with(
        &self,
        hash: u64,
        mut eq: impl FnMut(&T) -> bool,
        hash_of: impl Fn(&T) -> u64,
        make: impl FnOnce() -> T,
    ) -> bool {
        let mut table = self.shard(hash).write();
        match table.entry(hash, |element| eq(element), &hash_of) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(make());
                true
            }
        }
    }

    /// Removes and returns the element matching `hash`/`eq`, if present.
    pub fn remove(&self, hash: u64, mut eq: impl FnMut(&T) -> bool) -> Option<T> {
        match self.shard(hash).write().find_entry(hash, |element| eq(element)) {
            Ok(occupied) => Some(occupied.remove().0),
            Err(_) => None,
        }
    }
}

impl<T, S> ShardedHashMap<T, S>
where
    S: BuildHasher,
{
    /// Hashes `value` with the table's hash builder. Raw callers use this so the
    /// hashes they pass to [`ShardedHashMap::find`] and friends are consistent
    /// with the table's convenience methods.
    pub fn hash<Q>(&self, value: &Q) -> u64
    where
        Q: ?Sized + Hash,
    {
        self.hasher.hash_one(value)
    }

    /// Inserts `value` unless an equal element is already present. Returns the
    /// resident element and whether it was freshly inserted.
    pub fn insert(&self, value: T) -> (T, bool)
    where
        T: Hash + Eq + Clone,
    {
        let hash = self.hash(&value);
        let mut table = self.shard(hash).write();
        match table.entry(
            hash,
            |element| element == &value,
            |element| self.hasher.hash_one(element),
        ) {
            Entry::Occupied(entry) => (entry.get().clone(), false),
            Entry::Vacant(entry) => (entry.insert(value).get().clone(), true),
        }
    }

    /// Returns a clone of the element equivalent to `value`, if present.
    pub fn get<Q>(&self, value: &Q) -> Option<T>
    where
        T: Clone,
        Q: ?Sized + Hash + Equivalent<T>,
    {
        let hash = self.hash(value);
        self.shard(hash)
            .read()
            .find(hash, |element| value.equivalent(element))
            .cloned()
    }

    /// Returns true if an element equivalent to `value` is present.
    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        Q: ?Sized + Hash + Equivalent<T>,
    {
        let hash = self.hash(value);
        self.shard(hash)
            .read()
            .find(hash, |element| value.equivalent(element))
            .is_some()
    }

    /// Removes and returns the element equivalent to `value`, if present.
    pub fn remove_equiv<Q>(&self, value: &Q) -> Option<T>
    where
        Q: ?Sized + Hash + Equivalent<T>,
    {
        let hash = self.hash(value);
        match self
            .shard(hash)
            .write()
            .find_entry(hash, |element| value.equivalent(element))
        {
            Ok(occupied) => Some(occupied.remove().0),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use rand::RngExt;

    use merc_utilities::random_test;
    use merc_utilities::random_test_threads;

    use super::ShardedHashMap;

    /// Drives the convenience API with random operations against a `HashSet`
    /// oracle, checking that the table agrees with the model after each step.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_insert_get_remove() {
        random_test(100, |rng| {
            let map: ShardedHashMap<u64> = ShardedHashMap::with_shards(rng.random_range(1..=16));
            let mut model: HashSet<u64> = HashSet::new();

            for _ in 0..1000 {
                let value = rng.random_range(0..64u64);
                match rng.random_range(0..4) {
                    0 => assert_eq!(map.insert(value).1, model.insert(value), "fresh-insert flags agree"),
                    1 => assert_eq!(map.contains(&value), model.contains(&value)),
                    2 => {
                        let expected = model.contains(&value).then_some(value);
                        assert_eq!(map.get(&value), expected);
                    }
                    _ => assert_eq!(map.remove_equiv(&value).is_some(), model.remove(&value)),
                }
                assert_eq!(map.len(), model.len());
            }
        });
    }

    /// Exercises the raw API the way the forest does: the table stores indices
    /// into a side vector and the equality/hash closures dereference it. Random
    /// payloads are interned and checked against a `HashMap` oracle mapping each
    /// payload to the index it should resolve to.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_raw_interning_dedup() {
        random_test(50, |rng| {
            let map: ShardedHashMap<usize> = ShardedHashMap::with_shards(rng.random_range(1..=8));
            let store: Mutex<Vec<u64>> = Mutex::new(Vec::new());

            let intern = |value: u64| -> usize {
                let hash = map.hash(&value);
                let eq = |&index: &usize| store.lock().unwrap()[index] == value;
                if let Some(index) = map.find(hash, eq) {
                    return index;
                }
                map.find_or_insert_with(
                    hash,
                    eq,
                    |&index: &usize| map.hash(&store.lock().unwrap()[index]),
                    || {
                        let mut store = store.lock().unwrap();
                        store.push(value);
                        store.len() - 1
                    },
                )
                .0
            };

            let mut model: HashMap<u64, usize> = HashMap::new();
            for _ in 0..500 {
                let value = rng.random_range(0..32u64);
                let index = intern(value);
                let next = model.len();
                assert_eq!(
                    index,
                    *model.entry(value).or_insert(next),
                    "equal payloads share an index"
                );
            }

            assert_eq!(map.len(), model.len());
            assert_eq!(
                store.lock().unwrap().len(),
                model.len(),
                "no payload pushed for a duplicate"
            );
        });
    }

    /// Concurrently inserts random values from a fixed value space. Across all
    /// threads each value yields exactly one fresh insertion, so the count of
    /// fresh insertions must equal the table size.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn random_concurrent_insert_counts_each_once() {
        let map: Arc<ShardedHashMap<u64>> = Arc::new(ShardedHashMap::with_shards(8));
        let inserted = Arc::new(AtomicUsize::new(0));
        let values = 256u64;

        random_test_threads(
            2000,
            8,
            || (Arc::clone(&map), Arc::clone(&inserted)),
            move |rng, (map, inserted)| {
                let value = rng.random_range(0..values);
                if map.insert(value).1 {
                    inserted.fetch_add(1, Ordering::Relaxed);
                }
            },
        );

        assert_eq!(
            inserted.load(Ordering::Relaxed),
            map.len(),
            "every fresh insertion is a distinct resident value"
        );
        map.for_each(|&value| assert!(value < values, "only inserted values are resident"));
    }
}
