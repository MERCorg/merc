use std::cell::OnceCell;

use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbol;
use merc_data::DataVariable;
use merc_utilities::debug_trace;

use super::Config;
use super::Evaluate;
use super::InnermostStack;
use super::Resumed;
use super::Scheduler;
use super::Stacks;

use crate::Enumerator;
use crate::RewriteEngine;
use crate::RewriteMetrics;
use crate::Rule;
use crate::binder::BinderRewriter;
use crate::binder::BinderState;
use crate::binder::rewrite_binder;
use crate::matching::condition_cache::ConditionCache;
use crate::matching::conditions::EMACondition;
use crate::matching::conditions::extend_conditions;
use crate::matching::nonlinear::EquivalenceClass;
use crate::matching::nonlinear::check_equivalence_classes;
use crate::matching::nonlinear::derive_equivalence_classes;
use crate::set_automaton::MatchAnnouncement;
use crate::set_automaton::MatchResult;
use crate::set_automaton::SetAutomaton;
use crate::subject::Subject;
use crate::utilities::EmptySubstitution;
use crate::utilities::Instruction;
use crate::utilities::RewriteSubstitution;
use crate::utilities::SliceSubstitution;
use crate::utilities::TermStack;
use crate::utilities::TermStackBuilder;

/// What [rewrite_aux] does next, after the configuration on top has been looked at.
enum Step<'p, S: Scheduler> {
    Continue,

    /// Match the redex on top of the stack, which applies `head` to `arity` arguments.
    Match {
        head: Head<'p>,
        arity: usize,
    },

    /// Check the conditions of the rule at which `walk` stopped for the redex on top, see
    /// [find_match], and continue as `then` says.
    Conditions {
        automaton: &'p SetAutomaton<Annotation>,
        head: Head<'p>,
        arity: usize,
        walk: Walk<'p>,
        then: Then<S::Continuation>,
    },

    /// Continue the work of the scheduler, see [Scheduler::resume].
    Resume(S::Continuation),

    /// Evaluate a hook of a term stack, see [Scheduler::hook].
    Hook {
        hook: &'p S::Hook,
        scope: S::Scope,
    },

    /// Rewrite a binder of the input term under `sigma`.
    SubjectBinder(DataAbstraction),

    /// Rewrite the binder `b` of a right-hand side, given as the `template` `lambda x0, .., xn .
    /// b` over its free variables, under `xi := v_i` for the `arity` terms `v_i` on top.
    RhsBinder {
        template: &'p DataAbstraction,
        arity: usize,
    },

    /// The normal form of the input term.
    Return(DataExpression),
}

/// What happens with the rule that is found after checking conditions.
enum Then<K> {
    /// The machine replaces the redex.
    Reduce,
    /// The scheduler continues, see [Scheduler::matched].
    Resume(K),
}

/// The rewriting machine shared by the rewriters: the automaton of the rules, and the stacks and
/// caches that rewriting reuses. The rewriters differ only in their [Scheduler].
///
/// The right-hand sides of the rules are term stacks (see [TermStack]), which the machine
/// evaluates in place.
pub(crate) struct RewriteMachine<E: Enumerator, S: Scheduler> {
    /// Matches the symbols that the scheduler leaves to the machine.
    automaton: SetAutomaton<Annotation>,
    term_stacks: TermStacks<S::Hook>,
    scheduler: S,
    state: S::State,
    stack: InnermostStack<S>,
    builder: TermStackBuilder,
    /// Caches condition-side normal forms across the whole computation.
    condition_cache: ConditionCache,
    /// Decides `exists`/`forall` quantifiers the rewriter reaches; see [Enumerator].
    enumerator: E,
    /// The state for rewriting binders, see [BinderState].
    binders: BinderState<E::Frame>,
    /// The metrics of the last rewrite.
    metrics: RewriteMetrics,
}

impl<E: Enumerator, S: Scheduler> RewriteMachine<E, S> {
    pub fn new(
        automaton: SetAutomaton<Annotation>,
        term_stacks: TermStacks<S::Hook>,
        scheduler: S,
        max_entries: Option<usize>,
        enumerator: E,
    ) -> Self {
        RewriteMachine {
            automaton,
            term_stacks,
            scheduler,
            state: S::State::default(),
            stack: InnermostStack::default(),
            builder: TermStackBuilder::new(),
            condition_cache: ConditionCache::new(max_entries),
            enumerator,
            binders: BinderState::default(),
            metrics: RewriteMetrics::default(),
        }
    }

    pub fn metrics(&self) -> &RewriteMetrics {
        &self.metrics
    }

    /// Replaces the free variables of `t` according to `sigma` and rewrites the result to normal
    /// form.
    ///
    /// `sigma` must map every variable to a term already in normal form: replacements are spliced
    /// in without being rewritten again.
    pub fn rewrite_under<Sigma: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &Sigma) -> DataExpression {
        self.metrics.reset();

        debug_trace!("input: {}", t);

        let mut context = RewriteContext {
            stack: &mut self.stack,
            builder: &mut self.builder,
            metrics: &mut self.metrics,
            automaton: &self.automaton,
            term_stacks: &self.term_stacks,
            state: &mut self.state,
            scheduler: &self.scheduler,
            condition_cache: &mut self.condition_cache,
            enumerator: &self.enumerator,
            binders: &mut self.binders,
        };
        rewrite_aux(&mut context, t, sigma)
    }
}

/// The term stacks of a [RewriteMachine], which [TermStackId]s refer to: the right-hand sides of
/// the rules, and whatever else its scheduler compiles.
pub(crate) struct TermStacks<H> {
    stacks: Vec<TermStack<H>>,
}

impl<H> Default for TermStacks<H> {
    fn default() -> Self {
        TermStacks { stacks: Vec::new() }
    }
}

/// The index of a term stack in [TermStacks]. Only [TermStacks] makes one, so it refers to a term
/// stack of the machine that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TermStackId(u32);

impl<H> TermStacks<H> {
    /// Adds `term_stack`, the right-hand side of `rule`, and returns the annotation of the rule.
    pub fn annotate(&mut self, rule: &Rule, term_stack: TermStack<H>) -> Annotation {
        let right_hand_side = self.reserve();
        self.set(right_hand_side, term_stack);
        Annotation {
            equivalence_classes: derive_equivalence_classes(rule),
            conditions: extend_conditions(rule),
            right_hand_side,
        }
    }

    /// Returns the id of a new term stack, which [TermStacks::set] fills in, so that a term stack
    /// can refer to one that is compiled after it.
    pub fn reserve(&mut self) -> TermStackId {
        self.stacks.push(TermStack::default());
        TermStackId(u32::try_from(self.stacks.len() - 1).expect("fewer than 2^32 term stacks"))
    }

    /// Sets the term stack of a reserved `id`.
    pub fn set(&mut self, id: TermStackId, term_stack: TermStack<H>) {
        self.stacks[id.0 as usize] = term_stack;
    }

    /// Returns the term stack that `id` refers to.
    pub fn get(&self, id: TermStackId) -> &TermStack<H> {
        &self.stacks[id.0 as usize]
    }
}

/// What the machine needs to apply a rule.
pub(crate) struct Annotation {
    /// Positions in the pattern with the same variable, for non-linear patterns
    pub equivalence_classes: Vec<EquivalenceClass>,
    pub conditions: Vec<EMACondition>,
    /// The right-hand side.
    pub right_hand_side: TermStackId,
}

/// The [RewriteMachine] during one step, with the guards on its stacks held. This is all that a
/// [Scheduler] can reach.
///
/// The fields are public so that they can be borrowed separately, for example to schedule a term
/// that is read from `state`.
pub(crate) struct MachineView<'c, 't, 'a, S: Scheduler> {
    pub stacks: Stacks<'c, 't, S>,
    pub term_stacks: &'a TermStacks<S::Hook>,
    pub state: &'a mut S::State,
    pub metrics: &'a mut RewriteMetrics,
}

impl<S: Scheduler> MachineView<'_, '_, '_, S> {
    /// Walks `automaton` from its start for `symbol` applied to the `arity` terms from slot
    /// `base` on, see [Walk].
    pub fn walk<'w>(
        &mut self,
        automaton: &'w SetAutomaton<Annotation>,
        symbol: &DataFunctionSymbol,
        base: usize,
        arity: usize,
    ) -> Walk<'w> {
        let subject = Subject::Arguments(symbol.copy(), self.stacks.arguments(base, arity));
        walk(automaton, &subject, self.metrics, Cursor::START)
    }
}

/// The state that rewriting a term needs, borrowed from the [RewriteMachine].
///
/// The fields are borrowed separately, rather than through `&mut RewriteMachine`, so that a nested
/// rewrite (a condition, or a quantifier's candidate body) can reuse the same stack while the
/// automaton is borrowed. It is also the [RewriteEngine] through which a binder is rewritten
/// (see [rewrite_binder]), with the same stack, automaton and enumerator.
///
/// A nested quantifier is therefore first tried while rewriting the outer body, with the outer
/// variables still free. If that does not settle it, it remains in the outer body, and is tried
/// again each time the outer search rewrites that body under a candidate value, on a frame of
/// its own.
struct RewriteContext<'a, E: Enumerator, S: Scheduler> {
    stack: &'a mut InnermostStack<S>,
    builder: &'a mut TermStackBuilder,
    metrics: &'a mut RewriteMetrics,
    automaton: &'a SetAutomaton<Annotation>,
    term_stacks: &'a TermStacks<S::Hook>,
    state: &'a mut S::State,
    scheduler: &'a S,
    condition_cache: &'a mut ConditionCache,
    enumerator: &'a E,
    binders: &'a mut BinderState<E::Frame>,
}

impl<E: Enumerator, S: Scheduler> RewriteContext<'_, E, S> {
    /// Calls `f` with the guards on both stacks held.
    fn with_machine<F, R>(&mut self, f: F) -> R
    where
        F: for<'c, 't, 'm> FnOnce(&mut MachineView<'c, 't, 'm, S>) -> R,
    {
        let RewriteContext {
            stack,
            term_stacks,
            state,
            metrics,
            ..
        } = self;
        stack.configs.write(|configs| {
            stack.terms.write(|terms| {
                f(&mut MachineView {
                    stacks: Stacks { configs, terms },
                    term_stacks: *term_stacks,
                    state: &mut **state,
                    metrics,
                })
            })
        })
    }
}

/// Rewrites `input_term` to normal form, replacing its free variables by their image under
/// `sigma`.
///
/// Uses a stack of terms and configurations to avoid recursion and to keep track of terms in
/// normal form without explicit tagging. Every configuration leaves its result on top of the
/// term stack:
///     - Rewrite { term }: Pushes the normal form of `term`, by rewriting its arguments from
///       left to right and then matching the redex, unless the scheduler takes it over.
///     - Construct { symbol, arity }: Matches `symbol` applied to the `arity` terms on top, and
///       replaces them by its normal form.
///     - Evaluate: Evaluates the term stack of a right-hand side in place, keeping the index of
///       its next instruction; see [Evaluate].
///     - Resume: Continues the work of the scheduler.
///     - Return: Pops the normal form of the input term.
///
/// Free variables of `input_term` are replaced by their image under `sigma` when they are
/// reached, which is the point where the substitution is consumed: every term that is
/// matched, constructed or used to instantiate a condition afterwards is already
/// substituted.
///
/// A binder is rewritten by [rewrite_binder] under a substitution for its free variables, and
/// which one depends on where it comes from:
///     - A binder of the input term is reached through `Rewrite`, while `sigma` is not yet
///       consumed for its free variables, so its body is rewritten under `sigma`.
///     - A binder of a right-hand side is reached by its term stack. Its free variables are
///       variables of the rule, and their values are the normal forms of the matched subterms,
///       which are rewritten like any other argument first. These are already substituted, so
///       its body is rewritten under the values instead of `sigma`.
///
/// See [binders in right-hand sides](https://mercorg.github.io/merc-website/developer/rewriting/quantifiers/#binders-in-right-hand-sides).
fn rewrite_aux<Sigma: RewriteSubstitution, E: Enumerator, S: Scheduler>(
    context: &mut RewriteContext<'_, E, S>,
    input_term: &DataExpression,
    sigma: &Sigma,
) -> DataExpression {
    context.metrics.recursion();

    // Every binder of input_term is rewritten under sigma, so its range is computed once, when
    // the first one is reached.
    let sigma_range = OnceCell::new();

    context.with_machine(|machine| {
        machine.stacks.configs.push(Config::Return);
        machine.stacks.schedule_rewrite(&input_term.copy());
    });

    loop {
        debug_trace!("{}", context.stack);

        /// The number of steps that are taken without leaving the guards on the stacks.
        const LOCAL_STEPS: usize = 256;

        let scheduler = context.scheduler;
        let automaton = context.automaton;
        let term_stacks = context.term_stacks;
        let step = context.with_machine(|machine| {
            // The steps that only change the stacks are taken without leaving the guards. The
            // guards postpone garbage collection, so they are left after a bounded number.
            for _ in 0..LOCAL_STEPS {
                let step = 'step: {
                    match machine.stacks.configs.last() {
                        // A construct stays on the stack while its redex is matched, which keeps
                        // its symbol protected; its arguments are already the terms on top.
                        Some(Config::Construct { arity, .. }) => {
                            break 'step Step::Match {
                                head: Head::Construct,
                                arity: *arity,
                            };
                        }
                        Some(Config::Evaluate(_)) => {
                            break 'step evaluate_term_stack(machine, scheduler, term_stacks);
                        }
                        _ => {}
                    }

                    match machine
                        .stacks
                        .configs
                        .pop()
                        .expect("The stack always ends with a Return configuration")
                    {
                        Config::Rewrite { term } => decompose(machine, scheduler, &term, sigma),
                        Config::Resume(continuation) => Step::Resume(continuation),
                        Config::Return => Step::Return(
                            machine
                                .stacks
                                .terms
                                .pop()
                                .expect("The result should be the last element on the stack")
                                .expect("The result should be Some")
                                .protect(),
                        ),
                        Config::Construct { .. } | Config::Evaluate(_) => {
                            unreachable!("these configurations are handled before they are taken from the stack")
                        }
                    }
                };

                // A step leaves what it protects on the stacks, but a later step may take it off.
                machine.stacks.checkpoint();

                match step {
                    Step::Continue => {}
                    Step::Match { head, arity } => match walk_redex(machine, automaton, head, arity) {
                        Walk::Done(found) => reduce(machine, scheduler, head, arity, found),
                        // Checking conditions rewrites, which needs the stacks.
                        walk => {
                            return Step::Conditions {
                                automaton,
                                head,
                                arity,
                                walk,
                                then: Then::Reduce,
                            };
                        }
                    },
                    Step::Resume(continuation) => match scheduler.resume(machine, continuation) {
                        Resumed::Done => {}
                        Resumed::Conditions {
                            automaton,
                            head,
                            arity,
                            walk,
                            continuation,
                        } => {
                            return Step::Conditions {
                                automaton,
                                head,
                                arity,
                                walk,
                                then: Then::Resume(continuation),
                            };
                        }
                    },
                    Step::Hook { hook, scope } => scheduler.hook(machine, hook, scope),
                    step => return step,
                }
                machine.stacks.checkpoint();
            }
            Step::Continue
        });

        match step {
            Step::Continue => {}
            Step::Match { .. } | Step::Resume(_) | Step::Hook { .. } => {
                unreachable!("these are handled within the guards on the stacks")
            }
            Step::Conditions {
                automaton,
                head,
                arity,
                walk,
                then,
            } => {
                let found = find_match(context, automaton, head, arity, walk);
                context.with_machine(|machine| match then {
                    Then::Reduce => reduce(machine, scheduler, head, arity, found),
                    Then::Resume(continuation) => scheduler.matched(machine, continuation, found),
                });
            }
            Step::SubjectBinder(abstraction) => {
                let range = sigma_range.get_or_init(|| sigma.range());
                let value = rewrite_binder(context, &abstraction, sigma, range);
                context.with_machine(|machine| machine.stacks.push(&value.copy()));
            }
            Step::RhsBinder { template, arity } => {
                let values: Vec<DataExpression> = context.with_machine(|machine| {
                    let first = machine.stacks.len() - arity;
                    let values = machine
                        .stacks
                        .arguments(first, arity)
                        .iter()
                        .flatten()
                        .map(|value| value.protect())
                        .collect();
                    machine.stacks.truncate(first);
                    values
                });

                // The lambda only records which variables the values are for.
                let variables: Vec<DataVariable> = template.variables().into_iter().collect();
                let abstraction: DataAbstraction = template.body().protect().into();

                let values = SliceSubstitution::new(&variables, &values);
                let value = rewrite_binder(context, &abstraction, &values, &values.range());
                context.with_machine(|machine| machine.stacks.push(&value.copy()));
            }
            Step::Return(result) => return result,
        }
    }
}

/// Takes `term` apart, to push its normal form.
fn decompose<'p, S: Scheduler, Sigma: RewriteSubstitution>(
    machine: &mut MachineView<'_, '_, '_, S>,
    scheduler: &S,
    term: &DataExpressionRef<'_>,
    sigma: &Sigma,
) -> Step<'p, S> {
    match term.kind() {
        // A machine number is a value in normal form, without a head symbol to match.
        DataExpressionKind::MachineNumber(_) => machine.stacks.push(term),
        // A binder has no head function symbol either, and is not addressed by the set automaton.
        DataExpressionKind::Abstraction(binder) => return Step::SubjectBinder(binder.protect()),
        DataExpressionKind::Application(_) | DataExpressionKind::FunctionSymbol(_) => {
            if scheduler.decompose(machine, term) {
                return Step::Continue;
            }

            let arity = term.data_arguments().len();
            machine.stacks.schedule_construct(&term.data_function_symbol(), arity);
            // The last configuration runs first, so the arguments are scheduled from right to
            // left to be rewritten from left to right.
            for index in (0..arity).rev() {
                machine.stacks.schedule_rewrite(&term.data_arg(index));
            }
        }
        // A variable has no head symbol to match on. It is replaced by its image under sigma,
        // which is assumed to be in normal form. Variables outside the domain of sigma are their
        // own normal form.
        DataExpressionKind::Variable(variable) => match sigma.get(&variable) {
            Some(replacement) => machine.stacks.push(&replacement.copy()),
            None => machine.stacks.push(term),
        },
        DataExpressionKind::WhereClause(_) => {
            unimplemented!("rewriting a where clause: {term}")
        }
    }
    Step::Continue
}

/// Evaluates the term stack of the [Config::Evaluate] on top of the stack until it reaches an
/// instruction that needs more than the stacks, or its end.
fn evaluate_term_stack<'p, S: Scheduler>(
    machine: &mut MachineView<'_, '_, '_, S>,
    scheduler: &S,
    term_stacks: &'p TermStacks<S::Hook>,
) -> Step<'p, S> {
    let Some(Config::Evaluate(top)) = machine.stacks.configs.last() else {
        unreachable!("the configuration on top is an evaluation");
    };
    let Evaluate {
        term_stack,
        mut pc,
        base,
        scope,
    } = *top;
    let instructions = &term_stacks.get(term_stack).instructions;

    let suspended = loop {
        let Some(instruction) = instructions.get(pc as usize) else {
            break None;
        };
        pc += 1;
        match instruction {
            Instruction::Variable(position) => scheduler.variable(machine, scope, base, position),
            Instruction::Term(term) => machine.stacks.push(&term.copy()),
            Instruction::Apply { symbol, arity } => {
                break Some(Step::Match {
                    head: Head::Symbol(symbol),
                    arity: *arity,
                });
            }
            Instruction::Binder { template, arity } => {
                break Some(Step::RhsBinder {
                    template,
                    arity: *arity,
                });
            }
            Instruction::Hook(hook) => break Some(Step::Hook { hook, scope }),
        }
    };

    if let Some(step) = suspended {
        let Some(Config::Evaluate(top)) = machine.stacks.configs.last_mut() else {
            unreachable!("the configuration on top is an evaluation");
        };
        top.pc = pc;
        return step;
    }

    // The value of the term stack replaces the arguments of the matched term, if they are still
    // there.
    machine.stacks.configs.pop();
    let value = machine
        .stacks
        .terms
        .pop()
        .expect("a term stack has a value")
        .expect("the value of a term stack is present");
    machine.stacks.terms.truncate(base);
    machine.stacks.terms.push(Some(value));
    scheduler.leave(machine, scope);
    Step::Continue
}

/// The head symbol of the redex on top of the stack, which is applied to the terms on top.
#[derive(Clone, Copy)]
pub(crate) enum Head<'a> {
    /// The symbol of the [Config::Construct] on top of the configurations.
    Construct,
    /// The symbol of an application in a term stack that is being evaluated, or of a scheduler.
    Symbol(&'a DataFunctionSymbol),
}

/// Replaces the redex on top of the stack by the native value or the right-hand side of the rule
/// that `found` is, or by its normal form if it is `None`.
fn reduce<S: Scheduler>(
    machine: &mut MachineView<'_, '_, '_, S>,
    scheduler: &S,
    head: Head<'_>,
    arity: usize,
    found: Option<MatchResult<'_, Annotation>>,
) {
    let symbol = match head {
        Head::Construct => {
            let Some(Config::Construct { symbol, .. }) = machine.stacks.configs.pop() else {
                unreachable!("the redex is that of the construct on top of the stack");
            };
            symbol
        }
        Head::Symbol(symbol) => symbol.copy(),
    };

    let first = machine.stacks.len() - arity;
    match found {
        Some(MatchResult::Native(result)) => {
            debug_trace!("native rewrite {} => {}", symbol, result);
            machine.metrics.native_applied(&symbol);
            machine.stacks.replace_from(first, &result.copy());
        }
        Some(MatchResult::Rule(announcement, annotation)) => {
            debug_trace!("rewrite {} using rule {}", symbol, announcement.rule);
            machine.metrics.rule_applied(&announcement.rule);
            let scope = scheduler.enter(machine, annotation.right_hand_side, arity);
            machine
                .stacks
                .schedule_evaluate(annotation.right_hand_side, first, scope);
        }
        None => {
            // No rule applies, so the redex is a normal form, which is only constructed now.
            let term: DataExpression = if arity == 0 {
                symbol.protect().into()
            } else {
                DataApplication::with_iter(&symbol, arity, machine.stacks.arguments(first, arity).iter().flatten())
                    .into()
            };
            machine.stacks.replace_from(first, &term.copy());
        }
    }
}

/// Walks the automaton for the redex on top of the stack, within the guards on the stacks.
fn walk_redex<'a, S: Scheduler>(
    machine: &mut MachineView<'_, '_, '_, S>,
    automaton: &'a SetAutomaton<Annotation>,
    head: Head<'_>,
    arity: usize,
) -> Walk<'a> {
    let symbol = match head {
        Head::Construct => match machine.stacks.configs.last() {
            Some(Config::Construct { symbol, .. }) => symbol.copy(),
            _ => unreachable!("the redex belongs to the construct on top of the stack"),
        },
        Head::Symbol(symbol) => symbol.copy(),
    };
    let first = machine.stacks.len() - arity;
    walk(
        automaton,
        &Subject::Arguments(symbol, machine.stacks.arguments(first, arity)),
        machine.metrics,
        Cursor::START,
    )
}

impl<E: Enumerator, S: Scheduler> RewriteEngine for RewriteContext<'_, E, S> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.rewrite_with(t, &EmptySubstitution)
    }

    fn rewrite_with<Sigma: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &Sigma) -> DataExpression {
        rewrite_aux(self, t, sigma)
    }
}

impl<E: Enumerator, S: Scheduler> BinderRewriter for RewriteContext<'_, E, S> {
    fn decide(
        &mut self,
        binder: BinderType,
        variables: &[DataVariable],
        body: &DataExpression,
    ) -> Option<DataExpression> {
        // The frames of the enclosing searches are taken off the stack while they
        // run, so the one on top is free to reuse.
        let enumerator = self.enumerator;
        let mut frame = self.binders.frames.pop().unwrap_or_default();
        let result = enumerator.decide(&mut frame, self, binder, variables, body);
        self.binders.frames.push(frame);
        result
    }

    fn fresh_variable(&mut self, variable: &DataVariable) -> DataVariable {
        self.binders.fresh_variable(variable)
    }
}

/// Uses the automaton to find a match for the redex on top of the stack: either a rewrite rule,
/// or — when its head symbol is a machine-word operation — the natively-evaluated result.
///
/// Continues from `walk`, where the walk for the redex on top stopped.
///
/// The automaton is passed separately from the context, so that the match it returns does not
/// borrow the context.
fn find_match<'a, E: Enumerator, S: Scheduler>(
    context: &mut RewriteContext<'_, E, S>,
    automaton: &'a SetAutomaton<Annotation>,
    head: Head<'_>,
    arity: usize,
    mut walk: Walk<'a>,
) -> Option<MatchResult<'a, Annotation>> {
    // The redex is read from the term stack, which checking conditions rewrites on. The walk
    // therefore stops at a rule with conditions, which are checked outside of it, and continues
    // after that rule when they do not hold.
    loop {
        match walk {
            Walk::Done(result) => return result,
            Walk::Conditional {
                announcement,
                annotation,
                next,
            } => {
                if check_conditions(context, &annotation.conditions, head, arity) {
                    return Some(MatchResult::Rule(announcement, annotation));
                }
                let metrics = &mut *context.metrics;
                walk = with_redex(context.stack, head, arity, |subject| {
                    self::walk(automaton, subject, metrics, next)
                });
            }
        }
    }
}

/// Where [walk] continues: at a state, from one of the announcements of its transition.
#[derive(Clone, Copy)]
pub(crate) struct Cursor {
    state: usize,
    announcement: usize,
}

impl Cursor {
    /// The start of the walk.
    const START: Cursor = Cursor {
        state: 0,
        announcement: 0,
    };
}

/// Where [walk] stops.
pub(crate) enum Walk<'a> {
    /// The redex matches this rule, or is natively evaluated, or nothing matches.
    Done(Option<MatchResult<'a, Annotation>>),
    /// The redex matches this rule if its conditions hold; otherwise the walk continues at `next`.
    Conditional {
        announcement: &'a MatchAnnouncement,
        annotation: &'a Annotation,
        next: Cursor,
    },
}

/// Walks the automaton for `subject` from `cursor`, up to the first rule that matches it
/// without conditions, or the first rule with conditions.
fn walk<'a>(
    automaton: &'a SetAutomaton<Annotation>,
    subject: &Subject<'_>,
    metrics: &mut RewriteMetrics,
    mut cursor: Cursor,
) -> Walk<'a> {
    loop {
        let state = &automaton.states()[cursor.state];

        // A state that the walk continues at has been compared already.
        if cursor.announcement == 0 {
            metrics.symbol_compared();
        }

        // A variable at the position of the label matches no pattern.
        let Some(transition) = subject
            .operation_at(state.label())
            .and_then(|operation| automaton.get_transition(cursor.state, operation))
        else {
            return Walk::Done(None);
        };

        // The very first transition observes the redex's own head symbol (state 0's label is
        // always the root position ε). That is the only point at which `native` refers to
        // the redex as a whole, rather than to some other subterm the automaton happens to
        // inspect while narrowing down candidate rules.
        if cursor.state == 0
            && let Some(op) = transition.native
        {
            return Walk::Done(op.evaluate(subject.arguments()).map(MatchResult::Native));
        }

        for (index, (announcement, annotation)) in transition.announcements.iter().enumerate().skip(cursor.announcement)
        {
            if check_equivalence_classes(subject, &annotation.equivalence_classes) {
                if annotation.conditions.is_empty() {
                    return Walk::Done(Some(MatchResult::Rule(announcement, annotation)));
                }
                return Walk::Conditional {
                    announcement,
                    annotation,
                    next: Cursor {
                        state: cursor.state,
                        announcement: index + 1,
                    },
                };
            }
        }

        // Without a destination state there is no match.
        let Some((_, destination)) = transition.destinations.first() else {
            return Walk::Done(None);
        };
        cursor = Cursor {
            state: *destination,
            announcement: 0,
        };
    }
}

/// Calls `f` with the redex on top of the stack.
fn with_redex<S: Scheduler, R, F>(stack: &InnermostStack<S>, head: Head<'_>, arity: usize, f: F) -> R
where
    F: FnOnce(&Subject<'_>) -> R,
{
    let read_terms = stack.terms.read();
    let arguments = &read_terms[read_terms.len() - arity..];
    match head {
        Head::Construct => {
            let read_configs = stack.configs.read();
            let Some(Config::Construct { symbol, .. }) = read_configs.last() else {
                unreachable!("the redex belongs to the construct on top of the stack");
            };
            f(&Subject::Arguments(symbol.copy(), arguments))
        }
        Head::Symbol(symbol) => f(&Subject::Arguments(symbol.copy(), arguments)),
    }
}

/// Checks whether the condition holds for given match announcement.
///
/// The condition sides are built from the arguments of the redex, which are already substituted
/// normal forms, so the nested normalisations must not apply the substitution a second time.
fn check_conditions<E: Enumerator, S: Scheduler>(
    context: &mut RewriteContext<'_, E, S>,
    conditions: &[EMACondition],
    head: Head<'_>,
    arity: usize,
) -> bool {
    for c in conditions {
        let builder = &mut *context.builder;
        let (lhs, rhs) = with_redex(context.stack, head, arity, |subject| {
            (
                c.lhs_term_stack.instantiate(subject, builder),
                c.rhs_term_stack.instantiate(subject, builder),
            )
        });

        // Syntactically equal sides have the same normal form, so neither needs normalising.
        if lhs == rhs {
            if c.equality {
                continue;
            }
            return false;
        }

        let rhs_normal = normalise_condition_side(context, &rhs);
        let lhs_normal = normalise_condition_side(context, &lhs);

        if (lhs_normal != rhs_normal && c.equality) || (lhs_normal == rhs_normal && !c.equality) {
            return false;
        }
    }

    true
}

/// Normalises `term` as a condition side, returning `condition_cache`'s
/// stored normal form for it if present, and storing the result there
/// for reuse otherwise.
fn normalise_condition_side<E: Enumerator, S: Scheduler>(
    context: &mut RewriteContext<'_, E, S>,
    term: &DataExpression,
) -> DataExpression {
    if let Some(normal_form) = context.condition_cache.get(term) {
        context.metrics.condition_cache_hit();
        return normal_form;
    }

    let normal_form = rewrite_aux(context, term, &EmptySubstitution);
    context.condition_cache.insert(term.clone(), normal_form.clone());
    normal_form
}
