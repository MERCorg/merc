use std::fmt;

use super::Bound;
use super::Condition;
use super::Eq as PresEq;
use super::FixedPointOperator;
use super::Quantifier;

/// Every word-like mCRL2 keyword that doesn't already have its own small AST enum: contrast
/// [`Quantifier`] (`exists`/`forall`), [`Bound`] (`inf`/`sup`/`sum`) and [`FixedPointOperator`]
/// (`mu`/`nu`), each of which is its own reserved-word type. Built-in sort names (`Bool`, `List`,
/// …) are deliberately not here either — those are [`super::Sort`] and [`super::ComplexSort`].
///
/// See [`KEYWORDS`] and [`MODAL_KEYWORDS`] for the combined, ready-to-use word lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Keyword {
    Sort,
    Cons,
    Map,
    Glob,
    Act,
    Proc,
    Init,
    Var,
    Eqn,
    Struct,
    Whr,
    End,
    Lambda,
    Dist,
    Val,
    True,
    False,
    Delta,
    Tau,
    Hide,
    Block,
    Allow,
    Comm,
    Rename,
    Pbes,
    Pres,
    /// Modal (mu-calculus) formula-only keyword.
    Form,
    /// Modal (mu-calculus) formula-only keyword.
    Delay,
    /// Modal (mu-calculus) formula-only keyword.
    Yaled,
}

impl Keyword {
    /// This keyword's literal mCRL2 spelling; also [`Keyword`]'s own [`Display`](fmt::Display)
    /// text.
    pub const fn name(self) -> &'static str {
        match self {
            Keyword::Sort => "sort",
            Keyword::Cons => "cons",
            Keyword::Map => "map",
            Keyword::Glob => "glob",
            Keyword::Act => "act",
            Keyword::Proc => "proc",
            Keyword::Init => "init",
            Keyword::Var => "var",
            Keyword::Eqn => "eqn",
            Keyword::Struct => "struct",
            Keyword::Whr => "whr",
            Keyword::End => "end",
            Keyword::Lambda => "lambda",
            Keyword::Dist => "dist",
            Keyword::Val => "val",
            Keyword::True => "true",
            Keyword::False => "false",
            Keyword::Delta => "delta",
            Keyword::Tau => "tau",
            Keyword::Hide => "hide",
            Keyword::Block => "block",
            Keyword::Allow => "allow",
            Keyword::Comm => "comm",
            Keyword::Rename => "rename",
            Keyword::Pbes => "pbes",
            Keyword::Pres => "pres",
            Keyword::Form => "form",
            Keyword::Delay => "delay",
            Keyword::Yaled => "yaled",
        }
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Every word-like mCRL2 keyword relevant to a process/data specification: [`Keyword`]'s own
/// non-modal-only variants, plus [`Quantifier`]'s, [`Bound::Sum`]'s and [`FixedPointOperator`]'s
/// reserved words. Built-in sort names are not included, see [`Keyword`].
pub const KEYWORDS: &[&str] = &[
    Quantifier::Exists.name(),
    Quantifier::Forall.name(),
    Bound::Sum.name(),
    FixedPointOperator::Least.name(),
    FixedPointOperator::Greatest.name(),
    Keyword::Sort.name(),
    Keyword::Cons.name(),
    Keyword::Map.name(),
    Keyword::Glob.name(),
    Keyword::Act.name(),
    Keyword::Proc.name(),
    Keyword::Init.name(),
    Keyword::Var.name(),
    Keyword::Eqn.name(),
    Keyword::Struct.name(),
    Keyword::Whr.name(),
    Keyword::End.name(),
    Keyword::Lambda.name(),
    Keyword::Dist.name(),
    Keyword::Val.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    Keyword::Delta.name(),
    Keyword::Tau.name(),
    Keyword::Hide.name(),
    Keyword::Block.name(),
    Keyword::Allow.name(),
    Keyword::Comm.name(),
    Keyword::Rename.name(),
    Keyword::Pbes.name(),
    Keyword::Pres.name(),
];

/// Keywords meaningful only inside a modal (mu-calculus) formula: [`Keyword`]'s modal-only
/// variants plus [`Bound::Inf`] and [`Bound::Sup`].
pub const MODAL_KEYWORDS: &[&str] = &[
    Keyword::Form.name(),
    Keyword::Delay.name(),
    Keyword::Yaled.name(),
    Bound::Inf.name(),
    Bound::Sup.name(),
];

/// The word-like keywords that can lead a [`super::DataExpr`].
pub const DATA_EXPR_KEYWORDS: &[&str] = &[
    Quantifier::Forall.name(),
    Quantifier::Exists.name(),
    Keyword::Lambda.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    Keyword::Whr.name(),
    Keyword::End.name(),
];

/// The word-like keywords that can lead a [`super::ProcessExpr`].
pub const PROC_EXPR_KEYWORDS: &[&str] = &[
    Keyword::Delta.name(),
    Keyword::Tau.name(),
    Bound::Sum.name(),
    Keyword::Dist.name(),
    Keyword::Hide.name(),
    Keyword::Block.name(),
    Keyword::Allow.name(),
    Keyword::Comm.name(),
    Keyword::Rename.name(),
];

/// The word-like keywords that can lead a [`super::PbesExpr`].
pub const PBES_EXPR_KEYWORDS: &[&str] = &[
    Quantifier::Forall.name(),
    Quantifier::Exists.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    Keyword::Val.name(),
];

/// The word-like keywords that can lead a [`super::PresExpr`].
/// `mcrl2_grammar.pest`.
pub const PRES_EXPR_KEYWORDS: &[&str] = &[
    Bound::Inf.name(),
    Bound::Sup.name(),
    Bound::Sum.name(),
    Keyword::Val.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    PresEq::EqInf.name(),
    PresEq::EqnInf.name(),
    Condition::Condsm.name(),
    Condition::Condeq.name(),
];

/// The word-like keywords that can lead an [`super::ActFrm`].
/// `MultAct` in `mcrl2_grammar.pest`.
pub const ACT_FRM_KEYWORDS: &[&str] = &[
    Quantifier::Forall.name(),
    Quantifier::Exists.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    Keyword::Val.name(),
    Keyword::Tau.name(),
];

/// The word-like keywords that can lead a [`super::StateFrm`].
pub const STATE_FRM_KEYWORDS: &[&str] = &[
    FixedPointOperator::Least.name(),
    FixedPointOperator::Greatest.name(),
    Quantifier::Forall.name(),
    Quantifier::Exists.name(),
    Bound::Inf.name(),
    Bound::Sup.name(),
    Bound::Sum.name(),
    Keyword::True.name(),
    Keyword::False.name(),
    Keyword::Delay.name(),
    Keyword::Yaled.name(),
    Keyword::Val.name(),
];
