#![forbid(unsafe_code)]

use std::fmt;

use ahash::AHashMap;
use bitvec::vec::BitVec;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;
use merc_data::DataVariableRef;

use crate::Rule;
use crate::set_automaton::free_variables;
use crate::utilities::DataPosition;

/// A set of argument indices of a function symbol, with one bit per argument.
pub type ArgumentSet = BitVec;

/// One step of a [Strategy].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StrategyStep {
    /// Rewrite the argument with this index to normal form.
    Rewrite(usize),
    /// Try these rules, given as indices into the rules the strategy was generated from.
    Try(Vec<usize>),
}

/// A strategy annotation for one function symbol, in the sense of van de Pol's just-in-time
/// rewriting: an arbitrary sequence of arguments to rewrite and rules to try at the root.
///
/// After the last step the arguments that are not rewritten yet are rewritten, so that
/// executing a strategy always yields a normal form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Strategy {
    pub arity: usize,
    pub steps: Vec<StrategyStep>,
}

impl Strategy {
    /// Generates the default strategy for the rules of a symbol of the given arity, all of
    /// whose left-hand sides apply that symbol to `arity` arguments.
    ///
    /// A rule is tried as soon as all its needed arguments (see [needed_arguments]) are
    /// rewritten, and otherwise the argument needed by the most of the remaining rules is
    /// rewritten next. Ties go to the lowest index, since the rewriting machine rewrites the
    /// arguments of an eager symbol from left to right as well.
    pub fn generate(arity: usize, rules: &[Rule]) -> Strategy {
        let mut pending: Vec<(usize, ArgumentSet)> = rules
            .iter()
            .enumerate()
            .map(|(index, rule)| (index, needed_arguments(rule, arity)))
            .collect();
        let mut rewritten = ArgumentSet::repeat(false, arity);
        let mut steps = Vec::new();

        while !pending.is_empty() {
            let ready: Vec<usize> = pending
                .iter()
                .filter(|(_, needed)| needed.iter_ones().all(|argument| rewritten[argument]))
                .map(|(index, _)| *index)
                .collect();
            if !ready.is_empty() {
                pending.retain(|(index, _)| !ready.contains(index));
                steps.push(StrategyStep::Try(ready));
                continue;
            }

            // Every pending rule needs an argument that is not rewritten yet.
            let next = (0..arity)
                .filter(|argument| !rewritten[*argument])
                .max_by_key(|argument| {
                    let count = pending.iter().filter(|(_, needed)| needed[*argument]).count();
                    // max_by_key returns the last maximum, so prefer lower indices explicitly.
                    (count, std::cmp::Reverse(*argument))
                })
                .expect("a pending rule needs an argument that is not rewritten yet");
            rewritten.set(next, true);
            steps.push(StrategyStep::Rewrite(next));
        }

        Strategy { arity, steps }
    }

    /// Returns the arguments that are rewritten before the step with the given index.
    pub fn rewritten_before(&self, step: usize) -> ArgumentSet {
        let mut rewritten = ArgumentSet::repeat(false, self.arity);
        for strategy_step in &self.steps[..step] {
            if let StrategyStep::Rewrite(argument) = strategy_step {
                rewritten.set(*argument, true);
            }
        }
        rewritten
    }

    /// Returns the number of leading [StrategyStep::Rewrite] steps.
    pub fn prefix(&self) -> usize {
        self.steps
            .iter()
            .take_while(|step| matches!(step, StrategyStep::Rewrite(_)))
            .count()
    }

    /// Returns true iff every argument is rewritten before any rule is tried, which is what the
    /// innermost rewriter does for every symbol.
    pub fn is_eager(&self) -> bool {
        self.rewritten_before(self.prefix()).all()
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                write!(f, ", ")?;
            }
            match step {
                StrategyStep::Rewrite(argument) => write!(f, "{argument}")?,
                StrategyStep::Try(rules) => write!(f, "{rules:?}")?,
            }
        }
        Ok(())
    }
}

/// A lazy symbol, as a right-hand side is compiled for it.
#[derive(Clone, Debug)]
pub(crate) struct LazySymbol {
    /// The index of its strategy.
    pub strategy: usize,
    pub arity: usize,
    /// The number of leading [StrategyStep::Rewrite] steps of its strategy, see
    /// [Strategy::prefix].
    pub prefix: usize,
    /// The arguments those steps rewrite.
    pub prefix_arguments: ArgumentSet,
}

/// What a right-hand side is compiled for: the lazy symbols, whose occurrences suspend their
/// arguments, and the arguments of the rule's left-hand side that are suspended when the rule
/// is applied. The default has neither, as for the innermost rewriter.
#[derive(Default)]
pub(crate) struct Laziness<'a> {
    /// The lazy symbols by operation id.
    pub symbols: Option<&'a AHashMap<usize, LazySymbol>>,
    pub suspended: ArgumentSet,
}

impl Laziness<'_> {
    /// Returns the lazy symbol `symbol` applied to `arity` arguments, if it is one.
    pub fn lazy_symbol(&self, symbol: &DataFunctionSymbolRef<'_>, arity: usize) -> Option<LazySymbol> {
        self.symbols?
            .get(&symbol.operation_id())
            .filter(|lazy| lazy.arity == arity)
            .cloned()
    }

    /// Returns the index of the suspended argument at `position` of the left-hand side, if the
    /// variable at that position is one.
    pub fn suspended_argument(&self, position: &DataPosition) -> Option<usize> {
        match position.indices() {
            [index] if self.suspended.get(index - 1).is_some_and(|suspended| *suspended) => Some(index - 1),
            _ => None,
        }
    }
}

/// Returns the arguments that must be in normal form before `rule` is tried, for a rule whose
/// left-hand side applies its head symbol to `arity` arguments.
///
/// An argument is needed when it is not a variable, since matching inspects it. A variable
/// argument is needed when it occurs elsewhere in the left-hand side (matching compares the
/// normal forms), in the condition (conditions are built from normal forms), more than once in
/// the right-hand side (it would otherwise be rewritten once per copy) or in a binder of the
/// right-hand side (binders are rewritten under the normal forms of their free variables).
/// The first four are mCRL2's dependencies; the last is merc's.
pub(crate) fn needed_arguments(rule: &Rule, arity: usize) -> ArgumentSet {
    let mut needed = ArgumentSet::repeat(false, arity);
    let lhs = rule.lhs.copy();

    let condition_variables: Vec<_> = rule
        .conditions
        .iter()
        .flat_map(|condition| free_variables(&condition.lhs).chain(free_variables(&condition.rhs)))
        .collect();

    for (index, argument) in lhs.data_arguments().enumerate().take(arity) {
        let DataExpressionKind::Variable(variable) = argument.kind() else {
            needed.set(index, true);
            continue;
        };

        let mut lhs_count = Occurrences::default();
        lhs_count.count(&lhs, &variable, false);
        let mut rhs_count = Occurrences::default();
        rhs_count.count(&rule.rhs.copy(), &variable, false);

        if lhs_count.total > 1
            || condition_variables.iter().any(|condition| condition.copy() == variable)
            || rhs_count.total > 1
            || rhs_count.under_binder
        {
            needed.set(index, true);
        }
    }

    needed
}

/// The free occurrences of a variable in a term.
#[derive(Default)]
struct Occurrences {
    total: usize,
    /// Whether one of them is inside a binder.
    under_binder: bool,
}

impl Occurrences {
    fn count(&mut self, term: &DataExpressionRef<'_>, variable: &DataVariableRef<'_>, in_binder: bool) {
        match term.kind() {
            DataExpressionKind::Variable(occurrence) => {
                if occurrence == *variable {
                    self.total += 1;
                    self.under_binder |= in_binder;
                }
            }
            DataExpressionKind::Application(_) => {
                for argument in term.data_arguments() {
                    self.count(&argument, variable, in_binder);
                }
            }
            DataExpressionKind::Abstraction(abstraction) => {
                // A binder of the same variable shadows it.
                if !abstraction.variables().iter().any(|bound| bound.copy() == *variable) {
                    self.count(&abstraction.body(), variable, true);
                }
            }
            DataExpressionKind::FunctionSymbol(_)
            | DataExpressionKind::MachineNumber(_)
            | DataExpressionKind::WhereClause(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Rule;
    use crate::strategy::Strategy;
    use crate::strategy::StrategyStep;
    use crate::test_utility::create_rewrite_rule;

    fn rules(rules: &[(&str, &str)], variables: &[&str]) -> Vec<Rule> {
        rules
            .iter()
            .map(|(lhs, rhs)| create_rewrite_rule(lhs, rhs, variables).unwrap())
            .collect()
    }

    #[test]
    fn test_strategy_of_if_rewrites_the_condition_first() {
        let rules = rules(&[("if(true, x, y)", "x"), ("if(false, x, y)", "y")], &["x", "y"]);
        let strategy = Strategy::generate(3, &rules);

        assert_eq!(
            strategy.steps,
            [StrategyStep::Rewrite(0), StrategyStep::Try(vec![0, 1])]
        );
        assert!(!strategy.is_eager());
    }

    #[test]
    fn test_strategy_of_and_breaks_the_tie_left_to_right() {
        let rules = rules(
            &[
                ("and(b, true)", "b"),
                ("and(b, false)", "false"),
                ("and(true, b)", "b"),
                ("and(false, b)", "false"),
            ],
            &["b"],
        );
        let strategy = Strategy::generate(2, &rules);

        assert_eq!(
            strategy.steps,
            [
                StrategyStep::Rewrite(0),
                StrategyStep::Try(vec![2, 3]),
                StrategyStep::Rewrite(1),
                StrategyStep::Try(vec![0, 1]),
            ]
        );
    }

    #[test]
    fn test_non_linear_and_duplicated_variables_are_needed() {
        // x occurs twice in the left-hand side and y twice in the right-hand side.
        let rules = rules(&[("f(x, x, y)", "g(y, y)")], &["x", "y"]);
        let strategy = Strategy::generate(3, &rules);

        assert!(strategy.is_eager(), "strategy {strategy}");
    }

    #[test]
    fn test_unused_argument_is_never_rewritten_before_the_rule() {
        let rules = rules(&[("first(x, y)", "x")], &["x", "y"]);
        let strategy = Strategy::generate(2, &rules);

        assert_eq!(strategy.steps, [StrategyStep::Try(vec![0])]);
        assert_eq!(strategy.prefix(), 0);
    }
}
