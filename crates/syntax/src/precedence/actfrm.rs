use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::ActFrm;
use crate::ActFrmBinaryOp;
use crate::ActFrmKind;
use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::Quantifier;
use crate::Rule;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [ActFrmKind], lowest level first — see [build_pratt_parser] and
/// [Operator]. `Rule::ActFrmAt` (postfix, level 4) has no [ActFrmKind] variant of its own — it
/// only participates in [ACTFRM_PRATT_PARSER]'s precedence climbing, so [Operator]'s levels below
/// skip straight from 3 to 5.
const ACTFRM_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::ActFrmExists,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::ActFrmForall,
        fixity: Fixity::Prefix(0),
    },
    RuleFixity {
        rule: Rule::ActFrmImplies,
        fixity: Fixity::Infix(1, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ActFrmUnion,
        fixity: Fixity::Infix(2, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ActFrmIntersect,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::ActFrmAt,
        fixity: Fixity::Postfix(4),
    },
    RuleFixity {
        rule: Rule::ActFrmNegation,
        fixity: Fixity::Prefix(5),
    },
];

/// Defines the operator precedence for action formulas using a Pratt parser.
static ACTFRM_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(ACTFRM_OPERATORS));

fn actfrm_primary(primary: Pair<'_, Rule>) -> ParseResult<ActFrm> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::ActFrmTrue => Ok(ActFrmKind::True.spanned(span)),
        Rule::ActFrmFalse => Ok(ActFrmKind::False.spanned(span)),
        Rule::MultAct => Ok(ActFrmKind::MultAct(Mcrl2Parser::MultAct(Node::new(primary))?).spanned(span)),
        Rule::DataValExpr => Ok(ActFrmKind::DataExprVal(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span)),
        Rule::ActFrmBrackets => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_actfrm(inner.into_inner())
        }
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn actfrm_prefix(prefix: Pair<'_, Rule>, expr: ParseResult<ActFrm>) -> ParseResult<ActFrm> {
    let start = prefix.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match prefix.as_rule() {
        Rule::ActFrmExists => Ok(ActFrmKind::Quantifier {
            quantifier: Quantifier::Exists,
            variables: Mcrl2Parser::ActFrmExists(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::ActFrmForall => Ok(ActFrmKind::Quantifier {
            quantifier: Quantifier::Forall,
            variables: Mcrl2Parser::ActFrmForall(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::ActFrmNegation => Ok(ActFrmKind::Negation(Box::new(expr)).spanned(span)),
        _ => unimplemented!("Unexpected prefix operator: {:?}", prefix.as_rule()),
    }
}

fn actfrm_infix(lhs: ParseResult<ActFrm>, op: Pair<'_, Rule>, rhs: ParseResult<ActFrm>) -> ParseResult<ActFrm> {
    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    let op = match op.as_rule() {
        Rule::ActFrmUnion => ActFrmBinaryOp::Union,
        Rule::ActFrmIntersect => ActFrmBinaryOp::Intersect,
        Rule::ActFrmImplies => ActFrmBinaryOp::Implies,
        _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
    };
    Ok(ActFrmKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

/// Parses a sequence of `Rule` pairs into an `ActFrm` using a Pratt parser defined in [ACTFRM_PRATT_PARSER] for operator precedence.
///
/// # Panics
///
/// Panics if `pairs` were not produced by the `ActFrm` grammar rule.
#[allow(clippy::result_large_err)]
pub fn parse_actfrm(pairs: Pairs<Rule>) -> ParseResult<ActFrm> {
    ACTFRM_PRATT_PARSER
        .map_primary(actfrm_primary)
        .map_prefix(actfrm_prefix)
        .map_infix(actfrm_infix)
        .parse(pairs)
}

impl Operator for ActFrmKind {
    fn fixity(&self) -> Fixity {
        match self {
            ActFrmKind::Quantifier { .. } => Fixity::Prefix(0),
            ActFrmKind::Binary { op, .. } => match op {
                ActFrmBinaryOp::Implies => Fixity::Infix(1, Assoc::Right),
                ActFrmBinaryOp::Union => Fixity::Infix(2, Assoc::Right),
                ActFrmBinaryOp::Intersect => Fixity::Infix(3, Assoc::Right),
            },
            ActFrmKind::Negation(_) => Fixity::Prefix(5),
            ActFrmKind::True | ActFrmKind::False | ActFrmKind::MultAct(_) | ActFrmKind::DataExprVal(_) => {
                Fixity::Primary
            }
        }
    }

    fn operand(&self) -> Option<&ActFrm> {
        match self {
            ActFrmKind::Negation(inner) => Some(inner),
            ActFrmKind::Quantifier { body, .. } => Some(body),
            _ => None,
        }
    }
}
