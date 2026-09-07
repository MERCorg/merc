use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ops::Index;
use std::ops::IndexMut;
use std::slice::SliceIndex;

/// A trait for index types.
pub trait MercIndex: Copy + PartialEq {
    /// The underlying target type.
    type Target;

    /// Returns the underlying index.
    fn index(&self) -> Self::Target;
}

impl MercIndex for () {
    type Target = ();

    fn index(&self) -> Self::Target {}
}

/// A newtype index wrapping an inner value of type `T`, distinguished at compile time by `Tag`.
/// Two `TagIndex` values compare, order, and hash together only when they share both `T` and
/// `Tag`, so indices from different domains (e.g. state, action, priority) cannot be mixed up or
/// compared with each other. `new` accepts any `T` unconditionally, so a `TagIndex` is not by
/// itself a proof that the wrapped value is a valid index into its domain.
///
/// Implements the traits typically needed for indices (`PartialEq`, `Eq`, `PartialOrd`, `Ord`,
/// `Hash`) but not arithmetic, since indices are not naturally added or subtracted. Implements
/// `Index`/`IndexMut` on `Vec<U>`/`[U]` for direct indexing; `value()` returns the underlying `T`.
pub struct TagIndex<T, Tag> {
    index: T,

    /// Ensures that the Tag is used by the struct
    marker: PhantomData<fn() -> Tag>,
}

impl<T: Copy + PartialEq, Tag> MercIndex for TagIndex<T, Tag> {
    type Target = T;

    fn index(&self) -> Self::Target {
        self.index
    }
}

impl<T: Default, Tag> Default for TagIndex<T, Tag> {
    fn default() -> Self {
        Self {
            index: T::default(),
            marker: PhantomData,
        }
    }
}

impl<T: Eq, Tag> Eq for TagIndex<T, Tag> {}

impl<T: PartialEq, Tag> PartialEq for TagIndex<T, Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T: Ord, Tag> Ord for TagIndex<T, Tag> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl<T: PartialOrd, Tag> PartialOrd for TagIndex<T, Tag> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.index.partial_cmp(&other.index)
    }
}

impl<T: Hash, Tag> Hash for TagIndex<T, Tag> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<T: Clone, Tag> Clone for TagIndex<T, Tag> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            marker: self.marker,
        }
    }
}

impl<T: PartialEq, Tag> PartialEq<T> for TagIndex<T, Tag> {
    fn eq(&self, other: &T) -> bool {
        self.index.eq(other)
    }
}

impl<T: PartialOrd, Tag> PartialOrd<T> for TagIndex<T, Tag> {
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        self.index.partial_cmp(other)
    }
}

impl<T: Copy, Tag> Copy for TagIndex<T, Tag> {}

impl<T, Tag> TagIndex<T, Tag> {
    pub fn new(index: T) -> Self {
        Self {
            index,
            marker: PhantomData,
        }
    }
}

impl<T: Copy, Tag> TagIndex<T, Tag> {
    /// Returns the underlying index value.
    pub fn value(&self) -> T {
        self.index
    }
}

impl<T: fmt::Debug, Tag> fmt::Debug for TagIndex<T, Tag> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.index.fmt(f)
    }
}

impl<T: fmt::Display, Tag> fmt::Display for TagIndex<T, Tag> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.index.fmt(f)
    }
}

// Convenient traits for using the `TagIndex`.
impl<T: Copy + SliceIndex<[U], Output = U>, U, Tag> Index<TagIndex<T, Tag>> for Vec<U> {
    type Output = U;

    fn index(&self, index: TagIndex<T, Tag>) -> &Self::Output {
        &self[index.value()]
    }
}

impl<T: Copy + SliceIndex<[U], Output = U>, U, Tag> Index<TagIndex<T, Tag>> for [U] {
    type Output = U;

    fn index(&self, index: TagIndex<T, Tag>) -> &Self::Output {
        &self[index.value()]
    }
}

impl<T: Copy + SliceIndex<[U], Output = U>, U, Tag> IndexMut<TagIndex<T, Tag>> for Vec<U> {
    fn index_mut(&mut self, index: TagIndex<T, Tag>) -> &mut Self::Output {
        &mut self[index.value()]
    }
}

impl<T: Copy + SliceIndex<[U], Output = U>, U, Tag> IndexMut<TagIndex<T, Tag>> for [U] {
    fn index_mut(&mut self, index: TagIndex<T, Tag>) -> &mut Self::Output {
        &mut self[index.value()]
    }
}

impl<T, Tag> Deref for TagIndex<T, Tag> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

/// Hands out consecutive `TagIndex<usize, Tag>` values, starting at 0.
pub struct IdAllocator<Tag> {
    next: usize,

    marker: PhantomData<fn() -> Tag>,
}

impl<Tag> Default for IdAllocator<Tag> {
    fn default() -> Self {
        Self {
            next: 0,
            marker: PhantomData,
        }
    }
}

impl<Tag> IdAllocator<Tag> {
    /// Returns the next id in the sequence.
    pub fn alloc(&mut self) -> TagIndex<usize, Tag> {
        let id = TagIndex::new(self.next);
        self.next += 1;
        id
    }
}
