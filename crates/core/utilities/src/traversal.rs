use std::ops::ControlFlow;

/// What a traversal does below the node that its callback has just seen.
///
/// `N` is the type of a replacement node. Instantiating `N = Infallible` makes [Step::Replace]
/// impossible to construct, which is how the read-only traversals rule substitution out without
/// needing a second enum.
///
/// Terms and syntax trees are often maximally shared or repeated, so a callback whose work per
/// node is not trivial can remember the nodes it has already seen and return [Step::Prune] for
/// the repeats, which keeps the traversal linear in the size of the graph rather than of the
/// tree it unfolds to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Step<N, C> {
    /// Replace the node with `N`. The replacement is deliberately *not* descended into, since a
    /// callback that rewrites a node into something containing that same node would otherwise
    /// never terminate. Recurse explicitly on the replacement when that is wanted.
    Replace(N),
    /// Keep the node and descend into its children, carrying this context.
    Into(C),
    /// Keep the node but skip its children.
    Prune,
}

/// The result of visiting a single node: fail with `E`, stop the traversal with `T`, or continue
/// with a [Step].
///
/// The context `C` is threaded from a node to its children, which lets a traversal track where it
/// is without maintaining a stack of its own. It is required to be `Copy`: a `Clone` context is
/// cloned at every node, which is a performance trap dressed up as flexibility. State that grows
/// along a path, such as the variables bound above the current node, belongs in the callback
/// itself (push on entry, truncate on exit) or in a `Copy` slice.
pub type Visit<N, C, T, E> = Result<ControlFlow<T, Step<N, C>>, E>;
