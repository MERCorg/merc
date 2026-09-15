use pest::pratt_parser::Op;
use pest::pratt_parser::PrattParser;

use crate::Rule;
use crate::Spanned;

mod actfrm;
mod dataexpr;
mod pbesexpr;
mod presexpr;
mod procexpr;
mod regfrm;
mod sortexpr;
mod statefrm;

pub use actfrm::parse_actfrm;
pub use dataexpr::parse_dataexpr;
pub use pbesexpr::parse_pbesexpr;
pub use presexpr::parse_presexpr;
pub use procexpr::parse_process_expr;
pub use regfrm::parse_regfrm;
pub use sortexpr::parse_sortexpr;
pub use sortexpr::parse_sortexpr_primary;
pub use statefrm::parse_statefrm;

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
