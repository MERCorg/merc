use std::cell::OnceCell;
use std::cell::RefCell;

use merc_aterm::ProtectedWriteGuard;
use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbol;
use merc_data::DataVariable;

use crate::Enumerator;
use crate::NoQuantifierEnumerator;
use crate::RewriteEngine;
use crate::RewriteMetrics;
use crate::RewriteSpecification;
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
use crate::subject::subterm;
use crate::utilities::Config;
use crate::utilities::EmptySubstitution;
use crate::utilities::Evaluate;
use crate::utilities::InnermostStack;
use crate::utilities::Instruction;
use crate::utilities::RewriteSubstitution;
use crate::utilities::SliceSubstitution;
use crate::utilities::TermStack;
use crate::utilities::TermStackBuilder;
use merc_utilities::debug_trace;

impl<E: Enumerator> RewriteEngine for InnermostRewriter<E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.machine.rewrite_under(t, &EmptySubstitution)
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        self.machine.rewrite_under(t, sigma)
    }
}

/// What [rewrite_aux] does next, after the configuration on top has been looked at.
enum Step<'p> {
    Continue,

    /// Match the redex on top of the stack, which applies `head` to `arity` arguments.
    Match {
        head: Head<'p>,
        arity: usize,
    },

    /// Check the conditions of the rule at which `walk` stopped for the redex on top, see
    /// [find_match].
    Conditions {
        head: Head<'p>,
        arity: usize,
        walk: Walk<'p>,
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

impl InnermostRewriter<NoQuantifierEnumerator> {
    /// Creates a new InnermostRewriter from the given rewrite specification.
    ///
    /// Built without an [Enumerator]: any `exists`/`forall` quantifier the
    /// rewriter reaches is left undecided, over its rewritten body. Use
    /// [InnermostRewriter::with_enumerator] to have quantifiers decided.
    pub fn new(spec: &RewriteSpecification) -> Self {
        Self::with_condition_cache_capacity(spec, Some(crate::matching::condition_cache::DEFAULT_MAX_ENTRIES))
    }

    /// Like [InnermostRewriter::new], but `max_entries` sets the condition
    /// cache's capacity instead of the default, and `None` disables it (see
    /// `ConditionCache::new`).
    pub fn with_condition_cache_capacity(spec: &RewriteSpecification, max_entries: Option<usize>) -> Self {
        Self::build(spec, max_entries, NoQuantifierEnumerator)
    }
}

impl<E: Enumerator> InnermostRewriter<E> {
    fn build(spec: &RewriteSpecification, max_entries: Option<usize>, enumerator: E) -> Self {
        // The automaton annotates the rules through a shared reference.
        let term_stacks = RefCell::new(TermStacks::default());
        let automaton = SetAutomaton::with_binders(spec, |rule| {
            term_stacks.borrow_mut().annotate(rule, TermStack::from_rule(rule))
        });

        InnermostRewriter {
            machine: RewriteMachine::new(automaton, term_stacks.into_inner(), max_entries, enumerator),
        }
    }

    /// Like [InnermostRewriter::new], but `enumerator` is fed to the rewriter
    /// to decide `exists`/`forall` quantifiers it reaches.
    pub fn with_enumerator(spec: &RewriteSpecification, enumerator: E) -> Self {
        Self::with_enumerator_and_condition_cache_capacity(
            spec,
            enumerator,
            Some(crate::matching::condition_cache::DEFAULT_MAX_ENTRIES),
        )
    }

    /// Like [InnermostRewriter::with_enumerator], but `max_entries` sets the
    /// condition cache's capacity as
    /// [InnermostRewriter::with_condition_cache_capacity] does.
    pub fn with_enumerator_and_condition_cache_capacity(
        spec: &RewriteSpecification,
        enumerator: E,
        max_entries: Option<usize>,
    ) -> Self {
        Self::build(spec, max_entries, enumerator)
    }

    /// Returns the metrics of the last rewrite, see [RewriteMetrics].
    pub fn metrics(&self) -> &RewriteMetrics {
        self.machine.metrics()
    }
}

/// The rewriting machine of the [InnermostRewriter]: the automaton of the rules, and the stacks
/// and caches that rewriting reuses.
///
/// The right-hand sides of the rules are term stacks (see [TermStack]), which the machine evaluates in
/// place.
pub(crate) struct RewriteMachine<E: Enumerator> {
    automaton: SetAutomaton<Annotation>,
    term_stacks: TermStacks,
    stack: InnermostStack,
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

impl<E: Enumerator> RewriteMachine<E> {
    fn new(
        automaton: SetAutomaton<Annotation>,
        term_stacks: TermStacks,
        max_entries: Option<usize>,
        enumerator: E,
    ) -> Self {
        RewriteMachine {
            automaton,
            term_stacks,
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
    pub fn rewrite_under<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        self.metrics.reset();

        debug_trace!("input: {}", t);

        let mut context = RewriteContext {
            stack: &mut self.stack,
            builder: &mut self.builder,
            metrics: &mut self.metrics,
            automaton: &self.automaton,
            term_stacks: &self.term_stacks,
            condition_cache: &mut self.condition_cache,
            enumerator: &self.enumerator,
            binders: &mut self.binders,
        };
        rewrite_aux(&mut context, t, sigma)
    }
}

/// The right-hand sides of the rules of a [RewriteMachine], which [TermStackId]s refer to.
#[derive(Default)]
struct TermStacks {
    stacks: Vec<TermStack>,
}

/// The index of a right-hand side in [TermStacks]. Only [TermStacks::annotate] makes one, so
/// it refers to a right-hand side of the machine whose automaton holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TermStackId(u32);

impl TermStacks {
    /// Adds `term_stack`, the right-hand side of `rule`, and returns the annotation of the rule.
    fn annotate(&mut self, rule: &Rule, term_stack: TermStack) -> Annotation {
        self.stacks.push(term_stack);
        Annotation {
            equivalence_classes: derive_equivalence_classes(rule),
            conditions: extend_conditions(rule),
            right_hand_side: TermStackId(u32::try_from(self.stacks.len() - 1).expect("fewer than 2^32 term stacks")),
        }
    }

    /// Returns the right-hand side that `id` refers to.
    fn get(&self, id: TermStackId) -> &TermStack {
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
struct RewriteContext<'a, E: Enumerator> {
    stack: &'a mut InnermostStack,
    builder: &'a mut TermStackBuilder,
    metrics: &'a mut RewriteMetrics,
    automaton: &'a SetAutomaton<Annotation>,
    term_stacks: &'a TermStacks,
    condition_cache: &'a mut ConditionCache,
    enumerator: &'a E,
    binders: &'a mut BinderState<E::Frame>,
}

/// Rewrites `input_term` to normal form, replacing its free variables by their image under
/// `sigma`.
///
/// Uses a stack of terms and configurations to avoid recursion and to keep track of terms in
/// normal form without explicit tagging. Every configuration leaves its result on top of the
/// term stack:
///     - Rewrite { term }: Pushes the normal form of `term`, by rewriting its arguments from
///       left to right and then matching the redex.
///     - Construct { symbol, arity }: Matches `symbol` applied to the `arity` terms on top, and
///       replaces them by its normal form.
///     - Evaluate: Evaluates the term stack of a right-hand side in place, keeping the index of
///       its next instruction; see [Evaluate].
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
fn rewrite_aux<S: RewriteSubstitution, E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
    input_term: &DataExpression,
    sigma: &S,
) -> DataExpression {
    context.metrics.recursion();

    // Every binder of input_term is rewritten under sigma, so its range is computed once, when
    // the first one is reached.
    let sigma_range = OnceCell::new();

    context.stack.configs.write(|write_configs| {
        write_configs.push(Config::Return);
        InnermostStack::add_rewrite(write_configs, &input_term.copy());
    });

    loop {
        debug_trace!("{}", context.stack);

        /// The number of steps that are taken without leaving the guards on the stacks.
        const LOCAL_STEPS: usize = 256;

        let automaton = context.automaton;
        let term_stacks = context.term_stacks;
        let metrics = &mut *context.metrics;
        let stack = &mut *context.stack;
        let step = stack.configs.write(|write_configs| {
            stack.terms.write(|write_terms| {
                // The steps that only change the stacks are taken without leaving the guards. The
                // guards postpone garbage collection, so they are left after a bounded number.
                for _ in 0..LOCAL_STEPS {
                    let step = 'step: {
                        match write_configs.last() {
                            // A construct stays on the stack while its redex is matched, which keeps
                            // its symbol protected; its arguments are already the terms on top.
                            Some(Config::Construct { arity, .. }) => {
                                break 'step Step::Match {
                                    head: Head::Construct,
                                    arity: *arity,
                                };
                            }
                            Some(Config::Evaluate(_)) => {
                                break 'step evaluate_term_stack(write_configs, write_terms, term_stacks);
                            }
                            _ => {}
                        }

                        match write_configs
                            .pop()
                            .expect("The stack always ends with a Return configuration")
                        {
                            Config::Rewrite { term } => decompose(write_configs, write_terms, &term, sigma),
                            Config::Return => Step::Return(
                                write_terms
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
                    write_configs.checkpoint();
                    write_terms.checkpoint();

                    match step {
                        Step::Continue => {}
                        Step::Match { head, arity } => {
                            match walk_redex(write_configs, write_terms, automaton, metrics, head, arity) {
                                Walk::Done(found) => {
                                    reduce(write_configs, write_terms, term_stacks, metrics, head, arity, found)
                                }
                                // Checking conditions rewrites, which needs the stacks.
                                walk => return Step::Conditions { head, arity, walk },
                            }
                            write_configs.checkpoint();
                            write_terms.checkpoint();
                        }
                        step => return step,
                    }
                }
                Step::Continue
            })
        });

        match step {
            Step::Continue => {}
            Step::Match { .. } => unreachable!("a redex is matched within the guards on the stacks"),
            Step::Conditions { head, arity, walk } => {
                let automaton = context.automaton;
                let found = find_match(context, automaton, head, arity, walk);
                let RewriteContext {
                    stack,
                    metrics,
                    term_stacks,
                    ..
                } = context;
                stack.configs.write(|write_configs| {
                    stack.terms.write(|write_terms| {
                        reduce(write_configs, write_terms, term_stacks, metrics, head, arity, found)
                    })
                });
            }
            Step::SubjectBinder(abstraction) => {
                let range = sigma_range.get_or_init(|| sigma.range());
                let value = rewrite_binder(context, &abstraction, sigma, range);
                context
                    .stack
                    .terms
                    .write(|write_terms| InnermostStack::push_value(write_terms, &value.copy()));
            }
            Step::RhsBinder { template, arity } => {
                let values: Vec<DataExpression> = context.stack.terms.write(|write_terms| {
                    let first = write_terms.len() - arity;
                    let values = write_terms[first..]
                        .iter()
                        .flatten()
                        .map(|value| value.protect())
                        .collect();
                    write_terms.truncate(first);
                    values
                });

                // The lambda only records which variables the values are for.
                let variables: Vec<DataVariable> = template.variables().into_iter().collect();
                let abstraction: DataAbstraction = template.body().protect().into();

                let values = SliceSubstitution::new(&variables, &values);
                let value = rewrite_binder(context, &abstraction, &values, &values.range());
                context
                    .stack
                    .terms
                    .write(|write_terms| InnermostStack::push_value(write_terms, &value.copy()));
            }
            Step::Return(result) => return result,
        }
    }
}

/// Takes `term` apart, to push its normal form.
fn decompose<'p, S: RewriteSubstitution>(
    write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
    write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
    term: &DataExpressionRef<'_>,
    sigma: &S,
) -> Step<'p> {
    match term.kind() {
        // A machine number is a value in normal form, without a head symbol to match.
        DataExpressionKind::MachineNumber(_) => InnermostStack::push_value(write_terms, term),
        // A binder has no head function symbol either, and is not addressed by the set automaton.
        DataExpressionKind::Abstraction(binder) => return Step::SubjectBinder(binder.protect()),
        DataExpressionKind::Application(_) | DataExpressionKind::FunctionSymbol(_) => {
            let arity = term.data_arguments().len();
            InnermostStack::add_construct(write_configs, &term.data_function_symbol(), arity);
            // The last configuration runs first, so the arguments are pushed from right to left
            // to be rewritten from left to right.
            for index in (0..arity).rev() {
                InnermostStack::add_rewrite(write_configs, &term.data_arg(index));
            }
        }
        // A variable has no head symbol to match on. It is replaced by its image under sigma,
        // which is assumed to be in normal form. Variables outside the domain of sigma are their
        // own normal form.
        DataExpressionKind::Variable(variable) => match sigma.get(&variable) {
            Some(replacement) => InnermostStack::push_value(write_terms, &replacement.copy()),
            None => InnermostStack::push_value(write_terms, term),
        },
        DataExpressionKind::WhereClause(_) => {
            unimplemented!("rewriting a where clause: {term}")
        }
    }
    Step::Continue
}

/// Evaluates the term stack of the [Config::Evaluate] on top of the stack until it reaches an
/// instruction that needs
/// more than the stacks, or its end.
fn evaluate_term_stack<'p>(
    write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
    write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
    term_stacks: &'p TermStacks,
) -> Step<'p> {
    let Some(Config::Evaluate(evaluate)) = write_configs.last_mut() else {
        unreachable!("the configuration on top is an evaluation");
    };
    let term_stack = term_stacks.get(evaluate.term_stack);

    while evaluate.pc < evaluate.end {
        let pc = evaluate.pc;
        evaluate.pc += 1;
        match &term_stack.instructions[pc as usize] {
            Instruction::Variable(position) => {
                // Safety: the value is stored in the container on the next line.
                let value = unsafe { write_terms.protect(&subterm(&write_terms[evaluate.base..], position)) };
                write_terms.push(Some(value.into()));
            }
            Instruction::Term(term) => InnermostStack::push_value(write_terms, &term.copy()),
            Instruction::Apply { symbol, arity } => {
                return Step::Match {
                    head: Head::Symbol(symbol),
                    arity: *arity,
                };
            }
            Instruction::Binder { template, arity } => {
                return Step::RhsBinder {
                    template,
                    arity: *arity,
                };
            }
            Instruction::Hook(hook) => match *hook {},
        }
    }

    // The value of the term stack replaces the arguments of the matched term.
    let base = evaluate.base;
    write_configs.pop();
    let value = write_terms
        .pop()
        .expect("a term stack has a value")
        .expect("the value of a term stack is present");
    write_terms.truncate(base);
    write_terms.push(Some(value));
    Step::Continue
}

/// The head symbol of the redex on top of the stack, which is applied to the terms on top.
#[derive(Clone, Copy)]
enum Head<'a> {
    /// The symbol of the [Config::Construct] on top of the configurations.
    Construct,
    /// The symbol of an application in a term stack that is being evaluated.
    Symbol(&'a DataFunctionSymbol),
}

/// Replaces the redex on top of the stack by the native value or the right-hand side of the rule
/// that `found` is, or by its normal form if it is `None`.
fn reduce(
    write_configs: &mut ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
    write_terms: &mut ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
    term_stacks: &TermStacks,
    metrics: &mut RewriteMetrics,
    head: Head<'_>,
    arity: usize,
    found: Option<MatchResult<'_, Annotation>>,
) {
    let symbol = match head {
        Head::Construct => {
            let Some(Config::Construct { symbol, .. }) = write_configs.pop() else {
                unreachable!("the redex is that of the construct on top of the stack");
            };
            symbol
        }
        Head::Symbol(symbol) => symbol.copy(),
    };

    let first = write_terms.len() - arity;
    match found {
        Some(MatchResult::Native(result)) => {
            debug_trace!("native rewrite {} => {}", symbol, result);
            metrics.native_applied(&symbol);
            InnermostStack::replace_values(write_terms, first, &result.copy());
        }
        Some(MatchResult::Rule(announcement, annotation)) => {
            debug_trace!("rewrite {} using rule {}", symbol, announcement.rule);
            metrics.rule_applied(&announcement.rule);

            // The right-hand side is evaluated on top of the arguments, which its variables refer
            // to, and replaces them by its value.
            let block = term_stacks.get(annotation.right_hand_side).blocks[0];
            write_configs.push(Config::Evaluate(Evaluate::new(
                annotation.right_hand_side,
                block,
                first,
            )));
        }
        None => {
            // No rule applies, so the redex is a normal form, which is only constructed now.
            let term: DataExpression = if arity == 0 {
                symbol.protect().into()
            } else {
                DataApplication::with_iter(&symbol, arity, write_terms[first..].iter().flatten()).into()
            };
            InnermostStack::replace_values(write_terms, first, &term.copy());
        }
    }
}

/// Walks the automaton for the redex on top of the stack, within the guards on the stacks.
fn walk_redex<'a>(
    write_configs: &ProtectedWriteGuard<'_, '_, Vec<Config<'_>>>,
    write_terms: &ProtectedWriteGuard<'_, '_, Vec<Option<DataExpressionRef<'_>>>>,
    automaton: &'a SetAutomaton<Annotation>,
    metrics: &mut RewriteMetrics,
    head: Head<'_>,
    arity: usize,
) -> Walk<'a> {
    let symbol = match head {
        Head::Construct => match write_configs.last() {
            Some(Config::Construct { symbol, .. }) => symbol.copy(),
            _ => unreachable!("the redex belongs to the construct on top of the stack"),
        },
        Head::Symbol(symbol) => symbol.copy(),
    };
    let arguments = &write_terms[write_terms.len() - arity..];
    walk(
        automaton,
        &Subject::Arguments(symbol, arguments),
        metrics,
        Cursor::START,
    )
}

impl<E: Enumerator> RewriteEngine for RewriteContext<'_, E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.rewrite_with(t, &EmptySubstitution)
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        rewrite_aux(self, t, sigma)
    }
}

impl<E: Enumerator> BinderRewriter for RewriteContext<'_, E> {
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
fn find_match<'a, E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
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
struct Cursor {
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
enum Walk<'a> {
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
fn with_redex<R, F>(stack: &InnermostStack, head: Head<'_>, arity: usize, f: F) -> R
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
fn check_conditions<E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
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
fn normalise_condition_side<E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
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

/// Innermost Adaptive Pattern Matching Automaton (APMA) rewrite engine.
pub struct InnermostRewriter<E: Enumerator = NoQuantifierEnumerator> {
    machine: RewriteMachine<E>,
}

pub struct AnnouncementInnermost {
    /// Positions in the pattern with the same variable, for non-linear patterns
    pub equivalence_classes: Vec<EquivalenceClass>,

    /// Conditions for the left hand side.
    pub conditions: Vec<EMACondition>,

    /// The innermost stack for the right hand side of the rewrite rule.
    pub rhs_stack: TermStack,
}

impl AnnouncementInnermost {
    pub fn new(rule: &Rule) -> AnnouncementInnermost {
        AnnouncementInnermost {
            conditions: extend_conditions(rule),
            equivalence_classes: derive_equivalence_classes(rule),
            rhs_stack: TermStack::from_rule(rule),
        }
    }
}
