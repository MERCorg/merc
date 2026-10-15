use merc_data::BinderType;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_sabre::Enumerator as SabreEnumerator;
use merc_sabre::RewriteEngine;

use crate::Enumerator;
use crate::QuantifierKind;
use crate::WitnessOutcome;
use crate::fresh::FreshVariableGenerator;

/// Implements [`merc_sabre::Enumerator`] on top of
/// [`Enumerator::find_witness`].
///
/// Keeps the fresh variable generator around for each call. This does mean that
/// every rewrite must be in the same context.
pub struct QuantifierEnumerator {
    enumerator: Enumerator,
    generator: FreshVariableGenerator,
}

impl QuantifierEnumerator {
    pub fn new(enumerator: Enumerator, generator: FreshVariableGenerator) -> Self {
        QuantifierEnumerator { enumerator, generator }
    }
}

impl SabreEnumerator for QuantifierEnumerator {
    fn decide<R: RewriteEngine>(
        &mut self,
        rewriter: &mut R,
        binder: BinderType,
        vars: &[DataVariable],
        body: &DataExpression,
    ) -> Option<bool> {
        let kind = match binder {
            BinderType::Exists => QuantifierKind::Exists,
            BinderType::Forall => QuantifierKind::Forall,
            _ => unreachable!("Enumerator::decide is only ever called for Exists/Forall"),
        };

        match self
            .enumerator
            .find_witness(rewriter, &mut self.generator, vars, body, kind)
        {
            WitnessOutcome::Found(_) => Some(matches!(kind, QuantifierKind::Exists)),
            WitnessOutcome::NoneExists => Some(matches!(kind, QuantifierKind::Forall)),
            WitnessOutcome::LimitReached => None,
        }
    }
}
