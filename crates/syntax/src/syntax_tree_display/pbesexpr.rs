use std::fmt;

use itertools::Itertools;

use crate::PbesExpr;
use crate::PbesExprBinaryOp;
use crate::PbesExprKind;

impl fmt::Display for PbesExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            PbesExprKind::True => write!(f, "true"),
            PbesExprKind::False => write!(f, "false"),
            PbesExprKind::PropVarInst(instance) => write!(f, "{instance}"),
            PbesExprKind::Negation(expr) => write!(f, "(! {expr})"),
            PbesExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            PbesExprKind::Quantifier {
                quantifier,
                variables,
                body,
            } => write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body),
            PbesExprKind::DataValExpr(data_expr) => write!(f, "val({data_expr})"),
        }
    }
}

impl fmt::Display for PbesExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PbesExprBinaryOp::Conjunction => write!(f, "&&"),
            PbesExprBinaryOp::Disjunction => write!(f, "||"),
            PbesExprBinaryOp::Implies => write!(f, "=>"),
        }
    }
}
