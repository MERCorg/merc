use std::collections::VecDeque;
use std::fmt;

use ahash::HashMap;
use ahash::HashMapExt;
use merc_aterm::ATermRef;
use merc_aterm::Markable;
use merc_aterm::Protected;
use merc_aterm::SymbolRef;
use merc_aterm::Term;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;
use merc_data::DataVariable;
use merc_data::DataWhereClause;
use merc_data::DataWhrDecl;
use merc_data::is_data_variable;
use merc_utilities::debug_trace;

use crate::Rule;
use crate::set_automaton::free_variables;
use crate::utilities::InnermostStack;

use super::DataPosition;
use super::DataPositionIterator;

/// A stack used to represent a term with free variables that can be constructed
/// efficiently.
///
/// It stores as much as possible in the term pool. Due to variables it cannot
/// be fully compressed. For variables it stores the position in the lhs of a
/// rewrite rule where the concrete term can be found that will replace the
/// variable.
///
#[derive(Hash, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TermStack {
    /// The innermost rewrite stack for the right hand side and the positions that must be added to the stack.
    pub innermost_stack: Protected<Vec<Config<'static>>>,
    /// The variables of the left-hand side that must be placed at certain places in the stack.
    pub variables: Vec<(DataPosition, usize)>,
    /// The number of elements that must be reserved on the innermost stack.
    pub stack_size: usize,
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

impl TermStack {
    /// Construct a new right-hand stack for a given equation/rewrite rule.
    pub fn new(rule: &Rule) -> TermStack {
        Self::from_term(&rule.rhs.copy(), &create_var_map(&rule.lhs))
    }

    /// Construct a term stack from a data expression where variables are taken from a specific position of the left hand side.
    ///
    /// The slots are allocated in breadth-first order, so that the arguments of a
    /// [Config::Construct] are the last ones on the stack when it is evaluated. A binder is opaque
    /// to the stack: its children are its free variables, which are filled in from the left-hand
    /// side like any other variable.
    pub fn from_term(term: &DataExpressionRef, var_map: &HashMap<DataVariable, DataPosition>) -> TermStack {
        // Compute the extra information for the InnermostRewriter.
        let mut innermost_stack: Protected<Vec<Config>> = Protected::new(vec![]);
        let mut variables = vec![];
        let mut stack_size = 0;

        let mut queue: VecDeque<DataExpression> = VecDeque::from([term.protect()]);
        while let Some(term) = queue.pop_front() {
            match term.kind() {
                DataExpressionKind::Variable(variable) => {
                    variables.push((
                        var_map
                            .get(&variable.protect())
                            .expect("All variables in the right hand side must occur in the left hand side")
                            .clone(),
                        stack_size,
                    ));
                }
                DataExpressionKind::MachineNumber(_) => {
                    // A machine number is a constant with no head function symbol to
                    // construct from, so it gets its own slot that is filled with the
                    // literal term. Right-hand sides contain them whenever numeric
                    // literals are lowered with `NumberEncoding::MachineWord`, where a
                    // number is a chain of `@word` digits.
                    innermost_stack.write(|write| {
                        // Safety: term is pushed into the container on the next line.
                        let t = unsafe { write.protect(&term) };
                        write.push(Config::Term(t.into(), stack_size));
                    });
                }
                DataExpressionKind::Abstraction(_) => {
                    // Closing the binder over its free variables records which variables the
                    // values are for, so they need not be computed again for every instance.
                    let free: Vec<DataVariable> = free_variables(&term).collect();
                    let template: DataExpression = DataAbstraction::new(BinderType::Lambda, &free, term.clone()).into();
                    innermost_stack.write(|write| {
                        // Safety: template is pushed into the container on the next line.
                        let t = unsafe { write.protect(&template) };
                        write.push(Config::Binder(t.into(), free.len(), stack_size));
                    });
                    queue.extend(free.into_iter().map(DataExpression::from));
                }
                DataExpressionKind::Application(_) | DataExpressionKind::FunctionSymbol(_) => {
                    let arity = term.data_arguments().len();
                    innermost_stack.write(|write| {
                        // Safety: symbol is pushed into the container on the next line.
                        let symbol = unsafe { write.protect(&term.data_function_symbol()) };
                        write.push(Config::Construct(symbol.into(), arity, stack_size));
                    });
                    queue.extend(term.data_arguments().map(|argument| argument.protect()));
                }
                DataExpressionKind::WhereClause(_) => {
                    unreachable!("is_supported_rule keeps no rule with a where clause in its right-hand side")
                }
            }
            stack_size += 1;
        }

        // A term always contributes at least its root variable or function
        // symbol to the stack. `InnermostStack::integrate` relies on this
        // (`stack_size - 1` would otherwise underflow).
        debug_assert!(
            stack_size >= 1,
            "A right-hand side must contain at least one variable or function symbol"
        );

        TermStack {
            innermost_stack,
            stack_size,
            variables,
        }
    }

    // See [evaluate_with]
    pub fn evaluate<'a, 'b, T: Term<'a, 'b>>(&self, term: &'b T) -> DataExpression {
        let mut builder = TermStackBuilder::new();
        self.evaluate_with(term, &mut builder)
    }

    /// Evaluate the rhs stack for the given term and returns the result.
    pub fn evaluate_with<'a, 'b, T: Term<'a, 'b>>(
        &self,
        term: &'b T,
        builder: &mut TermStackBuilder,
    ) -> DataExpression {
        let stack = &mut builder.stack;
        stack.terms.write(|write| {
            write.clear();
            write.push(None);
        });

        stack.configs.write(|write_configs| {
            stack.terms.write(|write_terms| {
                InnermostStack::integrate(
                    write_configs,
                    write_terms,
                    self,
                    &DataExpressionRef::from(term.copy()),
                    0,
                )
            })
        });

        loop {
            debug_trace!("{}", stack);

            let done = stack.configs.write(|write_configs| {
                let Some(config) = write_configs.pop() else {
                    return true;
                };

                stack.terms.write(|write_terms| match config {
                    Config::Construct(symbol, arity, index) => {
                        // Take the last arity arguments.
                        let length = write_terms.len();

                        let arguments = &write_terms[length - arity..];

                        let term: DataExpression = if arguments.is_empty() {
                            symbol.protect().into()
                        } else {
                            DataApplication::with_iter(&symbol.copy(), arguments.len(), arguments.iter().flatten())
                                .into()
                        };

                        // Add the term on the stack.
                        write_terms.drain(length - arity..);
                        // Safety: t is stored in the container on the next line.
                        let t = unsafe { write_terms.protect(&term) };
                        write_terms[index] = Some(t.into());
                    }
                    Config::Term(term, index) => {
                        // Safety: t is stored in the container on the next line.
                        let t = unsafe { write_terms.protect(&term) };
                        write_terms[index] = Some(t.into());
                    }
                    Config::Binder(template, arity, index) => {
                        // Substituting the values into the binder could capture them, so they
                        // are bound by a where clause around it instead.
                        let length = write_terms.len();
                        let template: DataAbstraction = template.protect().into();
                        let binder = template.body().protect();
                        let term: DataExpression = if arity == 0 {
                            binder
                        } else {
                            let declarations: Vec<DataWhrDecl> = template
                                .variables()
                                .into_iter()
                                .zip(write_terms[length - arity..].iter().flatten())
                                .map(|(variable, value)| DataWhrDecl::new(variable, value.protect()))
                                .collect();
                            DataWhereClause::new(binder, &declarations).into()
                        };

                        write_terms.drain(length - arity..);
                        // Safety: t is stored in the container on the next line.
                        let t = unsafe { write_terms.protect(&term) };
                        write_terms[index] = Some(t.into());
                    }
                    Config::Rewrite(_) => {
                        unreachable!("This case should not happen");
                    }
                    Config::Return() => {
                        unreachable!("This case should not happen");
                    }
                });
                false
            });

            if done {
                break;
            }
        }

        debug_assert!(
            stack.terms.read().len() == 1,
            "Expect exactly one term on the result stack"
        );

        stack.terms.write(|write_terms| {
            write_terms
                .pop()
                .expect("The result should be the last element on the stack")
                .expect("The result should be Some")
                .protect()
        })
    }

    /// Used to check if a subterm is duplicated, for example "times(s(x), y) =
    /// plus(y, times(x,y))" is duplicating.
    pub(crate) fn contains_duplicate_var_references(&self) -> bool {
        let mut variables: Vec<&DataPosition> = self.variables.iter().map(|(v, _)| v).collect();

        // Check if there are duplicates.
        variables.sort_unstable();
        let len = variables.len();
        variables.dedup();

        len != variables.len()
    }
}

impl Clone for TermStack {
    fn clone(&self) -> Self {
        // TODO: It would make sense if Protected could implement Clone.
        let mut innermost_stack: Protected<Vec<Config>> = Protected::new(vec![]);

        let read = self.innermost_stack.read();
        innermost_stack.write(|write| {
            for t in read.iter() {
                match t {
                    Config::Rewrite(x) => write.push(Config::Rewrite(*x)),
                    Config::Construct(f, x, y) => {
                        write.push(Config::Construct(f.copy(), *x, *y));
                    }
                    Config::Term(t, y) => {
                        write.push(Config::Term(t.copy(), *y));
                    }
                    Config::Binder(t, x, y) => {
                        write.push(Config::Binder(t.copy(), *x, *y));
                    }
                    Config::Return() => write.push(Config::Return()),
                }
            }
        });

        Self {
            variables: self.variables.clone(),
            stack_size: self.stack_size,
            innermost_stack,
        }
    }
}

pub struct TermStackBuilder {
    stack: InnermostStack,
}

impl TermStackBuilder {
    pub fn new() -> Self {
        Self {
            stack: InnermostStack::default(),
        }
    }
}

impl Default for TermStackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a mapping of variables to their position in the given term
pub fn create_var_map(t: &DataExpression) -> HashMap<DataVariable, DataPosition> {
    let mut result = HashMap::new();

    for (term, position) in DataPositionIterator::new(t.copy()) {
        if is_data_variable(&term) {
            result.insert(term.protect().into(), position.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use ahash::HashMap;
    use ahash::HashMapExt;
    use merc_aterm::Protected;
    use merc_data::BinderType;
    use merc_data::DataAbstraction;
    use merc_data::DataApplication;
    use merc_data::DataExpression;
    use merc_data::DataFunctionSymbol;
    use merc_data::DataVariable;
    use merc_data::DataWhereClause;
    use merc_data::DataWhrDecl;
    use merc_utilities::test_logger;

    use crate::Rule;
    use crate::test_utility::create_rewrite_rule;
    use crate::utilities::Config;
    use crate::utilities::DataPosition;
    use crate::utilities::TermStack;
    use crate::utilities::create_var_map;

    use test_log::test;

    #[test]
    fn test_rhs_stack() {
        let rhs_stack = TermStack::new(&create_rewrite_rule("fact(s(N))", "times(s(N), fact(N))", &["N"]).unwrap());
        let mut expected: Protected<Vec<Config<'static>>> = Protected::new(vec![]);

        let t1 = DataFunctionSymbol::new("times");
        let t2 = DataFunctionSymbol::new("s");
        let t3 = DataFunctionSymbol::new("fact");

        expected.write(|write| {
            write.push(Config::Construct(t1.copy(), 2, 0));
            write.push(Config::Construct(t2.copy(), 1, 1));
            write.push(Config::Construct(t3.copy(), 1, 2));
        });

        // Check if the resulting construction succeeded.
        assert_eq!(
            rhs_stack.innermost_stack, expected,
            "The resulting config stack is not as expected"
        );

        assert_eq!(rhs_stack.stack_size, 5, "The stack size does not match");

        // Test the evaluation
        let lhs = DataExpression::from_string("fact(s(a))").unwrap();
        let rhs = DataExpression::from_string("times(s(a), fact(a))").unwrap();

        assert_eq!(
            rhs_stack.evaluate(&lhs),
            rhs,
            "The rhs stack does not evaluate to the expected term"
        );
    }

    #[test]
    fn test_rhs_stack_binder() {
        let variables = AHashSet::from([String::from("x"), String::from("y")]);
        let lhs = DataExpression::from_string_untyped("g(x)", &variables).unwrap();
        let x = DataVariable::from(DataExpression::from_string_untyped("x", &variables).unwrap());
        let y = DataVariable::from(DataExpression::from_string_untyped("y", &variables).unwrap());
        let body = DataExpression::from_string_untyped("f(x, y)", &variables).unwrap();
        let binder: DataExpression = DataAbstraction::new(BinderType::Exists, std::slice::from_ref(&y), body).into();

        // h(exists y. f(x, y)): the binder's only child is its free variable `x`, taken from g(x).
        let h = DataFunctionSymbol::new("h");
        let rhs: DataExpression = DataApplication::with_args(&h, std::slice::from_ref(&binder)).into();
        let rhs_stack = TermStack::new(&Rule::new(lhs, rhs));

        // The binder is stored closed over its free variable, as `lambda x . exists y. f(x, y)`.
        let template: DataExpression =
            DataAbstraction::new(BinderType::Lambda, std::slice::from_ref(&x), binder.clone()).into();
        let mut expected: Protected<Vec<Config<'static>>> = Protected::new(vec![]);
        expected.write(|write| {
            write.push(Config::Construct(h.copy(), 1, 0));
            write.push(Config::Binder(template.copy(), 1, 1));
        });
        assert_eq!(rhs_stack.innermost_stack, expected);
        assert_eq!(rhs_stack.stack_size, 3);
        assert_eq!(rhs_stack.variables, [(DataPosition::new(&[1]), 2)]);

        // Substituting `y` for `x` would be captured, so the value is bound by a where clause.
        let instance = rhs_stack.evaluate(&DataExpression::from_string_untyped("g(y)", &variables).unwrap());
        let where_clause: DataExpression = DataWhereClause::new(binder, &[DataWhrDecl::new(x, y.into())]).into();
        let expected: DataExpression = DataApplication::with_args(&h, &[where_clause]).into();
        assert_eq!(instance, expected);
    }

    #[test]
    fn test_rhs_stack_variable() {
        let rhs = TermStack::new(&create_rewrite_rule("f(x)", "x", &["x"]).unwrap());

        // Check if the resulting construction succeeded.
        assert!(
            rhs.innermost_stack.read().is_empty(),
            "The resulting config stack is not as expected"
        );

        assert_eq!(rhs.stack_size, 1, "The stack size does not match");
    }

    #[test]
    fn test_evaluation() {
        test_logger();

        let rhs = DataExpression::from_string_untyped("f(f(a,a),x)", &AHashSet::from([String::from("x")])).unwrap();
        let lhs = DataExpression::from_string("g(b)").unwrap();

        // Make a variable map with only x@1.
        let mut map = HashMap::new();
        map.insert(DataVariable::new("x"), DataPosition::new(&[1]));

        let sctt = TermStack::from_term(&rhs.copy(), &map);

        let expected = DataExpression::from_string("f(f(a,a),b)").unwrap();

        assert_eq!(sctt.evaluate(&lhs), expected);
    }

    #[test]
    fn test_create_varmap() {
        let t = DataExpression::from_string_untyped("f(x,x)", &AHashSet::from([String::from("x")])).unwrap();
        let x = DataVariable::new("x");

        let map = create_var_map(&t);
        assert!(map.contains_key(&x));
    }

    #[test]
    fn test_is_duplicating() {
        let rhs = DataExpression::from_string_untyped("f(x,x)", &AHashSet::from([String::from("x")])).unwrap();

        // Make a variable map with only x@1.
        let mut map = HashMap::new();
        map.insert(DataVariable::new("x"), DataPosition::new(&[1]));

        let sctt = TermStack::from_term(&rhs.copy(), &map);
        assert!(sctt.contains_duplicate_var_references(), "This sctt is duplicating");
    }
}
