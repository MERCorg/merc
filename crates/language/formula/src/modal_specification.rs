use std::collections::HashMap;
use std::collections::HashSet;

use merc_syntax::ActDecl;
use merc_syntax::SortExpression;
use merc_syntax::SourceMap;
use merc_syntax::StateFrm;
use merc_syntax::StateVarId;
use merc_syntax::UntypedStateFrmSpec;

use crate::variable_resolution::resolve_modal_variables;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;
use merc_typecheck::TypingInfo;
use merc_typecheck::internal::ResolvedSortId;
use merc_typecheck::internal::checking;

use crate::ModalError;
use crate::check;
use crate::modal_equation_system::ModalEquationSystem;

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
    /// The formula's own checked, flattened fixpoint-equation view.
    equation_system: ModalEquationSystem,
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
        // A pure syntactic pass, before anything else needs `spec`.
        let mut state_var_ids = resolve_modal_variables(&mut spec.formula);

        let data_spec = std::mem::take(&mut spec.data_specification);
        let mut data = DataSpecification::from_untyped_with(data_spec, encoding, sources)?;

        // Flattens the formula's nested `mu`/`nu`s into a ranked list of equations up front.
        let equation_system = ModalEquationSystem::new(&spec.formula, &mut state_var_ids);

        let tables = DeclarationTables::build(
            &mut data,
            &spec.action_declarations,
            equation_system,
            resolve_declared_sort,
        )?;
        let typing = check::check_modal_specification(&mut data, &tables, &spec, formula_type)?;

        Ok(ModalSpecification {
            spec,
            data,
            typing,
            formula_type,
            equation_system: tables.equation_system,
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

    /// The formula's own checked, flattened fixpoint-equation view — the intended input for
    /// anything that wants to *use* the type-checked formula (e.g. `merc_vpg::translate`), rather
    /// than [`Self::formula`]'s raw nested tree.
    pub fn equation_system(&self) -> &ModalEquationSystem {
        &self.equation_system
    }

    /// Every checked expression's typing across the *whole* specification.
    pub fn typing_info(&self) -> TypingInfo {
        let mut info = self.data.typing_info();
        info.merge(self.typing.clone());
        info
    }

    /// The sort of this modal formula.
    pub fn val_sort(&self) -> FormulaType {
        self.formula_type
    }
}

/// How a state formula's action references are checked: decided once for the whole spec, from
/// whether it declares any `act` at all, rather than inferred per call site from an incidentally
/// empty [`checking::ActionTable`].
pub(super) enum ActionChecking {
    /// At least one `act` was declared: every reference must resolve against the table.
    Declared(checking::ActionTable),
    /// No `act` at all: every action is a "simple action" — a plain LTS label, matched
    /// structurally rather than typed.
    Simple,
}

/// The resolved `act` declaration table, plus the formula's own flattened fixpoint-equation view
/// and the resolved sorts of each equation's parameters — built once by [`Self::build`] and used by
/// [`super::check`]'s per-equation walk to resolve every action instance and fixpoint-variable
/// reference it reaches.
pub(super) struct DeclarationTables {
    pub(super) actions: ActionChecking,
    /// The formula's own equations, in the shape [`super::check`]'s per-equation loop walks.
    pub(super) equation_system: ModalEquationSystem,
    /// The resolved sort of each parameter of each equation: `parameter_sorts[i][j]` is the sort
    /// of the `j`th argument of the variable of the `i`th equation of `equation_system`.
    pub(super) parameter_sorts: Vec<Vec<ResolvedSortId>>,
    /// A fixpoint variable's own [`StateVarId`] -> index into `equation_system`'s own equations
    /// and `parameter_sorts`.
    pub(super) equations_by_id: HashMap<StateVarId, usize>,
}

impl DeclarationTables {
    fn build<F>(
        data: &mut DataSpecification,
        action_declarations: &[ActDecl],
        equation_system: ModalEquationSystem,
        mut resolve_declared_sort: F,
    ) -> Result<Self, ModalError>
    where
        F: FnMut(&mut DataSpecification, &SortExpression) -> Result<ResolvedSortId, ModalError>,
    {
        let actions = if action_declarations.is_empty() {
            ActionChecking::Simple
        } else {
            ActionChecking::Declared(checking::ActionTable::build(
                data,
                action_declarations,
                &mut resolve_declared_sort,
            )?)
        };

        let mut parameter_sorts = Vec::with_capacity(equation_system.len());
        let mut equations_by_id = HashMap::with_capacity(equation_system.len());
        for (index, equation) in equation_system.iter().enumerate() {
            let variable = equation.variable();

            let mut sorts = Vec::with_capacity(variable.arguments.len());
            let mut seen = HashSet::new();
            for argument in &variable.arguments {
                if !seen.insert(argument.decl.identifier.as_str()) {
                    return Err(ModalError::DuplicateFixedPointParameter {
                        variable: variable.identifier.node.clone(),
                        name: argument.decl.identifier.node.clone(),
                        span: argument.decl.identifier.span.clone(),
                    });
                }
                sorts.push(resolve_declared_sort(data, &argument.decl.sort)?);
            }

            // `ModalEquationSystem` itself already guarantees every equation's own `StateVarId`
            // is unique (it's built from a resolved formula), so this can't collide.
            let id = variable.id.expect("ModalEquationSystem requires a resolved formula");
            equations_by_id.insert(id, index);
            parameter_sorts.push(sorts);
        }

        Ok(DeclarationTables {
            actions,
            equation_system,
            parameter_sorts,
            equations_by_id,
        })
    }
}

/// Resolves a sort expression occurring in an `act`/fixpoint-variable-parameter/binder declaration:
/// rejects an anonymous `struct` (never legal here), then defers to
/// [`DataSpecification::resolve_declared_sort`] for the rest.
pub(super) fn resolve_declared_sort(
    data: &mut DataSpecification,
    sort: &SortExpression,
) -> Result<ResolvedSortId, ModalError> {
    checking::resolve_declared_sort(data, sort, |span| ModalError::AnonymousStructInDeclaration { span })
}
