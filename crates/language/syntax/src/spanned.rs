use std::cmp::Ordering;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;
use std::ops::DerefMut;

use merc_utilities::Span;

/// Lets a value detach its own same-type recursive children, one level at a time, in place of
/// letting the compiler's default field-by-field drop glue reach them by native recursion.
pub trait TakeRecursiveChildren: Sized {
    /// Replaces every same-type recursive child of `self` with a cheap, non-recursive value,
    /// pushing the real (possibly still deep) one it replaced onto `stack` instead of returning it,
    /// so a caller can keep detaching layers iteratively without native recursion.
    fn take_recursive_children(&mut self, stack: &mut Vec<Self>) {
        let _ = stack;
    }
}

impl TakeRecursiveChildren for String {}

/// A value of type `T` paired with the source [Span] it originates from.
///
/// Equality, ordering and hashing deliberately ignore the [Span] and consider
/// only `node`, so two structurally identical values at different source
/// locations compare and hash equal. Many passes rely on this structural
/// equality (hash maps, deduplication, `assert_eq!` in tests).
#[derive(Clone, Debug, Default)]
pub struct Spanned<T: TakeRecursiveChildren> {
    /// The wrapped value.
    pub node: T,
    /// The source location the value originates from.
    pub span: Span,
}

impl<T: TakeRecursiveChildren + Default> Spanned<T> {
    /// Splits `self` into its wrapped value and its span, without a partial move.
    pub fn into_parts(mut self) -> (T, Span) {
        (std::mem::take(&mut self.node), std::mem::take(&mut self.span))
    }

    /// Discards the span and returns just the wrapped value; see [Self::into_parts].
    pub fn into_node(self) -> T {
        self.into_parts().0
    }

    /// Transforms the wrapped value while preserving the span.
    pub fn map<U: TakeRecursiveChildren + Default>(self, function: impl FnOnce(T) -> U) -> Spanned<U> {
        let (node, span) = self.into_parts();
        Spanned {
            node: function(node),
            span,
        }
    }
}

/// Wraps `node` together with its source `span`.
pub fn respan<T: TakeRecursiveChildren>(span: Span, node: T) -> Spanned<T> {
    Spanned { node, span }
}

impl<T: TakeRecursiveChildren> Deref for Spanned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl<T: TakeRecursiveChildren> DerefMut for Spanned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.node
    }
}

impl<T: TakeRecursiveChildren + PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl<T: TakeRecursiveChildren + Eq> Eq for Spanned<T> {}

impl<T: TakeRecursiveChildren + PartialOrd> PartialOrd for Spanned<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.node.partial_cmp(&other.node)
    }
}

impl<T: TakeRecursiveChildren + Ord> Ord for Spanned<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.node.cmp(&other.node)
    }
}

impl<T: TakeRecursiveChildren + Hash> Hash for Spanned<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

/// Prevents the compiler's default, field-by-field drop glue from recursing through a
/// `Box`-chained tree one node at a time -- unbounded for a pathologically deep tree, and the
/// reason [TakeRecursiveChildren] exists (see its own doc comment).
impl<T: TakeRecursiveChildren> Drop for Spanned<T> {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        self.node.take_recursive_children(&mut stack);

        while let Some(mut child) = stack.pop() {
            child.take_recursive_children(&mut stack);
        }
    }
}
