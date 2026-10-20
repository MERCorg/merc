use std::hint::black_box;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

use criterion::Criterion;

/// The number of increments each thread performs per measured iteration.
pub const NUM_ITERATIONS: usize = 100000;
/// The number of threads to use for the benchmarks.
pub const THREADS: [usize; 6] = [1, 2, 4, 8, 16, 32];

/// Execute the benchmark for a given counter implementation.
///
/// Spawns `num_threads` worker threads that all hammer `increment` on a shared
/// counter `num_iterations` times per measured iteration, isolating the
/// cross-thread contention cost of the increment operation.
pub fn benchmark<T, I>(
    c: &mut Criterion,
    name: &str,
    shared: T,
    increment: I,
    num_threads: usize,
    num_iterations: usize,
) where
    T: Clone + Send + 'static,
    I: FnOnce(&T) + Send + Copy + 'static,
{
    // Share threads to avoid the overhead of spawning per measured iteration.
    let mut threads = vec![];

    #[derive(Clone)]
    struct ThreadInfo<T> {
        busy: Arc<AtomicBool>,
        begin_barrier: Arc<Barrier>,
        end_barrier: Arc<Barrier>,
        shared: T,
    }

    let info = ThreadInfo {
        busy: Arc::new(AtomicBool::new(true)),
        begin_barrier: Arc::new(Barrier::new(num_threads + 1)),
        end_barrier: Arc::new(Barrier::new(num_threads + 1)),
        shared,
    };

    for _ in 0..num_threads {
        let info = info.clone();
        threads.push(thread::spawn(move || {
            loop {
                info.begin_barrier.wait();

                if !info.busy.load(Ordering::SeqCst) {
                    // Quit the thread.
                    break;
                }

                // We execute it a fixed number of times.
                for _ in 0..num_iterations {
                    increment(&info.shared);
                    black_box(());
                }

                info.end_barrier.wait();
            }
        }));
    }

    c.bench_function(format!("{name} {num_threads} {num_iterations}").as_str(), |bencher| {
        bencher.iter(|| {
            info.begin_barrier.wait();

            info.end_barrier.wait();
        });
    });

    // Tell the threads to quit and wait for them to join.
    info.busy.store(false, Ordering::SeqCst);
    info.begin_barrier.wait();

    for thread in threads {
        thread.join().unwrap();
    }
}
