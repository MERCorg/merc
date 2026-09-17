use merc_syntax::ActDecl;
use merc_syntax::SortExpression;
use merc_syntax::SourceMap;
use merc_syntax::StateFrm;
use merc_syntax::UntypedStateFrmSpec;

use crate::DataSpecification;
use crate::NumberEncoding;
use crate::ResolvedSortId;
use crate::TypingInfo;
use crate::checking;

use super::ModalError;
use super::check;

/// Whether a state formula's `val(...)` occurrences are `Real`- or `Bool`-sorted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaType {
    /// Quantitative: every state-level `val(...)` is `Real`-sorted, or `Bool`
    /// sorts mapped to `Real`.
    Real,
    /// Plain mu-calculus: every state-level `val(...)` is `Bool`-sorted.
    Bool,
}

/// A type-checked modal (mu-calculus) state formula: the data specification plus its `act`
/// declarations and the formula itself, all resolved and checked against it — every action
/// instance, fixpoint (`mu`/`nu`) variable, and `forall`/`exists`/`inf`/`sup`/`sum` binder
/// included.
pub struct ModalSpecification {
    /// The original specification, *minus* its data specification.
    spec: UntypedStateFrmSpec,
    data: DataSpecification,
    /// Every checked expression's `TypingInfo`.
    typing: TypingInfo,
    /// The type every state-level `val(...)` occurrence was checked against.
    formula_type: FormulaType,
}

impl ModalSpecification {
    /// Type checks `spec` against a fresh, throwaway [`SourceMap`], using the default number
    /// encoding.
    pub fn from_untyped(spec: UntypedStateFrmSpec, formula_type: FormulaType) -> Result<Self, ModalError> {
        Self::from_untyped_with(spec, formula_type, NumberEncoding::default(), &mut SourceMap::new())
    }

    /// Type checks `spec` against the given number encoding and source map, requiring val
    /// occurrences to conform to `formula_type`.
    ///
    /// `sources` also accumulates the system-defined ("Appendix B") content this generates.
    pub fn from_untyped_with(
        mut spec: UntypedStateFrmSpec,
        formula_type: FormulaType,
        encoding: NumberEncoding,
        sources: &mut SourceMap,
    ) -> Result<Self, ModalError> {
        // A pure syntactic pass, before anything else needs `spec` — see
        // `resolution::variable_resolution`.
        crate::resolve_modal_variables(&mut spec);

        let data_spec = std::mem::take(&mut spec.data_specification);
        let mut data = DataSpecification::from_untyped_with(data_spec, encoding, sources)?;

        let tables = DeclarationTables::build(&mut data, &spec.action_declarations, resolve_declared_sort)?;
        let typing = check::check_modal_specification(&mut data, &tables, &spec, formula_type)?;

        Ok(ModalSpecification {
            spec,
            data,
            typing,
            formula_type: formula_type,
        })
    }

    /// The checked data specification.
    pub fn data_specification(&self) -> &DataSpecification {
        &self.data
    }

    /// Consumes `self`, returning the checked data specification.
    pub fn into_data_specification(self) -> DataSpecification {
        self.data
    }

    /// The `act` declarations, in scope in every modality's action/regular formula.
    pub fn action_declarations(&self) -> &[ActDecl] {
        &self.spec.action_declarations
    }

    /// The state formula itself.
    pub fn formula(&self) -> &StateFrm {
        &self.spec.formula
    }

    /// Every checked expression's typing across the *whole* specification.
    pub fn typing_info(&mut self) -> TypingInfo {
        let mut info = self.data.typing_info();
        info.merge(self.typing.clone());
        info
    }

    /// The sort of this modal formula.
    pub fn val_sort(&self) -> FormulaType {
        self.formula_type
    }
}

/// The resolved `act` declaration table, used by [`super::check`]'s scoped walk to resolve every
/// action instance it reaches. Modal has no `proc`/`glob` declarations of its own to add, unlike
/// `crate::process::process_specification::DeclarationTables`, so this is exactly
/// [`checking::ActionTable`], shared verbatim rather than wrapping it.
pub(super) type DeclarationTables = checking::ActionTable;

/// Resolves a sort expression occurring in an `act`/fixpoint-variable-parameter/binder declaration:
/// rejects an anonymous `struct` (never legal here), then defers to
/// [`DataSpecification::resolve_declared_sort`] for the rest.
pub(super) fn resolve_declared_sort(
    data: &mut DataSpecification,
    sort: &SortExpression,
) -> Result<ResolvedSortId, ModalError> {
    checking::resolve_declared_sort(data, sort, |span| ModalError::AnonymousStructInDeclaration { span })
}
