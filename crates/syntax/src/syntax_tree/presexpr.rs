use merc_utilities::Span;

use crate::spanned::Spanned;

use super::Bound;
use super::Condition;
use super::DataExpr;
use super::Eq;
use super::IdDecl;
use super::PropVarInst;

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
pub enum PresExprBinaryOp {
    Implies,
    Disjunction,
    Conjunction,
    Add,
}
