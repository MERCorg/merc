use std::cell::RefCell;
use std::mem::ManuallyDrop;

use merc_aterm::ATermString;
use merc_aterm::Symb;
use merc_aterm::Term;

use crate::BasicSort;
use crate::DataApplication;
use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataFunctionSymbol;
use crate::DataFunctionSymbolRef;
use crate::SortArrow;
use crate::SortExpression;
use crate::is_data_application;
use crate::is_data_function_symbol;

/// Function symbol names for mCRL2's `Bool` connectives, exactly as they occur
/// in the standard prelude.
const AND: &str = "&&";
const OR: &str = "||";
const NOT: &str = "!";
const EQ: &str = "==";
const NEQ: &str = "!=";
const IMPLIES: &str = "=>";

/// The top-level shape of a data expression as one of mCRL2's `Bool` connectives, borrowing its
/// operands. Data expressions are open-ended, so anything else is [BoolKind::Other].
pub enum BoolKind<'a> {
    And(DataExpressionRef<'a>, DataExpressionRef<'a>),
    Or(DataExpressionRef<'a>, DataExpressionRef<'a>),
    Implies(DataExpressionRef<'a>, DataExpressionRef<'a>),
    Not(DataExpressionRef<'a>),
    /// `lhs == rhs`, for any domain sort.
    Equal(DataExpressionRef<'a>, DataExpressionRef<'a>),
    /// `lhs != rhs`, for any domain sort.
    NotEqual(DataExpressionRef<'a>, DataExpressionRef<'a>),
    Other,
}

impl<'a> DataExpressionRef<'a> {
    /// The top-level `Bool` connective of this expression. The operands live as long as this
    /// reference, so that a traversal can keep them on an explicit stack.
    pub fn bool_kind(&self) -> BoolKind<'a> {
        if !is_data_application(self) {
            return BoolKind::Other;
        }

        // The head of an application need not be a function symbol, e.g. a variable of function sort.
        let head = self.arg(0);
        if !is_data_function_symbol(&head) {
            return BoolKind::Other;
        }

        let symbol = DataFunctionSymbolRef::from(head);
        let operand = |index: usize| DataExpressionRef::from(self.arg(index));

        // The `DataAppl` symbol counts the function symbol as well as the arguments.
        BOOL_SYMBOLS.with_borrow(|bs| match self.get_head_symbol().arity() {
            2 if symbol == bs.not.copy() => BoolKind::Not(operand(1)),
            3 if symbol == bs.and.copy() => BoolKind::And(operand(1), operand(2)),
            3 if symbol == bs.or.copy() => BoolKind::Or(operand(1), operand(2)),
            3 if symbol == bs.implies.copy() => BoolKind::Implies(operand(1), operand(2)),
            3 if symbol.name() == bs.eq_name.copy() => BoolKind::Equal(operand(1), operand(2)),
            3 if symbol.name() == bs.neq_name.copy() => BoolKind::NotEqual(operand(1), operand(2)),
            _ => BoolKind::Other,
        })
    }
}

impl DataExpression {
    /// Builds `lhs && rhs`.
    pub fn and(lhs: DataExpression, rhs: DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.and, &[lhs, rhs]).into())
    }

    /// Builds `lhs || rhs`.
    pub fn or(lhs: DataExpression, rhs: DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.or, &[lhs, rhs]).into())
    }

    /// Builds `!operand`.
    pub fn negation(operand: DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.not, &[operand]).into())
    }

    /// Builds `lhs => rhs`.
    pub fn implies(lhs: DataExpression, rhs: DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.implies, &[lhs, rhs]).into())
    }

    /// Builds `lhs == rhs`, where both sides have sort `domain`.
    pub fn equal(domain: SortExpression, lhs: DataExpression, rhs: DataExpression) -> DataExpression {
        let symbol = function_symbol(EQ, SortArrow::new(&[domain.clone(), domain], bool_sort()));
        DataApplication::with_args(&symbol, &[lhs, rhs]).into()
    }

    /// Builds `lhs != rhs`, where both sides have sort `domain`.
    pub fn not_equal(domain: SortExpression, lhs: DataExpression, rhs: DataExpression) -> DataExpression {
        let symbol = function_symbol(NEQ, SortArrow::new(&[domain.clone(), domain], bool_sort()));
        DataApplication::with_args(&symbol, &[lhs, rhs]).into()
    }
}

fn bool_sort() -> SortExpression {
    SortExpression::from(BasicSort::new("Bool"))
}

/// Builds the function symbol named `name` with the function sort `arrow`.
fn function_symbol(name: &'static str, arrow: SortArrow) -> DataFunctionSymbol {
    let sort: SortExpression = arrow.into();
    DataFunctionSymbol::with_sort(name, sort.copy())
}

thread_local! {
    /// Canonical terms for the boolean connectives.
    static BOOL_SYMBOLS: RefCell<BoolSymbols> = RefCell::new(BoolSymbols::new());
}

/// `&&`, `||`, `!`, and `=>` always have the same (monomorphic) `Bool` sort, so each is built
/// once as the exact function symbol every application and comparison uses. `==` and `!=` are
/// polymorphic in their domain sort, so only their name is canonicalized and they are matched on
/// the name alone, ignoring the domain.
///
/// All fields are wrapped in `ManuallyDrop` so that their destructors never run at thread
/// exit: a term must never be dropped after its owning thread-local term pool has already
/// been torn down, and thread-local destruction order is unspecified (see
/// [`crate::data_terms::DataSymbols`]).
struct BoolSymbols {
    and: ManuallyDrop<DataFunctionSymbol>,
    or: ManuallyDrop<DataFunctionSymbol>,
    not: ManuallyDrop<DataFunctionSymbol>,
    implies: ManuallyDrop<DataFunctionSymbol>,
    eq_name: ManuallyDrop<ATermString>,
    neq_name: ManuallyDrop<ATermString>,
}

impl BoolSymbols {
    fn new() -> Self {
        Self {
            and: ManuallyDrop::new(function_symbol(
                AND,
                SortArrow::new(&[bool_sort(), bool_sort()], bool_sort()),
            )),
            or: ManuallyDrop::new(function_symbol(
                OR,
                SortArrow::new(&[bool_sort(), bool_sort()], bool_sort()),
            )),
            not: ManuallyDrop::new(function_symbol(NOT, SortArrow::new(&[bool_sort()], bool_sort()))),
            implies: ManuallyDrop::new(function_symbol(
                IMPLIES,
                SortArrow::new(&[bool_sort(), bool_sort()], bool_sort()),
            )),
            eq_name: ManuallyDrop::new(ATermString::new(EQ)),
            neq_name: ManuallyDrop::new(ATermString::new(NEQ)),
        }
    }
}
