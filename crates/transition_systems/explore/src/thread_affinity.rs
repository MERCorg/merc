use std::ops::Deref;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

use log::debug;
use log::info;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;

use merc_utilities::MercError;

use crate::CpuTopology;

/// A rayon thread pool whose custom-pinned worker threads are joined on drop.
pub struct PinnedThreadPool {
    pool: Option<ThreadPool>,
    workers: Vec<JoinHandle<()>>,
}

impl Deref for PinnedThreadPool {
    type Target = ThreadPool;

    fn deref(&self) -> &ThreadPool {
        self.pool.as_ref().expect("pool is only removed on drop")
    }
}

impl Drop for PinnedThreadPool {
    fn drop(&mut self) {
        // Drop the inner pool first to signal every worker to terminate.
        drop(self.pool.take());

        // Then wait for each worker's OS thread to finish.
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Builds a rayon thread pool with `num_threads` workers.
///
/// When `pinned` is set, workers are pinned round-robin to the available CPU
/// cores, and the measured CPU topology (SMT/shared-cache/socket clustering
/// and inter-core latencies) is logged at info level. If no core information
/// is available on this platform, the pool is still created but worker
/// pinning is skipped.
///
/// The returned pool joins its worker threads when dropped.
///
/// # Errors
///
/// Returns an error if the underlying thread pool fails to build.
pub fn configure_rayon_thread_pool(num_threads: usize, pinned: bool) -> Result<PinnedThreadPool, MercError> {
    let worker_count = num_threads.max(1);
    let cores = if pinned {
        match CpuTopology::detect() {
            Ok(topology) => info!("Detected CPU topology:\n{topology}"),
            Err(error) => debug!("Failed to detect CPU topology: {error}"),
        }

        core_affinity2::get_core_ids().unwrap_or_default()
    } else {
        Vec::new()
    };

    let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::with_capacity(worker_count)));
    let workers_for_handler = workers.clone();

    let pool = ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .spawn_handler(move |thread| {
            let thread_index = thread.index();
            let core = if cores.is_empty() {
                None
            } else {
                Some(cores[thread_index % cores.len()])
            };

            let mut builder = std::thread::Builder::new();
            if let Some(name) = thread.name() {
                builder = builder.name(name.to_owned());
            }
            if let Some(stack_size) = thread.stack_size() {
                builder = builder.stack_size(stack_size);
            }

            let handle = builder.spawn(move || {
                if let Some(core) = core {
                    match core.set_affinity() {
                        Ok(()) => {
                            debug!("Pinned rayon worker thread {} to core {}", thread_index, core.0);
                        }
                        Err(error) => {
                            debug!(
                                "Failed to pin rayon worker thread {} to core {}: {error}",
                                thread_index, core.0
                            );
                        }
                    }
                }

                thread.run();
            })?;

            workers_for_handler
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(handle);

            Ok(())
        })
        .build()
        .map_err(|error| MercError::from(format!("Failed to build thread pool: {error}")))?;

    let workers = std::mem::take(&mut *workers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));

    Ok(PinnedThreadPool {
        pool: Some(pool),
        workers,
    })
}
