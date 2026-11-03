use std::borrow::Cow;

use ahash::AHashSet;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_data::mark_occurrences;
use merc_utilities::FreshNameGenerator;

use crate::RewriteEngine;
use crate::utilities::RewriteSubstitution;

/// A rewriter that can rewrite the body of a binder and decide the quantifiers among them.
pub(crate) trait BinderRewriter: RewriteEngine {
    /// Returns a term equivalent to `binder variables . body`, where `body` is in normal form,
    /// or `None` to keep the quantifier as it is. See [crate::Enumerator::decide].
    fn decide(
        &mut self,
        binder: BinderType,
        variables: &[DataVariable],
        body: &DataExpression,
    ) -> Option<DataExpression>;

    /// Returns a name starting with `base` that this rewriter has not returned before, see
    /// [BinderState].
    fn fresh_name(&mut self, base: &str) -> String;
}

/// The state a rewriter keeps for the binders it rewrites.
pub(crate) struct BinderState<F> {
    /// One search frame per quantifier nesting depth, reused between searches.
    pub frames: Vec<F>,
    /// Names the fresh variables that bound variables are renamed to.
    pub names: FreshNameGenerator,
}

impl<F> Default for BinderState<F> {
    fn default() -> Self {
        BinderState {
            frames: Vec::new(),
            names: FreshNameGenerator::new([]),
        }
    }
}

/// Rewrites a binder `Q x1, .., xn . body` (`Q` being `exists`, `forall`, `lambda`, ...) under
/// `sigma`, and returns a normal form of the binder with `sigma` applied to it.
///
/// The body is rewritten under the [Scope] of the binder, which applies `sigma` without
/// capturing its images, after which an `exists`/`forall` is handed to
/// [BinderRewriter::decide]. Lambdas and comprehensions are rebuilt over their rewritten body,
/// since their reduction is not implemented.
///
/// See [rewriting a binder](https://mercorg.github.io/merc-website/developer/rewriting/quantifiers/#rewriting-a-binder).
///
/// `range` must be `sigma.range()`, which the caller computes once for all binders it rewrites
/// under the same `sigma`.
pub(crate) fn rewrite_binder<R: BinderRewriter, S: RewriteSubstitution>(
    rewriter: &mut R,
    abstraction: &DataAbstraction,
    sigma: &S,
    range: &AHashSet<DataVariable>,
) -> DataExpression {
    let binder = abstraction.binder_type();
    let body = abstraction.body().protect();

    let scope = Scope::new(rewriter, abstraction, &body, sigma, range);
    let body = rewriter.rewrite_with(&body, &scope);
    let renamed: Vec<DataVariable> = scope.renamed.into_iter().map(DataVariable::from).collect();

    if matches!(binder, BinderType::Exists | BinderType::Forall)
        && let Some(result) = rewriter.decide(binder, &renamed, &body)
    {
        return result;
    }

    DataAbstraction::new(binder, &renamed, body).into()
}

/// The substitution under which the body of a binder `Q x1, .., xn . body` is rewritten. It
/// is a new substitution that wraps the enclosing one, `outer`, which is left unchanged:
///
/// - a bound variable `xi` is mapped to `renamed[i]`, and `outer` is never asked for it;
/// - any other variable is mapped as by `outer`.
///
/// Each `renamed[i]` is `xi` itself, unless `xi` occurs in an image of `outer`. Then a bound
/// `xi` would capture that occurrence, so it is renamed to a fresh `xi@k`, as in mCRL2's
/// `rewrite_single_lambda`. For example, `exists y . f(x, y)` under `outer = {x := y}` is
/// rewritten as `exists y@0 . f(y, y@0)`: `x` gets its image `y` from `outer`, and the bound `y`
/// is mapped to `y@0`. Mapping an unrenamed `xi` to itself is what keeps `outer` from replacing
/// it.
///
/// `range` holds the variables that occur in the images of this substitution: those of
/// `outer`, plus the fresh names. A nested binder reads it instead of traversing all images
/// again.
struct Scope<'a> {
    /// The bound variables `x1, .., xn`.
    bound: Vec<DataVariable>,
    /// The variable each bound variable is mapped to in the body, and bound by in the result.
    renamed: Vec<DataExpression>,
    outer: &'a dyn RewriteSubstitution,
    range: Cow<'a, AHashSet<DataVariable>>,
}

impl<'a> Scope<'a> {
    fn new<R: BinderRewriter>(
        rewriter: &mut R,
        abstraction: &DataAbstraction,
        body: &DataExpression,
        outer: &'a dyn RewriteSubstitution,
        range: &'a AHashSet<DataVariable>,
    ) -> Self {
        let bound: Vec<DataVariable> = abstraction.variables().into_iter().collect();
        let mut range = Cow::Borrowed(range);

        let mut renamed = Vec::with_capacity(bound.len());
        for variable in &bound {
            if !range.contains(variable) {
                renamed.push(variable.clone().into());
                continue;
            }

            // The fresh names contain an `@`, which cannot occur in a parsed identifier, so they
            // are unique within this rewriter, but a term may come from another one.
            let base = format!("{}@", variable.name());
            let fresh = loop {
                let candidate = DataVariable::with_sort(rewriter.fresh_name(&base).as_str(), variable.sort());
                let mut occurs = [range.contains(&candidate) || bound.contains(&candidate)];
                mark_occurrences(body, std::slice::from_ref(&candidate), &mut occurs);
                if !occurs[0] {
                    break candidate;
                }
            };

            range.to_mut().insert(fresh.clone());
            renamed.push(fresh.into());
        }

        Scope {
            bound,
            renamed,
            outer,
            range,
        }
    }
}

impl RewriteSubstitution for Scope<'_> {
    fn get<'a>(&'a self, variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>> {
        match self.bound.iter().position(|bound| bound.copy() == *variable) {
            Some(index) => Some(self.renamed[index].copy()),
            None => self.outer.get(variable),
        }
    }

    fn for_each_binding(&self, f: &mut dyn FnMut(&DataVariableRef<'_>, &DataExpressionRef<'_>)) {
        for (variable, image) in self.bound.iter().zip(&self.renamed) {
            f(&variable.copy(), &image.copy());
        }
        self.outer.for_each_binding(f);
    }

    fn range(&self) -> Cow<'_, AHashSet<DataVariable>> {
        Cow::Borrowed(&self.range)
    }
}
