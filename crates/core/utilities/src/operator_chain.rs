use std::collections::HashSet;
use std::hash::Hash;
use std::iter::FusedIterator;

use crate::recycle_vec::recycle_vec;

/// A handle to a tree node that may be an application of a binary connective, so that nested
/// applications of the same connective form a chain `a op b op c ...` that [ChainIter] flattens.
///
/// Implemented on cheap handles, such as `&T` or term references, rather than on the nodes
/// themselves, so that operands can be handed out with the lifetime of the whole tree.
pub trait BinaryChain: Clone {
    /// The connective.
    type Op: Copy;

    /// Returns the operands of this node if it is an application of `op`.
    fn binary_operands(&self, op: Self::Op) -> Option<(Self, Self)>;
}

/// Reusable storage for the worklist of a [ChainIter], so that repeated traversals do not
/// allocate.
///
/// The storage is empty between traversals, so `U` need not be the handle type of any particular
/// traversal: a long-lived context can keep, for example, a `ChainStack<TermRef<'static>>` and
/// lend it to traversals of shorter-lived terms. `U` must have the same size and alignment as the
/// handle type, so that the allocation can be reused; [ChainIter::new] checks this at compile time.
pub struct ChainStack<U> {
    stack: Vec<U>,
}

impl<U> ChainStack<U> {
    pub fn new() -> Self {
        ChainStack { stack: Vec::new() }
    }
}

impl<U> Default for ChainStack<U> {
    fn default() -> Self {
        ChainStack::new()
    }
}

/// Yields every operand of a nested `op` chain, in the order in which they are written. A node
/// that is not an application of `op` is a chain of one.
///
/// The walk uses an explicit stack rather than recursion, so that very deep chains, such as those
/// produced by quantifier enumeration, cannot overflow the call stack.
pub struct ChainIter<'s, T: BinaryChain, U> {
    stack: Vec<T>,
    op: T::Op,

    /// Receives the allocation of `stack` back when the traversal ends.
    storage: &'s mut ChainStack<U>,
}

impl<'s, T: BinaryChain, U> ChainIter<'s, T, U> {
    /// Starts a traversal of the `op` chain rooted at `root`, using the allocation of `storage`.
    pub fn new(root: T, op: T::Op, storage: &'s mut ChainStack<U>) -> Self {
        const {
            assert!(
                size_of::<T>() == size_of::<U>() && align_of::<T>() == align_of::<U>(),
                "the storage element type must have the layout of the handle type"
            );
        }

        let mut stack = recycle_vec(std::mem::take(&mut storage.stack));
        stack.push(root);

        ChainIter { stack, op, storage }
    }
}

impl<T: BinaryChain, U> Iterator for ChainIter<'_, T, U> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        while let Some(node) = self.stack.pop() {
            match node.binary_operands(self.op) {
                Some((lhs, rhs)) => {
                    // Right operand first, so that the left one is popped first.
                    self.stack.push(rhs);
                    self.stack.push(lhs);
                }
                None => return Some(node),
            }
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Every node on the stack yields at least one operand.
        (self.stack.len(), None)
    }
}

impl<T: BinaryChain, U> FusedIterator for ChainIter<'_, T, U> {}

impl<T: BinaryChain, U> Drop for ChainIter<'_, T, U> {
    fn drop(&mut self) {
        self.storage.stack = recycle_vec(std::mem::take(&mut self.stack));
    }
}

/// The operands of the `op` chain rooted at `root`, like [ChainIter], but each one only once, in
/// the order of its first occurrence.
///
/// Every node is visited at most once, including the inner nodes of the chain: on maximally
/// shared terms the operands of a shared subchain are then expanded once rather than at every
/// occurrence, which would be exponential for nested sharing.
pub fn unique_chain_operands<T: BinaryChain + Hash + Eq>(root: T, op: T::Op) -> Vec<T> {
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while let Some(node) = stack.pop() {
        if !seen.insert(node.clone()) {
            continue;
        }

        match node.binary_operands(op) {
            Some((lhs, rhs)) => {
                stack.push(rhs);
                stack.push(lhs);
            }
            None => result.push(node),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::BinaryChain;
    use super::ChainIter;
    use super::ChainStack;
    use super::unique_chain_operands;

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Op {
        And,
        Or,
    }

    #[derive(PartialEq, Eq, Hash)]
    enum Expr {
        Var(&'static str),
        Binary(Op, Box<Expr>, Box<Expr>),
    }

    impl<'a> BinaryChain for &'a Expr {
        type Op = Op;

        fn binary_operands(&self, op: Op) -> Option<(&'a Expr, &'a Expr)> {
            match *self {
                Expr::Binary(node_op, lhs, rhs) if *node_op == op => Some((lhs, rhs)),
                _ => None,
            }
        }
    }

    fn binary(op: Op, lhs: Expr, rhs: Expr) -> Expr {
        Expr::Binary(op, Box::new(lhs), Box::new(rhs))
    }

    fn names<'a>(operands: impl Iterator<Item = &'a Expr>) -> Vec<&'static str> {
        operands
            .map(|operand| match operand {
                Expr::Var(name) => *name,
                Expr::Binary(..) => "<binary>",
            })
            .collect()
    }

    #[test]
    fn test_chain_iter_yields_operands_in_written_order() {
        let left_nested = binary(Op::And, binary(Op::And, Expr::Var("A"), Expr::Var("B")), Expr::Var("C"));
        let right_nested = binary(Op::And, Expr::Var("A"), binary(Op::And, Expr::Var("B"), Expr::Var("C")));

        let mut storage = ChainStack::<&Expr>::new();
        assert_eq!(
            names(ChainIter::new(&left_nested, Op::And, &mut storage)),
            ["A", "B", "C"]
        );
        assert_eq!(
            names(ChainIter::new(&right_nested, Op::And, &mut storage)),
            ["A", "B", "C"]
        );
    }

    #[test]
    fn test_chain_iter_stops_at_other_connectives() {
        let expr = binary(Op::And, Expr::Var("A"), binary(Op::Or, Expr::Var("B"), Expr::Var("C")));

        let mut storage = ChainStack::<&Expr>::new();
        assert_eq!(names(ChainIter::new(&expr, Op::And, &mut storage)), ["A", "<binary>"]);
        assert_eq!(ChainIter::new(&expr, Op::Or, &mut storage).count(), 1);
    }

    #[test]
    fn test_chain_stack_keeps_allocation_across_lifetimes() {
        let mut storage = ChainStack::<&'static Expr>::new();

        let expr = binary(Op::And, Expr::Var("A"), binary(Op::And, Expr::Var("B"), Expr::Var("C")));
        ChainIter::new(&expr, Op::And, &mut storage).for_each(drop);
        let capacity = storage.stack.capacity();
        assert!(capacity > 0);

        let other = binary(Op::And, Expr::Var("D"), Expr::Var("E"));
        assert_eq!(names(ChainIter::new(&other, Op::And, &mut storage)), ["D", "E"]);
        assert_eq!(storage.stack.capacity(), capacity);
    }

    #[test]
    fn test_chain_iter_size_hint_is_a_lower_bound() {
        let expr = binary(Op::And, Expr::Var("A"), binary(Op::And, Expr::Var("B"), Expr::Var("C")));

        let mut storage = ChainStack::<&Expr>::new();
        let mut iter = ChainIter::new(&expr, Op::And, &mut storage);
        assert_eq!(iter.size_hint(), (1, None));

        iter.next();
        assert_eq!(iter.size_hint(), (1, None));
        iter.next();
        iter.next();
        assert_eq!(iter.size_hint(), (0, None));
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_unique_chain_operands_skips_duplicates() {
        let expr = binary(
            Op::And,
            binary(Op::And, Expr::Var("A"), Expr::Var("B")),
            binary(Op::And, Expr::Var("A"), binary(Op::Or, Expr::Var("B"), Expr::Var("C"))),
        );

        assert_eq!(
            names(unique_chain_operands(&expr, Op::And).into_iter()),
            ["A", "B", "<binary>"]
        );
    }
}
