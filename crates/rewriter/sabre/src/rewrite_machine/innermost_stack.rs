use std::fmt;

use merc_aterm::ATermRef;
use merc_aterm::Markable;
use merc_aterm::Protected;
use merc_aterm::SymbolRef;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;

use super::Scheduler;
use super::TermStackId;

/// The stacks of the rewriting machine: what to do next, and the terms computed so far.
///
/// Every configuration takes its inputs from the top of the term stack and leaves its result
/// there, so that the terms on the stack are normal forms, apart from the arguments of a redex
/// whose right-hand side is being evaluated, and what a [Scheduler] keeps there.
pub(crate) struct InnermostStack<S: Scheduler> {
    pub configs: Protected<Vec<Config<'static, S>>>,
    pub terms: Protected<Vec<Option<DataExpressionRef<'static>>>>,
}

impl<S: Scheduler> Default for InnermostStack<S> {
    fn default() -> Self {
        InnermostStack {
            configs: Protected::new(Vec::new()),
            terms: Protected::new(Vec::new()),
        }
    }
}

/// A configuration of the rewriting machine.
pub(crate) enum Config<'a, S: Scheduler> {
    /// Pushes the normal form of `term`.
    Rewrite { term: DataExpressionRef<'a> },
    /// Replaces `symbol` applied to the `arity` terms on top by its normal form.
    Construct {
        symbol: DataFunctionSymbolRef<'a>,
        arity: usize,
    },
    /// Evaluates a term stack, see [Evaluate].
    Evaluate(Evaluate<S::Scope>),
    /// Continues the work of the scheduler.
    Resume(S::Continuation),
    /// Pops the normal form of the term being rewritten.
    Return,
}

/// Evaluates the instructions from `pc` on of a term stack, and replaces the terms from `base` on
/// by the term that it computes. The `scope` says what its variables stand for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Evaluate<V> {
    pub term_stack: TermStackId,
    pub pc: u32,
    pub base: usize,
    pub scope: V,
}

impl<V> Evaluate<V> {
    /// Evaluates `term_stack` from its first instruction.
    pub fn new(term_stack: TermStackId, base: usize, scope: V) -> Evaluate<V> {
        Evaluate {
            term_stack,
            pc: 0,
            base,
            scope,
        }
    }
}

impl<S: Scheduler> Markable for Config<'_, S> {
    fn mark(&self, marker: &mut Marker<'_>) {
        match self {
            Config::Rewrite { term } => term.mark(marker),
            Config::Construct { symbol, .. } => symbol.mark(marker),
            Config::Evaluate(_) | Config::Resume(_) | Config::Return => {}
        }
    }

    fn contains_term(&self, term: &ATermRef<'_>) -> bool {
        match self {
            Config::Rewrite { term: t } => t.contains_term(term),
            Config::Construct { symbol, .. } => symbol.contains_term(term),
            Config::Evaluate(_) | Config::Resume(_) | Config::Return => false,
        }
    }

    fn contains_symbol(&self, symbol: &SymbolRef<'_>) -> bool {
        match self {
            Config::Rewrite { term } => term.contains_symbol(symbol),
            Config::Construct { symbol: s, .. } => s.contains_symbol(symbol),
            Config::Evaluate(_) | Config::Resume(_) | Config::Return => false,
        }
    }

    fn len(&self) -> usize {
        match self {
            Config::Rewrite { .. } | Config::Construct { .. } => 1,
            Config::Evaluate(_) | Config::Resume(_) | Config::Return => 0,
        }
    }
}

// SAFETY: `Config` only borrows the term pool through its `DataFunctionSymbolRef`
// / `DataExpressionRef` fields, which are themselves lifetime-erasable handles
// into the global term pool, and the types of the scheduler hold no borrows. Transmuting only changes the
// lifetime parameter, so the layout is identical.
unsafe impl<S: Scheduler> Transmutable for Config<'static, S> {
    type Target<'a> = Config<'a, S>;

    unsafe fn transmute_lifetime<'a>(&'_ self) -> &'a Self::Target<'a> {
        // SAFETY: see the trait impl comment above; the caller upholds that 'a does not
        // outlive the borrow of `self`.
        unsafe { std::mem::transmute::<&Self, &'a Config<'a, S>>(self) }
    }

    unsafe fn transmute_lifetime_mut<'a>(&'_ mut self) -> &'a mut Self::Target<'a> {
        // SAFETY: see the trait impl comment above; the caller upholds that 'a does not
        // outlive the borrow of `self`.
        unsafe { std::mem::transmute::<&mut Self, &'a mut Config<'a, S>>(self) }
    }
}

impl<S: Scheduler> fmt::Display for Config<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Config::Rewrite { term } => write!(f, "Rewrite({term})"),
            Config::Construct { symbol, arity } => write!(f, "Construct({symbol}, {arity})"),
            Config::Evaluate(evaluate) => write!(f, "{evaluate:?}"),
            Config::Resume(continuation) => write!(f, "Resume({continuation:?})"),
            Config::Return => write!(f, "Return"),
        }
    }
}

impl<S: Scheduler> fmt::Display for InnermostStack<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Terms: [")?;
        for (i, entry) in self.terms.read().iter().enumerate() {
            match entry {
                Some(term) => writeln!(f, "{i}\t{term}")?,
                None => writeln!(f, "{i}\tNone")?,
            }
        }
        writeln!(f, "]")?;

        writeln!(f, "Configs: [")?;
        for config in self.configs.read().iter() {
            writeln!(f, "\t{config}")?;
        }
        write!(f, "]")
    }
}
