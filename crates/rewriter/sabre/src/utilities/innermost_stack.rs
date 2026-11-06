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
use merc_utilities::debug_trace;

use crate::utilities::DataPositionIndexed;

use super::Instruction;
use super::TermStack;

/// This stack is used to avoid recursion and also to keep track of terms in
/// normal forms by explicitly representing the rewrites of a right hand
/// side.
#[derive(Default)]
pub struct InnermostStack {
    pub configs: Protected<Vec<Config<'static>>>,
    pub terms: Protected<Vec<Option<DataExpressionRef<'static>>>>,
}

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum Config<'a> {
    /// Rewrite the top of the stack and put result at the given index.
    Rewrite(usize),
    /// Constructs function symbol with given arity at the given index.
    Construct(DataFunctionSymbolRef<'a>, usize, usize),
    /// A concrete term to be placed at the current position in the stack.
    Term(DataExpressionRef<'a>, usize),
    /// Instantiates the binder `b`, given as `lambda x0, .., xn . b` over its free variables
    /// `x0, .., xn`, with the given number of values for them on top of the stack, and places
    /// the result at the given index.
    Binder(DataExpressionRef<'a>, usize, usize),
    /// Yields the given index as returned term.
    Return(),
}

impl Markable for Config<'_> {
    fn mark(&self, marker: &mut Marker<'_>) {
        match self {
            Config::Construct(t, _, _) => t.mark(marker),
            Config::Term(t, _) | Config::Binder(t, _, _) => t.mark(marker),
            Config::Rewrite(_) | Config::Return() => {}
        }
    }

    fn contains_term(&self, term: &ATermRef<'_>) -> bool {
        match self {
            Config::Construct(t, _, _) => t.contains_term(term),
            Config::Term(t, _) | Config::Binder(t, _, _) => t.contains_term(term),
            Config::Rewrite(_) | Config::Return() => false,
        }
    }

    fn contains_symbol(&self, symbol: &SymbolRef<'_>) -> bool {
        match self {
            Config::Construct(t, _, _) => t.contains_symbol(symbol),
            Config::Term(t, _) | Config::Binder(t, _, _) => t.contains_symbol(symbol),
            Config::Rewrite(_) | Config::Return() => false,
        }
    }

    fn len(&self) -> usize {
        match self {
            Config::Construct(_, _, _) | Config::Term(_, _) | Config::Binder(_, _, _) => 1,
            Config::Rewrite(_) | Config::Return() => 0,
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
            Config::Rewrite(result) => write!(f, "Rewrite({result})"),
            Config::Construct(symbol, arity, result) => {
                write!(f, "Construct({symbol}, {arity}, {result})")
            }
            Config::Term(term, result) => {
                write!(f, "Term({term}, {result})")
            }
            Config::Binder(binder, arity, result) => {
                write!(f, "Binder({binder}, {arity}, {result})")
            }
            Config::Return() => write!(f, "Return()"),
        }
    }
}

impl InnermostStack {
    /// Updates the InnermostStack to integrate the rhs_stack instructions.
    ///
    /// The program is the term in post-order, so in reverse it reaches every subterm before its
    /// arguments, and those from the last to the first. An application gets slots on top of the
    /// stack for its arguments, which the instructions of the arguments fill: a variable
    /// directly, and a constant, an application or a binder by a configuration, which runs once
    /// the arguments of the application or the binder are in place.
    pub fn integrate(
        write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
        write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
        rhs_stack: &TermStack,
        term: &DataExpressionRef<'_>,
        result_index: usize,
    ) {
        debug_trace!("\t applied stack: {}", rhs_stack);

        // The slots that the remaining instructions fill, the next one on top.
        let mut slots = vec![result_index];
        for instruction in rhs_stack.block(0).iter().rev() {
            let index = slots.pop().expect("every instruction has a slot for its value");
            let arity = match instruction {
                Instruction::Variable(position) => {
                    // Safety: term is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&term.get_data_position(position)) }.into());
                    0
                }
                Instruction::Term(value) => {
                    // Safety: value is pushed into the container on the next line.
                    let value = unsafe { write_configs.protect(value) };
                    write_configs.push(Config::Term(value.into(), index));
                    0
                }
                Instruction::Apply { symbol, arity } => {
                    InnermostStack::add_result(write_configs, symbol.copy(), *arity, index);
                    *arity
                }
                Instruction::Binder { template, arity } => {
                    // Safety: template is pushed into the container on the next line.
                    let template = unsafe { write_configs.protect(template) };
                    write_configs.push(Config::Binder(template.into(), *arity, index));
                    *arity
                }
                Instruction::Hole(hole) => match *hole {},
            };

            let top_of_stack = write_terms.len();
            write_terms.resize(top_of_stack + arity, None);
            slots.extend(top_of_stack..top_of_stack + arity);
        }
    }

    /// Indicate that the given symbol with arity can be constructed at the given index.
    pub fn add_result(
        write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
        symbol: DataFunctionSymbolRef<'_>,
        arity: usize,
        index: usize,
    ) {
        // Safety: symbol is pushed into the container on the next line.
        let symbol = unsafe { write_configs.protect(&symbol) };
        write_configs.push(Config::Construct(symbol.into(), arity, index));
    }

    /// Indicate that the term must be rewritten and its result must be placed at the given index.
    pub fn add_rewrite(
        write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
        write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
        term: DataExpressionRef<'_>,
        index: usize,
    ) {
        // Safety: term is pushed into the container two lines below.
        let term = unsafe { write_terms.protect(&term) };
        write_configs.push(Config::Rewrite(index));
        write_terms.push(Some(term.into()));
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
