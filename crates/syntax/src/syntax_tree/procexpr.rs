use merc_utilities::Span;

use crate::spanned::Spanned;

use super::ActionName;
use super::Assignment;
use super::CommExpr;
use super::DataExpr;
use super::IdDecl;
use super::MultiActionLabel;
use super::Rename;

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
