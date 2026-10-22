use crate::BinaryChain;
use crate::ChainIter;
use crate::ChainStack;

/// A term language with Boolean connectives, such as data expressions or PBES
/// expressions.
///
/// The `optimized_*` functions below simplify when an operand is a constant, so
/// that algorithms that build formulas incrementally do not accumulate `true &&
/// …` and `!!…`.
pub trait Boolean: Clone + PartialEq {
    fn true_constant() -> Self;
    fn false_constant() -> Self;
    fn is_true(&self) -> bool;
    fn is_false(&self) -> bool;

    /// When `self` is a negation `!x`, its operand `x`.
    fn negated(&self) -> Option<Self>;
    fn make_not(self) -> Self;
    fn make_and(self, rhs: Self) -> Self;
    fn make_or(self, rhs: Self) -> Self;
    fn make_imp(self, rhs: Self) -> Self;
}

/// `!arg`, simplified for constants and double negation.
pub fn optimized_not<T: Boolean>(arg: T) -> T {
    if arg.is_true() {
        T::false_constant()
    } else if arg.is_false() {
        T::true_constant()
    } else if let Some(inner) = arg.negated() {
        inner
    } else {
        arg.make_not()
    }
}

/// `lhs && rhs`, simplified for constants and equal operands.
pub fn optimized_and<T: Boolean>(lhs: T, rhs: T) -> T {
    if lhs.is_true() || rhs.is_false() {
        rhs
    } else if rhs.is_true() || lhs.is_false() || lhs == rhs {
        lhs
    } else {
        lhs.make_and(rhs)
    }
}

/// `lhs || rhs`, simplified for constants and equal operands.
pub fn optimized_or<T: Boolean>(lhs: T, rhs: T) -> T {
    if lhs.is_false() || rhs.is_true() {
        rhs
    } else if rhs.is_false() || lhs.is_true() || lhs == rhs {
        lhs
    } else {
        lhs.make_or(rhs)
    }
}

/// `lhs => rhs`, simplified for constants and equal operands.
pub fn optimized_impies<T: Boolean>(lhs: T, rhs: T) -> T {
    if lhs.is_true() {
        rhs
    } else if lhs.is_false() || rhs.is_true() || lhs == rhs {
        T::true_constant()
    } else if rhs.is_false() {
        optimized_not(lhs)
    } else {
        lhs.make_imp(rhs)
    }
}

/// The conjunction of `terms`, `true` when there are none. Stops at the first `false`.
pub fn optimized_conjunction<T: Boolean, I: IntoIterator<Item = T>>(terms: I) -> T {
    let mut result = T::true_constant();
    for term in terms {
        if term.is_false() {
            return term;
        }

        result = optimized_and(result, term);
    }

    result
}

/// The disjunction of `terms`, `false` when there are none. Stops at the first `true`.
pub fn optimized_disjunction<T: Boolean, I: IntoIterator<Item = T>>(terms: I) -> T {
    let mut result = T::false_constant();
    for term in terms {
        if term.is_true() {
            return term;
        }

        result = optimized_or(result, term);
    }

    result
}

/// A [BinaryChain] handle into a [Boolean] term language, which identifies the connectives that
/// form conjunctions and disjunctions.
pub trait BooleanChain: BinaryChain {
    type Owned: Boolean;

    fn conjunction() -> Self::Op;
    fn disjunction() -> Self::Op;

    /// The term this handle refers to.
    fn into_owned(self) -> Self::Owned;
}

/// Simplifies the conjunction chain rooted at `root` as a whole, rather than only its outermost
/// operands: it is `false` as soon as any operand is, and `true` operands are dropped.
pub fn optimized_conjunction_chain<H: BooleanChain, U>(root: H, storage: &mut ChainStack<U>) -> H::Owned {
    optimized_conjunction(ChainIter::new(root, H::conjunction(), storage).map(H::into_owned))
}

/// Simplifies the disjunction chain rooted at `root` as a whole, rather than only its outermost
/// operands: it is `true` as soon as any operand is, and `false` operands are dropped.
pub fn optimized_disjunction_chain<H: BooleanChain, U>(root: H, storage: &mut ChainStack<U>) -> H::Owned {
    optimized_disjunction(ChainIter::new(root, H::disjunction(), storage).map(H::into_owned))
}

#[cfg(test)]
mod tests {
    use crate::BinaryChain;
    use crate::ChainStack;

    use super::Boolean;
    use super::BooleanChain;
    use super::optimized_conjunction_chain;
    use super::optimized_disjunction_chain;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Op {
        And,
        Or,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Expr {
        True,
        False,
        Var(&'static str),
        Not(Box<Expr>),
        Binary(Op, Box<Expr>, Box<Expr>),
        Imp(Box<Expr>, Box<Expr>),
    }

    impl Boolean for Expr {
        fn true_constant() -> Self {
            Expr::True
        }

        fn false_constant() -> Self {
            Expr::False
        }

        fn is_true(&self) -> bool {
            *self == Expr::True
        }

        fn is_false(&self) -> bool {
            *self == Expr::False
        }

        fn negated(&self) -> Option<Self> {
            match self {
                Expr::Not(inner) => Some((**inner).clone()),
                _ => None,
            }
        }

        fn make_not(self) -> Self {
            Expr::Not(Box::new(self))
        }

        fn make_and(self, rhs: Self) -> Self {
            Expr::Binary(Op::And, Box::new(self), Box::new(rhs))
        }

        fn make_or(self, rhs: Self) -> Self {
            Expr::Binary(Op::Or, Box::new(self), Box::new(rhs))
        }

        fn make_imp(self, rhs: Self) -> Self {
            Expr::Imp(Box::new(self), Box::new(rhs))
        }
    }

    impl<'a> BinaryChain for &'a Expr {
        type Op = Op;

        fn binary_operands(self, op: Op) -> Option<(&'a Expr, &'a Expr)> {
            match self {
                Expr::Binary(node_op, lhs, rhs) if *node_op == op => Some((lhs, rhs)),
                _ => None,
            }
        }
    }

    impl BooleanChain for &Expr {
        type Owned = Expr;

        fn conjunction() -> Op {
            Op::And
        }

        fn disjunction() -> Op {
            Op::Or
        }

        fn into_owned(self) -> Expr {
            self.clone()
        }
    }

    fn var(name: &'static str) -> Expr {
        Expr::Var(name)
    }

    #[test]
    fn test_conjunction_chain_absorbs_nested_false() {
        let expr = var("A").make_and(var("B").make_and(Expr::False.make_and(var("C"))));

        let mut storage = ChainStack::<&Expr>::new();
        assert_eq!(optimized_conjunction_chain(&expr, &mut storage), Expr::False);
    }

    #[test]
    fn test_conjunction_chain_drops_nested_true() {
        let expr = var("A").make_and(Expr::True.make_and(var("B")));

        let mut storage = ChainStack::<&Expr>::new();
        assert_eq!(
            optimized_conjunction_chain(&expr, &mut storage),
            var("A").make_and(var("B"))
        );
    }

    #[test]
    fn test_disjunction_chain_keeps_other_connectives() {
        // The nested conjunction is an operand of the disjunction, so its `false` is not absorbing.
        let conjunction = var("B").make_and(Expr::False);
        let expr = Expr::False.make_or(var("A").make_or(conjunction.clone()));

        let mut storage = ChainStack::<&Expr>::new();
        assert_eq!(
            optimized_disjunction_chain(&expr, &mut storage),
            var("A").make_or(conjunction)
        );
        assert_eq!(
            optimized_disjunction_chain(&var("A").make_or(Expr::True), &mut storage),
            Expr::True
        );
    }
}
