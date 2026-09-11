use std::sync::LazyLock;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::pratt_parser::Op;
use pest::pratt_parser::PrattParser;

use merc_pest_consume::Node;
use merc_utilities::Span;

use crate::ActFrm;
use crate::ActFrmBinaryOp;
use crate::ActFrmKind;
use crate::Bound;
use crate::DataExpr;
use crate::DataExprBinaryOp;
use crate::DataExprKind;
use crate::DataExprUnaryOp;
use crate::FixedPointOperator;
use crate::Mcrl2Parser;
use crate::ModalityOperator;
use crate::ParseResult;
use crate::PbesExpr;
use crate::PbesExprBinaryOp;
use crate::PbesExprKind;
use crate::PresExpr;
use crate::PresExprBinaryOp;
use crate::PresExprKind;
use crate::ProcExprBinaryOp;
use crate::ProcessExpr;
use crate::ProcessExprKind;
use crate::Quantifier;
use crate::RegFrm;
use crate::RegFrmKind;
use crate::Rule;
use crate::Sort;
use crate::Spanned;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateFrmOp;
use crate::StateFrmUnaryOp;
use crate::syntax_tree::SortExpression;
use crate::syntax_tree::SortExpressionKind;

/// An operator's associativity, independent of `pest`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Assoc {
    Left,
    Right,
}

impl From<Assoc> for pest::pratt_parser::Assoc {
    fn from(assoc: Assoc) -> Self {
        match assoc {
            Assoc::Left => pest::pratt_parser::Assoc::Left,
            Assoc::Right => pest::pratt_parser::Assoc::Right,
        }
    }
}

/// How an AST node's own operator participates in precedence: a prefix, infix or postfix
/// operator at the given level. Higher levels bind tighter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fixity {
    Prefix(u8),
    Infix(u8, Assoc),
    Postfix(u8),
    Primary,
}

/// Implemented by every `*Kind` enum whose values are Pratt-parsed.
pub trait Operator: Sized {
    /// Returns the fixity and precedence level of this operator.
    fn fixity(&self) -> Fixity;

    /// Returns the operand of this operator if it has one (prefix or postfix), or `None` otherwise.
    fn operand(&self) -> Option<&Spanned<Self>>;
}

/// One entry in a `*_OPERATORS` table: which grammar rule an operator parses from, alongside its
/// [Fixity].
#[derive(Clone, Copy)]
struct RuleFixity {
    rule: Rule,
    fixity: Fixity,
}

/// Builds a [PrattParser] whose precedence levels are exactly `table`'s own [Fixity] levels:
/// entries sharing a level become one Pratt-parser precedence step (combined with `|`, exactly as
/// a hand-written `.op(Op::infix(...) | Op::prefix(...))` would), lowest level first.
///
/// # Panics
///
/// Panics if `table` contains a `Fixity::Primary` entry — a primary rule is handled by
/// `map_primary` alone and never belongs in this table.
fn build_pratt_parser(table: &[RuleFixity]) -> PrattParser<Rule> {
    let max_level = table
        .iter()
        .map(|entry| match entry.fixity {
            Fixity::Prefix(level) | Fixity::Postfix(level) | Fixity::Infix(level, _) => level,
            Fixity::Primary => unreachable!("a primary rule never belongs in a *_OPERATORS table"),
        })
        .max()
        .unwrap_or(0);

    let mut parser = PrattParser::new();
    for level in 0..=max_level {
        let level_ops = table
            .iter()
            .filter(|entry| match entry.fixity {
                Fixity::Prefix(l) | Fixity::Postfix(l) | Fixity::Infix(l, _) => l == level,
                Fixity::Primary => false,
            })
            .map(|entry| match entry.fixity {
                Fixity::Prefix(_) => Op::prefix(entry.rule),
                Fixity::Postfix(_) => Op::postfix(entry.rule),
                Fixity::Infix(_, assoc) => Op::infix(entry.rule, assoc.into()),
                Fixity::Primary => unreachable!("a primary rule never belongs in a *_OPERATORS table"),
            })
            .reduce(|a, b| a | b);
        if let Some(level_ops) = level_ops {
            parser = parser.op(level_ops);
        }
    }
    parser
}

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

pub static SORT_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(SORTEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_sortexpr_primary(primary: Pair<'_, Rule>) -> ParseResult<SortExpression> {
    let span: Span = primary.as_span().into();
    match primary.as_rule() {
        Rule::IdAt => Ok(SortExpressionKind::Reference(Mcrl2Parser::IdAt(Node::new(primary))?.node).spanned(span)),
        Rule::SortExpr => Mcrl2Parser::SortExpr(Node::new(primary)),

        Rule::SortExprBool => Ok(SortExpressionKind::Simple(Sort::Bool).spanned(span)),
        Rule::SortExprInt => Ok(SortExpressionKind::Simple(Sort::Int).spanned(span)),
        Rule::SortExprPos => Ok(SortExpressionKind::Simple(Sort::Pos).spanned(span)),
        Rule::SortExprNat => Ok(SortExpressionKind::Simple(Sort::Nat).spanned(span)),
        Rule::SortExprReal => Ok(SortExpressionKind::Simple(Sort::Real).spanned(span)),

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

pub static DATAEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> =
    LazyLock::new(|| build_pratt_parser(DATAEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_dataexpr(pairs: Pairs<Rule>) -> ParseResult<DataExpr> {
    DATAEXPR_PRATT_PARSER
        .map_primary(|primary| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .map_postfix(|expr, postfix| {
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
        })
        .map_prefix(|prefix, expr| {
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
        })
        .parse(pairs)
}

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

pub static PROCEXPR_PRATT_PARSER: LazyLock<PrattParser<Rule>> =
    LazyLock::new(|| build_pratt_parser(PROCEXPR_OPERATORS));

#[allow(clippy::result_large_err)]
pub fn parse_process_expr(pairs: Pairs<Rule>) -> ParseResult<ProcessExpr> {
    PROCEXPR_PRATT_PARSER
        .map_primary(|primary| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .map_prefix(|prefix, expr| {
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
        })
        .map_postfix(|expr, postfix| {
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
        })
        .parse(pairs)
}

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
pub static ACTFRM_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(ACTFRM_OPERATORS));

/// Parses a sequence of `Rule` pairs into an `ActFrm` using a Pratt parser defined in [ACTFRM_PRATT_PARSER] for operator precedence.
///
/// # Panics
///
/// Panics if `pairs` were not produced by the `ActFrm` grammar rule.
#[allow(clippy::result_large_err)]
pub fn parse_actfrm(pairs: Pairs<Rule>) -> ParseResult<ActFrm> {
    ACTFRM_PRATT_PARSER
        .map_primary(|primary| {
            let span: Span = primary.as_span().into();
            match primary.as_rule() {
                Rule::ActFrmTrue => Ok(ActFrmKind::True.spanned(span)),
                Rule::ActFrmFalse => Ok(ActFrmKind::False.spanned(span)),
                Rule::MultAct => Ok(ActFrmKind::MultAct(Mcrl2Parser::MultAct(Node::new(primary))?).spanned(span)),
                Rule::DataValExpr => {
                    Ok(ActFrmKind::DataExprVal(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span))
                }
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
        })
        .map_prefix(|prefix, expr| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .parse(pairs)
}

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
pub static REGFRM_PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| build_pratt_parser(REGFRM_OPERATORS));

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
        .map_primary(|primary| {
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
        })
        .map_prefix(|prefix, expr| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .map_postfix(|expr, postfix| {
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
        })
        .parse(pairs)
}

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

#[allow(clippy::result_large_err)]
pub fn parse_pbesexpr(pairs: Pairs<Rule>) -> ParseResult<PbesExpr> {
    PBESEXPR_PRATT_PARSER
        .map_primary(|primary| {
            let span: Span = primary.as_span().into();
            match primary.as_rule() {
                Rule::DataValExpr => {
                    Ok(PbesExprKind::DataValExpr(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span))
                }
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
                Rule::PropVarInst => {
                    Ok(PbesExprKind::PropVarInst(Mcrl2Parser::PropVarInst(Node::new(primary))?).spanned(span))
                }
                _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
            }
        })
        .map_prefix(|op, expr| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .parse(pairs)
}

/// Precedence table for [PresExprKind], lowest level first — see [build_pratt_parser] and
/// [Operator]. Note that a PRES expression's `Implies`/`Disj`/`Conj` still parse from the shared
/// `PbesExprImplies`/`PbesExprDisj`/`PbesExprConj` grammar rules (see [PresExprBinaryOp] and
/// [parse_presexpr]'s own `map_infix`), same as [PBESEXPR_OPERATORS].
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
        .map_primary(|primary| {
            let span: Span = primary.as_span().into();
            match primary.as_rule() {
                Rule::DataValExpr => {
                    Ok(PresExprKind::DataValExpr(Mcrl2Parser::DataValExpr(Node::new(primary))?).spanned(span))
                }
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
                Rule::PropVarInst => {
                    Ok(PresExprKind::PropVarInst(Mcrl2Parser::PropVarInst(Node::new(primary))?).spanned(span))
                }
                Rule::PresExprEqinf => Ok(Mcrl2Parser::PresExprEqinf(Node::new(primary))?),
                Rule::PresExprEqninf => Ok(Mcrl2Parser::PresExprEqninf(Node::new(primary))?),
                Rule::PresExprCondsm => Ok(Mcrl2Parser::PresExprCondsm(Node::new(primary))?),
                Rule::PresExprCondeq => Ok(Mcrl2Parser::PresExprCondeq(Node::new(primary))?),
                _ => unimplemented!("Unexpected rule: {:?}", primary.as_rule()),
            }
        })
        .map_prefix(|op, expr| {
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
        })
        .map_infix(|lhs, op, rhs| {
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
        })
        .map_postfix(|expr, postfix| {
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
        })
        .parse(pairs)
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
