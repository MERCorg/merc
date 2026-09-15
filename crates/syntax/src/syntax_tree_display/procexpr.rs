use std::fmt;

use itertools::Itertools;

use crate::ProcExprBinaryOp;
use crate::ProcessExpr;
use crate::ProcessExprKind;

impl fmt::Display for ProcExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcExprBinaryOp::Sequence => write!(f, "."),
            ProcExprBinaryOp::Choice => write!(f, "+"),
            ProcExprBinaryOp::Parallel => write!(f, "||"),
            ProcExprBinaryOp::LeftMerge => write!(f, "||_"),
            ProcExprBinaryOp::CommMerge => write!(f, "|"),
            ProcExprBinaryOp::Until => write!(f, "<<"),
        }
    }
}

impl fmt::Display for ProcessExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.node {
            ProcessExprKind::Id(identifier, assignments) => {
                if assignments.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, assignments.iter().format(", "))
                }
            }
            ProcessExprKind::Action(identifier, data_exprs) => {
                if data_exprs.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, data_exprs.iter().format(", "))
                }
            }
            ProcessExprKind::Delta => write!(f, "delta"),
            ProcessExprKind::Tau => write!(f, "tau"),
            ProcessExprKind::Sum { variables, operand } => {
                write!(f, "(sum {} . {})", variables.iter().format(", "), operand)
            }
            ProcessExprKind::Dist {
                variables,
                expr,
                operand,
            } => write!(f, "(dist {} [{}] . {})", variables.iter().format(", "), expr, operand),
            ProcessExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            ProcessExprKind::Hide { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "hide({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Rename { renames, operand } => {
                if !renames.is_empty() {
                    write!(f, "rename({{{}}}, {})", renames.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Allow { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "allow({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Block { actions, operand } => {
                if !actions.is_empty() {
                    write!(f, "block({{{}}}, {})", actions.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Comm { comm, operand } => {
                if !comm.is_empty() {
                    write!(f, "comm({{{}}}, {})", comm.iter().format(", "), operand)
                } else {
                    Ok(())
                }
            }
            ProcessExprKind::Condition { condition, then, else_ } => {
                // Wrap the whole conditional so it stays a single unit when it is
                // an operand of a higher-precedence operator such as sequence.
                if let Some(else_) = else_ {
                    write!(f, "(({condition}) -> ({then}) <> ({else_}))")
                } else {
                    write!(f, "(({condition}) -> ({then}))")
                }
            }
            ProcessExprKind::At { expr, operand } => write!(f, "({expr})@({operand})"),
        }
    }
}
