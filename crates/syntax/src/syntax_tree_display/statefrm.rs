use std::fmt;

use itertools::Itertools;

use crate::FixedPointOperator;
use crate::ModalityOperator;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateFrmOp;
use crate::StateFrmUnaryOp;
use crate::StateVarAssignment;
use crate::StateVarDecl;

impl fmt::Display for StateFrmUnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StateFrmUnaryOp::Minus => write!(f, "-"),
            StateFrmUnaryOp::Negation => write!(f, "!"),
        }
    }
}

impl fmt::Display for FixedPointOperator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FixedPointOperator::Greatest => write!(f, "nu"),
            FixedPointOperator::Least => write!(f, "mu"),
        }
    }
}

impl fmt::Display for StateFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            StateFrmKind::True => write!(f, "true"),
            StateFrmKind::False => write!(f, "false"),
            StateFrmKind::DataValExpr(expr) => write!(f, "val({expr})"),
            StateFrmKind::Id(identifier, args) | StateFrmKind::Resolved(identifier, args, _) => {
                if args.is_empty() {
                    write!(f, "{identifier}")
                } else {
                    write!(f, "{}({})", identifier, args.iter().format(", "))
                }
            }
            StateFrmKind::Unary { op, expr } => write!(f, "({op} {expr})"),
            StateFrmKind::Modality {
                operator,
                formula,
                expr,
            } => match operator {
                ModalityOperator::Box => write!(f, "[{formula}]{expr}"),
                ModalityOperator::Diamond => write!(f, "<{formula}>{expr}"),
            },
            StateFrmKind::Quantifier {
                quantifier,
                variables,
                body,
            } => {
                write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body)
            }
            StateFrmKind::Bound {
                bound: quantifier,
                variables,
                body,
            } => {
                write!(f, "({} {} . {})", quantifier, variables.iter().format(", "), body)
            }
            StateFrmKind::Binary { op, lhs, rhs } => {
                write!(f, "({lhs} {op} {rhs})")
            }
            StateFrmKind::FixedPoint {
                operator,
                variable,
                body,
            } => {
                write!(f, "({operator} {variable} . {body})")
            }
            StateFrmKind::Delay(Some(expr)) => write!(f, "delay@({expr})"),
            StateFrmKind::Delay(None) => write!(f, "delay"),
            StateFrmKind::Yaled(Some(expr)) => write!(f, "yaled@({expr})"),
            StateFrmKind::Yaled(None) => write!(f, "yaled"),
            StateFrmKind::DataValExprLeftMult(value, expr) => write!(f, "(val({value}) * {expr})"),
            StateFrmKind::DataValExprRightMult(expr, value) => write!(f, "({expr} * val({value}))"),
        }
    }
}

impl fmt::Display for StateVarDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.arguments.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.arguments.iter().format(","))
        }
    }
}

impl fmt::Display for StateVarAssignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} : {} = {}", self.identifier.node, self.sort, self.expr)
    }
}

impl fmt::Display for StateFrmOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StateFrmOp::Implies => write!(f, "=>"),
            StateFrmOp::Conjunction => write!(f, "&&"),
            StateFrmOp::Disjunction => write!(f, "||"),
            StateFrmOp::Addition => write!(f, "+"),
        }
    }
}
