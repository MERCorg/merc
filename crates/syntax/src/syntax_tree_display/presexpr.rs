use std::fmt;

use itertools::Itertools;

use crate::Condition;
use crate::Eq;
use crate::PresExpr;
use crate::PresExprBinaryOp;
use crate::PresExprKind;

impl fmt::Display for Eq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Eq::EqInf => write!(f, "eqinf"),
            Eq::EqnInf => write!(f, "eqninf"),
        }
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Condition::Condsm => write!(f, "condsm"),
            Condition::Condeq => write!(f, "condeq"),
        }
    }
}

impl fmt::Display for PresExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PresExprBinaryOp::Conjunction => write!(f, "&&"),
            PresExprBinaryOp::Disjunction => write!(f, "||"),
            PresExprBinaryOp::Implies => write!(f, "=>"),
            PresExprBinaryOp::Add => write!(f, "+"),
        }
    }
}

impl fmt::Display for PresExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            PresExprKind::True => write!(f, "true"),
            PresExprKind::False => write!(f, "false"),
            PresExprKind::PropVarInst(instance) => write!(f, "{instance}"),
            PresExprKind::DataValExpr(data_expr) => write!(f, "val({data_expr})"),
            PresExprKind::Negation(expr) => write!(f, "(- {expr})"),
            PresExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            PresExprKind::Bound { op, variables, expr } => {
                write!(f, "({} {} . {})", op, variables.iter().format(", "), expr)
            }
            PresExprKind::Equal { eq, body } => write!(f, "{eq}({body})"),
            PresExprKind::Condition {
                condition,
                lhs,
                then,
                else_,
            } => {
                write!(f, "{condition}({lhs}, {then}, {else_})")
            }
            PresExprKind::LeftConstantMultiply { constant, expr } => write!(f, "(val({constant}) * {expr})"),
            PresExprKind::RightConstantMultiply { expr, constant } => write!(f, "({expr} * val({constant}))"),
        }
    }
}
