use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::PbesExpr;
use crate::PbesExprBinaryOp;
use crate::PbesExprKind;
use crate::Quantifier;
use crate::Rule;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [PbesExprKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const PBESEXPR_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::PbesExprForall,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::PbesExprExists,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::PbesExprImplies,
        fixity: Fixity::Infix(1, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprDisj,
        fixity: Fixity::Infix(2, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprConj,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::PbesExprNegation,
        fixity: Fixity::Prefix(4),
    },
];

static PBESEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(PBESEXPR_OPERATORS));

fn pbesexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<PbesExpr> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::DataValExpr => Ok(PbesExprKind::DataValExpr(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span)),
        Rule::PbesExprParens => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_pbesexpr(inner.into_inner())
        }
        Rule::PbesExprTrue => Ok(PbesExprKind::True.spanned(span)),
        Rule::PbesExprFalse => Ok(PbesExprKind::False.spanned(span)),
        Rule::PropVarInst => Ok(PbesExprKind::PropVarInst(Mcrl2Parser::PropVarInst(Node::new(primary))?).spanned(span)),
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn pbesexpr_prefix(op: Pair<'_, Rule>, expr: ParseResult<PbesExpr>) -> ParseResult<PbesExpr> {
    let start = op.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match op.as_rule() {
        Rule::PbesExprNegation => Ok(PbesExprKind::Negation(Box::new(expr)).spanned(span)),
        Rule::PbesExprExists => Ok(PbesExprKind::Quantifier {
            quantifier: Quantifier::Exists,
            variables: Mcrl2Parser::PbesExprExists(Node::new(op))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::PbesExprForall => Ok(PbesExprKind::Quantifier {
            quantifier: Quantifier::Forall,
            variables: Mcrl2Parser::PbesExprForall(Node::new(op))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected prefix operator: {:?}", op.as_rule()),
    }
}

fn pbesexpr_infix(lhs: ParseResult<PbesExpr>, op: Pair<'_, Rule>, rhs: ParseResult<PbesExpr>) -> ParseResult<PbesExpr> {
    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    let op = match op.as_rule() {
        Rule::PbesExprConj => PbesExprBinaryOp::Conjunction,
        Rule::PbesExprDisj => PbesExprBinaryOp::Disjunction,
        Rule::PbesExprImplies => PbesExprBinaryOp::Implies,
        _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
    };
    Ok(PbesExprKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

#[allow(clippy::result_large_err)]
pub fn parse_pbesexpr(pairs: Pairs<Rule>) -> ParseResult<PbesExpr> {
    PBESEXPR_PRATT_PARSER
        .map_primary(pbesexpr_primary)
        .map_prefix(pbesexpr_prefix)
        .map_infix(pbesexpr_infix)
        .parse(pairs)
}

impl Operator for PbesExprKind {
    fn fixity(&self) -> Fixity {
        match self {
            PbesExprKind::Quantifier { .. } => Fixity::Prefix(0),
            PbesExprKind::Binary { op, .. } => match op {
                PbesExprBinaryOp::Implies => Fixity::Infix(1, Assoc::Right),
                PbesExprBinaryOp::Disjunction => Fixity::Infix(2, Assoc::Right),
                PbesExprBinaryOp::Conjunction => Fixity::Infix(3, Assoc::Right),
            },
            PbesExprKind::Negation(_) => Fixity::Prefix(4),
            PbesExprKind::DataValExpr(_) | PbesExprKind::PropVarInst(_) | PbesExprKind::True | PbesExprKind::False => {
                Fixity::Primary
            }
        }
    }

    fn operand(&self) -> Option<&PbesExpr> {
        match self {
            PbesExprKind::Negation(inner) => Some(inner),
            PbesExprKind::Quantifier { body, .. } => Some(body),
            _ => None,
        }
    }
}
