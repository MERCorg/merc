use std::collections::HashMap;

use crate::PbesExpression;
use crate::PbesExpressionRef;
use crate::PbesFold;
use crate::PbesKind;
use crate::fold_pbes_expr;
use crate::is_counter_example_name;

/// Returns true iff `expr` is free of propositional variables. If `allow_ce` is true,
/// counter-example propositional variables are not counted.
pub fn is_simple_expression(expr: &PbesExpressionRef<'_>, allow_ce: bool) -> bool {
    !expr.any(|node| match node.kind() {
        PbesKind::Instantiation(inst) => !allow_ce || !is_counter_example_name(inst.name().value()),
        _ => false,
    })
}

/// Memoises [is_simple_expression] for every subexpression it is asked about: without it, heavily nested formulas re-traverse the same
/// subtrees at every level of the conversion, which is quadratic. Expressions are maximally
/// shared, so the cache also answers for the subexpressions that spawned equations and other
/// equations have in common.
#[derive(Default)]
pub(crate) struct SimpleExpressionCache {
    /// Indexed by `allow_ce`.
    cache: [HashMap<PbesExpression, bool>; 2],
}

impl SimpleExpressionCache {
    pub(crate) fn is_simple(&mut self, expr: &PbesExpressionRef<'_>, allow_ce: bool) -> bool {
        fold_pbes_expr(expr, &mut self.cache[usize::from(allow_ce)], |_, node| match node {
            PbesFold::Instantiation(inst) => allow_ce && is_counter_example_name(inst.name().value()),
            PbesFold::True | PbesFold::False | PbesFold::Data(_) => true,
            PbesFold::Not(body) | PbesFold::Forall(_, body) | PbesFold::Exists(_, body) => body,
            PbesFold::And(lhs, rhs) | PbesFold::Or(lhs, rhs) | PbesFold::Imp(lhs, rhs) => lhs && rhs,
        })
    }
}
