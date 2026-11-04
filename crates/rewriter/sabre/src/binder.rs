use std::borrow::Cow;

use ahash::AHashSet;
use merc_aterm::ATermString;
use merc_aterm::Symbol;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;

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

    /// Returns a variable of the sort of `variable` whose name occurs in no term, see
    /// [BinderState::fresh_variable].
    fn fresh_variable(&mut self, variable: &DataVariable) -> DataVariable;
}

/// The state a rewriter keeps for the binders it rewrites.
pub(crate) struct BinderState<F> {
    /// One search frame per quantifier nesting depth, reused between searches.
    pub frames: Vec<F>,
    /// The index of the next fresh variable.
    next_fresh: usize,
}

impl<F> Default for BinderState<F> {
    fn default() -> Self {
        BinderState {
            frames: Vec::new(),
            next_fresh: 0,
        }
    }
}

impl<F> BinderState<F> {
    /// Returns a variable of the sort of `variable`, named `<name>@<index>`, whose name occurs
    /// in no term that is alive.
    ///
    /// A variable's name is a symbol, which every live term that uses it keeps in the symbol
    /// pool, so a name that the pool creates anew occurs in no term: not in a body or an image,
    /// where a binder would capture it, nor in a term from another rewriter or read from a file.
    /// No term has to be traversed. The `@` cannot occur in a parsed identifier, so the first
    /// candidate is nearly always new.
    pub fn fresh_variable(&mut self, variable: &DataVariable) -> DataVariable {
        loop {
            let index = self.next_fresh;
            self.next_fresh += 1;

            if let Some(name) = Symbol::new_unused_fmt(format_args!("{}@{index}", variable.name()), 0) {
                return DataVariable::with_sort(ATermString::with_symbol(&name), variable.sort());
            }
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
/// `range` must be `sigma.range()`. It is passed in so that a caller that rewrites several
/// binders under the same `sigma`, such as the siblings in `f(exists x . g(x), exists y . h(y))`,
/// computes it only once.
///
/// See [rewriting a binder](https://mercorg.github.io/merc-website/developer/rewriting/quantifiers/#rewriting-a-binder).
pub(crate) fn rewrite_binder<R: BinderRewriter, S: RewriteSubstitution>(
    rewriter: &mut R,
    abstraction: &DataAbstraction,
    sigma: &S,
    range: &AHashSet<DataVariable>,
) -> DataExpression {
    let binder = abstraction.binder_type();
    let body = abstraction.body().protect();

    let scope = Scope::new(rewriter, abstraction, sigma, range);
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
/// `outer`, plus the fresh names. It borrows the range of `outer`, and is only copied when a
/// variable is renamed. A nested binder reads it instead of traversing all images again.
struct Scope<'a> {
    /// The bound variables `x1, .., xn`.
    bound: Vec<DataVariable>,
    /// The variable each bound variable is mapped to in the body, and bound by in the result.
    renamed: Vec<DataExpression>,
    outer: &'a dyn RewriteSubstitution,
    range: Cow<'a, AHashSet<DataVariable>>,
}

impl<'a> Scope<'a> {
    /// Returns the scope of `abstraction` inside `outer`, renaming the bound variables that
    /// occur in `range`, which must be `outer.range()`.
    fn new<R: BinderRewriter>(
        rewriter: &mut R,
        abstraction: &DataAbstraction,
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

            let fresh = rewriter.fresh_variable(variable);
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
