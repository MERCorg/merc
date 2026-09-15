use merc_utilities::Span;

use crate::spanned::Spanned;

use super::DataExpr;
use super::IdDecl;
use super::MultiAction;
use super::Quantifier;

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
