use std::cell::RefCell;

use ahash::AHashMap;
use ahash::HashMap;
use log::debug;
use log::info;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbol;
use merc_data::DataFunctionSymbolRef;
use merc_data::DataVariable;
use merc_data::is_data_application;
use merc_data::is_data_function_symbol;

use super::strategy::ArgumentSet;
use super::strategy::Laziness;
use super::strategy::LazySymbol;
use super::strategy::Strategy;
use super::strategy::StrategyStep;
use crate::RewriteSpecification;
use crate::Rule;
use crate::rewrite_machine::Annotation;
use crate::rewrite_machine::TermStackId;
use crate::rewrite_machine::TermStacks;
use crate::set_automaton::SetAutomaton;
use crate::set_automaton::is_supported_rule_with;
use crate::utilities::DataPosition;
use crate::utilities::Replacement;
use crate::utilities::TermStack;
use crate::utilities::create_var_map;

/// The strategies of the lazy symbols: those whose strategy tries a rule before all their
/// arguments are rewritten. It is the scheduler of the [super::JittyRewriter].
pub(crate) struct Strategies {
    pub(super) strategies: Vec<LazyStrategy>,
    /// The index of the strategy of each lazy symbol, by operation id.
    pub(super) index: AHashMap<usize, usize>,
    /// Where the suspended arguments of the lazy occurrences come from.
    pub(super) occurrences: Vec<Box<[LazyArgument]>>,
}

/// The strategy of one lazy symbol, with what executing it needs.
pub(super) struct LazyStrategy {
    pub symbol: DataFunctionSymbol,
    pub strategy: Strategy,
    /// The arguments rewritten before each step, and after the last one.
    pub rewritten: Vec<ArgumentSet>,
    /// The arguments that are not rewritten after the last step.
    pub remaining: ArgumentSet,
    /// For each step that tries rules, an automaton for just those rules.
    pub automata: Vec<Option<SetAutomaton<Annotation>>>,
}

/// A hook in a right-hand side compiled for laziness.
#[derive(Clone, Debug)]
pub(crate) enum LazyHook {
    /// A suspended variable of the rule, the argument with this index, whose normal form is
    /// needed here.
    Variable(usize),
    /// The slot of a suspended argument of the lazy occurrence that follows.
    Suspended,
    /// An occurrence of a lazy symbol, whose arguments are on top: its strategy starts at
    /// `step`, and `arguments` says where the suspended ones come from.
    Occurrence {
        strategy: usize,
        step: usize,
        arguments: ArgumentsId,
    },
}

/// The index of the arguments of a lazy occurrence in [Strategies].
#[derive(Clone, Copy, Debug)]
pub(super) struct ArgumentsId(u32);

/// Where an argument of a lazy occurrence comes from.
#[derive(Clone, Copy, Debug)]
pub(super) enum LazyArgument {
    /// Rewritten before the strategy starts, so it is a normal form on the stack.
    Compiled,
    /// The term stack that builds it, as a closure.
    Deferred(TermStackId),
    /// A suspended argument of the rule, with this index, which is passed on as it is, so that a
    /// variable that is passed along a chain of rules does not build a chain of closures.
    Forward(usize),
}

impl Strategies {
    /// Generates the strategies of `spec`, and returns them with the automaton for the rules of
    /// the eager symbols. The right-hand sides are added to `term_stacks`.
    ///
    /// The rules of a lazy symbol are left out of that automaton, so that matching a lazy
    /// symbol in it finds nothing: the scheduler relies on that to construct its normal form
    /// once its strategy is done.
    pub fn build(
        spec: &RewriteSpecification,
        term_stacks: &RefCell<TermStacks<LazyHook>>,
    ) -> (SetAutomaton<Annotation>, Strategies) {
        let rules: Vec<Rule> = spec
            .rewrite_rules()
            .iter()
            .filter(|rule| is_supported_rule_with(rule, true))
            .cloned()
            .collect();
        let lazy = lazy_strategies(spec, &rules);
        let symbols = lazy_symbols(&lazy);

        let eager_rules: Vec<Rule> = rules
            .iter()
            .filter(|rule| head(rule).is_none_or(|symbol| !symbols.contains_key(&symbol.operation_id())))
            .cloned()
            .collect();
        let occurrences = RefCell::new(Vec::new());
        let laziness = Laziness {
            symbols: Some(&symbols),
            suspended: ArgumentSet::default(),
        };
        let automaton = SetAutomaton::with_binders(&spec.with_rules(eager_rules), |rule| {
            annotate(rule, &laziness, term_stacks, &occurrences)
        });

        let strategies: Vec<LazyStrategy> = lazy
            .into_iter()
            .map(|(symbol, strategy, members)| {
                LazyStrategy::new(symbol, strategy, &members, &rules, &symbols, term_stacks, &occurrences)
            })
            .collect();
        info!("{} of the symbols with rules are lazy", strategies.len());

        (
            automaton,
            Strategies {
                strategies,
                index: symbols.into_iter().map(|(id, lazy)| (id, lazy.strategy)).collect(),
                occurrences: occurrences.into_inner(),
            },
        )
    }

    /// Returns the index of the strategy of `symbol` applied to `arity` arguments, if it is a
    /// lazy symbol.
    pub(super) fn lookup(&self, symbol: &DataFunctionSymbolRef<'_>, arity: usize) -> Option<usize> {
        let strategy = *self.index.get(&symbol.operation_id())?;
        (self.strategies[strategy].strategy.arity == arity).then_some(strategy)
    }

    /// Returns where the arguments of a lazy occurrence come from.
    pub(super) fn arguments(&self, id: ArgumentsId) -> &[LazyArgument] {
        &self.occurrences[id.0 as usize]
    }
}

/// Returns the head symbol of the left-hand side of `rule`, if it is a function symbol applied
/// to arguments.
fn head(rule: &Rule) -> Option<DataFunctionSymbolRef<'_>> {
    (is_data_application(&rule.lhs) || is_data_function_symbol(&rule.lhs)).then(|| rule.lhs.data_function_symbol())
}

/// Returns the rules of every head symbol, as indices in `rules`, in the order of the
/// specification.
fn rules_by_head(rules: &[Rule]) -> Vec<(DataFunctionSymbol, Vec<usize>)> {
    let mut groups: Vec<(DataFunctionSymbol, Vec<usize>)> = Vec::new();
    let mut group_of: AHashMap<usize, usize> = AHashMap::new();
    for (index, rule) in rules.iter().enumerate() {
        let Some(symbol) = head(rule) else {
            continue;
        };
        let group = *group_of.entry(symbol.operation_id()).or_insert_with(|| {
            groups.push((symbol.protect(), Vec::new()));
            groups.len() - 1
        });
        groups[group].1.push(index);
    }
    groups
}

/// Generates the strategy of every head symbol, and returns the lazy ones with their rules, as
/// indices in `rules`.
fn lazy_strategies(spec: &RewriteSpecification, rules: &[Rule]) -> Vec<(DataFunctionSymbol, Strategy, Vec<usize>)> {
    let native: Vec<usize> = spec
        .native_symbols()
        .iter()
        .map(|(symbol, _)| symbol.operation_id())
        .collect();

    let mut lazy = Vec::new();
    for (symbol, members) in rules_by_head(rules) {
        let arity = rules[members[0]].lhs.data_arguments().len();

        // A native symbol stays eager, as does one whose rules apply it to different numbers of
        // arguments (higher-order).
        if arity == 0
            || native.contains(&symbol.operation_id())
            || members
                .iter()
                .any(|member| rules[*member].lhs.data_arguments().len() != arity)
        {
            continue;
        }

        let group_rules: Vec<Rule> = members.iter().map(|member| rules[*member].clone()).collect();
        let strategy = Strategy::generate(arity, &group_rules);
        if !strategy.is_eager() {
            debug!("strategy of {symbol}: {strategy}");
            lazy.push((symbol, strategy, members));
        }
    }
    lazy
}

/// Returns the lazy symbols by operation id, with the index of each in `lazy` as its strategy.
fn lazy_symbols(lazy: &[(DataFunctionSymbol, Strategy, Vec<usize>)]) -> AHashMap<usize, LazySymbol> {
    lazy.iter()
        .enumerate()
        .map(|(index, (symbol, strategy, _))| {
            (
                symbol.operation_id(),
                LazySymbol {
                    strategy: index,
                    arity: strategy.arity,
                    prefix: strategy.prefix(),
                    prefix_arguments: strategy.rewritten_before(strategy.prefix()),
                },
            )
        })
        .collect()
}

impl LazyStrategy {
    /// Prepares `strategy` of `symbol` for execution. Every step that tries rules gets an
    /// automaton for those rules, whose right-hand sides are compiled with the arguments that the
    /// step has not rewritten yet suspended.
    fn new(
        symbol: DataFunctionSymbol,
        strategy: Strategy,
        members: &[usize],
        rules: &[Rule],
        symbols: &AHashMap<usize, LazySymbol>,
        term_stacks: &RefCell<TermStacks<LazyHook>>,
        occurrences: &RefCell<Vec<Box<[LazyArgument]>>>,
    ) -> LazyStrategy {
        let rewritten: Vec<ArgumentSet> = (0..=strategy.steps.len())
            .map(|step| strategy.rewritten_before(step))
            .collect();

        let automata = strategy
            .steps
            .iter()
            .enumerate()
            .map(|(step, strategy_step)| {
                let StrategyStep::Try(tried) = strategy_step else {
                    return None;
                };

                let step_rules = tried.iter().map(|rule| rules[members[*rule]].clone()).collect();
                let laziness = Laziness {
                    symbols: Some(symbols),
                    suspended: !rewritten[step].clone(),
                };
                Some(SetAutomaton::with_binders(
                    &RewriteSpecification::new(step_rules),
                    |rule| annotate(rule, &laziness, term_stacks, occurrences),
                ))
            })
            .collect();

        LazyStrategy {
            symbol,
            strategy,
            remaining: !rewritten[rewritten.len() - 1].clone(),
            rewritten,
            automata,
        }
    }
}

/// Compiles the right-hand side of `rule` for `laziness`, and returns its annotation.
fn annotate(
    rule: &Rule,
    laziness: &Laziness<'_>,
    term_stacks: &RefCell<TermStacks<LazyHook>>,
    occurrences: &RefCell<Vec<Box<[LazyArgument]>>>,
) -> Annotation {
    let mut term_stacks = term_stacks.borrow_mut();
    let term_stack = compile(rule, laziness, &mut term_stacks, &mut occurrences.borrow_mut());
    term_stacks.annotate(rule, term_stack)
}

/// Compiles the right-hand side of `rule` for `laziness`, and every suspended argument in it into
/// a term stack of its own in `term_stacks`.
fn compile(
    rule: &Rule,
    laziness: &Laziness<'_>,
    term_stacks: &mut TermStacks<LazyHook>,
    occurrences: &mut Vec<Box<[LazyArgument]>>,
) -> TermStack<LazyHook> {
    let var_map = create_var_map(&rule.lhs);

    // A suspended argument can contain suspended arguments, which are compiled after the term
    // stack that refers to them to avoid recursion.
    let mut deferred = Vec::new();
    let mut compiler = Compiler {
        var_map: &var_map,
        laziness,
        term_stacks,
        occurrences,
        deferred: &mut deferred,
    };
    let term_stack = compiler.term_stack(&rule.rhs.copy());
    while let Some((id, term)) = compiler.deferred.pop() {
        let suspended = compiler.term_stack(&term.copy());
        compiler.term_stacks.set(id, suspended);
    }
    term_stack
}

/// What [compile] needs for every term stack of a right-hand side.
struct Compiler<'a> {
    var_map: &'a HashMap<DataVariable, DataPosition>,
    laziness: &'a Laziness<'a>,
    term_stacks: &'a mut TermStacks<LazyHook>,
    occurrences: &'a mut Vec<Box<[LazyArgument]>>,
    /// The suspended arguments that still have to be compiled.
    deferred: &'a mut Vec<(TermStackId, DataExpression)>,
}

impl Compiler<'_> {
    /// Compiles `term`. A suspended variable becomes a [LazyHook::Variable]. An occurrence of a
    /// lazy symbol becomes its arguments followed by a [LazyHook::Occurrence]: the arguments that
    /// its strategy rewrites before trying any rule are compiled in place, and every other one is
    /// a [LazyHook::Suspended] slot whose value is built by a term stack of its own when the
    /// strategy needs it.
    fn term_stack(&mut self, term: &DataExpressionRef<'_>) -> TermStack<LazyHook> {
        let var_map = self.var_map;
        let laziness = self.laziness;
        let suspended_variable = |term: &DataExpressionRef<'_>| match term.kind() {
            DataExpressionKind::Variable(variable) => var_map
                .get(&variable.protect())
                .and_then(|position| laziness.suspended_argument(position)),
            _ => None,
        };

        TermStack::new(term, var_map, |term| {
            if let Some(argument) = suspended_variable(term) {
                return Some(vec![Replacement::Hook(LazyHook::Variable(argument))]);
            }

            if !is_data_application(term) {
                return None;
            }
            let lazy = laziness.lazy_symbol(&term.data_function_symbol(), term.data_arguments().len())?;

            let mut replacement = Vec::with_capacity(lazy.arity + 1);
            let mut arguments = Vec::with_capacity(lazy.arity);
            for (index, argument) in term.data_arguments().enumerate() {
                if lazy.prefix_arguments[index] {
                    replacement.push(Replacement::Term(argument.protect()));
                    arguments.push(LazyArgument::Compiled);
                } else {
                    replacement.push(Replacement::Hook(LazyHook::Suspended));
                    arguments.push(match suspended_variable(&argument) {
                        Some(rule_argument) => LazyArgument::Forward(rule_argument),
                        None => {
                            let id = self.term_stacks.reserve();
                            self.deferred.push((id, argument.protect()));
                            LazyArgument::Deferred(id)
                        }
                    });
                }
            }

            self.occurrences.push(arguments.into());
            replacement.push(Replacement::Hook(LazyHook::Occurrence {
                strategy: lazy.strategy,
                step: lazy.prefix,
                arguments: ArgumentsId(
                    u32::try_from(self.occurrences.len() - 1).expect("fewer than 2^32 lazy occurrences"),
                ),
            }));
            Some(replacement)
        })
    }
}
