use ahash::AHashSet;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::mark_occurrences;
use merc_sabre::Enumerator as SabreEnumerator;
use merc_sabre::RewriteEngine;

use crate::Enumerator;
use crate::EnumeratorFrame;
use crate::QuantifierKind;
use crate::WitnessOutcome;

/// Decides quantifiers through [`Enumerator::find_witness`].
///
/// Every rewrite fed to this enumerator must be in the same context, since
/// the fresh variable generator is shared by all of them.
impl SabreEnumerator for Enumerator {
    type Frame = EnumeratorFrame;

    fn decide<R: RewriteEngine>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        binder: BinderType,
        vars: &[DataVariable],
        body: &DataExpression,
    ) -> Option<DataExpression> {
        let kind = match binder {
            BinderType::Exists => QuantifierKind::Exists,
            BinderType::Forall => QuantifierKind::Forall,
            _ => unreachable!("Enumerator::decide is only ever called for Exists/Forall"),
        };

        // A bound variable that does not occur in the body can be dropped, since every sort is
        // non-empty: `exists x . p` is `p`.
        let vars = occurring(vars, body);
        if vars.is_empty() {
            return Some(body.clone());
        }

        // The variables of a sort that cannot be enumerated, such as `Real`, are split off before
        // the search. Handed to it, the first one it expands abandons the whole goal
        // (`Outcome::NotEnumerable`), so not even the enumerable variables would be eliminated.
        // Quantifiers of the same kind commute, so `exists x: Real, p: Pos . φ` equals
        // `exists x: Real . exists p: Pos . φ`. The inner quantifier is searched as usual, and
        // `x` stays bound around whatever the search leaves. For example,
        // `exists x: Real, p: Pos . p == 1 && x > 4` becomes `exists x: Real . x > 4`.
        let (searched, others): (Vec<DataVariable>, Vec<DataVariable>) =
            vars.iter().cloned().partition(|variable| self.is_enumerable(variable));
        if searched.is_empty() {
            // Nothing can be searched, so the quantifier is kept.
            return Some(DataAbstraction::new(binder, &vars, body.clone()).into());
        }

        let search = self.find_witness_normalized(frame, rewriter, &searched, body.clone(), kind);
        match search.outcome {
            // These two decisions hold for every value of `others`: the leaf that is a witness
            // is the literal `true` (or `false` for `forall`), and so is every branch of a search
            // that is exhausted without one. Since every sort is non-empty, `others` are dropped
            // as well: `exists x: Real, p: Pos . p == 1` is `true`.
            WitnessOutcome::Found(_) => Some(DataExpression::bool_literal(matches!(kind, QuantifierKind::Exists))),
            WitnessOutcome::NoneExists => Some(DataExpression::bool_literal(matches!(kind, QuantifierKind::Forall))),
            WitnessOutcome::LimitReached => {
                if let Some(residuals) = search.residuals {
                    // The search was exhausted without a decision: every branch was rejected or
                    // ended in a leaf that binds all of `searched` and whose body is still
                    // undecided, because it depends on `others` or on a variable that is free in
                    // the quantifier, such as that of an enclosing one. The rejected branches
                    // contribute nothing, so the quantifier over `searched` equals the
                    // disjunction of the undecided bodies (the residuals) for `exists`, and their
                    // conjunction for `forall`. For example, `exists y: Bool . (y && c) || (!y && d)`
                    // leaves `c` for `y = true` and `d` for `y = false`, and becomes `c || d`.
                    //
                    // The quantifier is replaced by the residuals, so that an
                    // enclosing search does not repeat this one for every value of its own
                    // variables. In `forall x: Bool . exists y: Bool . y == x`, the body of the
                    // outer quantifier is rewritten first, with `x` free, and the inner one
                    // becomes `x || !x` there. The outer search then rewrites `x || !x` under
                    // `x = true` and `x = false`, rather than searching `y` again under each.
                    // The `others` were not searched, so they are still bound around the result.
                    let combine = match kind {
                        QuantifierKind::Exists => DataExpression::or,
                        QuantifierKind::Forall => DataExpression::and,
                    };
                    let combine = |lhs: DataExpression, rhs: DataExpression| combine(&lhs, &rhs);
                    // Several leaves often leave the same body, e.g. one that no longer depends
                    // on the variables.
                    let mut seen = AHashSet::new();
                    let combined = residuals
                        .into_iter()
                        .filter(|residual| seen.insert(residual.clone()))
                        .reduce(combine)
                        .expect("an undecided search has an undecided leaf");
                    let result = rewriter.rewrite(&combined);

                    let others = occurring(&others, &result);
                    if others.is_empty() {
                        Some(result)
                    } else {
                        Some(DataAbstraction::new(binder, &others, result).into())
                    }
                } else {
                    // A limit was reached, or the search met a sort it cannot enumerate, so the
                    // quantifier is kept. An enclosing search tries it again under its values.
                    Some(DataAbstraction::new(binder, &vars, body.clone()).into())
                }
            }
        }
    }
}

/// The variables of `vars` that occur in `body`, in `vars` order.
fn occurring(vars: &[DataVariable], body: &DataExpression) -> Vec<DataVariable> {
    let mut occurs = vec![false; vars.len()];
    mark_occurrences(body, vars, &mut occurs);
    vars.iter()
        .zip(occurs)
        .filter(|(_, occurs)| *occurs)
        .map(|(variable, _)| variable.clone())
        .collect()
}
