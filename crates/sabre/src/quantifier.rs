use merc_data::BinderType;
use merc_data::DataExpression;
use merc_data::DataVariable;

use crate::RewriteEngine;

/// Fed to a rewriter to decide `exists`/`forall` quantifiers it cannot reduce
/// on its own.
///
/// Defined here rather than in `merc_enumerate`, and the rewriter
/// parameterised over it rather than depending on `merc_enumerate`
/// concretely, because `merc_enumerate` already depends on `merc_sabre`
/// (an enumerator closes candidate substitutions through a [RewriteEngine]):
/// `merc_sabre` cannot depend back on it without a cycle. `merc_enumerate`
/// implements this trait instead.
pub trait Enumerator {
    /// Attempts to decide `binder vars . body`, where `body` is the
    /// quantifier's body, already rewritten with `vars` held as free normal
    /// forms.
    ///
    /// Returns `Some(true)`/`Some(false)` once the search settles the
    /// quantifier, or `None` when it can't.
    fn decide<R: RewriteEngine>(
        &mut self,
        rewriter: &mut R,
        binder: BinderType,
        vars: &[DataVariable],
        body: &DataExpression,
    ) -> Option<bool>;
}

/// The default [`Enumerator`]: never decides, so every quantifier it reaches
/// is left in place, over its rewritten body.
///
/// Used by [`InnermostRewriter::new`](crate::InnermostRewriter::new) so that
/// building a rewriter without an enumerator no longer panics on a
/// quantifier (as it used to, since binders had no accessors at all), it
/// simply leaves it undecided.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoQuantifierEnumerator;

impl Enumerator for NoQuantifierEnumerator {
    fn decide<R: RewriteEngine>(
        &mut self,
        _rewriter: &mut R,
        _binder: BinderType,
        _vars: &[DataVariable],
        _body: &DataExpression,
    ) -> Option<bool> {
        None
    }
}
