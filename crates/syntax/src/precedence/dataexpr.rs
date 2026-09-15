use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::DataExpr;
use crate::DataExprBinaryOp;
use crate::DataExprKind;
use crate::DataExprUnaryOp;
use crate::Mcrl2Parser;
use crate::ParseResult;
use crate::Quantifier;
use crate::Rule;

use super::Assoc;
use super::Fixity;
use super::Operator;
use super::RuleFixity;
use super::build_pratt_parser;

/// Precedence table for [DataExprKind], lowest level first — see [build_pratt_parser] and
/// [Operator].
const DATAEXPR_OPERATORS: &[RuleFixity] = &[
    RuleFixity {
        rule: Rule::DataExprWhr,
        fixity: Fixity::Postfix(0),
    },
    RuleFixity {
        rule: Rule::DataExprForall,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::DataExprExists,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::DataExprLambda,
        fixity: Fixity::Prefix(1),
    },
    RuleFixity {
        rule: Rule::DataExprImpl,
        fixity: Fixity::Infix(2, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::DataExprDisj,
        fixity: Fixity::Infix(3, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::DataExprConj,
        fixity: Fixity::Infix(4, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::DataExprEq,
        fixity: Fixity::Infix(5, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprNeq,
        fixity: Fixity::Infix(5, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprLess,
        fixity: Fixity::Infix(6, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprLeq,
        fixity: Fixity::Infix(6, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprGeq,
        fixity: Fixity::Infix(6, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprGreater,
        fixity: Fixity::Infix(6, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprIn,
        fixity: Fixity::Infix(6, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprCons,
        fixity: Fixity::Infix(7, Assoc::Right),
    },
    RuleFixity {
        rule: Rule::DataExprSnoc,
        fixity: Fixity::Infix(8, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprConcat,
        fixity: Fixity::Infix(9, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprAdd,
        fixity: Fixity::Infix(10, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprSubtract,
        fixity: Fixity::Infix(10, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprDiv,
        fixity: Fixity::Infix(11, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprIntDiv,
        fixity: Fixity::Infix(11, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprMod,
        fixity: Fixity::Infix(11, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprMult,
        fixity: Fixity::Infix(12, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprAt,
        fixity: Fixity::Infix(12, Assoc::Left),
    },
    RuleFixity {
        rule: Rule::DataExprMinus,
        fixity: Fixity::Prefix(12),
    },
    RuleFixity {
        rule: Rule::DataExprNegation,
        fixity: Fixity::Prefix(12),
    },
    RuleFixity {
        rule: Rule::DataExprSize,
        fixity: Fixity::Prefix(12),
    },
    RuleFixity {
        rule: Rule::DataExprUpdate,
        fixity: Fixity::Postfix(13),
    },
    RuleFixity {
        rule: Rule::DataExprApplication,
        fixity: Fixity::Postfix(13),
    },
];

static DATAEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(DATAEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_dataexpr(pairs: Pairs<Rule>) -> ParseResult<DataExpr> {
    DATAEXPR_PRATT_PARSER
        .map_primary(dataexpr_primary)
        .map_infix(dataexpr_infix)
        .map_postfix(dataexpr_postfix)
        .map_prefix(dataexpr_prefix)
        .parse(pairs)
}

fn dataexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<DataExpr> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::DataExprTrue => Ok(DataExprKind::Bool(true).spanned(span)),
        Rule::DataExprFalse => Ok(DataExprKind::Bool(false).spanned(span)),
        Rule::DataExprEmptyList => Ok(DataExprKind::EmptyList.spanned(span)),
        Rule::DataExprEmptySet => Ok(DataExprKind::EmptySet.spanned(span)),
        Rule::DataExprEmptyBag => Ok(DataExprKind::EmptyBag.spanned(span)),
        Rule::DataExprListEnum => Mcrl2Parser::DataExprListEnum(Node::new(primary)),
        Rule::DataExprBagEnum => Mcrl2Parser::DataExprBagEnum(Node::new(primary)),
        Rule::DataExprSetBagComp => Mcrl2Parser::DataExprSetBagComp(Node::new(primary)),
        Rule::DataExprSetEnum => Mcrl2Parser::DataExprSetEnum(Node::new(primary)),
        Rule::Number => Mcrl2Parser::Number(Node::new(primary)),
        Rule::IdAt => Ok(DataExprKind::Id(Mcrl2Parser::IdAt(Node::new(primary))?.node).spanned(span)),

        Rule::DataExprBrackets => {
            // Handle parentheses by recursively parsing the inner expression
            let inner = primary
                .into_inner()
                .next()
                .expect("Expected inner expression in brackets");
            parse_dataexpr(inner.into_inner())
        }

        _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
    }
}

fn dataexpr_infix(lhs: ParseResult<DataExpr>, op: Pair<'_, Rule>, rhs: ParseResult<DataExpr>) -> ParseResult<DataExpr> {
    let op_kind = match op.as_rule() {
        Rule::DataExprConj => DataExprBinaryOp::Conj,
        Rule::DataExprDisj => DataExprBinaryOp::Disj,
        Rule::DataExprEq => DataExprBinaryOp::Equal,
        Rule::DataExprNeq => DataExprBinaryOp::NotEqual,
        Rule::DataExprLess => DataExprBinaryOp::LessThan,
        Rule::DataExprLeq => DataExprBinaryOp::LessEqual,
        Rule::DataExprGreater => DataExprBinaryOp::GreaterThan,
        Rule::DataExprGeq => DataExprBinaryOp::GreaterEqual,
        Rule::DataExprIn => DataExprBinaryOp::In,
        Rule::DataExprCons => DataExprBinaryOp::Cons,
        Rule::DataExprSnoc => DataExprBinaryOp::Snoc,
        Rule::DataExprConcat => DataExprBinaryOp::Concat,
        Rule::DataExprAdd => DataExprBinaryOp::Add,
        Rule::DataExprSubtract => DataExprBinaryOp::Subtract,
        Rule::DataExprDiv => DataExprBinaryOp::Div,
        Rule::DataExprIntDiv => DataExprBinaryOp::IntDiv,
        Rule::DataExprMod => DataExprBinaryOp::Mod,
        Rule::DataExprMult => DataExprBinaryOp::Multiply,
        Rule::DataExprAt => DataExprBinaryOp::At,
        Rule::DataExprImpl => DataExprBinaryOp::Implies,
        _ => unimplemented!("Unexpected binary operator rule: {:?}", op.as_rule()),
    };

    let lhs = lhs?;
    let rhs = rhs?;
    let span = Span {
        start: lhs.span.start,
        end: rhs.span.end,
    };
    Ok(DataExprKind::Binary {
        op: op_kind,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .spanned(span))
}

fn dataexpr_postfix(expr: ParseResult<DataExpr>, postfix: Pair<'_, Rule>) -> ParseResult<DataExpr> {
    let expr = expr?;
    let end = postfix.as_span().end();
    let span = Span {
        start: expr.span.start,
        end,
    };
    match postfix.as_rule() {
        Rule::DataExprUpdate => Ok(DataExprKind::FunctionUpdate {
            expr: Box::new(expr),
            update: Box::new(Mcrl2Parser::DataExprUpdate(Node::new(postfix))?),
        }
        .spanned(span)),
        Rule::DataExprApplication => Ok(DataExprKind::Application {
            function: Box::new(expr),
            arguments: Mcrl2Parser::DataExprApplication(Node::new(postfix))?,
        }
        .spanned(span)),
        Rule::DataExprWhr => Ok(DataExprKind::Whr {
            expr: Box::new(expr),
            assignments: Mcrl2Parser::DataExprWhr(Node::new(postfix))?,
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected postfix operator: {:?}", postfix.as_rule()),
    }
}

fn dataexpr_prefix(prefix: Pair<'_, Rule>, expr: ParseResult<DataExpr>) -> ParseResult<DataExpr> {
    let start = prefix.as_span().start();
    let expr = expr?;
    let span = Span {
        start,
        end: expr.span.end,
    };
    match prefix.as_rule() {
        Rule::DataExprForall => Ok(DataExprKind::Quantifier {
            op: Quantifier::Forall,
            variables: Mcrl2Parser::DataExprForall(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::DataExprExists => Ok(DataExprKind::Quantifier {
            op: Quantifier::Exists,
            variables: Mcrl2Parser::DataExprExists(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::DataExprLambda => Ok(DataExprKind::Lambda {
            variables: Mcrl2Parser::DataExprLambda(Node::new(prefix))?,
            body: Box::new(expr),
        }
        .spanned(span)),
        Rule::DataExprNegation => Ok(DataExprKind::Unary {
            op: DataExprUnaryOp::Negation,
            expr: Box::new(expr),
        }
        .spanned(span)),
        Rule::DataExprMinus => Ok(DataExprKind::Unary {
            op: DataExprUnaryOp::Minus,
            expr: Box::new(expr),
        }
        .spanned(span)),
        Rule::DataExprSize => Ok(DataExprKind::Unary {
            op: DataExprUnaryOp::Size,
            expr: Box::new(expr),
        }
        .spanned(span)),
        _ => unimplemented!("Unexpected prefix operator: {:?}", prefix.as_rule()),
    }
}

impl Operator for DataExprKind {
    fn fixity(&self) -> Fixity {
        match self {
            DataExprKind::Whr { .. } => Fixity::Postfix(0),
            DataExprKind::Lambda { .. } => Fixity::Prefix(1),
            DataExprKind::Quantifier { .. } => Fixity::Prefix(1),
            DataExprKind::Binary { op, .. } => match op {
                DataExprBinaryOp::Implies => Fixity::Infix(2, Assoc::Right),
                DataExprBinaryOp::Disj => Fixity::Infix(3, Assoc::Right),
                DataExprBinaryOp::Conj => Fixity::Infix(4, Assoc::Right),
                DataExprBinaryOp::Equal | DataExprBinaryOp::NotEqual => Fixity::Infix(5, Assoc::Left),
                DataExprBinaryOp::LessThan
                | DataExprBinaryOp::LessEqual
                | DataExprBinaryOp::GreaterThan
                | DataExprBinaryOp::GreaterEqual
                | DataExprBinaryOp::In => Fixity::Infix(6, Assoc::Left),
                DataExprBinaryOp::Cons => Fixity::Infix(7, Assoc::Right),
                DataExprBinaryOp::Snoc => Fixity::Infix(8, Assoc::Left),
                DataExprBinaryOp::Concat => Fixity::Infix(9, Assoc::Left),
                DataExprBinaryOp::Add | DataExprBinaryOp::Subtract => Fixity::Infix(10, Assoc::Left),
                DataExprBinaryOp::Div | DataExprBinaryOp::IntDiv | DataExprBinaryOp::Mod => {
                    Fixity::Infix(11, Assoc::Left)
                }
                DataExprBinaryOp::Multiply | DataExprBinaryOp::At => Fixity::Infix(12, Assoc::Left),
            },
            DataExprKind::Unary { .. } => Fixity::Prefix(12),
            DataExprKind::FunctionUpdate { .. } | DataExprKind::Application { .. } => Fixity::Postfix(13),
            DataExprKind::Id(_)
            | DataExprKind::Resolved(_, _)
            | DataExprKind::Number(_)
            | DataExprKind::Bool(_)
            | DataExprKind::EmptyList
            | DataExprKind::List(_)
            | DataExprKind::EmptySet
            | DataExprKind::Set(_)
            | DataExprKind::EmptyBag
            | DataExprKind::Bag(_)
            | DataExprKind::SetBagComp { .. } => Fixity::Primary,
        }
    }

    fn operand(&self) -> Option<&DataExpr> {
        match self {
            DataExprKind::Whr { expr, .. }
            | DataExprKind::FunctionUpdate { expr, .. }
            | DataExprKind::Application { function: expr, .. }
            | DataExprKind::Lambda { body: expr, .. }
            | DataExprKind::Quantifier { body: expr, .. }
            | DataExprKind::Unary { expr, .. } => Some(expr),
            _ => None,
        }
    }
}
