use std::sync::LazyLock;

use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::RegFrm;
use crate::RegFrmKind;
use crate::Rule;
use pest::iterators::Pairs;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [RegFrmKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const REGFRM_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::RegFrmAlternative,
        fixity: Fixity::Infix(0, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::RegFrmComposition,
        fixity: Fixity::Infix(1, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::RegFrmIteration,
        fixity: Fixity::Postfix(2),
    },
    RuleFixity {
        rule: Rule::RegFrmPlus,
        fixity: Fixity::Postfix(2),
    },
];

/// Defines the operator precedence for regular expressions using a Pratt parser.
static REGFRM_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(REGFRM_OPERATORS));

/// Parses a sequence of `Rule` pairs into an [RegFrm] using a Pratt parser defined in [REGFRM_PRATT_PARSER] for operator precedence.
///
/// # Panics
///
/// Panics if `pairs` were not produced by the `RegFrm` grammar rule.
#[allow(clippy::result_large_err)]
pub fn parse_regfrm(pairs: Pairs<Rule>) -> ParseResult<RegFrm> {
    REGFRM_PRATT_PARSER
        .map_primary(|primary| {
            let span: Span = primary.as_span().into();
            match primary.as_rule() {
                Rule::ActFrm => Ok(RegFrmKind::Action(Mcrl2Parser::ActFrm(Node::new(primary))?).spanned(span)),
                Rule::RegFrmBackets => {
                    // Handle parentheses by recursively parsing the inner expression
                    let inner = primary
                        .into_inner()
                        .next()
                        .expect("Expected inner expression in brackets");
                    parse_regfrm(inner.into_inner())
                }
                _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
            }
        })
        .map_infix(|lhs, op, rhs| {
            let lhs = lhs?;
            let rhs = rhs?;
            let span = Span {
                start: lhs.span.start,
                end: rhs.span.end,
            };
            match op.as_rule() {
                Rule::RegFrmAlternative => Ok(RegFrmKind::Choice {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }
                .spanned(span)),
                Rule::RegFrmComposition => Ok(RegFrmKind::Sequence {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }
                .spanned(span)),
                _ => unimplemented!("Unexpected binary operator: {:?}", op.as_rule()),
            }
        })
        .map_postfix(|expr, postfix| {
            let expr = expr?;
            let span = Span {
                start: expr.span.start,
                end: postfix.as_span().end(),
            };
            match postfix.as_rule() {
                Rule::RegFrmIteration => Ok(RegFrmKind::Iteration(Box::new(expr)).spanned(span)),
                Rule::RegFrmPlus => Ok(RegFrmKind::Plus(Box::new(expr)).spanned(span)),
                _ => unimplemented!("Unexpected rule: {:?}", postfix.as_rule()),
            }
        })
        .parse(pairs)
}

impl Operator for RegFrmKind {
    fn fixity(&self) -> Fixity {
        match self {
            RegFrmKind::Choice { .. } => Fixity::Infix(0, Assoc::Left),
            RegFrmKind::Sequence { .. } => Fixity::Infix(1, Assoc::Right),
            RegFrmKind::Iteration(_) | RegFrmKind::Plus(_) => Fixity::Postfix(2),
            RegFrmKind::Action(_) => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&RegFrm> {
        match self {
            RegFrmKind::Iteration(inner) | RegFrmKind::Plus(inner) => Some(inner),
            _ => None,
        }
    }
}
