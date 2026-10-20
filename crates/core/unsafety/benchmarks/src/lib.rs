use std::hash::BuildHasher;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread::{self};
use std::time::Duration;
use std::time::Instant;

use criterion::Criterion;
use dashmap::DashSet;
use rand::RngExt;
use rand::SeedableRng;
use rand::distr::Bernoulli;
use rand::distr::Distribution;
use rand::rngs::StdRng;

use merc_unsafety::ShardedHashMap;

/// The number of operations each thread performs per measured iteration.
pub const NUM_ITERATIONS: usize = 50000;
/// The number of threads to use for the benchmarks.
pub const THREADS: [usize; 6] = [1, 2, 4, 8, 16, 20];
/// The (average) number of `get` operations per `insert` operation.
pub const READ_RATIOS: [u32; 4] = [1, 10, 100, 1000];
/// The percentage of the value space that is pre-populated, controlling how
/// many lookups hit and how many insertions find an existing value.
pub const EXISTING_PERCENTAGES: [u64; 5] = [0, 50, 95, 99, 100];
/// The range `[0, VALUE_SPACE)` of key indices operations sample from. Kept
/// bounded so the table size stays bounded.
pub const VALUE_SPACE: u64 = 1 << 16;

/// Fixed seed so every run (and every baseline comparison) performs the exact
/// same operation sequence.
const SEED: u64 = 0x5eed_0f_5a4d_ed00;

/// Distance between consecutive [`KeyKind::Pointer`] keys, matching the size of
/// a binary term in the term pool (symbol plus two argument pointers, rounded up
/// to the allocator's alignment).
const POINTER_STRIDE: usize = 32;

/// Base address of the [`KeyKind::Pointer`] keys, a typical heap address.
const POINTER_BASE: usize = 0x7f3a_5c00_0000;

/// The shape of the keys stored in the set, which determines how well a hasher
/// distributes them.
#[derive(Clone, Copy, Debug)]
pub enum KeyKind {
    /// Dense integers `0, 1, 2, ...`, like the indices stored in
    /// `ConcurrentIndexedSet`.
    Dense,
    /// Aligned addresses spaced [`POINTER_STRIDE`] apart, like the term pointers
    /// stored in the term pool. Their low bits carry no entropy.
    Pointer,
}

impl KeyKind {
    /// Maps a key index in `[0, VALUE_SPACE)` to the actual key.
    pub fn key(self, index: u64) -> usize {
        match self {
            KeyKind::Dense => index as usize,
            KeyKind::Pointer => POINTER_BASE + index as usize * POINTER_STRIDE,
        }
    }
}

/// A concurrent set of `usize` values exercised by the benchmark harness.
/// `clone` must share the underlying storage so all threads hit one instance.
pub trait ConcurrentSet: Clone + Send + Sync + 'static {
    /// Returns true if `value` is present.
    fn get(&self, value: usize) -> bool;
    /// Inserts `value`, returning true if it was freshly inserted.
    fn insert(&self, value: usize) -> bool;
    /// Removes all values.
    fn clear(&self);
}

/// [`ShardedHashMap`] storing the values directly via its convenience API.
pub struct ShardedSet<S>(Arc<ShardedHashMap<usize, S>>);

impl<S> Clone for ShardedSet<S> {
    fn clone(&self) -> ShardedSet<S> {
        ShardedSet(self.0.clone())
    }
}

impl<S: Default> ShardedSet<S> {
    pub fn new() -> ShardedSet<S> {
        ShardedSet(Arc::new(ShardedHashMap::new()))
    }
}

impl<S: Default> Default for ShardedSet<S> {
    fn default() -> ShardedSet<S> {
        ShardedSet::new()
    }
}

impl<S: BuildHasher + Send + Sync + 'static> ConcurrentSet for ShardedSet<S> {
    fn get(&self, value: usize) -> bool {
        self.0.contains(&value)
    }

    fn insert(&self, value: usize) -> bool {
        self.0.insert(value).1
    }

    fn clear(&self) {
        self.0.clear();
    }
}

/// [`DashSet`] baseline, the concurrent set the rest of the crate uses.
pub struct DashSetWrapper<S>(Arc<DashSet<usize, S>>);

impl<S> Clone for DashSetWrapper<S> {
    fn clone(&self) -> DashSetWrapper<S> {
        DashSetWrapper(self.0.clone())
    }
}

impl<S: BuildHasher + Clone + Default> DashSetWrapper<S> {
    pub fn new() -> DashSetWrapper<S> {
        DashSetWrapper(Arc::new(DashSet::with_hasher(S::default())))
    }
}

impl<S: BuildHasher + Clone + Default> Default for DashSetWrapper<S> {
    fn default() -> DashSetWrapper<S> {
        DashSetWrapper::new()
    }
}

impl<S: BuildHasher + Clone + Send + Sync + 'static> ConcurrentSet for DashSetWrapper<S> {
    fn get(&self, value: usize) -> bool {
        self.0.contains(&value)
    }

    fn insert(&self, value: usize) -> bool {
        self.0.insert(value)
    }

    fn clear(&self) {
        self.0.clear();
    }
}

/// A single pre-generated operation: insert (true) or get (false) of a key.
#[derive(Clone, Copy)]
struct Operation {
    insert: bool,
    key: usize,
}

/// Generates the operation sequence of one thread, choosing a `get` over an
/// `insert` with odds `read_ratio` to one and sampling keys uniformly from
/// [`VALUE_SPACE`].
fn generate_operations(thread_id: usize, num_iterations: usize, read_ratio: u32, keys: KeyKind) -> Vec<Operation> {
    let mut rng = StdRng::seed_from_u64(SEED ^ thread_id as u64);
    // Sampling true means an insertion, occurring once per `read_ratio` ops.
    let dist = Bernoulli::from_ratio(1, read_ratio + 1).unwrap();

    (0..num_iterations)
        .map(|_| Operation {
            insert: dist.sample(&mut rng),
            key: keys.key(rng.random_range(0..VALUE_SPACE)),
        })
        .collect()
}

/// Drives `num_threads` threads that each perform `num_iterations` operations
/// per measured iteration on `set`, choosing a `get` over an `insert` with odds
/// `read_ratio` to one.
///
/// Before every measured iteration (untimed) the set is cleared and the first
/// `existing_percent` of [`VALUE_SPACE`] is inserted, so `existing_percent` is
/// the hit rate at the start of every iteration rather than only the first.
/// The operation sequences are generated up front, so the measurement contains
/// only set operations.
///
/// The threads time themselves: after being released they spin until all of
/// them are running, and the measurement spans from the earliest start to the
/// latest finish. Timing from the main thread instead would include the time the
/// operating system takes to wake the threads from the barrier, which on a busy
/// or virtualised machine is of the same order as the work itself.
#[allow(clippy::too_many_arguments)]
pub fn benchmark<S: ConcurrentSet>(
    c: &mut Criterion,
    name: &str,
    set: S,
    keys: KeyKind,
    num_threads: usize,
    num_iterations: usize,
    read_ratio: u32,
    existing_percent: u64,
) {
    let existing = VALUE_SPACE * existing_percent / 100;
    let populate = |set: &S| {
        set.clear();
        for index in 0..existing {
            set.insert(keys.key(index));
        }
    };

    // Share threads to avoid spawn overhead between samples.
    let mut threads = vec![];

    #[derive(Clone)]
    struct ThreadInfo<S> {
        busy: Arc<AtomicBool>,
        begin_barrier: Arc<Barrier>,
        end_barrier: Arc<Barrier>,
        /// Number of threads that are running, used to start them together.
        running: Arc<AtomicUsize>,
        /// Per thread start and end of its operations, in nanoseconds since `epoch`.
        start: Arc<[AtomicU64]>,
        end: Arc<[AtomicU64]>,
        epoch: Instant,
        set: S,
    }

    let info = ThreadInfo {
        busy: Arc::new(AtomicBool::new(true)),
        begin_barrier: Arc::new(Barrier::new(num_threads + 1)),
        end_barrier: Arc::new(Barrier::new(num_threads + 1)),
        running: Arc::new(AtomicUsize::new(0)),
        start: (0..num_threads).map(|_| AtomicU64::new(0)).collect(),
        end: (0..num_threads).map(|_| AtomicU64::new(0)).collect(),
        epoch: Instant::now(),
        set,
    };

    for thread_id in 0..num_threads {
        let info = info.clone();
        let operations = generate_operations(thread_id, num_iterations, read_ratio, keys);
        threads.push(thread::spawn(move || {
            loop {
                info.begin_barrier.wait();

                if !info.busy.load(Ordering::SeqCst) {
                    // Quit the thread.
                    break;
                }

                // Spin until every thread is running so they start together.
                info.running.fetch_add(1, Ordering::SeqCst);
                while info.running.load(Ordering::SeqCst) < num_threads {
                    std::hint::spin_loop();
                }

                let start = info.epoch.elapsed().as_nanos() as u64;
                for operation in &operations {
                    if operation.insert {
                        black_box(info.set.insert(operation.key));
                    } else {
                        black_box(info.set.get(operation.key));
                    }
                }
                let end = info.epoch.elapsed().as_nanos() as u64;

                info.start[thread_id].store(start, Ordering::SeqCst);
                info.end[thread_id].store(end, Ordering::SeqCst);
                info.end_barrier.wait();
            }
        }));
    }

    c.bench_function(
        format!("{name} keys {keys:?} threads {num_threads} iterations {num_iterations} read-ratio {read_ratio} existing {existing_percent}%").as_str(),
        |bencher| {
            bencher.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    populate(&info.set);
                    info.running.store(0, Ordering::SeqCst);

                    info.begin_barrier.wait();
                    info.end_barrier.wait();

                    let start = info.start.iter().map(|t| t.load(Ordering::SeqCst)).min().unwrap();
                    let end = info.end.iter().map(|t| t.load(Ordering::SeqCst)).max().unwrap();
                    total += Duration::from_nanos(end - start);
                }
                total
            });
        },
    );

    // Tell the threads to quit and wait for them to join.
    info.busy.store(false, Ordering::SeqCst);
    info.begin_barrier.wait();

    for thread in threads {
        thread.join().unwrap();
    }
}
