use std::fmt;

use itertools::Itertools;

use crate::ActFrm;
use crate::ActFrmBinaryOp;
use crate::ActFrmKind;

impl fmt::Display for ActFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            ActFrmKind::False => write!(f, "false"),
            ActFrmKind::True => write!(f, "true"),
            ActFrmKind::MultAct(action) => write!(f, "{action}"),
            ActFrmKind::Binary { op, lhs, rhs } => {
                // Wrap the whole expression (not just the operands) so that a
                // surrounding tighter operator such as `!` cannot re-associate.
                write!(f, "({lhs} {op} {rhs})")
            }
            ActFrmKind::DataExprVal(expr) => write!(f, "val({expr})"),
            ActFrmKind::Quantifier {
                quantifier,
                variables,
                body,
            } => write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body),
            ActFrmKind::Negation(expr) => write!(f, "(!{expr})"),
        }
    }
}

impl fmt::Display for ActFrmBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ActFrmBinaryOp::Implies => write!(f, "=>"),
            ActFrmBinaryOp::Intersect => write!(f, "&&"),
            ActFrmBinaryOp::Union => write!(f, "||"),
        }
    }
}
