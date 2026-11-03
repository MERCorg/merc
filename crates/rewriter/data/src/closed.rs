use std::ops::ControlFlow;

use ahash::AHashSet;
use merc_aterm::Term;
use merc_utilities::Step;

use crate::DataExpressionRef;
use crate::DataVariable;
use crate::is_data_variable;
use crate::visit_data_expr;

/// Returns true iff `term` contains no data variables, i.e. it is a ground term.
///
/// A closed term normalises to the same result under every substitution, which is what makes
/// it sound to cache its normal form across calls that pass different substitutions.
///
/// Occurrences of variables bound by a binder or a where clause also count, so such a term may
/// be reported as not closed even though it is; that is conservative for the caching above.
pub fn is_closed<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    // Terms are maximally shared, so the same subterm is typically reachable along many paths.
    // Remembering the ones already seen keeps this linear in the size of the term graph rather
    // than the size of the tree it unfolds to.
    let mut visited: AHashSet<DataExpressionRef<'a>> = AHashSet::new();

    let variable: Option<()> = visit_data_expr(&DataExpressionRef::from(term.copy()), (), |expr, context| {
        if !visited.insert(expr.copy()) {
            ControlFlow::Continue(Step::Prune)
        } else if is_data_variable(expr) {
            ControlFlow::Break(())
        } else {
            // A function symbol and a machine number have no children, and the head function
            // symbol of an application is closed by definition, so neither needs to be recognised
            // separately here.
            ControlFlow::Continue(Step::Into(context))
        }
    });

    variable.is_none()
}

/// Sets `occurs[i]` for every `variables[i]` that occurs in `term`, stopping as soon as all of
/// them are set.
///
/// As for [is_closed], occurrences bound by a binder or a where clause inside `term` count too.
///
/// # Panics
///
/// Panics if `occurs` is shorter than `variables`.
pub fn mark_occurrences<'a, 'b, T: Term<'a, 'b>>(term: &'b T, variables: &[DataVariable], occurs: &mut [bool]) {
    let occurs = &mut occurs[..variables.len()];
    if occurs.iter().all(|found| *found) {
        return;
    }

    // See is_closed for why keying on the term address is valid.
    let mut visited = AHashSet::new();

    let _: Option<()> = visit_data_expr(&DataExpressionRef::from(term.copy()), (), |expr, context| {
        if !visited.insert(expr.index()) {
            ControlFlow::Continue(Step::Prune)
        } else if is_data_variable(expr) {
            for (variable, found) in variables.iter().zip(occurs.iter_mut()) {
                *found |= variable.index() == expr.index();
            }

            if occurs.iter().all(|found| *found) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(Step::Prune)
            }
        } else {
            ControlFlow::Continue(Step::Into(context))
        }
    });
}

/// Inserts every variable that occurs in `term` into `variables`.
///
/// As for [is_closed], occurrences bound by a binder or a where clause inside `term` count too.
pub fn collect_variables<'a, 'b, T: Term<'a, 'b>>(term: &'b T, variables: &mut AHashSet<DataVariable>) {
    // See is_closed for why keying on the term address is valid.
    let mut visited = AHashSet::new();

    let _: Option<()> = visit_data_expr(&DataExpressionRef::from(term.copy()), (), |expr, context| {
        if !visited.insert(expr.index()) {
            ControlFlow::Continue(Step::Prune)
        } else if is_data_variable(expr) {
            variables.insert(DataVariable::from(expr.protect()));
            ControlFlow::Continue(Step::Prune)
        } else {
            ControlFlow::Continue(Step::Into(context))
        }
    });
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;

    use crate::DataExpression;
    use crate::DataVariable;
    use crate::collect_variables;
    use crate::is_closed;
    use crate::mark_occurrences;

    #[test]
    fn test_is_closed_ground_term() {
        let term = DataExpression::from_string("s(s(a), b)").unwrap();
        assert!(is_closed(&term));
    }

    #[test]
    fn test_is_closed_with_variable() {
        let variables = AHashSet::from_iter(["x".to_string()]);
        let term = DataExpression::from_string_untyped("s(s(x), b)", &variables).unwrap();
        assert!(!is_closed(&term));
    }

    #[test]
    fn test_is_closed_bare_variable() {
        let variables = AHashSet::from_iter(["x".to_string()]);
        let term = DataExpression::from_string_untyped("x", &variables).unwrap();
        assert!(!is_closed(&term));
    }

    #[test]
    fn test_mark_occurrences_finds_only_occurring_variables() {
        let variables = AHashSet::from_iter(["x".to_string(), "y".to_string()]);
        let term = DataExpression::from_string_untyped("s(x, s(x, b))", &variables).unwrap();
        let candidates = [DataVariable::new("x"), DataVariable::new("y")];

        let mut occurs = [false; 2];
        mark_occurrences(&term, &candidates, &mut occurs);
        assert_eq!(occurs, [true, false]);
    }

    #[test]
    fn test_collect_variables_collects_each_variable_once() {
        let variables = AHashSet::from_iter(["x".to_string(), "y".to_string()]);
        let term = DataExpression::from_string_untyped("s(x, s(y, x))", &variables).unwrap();

        let mut collected = AHashSet::new();
        collect_variables(&term, &mut collected);
        assert_eq!(
            collected,
            AHashSet::from_iter([DataVariable::new("x"), DataVariable::new("y")])
        );
    }
}
