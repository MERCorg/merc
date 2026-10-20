use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use crossbeam_utils::CachePadded;
use thread_local::ThreadLocal;

/// A monotonic counter that avoids the cross-thread contention of a single
/// shared [`AtomicU64`].
///
/// Increments are cheap and uncontended; reading the total is `O(threads that
/// have touched this counter)` and is intended for occasional reporting rather
/// than the hot path. Like a relaxed atomic, a [`get`](Self::get) concurrent
/// with increments observes a value somewhere between the pre- and post-state
/// and carries no ordering guarantees with respect to other memory.
#[derive(Default)]
pub struct ShardedCounter {
    /// One cache-line-padded atomic per thread that has touched this counter.
    shards: ThreadLocal<CachePadded<AtomicU64>>,
}

impl ShardedCounter {
    /// Creates a new counter starting at zero.
    #[must_use]
    pub fn new() -> Self {
        ShardedCounter {
            shards: ThreadLocal::new(),
        }
    }

    /// Adds `value` to the calling thread's shard. Uncontended with respect to
    /// other threads.
    #[inline]
    pub fn add(&self, value: u64) {
        self.shard().fetch_add(value, Ordering::Relaxed);
    }

    /// Increments the calling thread's shard by one.
    #[inline]
    pub fn increment(&self) {
        self.add(1);
    }

    /// Returns the sum of all per-thread shards.
    ///
    /// This iterates every thread's shard, so it is `O(threads)`; call it for
    /// reporting, not in a hot loop. The result is only a snapshot when no other
    /// thread is incrementing concurrently.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.shards.iter().map(|shard| shard.load(Ordering::Relaxed)).sum()
    }

    /// Resets every shard to zero.
    ///
    /// Requires `&mut self`, which statically guarantees no concurrent
    /// increments are in flight.
    pub fn reset(&mut self) {
        for shard in self.shards.iter_mut() {
            shard.store(0, Ordering::Relaxed);
        }
    }

    /// Returns the calling thread's shard, creating it on first use.
    #[inline]
    fn shard(&self) -> &AtomicU64 {
        self.shards.get_or(|| CachePadded::new(AtomicU64::new(0)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use crate::ShardedCounter;

    #[test]
    fn test_single_thread() {
        let counter = ShardedCounter::new();
        assert_eq!(counter.get(), 0);

        counter.increment();
        counter.add(41);
        assert_eq!(counter.get(), 42);
    }

    #[test]
    fn test_reset() {
        let mut counter = ShardedCounter::new();
        counter.add(10);
        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_concurrent_increments() {
        const THREADS: u64 = 8;
        const PER_THREAD: u64 = 100_000;

        let counter = Arc::new(ShardedCounter::new());
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let counter = Arc::clone(&counter);
                thread::spawn(move || {
                    for _ in 0..PER_THREAD {
                        counter.increment();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // No increments are lost: each thread owns its own shard.
        assert_eq!(counter.get(), THREADS * PER_THREAD);
    }
}
