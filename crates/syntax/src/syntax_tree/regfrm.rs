use merc_utilities::Span;

use crate::spanned::Spanned;

use super::ActFrm;

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
