use merc_aterm::storage::THREAD_TERM_POOL;
use merc_aterm::storage::ThreadTermPool;
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
use crate::RewriteSpecification;
use crate::RewritingStatistics;
use crate::Rule;
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
use crate::utilities::Shadowed;
use crate::utilities::TermStack;
use crate::utilities::TermStackBuilder;
use merc_utilities::debug_trace;

impl<E: Enumerator> RewriteEngine for InnermostRewriter<E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.rewrite_with_statistics(t).0
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        self.rewrite_under_with_statistics(t, sigma).0
    }
}

/// What [rewrite_aux] does after a configuration has been taken from the stack.
enum Step {
    Continue,

    /// Find a match for the constructed term, and place the result at the index.
    Match(DataExpression, usize),

    /// Decide, or reconstruct, the binder term and place the result at the index.
    Binder(DataAbstraction, usize),

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
    /// cache's capacity (0 means unbounded; see `ConditionCache::new`)
    /// instead of the default.
    pub fn with_condition_cache_capacity(spec: &RewriteSpecification, max_entries: Option<usize>) -> Self {
        Self::build(spec, max_entries, NoQuantifierEnumerator)
    }
}

impl<E: Enumerator> InnermostRewriter<E> {
    fn build(spec: &RewriteSpecification, max_entries: Option<usize>, enumerator: E) -> Self {
        let apma = SetAutomaton::new(spec, AnnouncementInnermost::new, true);

        InnermostRewriter {
            apma,
            stack: InnermostStack::default(),
            builder: TermStackBuilder::new(),
            condition_cache: ConditionCache::new(max_entries),
            enumerator,
            frames: Vec::new(),
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

    /// Rewrites `t` to normal form and returns the normal form together with the
    /// [RewritingStatistics] gathered while doing so, most notably the number of
    /// applied rewrite steps.
    pub fn rewrite_with_statistics(&mut self, t: &DataExpression) -> (DataExpression, RewritingStatistics) {
        self.rewrite_under_with_statistics(t, &EmptySubstitution)
    }

    /// Replaces the free variables of `t` according to `sigma` and rewrites the result to normal
    /// form, returning it together with the gathered [RewritingStatistics].
    ///
    /// `sigma` must map every variable to a term already in normal form: replacements are spliced
    /// in without being rewritten again.
    pub fn rewrite_under_with_statistics<S: RewriteSubstitution>(
        &mut self,
        t: &DataExpression,
        sigma: &S,
    ) -> (DataExpression, RewritingStatistics) {
        let mut stats = RewritingStatistics::default();

        debug_trace!("input: {}", t);

        let result = THREAD_TERM_POOL.with(|tp| {
            rewrite_aux(
                tp,
                &mut self.stack,
                &mut self.builder,
                &mut stats,
                &self.apma,
                &mut self.condition_cache,
                t,
                sigma,
                &self.enumerator,
                &mut self.frames,
            )
        });

        (result, stats)
    }
}

/// Rewrites `input_term` to normal form, replacing its free variables by their image under
/// `sigma`.
///
/// `automaton`, `stack` and `builder` are passed as separate parameters, rather than bundled
/// behind `&mut self`, so the borrow checker allows each to be borrowed independently. In
/// particular, so a nested rewrite (a condition, or a quantifier's candidate body) can reuse
/// the same stack and automaton without holding a borrow of the whole [InnermostRewriter],
/// which is what lets a quantifier's search call back into it (see [evaluate_binder]).
///
/// Uses a stack of terms and configurations to avoid recursion and to keep track of terms in
/// normal form without explicit tagging. Each configuration is one of:
///     - Return(): Returns the top of the stack.
///     - Rewrite(index): Rewrites the top of the term stack and places the result at the given
///       index.
///     - Construct(symbol, arity, index): Constructs `symbol` applied to the `arity` terms at
///       the top of the stack and places the result at the given index.
///
/// Free variables of `input_term` are replaced by their image under `sigma` when they are
/// reached, which is the point where the substitution is consumed: every term that is
/// matched, constructed or used to instantiate a condition afterwards is already
/// substituted.
#[allow(clippy::too_many_arguments)]
fn rewrite_aux<S: RewriteSubstitution, E: Enumerator>(
    tp: &ThreadTermPool,
    stack: &mut InnermostStack,
    builder: &mut TermStackBuilder,
    stats: &mut RewritingStatistics,
    automaton: &SetAutomaton<AnnouncementInnermost>,
    condition_cache: &mut ConditionCache,
    input_term: &DataExpression,
    sigma: &S,
    enumerator: &E,
    frames: &mut Vec<E::Frame>,
) -> DataExpression {
    stats.recursions += 1;
    stack.terms.write(|write_terms| {
        stack.configs.write(|write_configs| {
            // Push the result term to the stack.
            let top_of_stack = write_terms.len();
            write_configs.push(Config::Return());
            write_terms.push(None);
            InnermostStack::add_rewrite(write_configs, write_terms, input_term.copy(), top_of_stack);
        })
    });

    loop {
        debug_trace!("{}", stack);

        let step = stack.configs.write(|write_configs| {
            let config = write_configs
                .pop()
                .expect("The stack always ends with a Return configuration");
            stack.terms.write(|write_terms| match config {
                Config::Rewrite(result) => {
                    let term = write_terms.pop().unwrap().unwrap();

                    // A machine number is a value in normal form; it has no head function
                    // symbol to decompose, so place it directly at the result index.
                    if is_data_machine_number(&term) {
                        // Safety: term is stored in the container on the same line.
                        write_terms[result] = Some(unsafe { write_terms.protect(&term) }.into());
                        return Step::Continue;
                    }

                    // A binder has no head function symbol either, and is not addressed by
                    // the set automaton.
                    if is_data_binder(&term) {
                        return Step::Binder(term.protect().into(), result);
                    }

                    if let Some(symbol) = term.try_data_function_symbol() {
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
                    } else {
                        // A variable has no head symbol to match on. It is replaced by its
                        // image under sigma, which is assumed to be in normal form, so it
                        // goes straight into the result slot without being constructed.
                        // Variables outside the domain of sigma are their own normal form.
                        let variable: DataVariableRef<'_> = term.copy().into();
                        let replacement = sigma.get(&variable).unwrap_or_else(|| term.copy());

                        // Safety: replacement is stored in the container on the same line.
                        write_terms[result] = Some(unsafe { write_terms.protect(&replacement) }.into());
                    }
                    Step::Continue
                }
                Config::Construct(symbol, arity, index) => {
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
                Config::Term(term, index) => {
                    // A constant carried by a right-hand side (a machine number)
                    // is already in normal form: place it at its index directly.
                    // Safety: term is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&term) }.into());
                    Step::Continue
                }
                Config::Return() => Step::Return(
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
            Step::Binder(abstraction, index) => {
                let value = evaluate_binder(
                    tp,
                    stack,
                    builder,
                    stats,
                    automaton,
                    condition_cache,
                    &abstraction,
                    sigma,
                    enumerator,
                    frames,
                );

                stack.terms.write(|write_terms| {
                    // Safety: value is stored in the container on the same line.
                    write_terms[index] = Some(unsafe { write_terms.protect(&value) }.into());
                });
            }
            Step::Match(term, index) => {
                match find_match(
                    tp,
                    stack,
                    builder,
                    stats,
                    automaton,
                    condition_cache,
                    &term.copy(),
                    enumerator,
                    frames,
                ) {
                    Some(MatchResult::Native(result)) => {
                        debug_trace!("native rewrite {} => {}", term, result);

                        stack.terms.write(|write_terms| {
                            // Safety: result is stored in the container on the same line.
                            write_terms[index] = Some(unsafe { write_terms.protect(&result) }.into());
                        });
                        stats.rewrite_steps += 1;
                    }
                    Some(MatchResult::Rule(_announcement, annotation)) => {
                        debug_trace!(
                            "rewrite {} => {} using rule {}",
                            term,
                            annotation.rhs_stack.evaluate(&term),
                            _announcement.rule
                        );

                        // Reacquire the write access and add the matching RHSStack.
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
                        stats.rewrite_steps += 1;
                    }
                    None => {
                        // Add the term on the stack.
                        stack.terms.write(|write_terms| {
                            // Safety: term is stored in the container on the same line.
                            write_terms[index] = Some(unsafe { write_terms.protect(&term) }.into());
                        });
                    }
                }
            }
            Step::Return(result) => return result,
        }

        if cfg!(debug_assertions) {
            let read_configs = stack.configs.read();
            for (index, term) in stack.terms.read().iter().enumerate() {
                if term.is_none() {
                    debug_assert!(
                        read_configs.iter().any(|x| {
                            match x {
                                Config::Construct(_, _, result) => index == *result,
                                Config::Rewrite(result) => index == *result,
                                Config::Term(_, result) => index == *result,
                                Config::Return() => true,
                            }
                        }),
                        "The default term at index {index} is not a result of any operation."
                    );
                }
            }
        }
    }
}

/// Decides, or reconstructs unevaluated, the binder term `abstraction`.
///
/// The body is rewritten first, with the bound variables held as free normal
/// forms (shadowed against `sigma`, so an enclosing substitution cannot reach
/// into the binder's own scope), and — for `Exists`/`Forall` — `enumerator`
/// is then asked to decide it.
///
/// As in mCRL2's `Rewriter::quantifier_enumeration`, a nested quantifier is
/// therefore first tried while rewriting the outer body, with the outer
/// variables still free. If that does not settle it, it remains in the outer
/// body, and is tried again each time the outer search rewrites that body
/// under a candidate value: [NestedRewriteView] decides quantifiers through
/// the same `enumerator`, on a frame of its own.
#[allow(clippy::too_many_arguments)]
fn evaluate_binder<S: RewriteSubstitution, E: Enumerator>(
    tp: &ThreadTermPool,
    stack: &mut InnermostStack,
    builder: &mut TermStackBuilder,
    stats: &mut RewritingStatistics,
    automaton: &SetAutomaton<AnnouncementInnermost>,
    condition_cache: &mut ConditionCache,
    abstraction: &DataAbstraction,
    sigma: &S,
    enumerator: &E,
    frames: &mut Vec<E::Frame>,
) -> DataExpression {
    let binder = abstraction.binder_type();
    let vars: Vec<DataVariable> = abstraction.variables().into_iter().collect();
    let body = abstraction.body().protect();

    let shadowed = Shadowed::new(sigma, &vars);
    let rewritten_body = rewrite_aux(
        tp,
        stack,
        builder,
        stats,
        automaton,
        condition_cache,
        &body,
        &shadowed,
        enumerator,
        frames,
    );

    if !matches!(binder, BinderType::Exists | BinderType::Forall) {
        // Lambda / SetComp / BagComp: reduction is unimplemented.
        return DataAbstraction::new(binder, &vars, rewritten_body).into();
    }

    // The frames of the enclosing searches are taken off `frames` while they
    // run, so the one on top is free to reuse.
    let mut frame = frames.pop().unwrap_or_default();
    let mut view = NestedRewriteView {
        tp,
        stack,
        builder,
        stats,
        automaton,
        condition_cache,
        enumerator,
        frames,
    };

    let decision = enumerator.decide(&mut frame, &mut view, binder, &vars, &rewritten_body);
    frames.push(frame);

    match decision {
        Some(value) => DataExpression::bool_literal(value),
        // No decision could be made, yield the reconstructed binder.
        None => DataAbstraction::new(binder, &vars, rewritten_body).into(),
    }
}

/// A [RewriteEngine] handle used only while a [Enumerator::decide] call is in
/// progress (see [evaluate_binder]): rewrites through the same innermost
/// stack and automaton, and decides nested quantifiers through the same
/// enumerator.
struct NestedRewriteView<'a, E: Enumerator> {
    tp: &'a ThreadTermPool,
    stack: &'a mut InnermostStack,
    builder: &'a mut TermStackBuilder,
    stats: &'a mut RewritingStatistics,
    automaton: &'a SetAutomaton<AnnouncementInnermost>,
    condition_cache: &'a mut ConditionCache,
    enumerator: &'a E,
    frames: &'a mut Vec<E::Frame>,
}

impl<E: Enumerator> RewriteEngine for NestedRewriteView<'_, E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.rewrite_with(t, &EmptySubstitution)
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        rewrite_aux(
            self.tp,
            self.stack,
            self.builder,
            self.stats,
            self.automaton,
            self.condition_cache,
            t,
            sigma,
            self.enumerator,
            self.frames,
        )
    }
}

/// Use the APMA to find a match for the given term: either a rewrite rule,
/// or — when the term's head symbol is a machine-word operation — the
/// natively-evaluated result.
#[allow(clippy::too_many_arguments)]
fn find_match<'a, E: Enumerator>(
    tp: &ThreadTermPool,
    stack: &mut InnermostStack,
    builder: &mut TermStackBuilder,
    stats: &mut RewritingStatistics,
    automaton: &'a SetAutomaton<AnnouncementInnermost>,
    condition_cache: &mut ConditionCache,
    t: &DataExpressionRef<'_>,
    enumerator: &E,
    frames: &mut Vec<E::Frame>,
) -> Option<MatchResult<'a, AnnouncementInnermost>> {
    // Start at the initial state
    let mut state_index = 0;
    loop {
        let state = &automaton.states()[state_index];

        // Get the symbol at the position state.label; a variable there matches no pattern.
        stats.symbol_comparisons += 1;
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
                    && check_conditions(
                        tp,
                        stack,
                        builder,
                        stats,
                        automaton,
                        condition_cache,
                        annotation,
                        t,
                        enumerator,
                        frames,
                    )
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
#[allow(clippy::too_many_arguments)]
fn check_conditions<E: Enumerator>(
    tp: &ThreadTermPool,
    stack: &mut InnermostStack,
    builder: &mut TermStackBuilder,
    stats: &mut RewritingStatistics,
    automaton: &SetAutomaton<AnnouncementInnermost>,
    condition_cache: &mut ConditionCache,
    announcement: &AnnouncementInnermost,
    t: &DataExpressionRef<'_>,
    enumerator: &E,
    frames: &mut Vec<E::Frame>,
) -> bool {
    for c in &announcement.conditions {
        let rhs: DataExpression = c.rhs_term_stack.evaluate_with(t, builder);
        let lhs: DataExpression = c.lhs_term_stack.evaluate_with(t, builder);

        let rhs_normal = normalise_condition_side(
            tp,
            stack,
            builder,
            stats,
            automaton,
            condition_cache,
            &rhs,
            enumerator,
            frames,
        );
        let lhs_normal = normalise_condition_side(
            tp,
            stack,
            builder,
            stats,
            automaton,
            condition_cache,
            &lhs,
            enumerator,
            frames,
        );

        if (lhs_normal != rhs_normal && c.equality) || (lhs_normal == rhs_normal && !c.equality) {
            return false;
        }
    }

    true
}

/// Normalises `term` as a condition side, returning `condition_cache`'s
/// stored normal form for it if present, and storing the result there
/// for reuse otherwise.
#[allow(clippy::too_many_arguments)]
fn normalise_condition_side<E: Enumerator>(
    tp: &ThreadTermPool,
    stack: &mut InnermostStack,
    builder: &mut TermStackBuilder,
    stats: &mut RewritingStatistics,
    automaton: &SetAutomaton<AnnouncementInnermost>,
    condition_cache: &mut ConditionCache,
    term: &DataExpression,
    enumerator: &E,
    frames: &mut Vec<E::Frame>,
) -> DataExpression {
    if let Some(normal_form) = condition_cache.get(term) {
        stats.condition_cache_hits += 1;
        return normal_form;
    }

    let normal_form = rewrite_aux(
        tp,
        stack,
        builder,
        stats,
        automaton,
        condition_cache,
        term,
        &EmptySubstitution,
        enumerator,
        frames,
    );
    condition_cache.insert(term.clone(), normal_form.clone());
    normal_form
}

/// Innermost Adaptive Pattern Matching Automaton (APMA) rewrite engine.
pub struct InnermostRewriter<E: Enumerator = NoQuantifierEnumerator> {
    apma: SetAutomaton<AnnouncementInnermost>,
    stack: InnermostStack,
    builder: TermStackBuilder,
    /// Caches condition-side normal forms across the whole computation.
    condition_cache: ConditionCache,
    /// Decides `exists`/`forall` quantifiers the rewriter reaches; see
    /// [Enumerator].
    enumerator: E,
    /// One search frame per quantifier nesting depth, reused between searches.
    frames: Vec<E::Frame>,
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
