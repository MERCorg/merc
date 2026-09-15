use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Bound;
use crate::FixedPointOperator;
use crate::Mcrl2Parser;
use crate::ModalityOperator;
use crate::ParseResult;
use crate::Quantifier;
use crate::Rule;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateFrmOp;
use crate::StateFrmUnaryOp;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [StateFrmKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const STATEFRM_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::StateFrmMu,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::StateFrmNu,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::StateFrmForall,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::StateFrmExists,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::StateFrmInf,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::StateFrmSup,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::StateFrmSum,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::StateFrmAddition,
        fixity: Fixity::Infix(2, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::StateFrmImplication,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::StateFrmDisjunction,
        fixity: Fixity::Infix(4, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::StateFrmConjunction,
        fixity: Fixity::Infix(5, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::StateFrmLeftConstantMultiply,
        fixity: Fixity::Prefix(6),
    },
    RuleFixity {
        rule: Rule::StateFrmRightConstantMultiply,
        fixity: Fixity::Postfix(6),
    },
    RuleFixity {
        rule: Rule::StateFrmBox,
        fixity: Fixity::Prefix(7),
    },
    RuleFixity {
        rule: Rule::StateFrmDiamond,
        fixity: Fixity::Prefix(7),
    },
    RuleFixity {
        rule: Rule::StateFrmNegation,
        fixity: Fixity::Prefix(8),
    },
    RuleFixity {
        rule: Rule::StateFrmUnaryMinus,
        fixity: Fixity::Prefix(8),
    },
];

/// Defines the operator precedence for state formulas using a Pratt parser.
static STATEFRM_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(STATEFRM_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_statefrm(pairs: Pairs<Rule>) -> ParseResult<StateFrm> {
    STATEFRM_PRATT_PARSER
        .map_primary(statefrm_primary)
        .map_prefix(statefrm_prefix)
        .map_infix(statefrm_infix)
        .map_postfix(statefrm_postfix)
        .parse(pairs)
}

fn statefrm_primary(primary: Pair<'_, Rule>) -> ParseResult<StateFrm> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::StateFrmId => Mcrl2Parser::StateFrmId(Node::new(primary)),
        Rule::StateFrmTrue => Ok(StateFrmKind::True.spanned(span)),
        Rule::StateFrmFalse => Ok(StateFrmKind::False.spanned(span)),
        Rule::StateFrmDelay => Mcrl2Parser::StateFrmDelay(Node::new(primary)),
        Rule::StateFrmYaled => Mcrl2Parser::StateFrmYaled(Node::new(primary)),
        Rule::StateFrmNegation => Mcrl2Parser::StateFrmNegation(Node::new(primary)),
        Rule::StateFrmDataValExpr => {
            // `StateFrmDataValExpr` only wraps a `DataValExpr` child; unwrap before
            // consuming it, the same way `StateFrmBrackets` unwraps its own child below.
            let inner = primary
                .into_inner()
                .next()
                .expect("StateFrmDataValExpr always wraps a DataValExpr child");
            Ok(StateFrmKind::DataValExpr(Mcrl2Parser::DataValExpr(Node::new(inner))?).spanned(span))
        }
        Rule::StateFrmBrackets => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_statefrm(inner.into_inner())
        }
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn statefrm_prefix(prefix: Pair<'_, Rule>, expr: ParseResult<StateFrm>) -> ParseResult<StateFrm> {
    let start = prefix.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match prefix.as_rule() {
        Rule::StateFrmLeftConstantMultiply => Ok(StateFrmKind::DataValExprLeftMult(
            Mcrl2Parser::StateFrmLeftConstantMultiply(Node::new(prefix))?,
            Box::new(expr),
        )
        .spanned(span)),
        Rule::StateFrmDiamond => Ok(StateFrmKind::Modality {
            operator: ModalityOperator::Diamond,
            formula: Mcrl2Parser::StateFrmDiamond(Node::new(prefix))?,
            expr: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmBox => Ok(StateFrmKind::Modality {
            operator: ModalityOperator::Box,
            formula: Mcrl2Parser::StateFrmBox(Node::new(prefix))?,
            expr: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmExists => Ok(StateFrmKind::Quantifier {
            quantifier: Quantifier::Exists,
            variables: Mcrl2Parser::StateFrmExists(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmForall => Ok(StateFrmKind::Quantifier {
            quantifier: Quantifier::Forall,
            variables: Mcrl2Parser::StateFrmForall(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmMu => Ok(StateFrmKind::FixedPoint {
            operator: FixedPointOperator::Least,
            variable: Mcrl2Parser::StateFrmMu(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmNu => Ok(StateFrmKind::FixedPoint {
            operator: FixedPointOperator::Greatest,
            variable: Mcrl2Parser::StateFrmNu(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmNegation => Ok(StateFrmKind::Unary {
            op: StateFrmUnaryOp::Negation,
            expr: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmSup => Ok(StateFrmKind::Bound {
            bound: Bound::Sup,
            variables: Mcrl2Parser::StateFrmSup(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmSum => Ok(StateFrmKind::Bound {
            bound: Bound::Sum,
            variables: Mcrl2Parser::StateFrmSum(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::StateFrmInf => Ok(StateFrmKind::Bound {
            bound: Bound::Inf,
            variables: Mcrl2Parser::StateFrmInf(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected prefix operator: {:?}", prefix.as_rule()),
    }
}

fn statefrm_infix(lhs: ParseResult<StateFrm>, op: Pair<'_, Rule>, rhs: ParseResult<StateFrm>) -> ParseResult<StateFrm> {
    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    let op = match op.as_rule() {
        Rule::StateFrmAddition => StateFrmOp::Addition,
        Rule::StateFrmImplication => StateFrmOp::Implies,
        Rule::StateFrmDisjunction => StateFrmOp::Disjunction,
        Rule::StateFrmConjunction => StateFrmOp::Conjunction,
        _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
    };
    Ok(StateFrmKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

fn statefrm_postfix(expr: ParseResult<StateFrm>, postfix: Pair<'_, Rule>) -> ParseResult<StateFrm> {
    let expr = expr?;
    let span = Span {
        start: expr.span.start,
        end: postfix.as_span().end(),
    };
    match postfix.as_rule() {
        Rule::StateFrmRightConstantMultiply => Ok(StateFrmKind::DataValExprRightMult(
            Box::new(expr),
            Mcrl2Parser::StateFrmRightConstantMultiply(Node::new(postfix))?,
        )
        .spanned(span)),
        _ => unimplemented!("Unexpected binary operator: {:?}", postfix.as_rule()),
    }
}

impl Operator for StateFrmKind {
    fn fixity(&self) -> Fixity {
        match self {
            StateFrmKind::FixedPoint { .. } => Fixity::Prefix(0),
            StateFrmKind::Quantifier { .. } | StateFrmKind::Bound { .. } => Fixity::Prefix(1),
            StateFrmKind::Binary { op, .. } => match op {
                StateFrmOp::Addition => Fixity::Infix(2, Assoc::Left),
                StateFrmOp::Implies => Fixity::Infix(3, Assoc::Right),
                StateFrmOp::Disjunction => Fixity::Infix(4, Assoc::Right),
                StateFrmOp::Conjunction => Fixity::Infix(5, Assoc::Right),
            },
            StateFrmKind::DataValExprLeftMult(_, _) => Fixity::Prefix(6),
            StateFrmKind::DataValExprRightMult(_, _) => Fixity::Postfix(6),
            StateFrmKind::Modality { .. } => Fixity::Prefix(7),
            StateFrmKind::Unary { .. } => Fixity::Prefix(8),
            StateFrmKind::True
            | StateFrmKind::False
            | StateFrmKind::Delay(_)
            | StateFrmKind::Yaled(_)
            | StateFrmKind::Id(_, _)
            | StateFrmKind::Resolved(_, _, _)
            | StateFrmKind::DataValExpr(_) => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&StateFrm> {
        match self {
            StateFrmKind::DataValExprLeftMult(_, expr) => Some(expr),
            StateFrmKind::DataValExprRightMult(expr, _) => Some(expr),
            StateFrmKind::Modality { expr, .. }
            | StateFrmKind::Unary { expr, .. }
            | StateFrmKind::Quantifier { body: expr, .. }
            | StateFrmKind::Bound { body: expr, .. }
            | StateFrmKind::FixedPoint { body: expr, .. } => Some(expr),
            _ => None,
        }
    }
}
