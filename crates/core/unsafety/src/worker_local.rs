use std::sync::OnceLock;
use std::thread;
use std::thread::ThreadId;

use parking_lot::Mutex;

/// A value per worker of a thread pool, created on its worker on first use and dropped on its
/// worker with [`Self::clear_current`]. The value does not have to be `Send`, so thread-affine
/// state (such as an mCRL2 rewriter context) can be kept for the lifetime of the pool.
///
/// There is one slot per worker, see [`Self::new`]. A thread claims a free slot on its first use and
/// only ever accesses that slot, so a value is never shared between threads. Re-entrant use from the
/// same thread panics (the closure must not run other work of the pool while it holds the value).
///
/// A value whose closure panicked stays in its slot, so it is still dropped on its own thread.
pub struct WorkerLocal<T> {
    slots: Box<[Slot<T>]>,
}

struct Slot<T> {
    /// The thread that owns the value, set when the slot is claimed.
    owner: OnceLock<ThreadId>,
    /// Only locked by `owner`; the lock detects re-entrant use. It does not poison, so a panic in a
    /// closure leaves the value in place.
    value: Mutex<Option<T>>,
}

// SAFETY: a slot's value is only accessed by the thread recorded in its `owner` (`slot` only returns
// the slot of the calling thread), and it is never moved out to another thread (`Drop` leaks what is
// left), so a `T` that is not `Send` stays on its thread.
unsafe impl<T> Sync for WorkerLocal<T> {}

impl<T> WorkerLocal<T> {
    /// Creates an empty slot for each of `workers` threads.
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

    /// Runs `f` with the value of the calling thread, creating the value with `init` first if there
    /// is none yet.
    ///
    /// # Panics
    ///
    /// If more threads use this than it has slots, or on re-entrant use.
    pub fn with<R, I, F>(&self, init: I, f: F) -> R
    where
        I: FnOnce() -> T,
        F: FnOnce(&mut T) -> R,
    {
        let slot = self
            .slot(thread::current().id())
            .expect("WorkerLocal used from more threads than it has slots");
        let mut value = slot
            .value
            .try_lock()
            .expect("re-entrant use of a WorkerLocal value on the same worker");
        f(value.get_or_insert_with(init))
    }

    /// Returns the slot of thread `me`, claiming a free slot if it has none yet, or `None` when
    /// every slot belongs to another thread.
    fn slot(&self, me: ThreadId) -> Option<&Slot<T>> {
        // A slot that `me` claimed comes before every free slot, since `me` claims the first free
        // slot it finds; so the first slot that is owned by `me` or can be claimed by it is the one.
        self.slots.iter().find(|slot| *slot.owner.get_or_init(|| me) == me)
    }

    /// Drops the value of the calling thread, if it owns one. Call this on every worker (e.g. in
    /// a broadcast) before the `WorkerLocal` is dropped.
    pub fn clear_current(&self) {
        let me = thread::current().id();
        for slot in self.slots.iter() {
            if slot.owner.get() == Some(&me) {
                drop(slot.value.lock().take());
            }
        }
    }
}

impl<T> Drop for WorkerLocal<T> {
    fn drop(&mut self) {
        // Values that `clear_current` did not drop are leaked: dropping a thread-affine value on
        // this thread could be unsound.
        for slot in self.slots.iter_mut() {
            if let Some(value) = slot.value.get_mut().take() {
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
                        assert_eq!(local.with(|| Rc::new(index), |value| **value), index);
                    }
                    local.clear_current();
                });
            }
        });
    }

    #[test]
    #[should_panic(expected = "more threads than it has slots")]
    fn test_worker_local_too_many_threads() {
        let local: WorkerLocal<usize> = WorkerLocal::new(1);
        local.with(|| 1, |_| ());
        std::thread::scope(|s| {
            s.spawn(|| local.with(|| 2, |_| ()))
                .join()
                .unwrap_or_else(|e| std::panic::resume_unwind(e));
        });
    }

    /// A value whose `with` closure panicked must still never be dropped on a
    /// thread other than the one that created it, also not by `Drop` of the `WorkerLocal`.
    #[test]
    fn test_worker_local_poisoned_value_is_not_dropped_on_another_thread() {
        use std::marker::PhantomData;
        use std::sync::Arc;

        /// Not `Send`; records the thread it is dropped on.
        struct Affine {
            dropped_on: Arc<std::sync::Mutex<Option<ThreadId>>>,
            _not_send: PhantomData<*const ()>,
        }

        impl Drop for Affine {
            fn drop(&mut self) {
                *self.dropped_on.lock().unwrap() = Some(thread::current().id());
            }
        }

        let dropped_on = Arc::new(std::sync::Mutex::new(None));
        let local: WorkerLocal<Affine> = WorkerLocal::new(1);
        std::thread::scope(|s| {
            let result = s
                .spawn(|| {
                    local.with(
                        || Affine {
                            dropped_on: dropped_on.clone(),
                            _not_send: PhantomData,
                        },
                        |_| panic!("the closure panics while it holds the value"),
                    )
                })
                .join();
            assert!(result.is_err());
        });

        drop(local);
        assert_ne!(
            *dropped_on.lock().unwrap(),
            Some(thread::current().id()),
            "a thread-affine value was dropped on a thread that does not own it"
        );
    }

    /// `clear_current` is called from `Drop` impls (e.g. in a broadcast while unwinding), so it must
    /// not panic when an earlier use of the value panicked.
    #[test]
    fn test_worker_local_clear_current_after_panic_does_not_panic() {
        let local: WorkerLocal<usize> = WorkerLocal::new(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            local.with(|| 1, |_| panic!("the closure panics while it holds the value"))
        }));
        assert!(result.is_err());

        let cleared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| local.clear_current()));
        assert!(cleared.is_ok(), "clear_current panicked after a panic in a closure");
    }
}
