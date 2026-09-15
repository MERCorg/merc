use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::ProcExprBinaryOp;
use crate::ProcessExpr;
use crate::ProcessExprKind;
use crate::Rule;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [ProcessExprKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const PROCEXPR_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::ProcExprChoice,
        fixity: Fixity::Infix(0, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::ProcExprSum,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::ProcExprDist,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::ProcExprParallel,
        fixity: Fixity::Infix(2, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ProcExprLeftMerge,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ProcExprIf,
        fixity: Fixity::Prefix(4),
    },
    RuleFixity {
        rule: Rule::ProcExprIfThen,
        fixity: Fixity::Prefix(4),
    },
    RuleFixity {
        rule: Rule::ProcExprUntil,
        fixity: Fixity::Infix(5, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::ProcExprSeq,
        fixity: Fixity::Infix(6, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ProcExprAt,
        fixity: Fixity::Postfix(7),
    },
    RuleFixity {
        rule: Rule::ProcExprSync,
        fixity: Fixity::Infix(8, Assoc::Left),
    },
];

static PROCEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(PROCEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_process_expr(pairs: Pairs<Rule>) -> ParseResult<ProcessExpr> {
    PROCEXPR_PRATT_PARSER
        .map_primary(procexpr_primary)
        .map_infix(procexpr_infix)
        .map_prefix(procexpr_prefix)
        .map_postfix(procexpr_postfix)
        .parse(pairs)
}

fn procexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<ProcessExpr> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::ProcExprId => Ok(Mcrl2Parser::ProcExprId(Node::new(primary))?),
        Rule::ProcExprDelta => Ok(ProcessExprKind::Delta.spanned(span)),
        Rule::ProcExprTau => Ok(ProcessExprKind::Tau.spanned(span)),
        Rule::ProcExprBlock => Ok(Mcrl2Parser::ProcExprBlock(Node::new(primary))?),
        Rule::ProcExprAllow => Ok(Mcrl2Parser::ProcExprAllow(Node::new(primary))?),
        Rule::ProcExprHide => Ok(Mcrl2Parser::ProcExprHide(Node::new(primary))?),
        Rule::ProcExprRename => Ok(Mcrl2Parser::ProcExprRename(Node::new(primary))?),
        Rule::ProcExprComm => Ok(Mcrl2Parser::ProcExprComm(Node::new(primary))?),
        Rule::Action => {
            let action = Mcrl2Parser::Action(Node::new(primary))?;

            Ok(ProcessExprKind::Action(action.id, action.args).spanned(span))
        }
        Rule::ProcExprBrackets => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_process_expr(inner.into_inner())
        }
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn procexpr_infix(
    lhs: ParseResult<ProcessExpr>,
    op: Pair<'_, Rule>,
    rhs: ParseResult<ProcessExpr>,
) -> ParseResult<ProcessExpr> {
    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    let op = match op.as_rule() {
        Rule::ProcExprChoice => ProcExprBinaryOp::Choice,
        Rule::ProcExprParallel => ProcExprBinaryOp::Parallel,
        Rule::ProcExprLeftMerge => ProcExprBinaryOp::LeftMerge,
        Rule::ProcExprSeq => ProcExprBinaryOp::Sequence,
        Rule::ProcExprSync => ProcExprBinaryOp::CommMerge,
        Rule::ProcExprUntil => ProcExprBinaryOp::Until,
        _ => unimplemented!("Unexpected rule: {:?}", op.as_rule()),
    };
    Ok(ProcessExprKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

fn procexpr_prefix(prefix: Pair<'_, Rule>, expr: ParseResult<ProcessExpr>) -> ParseResult<ProcessExpr> {
    let start = prefix.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match prefix.as_rule() {
        Rule::ProcExprSum => Ok(ProcessExprKind::Sum {
            variables: Mcrl2Parser::ProcExprSum(Node::new(prefix))?,
            operand: Box::new(expr),
        }
        .spanned(span)),
        Rule::ProcExprDist => {
            let (variables, data_expr) = Mcrl2Parser::ProcExprDist(Node::new(prefix))?;

            Ok(ProcessExprKind::Dist {
                variables,
                expr: data_expr,
                operand: Box::new(expr),
            }
            .spanned(span))
        }
        Rule::ProcExprIf => {
            let condition = Mcrl2Parser::ProcExprIf(Node::new(prefix))?;

            Ok(ProcessExprKind::Condition {
                condition,
                then: Box::new(expr),
                else_: None,
            }
            .spanned(span))
        }
        Rule::ProcExprIfThen => {
            let (condition, then) = Mcrl2Parser::ProcExprIfThen(Node::new(prefix))?;

            Ok(ProcessExprKind::Condition {
                condition,
                then: Box::new(then),
                else_: Some(Box::new(expr)),
            }
            .spanned(span))
        }
        _ => unimplemented!("Unexpected rule: {:?}", prefix.as_rule()),
    }
}

fn procexpr_postfix(expr: ParseResult<ProcessExpr>, postfix: Pair<'_, Rule>) -> ParseResult<ProcessExpr> {
    let expr = expr?;
    let span = Span {
        start: expr.span.start,
        end: postfix.as_span().end(),
    };
    match postfix.as_rule() {
        Rule::ProcExprAt => Ok(ProcessExprKind::At {
            expr: Box::new(expr),
            operand: Mcrl2Parser::ProcExprAt(Node::new(postfix))?,
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected postfix rule: {:?}", postfix.as_rule()),
    }
}

impl Operator for ProcessExprKind {
    fn fixity(&self) -> Fixity {
        match self {
            ProcessExprKind::Binary { op, .. } => match op {
                ProcExprBinaryOp::Choice => Fixity::Infix(0, Assoc::Left),
                ProcExprBinaryOp::Parallel => Fixity::Infix(2, Assoc::Right),
                ProcExprBinaryOp::LeftMerge => Fixity::Infix(3, Assoc::Right),
                ProcExprBinaryOp::Until => Fixity::Infix(5, Assoc::Left),
                ProcExprBinaryOp::Sequence => Fixity::Infix(6, Assoc::Right),
                ProcExprBinaryOp::CommMerge => Fixity::Infix(8, Assoc::Left),
            },
            ProcessExprKind::Sum { .. } | ProcessExprKind::Dist { .. } => Fixity::Prefix(1),
            ProcessExprKind::Condition { .. } => Fixity::Prefix(4),
            ProcessExprKind::At { .. } => Fixity::Postfix(7),
            ProcessExprKind::Id(_, _)
            | ProcessExprKind::Action(_, _)
            | ProcessExprKind::Delta
            | ProcessExprKind::Tau
            | ProcessExprKind::Hide { .. }
            | ProcessExprKind::Rename { .. }
            | ProcessExprKind::Allow { .. }
            | ProcessExprKind::Block { .. }
            | ProcessExprKind::Comm { .. } => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&ProcessExpr> {
        match self {
            ProcessExprKind::Sum { operand, .. } | ProcessExprKind::Dist { operand, .. } => Some(operand),
            ProcessExprKind::Condition { then, else_, .. } => Some(else_.as_deref().unwrap_or(then)),
            ProcessExprKind::At { expr, .. } => Some(expr),
            _ => None,
        }
    }
}
