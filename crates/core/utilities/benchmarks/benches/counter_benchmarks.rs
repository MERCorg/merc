use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use criterion::Criterion;

use merc_utilities::ShardedCounter;

use benchmarks_utilities::NUM_ITERATIONS;
use benchmarks_utilities::THREADS;
use benchmarks_utilities::benchmark;

/// Benchmark the sharded counter, which keeps a per-thread shard so increments
/// stay uncontended as the thread count grows.
pub(crate) fn benchmark_sharded_counter(c: &mut Criterion) {
    for num_threads in THREADS {
        benchmark(
            c,
            "merc_utilities::ShardedCounter",
            Arc::new(ShardedCounter::new()),
            |counter| counter.increment(),
            num_threads,
            NUM_ITERATIONS,
        );
    }
}

/// Benchmark a single shared [`AtomicUsize`], the baseline whose contention the
/// sharded counter is meant to avoid.
pub(crate) fn benchmark_atomic_usize(c: &mut Criterion) {
    for num_threads in THREADS {
        benchmark(
            c,
            "std::sync::atomic::AtomicUsize",
            Arc::new(AtomicUsize::new(0)),
            |counter| {
                counter.fetch_add(1, Ordering::Relaxed);
            },
            num_threads,
            NUM_ITERATIONS,
        );
    }
}
