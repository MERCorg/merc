use std::fmt;

use itertools::Itertools;

use crate::Assignment;
use crate::DataExpr;
use crate::DataExprBinaryOp;
use crate::DataExprKind;
use crate::DataExprUnaryOp;
use crate::DataExprUpdate;

impl fmt::Display for Assignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} = {}", self.identifier, self.expr)
    }
}

impl fmt::Display for DataExprUnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataExprUnaryOp::Negation => write!(f, "!"),
            DataExprUnaryOp::Minus => write!(f, "-"),
            DataExprUnaryOp::Size => write!(f, "#"),
        }
    }
}

impl fmt::Display for DataExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            DataExprKind::EmptyList => write!(f, "[]"),
            DataExprKind::EmptyBag => write!(f, "{{:}}"),
            DataExprKind::EmptySet => write!(f, "{{}}"),
            DataExprKind::List(expressions) => write!(f, "[{}]", expressions.iter().format(", ")),
            DataExprKind::Bag(expressions) => write!(
                f,
                "{{ {} }}",
                expressions
                    .iter()
                    .format_with(", ", |e, f| f(&format_args!("{}: {}", e.expr, e.multiplicity)))
            ),
            DataExprKind::Set(expressions) => write!(f, "{{ {} }}", expressions.iter().format(", ")),
            DataExprKind::Id(identifier) | DataExprKind::Resolved(identifier, _) => write!(f, "{identifier}"),
            DataExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {op} {rhs})"),
            DataExprKind::Unary { op, expr } => write!(f, "({op} {expr})"),
            DataExprKind::Bool(value) => write!(f, "{value}"),
            DataExprKind::Quantifier { op, variables, body } => {
                write!(f, "({} {} . {})", op, variables.iter().format(", "), body)
            }
            DataExprKind::Lambda { variables, body } => {
                write!(f, "(lambda {} . {})", variables.iter().format(", "), body)
            }
            DataExprKind::Application { function, arguments } => {
                if arguments.is_empty() {
                    write!(f, "{function}")
                } else {
                    write!(f, "{}({})", function, arguments.iter().format(", "))
                }
            }
            DataExprKind::Number(value) => write!(f, "{value}"),
            DataExprKind::FunctionUpdate { expr, update } => write!(f, "{expr}[{update}]"),
            DataExprKind::SetBagComp { variable, predicate } => write!(f, "{{ {variable} | {predicate} }}"),
            DataExprKind::Whr { expr, assignments } => {
                write!(f, "{} whr {} end", expr, assignments.iter().format(", "))
            }
        }
    }
}

impl fmt::Display for DataExprUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.expr, self.update)
    }
}

impl fmt::Display for DataExprBinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DataExprBinaryOp::At => write!(f, "."),
            DataExprBinaryOp::Concat => write!(f, "++"),
            DataExprBinaryOp::Cons => write!(f, "|>"),
            DataExprBinaryOp::Equal => write!(f, "=="),
            DataExprBinaryOp::NotEqual => write!(f, "!="),
            DataExprBinaryOp::LessThan => write!(f, "<"),
            DataExprBinaryOp::LessEqual => write!(f, "<="),
            DataExprBinaryOp::GreaterThan => write!(f, ">"),
            DataExprBinaryOp::GreaterEqual => write!(f, ">="),
            DataExprBinaryOp::Conj => write!(f, "&&"),
            DataExprBinaryOp::Disj => write!(f, "||"),
            DataExprBinaryOp::Add => write!(f, "+"),
            DataExprBinaryOp::Subtract => write!(f, "-"),
            DataExprBinaryOp::Div => write!(f, "/"),
            DataExprBinaryOp::Implies => write!(f, "=>"),
            DataExprBinaryOp::In => write!(f, "in"),
            DataExprBinaryOp::IntDiv => write!(f, "div"),
            DataExprBinaryOp::Mod => write!(f, "mod"),
            DataExprBinaryOp::Multiply => write!(f, "*"),
            DataExprBinaryOp::Snoc => write!(f, "<|"),
        }
    }
}
