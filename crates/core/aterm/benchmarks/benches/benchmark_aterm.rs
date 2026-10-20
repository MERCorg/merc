//!
//! This is a copy from the benchmarks in the mCRL2 toolset for comparison purposes.
//!

use std::array::from_fn;
use std::collections::VecDeque;
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use merc_aterm::ATerm;
use merc_aterm::ATermRef;
use merc_aterm::ATermSend;
use merc_aterm::Symb;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_aterm::storage::THREAD_TERM_POOL;

/// Sets the number of threads for all the benchmarks.
pub const THREADS: [usize; 6] = [1, 2, 4, 8, 16, 32];

/// Executes a function across multiple threads and measures total execution time.
/// The main thread executes with id 0, while additional threads get ids 1..number_of_threads-1.
fn benchmark_threads<F>(number_of_threads: usize, f: F)
where
    F: Fn(usize) + Send + Sync + 'static,
{
    debug_assert!(number_of_threads > 0, "Number of threads must be greater than 0");

    let f = Arc::new(f);

    // Initialize worker threads (excluding main thread)
    let mut handles = Vec::with_capacity(number_of_threads - 1);
    for id in 1..number_of_threads {
        let f_clone = f.clone();
        let handle = thread::spawn(move || {
            f_clone(id);
        });
        handles.push(handle);
    }

    // Run the benchmark on the main thread with id 0
    f(0);

    // Wait for all worker threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked during benchmark");
    }
}

/// Creates a nested function application where f_1 = f(c, c) and f_i = f(f_{i-1}, f_{i-1}).
/// Applying this `depth` times returns f_{depth+1}; `c` is given by `leaf_name`.
/// The arity of the function symbols is a constant.
fn create_nested_function<const ARITY: usize>(function_name: &str, leaf_name: &str, depth: usize) -> ATerm {
    debug_assert!(depth > 0, "Depth must be greater than 0");

    // Create function symbols
    let f_symbol = Symbol::new(function_name, ARITY);
    let c_symbol = Symbol::new(leaf_name, 0);

    // Create the leaf term c
    let c_term = ATerm::constant(&c_symbol);

    // Initialize with f(c, ..., c)
    let mut f_term = ATerm::with_args(&f_symbol, &from_fn::<_, ARITY, _>(|_| c_term.copy())).protect();

    // Build nested structure: f(f_term, ..., f_term) for each level
    for _ in 0..depth {
        f_term.replace(ATerm::with_args(&f_symbol, &from_fn::<_, ARITY, _>(|_| f_term.copy())));
    }

    debug_assert_eq!(f_term.get_head_symbol().name(), function_name);
    debug_assert_eq!(f_term.get_head_symbol().arity(), ARITY);

    f_term
}

// In these three benchmarks all threads operate on a shared term.
fn benchmark_shared_creation(c: &mut Criterion) {
    const SIZE: usize = 400000;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("shared_creation", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let mut result = Duration::new(0, 0);
                    for _ in 0..iters {
                        let start = Instant::now();

                        benchmark_threads(num_threads, |_id| {
                            black_box(create_nested_function::<2>("f", "c", SIZE));
                        });

                        result += start.elapsed();

                        // For a garbage collection (not part of the measurement) to clean up the created terms.
                        THREAD_TERM_POOL.with(|tp| tp.force_collect_garbage());
                    }
                    result
                });
            },
        );
    }
}

/// Local function to count the number of subterms in a term.
fn inspect<'a>(term: &'a ATermRef<'a>, iterations: usize) -> u64 {
    let mut queue: VecDeque<ATermRef<'a>> = VecDeque::new();

    let mut count = 0;

    for _ in 0..iterations {
        // Simple breadth-first search to count elements
        queue.push_back(term.copy());

        while let Some(current_term) = queue.pop_front() {
            // Iterate through all arguments of the current term
            for arg in current_term.arguments() {
                count += 1;
                queue.push_back(arg);
            }
        }
    }

    count
}

fn benchmark_shared_inspect(c: &mut Criterion) {
    const SIZE: usize = 20;
    const ITERATIONS: usize = 1024;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("shared_inspect", num_threads),
            &num_threads,
            |b, &num_threads| {
                let shared_term = Arc::new(ATermSend::from(create_nested_function::<2>("f", "c", SIZE)));
                assert_eq!(inspect(&shared_term.copy(), 1), 4194302);

                b.iter(|| {
                    let term = shared_term.clone();

                    benchmark_threads(num_threads, move |_id| {
                        black_box(inspect(&term.copy(), ITERATIONS.div_ceil(num_threads)));
                    });
                });
            },
        );
    }
}

fn benchmark_shared_lookup(c: &mut Criterion) {
    let _ = env_logger::try_init();

    const SIZE: usize = 400_000;
    const ITERATIONS: usize = 1024;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("shared_lookup", num_threads),
            &num_threads,
            |b, &num_threads| {
                // Keep one protected instance
                let term = create_nested_function::<2>("f", "c", SIZE);

                b.iter(|| {
                    benchmark_threads(num_threads, move |_id| {
                        for _ in 0..ITERATIONS.div_ceil(num_threads) {
                            black_box(create_nested_function::<2>("f", "c", SIZE));
                        }
                    });
                });

                drop(term);
            },
        );
    }
}

/// The leaf symbol name used by a given thread in the "unique" benchmarks below, so that each
/// thread constructs (and later looks up) its own separate term.
fn unique_leaf_name(id: usize) -> String {
    format!("c{id}")
}

// In these three benchmarks all threads operate on their own separate term.
fn benchmark_unique_creation(c: &mut Criterion) {
    const SIZE: usize = 4_194_304;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("unique_creation", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let mut result = Duration::new(0, 0);
                    for _ in 0..iters {
                        let start = Instant::now();

                        benchmark_threads(num_threads, move |id| {
                            black_box(create_nested_function::<2>(
                                "f",
                                &unique_leaf_name(id),
                                SIZE.div_ceil(num_threads),
                            ));
                        });

                        result += start.elapsed();

                        // For a garbage collection (not part of the measurement) to clean up the created terms.
                        THREAD_TERM_POOL.with(|tp| tp.force_collect_garbage());
                    }

                    result
                });
            },
        );
    }
}

fn benchmark_unique_inspect(c: &mut Criterion) {
    const SIZE: usize = 20;
    const ITERATIONS: usize = 1024;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("unique_inspect", num_threads),
            &num_threads,
            |b, &num_threads| {
                let terms: Arc<Vec<ATermSend>> = Arc::new(
                    (0..num_threads)
                        .map(|id| ATermSend::from(create_nested_function::<2>("f", &unique_leaf_name(id), SIZE)))
                        .collect(),
                );

                b.iter(|| {
                    let terms = terms.clone();

                    benchmark_threads(num_threads, move |id| {
                        black_box(inspect(&terms[id].copy(), ITERATIONS.div_ceil(num_threads)));
                    });
                });
            },
        );
    }
}

fn benchmark_unique_lookup(c: &mut Criterion) {
    let _ = env_logger::try_init();

    const SIZE: usize = 400000;
    const ITERATIONS: usize = 1024;

    THREAD_TERM_POOL.with(|tp| tp.automatic_garbage_collection(false));

    for num_threads in THREADS {
        c.bench_with_input(
            BenchmarkId::new("unique_lookup", num_threads),
            &num_threads,
            |b, &num_threads| {
                // Keep one protected instance per thread, matching what each thread looks up
                // below, so the benchmark measures term pool lookups rather than creation.
                let terms: Vec<ATerm> = (0..num_threads)
                    .map(|id| create_nested_function::<2>("f", &unique_leaf_name(id), SIZE))
                    .collect();

                b.iter(|| {
                    benchmark_threads(num_threads, move |id| {
                        for _ in 0..ITERATIONS.div_ceil(num_threads) {
                            black_box(create_nested_function::<2>("f", &unique_leaf_name(id), SIZE));
                        }
                    });
                });

                drop(terms);
            },
        );
    }
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = benchmark_shared_creation,
        benchmark_unique_creation,
        benchmark_shared_lookup,
        benchmark_unique_lookup,
        benchmark_shared_inspect,
        benchmark_unique_inspect,
);
criterion_main!(benches);
