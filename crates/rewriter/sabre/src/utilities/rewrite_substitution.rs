#![forbid(unsafe_code)]

use std::borrow::Cow;

use ahash::AHashSet;
use ahash::HashMap;
use merc_aterm::ATerm;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_aterm::TermBuilder;
use merc_aterm::Yield;
use merc_aterm::storage::THREAD_TERM_POOL;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_data::collect_variables;
use merc_data::is_closed;

/// Maps free variables to their replacement while rewriting.
///
/// Implementations are usually taken as a generic parameter, so that the variable lookup is
/// monomorphised at every call site and [EmptySubstitution] collapses to nothing at all. The
/// trait is object safe for the places that need to erase the type, such as the scope of a
/// binder.
pub trait RewriteSubstitution {
    /// May only return `true` when [RewriteSubstitution::get] always returns `None`.
    ///
    /// Lets a caller skip work that the identity makes pointless, such as the traversal in
    /// [apply_substitution]. For [EmptySubstitution] this inlines to a constant.
    fn is_empty(&self) -> bool {
        false
    }

    /// Returns the replacement for `variable`, or `None` if it is not in the domain, in which
    /// case the variable is left unchanged.
    ///
    /// The returned term must be in normal form; rewriters splice it in without normalising it
    /// again.
    fn get<'a>(&'a self, variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>>;

    /// Calls `f` with every variable in the domain and its image. A variable may be reported
    /// more than once, when one image shadows another.
    fn for_each_binding(&self, f: &mut dyn FnMut(&DataVariableRef<'_>, &DataExpressionRef<'_>));

    /// Returns the variables that occur in the image of another variable: a binder of one of
    /// them is renamed apart before the substitution is applied to its body, since the image
    /// would otherwise be captured.
    ///
    /// An image `v` of `v` is skipped: it only matters under a binder of `v`, where `v` is not
    /// free. Over-approximating only costs a needless renaming. The default traverses every
    /// image, which is only done when a binder is reached; the scope of a binder keeps its range
    /// instead, so that nested binders do not traverse the same images again.
    fn range(&self) -> Cow<'_, AHashSet<DataVariable>> {
        let mut range = AHashSet::new();
        if !self.is_empty() {
            self.for_each_binding(&mut |variable, image| {
                if image.index() != variable.index() {
                    collect_variables(image, &mut range);
                }
            });
        }
        Cow::Owned(range)
    }
}

/// The identity substitution, which leaves every variable unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptySubstitution;

impl RewriteSubstitution for EmptySubstitution {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline(always)]
    fn get<'a>(&'a self, _variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>> {
        None
    }

    fn for_each_binding(&self, _f: &mut dyn FnMut(&DataVariableRef<'_>, &DataExpressionRef<'_>)) {}
}

impl RewriteSubstitution for HashMap<DataVariable, DataExpression> {
    fn is_empty(&self) -> bool {
        HashMap::is_empty(self)
    }

    fn get<'a>(&'a self, variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>> {
        // Protecting the key allocates an entry in the protection set on every lookup. mCRL2
        // keys its substitutions on the term pointer instead, which we can adopt by storing the
        // variable next to its value under its term index; deferred until it shows up in a
        // profile.
        HashMap::get(self, &variable.protect()).map(|value| value.copy())
    }

    fn for_each_binding(&self, f: &mut dyn FnMut(&DataVariableRef<'_>, &DataExpressionRef<'_>)) {
        for (variable, value) in self {
            f(&variable.copy(), &value.copy());
        }
    }
}

/// A substitution given as two parallel slices, useful for the handful of
/// bindings a single search branch or LPS successor introduces.
///
/// `values` must be at least as long as `variables`; [`Self::get`] panics
/// otherwise.
pub struct SliceSubstitution<'a> {
    pub variables: &'a [DataVariable],
    pub values: &'a [DataExpression],
}

impl<'a> SliceSubstitution<'a> {
    pub fn new(variables: &'a [DataVariable], values: &'a [DataExpression]) -> Self {
        SliceSubstitution { variables, values }
    }
}

impl RewriteSubstitution for SliceSubstitution<'_> {
    fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    fn get<'a>(&'a self, variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>> {
        self.variables
            .iter()
            .position(|v| v.copy() == *variable)
            .map(|index| self.values[index].copy())
    }

    fn for_each_binding(&self, f: &mut dyn FnMut(&DataVariableRef<'_>, &DataExpressionRef<'_>)) {
        for (variable, value) in self.variables.iter().zip(self.values) {
            f(&variable.copy(), &value.copy());
        }
    }
}

/// Replaces every free variable of `t` by its image under `sigma`.
///
/// The result is not normalised: rewrite rules can fire across the substitution boundary, so
/// callers must still rewrite the result.
///
/// # Panics
///
/// Panics if `t` contains a binder or where clause, whose bound variables would need
/// capture-avoiding renaming.
pub fn apply_substitution<S: RewriteSubstitution + ?Sized>(t: &DataExpression, sigma: &S) -> DataExpression {
    // A closed term has no free variables, so no substitution can change it.
    if sigma.is_empty() || is_closed(t) {
        return t.clone();
    }

    let mut builder = TermBuilder::<ATerm, Symbol>::new();

    THREAD_TERM_POOL
        .with(|tp| {
            builder.evaluate(
                tp,
                t.copy().protect().into(),
                |_tp, args, t| match DataExpressionRef::from(t.copy()).kind() {
                    DataExpressionKind::Variable(variable) => {
                        let replacement = sigma.get(&variable).map(|value| value.protect().into());

                        Ok(Yield::Term(replacement.unwrap_or(t)))
                    }
                    DataExpressionKind::FunctionSymbol(_) | DataExpressionKind::MachineNumber(_) => Ok(Yield::Term(t)),
                    DataExpressionKind::Application(_) => {
                        for arg in t.arguments() {
                            args.push(arg.protect());
                        }

                        Ok(Yield::Construct(t.get_head_symbol().protect()))
                    }
                    DataExpressionKind::Abstraction(_) | DataExpressionKind::WhereClause(_) => {
                        panic!("apply_substitution is not defined for binders and where clauses: {t}");
                    }
                },
                |tp, symbol, args| Ok(tp.create_term_iter(&symbol, args)),
            )
        })
        .expect("apply_substitution never fails")
        .into()
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use ahash::HashMap;
    use ahash::HashMapExt;
    use merc_data::DataExpression;
    use merc_data::DataVariable;

    use crate::utilities::EmptySubstitution;
    use crate::utilities::SliceSubstitution;
    use crate::utilities::apply_substitution;

    /// Parses `input` treating every name in `variables` as a variable.
    fn term(input: &str, variables: &[&str]) -> DataExpression {
        let variables: AHashSet<String> = variables.iter().map(|v| v.to_string()).collect();
        DataExpression::from_string_untyped(input, &variables).unwrap()
    }

    /// Builds a substitution mapping each name to the corresponding parsed closed term.
    fn substitution(entries: &[(&str, &str)]) -> HashMap<DataVariable, DataExpression> {
        let mut sigma = HashMap::new();
        for (name, value) in entries {
            sigma.insert(DataVariable::new(*name), term(value, &[]));
        }
        sigma
    }

    #[test]
    fn test_apply_substitution_replaces_variables() {
        let sigma = substitution(&[("x", "a")]);

        assert_eq!(
            apply_substitution(&term("f(x, g(x), b)", &["x"]), &sigma),
            term("f(a, g(a), b)", &[])
        );
    }

    #[test]
    fn test_apply_substitution_bare_variable() {
        let sigma = substitution(&[("x", "s(a)")]);

        assert_eq!(apply_substitution(&term("x", &["x"]), &sigma), term("s(a)", &[]));
    }

    #[test]
    fn test_apply_substitution_leaves_variables_outside_the_domain() {
        let sigma = substitution(&[("x", "a")]);
        let input = term("f(x, y)", &["x", "y"]);

        assert_eq!(apply_substitution(&input, &sigma), term("f(a, y)", &["y"]));
    }

    #[test]
    fn test_apply_substitution_preserves_closed_terms() {
        let sigma = substitution(&[("x", "a")]);
        let input = term("f(b, g(c))", &[]);

        assert_eq!(apply_substitution(&input, &sigma), input);
    }

    #[test]
    fn test_apply_substitution_empty_is_identity() {
        let input = term("f(x, g(y))", &["x", "y"]);

        assert_eq!(apply_substitution(&input, &EmptySubstitution), input);
    }

    #[test]
    fn test_slice_substitution_replaces_variables() {
        let variables = [DataVariable::new("x"), DataVariable::new("y")];
        let values = [term("a", &[]), term("b", &[])];
        let sigma = SliceSubstitution::new(&variables, &values);

        assert_eq!(
            apply_substitution(&term("f(x, y)", &["x", "y"]), &sigma),
            term("f(a, b)", &[])
        );
    }
}
