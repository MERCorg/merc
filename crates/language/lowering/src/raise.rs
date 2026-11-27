//! Raising: the inverse of lowering, turning a [DataExpression] back into the syntax tree that
//! denotes it, so that it can be printed as mCRL2 text.

use std::fmt;

use merc_aterm::ATerm;
use merc_aterm::ATermList;
use merc_aterm::Symb;
use merc_aterm::Term;
use merc_data::BinderType;
use merc_data::ContainerSortKind;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::SortArrow;
use merc_data::SortCons;
use merc_data::SortExpression as DataSortExpression;
use merc_data::is_container_sort;
use merc_data::is_function_sort;
use merc_data::is_no_value;
use merc_data::is_structured_sort;
use merc_syntax::Assignment;
use merc_syntax::ComplexSort;
use merc_syntax::ConstructorDecl;
use merc_syntax::DataExpr;
use merc_syntax::DataExprBinaryOp;
use merc_syntax::DataExprKind;
use merc_syntax::DataExprUnaryOp;
use merc_syntax::IdDecl;
use merc_syntax::Quantifier;
use merc_syntax::Sort;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::Spanned;
use merc_utilities::Span;

use crate::system_symbols::C_DUB;
use crate::system_symbols::C_INT;
use crate::system_symbols::C_NAT;
use crate::system_symbols::C_NEG;
use crate::system_symbols::C_REAL;
use crate::system_symbols::C0;
use crate::system_symbols::C1;
use crate::system_symbols::CONCAT_DIGIT;
use crate::system_symbols::EMPTY_FSET;
use crate::system_symbols::EMPTY_LIST;
use crate::system_symbols::FSET_INSERT;
use crate::system_symbols::MOST_SIGNIFICANT_DIGIT;
use crate::system_symbols::MOST_SIGNIFICANT_DIGIT_NAT;
use crate::system_symbols::POS2NAT;

/// Displays a data expression as mCRL2 text: numbers are decoded, the system defined operators
/// are printed infix or prefix, and only the parentheses that their precedence requires are
/// written. See [raise_data_expression] and [DataExpr::pretty].
///
/// The [Display](fmt::Display) of [DataExpression] itself prints the term structure instead.
pub struct Pretty<'a>(pub DataExpressionRef<'a>);

impl<'a> From<&'a DataExpression> for Pretty<'a> {
    fn from(expr: &'a DataExpression) -> Self {
        Pretty(expr.copy())
    }
}

impl fmt::Display for Pretty<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", raise_data_expression(&self.0.protect()))
    }
}

/// Raises a data expression to the syntax tree that denotes it, undoing what lowering encodes:
/// numbers become literals, applications of the system defined operators become operator nodes,
/// and lists of constructors become list and set literals. The result has no spans.
pub fn raise_data_expression(expr: &DataExpression) -> DataExpr {
    if let Some(number) = decode_number(expr) {
        return number;
    }

    match expr.kind() {
        DataExpressionKind::Variable(variable) => DataExprKind::Id(variable.name().to_string()).into(),
        DataExpressionKind::FunctionSymbol(symbol) => match symbol.name().value() {
            "true" => DataExprKind::Bool(true).into(),
            "false" => DataExprKind::Bool(false).into(),
            EMPTY_LIST => DataExprKind::EmptyList.into(),
            EMPTY_FSET => DataExprKind::EmptySet.into(),
            name => DataExprKind::Id(name.to_string()).into(),
        },
        DataExpressionKind::MachineNumber(number) => DataExprKind::Number(number.value().to_string()).into(),
        DataExpressionKind::Application(_) => raise_application(expr),
        DataExpressionKind::Abstraction(abstraction) => {
            let mut variables: Vec<IdDecl> = Vec::new();
            for variable in abstraction.variables().iter() {
                let sort = raise_sort(&variable.sort().protect());
                variables.push(IdDecl::new(variable.name().to_string(), sort, Span::default()));
            }
            let body = Box::new(raise_data_expression(&abstraction.body().protect()));

            let kind = match abstraction.binder_type() {
                BinderType::Lambda => DataExprKind::Lambda { variables, body },
                BinderType::Forall => {
                    let op = Quantifier::Forall;
                    DataExprKind::Quantifier { op, variables, body }
                }
                BinderType::Exists => {
                    let op = Quantifier::Exists;
                    DataExprKind::Quantifier { op, variables, body }
                }
                BinderType::SetComp | BinderType::BagComp => {
                    let [variable] = <[IdDecl; 1]>::try_from(variables).expect("a comprehension binds one variable");
                    DataExprKind::SetBagComp {
                        variable,
                        predicate: body,
                    }
                }
            };
            kind.into()
        }
        DataExpressionKind::WhereClause(clause) => {
            let mut assignments = Vec::new();
            for declaration in clause.declarations().iter() {
                let value = raise_data_expression(&declaration.expression().protect());
                assignments.push(Assignment::new(declaration.variable().name().to_string(), value));
            }
            let expr = Box::new(raise_data_expression(&clause.body().protect()));
            DataExprKind::Whr { expr, assignments }.into()
        }
    }
}

/// Raises an application, recognising the system defined operators by the name and arity of
/// their function symbol.
fn raise_application(expr: &DataExpression) -> DataExpr {
    let arguments = arguments(expr);
    let name = head_name(expr);

    if let Some(name) = &name {
        if let Some(elements) = constructor_chain(expr, "|>", EMPTY_LIST) {
            return DataExprKind::List(elements).into();
        }
        if let Some(elements) = constructor_chain(expr, FSET_INSERT, EMPTY_FSET) {
            return DataExprKind::Set(elements).into();
        }

        match arguments.as_slice() {
            [operand] => {
                if let Some(op) = DataExprUnaryOp::ALL.iter().find(|op| op.to_string() == *name) {
                    return DataExprKind::Unary {
                        op: op.clone(),
                        expr: Box::new(raise_data_expression(operand)),
                    }
                    .into();
                }
            }
            [lhs, rhs] => {
                if let Some(op) = DataExprBinaryOp::ALL.iter().find(|op| op.to_string() == *name) {
                    return DataExprKind::Binary {
                        op: op.clone(),
                        lhs: Box::new(raise_data_expression(lhs)),
                        rhs: Box::new(raise_data_expression(rhs)),
                    }
                    .into();
                }
            }
            _ => {}
        }
    }

    let head = DataExpression::from(expr.arg(0).protect());
    DataExprKind::Application {
        function: Box::new(raise_data_expression(&head)),
        arguments: arguments.iter().map(raise_data_expression).collect(),
    }
    .into()
}

/// The raised elements of `expr` when it is a chain `insert(e₁, insert(e₂, … empty))` of the
/// binary constructor `insert` ending in the constant `empty`.
fn constructor_chain(expr: &DataExpression, insert: &str, empty: &str) -> Option<Vec<DataExpr>> {
    let mut elements = Vec::new();
    let mut current = expr.clone();
    loop {
        match current.kind() {
            DataExpressionKind::FunctionSymbol(symbol) if symbol.name().value() == empty => return Some(elements),
            DataExpressionKind::Application(_)
                if head_name(&current).as_deref() == Some(insert) && current.data_arguments().len() == 2 =>
            {
                elements.push(raise_data_expression(&current.data_arg(0).protect()));
                current = current.data_arg(1).protect();
            }
            _ => return None,
        }
    }
}

/// Raises a sort of the term representation to the syntax that denotes it, including a structured
/// sort `struct c(p: D)?r | …`. The result has no spans.
pub fn raise_sort(sort: &DataSortExpression) -> SortExpression {
    if is_structured_sort(sort) {
        raise_structured_sort(sort)
    } else if is_function_sort(sort) {
        let arrow = SortArrow::from(ATerm::from(sort.clone()));
        SortExpressionKind::FlattenedFunction {
            domain: arrow.domain().iter().map(|domain| raise_sort(&domain)).collect(),
            range: Box::new(raise_sort(&arrow.codomain().protect())),
        }
        .into()
    } else if is_container_sort(sort) {
        let container = SortCons::from(ATerm::from(sort.clone()));
        let kind = match container.kind() {
            ContainerSortKind::List => ComplexSort::List,
            ContainerSortKind::Set => ComplexSort::Set,
            ContainerSortKind::Bag => ComplexSort::Bag,
            ContainerSortKind::FSet => ComplexSort::FSet,
            ContainerSortKind::FBag => ComplexSort::FBag,
        };
        SortExpressionKind::Complex(kind, Box::new(raise_sort(&container.element_sort().protect()))).into()
    } else {
        let name = sort.name();
        match [Sort::Bool, Sort::Pos, Sort::Nat, Sort::Int, Sort::Real]
            .into_iter()
            .find(|simple| simple.name() == name)
        {
            Some(simple) => SortExpressionKind::Simple(simple).into(),
            None => SortExpressionKind::Reference(name.to_string()).into(),
        }
    }
}

/// Raises a `SortStruct([StructCons(name, [StructProj(projection, sort), …], recogniser), …])`.
fn raise_structured_sort(sort: &DataSortExpression) -> SortExpression {
    // The identifier that `term` stores, or `None` for the marker of an absent name.
    let identifier = |term: ATerm| {
        let name = term.get_head_symbol().name();
        (!name.is_empty() && !is_no_value(&term)).then(|| Spanned::from(name.to_string()))
    };

    let mut constructors = Vec::new();
    for constructor in ATermList::<ATerm>::from(sort.arg(0).protect()).iter() {
        let mut args = Vec::new();
        for projection in ATermList::<ATerm>::from(constructor.arg(1).protect()).iter() {
            let projection_sort = raise_sort(&DataSortExpression::from(projection.arg(1).protect()));
            args.push((identifier(projection.arg(0).protect()), projection_sort));
        }

        constructors.push(ConstructorDecl {
            name: identifier(constructor.arg(0).protect()).expect("a constructor has a name"),
            args,
            recogniser: identifier(constructor.arg(2).protect()),
        });
    }

    SortExpressionKind::Struct { inner: constructors }.into()
}

/// The name of the function symbol at the head of `expr`, when it is a function symbol or an
/// application of one.
fn head_name(expr: &DataExpression) -> Option<String> {
    match expr.kind() {
        DataExpressionKind::FunctionSymbol(symbol) => Some(symbol.name().value().to_string()),
        DataExpressionKind::Application(_) => expr
            .try_data_function_symbol()
            .map(|symbol| symbol.name().value().to_string()),
        _ => None,
    }
}

/// The data arguments of `expr`, which are empty unless it is an application.
fn arguments(expr: &DataExpression) -> Vec<DataExpression> {
    match expr.kind() {
        DataExpressionKind::Application(_) => expr.data_arguments().map(|argument| argument.protect()).collect(),
        _ => Vec::new(),
    }
}

/// Raises the numeric constructors in either encoding of [`merc_typecheck::NumberEncoding`], see
/// `system_symbols`. A constant number becomes a literal; the constructors that embed one numeric
/// sort into a wider one are left out otherwise, since type checking inserts them again.
fn decode_number(expr: &DataExpression) -> Option<DataExpr> {
    if let Some(value) = decode_natural(expr) {
        return Some(DataExprKind::Number(value.to_string()).into());
    }

    let name = head_name(expr)?;
    let kind = match (name.as_str(), arguments(expr).as_slice()) {
        (C_NEG, [positive]) => DataExprKind::Unary {
            op: DataExprUnaryOp::Minus,
            expr: Box::new(raise_data_expression(positive)),
        },
        (C_NAT | POS2NAT | C_INT, [inner]) => return Some(raise_data_expression(inner)),
        (C_REAL, [numerator, denominator]) => {
            let numerator = raise_data_expression(numerator);
            if decode_natural(denominator).is_some_and(|denominator| denominator.digits == [1]) {
                return Some(numerator);
            }

            DataExprKind::Binary {
                op: DataExprBinaryOp::Div,
                lhs: Box::new(numerator),
                rhs: Box::new(raise_data_expression(denominator)),
            }
        }
        _ => return None,
    };
    Some(kind.into())
}

/// Decodes a constant `Pos` or `Nat`. The constructors are nested once per bit or machine word,
/// so the chain is walked iteratively, outermost first, and the steps are applied innermost first.
fn decode_natural(expr: &DataExpression) -> Option<Natural> {
    /// A step from a number to the number that contains it.
    enum Step {
        /// `2 * value + bit`, for the binary constructor `@cDub(bit, value)`.
        Bit(u64),
        /// `2⁶⁴ * value + word`, for the machine word constructor `@concat_digit(value, word)`.
        Word(u64),
    }

    let mut steps = Vec::new();
    let mut current = expr.clone();
    let mut value = loop {
        if let DataExpressionKind::MachineNumber(word) = current.kind() {
            break Natural::from(word.value());
        }

        let name = head_name(&current)?;
        current = match (name.as_str(), arguments(&current).as_slice()) {
            (C0, []) => break Natural::from(0),
            (C1, []) => break Natural::from(1),
            (C_DUB, [bit, rest]) => {
                let bit = match head_name(bit)?.as_str() {
                    "true" => 1,
                    "false" => 0,
                    _ => return None,
                };
                steps.push(Step::Bit(bit));
                rest.clone()
            }
            (CONCAT_DIGIT, [rest, word]) => {
                let DataExpressionKind::MachineNumber(word) = word.kind() else {
                    return None;
                };
                steps.push(Step::Word(word.value()));
                rest.clone()
            }
            (C_NAT | POS2NAT | MOST_SIGNIFICANT_DIGIT | MOST_SIGNIFICANT_DIGIT_NAT, [inner]) => inner.clone(),
            _ => return None,
        };
    };

    for step in steps.into_iter().rev() {
        match step {
            Step::Bit(bit) => value.mul_add(2, bit),
            Step::Word(word) => {
                // In two steps, since a factor of 2⁶⁴ does not fit the digit arithmetic.
                value.mul_add(1 << 32, 0);
                value.mul_add(1 << 32, word);
            }
        }
    }
    Some(value)
}

/// An arbitrarily large natural number, as little-endian digits in base `10⁹`, so that each digit
/// prints as nine decimal digits.
struct Natural {
    digits: Vec<u32>,
}

/// The base of the digits of a [Natural].
const DIGIT_BASE: u128 = 1_000_000_000;

impl From<u64> for Natural {
    fn from(value: u64) -> Self {
        let mut result = Natural { digits: vec![0] };
        result.mul_add(1, value);
        result
    }
}

impl Natural {
    /// Sets `self` to `self * factor + addend`.
    fn mul_add(&mut self, factor: u64, addend: u64) {
        let mut carry = addend as u128;
        for digit in &mut self.digits {
            let value = *digit as u128 * factor as u128 + carry;
            *digit = (value % DIGIT_BASE) as u32;
            carry = value / DIGIT_BASE;
        }

        while carry > 0 {
            self.digits.push((carry % DIGIT_BASE) as u32);
            carry /= DIGIT_BASE;
        }

        while self.digits.len() > 1 && self.digits.last() == Some(&0) {
            self.digits.pop();
        }
    }
}

impl fmt::Display for Natural {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut digits = self.digits.iter().rev();
        write!(f, "{}", digits.next().expect("a natural has at least one digit"))?;
        for digit in digits {
            write!(f, "{digit:09}")?;
        }
        Ok(())
    }
}
