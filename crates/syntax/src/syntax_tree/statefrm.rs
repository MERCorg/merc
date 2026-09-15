use merc_utilities::Span;

use crate::spanned::Spanned;

use super::Bound;
use super::DataExpr;
use super::IdDecl;
use super::Quantifier;
use super::RegFrm;
use super::SortExpression;
use super::StateVarId;
use super::VarId;

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
    /// Assigned during variable resolution; see [StateVarId].
    pub id: Option<StateVarId>,
}

impl StateVarDecl {
    /// Creates a new state variable declaration.
    pub fn new(identifier: String, arguments: Vec<StateVarAssignment>) -> Self {
        StateVarDecl {
            identifier,
            arguments,
            span: Span::default(),
            id: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StateVarAssignment {
    /// The parameter's own name.
    pub identifier: Spanned<String>,
    pub sort: SortExpression,
    pub expr: DataExpr,
    /// Assigned during variable resolution; see [VarId].
    pub id: Option<VarId>,
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
    /// A fixpoint-variable reference resolved to its declaring `mu`/`nu`'s own [StateVarId].
    Resolved(String, Vec<DataExpr>, StateVarId),
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
