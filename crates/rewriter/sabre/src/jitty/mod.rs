use std::cell::RefCell;

use merc_data::DataExpression;

use crate::Enumerator;
use crate::NoQuantifierEnumerator;
use crate::RewriteEngine;
use crate::RewriteMetrics;
use crate::RewriteSpecification;
use crate::matching::condition_cache::DEFAULT_MAX_ENTRIES;
use crate::rewrite_machine::RewriteMachine;
use crate::rewrite_machine::TermStacks;
use crate::utilities::EmptySubstitution;
use crate::utilities::RewriteSubstitution;

mod compile;
mod environment;
mod scheduler;
mod strategy;

use compile::Strategies;

/// A just-in-time rewriter: it rewrites an argument only when a rule needs it, following a
/// strategy per function symbol (van de Pol, "Just-in-time: on strategy annotations", 2001), as
/// mCRL2's jitty rewriter does. For `if(b, x, y)` it rewrites `b`, tries `if(true, x, y) = x`
/// and `if(false, x, y) = y`, and then rewrites only the selected branch.
///
/// It is the rewriting machine of the [crate::InnermostRewriter], with the strategies deciding
/// when arguments are rewritten.
pub struct JittyRewriter<E: Enumerator = NoQuantifierEnumerator> {
    machine: RewriteMachine<E, Strategies>,
}

impl<E: Enumerator> RewriteEngine for JittyRewriter<E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.machine.rewrite_under(t, &EmptySubstitution)
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        self.machine.rewrite_under(t, sigma)
    }
}

impl JittyRewriter<NoQuantifierEnumerator> {
    /// Creates a new rewriter for the given rewrite specification, which leaves quantifiers
    /// undecided, see [crate::InnermostRewriter::new].
    pub fn new(spec: &RewriteSpecification) -> Self {
        Self::with_condition_cache_capacity(spec, Some(DEFAULT_MAX_ENTRIES))
    }

    /// Like [JittyRewriter::new], but `max_entries` sets the condition cache's capacity, see
    /// [crate::InnermostRewriter::with_condition_cache_capacity].
    pub fn with_condition_cache_capacity(spec: &RewriteSpecification, max_entries: Option<usize>) -> Self {
        Self::with_enumerator_and_condition_cache_capacity(spec, NoQuantifierEnumerator, max_entries)
    }
}

impl<E: Enumerator> JittyRewriter<E> {
    /// Like [JittyRewriter::new], but `enumerator` decides the `exists`/`forall` quantifiers
    /// that the rewriter reaches.
    pub fn with_enumerator(spec: &RewriteSpecification, enumerator: E) -> Self {
        Self::with_enumerator_and_condition_cache_capacity(spec, enumerator, Some(DEFAULT_MAX_ENTRIES))
    }

    /// Like [JittyRewriter::with_enumerator], with the condition cache capacity of
    /// [JittyRewriter::with_condition_cache_capacity].
    pub fn with_enumerator_and_condition_cache_capacity(
        spec: &RewriteSpecification,
        enumerator: E,
        max_entries: Option<usize>,
    ) -> Self {
        // The automata annotate the rules through a shared reference.
        let term_stacks = RefCell::new(TermStacks::default());
        let (automaton, strategies) = Strategies::build(spec, &term_stacks);
        JittyRewriter {
            machine: RewriteMachine::new(automaton, term_stacks.into_inner(), strategies, max_entries, enumerator),
        }
    }

    /// Returns the metrics of the last rewrite, see [RewriteMetrics].
    pub fn metrics(&self) -> &RewriteMetrics {
        self.machine.metrics()
    }
}
