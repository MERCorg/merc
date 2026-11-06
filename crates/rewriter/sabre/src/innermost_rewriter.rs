use std::cell::OnceCell;

use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::is_data_machine_number;

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
use crate::set_automaton::MatchResult;
use crate::set_automaton::SetAutomaton;
use crate::set_automaton::machine_number_symbol;
use crate::utilities::Config;
use crate::utilities::DataPositionIndexed;
use crate::utilities::EmptySubstitution;
use crate::utilities::InnermostStack;
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

/// What [rewrite_aux] does after a configuration has been taken from the stack.
enum Step {
    Continue,

    /// Find a match for the constructed term, and place the result at the index.
    Match(DataExpression, usize),

    /// Rewrite a binder of the input term under `sigma`, and place the result at the index.
    SubjectBinder(DataAbstraction, usize),

    /// Rewrite the binder `b` of a right-hand side, given as the template `lambda x0, .., xn .
    /// b` over its free variables (see [Config::Binder]), under `xi := values[i]`, and place the
    /// result at the index.
    RhsBinder(DataAbstraction, Vec<DataExpression>, usize),

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
        let automaton = SetAutomaton::with_binders(spec, AnnouncementInnermost::new);

        InnermostRewriter {
            machine: RewriteMachine::new(automaton, max_entries, enumerator),
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
pub(crate) struct RewriteMachine<E: Enumerator> {
    automaton: SetAutomaton<AnnouncementInnermost>,
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
    pub fn new(automaton: SetAutomaton<AnnouncementInnermost>, max_entries: Option<usize>, enumerator: E) -> Self {
        RewriteMachine {
            automaton,
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
            condition_cache: &mut self.condition_cache,
            enumerator: &self.enumerator,
            binders: &mut self.binders,
        };
        rewrite_aux(&mut context, t, sigma)
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
struct RewriteContext<'a, E: Enumerator> {
    stack: &'a mut InnermostStack,
    builder: &'a mut TermStackBuilder,
    metrics: &'a mut RewriteMetrics,
    automaton: &'a SetAutomaton<AnnouncementInnermost>,
    condition_cache: &'a mut ConditionCache,
    enumerator: &'a E,
    binders: &'a mut BinderState<E::Frame>,
}

/// Rewrites `input_term` to normal form, replacing its free variables by their image under
/// `sigma`.
///
/// Uses a stack of terms and configurations to avoid recursion and to keep track of terms in
/// normal form without explicit tagging. Each configuration is one of:
///     - Return: Returns the top of the stack.
///     - Rewrite { index }: Rewrites the top of the term stack and places the result at the
///       given index.
///     - Construct { symbol, arity, index }: Constructs `symbol` applied to the `arity` terms at
///       the top of the stack and places the result at the given index.
///     - Term { term, index }: Places a normal form of a right-hand side at the given index.
///     - Binder { template, arity, index }: Rewrites a binder of a right-hand side, with the
///       values of its free variables at the top of the stack.
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
///     - A binder of a right-hand side is instantiated by `Binder`. Its free variables are
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

    let stack = &mut *context.stack;
    stack.terms.write(|write_terms| {
        stack.configs.write(|write_configs| {
            // Push the result term to the stack.
            let top_of_stack = write_terms.len();
            write_configs.push(Config::Return);
            write_terms.push(None);
            InnermostStack::add_rewrite(write_configs, write_terms, input_term.copy(), top_of_stack);
        })
    });

    loop {
        debug_trace!("{}", context.stack);

        let stack = &mut *context.stack;
        let step = stack.configs.write(|write_configs| {
            let config = write_configs
                .pop()
                .expect("The stack always ends with a Return configuration");
            stack.terms.write(|write_terms| match config {
                Config::Rewrite { index: result } => {
                    let term = write_terms.pop().unwrap().unwrap();

                    match term.kind() {
                        // A machine number is a value in normal form; it has no head function
                        // symbol to decompose, so place it directly at the result index.
                        DataExpressionKind::MachineNumber(_) => {
                            // Safety: term is stored in the container on the same line.
                            write_terms[result] = Some(unsafe { write_terms.protect(&term) }.into());
                        }
                        // A binder has no head function symbol either, and is not addressed by
                        // the set automaton.
                        DataExpressionKind::Abstraction(binder) => {
                            return Step::SubjectBinder(binder.protect(), result);
                        }
                        DataExpressionKind::Application(_) | DataExpressionKind::FunctionSymbol(_) => {
                            let symbol = term.data_function_symbol();
                            let arguments = term.data_arguments();

                            // For all the argument we reserve space on the stack.
                            let top_of_stack = write_terms.len();
                            for _ in 0..arguments.len() {
                                write_terms.push(Default::default());
                            }

                            // Safety: symbol is stored in the container on the next line.
                            let symbol = unsafe { write_configs.protect(&symbol) };
                            InnermostStack::add_result(write_configs, symbol.into(), arguments.len(), result);
                            for (offset, arg) in arguments.into_iter().enumerate() {
                                InnermostStack::add_rewrite(write_configs, write_terms, arg, top_of_stack + offset);
                            }
                        }
                        // A variable has no head symbol to match on. It is replaced by its
                        // image under sigma, which is assumed to be in normal form, so it
                        // goes straight into the result slot without being constructed.
                        // Variables outside the domain of sigma are their own normal form.
                        DataExpressionKind::Variable(variable) => {
                            let replacement = sigma.get(&variable).unwrap_or_else(|| term.copy());

                            // Safety: replacement is stored in the container on the same line.
                            write_terms[result] = Some(unsafe { write_terms.protect(&replacement) }.into());
                        }
                        DataExpressionKind::WhereClause(_) => {
                            unimplemented!("rewriting a where clause: {term}")
                        }
                    }
                    Step::Continue
                }
                Config::Construct { symbol, arity, index } => {
                    // Take the last arity arguments.
                    let length = write_terms.len();

                    let arguments = &write_terms[length - arity..];

                    let term: DataExpression = if arguments.is_empty() {
                        symbol.protect().into()
                    } else {
                        DataApplication::with_iter(&symbol, arguments.len(), arguments.iter().flatten()).into()
                    };

                    // Remove the arguments from the stack.
                    write_terms.drain(length - arity..);
                    Step::Match(term, index)
                }
                Config::Binder {
                    template: binder,
                    arity,
                    index,
                } => {
                    let length = write_terms.len();
                    let values: Vec<DataExpression> = write_terms[length - arity..]
                        .iter()
                        .flatten()
                        .map(|value| value.protect())
                        .collect();
                    write_terms.drain(length - arity..);
                    Step::RhsBinder(binder.protect().into(), values, index)
                }
                Config::Term { term, index } => {
                    // A constant carried by a right-hand side (a machine number)
                    // is already in normal form: place it at its index directly.
                    // Safety: term is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&term) }.into());
                    Step::Continue
                }
                Config::Return => Step::Return(
                    write_terms
                        .pop()
                        .expect("The result should be the last element on the stack")
                        .expect("The result should be Some")
                        .protect(),
                ),
            })
        });

        match step {
            Step::Continue => {}
            Step::SubjectBinder(abstraction, index) => {
                let range = sigma_range.get_or_init(|| sigma.range());
                let value = rewrite_binder(context, &abstraction, sigma, range);

                context.stack.terms.write(|write_terms| {
                    // Safety: value is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&value) }.into());
                });
            }
            Step::RhsBinder(template, values, index) => {
                // The lambda only records which variables the values are for.
                let variables: Vec<DataVariable> = template.variables().into_iter().collect();
                let abstraction: DataAbstraction = template.body().protect().into();

                let values = SliceSubstitution::new(&variables, &values);
                let value = rewrite_binder(context, &abstraction, &values, &values.range());

                context.stack.terms.write(|write_terms| {
                    // Safety: value is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&value) }.into());
                });
            }
            Step::Match(term, index) => {
                let automaton = context.automaton;
                match find_match(context, automaton, &term.copy()) {
                    Some(MatchResult::Native(result)) => {
                        debug_trace!("native rewrite {} => {}", term, result);

                        context.stack.terms.write(|write_terms| {
                            // Safety: result is stored in the container on the same line.
                            write_terms[index] = Some(unsafe { write_terms.protect(&result) }.into());
                        });
                        context.metrics.native_applied(&term.data_function_symbol());
                    }
                    Some(MatchResult::Rule(announcement, annotation)) => {
                        debug_trace!(
                            "rewrite {} => {} using rule {}",
                            term,
                            annotation.rhs_stack.evaluate(&term),
                            announcement.rule
                        );

                        // Reacquire the write access and add the matching RHSStack.
                        let stack = &mut *context.stack;
                        stack.terms.write(|write_terms| {
                            stack.configs.write(|write_configs| {
                                InnermostStack::integrate(
                                    write_configs,
                                    write_terms,
                                    &annotation.rhs_stack,
                                    &term.copy(),
                                    index,
                                )
                            })
                        });
                        context.metrics.rule_applied(&announcement.rule);
                    }
                    None => {
                        // Add the term on the stack.
                        context.stack.terms.write(|write_terms| {
                            // Safety: term is stored in the container on the same line.
                            write_terms[index] = Some(unsafe { write_terms.protect(&term) }.into());
                        });
                    }
                }
            }
            Step::Return(result) => return result,
        }

        if cfg!(debug_assertions) {
            let read_configs = context.stack.configs.read();
            for (index, term) in context.stack.terms.read().iter().enumerate() {
                if term.is_none() {
                    debug_assert!(
                        read_configs.iter().any(|x| {
                            match x {
                                Config::Construct { index: result, .. }
                                | Config::Rewrite { index: result }
                                | Config::Term { index: result, .. }
                                | Config::Binder { index: result, .. } => index == *result,
                                Config::Return => true,
                            }
                        }),
                        "The default term at index {index} is not a result of any operation."
                    );
                }
            }
        }
    }
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

/// Use the APMA to find a match for the given term: either a rewrite rule,
/// or — when the term's head symbol is a machine-word operation — the
/// natively-evaluated result.
///
/// The automaton is passed separately from the context, so that the match it returns does not
/// borrow the context.
fn find_match<'a, E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
    automaton: &'a SetAutomaton<AnnouncementInnermost>,
    t: &DataExpressionRef<'_>,
) -> Option<MatchResult<'a, AnnouncementInnermost>> {
    // Start at the initial state
    let mut state_index = 0;
    loop {
        let state = &automaton.states()[state_index];

        // Get the symbol at the position state.label; a variable there matches no pattern.
        context.metrics.symbol_compared();
        let pos = t.get_data_position(state.label());

        // A machine number carries no function symbol, so it is matched under
        // the shared stand-in symbol the automaton reserves for them.
        let operation_id = if is_data_machine_number(&pos) {
            machine_number_symbol().operation_id()
        } else {
            pos.try_data_function_symbol()?.operation_id()
        };

        // Get the transition for the label and check if there is a pattern match
        {
            let transition = automaton.get_transition(state_index, operation_id)?;

            // The very first transition observes the term's own head symbol
            // (state 0's label is always the root position ε). That is the
            // only point at which `native` refers to the term being matched
            // as a whole, rather than to some other subterm the automaton
            // happens to inspect while narrowing down candidate rules.
            if state_index == 0
                && let Some(op) = transition.native
            {
                return op.evaluate(t.data_arguments()).map(MatchResult::Native);
            }

            for (announcement, annotation) in &transition.announcements {
                if check_equivalence_classes(t, &annotation.equivalence_classes)
                    && check_conditions(context, annotation, t)
                {
                    // We found a matching pattern
                    return Some(MatchResult::Rule(announcement, annotation));
                }
            }

            // If there is no pattern match we check if the transition has a destination state
            if transition.destinations.is_empty() {
                // If there is no destination state there is no pattern match
                return None;
            }

            state_index = transition.destinations.first().unwrap().1;
        }
    }
}

/// Checks whether the condition holds for given match announcement.
///
/// The condition sides are built from the matched subterms, which are already substituted
/// normal forms, so the nested normalisations must not apply the substitution a second time.
fn check_conditions<E: Enumerator>(
    context: &mut RewriteContext<'_, E>,
    announcement: &AnnouncementInnermost,
    t: &DataExpressionRef<'_>,
) -> bool {
    for c in &announcement.conditions {
        let rhs: DataExpression = c.rhs_term_stack.evaluate_with(t, context.builder);
        let lhs: DataExpression = c.lhs_term_stack.evaluate_with(t, context.builder);

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
            rhs_stack: TermStack::new(rule),
        }
    }
}
