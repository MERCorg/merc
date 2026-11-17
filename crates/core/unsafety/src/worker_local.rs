use std::sync::Mutex;
use std::sync::OnceLock;
use std::thread;
use std::thread::ThreadId;

/// A value per worker of a thread pool, created on its worker on first use and dropped on its
/// worker with [`Self::clear_current`]. The value does not have to be `Send`, so thread-affine
/// state (such as an mCRL2 rewriter context) can be kept for the lifetime of the pool.
///
/// Workers are identified by an index below the number given to [`Self::new`], e.g. rayon's
/// `current_thread_index()`. Every slot remembers the thread that first used it and is only ever
/// accessed from that thread, so a wrong index makes [`Self::with`] panic instead of sharing a
/// value between threads. Re-entrant use from the same worker also panics (the closure must not
/// run other work of the pool while it holds the value).
pub struct WorkerLocal<T> {
    slots: Box<[Slot<T>]>,
}

struct Slot<T> {
    /// The thread that owns the value, set on first use.
    owner: OnceLock<ThreadId>,
    /// Only locked by `owner`; the lock detects re-entrant use.
    value: Mutex<Option<T>>,
}

// SAFETY: a slot's value is only accessed by the thread recorded in its `owner` (checked in
// `with` and `clear_current` before the value is touched), and it is never moved out to another
// thread (`Drop` leaks what is left), so a `T` that is not `Send` stays on its thread.
unsafe impl<T> Sync for WorkerLocal<T> {}

impl<T> WorkerLocal<T> {
    /// Creates an empty slot for each of `workers` workers.
    pub fn new(workers: usize) -> Self {
        Self {
            slots: (0..workers)
                .map(|_| Slot {
                    owner: OnceLock::new(),
                    value: Mutex::new(None),
                })
                .collect(),
        }
    }

    /// Runs `f` with the value of worker `index`, which must be the calling thread, creating the
    /// value with `init` first if there is none yet.
    ///
    /// # Panics
    ///
    /// If `index` is out of range, if slot `index` was used by another thread before, or on
    /// re-entrant use.
    pub fn with<R>(&self, index: usize, init: impl FnOnce() -> T, f: impl FnOnce(&mut T) -> R) -> R {
        let slot = self
            .slots
            .get(index)
            .expect("WorkerLocal::with called with a worker index out of range");
        let me = thread::current().id();
        assert_eq!(
            *slot.owner.get_or_init(|| me),
            me,
            "WorkerLocal slot {index} used from a thread that is not its worker"
        );
        let mut value = slot
            .value
            .try_lock()
            .expect("re-entrant use of a WorkerLocal value on the same worker");
        f(value.get_or_insert_with(init))
    }

    /// Drops the value of the calling thread, if it owns one. Call this on every worker (e.g. in
    /// a broadcast) before the `WorkerLocal` is dropped.
    pub fn clear_current(&self) {
        let me = thread::current().id();
        for slot in self.slots.iter() {
            if slot.owner.get() == Some(&me) {
                drop(slot.value.lock().expect("lock poisoned").take());
            }
        }
    }
}

impl<T> Drop for WorkerLocal<T> {
    fn drop(&mut self) {
        // Values that `clear_current` did not drop are leaked: dropping a thread-affine value on
        // this thread could be unsound.
        for slot in self.slots.iter_mut() {
            if let Some(value) = slot.value.get_mut().map(Option::take).unwrap_or(None) {
                std::mem::forget(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn test_worker_local_per_thread() {
        // `Rc` is not `Send`: every thread creates, uses and drops its own.
        let local: WorkerLocal<Rc<usize>> = WorkerLocal::new(4);
        std::thread::scope(|s| {
            for index in 0..4 {
                let local = &local;
                s.spawn(move || {
                    for _ in 0..3 {
                        assert_eq!(local.with(index, || Rc::new(index), |value| **value), index);
                    }
                    local.clear_current();
                });
            }
        });
    }

    #[test]
    #[should_panic(expected = "not its worker")]
    fn test_worker_local_wrong_thread() {
        let local: WorkerLocal<usize> = WorkerLocal::new(1);
        local.with(0, || 1, |_| ());
        std::thread::scope(|s| {
            s.spawn(|| local.with(0, || 2, |_| ()))
                .join()
                .unwrap_or_else(|e| std::panic::resume_unwind(e));
        });
    }
}
