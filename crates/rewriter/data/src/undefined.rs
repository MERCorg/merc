use merc_aterm::Term;

use crate::BasicSort;
use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataVariable;
use crate::SortExpression;

/// The name of the data variable that mCRL2 uses to represent the undefined real.
const UNDEFINED_REAL_NAME: &str = "@undefined_real";

/// Returns the data expression that mCRL2 defines for the default undefined real.
pub fn undefined_real() -> DataExpression {
    let sort: SortExpression = BasicSort::new("Real").into();
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
