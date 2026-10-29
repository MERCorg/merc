use merc_data::BinderType;
use merc_data::DataExpression;
use merc_data::DataVariable;
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
    ) -> Option<bool> {
        let kind = match binder {
            BinderType::Exists => QuantifierKind::Exists,
            BinderType::Forall => QuantifierKind::Forall,
            _ => unreachable!("Enumerator::decide is only ever called for Exists/Forall"),
        };

        match self.find_witness(frame, rewriter, vars, body, kind) {
            WitnessOutcome::Found(_) => Some(matches!(kind, QuantifierKind::Exists)),
            WitnessOutcome::NoneExists => Some(matches!(kind, QuantifierKind::Forall)),
            WitnessOutcome::LimitReached => None,
        }
    }
}
