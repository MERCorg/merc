use std::convert::Infallible;
use std::fmt;

use ahash::HashMap;
use ahash::HashMapExt;
use merc_aterm::Protected;
use merc_aterm::Term;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbol;
use merc_data::DataVariable;
use merc_data::DataWhereClause;
use merc_data::DataWhrDecl;
use merc_data::is_data_variable;

use crate::Rule;
use crate::set_automaton::free_variables;
use crate::subject::Subject;

use super::DataPosition;
use super::DataPositionIndexed;
use super::DataPositionIterator;

/// A term with variables, compiled to a program that builds an instance of it.
///
/// The program is the term in post-order: running it pushes the value of every subterm onto a
/// stack of values, so that the arguments of an [Instruction::Apply] are the values on top. A
/// variable is a position in the left-hand side of a rule, whose subterm is its value. The
/// right-hand side of `f(x, y) = g(h(x), y)` is the program
///
/// ```text
/// @1  h/1  @2  g/2
/// ```
///
/// where `@1` pushes the subterm at position 1 of the matched term, the value of `x`, and `h/1`
/// replaces the value on top by `h` applied to it.
///
/// A program can have holes of type `H` in place of subterms, which whoever runs it interprets;
/// the default `H` has no values, so a term stack built from a term has no holes.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TermStack<H = Infallible> {
    pub instructions: Vec<Instruction<H>>,
    /// The blocks of `instructions`, in which they are consecutive. Block 0 builds the term.
    pub blocks: Vec<Block>,
}

/// The instructions `begin..end` of a [TermStack], which build one term (see [TermStack]).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Block {
    pub begin: usize,
    pub end: usize,
}

/// An instruction of a [TermStack], which pushes one value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Instruction<H> {
    /// The subterm at this position of the left-hand side.
    Variable(DataPosition),
    /// A constant without a head symbol (a machine number).
    Term(DataExpression),
    /// The symbol applied to the `arity` values on top, which it replaces.
    Apply {
        symbol: DataFunctionSymbol,
        arity: usize,
    },
    /// The binder `b`, given as the `template` `lambda x0, .., xn . b` over its free variables,
    /// for the `arity` values of those variables on top, which it replaces.
    Binder {
        template: DataAbstraction,
        arity: usize,
    },
    Hole(H),
}

impl TermStack {
    /// Compiles the right-hand side of `rule`.
    pub fn new(rule: &Rule) -> TermStack {
        Self::from_term(&rule.rhs.copy(), &create_var_map(&rule.lhs))
    }

    /// Compiles `term`, whose variables are taken from the given positions of a left-hand side.
    ///
    /// A binder is opaque: its values are its free variables, which are taken from the left-hand
    /// side like any other variable.
    pub fn from_term(term: &DataExpressionRef, var_map: &HashMap<DataVariable, DataPosition>) -> TermStack {
        /// An item of the post-order traversal.
        enum Work {
            Visit(DataExpression),
            Emit(Instruction<Infallible>),
        }

        let mut instructions = Vec::new();
        let mut work = vec![Work::Visit(term.protect())];
        while let Some(item) = work.pop() {
            let term = match item {
                Work::Emit(instruction) => {
                    instructions.push(instruction);
                    continue;
                }
                Work::Visit(term) => term,
            };

            match term.kind() {
                DataExpressionKind::Variable(variable) => {
                    let position = var_map
                        .get(&variable.protect())
                        .expect("All variables in the right hand side must occur in the left hand side");
                    instructions.push(Instruction::Variable(position.clone()));
                }
                DataExpressionKind::MachineNumber(_) => {
                    // A machine number is a constant without a head symbol to apply. Right-hand
                    // sides contain them whenever numeric literals are lowered with
                    // `NumberEncoding::MachineWord`.
                    instructions.push(Instruction::Term(term.clone()));
                }
                DataExpressionKind::Abstraction(_) => {
                    // Closing the binder over its free variables records which variables the
                    // values are for, so they need not be computed again for every instance.
                    let free: Vec<DataVariable> = free_variables(&term).collect();
                    let template = DataAbstraction::new(BinderType::Lambda, &free, term.clone());
                    work.push(Work::Emit(Instruction::Binder {
                        template,
                        arity: free.len(),
                    }));
                    work.extend(free.into_iter().rev().map(|variable| Work::Visit(variable.into())));
                }
                DataExpressionKind::Application(_) | DataExpressionKind::FunctionSymbol(_) => {
                    let arguments = term.data_arguments();
                    work.push(Work::Emit(Instruction::Apply {
                        symbol: term.data_function_symbol().protect(),
                        arity: arguments.len(),
                    }));
                    let arguments: Vec<DataExpression> = arguments.map(|argument| argument.protect()).collect();
                    work.extend(arguments.into_iter().rev().map(Work::Visit));
                }
                DataExpressionKind::WhereClause(_) => {
                    unreachable!("is_supported_rule keeps no rule with a where clause in its right-hand side")
                }
            }
        }

        let end = instructions.len();
        TermStack {
            instructions,
            blocks: vec![Block { begin: 0, end }],
        }
    }

    /// Instantiates the term for the subterms of `term`.
    pub fn evaluate<'a, 'b, T: Term<'a, 'b>>(&self, term: &'b T) -> DataExpression {
        let mut builder = TermStackBuilder::new();
        self.evaluate_with(term, &mut builder)
    }

    /// Like [TermStack::evaluate], reusing the stack of `builder`.
    pub fn evaluate_with<'a, 'b, T: Term<'a, 'b>>(
        &self,
        term: &'b T,
        builder: &mut TermStackBuilder,
    ) -> DataExpression {
        self.instantiate(&Subject::Term(&DataExpressionRef::from(term.copy())), builder)
    }

    /// Instantiates the term for the subterms of `subject`.
    pub(crate) fn instantiate(&self, subject: &Subject<'_>, builder: &mut TermStackBuilder) -> DataExpression {
        builder.values.write(|values| values.clear());

        // Each instruction takes a guard of its own, since what a guard protects must still be in
        // its container when the guard is dropped, and an application removes its arguments.
        for instruction in self.block(0) {
            builder.values.write(|values| {
                let (value, arity): (DataExpression, usize) = match instruction {
                    Instruction::Variable(position) => (subject.get_data_position(position).protect(), 0),
                    Instruction::Term(term) => (term.clone(), 0),
                    Instruction::Apply { symbol, arity } => {
                        let first = values.len() - arity;
                        let term = if *arity == 0 {
                            symbol.clone().into()
                        } else {
                            DataApplication::with_iter(symbol, *arity, values[first..].iter()).into()
                        };
                        (term, *arity)
                    }
                    Instruction::Binder { template, arity } => {
                        // Substituting the values into the binder could capture them, so they
                        // are bound by a where clause around it instead.
                        let first = values.len() - arity;
                        let binder = template.body().protect();
                        let term = if *arity == 0 {
                            binder
                        } else {
                            let declarations: Vec<DataWhrDecl> = template
                                .variables()
                                .into_iter()
                                .zip(values[first..].iter())
                                .map(|(variable, value)| DataWhrDecl::new(variable, value.protect()))
                                .collect();
                            DataWhereClause::new(binder, &declarations).into()
                        };
                        (term, *arity)
                    }
                    Instruction::Hole(hole) => match *hole {},
                };

                let first = values.len() - arity;
                values.truncate(first);
                // Safety: value is stored in the container on the next line.
                let value = unsafe { values.protect(&value) };
                values.push(value.into());
            });
        }

        let values = builder.values.read();
        debug_assert_eq!(values.len(), 1, "a term has exactly one value");
        values.last().expect("a term has a value").protect()
    }

    /// Returns true iff a variable occurs more than once, for example in "times(s(x), y) =
    /// plus(y, times(x,y))".
    pub(crate) fn contains_duplicate_var_references(&self) -> bool {
        let mut variables: Vec<&DataPosition> = self.variables().collect();
        variables.sort_unstable();
        let len = variables.len();
        variables.dedup();

        len != variables.len()
    }
}

impl<H> TermStack<H> {
    /// Returns the instructions of block `block`.
    pub fn block(&self, block: usize) -> &[Instruction<H>] {
        let Block { begin, end } = self.blocks[block];
        &self.instructions[begin..end]
    }

    /// Returns the positions of the variables, once per occurrence.
    pub fn variables(&self) -> impl Iterator<Item = &DataPosition> {
        self.instructions.iter().filter_map(|instruction| match instruction {
            Instruction::Variable(position) => Some(position),
            _ => None,
        })
    }
}

impl<H: fmt::Debug> fmt::Display for TermStack<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for block in 0..self.blocks.len() {
            write!(f, "{block}:")?;
            for instruction in self.block(block) {
                match instruction {
                    Instruction::Variable(position) => write!(f, " @{position}")?,
                    Instruction::Term(term) => write!(f, " {term}")?,
                    Instruction::Apply { symbol, arity } => write!(f, " {symbol}/{arity}")?,
                    Instruction::Binder { template, arity } => write!(f, " ({template})/{arity}")?,
                    Instruction::Hole(hole) => write!(f, " {hole:?}")?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// The stack of values that [TermStack::evaluate_with] reuses.
pub struct TermStackBuilder {
    values: Protected<Vec<DataExpressionRef<'static>>>,
}

impl TermStackBuilder {
    pub fn new() -> Self {
        Self {
            values: Protected::new(Vec::new()),
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
    use crate::utilities::DataPosition;
    use crate::utilities::Instruction;
    use crate::utilities::TermStack;
    use crate::utilities::create_var_map;

    use test_log::test;

    #[test]
    fn test_rhs_stack() {
        let rhs_stack = TermStack::new(&create_rewrite_rule("fact(s(N))", "times(s(N), fact(N))", &["N"]).unwrap());

        let n = DataPosition::new(&[1, 1]);
        let apply = |name: &str, arity: usize| Instruction::Apply {
            symbol: DataFunctionSymbol::new(name),
            arity,
        };
        assert_eq!(
            rhs_stack.instructions,
            [
                Instruction::Variable(n.clone()),
                apply("s", 1),
                Instruction::Variable(n),
                apply("fact", 1),
                apply("times", 2),
            ]
        );

        let lhs = DataExpression::from_string("fact(s(a))").unwrap();
        let rhs = DataExpression::from_string("times(s(a), fact(a))").unwrap();
        assert_eq!(rhs_stack.evaluate(&lhs), rhs);
    }

    #[test]
    fn test_rhs_stack_binder() {
        let variables = AHashSet::from([String::from("x"), String::from("y")]);
        let lhs = DataExpression::from_string_untyped("g(x)", &variables).unwrap();
        let x = DataVariable::from(DataExpression::from_string_untyped("x", &variables).unwrap());
        let y = DataVariable::from(DataExpression::from_string_untyped("y", &variables).unwrap());
        let body = DataExpression::from_string_untyped("f(x, y)", &variables).unwrap();
        let binder: DataExpression = DataAbstraction::new(BinderType::Exists, std::slice::from_ref(&y), body).into();

        // h(exists y. f(x, y)): the binder's only value is its free variable `x`, taken from g(x).
        let h = DataFunctionSymbol::new("h");
        let rhs: DataExpression = DataApplication::with_args(&h, std::slice::from_ref(&binder)).into();
        let rhs_stack = TermStack::new(&Rule::new(lhs, rhs));

        // The binder is stored closed over its free variable, as `lambda x . exists y. f(x, y)`.
        let template = DataAbstraction::new(BinderType::Lambda, std::slice::from_ref(&x), binder.clone());
        assert_eq!(
            rhs_stack.instructions,
            [
                Instruction::Variable(DataPosition::new(&[1])),
                Instruction::Binder { template, arity: 1 },
                Instruction::Apply {
                    symbol: h.clone(),
                    arity: 1
                },
            ]
        );

        // Substituting `y` for `x` would be captured, so the value is bound by a where clause.
        let instance = rhs_stack.evaluate(&DataExpression::from_string_untyped("g(y)", &variables).unwrap());
        let where_clause: DataExpression = DataWhereClause::new(binder, &[DataWhrDecl::new(x, y.into())]).into();
        let expected: DataExpression = DataApplication::with_args(&h, &[where_clause]).into();
        assert_eq!(instance, expected);
    }

    #[test]
    fn test_rhs_stack_variable() {
        let rhs = TermStack::new(&create_rewrite_rule("f(x)", "x", &["x"]).unwrap());
        assert_eq!(rhs.instructions, [Instruction::Variable(DataPosition::new(&[1]))]);
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
