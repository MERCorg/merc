#![forbid(unsafe_code)]

use std::ops::ControlFlow;

use merc_data::BoolConnective;
use merc_data::BoolKind;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::visit_data_expr;
use merc_sabre::RewriteEngine;
use merc_sabre::utilities::SliceSubstitution;
use merc_utilities::ChainIter;
use merc_utilities::ChainStack;
use merc_utilities::Step;

use crate::QuantifierKind;
use crate::binding::BindingArena;
use crate::binding::BindingChain;
use crate::binding::BindingGuard;
use crate::binding::SingleSubstitution;

/// Applies [the one-point rule](https://mercorg.github.io/merc-website/developer/rewriting/enumeration/#the-one-point-rule) to a fixpoint,
/// for a quantifier of `kind` over `vars`, or a `sum` as [`QuantifierKind::Exists`].
///
/// The [`Enumerator`](crate::Enumerator) never applies the rule itself, so this is a
/// preprocessing step for the caller.
///
/// `condition` is the goal body to search for one-point (dis)equations over
/// `vars`, rewritten to its residual form in place. `rest` is *not* searched
/// but still has each eliminated variable's binding substituted in. This is
/// useful for summands for example. Returns the variables that were not
/// eliminated, in `vars` order.
///
/// `condition` must already be in normal form, so that the `e` of a matched
/// equation can be substituted without rewriting it again.
pub fn simplify_one_point<R: RewriteEngine>(
    rewriter: &mut R,
    kind: QuantifierKind,
    vars: &[DataVariable],
    condition: &mut DataExpression,
    rest: &mut [DataExpression],
) -> Vec<DataVariable> {
    let mut arena = BindingArena::default();
    arena.chain.write(|guard| {
        let (remaining_vars, residual, bindings) =
            apply_one_point_rule(rewriter, guard, vars.to_vec(), condition.clone(), kind);
        *condition = residual;

        // Every binding's value mentions only variables outside `vars`, so
        // the chain needs no resolving and applies as a plain substitution.
        let eliminated: Vec<DataVariable> = vars.iter().filter(|v| !remaining_vars.contains(v)).cloned().collect();
        let values: Vec<DataExpression> = eliminated
            .iter()
            .map(|v| {
                BindingArena::lookup(guard, bindings, &v.copy())
                    .expect("an eliminated variable is bound")
                    .protect()
            })
            .collect();
        let substitution = SliceSubstitution::new(&eliminated, &values);
        for term in rest {
            *term = rewriter.rewrite_with(term, &substitution);
        }

        remaining_vars
    })
}

/// Applies the one-point rule for `kind` to `(vars, body)` to a fixpoint:
/// repeatedly finds a top-level (dis)equation `x == e` / `e == x` fixing one of
/// the still-unbound variables to a term mentioning none of them, binds
/// `x := e` directly instead of enumerating its sort, and rewrites the residual
/// body under that binding.
///
/// Returns the narrowed variable list, the rewritten residual body, and the
/// accumulated bindings.
///
/// `body` must already be in normal form: every subterm of a normal form is
/// itself one, which is what makes the `e` side of a matched (dis)equation
/// usable directly as a substitution image without re-rewriting it.
///
/// See [the one-point rule](https://mercorg.github.io/merc-website/developer/rewriting/enumeration/#the-one-point-rule).
fn apply_one_point_rule<R: RewriteEngine>(
    rewriter: &mut R,
    guard: &mut BindingGuard<'_, '_>,
    mut vars: Vec<DataVariable>,
    mut body: DataExpression,
    kind: QuantifierKind,
) -> (Vec<DataVariable>, DataExpression, BindingChain) {
    let mut bindings = BindingChain::default();

    while let Some((variable, value)) = find_one_point_equation(&vars, &body, kind) {
        let position = vars
            .iter()
            .position(|v| *v == variable)
            .expect("find_one_point_equation only returns variables from `vars`");
        vars.remove(position);

        bindings = BindingArena::extend(guard, bindings, &variable.copy(), &value.copy());
        body = rewriter.rewrite_with(&body, &SingleSubstitution::new(variable.copy(), value.copy()));
    }

    (vars, body, bindings)
}

/// Finds the first top-level (dis)equation of `body` fixing some `x ∈ vars` to
/// a term `e` mentioning none of `vars` (including `x` itself), and returns
/// `(x, e)`. Which connective is split on, and which shape counts, is decided
/// by `kind`.
fn find_one_point_equation<'a>(
    vars: &[DataVariable],
    body: &'a DataExpression,
    kind: QuantifierKind,
) -> Option<(DataVariable, DataExpression)> {
    let equation = |term: DataExpressionRef<'a>| match term.bool_kind() {
        BoolKind::Equal(lhs, rhs) => Some((lhs, rhs)),
        _ => None,
    };

    // The connective whose operands are searched: a conjunct of an existential, or a disjunct of a
    // universal, quantifier.
    let connective = match kind {
        QuantifierKind::Exists => BoolConnective::And,
        QuantifierKind::Forall => BoolConnective::Or,
    };

    let mut storage = ChainStack::<DataExpressionRef<'a>>::new();
    for term in ChainIter::new(body.copy(), connective, &mut storage) {
        let sides = match (kind, term.bool_kind()) {
            (QuantifierKind::Exists, _) => equation(term),
            // The shapes that deny an equality: `e1 != e2`, `e1 == e2 => φ` and `!(e1 == e2)`.
            (QuantifierKind::Forall, BoolKind::NotEqual(lhs, rhs)) => Some((lhs, rhs)),
            (QuantifierKind::Forall, BoolKind::Implies(operand, _) | BoolKind::Not(operand)) => equation(operand),
            (QuantifierKind::Forall, _) => None,
        };

        let Some((lhs, rhs)) = sides else {
            continue;
        };

        for (side, other) in [(&lhs, &rhs), (&rhs, &lhs)] {
            let DataExpressionKind::Variable(variable) = side.kind() else {
                continue;
            };

            if vars.iter().any(|v| v.copy() == variable) && !mentions_any(other, vars) {
                return Some((variable.protect(), other.protect()));
            }
        }
    }
    None
}

/// Returns whether `term` mentions any variable in `vars`.
fn mentions_any(term: &DataExpressionRef<'_>, vars: &[DataVariable]) -> bool {
    visit_data_expr(term, (), |expr, context| {
        if let DataExpressionKind::Variable(variable) = expr.kind()
            && vars.iter().any(|v| v.copy() == variable)
        {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(Step::Into(context))
    })
    .is_some()
}

#[cfg(test)]
mod tests {
    use merc_data::BasicSort;
    use merc_data::DataApplication;
    use merc_data::DataExpression;
    use merc_data::DataFunctionSymbol;
    use merc_data::DataVariable;
    use merc_data::SortArrow;
    use merc_data::SortExpression;
    use merc_sabre::RewriteSpecification;
    use merc_sabre::Rule;
    use merc_sabre::SabreRewriter;

    use crate::binding::BindingArena;

    use super::apply_one_point_rule;
    use crate::QuantifierKind;

    fn d_sort() -> SortExpression {
        SortExpression::from(BasicSort::new("D"))
    }

    fn constant(name: &str) -> DataExpression {
        DataFunctionSymbol::with_sort(name, d_sort().copy()).into()
    }

    fn variable(name: &str) -> DataVariable {
        DataVariable::with_sort(name, d_sort().copy())
    }

    /// A small, hand-built rewrite system standing in for the `==`/`&&`
    /// fragment of the `Bool` prelude: `eq(x, x) = true`, `and(true, y) = y`,
    /// `and(false, y) = false`.
    fn rewriter() -> SabreRewriter {
        let x: DataExpression = variable("x").into();
        let bool_sort = SortExpression::bool();
        let y: DataExpression = DataVariable::with_sort("y", bool_sort.copy()).into();

        let spec = RewriteSpecification::new(vec![
            Rule::new(
                DataExpression::equal(&d_sort(), &x, &x),
                DataExpression::bool_literal(true),
            ),
            Rule::new(DataExpression::and(&DataExpression::bool_literal(true), &y), y.clone()),
            Rule::new(
                DataExpression::and(&DataExpression::bool_literal(false), &y),
                DataExpression::bool_literal(false),
            ),
        ]);
        SabreRewriter::new(&spec)
    }

    #[test]
    fn test_universal_binds_a_negated_equality_disjunct() {
        // `∀n. (!(n == five) || φ(n))` ≡ `φ(five)`.
        let mut rewriter = rewriter();
        let n = variable("n");
        let body = DataExpression::or(
            &DataExpression::negation(&DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five"))),
            &DataExpression::bool_literal(true),
        );

        let mut arena = BindingArena::default();
        let (vars, _residual, bindings) = arena
            .chain
            .write(|guard| apply_one_point_rule(&mut rewriter, guard, vec![n.clone()], body, QuantifierKind::Forall));

        assert!(vars.is_empty(), "the universal rule must bind `n`");
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                bindings,
                &[n],
                &mut resolved,
            )
        });
        assert_eq!(resolved, vec![constant("five")]);
    }

    #[test]
    fn test_universal_binds_an_implication_antecedent() {
        // `∀n. (n == five => φ(n))` ≡ `φ(five)`.
        let mut rewriter = rewriter();
        let n = variable("n");
        let body = DataExpression::implies(
            &DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five")),
            &DataExpression::bool_literal(true),
        );

        let mut arena = BindingArena::default();
        let (vars, _residual, bindings) = arena
            .chain
            .write(|guard| apply_one_point_rule(&mut rewriter, guard, vec![n.clone()], body, QuantifierKind::Forall));

        assert!(vars.is_empty(), "the universal rule must bind `n`");
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                bindings,
                &[n],
                &mut resolved,
            )
        });
        assert_eq!(resolved, vec![constant("five")]);
    }

    #[test]
    fn test_universal_ignores_a_plain_equality_conjunct() {
        // The existential shape: binding `n` here would narrow `∀n. n == five`
        // to the one point that satisfies it, turning a false quantifier true.
        let mut rewriter = rewriter();
        let n = variable("n");
        let body = DataExpression::and(
            &DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five")),
            &DataExpression::bool_literal(true),
        );

        let mut arena = BindingArena::default();
        let (vars, residual, _bindings) = arena.chain.write(|guard| {
            apply_one_point_rule(
                &mut rewriter,
                guard,
                vec![n.clone()],
                body.clone(),
                QuantifierKind::Forall,
            )
        });

        assert_eq!(vars, vec![n]);
        assert_eq!(residual, body);
    }

    #[test]
    fn test_existential_ignores_a_negated_equality_disjunct() {
        // The dual of the test above: `∃n. (!(n == five) || φ)` is not `φ(five)`.
        let mut rewriter = rewriter();
        let n = variable("n");
        let body = DataExpression::or(
            &DataExpression::negation(&DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five"))),
            &DataExpression::bool_literal(true),
        );

        let mut arena = BindingArena::default();
        let (vars, residual, _bindings) = arena.chain.write(|guard| {
            apply_one_point_rule(
                &mut rewriter,
                guard,
                vec![n.clone()],
                body.clone(),
                QuantifierKind::Exists,
            )
        });

        assert_eq!(vars, vec![n]);
        assert_eq!(residual, body);
    }

    #[test]
    fn test_single_conjunct_binds_directly() {
        let mut rewriter = rewriter();
        let n = variable("n");
        let body = DataExpression::and(
            &DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five")),
            &DataExpression::bool_literal(true),
        );

        let mut arena = BindingArena::default();
        let (vars, residual, bindings) = arena
            .chain
            .write(|guard| apply_one_point_rule(&mut rewriter, guard, vec![n.clone()], body, QuantifierKind::Exists));

        assert!(vars.is_empty());
        assert_eq!(residual, DataExpression::bool_literal(true));
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                bindings,
                &[n],
                &mut resolved,
            )
        });
        assert_eq!(resolved, vec![constant("five")]);
    }

    #[test]
    fn test_chained_conjuncts_resolve_in_one_pass() {
        // `m` only becomes a one-point conjunct (`m == five`) after `n` has
        // been eliminated and the body re-rewritten; the fixpoint loop must
        // catch it too, in the same call.
        let mut rewriter = rewriter();
        let n = variable("n");
        let m = variable("m");
        let body = DataExpression::and(
            &DataExpression::equal(&d_sort(), &n.clone().into(), &constant("five")),
            &DataExpression::equal(&d_sort(), &m.clone().into(), &n.clone().into()),
        );

        let mut arena = BindingArena::default();
        let (vars, residual, bindings) = arena.chain.write(|guard| {
            apply_one_point_rule(
                &mut rewriter,
                guard,
                vec![n.clone(), m.clone()],
                body,
                QuantifierKind::Exists,
            )
        });

        assert!(vars.is_empty());
        assert_eq!(residual, DataExpression::bool_literal(true));
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                bindings,
                &[n, m],
                &mut resolved,
            )
        });
        assert_eq!(resolved, vec![constant("five"), constant("five")]);
    }

    #[test]
    fn test_no_conjunct_leaves_variable_untouched() {
        let mut rewriter = rewriter();
        let n = variable("n");
        let f_sort: SortExpression = SortArrow::new(&[d_sort()], &d_sort()).into();
        let f = DataFunctionSymbol::with_sort("f", f_sort.copy());
        let body: DataExpression = DataApplication::with_args(&f, &[DataExpression::from(n.clone())]).into();

        let mut arena = BindingArena::default();
        let (vars, residual, _bindings) = arena.chain.write(|guard| {
            apply_one_point_rule(
                &mut rewriter,
                guard,
                vec![n.clone()],
                body.clone(),
                QuantifierKind::Exists,
            )
        });

        assert_eq!(vars, vec![n]);
        assert_eq!(residual, body);
    }

    #[test]
    fn test_self_referential_conjunct_is_not_a_one_point_rule() {
        // `n == f(n)` mentions `n` on both sides; binding it would be circular.
        let mut rewriter = rewriter();
        let n = variable("n");
        let f_sort: SortExpression = SortArrow::new(&[d_sort()], &d_sort()).into();
        let f = DataFunctionSymbol::with_sort("f", f_sort.copy());
        let f_n: DataExpression = DataApplication::with_args(&f, &[DataExpression::from(n.clone())]).into();
        let body = DataExpression::equal(&d_sort(), &n.clone().into(), &f_n);

        let mut arena = BindingArena::default();
        let (vars, residual, _bindings) = arena.chain.write(|guard| {
            apply_one_point_rule(
                &mut rewriter,
                guard,
                vec![n.clone()],
                body.clone(),
                QuantifierKind::Exists,
            )
        });

        assert_eq!(vars, vec![n]);
        assert_eq!(residual, body);
    }
}
