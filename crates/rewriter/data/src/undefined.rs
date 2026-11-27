use merc_aterm::ATermString;
use merc_aterm::Symb;
use merc_aterm::Term;

use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataVariable;
use crate::SortExpression;

/// The name of the identifier string that mCRL2 stores where a value is absent.
const NO_VALUE_NAME: &str = "@NoValue";

/// The name of the data variable that mCRL2 uses to represent the undefined real.
const UNDEFINED_REAL_NAME: &str = "@undefined_real";

/// Returns the identifier string `@NoValue`, which mCRL2 stores where a value is absent, for
/// example the projection or recogniser of a structured sort constructor that has none. It is
/// mCRL2's default identifier string, from which its default sort `SortId(@NoValue)` and default
/// variable `DataVarId(@NoValue, SortId(@NoValue))` are built.
pub fn no_value() -> ATermString {
    ATermString::new(NO_VALUE_NAME)
}

/// Returns true iff `term` is the identifier string returned by [`no_value`].
pub fn is_no_value<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    let symbol = term.get_head_symbol();
    symbol.arity() == 0 && symbol.name() == NO_VALUE_NAME
}

/// Returns the data expression that mCRL2 defines for the default undefined real.
pub fn undefined_real() -> DataExpression {
    let sort = SortExpression::real();
    DataVariable::with_sort(UNDEFINED_REAL_NAME, sort.copy()).into()
}

/// Returns true iff `term` is exactly the data expression returned by [`undefined_real`].
pub fn is_undefined_real<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    let candidate = DataExpressionRef::from(term.copy());
    candidate.protect() == undefined_real()
}

/// Returns true iff `term` is the undefined real data expression, comparing it against the
/// canonical [`undefined_real`] term. Convenience overload over [`is_undefined_real`] for a
/// value already wrapped in a [`DataExpressionRef`].
pub fn is_undefined_real_ref(term: DataExpressionRef<'_>) -> bool {
    is_undefined_real(&term)
}

#[cfg(test)]
mod tests {
    use merc_aterm::ATermString;

    use crate::BasicSort;
    use crate::DataVariable;
    use crate::SortExpression;
    use crate::is_no_value;
    use crate::no_value;

    #[test]
    fn test_is_no_value() {
        assert!(is_no_value(&no_value()));
        assert!(!is_no_value(&ATermString::new("NoValue")));
        assert!(!is_no_value(&ATermString::new("")));

        // The default variable of mCRL2 is built from `@NoValue`, but is not itself `@NoValue`.
        let variable = DataVariable::with_sort(no_value(), SortExpression::from(BasicSort::new(no_value())).copy());
        assert!(!is_no_value(&variable));
    }
}
