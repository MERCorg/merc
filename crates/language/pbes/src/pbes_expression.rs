use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::ControlFlow;

use delegate::delegate;
use merc_aterm::ATerm;
use merc_aterm::ATermArgs;
use merc_aterm::ATermIndex;
use merc_aterm::ATermList;
use merc_aterm::ATermRef;
use merc_aterm::ATermString;
use merc_aterm::Markable;
use merc_aterm::Symbol;
use merc_aterm::SymbolRef;
use merc_aterm::Term;
use merc_aterm::TermIterator;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::bool_literal;
use merc_data::is_bool_literal;
use merc_data::is_data_application;
use merc_data::is_data_binder;
use merc_data::is_data_function_symbol;
use merc_data::is_data_machine_number;
use merc_data::is_data_variable;
use merc_data::is_data_where_clause;
use merc_macros::merc_derive_terms;
use merc_macros::merc_term;
use merc_utilities::BinaryChain;
use merc_utilities::Boolean;
use merc_utilities::BooleanChain;
use merc_utilities::Step;
use merc_utilities::optimized_disjunction;
use merc_utilities::optimized_not;
use merc_utilities::unique_chain_operands;

use crate::visit_pbes_expr;

thread_local! {
    static PBES_SYMBOLS: PbesSymbols = PbesSymbols::new();
}

/// The function symbols of mCRL2's PBES terms, so that its binary format can be read and written
/// as is. The Boolean constants are the data constants `true` and `false`, as in mCRL2.
///
/// The symbols are wrapped in `ManuallyDrop` so that their destructors never run at thread exit.
struct PbesSymbols {
    and: ManuallyDrop<Symbol>,
    or: ManuallyDrop<Symbol>,
    imp: ManuallyDrop<Symbol>,
    not: ManuallyDrop<Symbol>,
    forall: ManuallyDrop<Symbol>,
    exists: ManuallyDrop<Symbol>,
    instantiation: ManuallyDrop<Symbol>,
    variable: ManuallyDrop<Symbol>,
    mu: ManuallyDrop<Symbol>,
    nu: ManuallyDrop<Symbol>,
}

impl PbesSymbols {
    fn new() -> Self {
        PbesSymbols {
            and: ManuallyDrop::new(Symbol::new("PBESAnd", 2)),
            or: ManuallyDrop::new(Symbol::new("PBESOr", 2)),
            imp: ManuallyDrop::new(Symbol::new("PBESImp", 2)),
            not: ManuallyDrop::new(Symbol::new("PBESNot", 1)),
            forall: ManuallyDrop::new(Symbol::new("PBESForall", 2)),
            exists: ManuallyDrop::new(Symbol::new("PBESExists", 2)),
            instantiation: ManuallyDrop::new(Symbol::new("PropVarInst", 2)),
            variable: ManuallyDrop::new(Symbol::new("PropVarDecl", 2)),
            mu: ManuallyDrop::new(Symbol::new("Mu", 0)),
            nu: ManuallyDrop::new(Symbol::new("Nu", 0)),
        }
    }
}

/// Returns true iff `term` is a propositional variable instantiation `X(e₁, …, eₙ)`.
pub fn is_propositional_variable_instantiation<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    PBES_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.instantiation.copy())
}

/// Returns true iff `term` is a propositional variable declaration `X(d₁: D₁, …, dₙ: Dₙ)`.
pub fn is_propositional_variable<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    PBES_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.variable.copy())
}

/// Returns true iff `term` is a PBES expression. Only the head symbol is inspected.
pub fn is_pbes_expression<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    let head = term.get_head_symbol();
    let is_connective = PBES_SYMBOLS.with(|symbols| {
        [
            &symbols.and,
            &symbols.or,
            &symbols.imp,
            &symbols.not,
            &symbols.forall,
            &symbols.exists,
            &symbols.instantiation,
        ]
        .into_iter()
        .any(|symbol| head == symbol.copy())
    });

    is_connective
        || is_data_variable(term)
        || is_data_function_symbol(term)
        || is_data_application(term)
        || is_data_machine_number(term)
        || is_data_binder(term)
        || is_data_where_clause(term)
}

/// The fixpoint of a PBES equation: `mu` for the least, `nu` for the greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixpointSymbol {
    Mu,
    Nu,
}

impl FixpointSymbol {
    /// The term `Mu` or `Nu` that represents this fixpoint in mCRL2's binary format.
    pub fn to_term(self) -> ATerm {
        PBES_SYMBOLS.with(|symbols| match self {
            FixpointSymbol::Mu => ATerm::constant(&*symbols.mu),
            FixpointSymbol::Nu => ATerm::constant(&*symbols.nu),
        })
    }

    /// The fixpoint represented by `term`, or `None` if `term` is not `Mu` or `Nu`.
    pub fn from_term<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> Option<Self> {
        let head = term.get_head_symbol();
        PBES_SYMBOLS.with(|symbols| {
            if head == symbols.mu.copy() {
                Some(FixpointSymbol::Mu)
            } else if head == symbols.nu.copy() {
                Some(FixpointSymbol::Nu)
            } else {
                None
            }
        })
    }
}

#[merc_derive_terms]
mod inner {
    use merc_aterm::ATermStringRef;
    use merc_macros::merc_ignore;

    use super::*;

    /// The right-hand side of a PBES equation: a Boolean data expression, a propositional variable
    /// instantiation, or a connective or quantifier applied to PBES expressions.
    ///
    /// Expressions are maximally shared terms, so equality and hashing take constant time and
    /// deeply nested expressions, such as the long chains produced by quantifier enumeration, are
    /// cloned and dropped without recursion. Use [PbesExpressionRef::kind] to match on one.
    #[merc_term(is_pbes_expression)]
    pub struct PbesExpression {
        term: ATerm,
    }

    impl PbesExpression {
        /// The Boolean constant `true`.
        #[merc_ignore]
        pub fn true_constant() -> Self {
            bool_literal(true).into()
        }

        /// The Boolean constant `false`.
        #[merc_ignore]
        pub fn false_constant() -> Self {
            bool_literal(false).into()
        }

        #[merc_ignore]
        pub fn not(body: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| ATerm::with_args(&*symbols.not, &[body.copy()]).protect().into())
        }

        #[merc_ignore]
        pub fn and(lhs: &PbesExpression, rhs: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| binary(&symbols.and, lhs, rhs))
        }

        #[merc_ignore]
        pub fn or(lhs: &PbesExpression, rhs: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| binary(&symbols.or, lhs, rhs))
        }

        #[merc_ignore]
        pub fn imp(lhs: &PbesExpression, rhs: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| binary(&symbols.imp, lhs, rhs))
        }

        #[merc_ignore]
        pub fn forall(variables: &[DataVariable], body: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| quantifier(&symbols.forall, variables, body))
        }

        #[merc_ignore]
        pub fn exists(variables: &[DataVariable], body: &PbesExpression) -> Self {
            PBES_SYMBOLS.with(|symbols| quantifier(&symbols.exists, variables, body))
        }

        /// The top-level shape of this expression.
        #[merc_ignore]
        pub fn kind(&self) -> PbesKind<'_> {
            self.copy().kind()
        }
    }

    /// An occurrence `X(e₁, …, eₙ)` of a propositional variable.
    #[merc_term(is_propositional_variable_instantiation)]
    pub struct PropositionalVariableInstantiation {
        term: ATerm,
    }

    impl PropositionalVariableInstantiation {
        #[merc_ignore]
        pub fn new(name: &str, arguments: &[DataExpression]) -> Self {
            let arguments = ATermList::from_double_iter(arguments.iter().cloned());
            let name = ATermString::new(name);
            PBES_SYMBOLS.with(|symbols| {
                let args: &[ATermRef<'_>] = &[name.copy().into(), arguments.copy()];
                ATerm::with_args(&*symbols.instantiation, args).protect().into()
            })
        }

        /// The instantiation that passes the parameters of `variable` on unchanged.
        #[merc_ignore]
        pub fn identity(variable: &PropositionalVariable) -> Self {
            let arguments: Vec<DataExpression> = variable.parameters().iter().map(DataExpression::from).collect();
            Self::new(variable.name().value(), &arguments)
        }

        pub fn name(&self) -> ATermStringRef<'_> {
            self.term.arg(0).into()
        }

        pub fn arguments(&self) -> ATermList<DataExpression> {
            self.term.arg(1).protect().into()
        }
    }

    /// The left-hand side `X(d₁: D₁, …, dₙ: Dₙ)` of a PBES equation.
    #[merc_term(is_propositional_variable)]
    pub struct PropositionalVariable {
        term: ATerm,
    }

    impl PropositionalVariable {
        #[merc_ignore]
        pub fn new(name: &str, parameters: &[DataVariable]) -> Self {
            let parameters = ATermList::from_double_iter(parameters.iter().cloned());
            let name = ATermString::new(name);
            PBES_SYMBOLS.with(|symbols| {
                let args: &[ATermRef<'_>] = &[name.copy().into(), parameters.copy()];
                ATerm::with_args(&*symbols.variable, args).protect().into()
            })
        }

        pub fn name(&self) -> ATermStringRef<'_> {
            self.term.arg(0).into()
        }

        pub fn parameters(&self) -> ATermList<DataVariable> {
            self.term.arg(1).protect().into()
        }
    }

    #[merc_ignore]
    impl From<PropositionalVariableInstantiation> for PbesExpression {
        fn from(value: PropositionalVariableInstantiation) -> Self {
            value.term.into()
        }
    }

    #[merc_ignore]
    impl From<DataExpression> for PbesExpression {
        /// The data expression must be of sort `Bool`.
        fn from(value: DataExpression) -> Self {
            ATerm::from(value).into()
        }
    }
}

pub use inner::PbesExpression;
pub use inner::PbesExpressionRef;
pub use inner::PropositionalVariable;
pub use inner::PropositionalVariableInstantiation;
pub use inner::PropositionalVariableInstantiationRef;
pub use inner::PropositionalVariableRef;

fn binary(symbol: &Symbol, lhs: &PbesExpression, rhs: &PbesExpression) -> PbesExpression {
    ATerm::with_args(symbol, &[lhs.copy(), rhs.copy()]).protect().into()
}

fn quantifier(symbol: &Symbol, variables: &[DataVariable], body: &PbesExpression) -> PbesExpression {
    let variables = ATermList::from_double_iter(variables.iter().cloned());
    let args: &[ATermRef<'_>] = &[variables.copy(), body.copy().into()];
    ATerm::with_args(symbol, args).protect().into()
}

/// The top-level shape of a [PbesExpression], borrowing its subterms.
pub enum PbesKind<'a> {
    True,
    False,
    /// A Boolean data expression other than the constants `true` and `false`.
    Data(DataExpressionRef<'a>),
    Instantiation(PropositionalVariableInstantiationRef<'a>),
    Not(PbesExpressionRef<'a>),
    And(PbesExpressionRef<'a>, PbesExpressionRef<'a>),
    Or(PbesExpressionRef<'a>, PbesExpressionRef<'a>),
    Imp(PbesExpressionRef<'a>, PbesExpressionRef<'a>),
    Forall(ATermList<DataVariable>, PbesExpressionRef<'a>),
    Exists(ATermList<DataVariable>, PbesExpressionRef<'a>),
}

impl<'a> PbesExpressionRef<'a> {
    /// The top-level shape of this expression. The subterms live as long as this reference, so
    /// that a traversal can keep them on an explicit stack.
    pub fn kind(&self) -> PbesKind<'a> {
        let term: ATermRef<'a> = self.term.copy();
        let operand = |index: usize| PbesExpressionRef::from(term.arg(index));
        let head = term.get_head_symbol();

        PBES_SYMBOLS.with(|symbols| {
            if head == symbols.and.copy() {
                PbesKind::And(operand(0), operand(1))
            } else if head == symbols.or.copy() {
                PbesKind::Or(operand(0), operand(1))
            } else if head == symbols.imp.copy() {
                PbesKind::Imp(operand(0), operand(1))
            } else if head == symbols.not.copy() {
                PbesKind::Not(operand(0))
            } else if head == symbols.forall.copy() {
                PbesKind::Forall(term.arg(0).protect().into(), operand(1))
            } else if head == symbols.exists.copy() {
                PbesKind::Exists(term.arg(0).protect().into(), operand(1))
            } else if head == symbols.instantiation.copy() {
                PbesKind::Instantiation(term.copy().into())
            } else if is_bool_literal(&term, true) {
                PbesKind::True
            } else if is_bool_literal(&term, false) {
                PbesKind::False
            } else {
                PbesKind::Data(term.copy().into())
            }
        })
    }

    /// Returns true iff `predicate` holds for some subexpression of `self`, including itself.
    pub fn any<F>(&self, mut predicate: F) -> bool
    where
        F: FnMut(&PbesExpressionRef<'a>) -> bool,
    {
        visit_pbes_expr(self, (), |expr, ()| {
            if predicate(expr) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(Step::Into(()))
            }
        })
        .is_some()
    }
}

/// A binary connective of PBES expressions, whose nested applications form a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbesConnective {
    And,
    Or,
}

impl Clone for PbesExpressionRef<'_> {
    fn clone(&self) -> Self {
        self.copy()
    }
}

impl<'a> BinaryChain for PbesExpressionRef<'a> {
    type Op = PbesConnective;

    fn binary_operands(&self, op: PbesConnective) -> Option<(Self, Self)> {
        match (self.kind(), op) {
            (PbesKind::And(lhs, rhs), PbesConnective::And) | (PbesKind::Or(lhs, rhs), PbesConnective::Or) => {
                Some((lhs, rhs))
            }
            _ => None,
        }
    }
}

impl BooleanChain for PbesExpressionRef<'_> {
    type Owned = PbesExpression;

    fn conjunction() -> PbesConnective {
        PbesConnective::And
    }

    fn disjunction() -> PbesConnective {
        PbesConnective::Or
    }

    fn into_owned(self) -> PbesExpression {
        self.protect()
    }
}

impl Boolean for PbesExpression {
    fn true_constant() -> Self {
        PbesExpression::true_constant()
    }

    fn false_constant() -> Self {
        PbesExpression::false_constant()
    }

    fn is_true(&self) -> bool {
        is_bool_literal(self, true)
    }

    fn is_false(&self) -> bool {
        is_bool_literal(self, false)
    }

    fn negated(&self) -> Option<Self> {
        match self.kind() {
            PbesKind::Not(inner) => Some(inner.protect()),
            _ => None,
        }
    }

    fn make_not(self) -> Self {
        PbesExpression::not(&self)
    }

    fn make_and(self, rhs: Self) -> Self {
        PbesExpression::and(&self, &rhs)
    }

    fn make_or(self, rhs: Self) -> Self {
        PbesExpression::or(&self, &rhs)
    }

    fn make_imp(self, rhs: Self) -> Self {
        PbesExpression::imp(&self, &rhs)
    }
}

/// The operands of a nested conjunction, in the order in which they occur and without duplicates,
/// as mCRL2's `split_and` returns a set.
pub fn split_and<'a>(expr: &PbesExpressionRef<'a>) -> Vec<PbesExpressionRef<'a>> {
    unique_chain_operands(expr.copy(), PbesConnective::And)
}

/// See [split_and]; splits a nested disjunction instead.
pub fn split_or<'a>(expr: &PbesExpressionRef<'a>) -> Vec<PbesExpressionRef<'a>> {
    unique_chain_operands(expr.copy(), PbesConnective::Or)
}

/// Assuming `expr` is `x₁ && … && xₙ`, returns `!x₁ || … || !xₙ`, which keeps the negation
/// pushed inwards.
pub fn distribute_not_over_and(expr: &PbesExpressionRef<'_>) -> PbesExpression {
    optimized_disjunction(split_and(expr).iter().map(|clause| optimized_not(clause.protect())))
}

/// Writes `x₁: D₁, …, xₙ: Dₙ`.
pub(crate) fn fmt_variables<'a, I>(f: &mut fmt::Formatter<'_>, variables: I) -> fmt::Result
where
    I: IntoIterator<Item = &'a DataVariable>,
{
    for (index, variable) in variables.into_iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{}: {}", variable, variable.sort())?;
    }
    Ok(())
}

impl fmt::Display for FixpointSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixpointSymbol::Mu => write!(f, "mu"),
            FixpointSymbol::Nu => write!(f, "nu"),
        }
    }
}

impl fmt::Display for PropositionalVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())?;
        let parameters = self.parameters().to_vec();
        if !parameters.is_empty() {
            write!(f, "(")?;
            fmt_variables(f, &parameters)?;
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl fmt::Display for PropositionalVariableInstantiationRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())?;
        let arguments = self.arguments();
        if !arguments.is_empty() {
            write!(f, "(")?;
            for (index, argument) in arguments.iter().enumerate() {
                if index > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{argument}")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl fmt::Display for PropositionalVariableInstantiation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.copy())
    }
}

impl fmt::Display for PbesExpressionRef<'_> {
    /// Writes the expression in mCRL2's textual syntax, with every compound operand in
    /// parentheses so that no precedence rules are needed to read it back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operand = |f: &mut fmt::Formatter<'_>, expr: &PbesExpressionRef<'_>| match expr.kind() {
            PbesKind::True | PbesKind::False | PbesKind::Data(_) | PbesKind::Instantiation(_) | PbesKind::Not(_) => {
                write!(f, "{expr}")
            }
            _ => write!(f, "({expr})"),
        };
        let binary = |f: &mut fmt::Formatter<'_>, lhs, op, rhs| {
            operand(f, &lhs)?;
            write!(f, " {op} ")?;
            operand(f, &rhs)
        };
        let quantifier = |f: &mut fmt::Formatter<'_>, name, variables: ATermList<DataVariable>, body| {
            write!(f, "{name} ")?;
            fmt_variables(f, &variables.to_vec())?;
            write!(f, " . ")?;
            operand(f, &body)
        };

        match self.kind() {
            PbesKind::True => write!(f, "true"),
            PbesKind::False => write!(f, "false"),
            PbesKind::Data(expr) => write!(f, "val({})", expr.protect()),
            PbesKind::Instantiation(inst) => write!(f, "{inst}"),
            PbesKind::Not(body) => {
                write!(f, "!")?;
                operand(f, &body)
            }
            PbesKind::And(lhs, rhs) => binary(f, lhs, "&&", rhs),
            PbesKind::Or(lhs, rhs) => binary(f, lhs, "||", rhs),
            PbesKind::Imp(lhs, rhs) => binary(f, lhs, "=>", rhs),
            PbesKind::Forall(variables, body) => quantifier(f, "forall", variables, body),
            PbesKind::Exists(variables, body) => quantifier(f, "exists", variables, body),
        }
    }
}

impl fmt::Display for PbesExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.copy())
    }
}
