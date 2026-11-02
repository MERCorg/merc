use std::cell::RefCell;
use std::mem::ManuallyDrop;

use merc_aterm::ATerm;
use merc_aterm::ATermString;
use merc_aterm::Symb;
use merc_aterm::Term;
use merc_utilities::BinaryChain;
use merc_utilities::Boolean;
use merc_utilities::BooleanChain;

use crate::DataApplication;
use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataFunctionSymbol;
use crate::DataFunctionSymbolRef;
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
    /// The canonical `Bool` literal `true`/`false`.
    pub fn bool_literal(value: bool) -> DataExpression {
        if value {
            TRUE_LITERAL.with(|term| (**term).clone().into())
        } else {
            FALSE_LITERAL.with(|term| (**term).clone().into())
        }
    }

    /// Builds `lhs && rhs`.
    pub fn and(lhs: &DataExpression, rhs: &DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.and, &[lhs.copy(), rhs.copy()]).into())
    }

    /// Builds `lhs || rhs`.
    pub fn or(lhs: &DataExpression, rhs: &DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.or, &[lhs.copy(), rhs.copy()]).into())
    }

    /// Builds `!operand`.
    pub fn negation(operand: &DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.not, &[operand.copy()]).into())
    }

    /// Builds `lhs => rhs`.
    pub fn implies(lhs: &DataExpression, rhs: &DataExpression) -> DataExpression {
        BOOL_SYMBOLS.with_borrow(|bs| DataApplication::with_args(&*bs.implies, &[lhs.copy(), rhs.copy()]).into())
    }

    /// Builds `lhs == rhs`, where both sides have sort `domain`.
    pub fn equal(domain: &SortExpression, lhs: &DataExpression, rhs: &DataExpression) -> DataExpression {
        let symbol = DataFunctionSymbol::with_signature(EQ, &[domain.clone(), domain.clone()], &SortExpression::bool());
        DataApplication::with_args(&symbol, &[lhs.copy(), rhs.copy()]).into()
    }

    /// Builds `lhs != rhs`, where both sides have sort `domain`.
    pub fn not_equal(domain: &SortExpression, lhs: &DataExpression, rhs: &DataExpression) -> DataExpression {
        let symbol =
            DataFunctionSymbol::with_signature(NEQ, &[domain.clone(), domain.clone()], &SortExpression::bool());
        DataApplication::with_args(&symbol, &[lhs.copy(), rhs.copy()]).into()
    }
}

/// Returns true iff `term` is the `Bool` literal `value`, in constant time.
pub fn is_bool_literal<'a, 'b>(term: &'b impl Term<'a, 'b>, value: bool) -> bool {
    let literal = if value { &TRUE_LITERAL } else { &FALSE_LITERAL };
    literal.with(|literal| term.copy() == literal.copy())
}

/// A connective of `Bool` whose nested applications form a chain, see [BinaryChain].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolConnective {
    And,
    Or,
}

impl Clone for DataExpressionRef<'_> {
    fn clone(&self) -> Self {
        self.copy()
    }
}

impl<'a> BinaryChain for DataExpressionRef<'a> {
    type Op = BoolConnective;

    fn binary_operands(&self, op: BoolConnective) -> Option<(Self, Self)> {
        match (self.bool_kind(), op) {
            (BoolKind::And(lhs, rhs), BoolConnective::And) | (BoolKind::Or(lhs, rhs), BoolConnective::Or) => {
                Some((lhs, rhs))
            }
            _ => None,
        }
    }
}

impl BooleanChain for DataExpressionRef<'_> {
    type Owned = DataExpression;

    fn conjunction() -> BoolConnective {
        BoolConnective::And
    }

    fn disjunction() -> BoolConnective {
        BoolConnective::Or
    }

    fn into_owned(self) -> DataExpression {
        self.protect()
    }
}

impl Boolean for DataExpression {
    fn true_constant() -> Self {
        DataExpression::bool_literal(true)
    }

    fn false_constant() -> Self {
        DataExpression::bool_literal(false)
    }

    fn is_true(&self) -> bool {
        is_bool_literal(self, true)
    }

    fn is_false(&self) -> bool {
        is_bool_literal(self, false)
    }

    fn negated(&self) -> Option<Self> {
        match self.copy().bool_kind() {
            BoolKind::Not(operand) => Some(operand.protect()),
            _ => None,
        }
    }

    fn make_not(self) -> Self {
        DataExpression::negation(&self)
    }

    fn make_and(self, rhs: Self) -> Self {
        DataExpression::and(&self, &rhs)
    }

    fn make_or(self, rhs: Self) -> Self {
        DataExpression::or(&self, &rhs)
    }

    fn make_imp(self, rhs: Self) -> Self {
        DataExpression::implies(&self, &rhs)
    }
}

thread_local! {
    /// Canonical terms for the boolean connectives.
    static BOOL_SYMBOLS: RefCell<BoolSymbols> = RefCell::new(BoolSymbols::new());

    /// The canonical `Bool` literals `true`/`false`.
    static TRUE_LITERAL: ManuallyDrop<ATerm> = ManuallyDrop::new(bool_literal_term("true"));
    static FALSE_LITERAL: ManuallyDrop<ATerm> = ManuallyDrop::new(bool_literal_term("false"));
}

/// Builds the nullary `Bool` function symbol term named `name` (`"true"` or `"false"`).
fn bool_literal_term(name: &str) -> ATerm {
    DataFunctionSymbol::with_sort(name, SortExpression::bool().copy()).into()
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
        let bool = SortExpression::bool();
        let binary = |name| {
            ManuallyDrop::new(DataFunctionSymbol::with_signature(
                name,
                &[bool.clone(), bool.clone()],
                &bool,
            ))
        };
        Self {
            and: binary(AND),
            or: binary(OR),
            not: ManuallyDrop::new(DataFunctionSymbol::with_signature(
                NOT,
                std::slice::from_ref(&bool),
                &bool,
            )),
            implies: binary(IMPLIES),
            eq_name: ManuallyDrop::new(ATermString::new(EQ)),
            neq_name: ManuallyDrop::new(ATermString::new(NEQ)),
        }
    }
}
