#![forbid(unsafe_code)]

use merc_aterm::storage::THREAD_TERM_POOL;
use merc_aterm::storage::ThreadTermPool;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_utilities::debug_trace;

use crate::RewriteMetrics;
use crate::RewriteSpecification;
use crate::matching::condition_cache::ConditionCache;
use crate::matching::nonlinear::check_equivalence_classes;
use crate::set_automaton::MatchAnnouncement;
use crate::set_automaton::SetAutomaton;
use crate::utilities::AnnouncementSabre;
use crate::utilities::ConfigurationStack;
use crate::utilities::DataPositionIndexed;
use crate::utilities::RewriteSubstitution;
use crate::utilities::SharedTermStack;
use crate::utilities::SideInfo;
use crate::utilities::SideInfoType;
use crate::utilities::TermStackBuilder;
use crate::utilities::apply_substitution;

/// A shared trait for all the rewriters
pub trait RewriteEngine {
    /// Rewrites the given term into normal form.
    fn rewrite(&mut self, term: &DataExpression) -> DataExpression;

    /// Rewrites the given term into normal form, replacing its free variables according to
    /// `sigma`.
    ///
    /// `sigma` must map every variable in its domain to a term already in normal form, since
    /// replacements are spliced in without being rewritten again; substitution happens exactly
    /// once per free variable occurrence, but rewrite rules can still fire across the
    /// substitution boundary, so `rewrite_with(plus(x, 0), {x -> 5})` yields `5` rather than
    /// `plus(5, 0)`. Variables outside the domain of `sigma` are left unchanged.
    ///
    /// # Panics
    ///
    /// Panics if `term` contains a binder or where clause, whose bound variables would need
    /// capture-avoiding renaming.
    fn rewrite_with<S: RewriteSubstitution>(&mut self, term: &DataExpression, sigma: &S) -> DataExpression {
        self.rewrite(&apply_substitution(term, sigma))
    }
}

/// The Set Automaton based Rewrite Engine implementation.
pub struct SabreRewriter {
    automaton: SetAutomaton<AnnouncementSabre>,
    /// A reusable builder for evaluating right-hand side term stacks, kept to
    /// avoid allocating a fresh one on every rewrite step.
    builder: TermStackBuilder,
    /// The shared LIFO term stack reused by every normalisation, including the
    /// nested ones performed while checking conditions. Keeping a single
    /// instance means only one protected container is ever registered, instead
    /// of one per configuration stack.
    term_stack: SharedTermStack,
    /// Caches condition-side normal forms across the whole computation —
    /// every top-level [rewrite call](SabreRewriter::rewrite), not just one —
    /// keyed by the substituted term; see `matching::condition_cache`.
    condition_cache: ConditionCache,
    /// The metrics of the last rewrite.
    metrics: RewriteMetrics,
}

impl RewriteEngine for SabreRewriter {
    fn rewrite(&mut self, term: &DataExpression) -> DataExpression {
        self.stack_based_normalise(term)
    }
}

impl SabreRewriter {
    pub fn new(spec: &RewriteSpecification) -> Self {
        Self::with_condition_cache_capacity(spec, Some(crate::matching::condition_cache::DEFAULT_MAX_ENTRIES))
    }

    /// Like [SabreRewriter::new], but `max_entries` sets the condition
    /// cache's capacity instead of the default, and `None` disables it (see
    /// `ConditionCache::new`).
    pub fn with_condition_cache_capacity(spec: &RewriteSpecification, max_entries: Option<usize>) -> Self {
        let automaton = SetAutomaton::new(spec, AnnouncementSabre::new, false);

        SabreRewriter {
            automaton,
            builder: TermStackBuilder::new(),
            term_stack: SharedTermStack::new(),
            condition_cache: ConditionCache::new(max_entries),
            metrics: RewriteMetrics::default(),
        }
    }

    /// Rewrites `t` to normal form.
    pub fn stack_based_normalise(&mut self, t: &DataExpression) -> DataExpression {
        self.metrics.reset();

        THREAD_TERM_POOL.with(|tp| {
            SabreRewriter::stack_based_normalise_aux(
                tp,
                &self.automaton,
                &mut self.builder,
                &mut self.term_stack,
                &mut self.condition_cache,
                t,
                &mut self.metrics,
            )
        })
    }

    /// Returns the metrics of the last rewrite, see [RewriteMetrics].
    pub fn metrics(&self) -> &RewriteMetrics {
        &self.metrics
    }

    /// Rewrites `t` to normal form using an explicit configuration stack instead of recursion.
    ///
    /// `tp` and `automaton` are passed as separate parameters, rather than bundled behind
    /// `&mut self`, so the term pool can be mutated while the automaton's state and transition
    /// data are still borrowed.
    #[allow(clippy::too_many_arguments)]
    fn stack_based_normalise_aux(
        tp: &ThreadTermPool,
        automaton: &SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        condition_cache: &mut ConditionCache,
        t: &DataExpression,
        metrics: &mut RewriteMetrics,
    ) -> DataExpression {
        metrics.recursion();

        // We explore the configuration tree depth first using a ConfigurationStack
        let mut cs = ConfigurationStack::new(term_stack, 0, t);

        // Big loop until we know we have a normal form
        'outer: loop {
            // Inner loop so that we can easily break; to the next iteration
            'skip_point: loop {
                debug_trace!("{}", cs);

                // Check if there is any configuration leaf left to explore, if not we have found a normal form
                if let Some(leaf_index) = cs.get_unexplored_leaf() {
                    let leaf_state = cs.stack[leaf_index].state;

                    match ConfigurationStack::pop_side_branch_leaf(&mut cs.side_branch_stack, leaf_index) {
                        None => {
                            if SabreRewriter::observe_leaf_transition(
                                tp, automaton, builder, term_stack, &mut cs, leaf_index, leaf_state, metrics,
                            ) {
                                break 'skip_point;
                            }
                        }
                        Some(sit) => {
                            SabreRewriter::handle_side_info(
                                tp,
                                automaton,
                                builder,
                                term_stack,
                                condition_cache,
                                &mut cs,
                                leaf_index,
                                sit,
                                metrics,
                            );
                        }
                    }
                } else {
                    // No configuration left to explore, we have found a normal form
                    break 'outer;
                }
            }
        }

        cs.compute_final_term(term_stack, tp)
    }

    /// Observes a leaf's symbol in the set automaton.
    ///
    /// Returns `true` when a rewrite rule was applied on the spot, in which case
    /// the caller restarts the configuration exploration (`break 'skip_point`).
    /// Returning `false` means the configuration only advanced (or backtracked),
    /// so the exploration loop can continue.
    #[allow(clippy::too_many_arguments)]
    fn observe_leaf_transition<'a>(
        tp: &ThreadTermPool,
        automaton: &'a SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        cs: &mut ConfigurationStack<'a>,
        leaf_index: usize,
        leaf_state: usize,
        metrics: &mut RewriteMetrics,
    ) -> bool {
        let read_terms = term_stack.terms.read();
        let leaf_term = &read_terms[cs.terms_base + leaf_index];

        // Observe a symbol according to the state label of the set automaton.
        let pos: DataExpressionRef = leaf_term.get_data_position(automaton.states()[leaf_state].label());

        metrics.symbol_compared();

        // Get the transition belonging to the observed symbol. A variable
        // has no head symbol and therefore matches no pattern position.
        let transition = pos
            .try_data_function_symbol()
            .and_then(|symbol| automaton.get_transition(leaf_state, symbol.operation_id()));

        if let Some(tr) = transition {
            // Loop over the match announcements of the transition
            for (announcement, annotation) in &tr.announcements {
                if annotation.conditions.is_empty() && annotation.equivalence_classes.is_empty() {
                    if annotation.is_duplicating {
                        debug_trace!("Delaying duplicating rule {}", announcement.rule);

                        // We do not want to apply duplicating rules straight away
                        cs.side_branch_stack.push(SideInfo {
                            corresponding_configuration: leaf_index,
                            info: SideInfoType::DelayedRewriteRule(announcement, annotation),
                        });
                    } else {
                        // For a rewrite rule that is not duplicating or has a condition we just apply it straight away
                        drop(read_terms);
                        SabreRewriter::apply_rewrite_rule(
                            tp,
                            automaton,
                            builder,
                            term_stack,
                            announcement,
                            annotation,
                            leaf_index,
                            cs,
                            metrics,
                        );
                        return true;
                    }
                } else {
                    // We delay the condition checks
                    debug_trace!("Delaying condition check for rule {}", announcement.rule);

                    cs.side_branch_stack.push(SideInfo {
                        corresponding_configuration: leaf_index,
                        info: SideInfoType::EquivalenceAndConditionCheck(announcement, annotation),
                    });
                }
            }

            drop(read_terms);
            if tr.destinations.is_empty() {
                // If there is no destination we are done matching and go back to the previous
                // configuration on the stack with information on the side stack.
                // Note, it could be that we stay at the same configuration and apply a rewrite
                // rule that was just discovered whilst exploring this configuration.
                let prev = cs.get_prev_with_side_info();
                cs.current_node = prev;
                if let Some(n) = prev {
                    cs.jump_back(term_stack, n, tp);
                }
            } else {
                // Grow the bud; if there is more than one destination a SideBranch object will be placed on the side stack
                let tr_slice = tr.destinations.as_slice();
                cs.grow(term_stack, leaf_index, tr_slice);
            }
        } else {
            drop(read_terms);
            let prev = cs.get_prev_with_side_info();
            cs.current_node = prev;
            if let Some(n) = prev {
                cs.jump_back(term_stack, n, tp);
            }
        }

        false
    }

    /// Handles a [SideInfoType] entry popped by the configuration stack, either
    /// picking the next child configuration, applying a delayed rewrite rule, or
    /// checking the delayed conditions (and then applying or backtracking).
    #[allow(clippy::too_many_arguments)]
    fn handle_side_info<'a>(
        tp: &ThreadTermPool,
        automaton: &'a SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        condition_cache: &mut ConditionCache,
        cs: &mut ConfigurationStack<'a>,
        leaf_index: usize,
        sit: SideInfoType<'a>,
        metrics: &mut RewriteMetrics,
    ) {
        match sit {
            SideInfoType::SideBranch(sb) => {
                // If there is a SideBranch pick the next child configuration
                cs.grow(term_stack, leaf_index, sb);
            }
            SideInfoType::DelayedRewriteRule(announcement, annotation) => {
                // apply the delayed rewrite rule
                SabreRewriter::apply_rewrite_rule(
                    tp,
                    automaton,
                    builder,
                    term_stack,
                    announcement,
                    annotation,
                    leaf_index,
                    cs,
                    metrics,
                );
            }
            SideInfoType::EquivalenceAndConditionCheck(announcement, annotation) => {
                // The equivalence classes and conditions are checked relative to
                // the match root, which sits at `announcement.position` inside the
                // leaf term (the same root used by `apply_rewrite_rule`). Protect
                // it so the shared term stack can be reused by the recursive
                // condition normalisation once the read guard is dropped.
                let read_terms = term_stack.terms.read();
                let leaf_term = &read_terms[cs.terms_base + leaf_index];
                let matched: DataExpression = leaf_term.get_data_position(&announcement.position).protect();
                drop(read_terms);

                // Apply the delayed rewrite rule if the conditions hold
                if check_equivalence_classes(&matched, &annotation.equivalence_classes)
                    && SabreRewriter::conditions_hold(
                        tp,
                        automaton,
                        builder,
                        term_stack,
                        condition_cache,
                        annotation,
                        &matched,
                        metrics,
                    )
                {
                    SabreRewriter::apply_rewrite_rule(
                        tp,
                        automaton,
                        builder,
                        term_stack,
                        announcement,
                        annotation,
                        leaf_index,
                        cs,
                        metrics,
                    );
                } else {
                    // The check failed, so this announcement does not apply. The
                    // side info was already popped, so move back to the previous
                    // configuration that still has side info.
                    let prev = cs.get_prev_with_side_info();
                    cs.current_node = prev;
                    if let Some(n) = prev {
                        cs.jump_back(term_stack, n, tp);
                    }
                }
            }
        }
    }

    /// Apply a rewrite rule and prune back
    #[allow(clippy::too_many_arguments)]
    fn apply_rewrite_rule(
        tp: &ThreadTermPool,
        automaton: &SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        announcement: &MatchAnnouncement,
        annotation: &AnnouncementSabre,
        leaf_index: usize,
        cs: &mut ConfigurationStack<'_>,
        metrics: &mut RewriteMetrics,
    ) {
        metrics.rule_applied(&announcement.rule);

        let read_terms = term_stack.terms.read();
        let leaf_subterm: &DataExpressionRef<'_> = &read_terms[cs.terms_base + leaf_index];

        // Computes the new subterm of the configuration
        let new_subterm = annotation
            .rhs_term_stack
            .evaluate_with(&leaf_subterm.get_data_position(&announcement.position), builder);

        debug_trace!(
            "rewrote {} to {} using rule {}",
            &leaf_subterm,
            &new_subterm,
            announcement.rule
        );

        // The match announcement tells us how far we need to prune back.
        let prune_point = leaf_index - announcement.symbols_seen;
        drop(read_terms);
        cs.prune(term_stack, tp, automaton, prune_point, new_subterm);
    }

    /// Checks conditions and subterm equality of non-linear patterns.
    ///
    /// `matched` is the subterm at the match root (`announcement.position`), which
    /// the caller has already resolved. The recursive normalisation reuses the
    /// shared `term_stack` (the parent frame's subterms sit below it, LIFO).
    #[allow(clippy::too_many_arguments)]
    fn conditions_hold(
        tp: &ThreadTermPool,
        automaton: &SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        condition_cache: &mut ConditionCache,
        annotation: &AnnouncementSabre,
        matched: &DataExpression,
        metrics: &mut RewriteMetrics,
    ) -> bool {
        for c in &annotation.conditions {
            let rhs: DataExpression = c.rhs_term_stack.evaluate_with(matched, builder);
            let lhs: DataExpression = c.lhs_term_stack.evaluate_with(matched, builder);

            // Equality => lhs == rhs.
            if !c.equality || lhs != rhs {
                let rhs_normal = SabreRewriter::normalise_condition_side(
                    tp,
                    automaton,
                    builder,
                    term_stack,
                    condition_cache,
                    &rhs,
                    metrics,
                );
                let lhs_normal = SabreRewriter::normalise_condition_side(
                    tp,
                    automaton,
                    builder,
                    term_stack,
                    condition_cache,
                    &lhs,
                    metrics,
                );

                // If lhs != rhs && !equality OR equality && lhs == rhs.
                if (!c.equality && lhs_normal == rhs_normal) || (c.equality && lhs_normal != rhs_normal) {
                    return false;
                }
            }
        }

        true
    }

    /// Normalises `term` as a condition side, returning `condition_cache`'s
    /// stored normal form for it if present, and storing the result there
    /// for reuse otherwise.
    #[allow(clippy::too_many_arguments)]
    fn normalise_condition_side(
        tp: &ThreadTermPool,
        automaton: &SetAutomaton<AnnouncementSabre>,
        builder: &mut TermStackBuilder,
        term_stack: &mut SharedTermStack,
        condition_cache: &mut ConditionCache,
        term: &DataExpression,
        metrics: &mut RewriteMetrics,
    ) -> DataExpression {
        if let Some(normal_form) = condition_cache.get(term) {
            metrics.condition_cache_hit();
            return normal_form;
        }

        let normal_form = SabreRewriter::stack_based_normalise_aux(
            tp,
            automaton,
            builder,
            term_stack,
            condition_cache,
            term,
            metrics,
        );
        condition_cache.insert(term.clone(), normal_form.clone());
        normal_form
    }
}
