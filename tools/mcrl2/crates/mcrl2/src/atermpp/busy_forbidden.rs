#[cfg(debug_assertions)]
use std::cell::Cell;
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ops::DerefMut;

use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_lock_exclusive;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_lock_shared;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_unlock_exclusive;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_unlock_shared;
use merc_utilities::PhantomUnsend;

/// Provides access to the mCRL2 busy forbidden protocol, where there
/// are thread local busy flags and one central storage for the forbidden
/// flags.
///
/// # Deadlock Warning
///
/// Care must be taken to avoid deadlocks since the FFI also uses the same flags.
/// In particular, **do not call FFI functions that may acquire busy/forbidden flags
/// while holding a lock (read or write) on a `BfTermPool`**. This can result in a
/// deadlock if the FFI function attempts to acquire a lock that is already held by
/// the current thread.
pub(crate) struct BfTermPool<T: ?Sized> {
    object: UnsafeCell<T>,
}

// SAFETY: the `UnsafeCell` is only ever accessed behind the busy/forbidden
// protocol, so sending the pool to another thread is sound whenever `T` itself
// is `Send`.
unsafe impl<T: Send> Send for BfTermPool<T> {}
// SAFETY: concurrent `&self` access goes through the busy/forbidden lock, which
// serialises writers against readers, so sharing is sound when `T` is `Send + Sync`.
unsafe impl<T: Send + Sync> Sync for BfTermPool<T> {}

impl<T> BfTermPool<T> {
    pub fn new(object: T) -> BfTermPool<T> {
        BfTermPool {
            object: UnsafeCell::new(object),
        }
    }
}

impl<'a, T: ?Sized> BfTermPool<T> {
    /// Provides read access to the underlying object.
    pub fn read(&'a self) -> BfTermPoolRead<'a, T> {
        mcrl2_aterm_pool_lock_shared();
        BfTermPoolRead {
            mutex: self,
            _marker: Default::default(),
        }
    }

    /// Provides write access to the underlying object.
    pub fn write(&'a self) -> BfTermPoolWrite<'a, T> {
        let exclusive = ExclusiveSection::enter();
        mcrl2_aterm_pool_lock_exclusive();

        BfTermPoolWrite {
            mutex: self,
            _exclusive: exclusive,
            _marker: Default::default(),
        }
    }

    /// Provides read access to the underlying object.
    ///
    /// # Safety
    ///
    /// Assumes that we are in an exclusive section.
    pub unsafe fn get(&'a self) -> &'a T {
        unsafe { &*self.object.get() }
    }

    /// Provides write access to the underlying object
    ///
    /// # Safety
    ///
    /// Provides mutable access given that other threads use [Self::write] and [Self::read]
    /// exclusively. If we are already in an exclusive context then lock can be
    /// set to false.
    pub unsafe fn write_exclusive(&'a self) -> BfTermPoolThreadWrite<'a, T> {
        // This is a lock shared, but assuming that only ONE thread uses this function.
        mcrl2_aterm_pool_lock_shared();

        BfTermPoolThreadWrite {
            mutex: self,
            locked: true,
            _marker: Default::default(),
        }
    }
}

pub(crate) struct BfTermPoolRead<'a, T: ?Sized> {
    mutex: &'a BfTermPool<T>,
    _marker: PhantomData<&'a ()>,
}

impl<T: ?Sized> Deref for BfTermPoolRead<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: this guard holds the shared (read) lock, so concurrent readers
        // are allowed but no writer can be active; an immutable borrow is sound.
        unsafe { &*self.mutex.object.get() }
    }
}

impl<T: ?Sized> Drop for BfTermPoolRead<'_, T> {
    fn drop(&mut self) {
        // If we leave the shared section and the counter is zero.
        mcrl2_aterm_pool_unlock_shared();
    }
}

pub(crate) struct BfTermPoolWrite<'a, T: ?Sized> {
    mutex: &'a BfTermPool<T>,
    /// Dropped after `Drop::drop` has left the exclusive section.
    _exclusive: ExclusiveSection,
    _marker: PhantomData<&'a ()>,
}

#[cfg(debug_assertions)]
thread_local! {
    static IN_EXCLUSIVE_SECTION: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as being inside the exclusive section of the aterm pool for its
/// lifetime. The exclusive lock is not reentrant, so in debug builds entering it again panics
/// instead of deadlocking.
pub(crate) struct ExclusiveSection {
    // The flag is thread local, so the section must end on the thread that entered it.
    _unsend: PhantomUnsend,
}

impl ExclusiveSection {
    pub(crate) fn enter() -> ExclusiveSection {
        #[cfg(debug_assertions)]
        IN_EXCLUSIVE_SECTION.with(|in_section| {
            assert!(
                !in_section.replace(true),
                "Can not acquire exclusive lock inside of exclusive section"
            );
        });

        ExclusiveSection {
            _unsend: Default::default(),
        }
    }
}

impl Drop for ExclusiveSection {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        IN_EXCLUSIVE_SECTION.with(|in_section| in_section.set(false));
    }
}

impl<T: ?Sized> Deref for BfTermPoolWrite<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: this guard holds the exclusive (write) lock, so it is the only
        // guard accessing the object; an immutable borrow is sound.
        unsafe { &*self.mutex.object.get() }
    }
}

impl<T: ?Sized> DerefMut for BfTermPoolWrite<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: this guard holds the exclusive (write) lock, so it is the only
        // guard accessing the object; a mutable borrow is sound.
        unsafe { &mut *self.mutex.object.get() }
    }
}

impl<T: ?Sized> Drop for BfTermPoolWrite<'_, T> {
    fn drop(&mut self) {
        mcrl2_aterm_pool_unlock_exclusive();
    }
}

pub(crate) struct BfTermPoolThreadWrite<'a, T: ?Sized> {
    mutex: &'a BfTermPool<T>,
    locked: bool,
    _marker: PhantomData<&'a ()>,
}

impl<T: ?Sized> BfTermPoolThreadWrite<'_, T> {
    /// Unlocks the guard prematurely, but returns whether the shared section was actually left.
    pub fn unlock(&mut self) -> bool {
        if self.locked {
            self.locked = false;
            mcrl2_aterm_pool_unlock_shared()
        } else {
            false
        }
    }
}

impl<T: ?Sized> Deref for BfTermPoolThreadWrite<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: this guard is created under the `write_exclusive` contract that
        // only a single thread uses it, so it is the sole accessor; an immutable
        // borrow is sound.
        unsafe { &*self.mutex.object.get() }
    }
}

impl<T: ?Sized> DerefMut for BfTermPoolThreadWrite<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: this guard is created under the `write_exclusive` contract that
        // only a single thread uses it, so it is the sole accessor; a mutable
        // borrow is sound.
        unsafe { &mut *self.mutex.object.get() }
    }
}

impl<T: ?Sized> Drop for BfTermPoolThreadWrite<'_, T> {
    fn drop(&mut self) {
        if self.locked {
            mcrl2_aterm_pool_unlock_shared();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use merc_utilities::random_test_threads;

    use crate::BfTermPool;

    #[test]
    fn test_random_busy_forbidden_threaded() {
        let pool = Arc::new(BfTermPool::new(0u64));

        random_test_threads(
            100,
            2,
            || pool.clone(),
            |_id, pool| {
                // Test read lock
                {
                    let guard = pool.read();
                    let _value = *guard;
                }

                // Test write lock and increment
                {
                    let mut guard = pool.write();
                    *guard += 1;
                }
            },
        );

        // Verify final count
        let guard = pool.read();
        let final_value = *guard;
        assert_eq!(final_value, 200); // 100 iterations * 2 threads
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Can not acquire exclusive lock inside of exclusive section")]
    fn test_nested_write_panics() {
        let pool = BfTermPool::new(0u64);

        let _guard = pool.write();
        let _nested = pool.write();
    }
}
