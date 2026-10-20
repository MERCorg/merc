use std::hash::Hash;

use rustc_hash::FxHashMap;

use crate::ByteCompressedVec;
use crate::CompressedEntry;

/// Below this many entries in a single bucket, duplicates are detected with a
/// linear scan (cheap, no allocation). Above it, a reused hash map scratch
/// buffer is used instead, so a handful of very high-multiplicity buckets
/// cannot make [`dedup_grouped`] quadratic in the bucket size.
pub const HASH_DEDUP_THRESHOLD: usize = 16;

/// What [`dedup_grouped`]/[`dedup_by_bucket`] found for a single entry; passed
/// to their callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupOutcome {
    /// This entry is the first with its `(bucket, key)` in the input, so it
    /// survives. `position` is its 0-based rank among kept entries so far -
    /// i.e. the index it would have in a compacted, bucket-grouped output.
    Keep { position: usize },
    /// This entry duplicates an earlier one in the same bucket. `position` is
    /// the one a caller received in that earlier entry's `Keep` outcome, so a
    /// caller merging payloads knows exactly where to merge this one into.
    Duplicate { position: usize },
}

/// Counts `num_entries` entries (numbered `0..num_entries`, in original
/// insertion order) per bucket via `bucket_of`, then scatters them into
/// bucket-grouped order - preserving insertion order within a bucket - via a
/// counting sort, calling `write(position, original_index)` once per entry in
/// that grouped order.
///
/// Returns `bucket_ends`: `bucket_ends[bucket]` is the number of grouped
/// positions occupied by `bucket` and every bucket before it - so `bucket`'s
/// own entries occupy `bucket_ends[bucket - 1]..bucket_ends[bucket]` (reading
/// `bucket_ends[bucket - 1]` as `0` when `bucket == 0`); this is the same
/// array [`dedup_grouped`] takes.
pub fn scatter_into_buckets(
    num_buckets: usize,
    num_entries: usize,
    bucket_of: impl Fn(usize) -> usize,
    mut write: impl FnMut(usize, usize),
) -> Vec<usize> {
    // Count entries per bucket, then turn the counts into prefix-sum start
    // offsets.
    let mut offsets = vec![0usize; num_buckets];
    for i in 0..num_entries {
        offsets[bucket_of(i)] += 1;
    }

    let mut running = 0usize;
    for count in &mut offsets {
        let bucket_len = *count;
        *count = running;
        running += bucket_len;
    }

    // Scatter, preserving insertion order within a bucket. `offsets` doubles
    // as the running cursor here, so afterwards `offsets[bucket]` holds the
    // *end* of that bucket (and thus the start of the next one, since buckets
    // are contiguous) rather than its start.
    for i in 0..num_entries {
        let pos = &mut offsets[bucket_of(i)];
        write(*pos, i);
        *pos += 1;
    }

    offsets
}

/// Compacts entries that are already in bucket-grouped order (see
/// [`scatter_into_buckets`]) - numbered `0..bucket_ends.last()` by their
/// position in that grouped order, *not* by any original index - dropping or
/// merging entries that share the same `key_of` within a bucket.
///
/// Two entries can only be duplicates of each other if they're in the same
/// bucket, so once entries are bucket-grouped this never needs a global sort -
/// just a single walk over each bucket, calling `on_entry(position, bucket,
/// outcome)` once per entry with a [`DedupOutcome`]. Small buckets
/// (`<= HASH_DEDUP_THRESHOLD`) use a linear scan to find a duplicate; larger
/// ones a reused hash map, so a handful of very high out-degree buckets can't
/// make this quadratic in the bucket size.
///
/// A caller that only wants to drop duplicates copies the surviving entry's
/// columns into fresh storage on `Keep` and does nothing on `Duplicate`; one
/// that wants to merge duplicate payloads (e.g. combining two edges' BDD
/// configurations with a boolean `or`) instead merges into whatever it wrote
/// for `Duplicate`'s `position` on the matching `Keep`.
pub fn dedup_grouped<K, FKey, FEntry>(bucket_ends: &[usize], key_of: FKey, mut on_entry: FEntry)
where
    K: Eq + Hash + Copy,
    FKey: Fn(usize) -> K,
    FEntry: FnMut(usize, usize, DedupOutcome),
{
    // That cache only ever needs to cover the *current* bucket: a later bucket
    // never looks back at an earlier one's keys.
    let mut bucket_keys: Vec<K> = Vec::new();
    let mut scratch: FxHashMap<K, usize> = FxHashMap::default();
    let mut start = 0usize;
    let mut total_survivors = 0usize;

    for (bucket, &end) in bucket_ends.iter().enumerate() {
        let bucket_start_position = total_survivors;

        if end - start <= HASH_DEDUP_THRESHOLD {
            // Small bucket: a linear scan avoids any allocation.
            bucket_keys.clear();
            for read in start..end {
                let key = key_of(read);
                let seen = bucket_keys.iter().position(|&seen_key| seen_key == key);

                if let Some(local_position) = seen {
                    on_entry(
                        read,
                        bucket,
                        DedupOutcome::Duplicate {
                            position: bucket_start_position + local_position,
                        },
                    );
                } else {
                    on_entry(
                        read,
                        bucket,
                        DedupOutcome::Keep {
                            position: total_survivors,
                        },
                    );
                    bucket_keys.push(key);
                    total_survivors += 1;
                }
            }
        } else {
            // Large ("hub") bucket: a hash map keeps this from degrading to
            // O(bucket_len^2).
            scratch.clear();
            for read in start..end {
                let key = key_of(read);

                if let Some(&position) = scratch.get(&key) {
                    on_entry(read, bucket, DedupOutcome::Duplicate { position });
                } else {
                    scratch.insert(key, total_survivors);
                    on_entry(
                        read,
                        bucket,
                        DedupOutcome::Keep {
                            position: total_survivors,
                        },
                    );
                    total_survivors += 1;
                }
            }
        }

        start = end;
    }
}

/// Deduplicates `num_entries` entries - numbered `0..num_entries` in original
/// insertion order - into `num_buckets` buckets, without ever globally sorting
/// or permuting the caller's own storage: [`scatter_into_buckets`] builds a
/// `position -> original_index` permutation, then [`dedup_grouped`] runs
/// against it, translating grouped positions back to original indices so
/// `on_entry` never has to deal with the grouped order itself.
///
/// `on_entry` receives the entry's *original* index (and its bucket - the
/// same value `bucket_of` would return for it), not a grouped or compacted
/// one, so a caller can index its own per-entry columns directly by that
/// original index from inside the callback, rather than physically
/// scattering them to match the bucket-grouped order.
pub fn dedup_by_bucket<K, FBucket, FKey, FEntry>(
    num_buckets: usize,
    num_entries: usize,
    bucket_of: FBucket,
    key_of: FKey,
    mut on_entry: FEntry,
) where
    K: Eq + Hash + Copy,
    FBucket: Fn(usize) -> usize,
    FKey: Fn(usize) -> K,
    FEntry: FnMut(usize, usize, DedupOutcome),
{
    if num_entries == 0 {
        return;
    }

    // `grouped` holds `original_index` values up to `num_entries - 1`, written
    // in original-index order (0, 1, 2, ...) at scattered positions.
    let mut grouped: ByteCompressedVec<usize> =
        ByteCompressedVec::with_capacity(num_entries, num_entries.bytes_required());
    grouped.resize_zeroed(num_entries, num_entries.bytes_required());
    let bucket_ends = scatter_into_buckets(num_buckets, num_entries, bucket_of, |position, original_index| {
        grouped.set(position, original_index)
    });
    dedup_grouped(
        &bucket_ends,
        |position| key_of(grouped.index(position)),
        |position, bucket, outcome| on_entry(grouped.index(position), bucket, outcome),
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::collections::HashSet;

    use itertools::Itertools;
    use rand::RngExt;

    use merc_utilities::random_test;

    use super::DedupOutcome;
    use super::HASH_DEDUP_THRESHOLD;
    use super::dedup_by_bucket;
    use super::dedup_grouped;
    use super::scatter_into_buckets;

    /// Drops duplicate `(bucket, key)` pairs down to the first occurrence of
    /// each pair, using [`dedup_by_bucket`], and returns the survivors in the
    /// order they were kept (i.e. already grouped by bucket). Also checks that
    /// the `bucket` handed to the callback always matches the entry's own
    /// bucket.
    fn drop_duplicates(num_buckets: usize, entries: &[(usize, u32)]) -> Vec<(usize, u32)> {
        let mut kept: Vec<(usize, u32)> = Vec::new();
        dedup_by_bucket(
            num_buckets,
            entries.len(),
            |i| entries[i].0,
            |i| entries[i].1,
            |i, bucket, outcome| {
                assert_eq!(bucket, entries[i].0, "on_entry's bucket should match the entry's own");
                if let DedupOutcome::Keep { position } = outcome {
                    debug_assert_eq!(position, kept.len(), "Keep positions should be sequential");
                    kept.push(entries[i]);
                }
            },
        );
        kept
    }

    /// Checks the drop mode against a trivial, independently implemented
    /// reference (deduplicating the raw entries with a `HashSet`), so the
    /// bucket-local approach is not silently missing or duplicating entries a
    /// naive approach would catch.
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_drop_matches_naive_reference() {
        random_test(200, |rng| {
            let num_buckets = rng.random_range(1..20);
            let entries: Vec<(usize, u32)> = (0..rng.random_range(0..200))
                .map(|_| (rng.random_range(0..num_buckets), rng.random_range(0..10)))
                .collect();

            let kept = drop_duplicates(num_buckets, &entries);

            assert!(kept.iter().all_unique(), "Every kept entry should be unique");
            let expected: HashSet<(usize, u32)> = entries.iter().copied().collect();
            let actual: HashSet<(usize, u32)> = kept.iter().copied().collect();
            assert_eq!(actual, expected, "Kept entries should match a naive HashSet reference");
        });
    }

    /// A single high-multiplicity bucket exercises the hash-map dedup path
    /// (`> HASH_DEDUP_THRESHOLD` entries in one bucket) rather than the
    /// linear-scan one used for small buckets.
    #[test]
    fn test_hub_bucket_uses_hash_path() {
        let entries: Vec<(usize, u32)> = (0..2)
            .flat_map(|_| (0..HASH_DEDUP_THRESHOLD as u32 * 2).map(|key| (0usize, key)))
            .collect();
        assert!(
            entries.len() / 2 > HASH_DEDUP_THRESHOLD,
            "bucket should exceed the threshold"
        );

        let kept = drop_duplicates(1, &entries);
        assert_eq!(kept.len(), HASH_DEDUP_THRESHOLD * 2);
        assert!(kept.iter().all_unique());
    }

    /// Merges duplicate `(bucket, key)` entries' `u32` payloads by summing
    /// them on `Duplicate`, checking the result against a naive reference -
    /// i.e. the merge mode used by `VariabilityParityGameBuilder::remove_duplicates`
    /// (which merges BDD configurations with a boolean `or` instead of a sum).
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_merge_matches_naive_reference() {
        random_test(200, |rng| {
            let num_buckets = rng.random_range(1..20);
            let entries: Vec<(usize, u32, u32)> = (0..rng.random_range(0..200))
                .map(|_| {
                    (
                        rng.random_range(0..num_buckets),
                        rng.random_range(0..10),
                        rng.random_range(0..100),
                    )
                })
                .collect();

            let mut merged: Vec<(usize, u32, u32)> = Vec::new();
            dedup_by_bucket(
                num_buckets,
                entries.len(),
                |i| entries[i].0,
                |i| entries[i].1,
                |i, _bucket, outcome| match outcome {
                    DedupOutcome::Keep { position } => {
                        debug_assert_eq!(position, merged.len(), "Keep positions should be sequential");
                        merged.push(entries[i]);
                    }
                    DedupOutcome::Duplicate { position } => {
                        merged[position].2 += entries[i].2;
                    }
                },
            );

            assert!(
                merged.iter().map(|&(bucket, key, _)| (bucket, key)).all_unique(),
                "Every merged entry's (bucket, key) should be unique"
            );

            let mut expected: HashMap<(usize, u32), u32> = HashMap::new();
            for &(bucket, key, payload) in &entries {
                *expected.entry((bucket, key)).or_default() += payload;
            }
            let actual: HashMap<(usize, u32), u32> = merged
                .iter()
                .map(|&(bucket, key, payload)| ((bucket, key), payload))
                .collect();
            assert_eq!(actual, expected, "Merged payloads should match a naive reference");
        });
    }

    /// Exercises `scatter_into_buckets` and `dedup_grouped` directly rather
    /// than through the `dedup_by_bucket` convenience wrapper, mirroring how
    /// `LtsBuilderMem::remove_duplicates` uses them.
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_scatter_then_dedup_grouped_in_place_matches_naive_reference() {
        random_test(200, |rng| {
            let num_buckets = rng.random_range(1..20);
            let entries: Vec<(usize, u32)> = (0..rng.random_range(0..200))
                .map(|_| (rng.random_range(0..num_buckets), rng.random_range(0..10)))
                .collect();

            // Scatter the payload straight into grouped order via a write callback.
            let scattered = RefCell::new(vec![0u32; entries.len()]);
            let bucket_ends = scatter_into_buckets(
                num_buckets,
                entries.len(),
                |i| entries[i].0,
                |position, i| {
                    scattered.borrow_mut()[position] = entries[i].1;
                },
            );

            let mut write = 0usize;
            let mut kept_buckets: Vec<usize> = Vec::new();

            dedup_grouped(
                &bucket_ends,
                |position| scattered.borrow()[position],
                |position, bucket, outcome| {
                    if let DedupOutcome::Keep {
                        position: kept_position,
                    } = outcome
                    {
                        debug_assert_eq!(kept_position, write, "Keep positions should be sequential");
                        if write != position {
                            let value = scattered.borrow()[position];
                            scattered.borrow_mut()[write] = value;
                        }
                        kept_buckets.push(bucket);
                        write += 1;
                    }
                },
            );

            let scattered = scattered.into_inner();
            let kept: Vec<(usize, u32)> = kept_buckets
                .into_iter()
                .zip(scattered[..write].iter().copied())
                .collect();

            assert!(kept.iter().all_unique(), "Every kept entry should be unique");
            let expected: HashSet<(usize, u32)> = entries.iter().copied().collect();
            let actual: HashSet<(usize, u32)> = kept.iter().copied().collect();
            assert_eq!(actual, expected, "Kept entries should match a naive HashSet reference");
        });
    }
}
