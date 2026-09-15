use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Bound;
use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::PresExpr;
use crate::PresExprBinaryOp;
use crate::PresExprKind;
use crate::Rule;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [PresExprKind], lowest level first — see [build_pratt_parser] and
/// [Operator]. Note that a PRES expression's `Implies`/`Disj`/`Conj` still parse from the shared
/// `PbesExprImplies`/`PbesExprDisj`/`PbesExprConj` grammar rules (see [PresExprBinaryOp] and
/// [parse_presexpr]'s own `map_infix`), same as `PBESEXPR_OPERATORS`.
const PRESEXPR_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::PresExprInf,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::PresExprSup,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::PresExprSum,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::PresExprAdd,
        fixity: Fixity::Infix(1, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprImplies,
        fixity: Fixity::Infix(2, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprDisj,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprConj,
        fixity: Fixity::Infix(4, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PresExprLeftConstantMultiply,
        fixity: Fixity::Prefix(5),
    },
    RuleFixity {
        rule: Rule::PresExprRightConstMultiply,
        fixity: Fixity::Postfix(5),
    },
    RuleFixity {
        rule: Rule::PresExprNegation,
        fixity: Fixity::Prefix(6),
    },
];

static PRESEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(PRESEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_presexpr(pairs: Pairs<Rule>) -> ParseResult<PresExpr> {
    PRESEXPR_PRATT_PARSER
        .map_primary(presexpr_primary)
        .map_prefix(presexpr_prefix)
        .map_infix(presexpr_infix)
        .map_postfix(presexpr_postfix)
        .parse(pairs)
}

fn presexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<PresExpr> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::DataValExpr => Ok(PresExprKind::DataValExpr(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span)),
        Rule::PresExprParens => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_presexpr(inner.into_inner())
        }
        Rule::PbesExprTrue => Ok(PresExprKind::True.spanned(span)),
        Rule::PbesExprFalse => Ok(PresExprKind::False.spanned(span)),
        Rule::PropVarInst => Ok(PresExprKind::PropVarInst(Mcrl2Parser::PropVarInst(Node::new(primary))?).spanned(span)),
        Rule::PresExprEqinf => Ok(Mcrl2Parser::PresExprEqinf(Node::new(primary))?),
        Rule::PresExprEqninf => Ok(Mcrl2Parser::PresExprEqninf(Node::new(primary))?),
        Rule::PresExprCondsm => Ok(Mcrl2Parser::PresExprCondsm(Node::new(primary))?),
        Rule::PresExprCondeq => Ok(Mcrl2Parser::PresExprCondeq(Node::new(primary))?),
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn presexpr_prefix(op: Pair<'_, Rule>, expr: ParseResult<PresExpr>) -> ParseResult<PresExpr> {
    let start = op.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match op.as_rule() {
        Rule::PresExprNegation => Ok(PresExprKind::Negation(Box::new(expr)).spanned(span)),
        Rule::PresExprInf => Ok(PresExprKind::Bound {
            op: Bound::Inf,
            expr: Box::new(expr),
            variables: Mcrl2Parser::PresExprInf(Node::new(op))?,
        }
        .spanned(span)),
        Rule::PresExprSup => Ok(PresExprKind::Bound {
            op: Bound::Sup,
            expr: Box::new(expr),
            variables: Mcrl2Parser::PresExprSup(Node::new(op))?,
        }
        .spanned(span)),
        Rule::PresExprSum => Ok(PresExprKind::Bound {
            op: Bound::Sum,
            expr: Box::new(expr),
            variables: Mcrl2Parser::PresExprSum(Node::new(op))?,
        }
        .spanned(span)),
        Rule::PresExprLeftConstantMultiply => Ok(PresExprKind::LeftConstantMultiply {
            constant: Mcrl2Parser::PresExprLeftConstantMultiply(Node::new(op))?,
            expr: Box::new(expr),
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected prefix operator: {:?}", op.as_rule()),
    }
}

fn presexpr_infix(lhs: ParseResult<PresExpr>, op: Pair<'_, Rule>, rhs: ParseResult<PresExpr>) -> ParseResult<PresExpr> {
    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    let op = match op.as_rule() {
        Rule::PbesExprImplies => PresExprBinaryOp::Implies,
        Rule::PbesExprDisj => PresExprBinaryOp::Disjunction,
        Rule::PbesExprConj => PresExprBinaryOp::Conjunction,
        Rule::PresExprAdd => PresExprBinaryOp::Add,
        _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
    };
    Ok(PresExprKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

fn presexpr_postfix(expr: ParseResult<PresExpr>, postfix: Pair<'_, Rule>) -> ParseResult<PresExpr> {
    let expr = expr?;
    let span = Span {
        start: expr.span.start,
        end: postfix.as_span().end(),
    };
    match postfix.as_rule() {
        Rule::PresExprRightConstMultiply => Ok(PresExprKind::RightConstantMultiply {
            expr: Box::new(expr),
            constant: Mcrl2Parser::PresExprRightConstMultiply(Node::new(postfix))?,
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected postfix operator: {:?}", postfix.as_rule()),
    }
}

impl Operator for PresExprKind {
    fn fixity(&self) -> Fixity {
        match self {
            PresExprKind::Bound { .. } => Fixity::Prefix(0),
            PresExprKind::Binary { op, .. } => match op {
                PresExprBinaryOp::Add => Fixity::Infix(1, Assoc::Right),
                PresExprBinaryOp::Implies => Fixity::Infix(2, Assoc::Right),
                PresExprBinaryOp::Disjunction => Fixity::Infix(3, Assoc::Right),
                PresExprBinaryOp::Conjunction => Fixity::Infix(4, Assoc::Right),
            },
            PresExprKind::LeftConstantMultiply { .. } => Fixity::Prefix(5),
            PresExprKind::RightConstantMultiply { .. } => Fixity::Postfix(5),
            PresExprKind::Negation(_) => Fixity::Prefix(6),
            // `Equal`/`Condition` parse as self-contained, closed productions (`f(x) eq-inf`,
            // `f(x) whr ...`-shaped, always delimited by their own keywords) — see
            // `parse_presexpr`'s `map_primary` — so they never interact with precedence climbing.
            PresExprKind::DataValExpr(_)
            | PresExprKind::PropVarInst(_)
            | PresExprKind::Equal { .. }
            | PresExprKind::Condition { .. }
            | PresExprKind::True
            | PresExprKind::False => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&PresExpr> {
        match self {
            PresExprKind::LeftConstantMultiply { expr, .. } | PresExprKind::RightConstantMultiply { expr, .. } => {
                Some(expr)
            }
            PresExprKind::Bound { expr, .. } => Some(expr),
            PresExprKind::Negation(inner) => Some(inner),
            _ => None,
        }
    }
}
