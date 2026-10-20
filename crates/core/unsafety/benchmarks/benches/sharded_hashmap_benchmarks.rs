use std::hash::BuildHasherDefault;
use std::hash::RandomState;

use criterion::Criterion;
use rustc_hash::FxBuildHasher;

use benchmarks_unsafety::ConcurrentSet;
use benchmarks_unsafety::DashSetWrapper;
use benchmarks_unsafety::EXISTING_PERCENTAGES;
use benchmarks_unsafety::KeyKind;
use benchmarks_unsafety::NUM_ITERATIONS;
use benchmarks_unsafety::READ_RATIOS;
use benchmarks_unsafety::ShardedSet;
use benchmarks_unsafety::THREADS;
use benchmarks_unsafety::benchmark;

/// The hasher used by `rustc-hash` 1.x, kept for comparison with the 2.x
/// [`FxBuildHasher`] the workspace uses.
type FxBuildHasherV1 = BuildHasherDefault<rustc_hash_v1::FxHasher>;

/// Benchmark the scalability of the sharded hash map across the number of
/// threads, the read/write (get/insert) ratio and the percentage of existing
/// versus newly inserted values, using the hasher its users use.
pub fn benchmark_sharded_hashmap(c: &mut Criterion) {
    for num_threads in THREADS {
        for read_ratio in READ_RATIOS {
            for existing_percent in EXISTING_PERCENTAGES {
                benchmark(
                    c,
                    "unsafety::ShardedHashMap",
                    ShardedSet::<FxBuildHasher>::new(),
                    KeyKind::Dense,
                    num_threads,
                    NUM_ITERATIONS,
                    read_ratio,
                    existing_percent,
                );
            }
        }
    }
}

/// Benchmark the same workloads against [`dashmap::DashSet`] as a baseline to
/// put the sharded hash map's scalability in context.
pub fn benchmark_dashset(c: &mut Criterion) {
    for num_threads in THREADS {
        for read_ratio in READ_RATIOS {
            for existing_percent in EXISTING_PERCENTAGES {
                benchmark(
                    c,
                    "dashmap::DashSet",
                    DashSetWrapper::<FxBuildHasher>::new(),
                    KeyKind::Dense,
                    num_threads,
                    NUM_ITERATIONS,
                    read_ratio,
                    existing_percent,
                );
            }
        }
    }
}

/// Runs the reduced hasher-comparison workload grid for one hasher.
fn benchmark_hasher<S: ConcurrentSet + Default>(c: &mut Criterion, name: &str) {
    for keys in [KeyKind::Dense, KeyKind::Pointer] {
        for num_threads in [1, 8] {
            for existing_percent in [0, 95] {
                benchmark(
                    c,
                    name,
                    S::default(),
                    keys,
                    num_threads,
                    NUM_ITERATIONS,
                    10,
                    existing_percent,
                );
            }
        }
    }
}

/// Compares hashers on the sharded hash map, for both dense integer keys and
/// pointer-like keys whose low bits carry no entropy. Filter with
/// `-- hasher::` to run only these.
pub fn benchmark_hashers(c: &mut Criterion) {
    benchmark_hasher::<ShardedSet<FxBuildHasher>>(c, "hasher::fx2");
    benchmark_hasher::<ShardedSet<FxBuildHasherV1>>(c, "hasher::fx1");
    benchmark_hasher::<ShardedSet<foldhash::fast::RandomState>>(c, "hasher::foldhash");
    benchmark_hasher::<ShardedSet<ahash::RandomState>>(c, "hasher::ahash");
    benchmark_hasher::<ShardedSet<RandomState>>(c, "hasher::siphash");
}
