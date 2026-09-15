use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::Rule;
use crate::Sort;
use crate::syntax_tree::SortExpression;
use crate::syntax_tree::SortExpressionKind;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [SortExpressionKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const SORTEXPR_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::SortExprFunction,
        fixity: Fixity::Infix(0, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::SortExprProduct,
        fixity: Fixity::Infix(1, Assoc::Left),
    },
];

static SORT_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(SORTEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_sortexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<SortExpression> {
    let span: Span = primary.as_span().into();
    if let Some(sort) = simple_sort(primary.as_rule()) {
        return Ok(SortExpressionKind::Simple(sort).spanned(span));
    }
    match primary.as_rule() {
        Rule::IdAt => Ok(SortExpressionKind::Reference(Mcrl2Parser::IdAt(Node::new(primary))?.node).spanned(span)),
        Rule::SortExpr => Mcrl2Parser::SortExpr(Node::new(primary)),

        Rule::SortExprList => Mcrl2Parser::SortExprList(Node::new(primary)),
        Rule::SortExprSet => Mcrl2Parser::SortExprSet(Node::new(primary)),
        Rule::SortExprBag => Mcrl2Parser::SortExprBag(Node::new(primary)),
        Rule::SortExprFSet => Mcrl2Parser::SortExprFSet(Node::new(primary)),
        Rule::SortExprFBag => Mcrl2Parser::SortExprFBag(Node::new(primary)),

        Rule::SortExprParens => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_sortexpr(inner.into_inner())
        }

        Rule::SortExprStruct => Mcrl2Parser::SortExprStruct(Node::new(primary)),
        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

/// The atomic sorts (`Bool`, `Int`, `Pos`, `Nat`, `Real`) that need no further parsing.
fn simple_sort(rule: Rule) -> Option<Sort> {
    match rule {
        Rule::SortExprBool => Some(Sort::Bool),
        Rule::SortExprInt => Some(Sort::Int),
        Rule::SortExprPos => Some(Sort::Pos),
        Rule::SortExprNat => Some(Sort::Nat),
        Rule::SortExprReal => Some(Sort::Real),
        _ => None,
    }
}

/// Parses a sequence of `Rule` pairs into a `SortExpression` using a Pratt parser for operator precedence.
///
/// # Panics
///
/// Panics if `pairs` were not produced by the `SortExpr` grammar rule.
#[allow(clippy::result_large_err)]
pub fn parse_sortexpr(pairs: Pairs<Rule>) -> ParseResult<SortExpression> {
    SORT_PRATT_PARSER
        .map_primary(|primary| parse_sortexpr_primary(primary))
        .map_infix(|lhs, op, rhs| {
            let lhs = lhs?;
            let rhs = rhs?;
            let span = Span {
                start: lhs.span.start,
                end: rhs.span.end,
            };
            match op.as_rule() {
                Rule::SortExprFunction => Ok(SortExpressionKind::Function {
                    domain: Box::new(lhs),
                    range: Box::new(rhs),
                }
                .spanned(span)),
                Rule::SortExprProduct => Ok(SortExpressionKind::Product {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }
                .spanned(span)),
                _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
            }
        })
        .parse(pairs)
}

impl Operator for SortExpressionKind {
    fn fixity(&self) -> Fixity {
        match self {
            SortExpressionKind::Function { .. } => Fixity::Infix(0, Assoc::Right),
            SortExpressionKind::Product { .. } => Fixity::Infix(1, Assoc::Left),
            SortExpressionKind::Struct { .. }
            | SortExpressionKind::Reference(_)
            | SortExpressionKind::TypeVar(_)
            | SortExpressionKind::ResolvedTypeVar(_)
            | SortExpressionKind::Simple(_)
            | SortExpressionKind::Complex(_, _)
            | SortExpressionKind::Resolved(_, _)
            | SortExpressionKind::FlattenedFunction { .. } => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&SortExpression> {
        None
    }
}
