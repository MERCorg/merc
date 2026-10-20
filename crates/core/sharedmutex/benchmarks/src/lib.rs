use std::hint::black_box;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicBool;
use std::thread::{self};

use criterion::Criterion;
use rand::distr::Bernoulli;
use rand::distr::Distribution;

/// The number of iterations to run for each benchmark.
pub const NUM_ITERATIONS: usize = 100000;
/// The number of threads to use for the benchmarks.
pub const THREADS: [usize; 6] = [1, 2, 4, 8, 16, 20];
/// The (average) number of read operations per write operation.
pub const READ_RATIOS: [u32; 6] = [1, 10, 100, 1000, 10000, 100000];

/// Execute the benchmarks for a given readers-writer lock implementation.
#[allow(clippy::too_many_arguments)]
pub fn benchmark<T, R, W>(
    c: &mut Criterion,
    name: &str,
    shared: T,
    read: R,
    write: W,
    num_threads: usize,
    num_iterations: usize,
    read_ratio: u32,
) where
    T: Clone + Send + 'static,
    R: FnOnce(&T) + Send + Copy + 'static,
    W: FnOnce(&T) + Send + Copy + 'static,
{
    // Share threads to avoid overhead.
    let mut threads = vec![];

    #[derive(Clone)]
    struct ThreadInfo<T> {
        busy: Arc<AtomicBool>,
        begin_barrier: Arc<Barrier>,
        end_barrier: Arc<Barrier>,
        dist: Bernoulli,
        shared: T,
    }

    let info = ThreadInfo {
        busy: Arc::new(AtomicBool::new(true)),
        begin_barrier: Arc::new(Barrier::new(num_threads + 1)),
        end_barrier: Arc::new(Barrier::new(num_threads + 1)),
        dist: Bernoulli::from_ratio(1, read_ratio).unwrap(),
        shared,
    };

    for _ in 0..num_threads {
        let info = info.clone();
        threads.push(thread::spawn(move || {
            let mut rng = rand::rng();

            loop {
                info.begin_barrier.wait();

                if !info.busy.load(std::sync::atomic::Ordering::SeqCst) {
                    // Quit the thread.
                    break;
                }

                // We execute it a fixed number of times.
                for _ in 0..num_iterations {
                    if info.dist.sample(&mut rng) {
                        write(&info.shared);
                        black_box(());
                    } else {
                        read(&info.shared);
                        black_box(());
                    }
                }

                info.end_barrier.wait();
            }
        }));
    }

    c.bench_function(
        format!("{name} {num_threads} {num_iterations} {read_ratio}").as_str(),
        |bencher| {
            bencher.iter(|| {
                info.begin_barrier.wait();

                info.end_barrier.wait();
            });
        },
    );

    // Tell the threads to quit and wait for them to join.
    info.busy.store(false, std::sync::atomic::Ordering::SeqCst);
    info.begin_barrier.wait();

    for thread in threads {
        thread.join().unwrap();
    }
}
