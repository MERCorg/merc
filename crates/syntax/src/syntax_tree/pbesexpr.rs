use merc_utilities::Span;

use crate::spanned::Spanned;

use super::DataExpr;
use super::IdDecl;
use super::PropVarInst;
use super::Quantifier;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PbesExprBinaryOp {
    Implies,
    Disjunction,
    Conjunction,
}
