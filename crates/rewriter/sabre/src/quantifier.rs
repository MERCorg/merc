use merc_data::BinderType;
use merc_data::DataExpression;
use merc_data::DataVariable;

use crate::RewriteEngine;

/// Fed to a rewriter to decide `exists`/`forall` quantifiers it cannot decide
/// on its own.
///
/// Split into the shared `&self` context and a per-search [Enumerator::Frame],
/// so that a quantifier reached while another one is being decided can be
/// decided by the same enumerator: the outer search's frame is still live,
/// so the nested search gets a frame of its own.
pub trait Enumerator {
    /// The scratch state of a single search. The rewriter keeps one per
    /// nesting depth and reuses them between searches.
    type Frame: Default;

    /// Attempts to decide `binder vars . body`, where `body` is the
    /// quantifier's body, already rewritten with `vars` held as free normal
    /// forms.
    ///
    /// Returns a normal form equivalent to the quantifier, which is a `Bool`
    /// literal once the search settles it, or `None` to keep it as given.
    /// `rewriter` decides nested quantifiers through this enumerator again, so
    /// no borrow of `self` may be held across a rewrite.
    fn decide<R: RewriteEngine>(
        &self,
        frame: &mut Self::Frame,
        rewriter: &mut R,
        binder: BinderType,
        vars: &[DataVariable],
        body: &DataExpression,
    ) -> Option<DataExpression>;
}

/// The default [`Enumerator`]: never decides, so every quantifier it reaches
/// is left in place, over its rewritten body.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoQuantifierEnumerator;

impl Enumerator for NoQuantifierEnumerator {
    type Frame = ();

    fn decide<R: RewriteEngine>(
        &self,
        _frame: &mut Self::Frame,
        _rewriter: &mut R,
        _binder: BinderType,
        _vars: &[DataVariable],
        _body: &DataExpression,
    ) -> Option<DataExpression> {
        None
    }
}
