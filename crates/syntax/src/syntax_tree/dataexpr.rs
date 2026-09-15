use merc_utilities::Span;

use crate::spanned::Spanned;

use super::IdDecl;
use super::Quantifier;
use super::VarId;

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
    /// A variable reference paired with its declaring binder's own [VarId]: not this
    /// occurrence's identity.
    Resolved(String, VarId),
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
    /// Assigned during variable resolution when this assignment is a `whr` binding (a new
    /// variable, in scope for the body).
    pub id: Option<VarId>,
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
        AssignmentData {
            identifier,
            expr,
            id: None,
        }
        .spanned(Span::default())
    }
}
