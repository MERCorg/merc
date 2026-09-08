use std::hash::Hash;

use merc_utilities::Span;
use merc_utilities::TagIndex;

use crate::spanned::Spanned;

/// A unique type for sort declarations.
pub struct DefTag;

/// The index type for a sort declaration, assigned during name resolution.
pub type DefId = TagIndex<usize, DefTag>;

/// A unique type for constructor declarations.
pub struct ConstructorTag;

/// The index type for a constructor declaration, local to
/// `UntypedDataSpecification::constructor_declarations`.
pub type ConstructorId = TagIndex<usize, ConstructorTag>;

/// A unique type for map declarations.
pub struct MapTag;

/// The index type for a map declaration, local to
/// `UntypedDataSpecification::map_declarations`.
pub type MapId = TagIndex<usize, MapTag>;

/// A unique type for equation specification blocks (`var ... eqn ...`).
pub struct EqnSpecTag;

/// The index type for an equation specification block, local to
/// `UntypedDataSpecification::equation_declarations`.
pub type EqnSpecId = TagIndex<usize, EqnSpecTag>;

/// A unique type for equation declarations.
pub struct EquationTag;

/// The index type for a single equation, local to its enclosing `EqnSpec`.
pub type EquationId = TagIndex<usize, EquationTag>;

/// A unique type for equation variable declarations.
pub struct EqnVarTag;

/// The index type for a variable in an equation block, local to its enclosing
/// [EqnSpec]. Assigned during declaration-id resolution.
pub type EqnVarId = TagIndex<usize, EqnVarTag>;

/// A complete mCRL2 process specification.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedProcessSpecification {
    pub data_specification: UntypedDataSpecification,
    pub global_variables: Vec<IdDecl>,
    pub action_declarations: Vec<ActDecl>,
    pub process_declarations: Vec<ProcDecl>,
    pub init: Option<ProcessExpr>,
}

/// An mCRL2 data specification.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct UntypedDataSpecification {
    pub sort_declarations: Vec<SortDecl>,
    pub constructor_declarations: Vec<IdDecl<ConstructorId>>,
    pub map_declarations: Vec<IdDecl<MapId>>,
    pub equation_declarations: Vec<EqnSpec>,
}

impl UntypedDataSpecification {
    /// Returns true if the data specification is empty.
    pub fn is_empty(&self) -> bool {
        self.sort_declarations.is_empty()
            && self.constructor_declarations.is_empty()
            && self.map_declarations.is_empty()
            && self.equation_declarations.is_empty()
    }

    /// Merges another data specification into the current one.
    pub fn merge(&mut self, other_spec: &UntypedDataSpecification) {
        self.sort_declarations.extend_from_slice(&other_spec.sort_declarations);
        self.constructor_declarations
            .extend_from_slice(&other_spec.constructor_declarations);
        self.map_declarations.extend_from_slice(&other_spec.map_declarations);
        self.equation_declarations
            .extend_from_slice(&other_spec.equation_declarations);
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

/// A propositional-variable identifier occurrence.
pub type PropVarName = Spanned<String>;

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

/// A declaration of an identifier with its sort.
///
/// Reused for every "name: sort" binding in the grammar. It defaults to [DefId]
/// for the binder-like uses that never assign one, and is instantiated with
/// [ConstructorId] or [MapId] where appropriate.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct IdDecl<Id = DefId> {
    /// Identifier being declared.
    pub identifier: Spanned<String>,
    /// Sort expression for this identifier
    pub sort: SortExpression,
    /// Unique ID assigned to this declaration during name/id resolution.
    pub id: Option<Id>,
}

impl<Id> IdDecl<Id> {
    /// Creates a new identifier declaration with the given identifier, sort, and the identifier's
    /// own span.
    pub fn new(identifier: String, sort: SortExpression, span: Span) -> Self {
        IdDecl {
            identifier: Spanned { node: identifier, span },
            sort,
            id: None,
        }
    }

    /// Reinterprets this declaration under a different id type.
    pub fn retag<NewId>(self) -> IdDecl<NewId> {
        IdDecl {
            identifier: self.identifier,
            sort: self.sort,
            id: None,
        }
    }
}

/// The kind of a [SortExpression] node.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SortExpressionKind {
    /// Product of two sorts (A # B)
    Product {
        lhs: Box<SortExpression>,
        rhs: Box<SortExpression>,
    },
    /// Function sort (A -> B)
    Function {
        domain: Box<SortExpression>,
        range: Box<SortExpression>,
    },
    Struct {
        inner: Vec<ConstructorDecl>,
    },
    /// Reference to a named sort
    Reference(String),
    /// Built-in simple sort
    Simple(Sort),
    /// Parameterized complex sort
    Complex(ComplexSort, Box<SortExpression>),
    /// Resolved reference to a sort after name resolution
    Resolved(String, DefId),
    /// Function sort (A_0 # ... # A_n -> B) after flattening (performed during name resolution)
    FlattenedFunction {
        domain: Vec<SortExpression>,
        range: Box<SortExpression>,
    },
}

/// A sort expression paired with the source [Span] it was parsed from.
pub type SortExpression = Spanned<SortExpressionKind>;

impl SortExpressionKind {
    /// Wraps this kind together with a source `span` into a [SortExpression].
    pub fn spanned(self, span: Span) -> SortExpression {
        Spanned { node: self, span }
    }
}

impl From<SortExpressionKind> for SortExpression {
    /// For synthetic expressions that have no source location.
    fn from(kind: SortExpressionKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

/// Constructor declaration
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ConstructorDecl {
    /// The constructor's own name (`c1`), with its declaration span.
    pub name: Spanned<String>,
    /// Each argument's optional projection-function name.
    pub args: Vec<(Option<Spanned<String>>, SortExpression)>,
    /// The recogniser function's name.
    pub projection: Option<Spanned<String>>,
}

/// Built-in simple sorts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Sort {
    Bool,
    Pos,
    Int,
    Nat,
    Real,
}

/// Complex (parameterized) sorts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ComplexSort {
    List,
    Set,
    FSet,
    FBag,
    Bag,
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
    pub id: Option<DefId>,
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
    pub variables: Vec<IdDecl<EqnVarId>>,
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

/// A process-declaration identifier occurrence.
pub type ProcessName = Spanned<String>;

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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum DataExprUnaryOp {
    Negation,
    Minus,
    Size,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum DataExprBinaryOp {
    Conj,
    Disj,
    Implies,
    Equal,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
    Cons,
    Snoc,
    In,
    Concat,
    Add,
    Subtract,
    Div,
    IntDiv,
    Mod,
    Multiply,
    At,
}

/// The kind of a [DataExpr] node.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum DataExprKind {
    Id(String),
    /// A variable reference paired with its declaring binder's own span: not this
    /// occurrence's span.
    Resolved(String, Span),
    Number(String), // Is string because the number can be any size.
    Bool(bool),
    Application {
        function: Box<DataExpr>,
        arguments: Vec<DataExpr>,
    },
    EmptyList,
    List(Vec<DataExpr>),
    EmptySet,
    Set(Vec<DataExpr>),
    EmptyBag,
    Bag(Vec<BagElement>),
    SetBagComp {
        variable: IdDecl,
        predicate: Box<DataExpr>,
    },
    Lambda {
        variables: Vec<IdDecl>,
        body: Box<DataExpr>,
    },
    Quantifier {
        op: Quantifier,
        variables: Vec<IdDecl>,
        body: Box<DataExpr>,
    },
    Unary {
        op: DataExprUnaryOp,
        expr: Box<DataExpr>,
    },
    Binary {
        op: DataExprBinaryOp,
        lhs: Box<DataExpr>,
        rhs: Box<DataExpr>,
    },
    FunctionUpdate {
        expr: Box<DataExpr>,
        update: Box<DataExprUpdate>,
    },
    Whr {
        expr: Box<DataExpr>,
        assignments: Vec<Assignment>,
    },
}

/// A data expression paired with the source [Span] it was
/// parsed from.
pub type DataExpr = Spanned<DataExprKind>;

impl DataExprKind {
    /// Wraps this kind together with a source `span`.
    pub fn spanned(self, span: Span) -> DataExpr {
        Spanned { node: self, span }
    }
}

impl From<DataExprKind> for DataExpr {
    /// For synthetic expressions that have no source location.
    fn from(kind: DataExprKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BagElement {
    pub expr: DataExpr,
    pub multiplicity: DataExpr,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct DataExprUpdate {
    pub expr: DataExpr,
    pub update: DataExpr,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct AssignmentData {
    pub identifier: String,
    pub expr: DataExpr,
}

/// A process-instantiation assignment (`x = e`, as in `P(x = 1)`), paired with the source [Span]
/// it was parsed from. Equality/ordering/hashing ignore the span, per [Spanned]'s documented
/// convention.
pub type Assignment = Spanned<AssignmentData>;

impl AssignmentData {
    /// Wraps this data together with a source `span`.
    pub fn spanned(self, span: Span) -> Assignment {
        Spanned { node: self, span }
    }
}

impl Assignment {
    /// Creates a new assignment with the given identifier and expression, with a default (empty)
    /// span.
    pub fn new(identifier: String, expr: DataExpr) -> Self {
        AssignmentData { identifier, expr }.spanned(Span::default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProcExprBinaryOp {
    Sequence,
    Choice,
    Parallel,
    LeftMerge,
    CommMerge,
    Until,
}

/// The kind of a [ProcessExpr] node, without its source span. Every recursive
/// child is a [ProcessExpr] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ProcessExprKind {
    // Both `Id`'s and `Action`'s own name keep the [Span] they were parsed from.
    Id(ActionName, Vec<Assignment>),
    Action(ActionName, Vec<DataExpr>),
    Delta,
    Tau,
    Sum {
        variables: Vec<IdDecl>,
        operand: Box<ProcessExpr>,
    },
    Dist {
        variables: Vec<IdDecl>,
        expr: DataExpr,
        operand: Box<ProcessExpr>,
    },
    Binary {
        op: ProcExprBinaryOp,
        lhs: Box<ProcessExpr>,
        rhs: Box<ProcessExpr>,
    },
    Hide {
        // Each action name keeps the [Span] it was parsed from, so a later pass can point at the
        // individual name rather than the whole `hide(...)` expression.
        actions: Vec<ActionName>,
        operand: Box<ProcessExpr>,
    },
    Rename {
        renames: Vec<Rename>,
        operand: Box<ProcessExpr>,
    },
    Allow {
        actions: Vec<MultiActionLabel>,
        operand: Box<ProcessExpr>,
    },
    Block {
        // See [ProcessExprKind::Hide] for why each name carries its own [Span].
        actions: Vec<ActionName>,
        operand: Box<ProcessExpr>,
    },
    Comm {
        comm: Vec<CommExpr>,
        operand: Box<ProcessExpr>,
    },
    Condition {
        condition: DataExpr,
        then: Box<ProcessExpr>,
        else_: Option<Box<ProcessExpr>>,
    },
    At {
        expr: Box<ProcessExpr>,
        operand: DataExpr,
    },
}

/// A process expression: a [ProcessExprKind] paired with the source [Span] it
/// was parsed from. Synthetic expressions built by later passes use
/// [Span::default].
pub type ProcessExpr = Spanned<ProcessExprKind>;

impl ProcessExprKind {
    /// Wraps this kind together with a source `span` into a [ProcessExpr].
    pub fn spanned(self, span: Span) -> ProcessExpr {
        Spanned { node: self, span }
    }
}

impl From<ProcessExprKind> for ProcessExpr {
    /// Wraps a kind into a [ProcessExpr] with a default (empty) span, for
    /// synthetic expressions that have no source location.
    fn from(kind: ProcessExprKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct UntypedStateFrmSpec {
    pub data_specification: UntypedDataSpecification,
    pub action_declarations: Vec<ActDecl>,
    pub formula: StateFrm,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum StateFrmUnaryOp {
    Minus,
    Negation,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum StateFrmOp {
    Addition,
    Implies,
    Disjunction,
    Conjunction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FixedPointOperator {
    Least,
    Greatest,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StateVarDecl {
    pub identifier: String,
    pub arguments: Vec<StateVarAssignment>,
    pub span: Span,
}

impl StateVarDecl {
    /// Creates a new state variable declaration.
    pub fn new(identifier: String, arguments: Vec<StateVarAssignment>) -> Self {
        StateVarDecl {
            identifier,
            arguments,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StateVarAssignment {
    /// The parameter's own name.
    pub identifier: Spanned<String>,
    pub sort: SortExpression,
    pub expr: DataExpr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ModalityOperator {
    Diamond,
    Box,
}

/// The kind of a [StateFrm] node, without its source span. Every recursive
/// child is a [StateFrm] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum StateFrmKind {
    True,
    False,
    /// `delay` or `delay@t`; the optional time is `None` for a bare `delay`.
    Delay(Option<DataExpr>),
    /// `yaled` or `yaled@t`; the optional time is `None` for a bare `yaled`.
    Yaled(Option<DataExpr>),
    Id(String, Vec<DataExpr>),
    /// A fixpoint-variable reference resolved to its declaring `mu`/`nu`.
    Resolved(String, Vec<DataExpr>, Span),
    DataValExprLeftMult(DataExpr, Box<StateFrm>),
    DataValExprRightMult(Box<StateFrm>, DataExpr),
    DataValExpr(DataExpr),
    Modality {
        operator: ModalityOperator,
        formula: RegFrm,
        expr: Box<StateFrm>,
    },
    Unary {
        op: StateFrmUnaryOp,
        expr: Box<StateFrm>,
    },
    Binary {
        op: StateFrmOp,
        lhs: Box<StateFrm>,
        rhs: Box<StateFrm>,
    },
    Quantifier {
        quantifier: Quantifier,
        variables: Vec<IdDecl>,
        body: Box<StateFrm>,
    },
    Bound {
        bound: Bound,
        variables: Vec<IdDecl>,
        body: Box<StateFrm>,
    },
    FixedPoint {
        operator: FixedPointOperator,
        variable: StateVarDecl,
        body: Box<StateFrm>,
    },
}

/// A state formula: a [StateFrmKind] paired with the source [Span] it was
/// parsed from. Synthetic formulas built by later passes use [Span::default].
pub type StateFrm = Spanned<StateFrmKind>;

impl StateFrmKind {
    /// Wraps this kind together with a source `span` into a [StateFrm].
    pub fn spanned(self, span: Span) -> StateFrm {
        Spanned { node: self, span }
    }
}

impl From<StateFrmKind> for StateFrm {
    /// Wraps a kind into a [StateFrm] with a default (empty) span, for
    /// synthetic formulas that have no source location.
    fn from(kind: StateFrmKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

/// An identifier occurrence naming an action or process, paired with the [Span] it was parsed
/// from, so a later pass can point at the individual name rather than the whole enclosing
/// expression. Used both for a name inside a `hide`/`block`/`allow`/`comm`/`rename` set and for
/// `ProcessExprKind::Action`/`Id`'s own name. Equality, ordering and hashing ignore the span
/// (see [Spanned]).
pub type ActionName = Spanned<String>;

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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Quantifier {
    Exists,
    Forall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ActFrmBinaryOp {
    Implies,
    Union,
    Intersect,
}

/// The kind of an [ActFrm] node, without its source span. Every recursive
/// child is an [ActFrm] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ActFrmKind {
    True,
    False,
    MultAct(MultiAction),
    DataExprVal(DataExpr),
    Negation(Box<ActFrm>),
    Quantifier {
        quantifier: Quantifier,
        variables: Vec<IdDecl>,
        body: Box<ActFrm>,
    },
    Binary {
        op: ActFrmBinaryOp,
        lhs: Box<ActFrm>,
        rhs: Box<ActFrm>,
    },
}

/// An action formula: an [ActFrmKind] paired with the source [Span] it was
/// parsed from. Synthetic formulas built by later passes use [Span::default].
pub type ActFrm = Spanned<ActFrmKind>;

impl ActFrmKind {
    /// Wraps this kind together with a source `span` into an [ActFrm].
    pub fn spanned(self, span: Span) -> ActFrm {
        Spanned { node: self, span }
    }
}

impl From<ActFrmKind> for ActFrm {
    /// Wraps a kind into an [ActFrm] with a default (empty) span, for
    /// synthetic formulas that have no source location.
    fn from(kind: ActFrmKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

/// The kind of a [PbesExpr] node, without its source span. Every recursive
/// child is a [PbesExpr] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PbesExprKind {
    DataValExpr(DataExpr),
    PropVarInst(PropVarInst),
    Quantifier {
        quantifier: Quantifier,
        variables: Vec<IdDecl>,
        body: Box<PbesExpr>,
    },
    Negation(Box<PbesExpr>),
    Binary {
        op: PbesExprBinaryOp,
        lhs: Box<PbesExpr>,
        rhs: Box<PbesExpr>,
    },
    True,
    False,
}

/// A PBES expression: a [PbesExprKind] paired with the source [Span] it was
/// parsed from. Synthetic expressions built by later passes use
/// [Span::default].
pub type PbesExpr = Spanned<PbesExprKind>;

impl PbesExprKind {
    /// Wraps this kind together with a source `span` into a [PbesExpr].
    pub fn spanned(self, span: Span) -> PbesExpr {
        Spanned { node: self, span }
    }
}

impl From<PbesExprKind> for PbesExpr {
    /// Wraps a kind into a [PbesExpr] with a default (empty) span, for
    /// synthetic expressions that have no source location.
    fn from(kind: PbesExprKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Eq {
    EqInf,
    EqnInf,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Condition {
    Condsm,
    Condeq,
}

// TODO: What should this be called?
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Bound {
    Inf,
    Sup,
    Sum,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PresExprBinaryOp {
    Implies,
    Disjunction,
    Conjunction,
    Add,
}

/// The kind of a [PresExpr] node, without its source span. Every recursive
/// child is a [PresExpr] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PresExprKind {
    DataValExpr(DataExpr),
    PropVarInst(PropVarInst),
    RightConstantMultiply {
        expr: Box<PresExpr>,
        constant: DataExpr,
    },
    LeftConstantMultiply {
        constant: DataExpr,
        expr: Box<PresExpr>,
    },
    Bound {
        op: Bound,
        variables: Vec<IdDecl>,
        expr: Box<PresExpr>,
    },
    Equal {
        eq: Eq,
        body: Box<PresExpr>,
    },
    Condition {
        condition: Condition,
        lhs: Box<PresExpr>,
        then: Box<PresExpr>,
        else_: Box<PresExpr>,
    },
    Negation(Box<PresExpr>),
    Binary {
        op: PresExprBinaryOp,
        lhs: Box<PresExpr>,
        rhs: Box<PresExpr>,
    },
    True,
    False,
}

/// A PRES expression: a [PresExprKind] paired with the source [Span] it was
/// parsed from. Synthetic expressions built by later passes use
/// [Span::default].
pub type PresExpr = Spanned<PresExprKind>;

impl PresExprKind {
    /// Wraps this kind together with a source `span` into a [PresExpr].
    pub fn spanned(self, span: Span) -> PresExpr {
        Spanned { node: self, span }
    }
}

impl From<PresExprKind> for PresExpr {
    /// Wraps a kind into a [PresExpr] with a default (empty) span, for
    /// synthetic expressions that have no source location.
    fn from(kind: PresExprKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PbesExprBinaryOp {
    Implies,
    Disjunction,
    Conjunction,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PresEquation {
    pub operator: FixedPointOperator,
    pub variable: PropVarDecl,
    pub formula: PresExpr,
    pub span: Span,
}

/// The kind of a [RegFrm] node, without its source span. Every recursive
/// child is a [RegFrm] (a [Spanned] wrapper), so each node carries its own
/// location.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RegFrmKind {
    Action(ActFrm),
    Iteration(Box<RegFrm>),
    Plus(Box<RegFrm>),
    Sequence { lhs: Box<RegFrm>, rhs: Box<RegFrm> },
    Choice { lhs: Box<RegFrm>, rhs: Box<RegFrm> },
}

/// A regular formula: a [RegFrmKind] paired with the source [Span] it was
/// parsed from. Synthetic formulas built by later passes use [Span::default].
pub type RegFrm = Spanned<RegFrmKind>;

impl RegFrmKind {
    /// Wraps this kind together with a source `span` into a [RegFrm].
    pub fn spanned(self, span: Span) -> RegFrm {
        Spanned { node: self, span }
    }
}

impl From<RegFrmKind> for RegFrm {
    /// Wraps a kind into a [RegFrm] with a default (empty) span, for
    /// synthetic formulas that have no source location.
    fn from(kind: RegFrmKind) -> Self {
        Spanned {
            node: kind,
            span: Span::default(),
        }
    }
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
