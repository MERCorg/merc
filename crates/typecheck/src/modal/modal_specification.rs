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
pub enum ValSort {
    /// Quantitative: every `val(...)` is `Real`-sorted, combined via a `*`-multiplier.
    Real,
    /// Plain mu-calculus: every `val(...)` is a `Bool`-sorted atom.
    Bool,
    /// The formula has no state-level `val(...)` at all, so nothing pins the choice down.
    Unknown,
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
    /// The sort the formula's `val(...)` occurrences fixed on.
    val_sort: ValSort,
}

impl ModalSpecification {
    /// Type checks `spec` against a fresh, throwaway [`SourceMap`], using the default number
    /// encoding.
    ///
    /// Prefer [`Self::from_untyped_with`] with a real `sources` whenever an error may have to be
    /// rendered afterwards: the `SourceMap` built here is discarded on return, so spans into
    /// imported or system-defined content have nothing left to render against.
    pub fn from_untyped(spec: UntypedStateFrmSpec) -> Result<Self, ModalError> {
        Self::from_untyped_with(spec, NumberEncoding::default(), &mut SourceMap::new())
    }

    /// Type checks `spec`: its data specification first, see
    /// [`DataSpecification::from_untyped_with`], then its `act` declarations' argument sorts, and
    /// finally the formula itself against them.
    ///
    /// `sources` also accumulates the system-defined ("Appendix B") content this generates. Pass
    /// the `SourceMap` `spec` was parsed (and, if applicable, `%import`-resolved) against, so that
    /// every span shares one offset space and renders correctly.
    pub fn from_untyped_with(
        mut spec: UntypedStateFrmSpec,
        encoding: NumberEncoding,
        sources: &mut SourceMap,
    ) -> Result<Self, ModalError> {
        // A pure syntactic pass, before anything else needs `spec` — see
        // `resolution::variable_resolution`.
        crate::resolve_modal_variables(&mut spec);

        let data_spec = std::mem::take(&mut spec.data_specification);
        let mut data = DataSpecification::from_untyped_with(data_spec, encoding, sources)?;

        let tables = DeclarationTables::build(
            &mut data,
            &spec.action_declarations,
            resolve_declared_sort,
            Some(|name, span| ModalError::DuplicateActionDeclaration { name, span }),
        )?;
        let (typing, val_sort) = check::check_modal_specification(&mut data, &tables, &spec)?;

        Ok(ModalSpecification {
            spec,
            data,
            typing,
            val_sort,
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

    /// Whether the formula's `val(...)` occurrences are `Real`- or `Bool`-sorted, as fixed by the
    /// first state-level one; see [`ValSort`].
    pub fn val_sort(&self) -> ValSort {
        self.val_sort
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
