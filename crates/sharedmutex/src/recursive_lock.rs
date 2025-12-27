//! Authors: Maurice Laveaux, Flip van Spaendonck and Jan Friso Groote

use std::cell::Cell;
use std::error::Error;
use std::mem;
use std::ops::Deref;
use std::ops::DerefMut;

use crate::BfSharedMutex;
use crate::BfSharedMutexReadGuard;
use crate::BfSharedMutexWriteGuard;

/// An extension of the [BfSharedMutex] that allows recursive read locking without deadlocks.
pub struct RecursiveLock<T> {
    inner: BfSharedMutex<T>,

    /// The number of times the current thread has read locked the mutex.
    recursive_depth: Cell<usize>,

    /// The number of calls to the write() method.
    write_calls: Cell<usize>,

    /// The number of calls to the read_recursive() method.
    read_recursive_calls: Cell<usize>,
}

impl<T> RecursiveLock<T> {
    /// Creates a new `RecursiveLock` with the given data.
    pub fn new(data: T) -> Self {
        RecursiveLock {
            inner: BfSharedMutex::new(data),
            recursive_depth: Cell::new(0),
            write_calls: Cell::new(0),
            read_recursive_calls: Cell::new(0),
        }
    }

    /// Creates a new `RecursiveLock` from an existing `BfSharedMutex`.
    pub fn from_mutex(mutex: BfSharedMutex<T>) -> Self {
        RecursiveLock {
            inner: mutex,
            recursive_depth: Cell::new(0),
            write_calls: Cell::new(0),
            read_recursive_calls: Cell::new(0),
        }
    }

    delegate::delegate! {
        to self.inner {
            pub fn data_ptr(&self) -> *const T;
            pub fn is_locked(&self) -> bool;
            pub fn is_locked_exclusive(&self) -> bool;
        }
    }

    /// Acquires a write lock on the mutex.
    pub fn write(&self) -> Result<RecursiveLockWriteGuard<'_, T>, Box<dyn Error + '_>> {
        debug_assert!(
            self.recursive_depth.get() == 0,
            "Cannot call write() inside a read section"
        );
        self.write_calls.set(self.write_calls.get() + 1);
        self.recursive_depth.set(1);
        Ok(RecursiveLockWriteGuard {
            mutex: self,
            guard: self.inner.write()?,
        })
    }

    /// Acquires a read lock on the mutex.
    pub fn read(&self) -> Result<BfSharedMutexReadGuard<'_, T>, Box<dyn Error + '_>> {
        debug_assert!(
            self.recursive_depth.get() == 0,
            "Cannot call read() inside a read section"
        );
        self.inner.read()
    }

    /// Acquires a read lock on the mutex, allowing for recursive read locking.
    pub fn read_recursive<'a>(&'a self) -> Result<RecursiveLockReadGuard<'a, T>, Box<dyn Error + 'a>> {
        self.read_recursive_calls.set(self.read_recursive_calls.get() + 1);
        if self.recursive_depth.get() == 0 {
            // If we are not already holding a read lock, we acquire one.
            // Acquire the read guard, but forget it to prevent it from being dropped.
            self.recursive_depth.set(1);
            mem::forget(self.inner.read());
            Ok(RecursiveLockReadGuard { mutex: self })
        } else {
            // If we are already holding a read lock, we just increment the depth.
            self.recursive_depth.set(self.recursive_depth.get() + 1);
            Ok(RecursiveLockReadGuard { mutex: self })
        }
    }

    /// Returns the number of times `write()` has been called.
    pub fn write_call_count(&self) -> usize {
        self.write_calls.get()
    }

    /// Returns the number of times `read_recursive()` has been called.
    pub fn read_recursive_call_count(&self) -> usize {
        self.read_recursive_calls.get()
    }
}

#[must_use = "Dropping the guard unlocks the recursive lock immediately"]
pub struct RecursiveLockReadGuard<'a, T> {
    mutex: &'a RecursiveLock<T>,
}

/// Allow dereferences the underlying object.
impl<T> Deref for RecursiveLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // There can only be shared guards, which only provide immutable access to the object.
        unsafe { self.mutex.inner.data_ptr().as_ref().unwrap_unchecked() }
    }
}

impl<T> Drop for RecursiveLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.recursive_depth.set(self.mutex.recursive_depth.get() - 1);
        if self.mutex.recursive_depth.get() == 0 {
            // If we are not holding a read lock anymore, we release the mutex.
            // This will allow other threads to acquire a read lock.
            unsafe {
                self.mutex.inner.create_read_guard_unchecked();
            }
        }
    }
}

#[must_use = "Dropping the guard unlocks the recursive lock immediately"]
pub struct RecursiveLockWriteGuard<'a, T> {
    mutex: &'a RecursiveLock<T>,
    guard: BfSharedMutexWriteGuard<'a, T>,
}

/// Allow dereferences the underlying object.
impl<T> Deref for RecursiveLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // There can only be shared guards, which only provide immutable access to the object.
        self.guard.deref()
    }
}

/// Allow dereferences the underlying object.
impl<T> DerefMut for RecursiveLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // There can only be shared guards, which only provide immutable access to the object.
        self.guard.deref_mut()
    }
}

impl<T> Drop for RecursiveLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.recursive_depth.set(self.mutex.recursive_depth.get() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_mutex() {
        let mutex = BfSharedMutex::new(100);
        let lock = RecursiveLock::from_mutex(mutex);
        assert_eq!(*lock.read().unwrap(), 100);
    }

    #[test]
    fn test_single_recursive_read() {
        let lock = RecursiveLock::new(42);
        let guard = lock.read_recursive().unwrap();
        assert_eq!(*guard, 42);
        assert_eq!(lock.recursive_depth.get(), 1);
    }

    #[test]
    fn test_nested_recursive_reads() {
        let lock = RecursiveLock::new(42);

        let guard1 = lock.read_recursive().unwrap();
        assert_eq!(*guard1, 42);
        assert_eq!(lock.recursive_depth.get(), 1);

        let guard2 = lock.read_recursive().unwrap();
        assert_eq!(*guard2, 42);
        assert_eq!(lock.recursive_depth.get(), 2);

        let guard3 = lock.read_recursive().unwrap();
        assert_eq!(*guard3, 42);
        assert_eq!(lock.recursive_depth.get(), 3);

        drop(guard3);
        assert_eq!(lock.recursive_depth.get(), 2);

        drop(guard2);
        assert_eq!(lock.recursive_depth.get(), 1);

        drop(guard1);
        assert_eq!(lock.recursive_depth.get(), 0);
    }

    #[test]
    fn test_write_call_counter() {
        let lock = RecursiveLock::new(42);

        // Initially, the counter should be 0
        assert_eq!(lock.write_call_count(), 0);

        // After one write call, counter should be 1
        {
            let _guard = lock.write().unwrap();
            assert_eq!(lock.write_call_count(), 1);
        }

        // After another write call, counter should be 2
        {
            let _guard = lock.write().unwrap();
            assert_eq!(lock.write_call_count(), 2);
        }

        // Counter should remain 2
        assert_eq!(lock.write_call_count(), 2);
    }

    #[test]
    fn test_read_recursive_call_counter() {
        let lock = RecursiveLock::new(42);

        // Initially, the counter should be 0
        assert_eq!(lock.read_recursive_call_count(), 0);

        // After one read_recursive call, counter should be 1
        {
            let _guard = lock.read_recursive().unwrap();
            assert_eq!(lock.read_recursive_call_count(), 1);
        }

        // After another read_recursive call, counter should be 2
        {
            let _guard = lock.read_recursive().unwrap();
            assert_eq!(lock.read_recursive_call_count(), 2);
        }

        // Test nested recursive reads increment the counter
        {
            let _guard1 = lock.read_recursive().unwrap();
            assert_eq!(lock.read_recursive_call_count(), 3);

            let _guard2 = lock.read_recursive().unwrap();
            assert_eq!(lock.read_recursive_call_count(), 4);
        }

        // Counter should remain 4
        assert_eq!(lock.read_recursive_call_count(), 4);
    }

    #[test]
    fn test_both_counters() {
        let lock = RecursiveLock::new(42);

        // Initially, both counters should be 0
        assert_eq!(lock.write_call_count(), 0);
        assert_eq!(lock.read_recursive_call_count(), 0);

        // Call write and check counters
        {
            let _guard = lock.write().unwrap();
            assert_eq!(lock.write_call_count(), 1);
            assert_eq!(lock.read_recursive_call_count(), 0);
        }

        // Call read_recursive and check counters
        {
            let _guard = lock.read_recursive().unwrap();
            assert_eq!(lock.write_call_count(), 1);
            assert_eq!(lock.read_recursive_call_count(), 1);
        }

        // Call write again
        {
            let _guard = lock.write().unwrap();
            assert_eq!(lock.write_call_count(), 2);
            assert_eq!(lock.read_recursive_call_count(), 1);
        }

        // Call read_recursive multiple times
        {
            let _guard1 = lock.read_recursive().unwrap();
            let _guard2 = lock.read_recursive().unwrap();
            assert_eq!(lock.write_call_count(), 2);
            assert_eq!(lock.read_recursive_call_count(), 3);
        }
    }
}
