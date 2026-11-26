use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// An [`Arc`] that cannot be cloned through a shared reference, and is therefore `Sync` whenever
/// `T` is, even when `T` is not `Send`.
///
/// An `Arc<T>` is only `Sync` when `T: Send + Sync`: a thread holding `&Arc<T>` could clone it
/// and drop the last reference there, and with it `T`, on the wrong thread. A `NoCloneArc` only
/// makes a new reference with [`NoCloneArc::share`], which takes `&mut self`. Another thread that
/// only holds a `&NoCloneArc<T>` gets a `&T` and nothing else, so `T` is never dropped there.
///
/// This is for values that are `Sync` but have to stay on one thread, such as an LPS backed by
/// mCRL2, and that are shared by several owners which all live on that thread.
pub struct NoCloneArc<T: ?Sized> {
    inner: Arc<T>,
}

impl<T> NoCloneArc<T> {
    /// Wraps `value` in a new reference counted allocation.
    pub fn new(value: T) -> Self {
        Self { inner: Arc::new(value) }
    }
}

impl<T: ?Sized> NoCloneArc<T> {
    /// Returns a new reference to the same value.
    ///
    /// This takes `&mut self` rather than `&self`, which is what makes `NoCloneArc` `Sync`: a
    /// thread can only call it when it has exclusive access, which it only gets for a value that it
    /// owns, or that was sent to it, which requires `NoCloneArc<T>: Send` and thus `T: Send`.
    pub fn share(&mut self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

// SAFETY: a `&NoCloneArc<T>` only gives access to a `&T`, which is fine to share when `T: Sync`.
// It does not give a way to create or drop a reference, see `share`, so `T` is only dropped on a
// thread that owns a `NoCloneArc<T>`. A `NoCloneArc<T>` only moves to another thread when it is
// `Send`, which (through `Arc<T>`) requires `T: Send + Sync`.
unsafe impl<T: ?Sized + Sync> Sync for NoCloneArc<T> {}

impl<T: ?Sized> Deref for NoCloneArc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: ?Sized> AsRef<T> for NoCloneArc<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for NoCloneArc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::marker::PhantomData;

    use super::NoCloneArc;

    /// `Sync`, but not `Send`, like an LPS that has to stay on its thread.
    struct ThreadAffine {
        value: usize,
        _not_send: PhantomData<*const ()>,
    }

    // SAFETY: only read through `&self`.
    unsafe impl Sync for ThreadAffine {}

    fn assert_sync<T: Sync>(_: &T) {}

    #[test]
    fn test_no_clone_arc_shared_reads() {
        let mut first = NoCloneArc::new(ThreadAffine {
            value: 42,
            _not_send: PhantomData,
        });
        let second = first.share();
        assert_sync(&second);

        let sum = Cell::new(0);
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| second.value);
            sum.set(first.value + handle.join().unwrap());
        });
        assert_eq!(sum.get(), 84);
    }
}
