use std::hash::Hash;

use merc_utilities::Span;

use crate::spanned::Spanned;

use super::ActionName;
use super::ConstructorId;
use super::DataExpr;
use super::EqnSpecId;
use super::EquationId;
use super::FixedPointOperator;
use super::IdDecl;
use super::MapId;
use super::PbesExpr;
use super::PresExpr;
use super::ProcessExpr;
use super::ProcessName;
use super::PropVarName;
use super::SortExpression;
use super::SortId;
use super::StateFrm;
use super::TypeVarId;

/// A complete mCRL2 process specification.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedProcessSpecification {
    pub data_specification: UntypedDataSpecification,
    pub global_variables: Vec<IdDecl>,
    pub action_declarations: Vec<ActDecl>,
    pub process_declarations: Vec<ProcDecl>,
    pub init: Option<ProcessExpr>,
}

impl UntypedProcessSpecification {
    /// Merges another process specification's declarations into this one.
    ///
    /// `other.init` is discarded: the importing file's own `init` always wins.
    /// A file meant to be imported would not usually declare one anyway.
    pub fn merge(&mut self, other: &UntypedProcessSpecification) {
        self.data_specification.merge(&other.data_specification);
        self.global_variables.extend_from_slice(&other.global_variables);
        self.action_declarations.extend_from_slice(&other.action_declarations);
        self.process_declarations.extend_from_slice(&other.process_declarations);
    }
}

/// An mCRL2 data specification.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedDataSpecification {
    pub sort_declarations: Vec<SortDecl>,
    pub constructor_declarations: Vec<IdDecl<ConstructorId>>,
    pub map_declarations: Vec<IdDecl<MapId>>,
    pub equation_declarations: Vec<EqnSpec>,
    pub type_var_declarations: Vec<TypeVarDecl>,
}

impl UntypedDataSpecification {
    /// Returns true if the data specification is empty.
    pub fn is_empty(&self) -> bool {
        self.sort_declarations.is_empty()
            && self.constructor_declarations.is_empty()
            && self.map_declarations.is_empty()
            && self.equation_declarations.is_empty()
            && self.type_var_declarations.is_empty()
    }

    /// Merges another data specification into the current one.
    pub fn merge(&mut self, other_spec: &UntypedDataSpecification) {
        self.sort_declarations.extend_from_slice(&other_spec.sort_declarations);
        self.constructor_declarations
            .extend_from_slice(&other_spec.constructor_declarations);
        self.map_declarations.extend_from_slice(&other_spec.map_declarations);
        self.equation_declarations
            .extend_from_slice(&other_spec.equation_declarations);
        self.type_var_declarations
            .extend_from_slice(&other_spec.type_var_declarations);
    }
}

/// A bound sort (type) variable's own declaration, introduced by a `type_var` block.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TypeVarDecl {
    /// The type variable's own name (`S`).
    pub identifier: String,
    /// Where the type variable is declared.
    pub span: Span,
    /// Unique ID assigned to this declaration during name resolution.
    pub id: Option<TypeVarId>,
}

impl TypeVarDecl {
    /// Creates a new type variable declaration with the given identifier and span.
    pub fn new(identifier: String, span: Span) -> Self {
        TypeVarDecl {
            identifier,
            span,
            id: None,
        }
    }
}

/// An mCRL2 parameterised boolean equation system (PBES).
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedPbes {
    pub data_specification: UntypedDataSpecification,
    pub global_variables: Vec<IdDecl>,
    pub equations: Vec<PbesEquation>,
    pub init: PropVarInst,
}

/// An mCRL2 parameterised real equation system (PRES).
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedPres {
    pub data_specification: UntypedDataSpecification,
    pub global_variables: Vec<IdDecl>,
    pub equations: Vec<PresEquation>,
    pub init: PropVarInst,
}

/// A `pbes`/`pres` equation's own declaration.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PropVarDecl {
    pub identifier: PropVarName,
    pub parameters: Vec<IdDecl>,
    pub span: Span,
}

impl PropVarDecl {
    /// Creates a new propositional variable declaration with the given identifier and parameters.
    pub fn new(identifier: String, parameters: Vec<IdDecl>) -> Self {
        PropVarDecl {
            identifier: PropVarName {
                node: identifier,
                span: Span::default(),
            },
            parameters,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct PropVarInstData {
    pub identifier: PropVarName,
    pub arguments: Vec<DataExpr>,
}

/// A propositional-variable instantiation, paired with the source [Span] it was parsed from.
/// Equality/ordering/hashing ignore the span, per [Spanned]'s documented convention.
pub type PropVarInst = Spanned<PropVarInstData>;

impl PropVarInstData {
    /// Wraps this data together with a source `span`.
    pub fn spanned(self, span: Span) -> PropVarInst {
        Spanned { node: self, span }
    }
}

impl PropVarInst {
    /// Creates a new instance of a propositional variable with the given identifier and
    /// arguments. Both the instantiation and the identifier itself get [Span::default], for a
    /// synthetic instantiation with no source location.
    pub fn new(identifier: String, arguments: Vec<DataExpr>) -> Self {
        PropVarInstData {
            identifier: PropVarName {
                node: identifier,
                span: Span::default(),
            },
            arguments,
        }
        .spanned(Span::default())
    }
}

/// Sort declaration
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SortDecl {
    /// Sort identifier
    pub identifier: String,
    /// Sort expression (if structured)
    pub expr: Option<SortExpression>,
    /// Where the sort is defined
    pub span: Span,
    /// Unique ID assigned to this declaration during name resolution.
    pub id: Option<SortId>,
}

impl SortDecl {
    /// Creates a new sort declaration with the given identifier, expression, and span.
    pub fn new(identifier: String, expr: Option<SortExpression>, span: Span) -> Self {
        SortDecl {
            identifier,
            expr,
            span,
            id: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EqnSpecData {
    pub variables: Vec<IdDecl>,
    pub equations: Vec<EqnDecl>,
    /// Unique ID assigned to this block during declaration-id resolution.
    pub id: Option<EqnSpecId>,
}

/// An equation-specification block (`var ... eqn ...`), paired with the source [Span] of the
/// whole block, from `var`/`eqn` (whichever comes first) to at least the final `;`.
/// Equality/ordering/hashing ignore the span, per [Spanned]'s documented convention.
pub type EqnSpec = Spanned<EqnSpecData>;

impl EqnSpecData {
    /// Wraps this data together with a source `span`.
    pub fn spanned(self, span: Span) -> EqnSpec {
        Spanned { node: self, span }
    }
}

/// Equation declaration
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EqnDecl {
    pub condition: Option<DataExpr>,
    pub lhs: DataExpr,
    pub rhs: DataExpr,
    pub span: Span,
    /// Unique ID assigned to this equation during declaration-id resolution,
    /// local to its enclosing [EqnSpec].
    pub id: Option<EquationId>,
}

/// Action declaration.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ActDecl {
    pub identifier: ActionName,
    pub args: Vec<SortExpression>,
    pub span: Span,
}

/// Process declaration.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProcDecl {
    pub identifier: ProcessName,
    pub params: Vec<IdDecl>,
    pub body: ProcessExpr,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct UntypedStateFrmSpec {
    pub data_specification: UntypedDataSpecification,
    pub action_declarations: Vec<ActDecl>,
    pub formula: StateFrm,
}

/// Represents a multi action label `a | b | c ...`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MultiActionLabel {
    pub actions: Vec<ActionName>,
}

impl MultiActionLabel {
    /// Creates a new multi-action label from a list of action identifiers.
    pub fn new(actions: Vec<ActionName>) -> Self {
        MultiActionLabel { actions }
    }

    /// Returns true if the multi-action label is empty (i.e., contains no actions).
    pub fn is_tau_label(&self) -> bool {
        self.actions.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Action {
    pub id: ActionName,
    pub args: Vec<DataExpr>,
}

impl Action {
    /// Creates a new action from an identifier and a list of arguments. `id` gets
    /// [Span::default], for a synthetic action with no source location.
    pub fn new(id: String, args: Vec<DataExpr>) -> Self {
        Action {
            id: ActionName {
                node: id,
                span: Span::default(),
            },
            args,
        }
    }
}

#[derive(Clone, Debug, Eq)]
pub struct MultiAction {
    pub actions: Vec<Action>,
}

impl MultiAction {
    /// Creates a new multi-action from a list of actions.
    pub fn new(actions: Vec<Action>) -> Self {
        MultiAction { actions }
    }

    /// Creates the empty multi-action, which represents the tau action.
    pub fn tau() -> Self {
        MultiAction { actions: Vec::new() }
    }
}

impl PartialEq for MultiAction {
    fn eq(&self, other: &Self) -> bool {
        // Check whether both multi-actions contain the same actions
        if self.actions.len() != other.actions.len() {
            return false;
        }

        // Map every action onto the other, equal length means they must be the same.
        for action in self.actions.iter() {
            if !other.actions.contains(action) {
                return false;
            }
        }

        true
    }
}

impl Hash for MultiAction {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let mut actions = self.actions.clone();
        // Sort the action ids to ensure that the hash is independent of the order.
        actions.sort();
        for action in actions {
            action.hash(state);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PbesEquation {
    pub operator: FixedPointOperator,
    pub variable: PropVarDecl,
    pub formula: PbesExpr,
    pub span: Span,
}

impl PbesEquation {
    /// Creates a new PBES equation with the given operator, variable and formula.
    pub fn new(operator: FixedPointOperator, variable: PropVarDecl, formula: PbesExpr) -> Self {
        PbesEquation {
            operator,
            variable,
            formula,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PresEquation {
    pub operator: FixedPointOperator,
    pub variable: PropVarDecl,
    pub formula: PresExpr,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Rename {
    pub from: ActionName,
    pub to: ActionName,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct CommExpr {
    pub from: MultiActionLabel,
    pub to: ActionName,
}

impl CommExpr {
    /// Creates a new communication expression from a multi-action label and a target action identifier.
    pub fn new(from: MultiActionLabel, to: ActionName) -> Self {
        CommExpr { from, to }
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct UntypedActionRenameSpec {
    pub data_specification: UntypedDataSpecification,
    pub action_declarations: Vec<ActDecl>,
    pub rename_declarations: Vec<ActionRenameDecl>,
}

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ActionRenameDecl {
    pub variables_specification: Vec<IdDecl>,
    pub rename_rule: ActionRenameRule,
}

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ActionRenameRule {
    pub condition: Option<DataExpr>,
    pub action: Action,
    pub rhs: ActionRHS,
}

#[derive(Debug, Eq, PartialEq, Hash)]
pub enum ActionRHS {
    Tau,
    Delta,
    Action(Action),
}
