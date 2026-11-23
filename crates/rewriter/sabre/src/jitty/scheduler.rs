use merc_data::DataExpressionRef;
use merc_utilities::debug_trace;

use super::compile::ArgumentsId;
use super::compile::LazyArgument;
use super::compile::LazyHook;
use super::compile::LazyStrategy;
use super::compile::Strategies;
use super::environment::Argument;
use super::environment::Closure;
use super::environment::EnvId;
use super::environment::Environments;
use super::strategy::StrategyStep;
use crate::rewrite_machine::Annotation;
use crate::rewrite_machine::Head;
use crate::rewrite_machine::MachineView;
use crate::rewrite_machine::Resumed;
use crate::rewrite_machine::Scheduler;
use crate::rewrite_machine::TermStackId;
use crate::rewrite_machine::Walk;
use crate::set_automaton::MatchAnnouncement;
use crate::set_automaton::MatchResult;
use crate::utilities::DataPosition;

/// The work of the [Strategies] on the stack.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Work {
    /// Continues a strategy.
    Strategy(StrategyFrame),
    /// Moves the term on top to `slot`, the slot of an argument of a strategy in progress.
    Store { slot: usize },
}

/// A strategy in progress.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StrategyFrame {
    strategy: u32,
    step: u32,
    /// The slot of the first argument on the term stack.
    base: usize,
    /// The lazy occurrence whose strategy this is, or `None` for a subterm of the input term,
    /// whose arguments that are not rewritten yet are its raw subterms.
    occurrence: Option<Occurrence>,
}

impl StrategyFrame {
    /// The strategy with index `strategy`, at `step`, for the arguments from slot `base` on.
    fn new(strategy: usize, step: usize, base: usize, occurrence: Option<Occurrence>) -> StrategyFrame {
        StrategyFrame {
            strategy: u32::try_from(strategy).expect("fewer than 2^32 strategies"),
            step: u32::try_from(step).expect("fewer than 2^32 steps"),
            base,
            occurrence,
        }
    }

    /// The same strategy, continued at `step`.
    fn at_step(self, step: usize) -> StrategyFrame {
        StrategyFrame {
            step: u32::try_from(step).expect("fewer than 2^32 steps"),
            ..self
        }
    }
}

/// A lazy occurrence in a right-hand side that is being evaluated. It holds a reference to the
/// environment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Occurrence {
    arguments: ArgumentsId,
    env: EnvId,
}

impl Strategies {
    fn lazy(&self, frame: &StrategyFrame) -> &LazyStrategy {
        &self.strategies[frame.strategy as usize]
    }

    /// Returns what the suspended argument `index` of `occurrence` stands for: a closure over its
    /// environment, or an argument of that environment.
    fn suspended_argument(&self, occurrence: Occurrence, index: usize) -> Argument<'static> {
        match self.arguments(occurrence.arguments)[index] {
            LazyArgument::Deferred(term_stack) => Argument::Closure(Closure {
                term_stack,
                env: occurrence.env,
            }),
            LazyArgument::Forward(rule_argument) => Argument::Forward(occurrence.env, rule_argument),
            LazyArgument::Compiled => unreachable!("a compiled argument is rewritten before the strategy starts"),
        }
    }

    /// Schedules rewriting argument `argument` of the strategy of `frame`, which is not rewritten
    /// yet, and storing its normal form in its slot.
    fn schedule_argument(&self, machine: &mut MachineView<'_, '_, '_, Self>, frame: &StrategyFrame, argument: usize) {
        let slot = frame.base + argument;
        machine.stacks.schedule_resume(Work::Store { slot });
        match frame.occurrence {
            // A raw subterm of the input term.
            None => machine.stacks.schedule_rewrite_slot(slot),
            Some(occurrence) => schedule_evaluation(machine, self.suspended_argument(occurrence, argument)),
        }
    }

    /// Records that the rules of step `step` of the strategy of `frame` were tried, and applies
    /// the rule that `found` is, if any. Returns whether a rule applied.
    fn tried(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        frame: &StrategyFrame,
        step: usize,
        found: Option<MatchResult<'_, Annotation>>,
    ) -> bool {
        machine.metrics.tried(found.is_some());
        let Some(MatchResult::Rule(announcement, annotation)) = found else {
            return false;
        };
        self.apply(machine, frame, step, announcement, annotation);
        true
    }

    /// Replaces the arguments of the strategy of `frame` by the right-hand side of the rule of
    /// `annotation`, which matched in step `step`.
    fn apply(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        frame: &StrategyFrame,
        step: usize,
        announcement: &MatchAnnouncement,
        annotation: &Annotation,
    ) {
        debug_trace!("rewrite using rule {}", announcement.rule);
        machine.metrics.rule_applied(&announcement.rule);

        let lazy = self.lazy(frame);
        let arity = lazy.strategy.arity;

        // The variables of a right-hand side with hooks refer to the arguments as they are now,
        // so a suspended argument of an occurrence is passed on.
        let right_hand_side = annotation.right_hand_side;
        let env = machine.term_stacks.get(right_hand_side).has_hooks().then(|| {
            let rewritten = &lazy.rewritten[step];
            machine.state.create(arity, |index| match frame.occurrence {
                Some(occurrence) if !rewritten[index] => self.suspended_argument(occurrence, index),
                _ => Argument::Term(machine.stacks.argument(frame.base + index)),
            })
        });

        let base = machine.stacks.len() - arity;
        if env.is_some() {
            machine.stacks.truncate(base);
        }
        machine.stacks.schedule_evaluate(right_hand_side, base, env);

        if let Some(occurrence) = frame.occurrence {
            machine.state.release(occurrence.env);
        }
    }

    /// Ends the strategy of `frame` without a rule applying: the term is a normal form once its
    /// remaining arguments are.
    fn finish(&self, machine: &mut MachineView<'_, '_, '_, Self>, frame: &StrategyFrame) {
        let lazy = self.lazy(frame);

        // The shared automaton has no rules for a lazy symbol, so constructing it only places it.
        machine
            .stacks
            .schedule_construct(&lazy.symbol.copy(), lazy.strategy.arity);
        // The last configuration runs first, so the arguments are scheduled from right to left to
        // be rewritten from left to right.
        for argument in lazy.remaining.iter_ones().rev() {
            self.schedule_argument(machine, frame, argument);
        }

        if let Some(occurrence) = frame.occurrence {
            machine.state.release(occurrence.env);
        }
    }
}

/// Schedules pushing the normal form of what `argument` stands for.
fn schedule_evaluation(machine: &mut MachineView<'_, '_, '_, Strategies>, argument: Argument<'_>) {
    match argument {
        Argument::Term(term) => machine.stacks.schedule_rewrite(&term),
        Argument::Closure(closure) => schedule_closure(machine, closure),
        Argument::Forward(env, index) => match machine.state.closure(env, index) {
            Some(closure) => schedule_closure(machine, closure),
            // An argument that is not a closure is a term, which may not be rewritten yet.
            None => machine.state.with_argument(env, index, |term| {
                machine.stacks.schedule_rewrite(term);
            }),
        },
    }
}

/// Schedules evaluating `closure`, whose value goes to the next slot, with a reference to its
/// environment.
fn schedule_closure(machine: &mut MachineView<'_, '_, '_, Strategies>, closure: Closure) {
    machine.state.retain(closure.env);
    let base = machine.stacks.len();
    machine
        .stacks
        .schedule_evaluate(closure.term_stack, base, Some(closure.env));
}

impl Scheduler for Strategies {
    type Hook = LazyHook;
    type Continuation = Work;
    /// The environment of a right-hand side with hooks. Without hooks, its variables are the
    /// arguments on the stack, as for the innermost rewriter.
    type Scope = Option<EnvId>;
    type State = Environments;

    /// Starts the strategy of `term` if its head is a lazy symbol: its arguments stay
    /// unrewritten in their slots until the strategy asks for them.
    fn decompose(&self, machine: &mut MachineView<'_, '_, '_, Self>, term: &DataExpressionRef<'_>) -> bool {
        let arguments = term.data_arguments();
        let Some(strategy) = self.lookup(&term.data_function_symbol(), arguments.len()) else {
            return false;
        };

        let base = machine.stacks.len();
        for argument in arguments {
            machine.stacks.push(&argument);
        }
        machine
            .stacks
            .schedule_resume(Work::Strategy(StrategyFrame::new(strategy, 0, base, None)));
        true
    }

    fn enter(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        right_hand_side: TermStackId,
        arity: usize,
    ) -> Option<EnvId> {
        if !machine.term_stacks.get(right_hand_side).has_hooks() {
            return None;
        }

        let base = machine.stacks.len() - arity;
        let env = machine
            .state
            .create(arity, |index| Argument::Term(machine.stacks.argument(base + index)));
        machine.stacks.truncate(base);
        Some(env)
    }

    fn variable(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        scope: Option<EnvId>,
        base: usize,
        position: &DataPosition,
    ) {
        match scope {
            None => machine.stacks.push_argument(base, position),
            Some(env) => machine.state.with_subterm(env, position, |value| {
                machine.stacks.push(value);
            }),
        }
    }

    fn leave(&self, machine: &mut MachineView<'_, '_, '_, Self>, scope: Option<EnvId>) {
        if let Some(env) = scope {
            machine.state.release(env);
        }
    }

    fn hook(&self, machine: &mut MachineView<'_, '_, '_, Self>, hook: &LazyHook, scope: Option<EnvId>) {
        let env = scope.expect("a term stack with hooks is evaluated in an environment");
        match hook {
            LazyHook::Suspended => machine.stacks.push_empty(),
            LazyHook::Occurrence {
                strategy,
                step,
                arguments,
            } => {
                machine.state.retain(env);
                let base = machine.stacks.len() - self.arguments(*arguments).len();
                let occurrence = Occurrence {
                    arguments: *arguments,
                    env,
                };
                machine.stacks.schedule_resume(Work::Strategy(StrategyFrame::new(
                    *strategy,
                    *step,
                    base,
                    Some(occurrence),
                )));
            }
            LazyHook::Variable(argument) => schedule_evaluation(machine, Argument::Forward(env, *argument)),
        }
    }

    /// The slots of the arguments that the steps before `step` rewrote hold normal forms; the
    /// others hold the raw subterms of the input term, without an occurrence, or are suspended
    /// arguments of the occurrence.
    ///
    /// Steps that only try rules run here directly. A step that rewrites an argument schedules
    /// that rewrite with the rest of the strategy after it, so that the machine performs it
    /// without recursion.
    fn resume<'s>(&'s self, machine: &mut MachineView<'_, '_, '_, Self>, continuation: Work) -> Resumed<'s, Work> {
        let frame = match continuation {
            Work::Store { slot } => {
                machine.stacks.move_top_to(slot);
                return Resumed::Done;
            }
            Work::Strategy(frame) => frame,
        };

        let lazy = self.lazy(&frame);
        let arity = lazy.strategy.arity;

        let mut step = frame.step as usize;
        loop {
            match lazy.strategy.steps.get(step) {
                Some(StrategyStep::Rewrite(argument)) => {
                    // The occurrence moves on to the rest of the strategy, with its reference.
                    machine.stacks.schedule_resume(Work::Strategy(frame.at_step(step + 1)));
                    self.schedule_argument(machine, &frame, *argument);
                    return Resumed::Done;
                }
                Some(StrategyStep::Try(_)) => {
                    let automaton = lazy.automata[step]
                        .as_ref()
                        .expect("every step that tries rules has an automaton");
                    let found = match machine.walk(automaton, &lazy.symbol, frame.base, arity) {
                        Walk::Done(found) => found,
                        walk => {
                            return Resumed::Conditions {
                                automaton,
                                head: Head::Symbol(&lazy.symbol),
                                arity,
                                walk,
                                continuation: Work::Strategy(frame.at_step(step)),
                            };
                        }
                    };

                    if self.tried(machine, &frame, step, found) {
                        return Resumed::Done;
                    }
                    step += 1;
                }
                None => {
                    self.finish(machine, &frame);
                    return Resumed::Done;
                }
            }
        }
    }

    /// Continues the step of a strategy in which a rule with conditions was reached.
    fn matched(
        &self,
        machine: &mut MachineView<'_, '_, '_, Self>,
        continuation: Work,
        found: Option<MatchResult<'_, Annotation>>,
    ) {
        let Work::Strategy(frame) = continuation else {
            unreachable!("only a strategy has conditions to check");
        };

        let step = frame.step as usize;
        if !self.tried(machine, &frame, step, found) {
            // The machine continues with the next step.
            machine.stacks.schedule_resume(Work::Strategy(frame.at_step(step + 1)));
        }
    }
}
