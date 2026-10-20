use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use merc_collections::VecSet;

/// An antichain is a structure (<, S) such that < is a preorder on S and no
/// two elements of S are comparable under <; it is dual to a chain. Here,
/// values are grouped by key, and the sets stored under each key are kept
/// pairwise incomparable under the subset relation.
pub struct Antichain<K, V> {
    storage: HashMap<K, VecSet<VecSet<V>>>,

    /// The largest number of keys the antichain has held at once.
    max_antichain: usize,
    /// Number of `insert` calls that added a new pair.
    antichain_misses: usize,
    /// Total number of `insert` calls.
    antichain_inserts: usize,
}

impl<K: Eq + Hash, V: Clone + Ord> Antichain<K, V> {
    /// Creates a new empty antichain.
    pub fn new() -> Self {
        Antichain {
            storage: HashMap::new(),
            max_antichain: 0,
            antichain_misses: 0,
            antichain_inserts: 0,
        }
    }

    /// Checks whether the antichain contains a pair (s, T') such that T ⊆ T',
    /// i.e., a stored set that is a superset of `value`.
    pub fn contains_superset(&self, key: &K, value: &VecSet<V>) -> bool {
        self.storage
            .get(key)
            .is_some_and(|entry| entry.iter().any(|inner_value| value.is_subset(inner_value)))
    }

    /// Checks whether the antichain contains a pair (s, T') such that T' ⊆ T,
    /// i.e., a stored set that is a subset of `value`.
    pub fn contains_subset(&self, key: &K, value: &VecSet<V>) -> bool {
        self.storage
            .get(key)
            .is_some_and(|entry| entry.iter().any(|inner_value| inner_value.is_subset(value)))
    }

    /// Returns true iff the antichain is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Returns the number of (key, value) pairs stored, i.e., how many items
    /// `iter` yields.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.storage.values().map(|values| values.len()).sum()
    }

    /// Returns `(max_antichain_size, insert_misses, insert_calls)`: the largest
    /// number of keys held at once, the number of `insert` calls that added a
    /// new pair, and the total number of `insert` calls.
    pub fn metrics(&self) -> (usize, usize, usize) {
        (self.max_antichain, self.antichain_misses, self.antichain_inserts)
    }

    /// Returns an iterator over the pairs in the antichain.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &VecSet<V>)> {
        self.storage
            .iter()
            .flat_map(|(key, values)| values.iter().map(move |value| (key, value)))
    }
}

impl<K: Eq + Hash, V: Clone + Ord> Default for Antichain<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V: fmt::Debug + Ord> Antichain<K, V> {
    /// Checks the internal consistency of the antichain invariant.
    #[cfg(test)]
    fn check_consistency(&self) {
        for values in self.storage.values() {
            for i in values.iter() {
                for j in values.iter() {
                    if i == j {
                        // Ignore identical entries
                        continue;
                    }

                    assert!(
                        !i.is_subset(j) && !j.is_subset(i),
                        "Antichain invariant violated: {:?} and {:?} are comparable.",
                        i,
                        j
                    );
                }
            }
        }
    }
}

/// Represents the antichain data structure used in the refinement checks.
pub trait AC<K: Eq + Hash, V: Clone + Ord> {
    /// Inserts `(key, value)` into the antichain.
    ///
    /// If a set already stored under `key` is a subset of `value`, `value` is
    /// dominated: nothing is inserted and this returns `false`. Otherwise
    /// `value` is inserted (any stored superset of `value` is removed, as it
    /// is now dominated) and this returns `true`.
    fn insert(&mut self, key: K, value: VecSet<V>) -> bool;

    /// Clears the antichain.
    fn clear(&mut self);
}

impl<K: Eq + Hash, V: Clone + Ord> AC<K, V> for Antichain<K, V> {
    fn insert(&mut self, key: K, value: VecSet<V>) -> bool {
        let mut inserted = false;
        self.storage
            .entry(key)
            .and_modify(|entry| {
                let mut contains = false;
                entry.retain(|inner_value| {
                    if inner_value.is_subset(&value) {
                        // The new value is a superset of an existing entry
                        contains = true;
                        true
                    } else if value.is_subset(inner_value) {
                        // Remove any entry that is a superset of the new value
                        false
                    } else {
                        // Leave incomparable entries unchanged
                        true
                    }
                });

                if !contains {
                    self.antichain_misses += 1; // Was not present
                    entry.insert(value.clone());
                    inserted = true;
                }
            })
            .or_insert_with(|| {
                self.antichain_misses += 1; // Was not present
                inserted = true;
                VecSet::singleton(value)
            });

        self.antichain_inserts += 1;
        self.max_antichain = self.max_antichain.max(self.storage.len());

        inserted
    }

    fn clear(&mut self) {
        self.storage.clear();
    }
}

impl<T: fmt::Debug, U: fmt::Debug> fmt::Debug for Antichain<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Antichain {{")?;
        for (key, values) in &self.storage {
            writeln!(f, "  {:?}: {:?}", key, values)?;
        }
        writeln!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use merc_collections::vecset;
    use merc_utilities::random_test;
    use rand::RngExt;

    use crate::AC;
    use crate::Antichain;

    #[test]
    fn test_antichain() {
        let mut antichain: Antichain<u32, u32> = Antichain::new();

        let inserted = antichain.insert(1, vecset![2, 3]);
        assert!(inserted);

        println!("{:?}", antichain);

        let inserted = antichain.insert(1, vecset![2, 3, 6]);
        assert!(
            !inserted,
            "The pair (1, {{2,3,6}}) should not be inserted in {:?}.",
            antichain
        );

        let inserted = antichain.insert(1, vecset![2]);
        assert!(
            inserted,
            "The pair (1, {{2}}) should overwrite (1, {{2, 3}}) in {:?}.",
            antichain
        );

        let inserted = antichain.insert(1, vecset![5, 6]);
        assert!(
            inserted,
            "The pair (1, {{5, 6}}) should be inserted since it is incomparable to existing pairs in {:?}.",
            antichain
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_antichain() {
        random_test(100, |rng| {
            let mut antichain: Antichain<u32, u32> = Antichain::new();

            // Insert random pairs into the antichain.
            for _ in 0..50 {
                let key = rng.random_range(0..10);
                let set_size = rng.random_range(1..5);
                let mut value = vecset![];

                for _ in 0..set_size {
                    value.insert(rng.random_range(0..20));
                }

                antichain.insert(key, value);
            }

            antichain.check_consistency();
        })
    }

    /// `len()` must count (key, value) pairs, not distinct keys: a key that
    /// maps to multiple incomparable sets should contribute one count per set.
    #[test]
    fn test_antichain_len_counts_pairs_not_keys() {
        let mut antichain: Antichain<u32, u32> = Antichain::new();

        // Insert two incomparable sets under key 1: {2} and {5, 6}.
        antichain.insert(1, vecset![2]);
        antichain.insert(1, vecset![5, 6]);
        // Insert one set under key 2.
        antichain.insert(2, vecset![10]);

        // The antichain has 2 pairs for key 1 and 1 pair for key 2 → 3 total.
        // Before the fix, `len()` returned `storage.len()` = 2 (two keys).
        assert_eq!(
            antichain.len(),
            3,
            "len() must count pairs, not keys; got {:?}",
            antichain
        );
        // Verify that iter yields exactly as many items as len() reports.
        assert_eq!(antichain.iter().count(), antichain.len());
    }
}
