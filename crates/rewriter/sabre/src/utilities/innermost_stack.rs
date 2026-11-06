use std::fmt;

use merc_aterm::ATermRef;
use merc_aterm::Markable;
use merc_aterm::Protected;
use merc_aterm::ProtectedWriteGuard;
use merc_aterm::SymbolRef;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;

use crate::innermost_rewriter::ProgramId;
use crate::utilities::Block;

/// The stacks of the rewriting machine: what to do next, and the terms computed so far.
///
/// Every configuration takes its inputs from the top of the term stack and leaves its result
/// there, so that the terms on the stack are normal forms, apart from the arguments of a redex
/// whose right-hand side is running.
#[derive(Default)]
pub(crate) struct InnermostStack {
    pub configs: Protected<Vec<Config<'static>>>,
    pub terms: Protected<Vec<Option<DataExpressionRef<'static>>>>,
}

/// A configuration of the rewriting machine.
pub(crate) enum Config<'a> {
    /// Pushes the normal form of `term`.
    Rewrite { term: DataExpressionRef<'a> },
    /// Replaces `symbol` applied to the `arity` terms on top by its normal form.
    Construct {
        symbol: DataFunctionSymbolRef<'a>,
        arity: usize,
    },
    /// Runs a block of a program, see [Evaluate].
    Evaluate(Evaluate),
    /// Pops the normal form of the term being rewritten.
    Return,
}

/// Runs the instructions `pc..end` of a program, which is one block of it, and replaces the
/// terms from `base` on by the term it computes.
///
/// Its variables are the arguments of the matched term, which are the terms at `base`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Evaluate {
    pub program: ProgramId,
    pub pc: u32,
    pub end: u32,
    pub base: usize,
}

impl Evaluate {
    /// Runs `block` of `program`.
    pub fn new(program: ProgramId, block: Block, base: usize) -> Evaluate {
        Evaluate {
            program,
            pc: u32::try_from(block.begin).expect("programs have fewer than 2^32 instructions"),
            end: u32::try_from(block.end).expect("programs have fewer than 2^32 instructions"),
            base,
        }
    }
}

impl Markable for Config<'_> {
    fn mark(&self, marker: &mut Marker<'_>) {
        match self {
            Config::Rewrite { term } => term.mark(marker),
            Config::Construct { symbol, .. } => symbol.mark(marker),
            Config::Evaluate(_) | Config::Return => {}
        }
    }

    fn contains_term(&self, term: &ATermRef<'_>) -> bool {
        match self {
            Config::Rewrite { term: t } => t.contains_term(term),
            Config::Construct { symbol, .. } => symbol.contains_term(term),
            Config::Evaluate(_) | Config::Return => false,
        }
    }

    fn contains_symbol(&self, symbol: &SymbolRef<'_>) -> bool {
        match self {
            Config::Rewrite { term } => term.contains_symbol(symbol),
            Config::Construct { symbol: s, .. } => s.contains_symbol(symbol),
            Config::Evaluate(_) | Config::Return => false,
        }
    }

    fn len(&self) -> usize {
        match self {
            Config::Rewrite { .. } | Config::Construct { .. } => 1,
            Config::Evaluate(_) | Config::Return => 0,
        }
    }
}

// SAFETY: `Config` only borrows the term pool through its `DataFunctionSymbolRef`
// / `DataExpressionRef` fields, which are themselves lifetime-erasable handles
// into the global term pool. Transmuting only changes the lifetime parameter, so
// the layout is identical.
unsafe impl Transmutable for Config<'static> {
    type Target<'a> = Config<'a>;

    unsafe fn transmute_lifetime<'a>(&'_ self) -> &'a Self::Target<'a> {
        // SAFETY: see the trait impl comment above; the caller upholds that 'a does not
        // outlive the borrow of `self`.
        unsafe { std::mem::transmute::<&Self, &'a Config>(self) }
    }

    unsafe fn transmute_lifetime_mut<'a>(&'_ mut self) -> &'a mut Self::Target<'a> {
        // SAFETY: see the trait impl comment above; the caller upholds that 'a does not
        // outlive the borrow of `self`.
        unsafe { std::mem::transmute::<&mut Self, &'a mut Config>(self) }
    }
}

impl fmt::Display for Config<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Config::Rewrite { term } => write!(f, "Rewrite({term})"),
            Config::Construct { symbol, arity } => write!(f, "Construct({symbol}, {arity})"),
            Config::Evaluate(evaluate) => write!(f, "{evaluate:?}"),
            Config::Return => write!(f, "Return"),
        }
    }
}

impl InnermostStack {
    /// Pushes the configuration that rewrites `term`.
    pub fn add_rewrite(write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>, term: &DataExpressionRef<'_>) {
        // Safety: term is pushed into the container on the next line.
        let term = unsafe { write_configs.protect(term) };
        write_configs.push(Config::Rewrite { term: term.into() });
    }

    /// Pushes the configuration that replaces `symbol` applied to the `arity` terms on top by
    /// its normal form.
    pub fn add_construct(
        write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
        symbol: &DataFunctionSymbolRef<'_>,
        arity: usize,
    ) {
        // Safety: symbol is pushed into the container on the next line.
        let symbol = unsafe { write_configs.protect(symbol) };
        write_configs.push(Config::Construct {
            symbol: symbol.into(),
            arity,
        });
    }

    /// Pushes `value` onto the term stack.
    pub fn push_value(
        write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
        value: &DataExpressionRef<'_>,
    ) {
        // Safety: value is pushed into the container on the next line.
        let value = unsafe { write_terms.protect(value) };
        write_terms.push(Some(value.into()));
    }

    /// Replaces the terms from `first` on by `value`.
    pub fn replace_values(
        write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
        first: usize,
        value: &DataExpressionRef<'_>,
    ) {
        // Safety: value is pushed into the container below, before the guard is dropped.
        let value = unsafe { write_terms.protect(value) };
        write_terms.truncate(first);
        write_terms.push(Some(value.into()));
    }
}

impl fmt::Display for InnermostStack {
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
