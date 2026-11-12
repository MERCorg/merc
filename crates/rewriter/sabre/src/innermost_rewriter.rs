use std::cell::RefCell;
use std::convert::Infallible;

use merc_data::DataExpression;
use merc_data::DataExpressionRef;

use crate::Enumerator;
use crate::NoQuantifierEnumerator;
use crate::RewriteEngine;
use crate::RewriteMetrics;
use crate::RewriteSpecification;
use crate::Rule;
use crate::matching::condition_cache::DEFAULT_MAX_ENTRIES;
use crate::matching::conditions::EMACondition;
use crate::matching::conditions::extend_conditions;
use crate::matching::nonlinear::EquivalenceClass;
use crate::matching::nonlinear::derive_equivalence_classes;
use crate::rewrite_machine::Annotation;
use crate::rewrite_machine::MachineView;
use crate::rewrite_machine::Resumed;
use crate::rewrite_machine::RewriteMachine;
use crate::rewrite_machine::Scheduler;
use crate::rewrite_machine::TermStackId;
use crate::rewrite_machine::TermStacks;
use crate::set_automaton::MatchResult;
use crate::set_automaton::SetAutomaton;
use crate::utilities::DataPosition;
use crate::utilities::EmptySubstitution;
use crate::utilities::RewriteSubstitution;
use crate::utilities::TermStack;

/// Innermost Adaptive Pattern Matching Automaton (APMA) rewrite engine.
pub struct InnermostRewriter<E: Enumerator = NoQuantifierEnumerator> {
    machine: RewriteMachine<E, InnermostScheduler>,
}

impl<E: Enumerator> RewriteEngine for InnermostRewriter<E> {
    fn rewrite(&mut self, t: &DataExpression) -> DataExpression {
        self.machine.rewrite_under(t, &EmptySubstitution)
    }

    fn rewrite_with<S: RewriteSubstitution>(&mut self, t: &DataExpression, sigma: &S) -> DataExpression {
        self.machine.rewrite_under(t, sigma)
    }
}

impl InnermostRewriter<NoQuantifierEnumerator> {
    /// Creates a new InnermostRewriter from the given rewrite specification.
    ///
    /// Built without an [Enumerator]: any `exists`/`forall` quantifier the
    /// rewriter reaches is left undecided, over its rewritten body. Use
    /// [InnermostRewriter::with_enumerator] to have quantifiers decided.
    pub fn new(spec: &RewriteSpecification) -> Self {
        Self::with_condition_cache_capacity(spec, Some(DEFAULT_MAX_ENTRIES))
    }

    /// Like [InnermostRewriter::new], but `max_entries` sets the condition
    /// cache's capacity instead of the default, and `None` disables it (see
    /// `ConditionCache::new`).
    pub fn with_condition_cache_capacity(spec: &RewriteSpecification, max_entries: Option<usize>) -> Self {
        Self::build(spec, max_entries, NoQuantifierEnumerator)
    }
}

impl<E: Enumerator> InnermostRewriter<E> {
    /// The innermost rewriter is the rewriting machine with the [Innermost] scheduler: every
    /// symbol is eager.
    fn build(spec: &RewriteSpecification, max_entries: Option<usize>, enumerator: E) -> Self {
        // The automaton annotates the rules through a shared reference.
        let term_stacks = RefCell::new(TermStacks::default());
        let automaton = SetAutomaton::with_binders(spec, |rule| {
            term_stacks.borrow_mut().annotate(rule, TermStack::from_rule(rule))
        });

        InnermostRewriter {
            machine: RewriteMachine::new(automaton, term_stacks.into_inner(), InnermostScheduler, max_entries, enumerator),
        }
    }

    /// Like [InnermostRewriter::new], but `enumerator` is fed to the rewriter
    /// to decide `exists`/`forall` quantifiers it reaches.
    pub fn with_enumerator(spec: &RewriteSpecification, enumerator: E) -> Self {
        Self::with_enumerator_and_condition_cache_capacity(spec, enumerator, Some(DEFAULT_MAX_ENTRIES))
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

/// The annotation of a rule for the rewriters that only instantiate its right-hand side: the
/// [crate::NaiveRewriter] and the compiling rewriter.
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

/// The scheduler of the [crate::InnermostRewriter]: every argument is rewritten first, and the
/// variables of a right-hand side are the arguments on the stack.
pub(crate) struct InnermostScheduler;

impl Scheduler for InnermostScheduler {
    type Hook = Infallible;
    type Continuation = Infallible;
    type Scope = ();
    type State = ();

    #[inline(always)]
    fn decompose(&self, _machine: &mut MachineView<'_, '_, '_, Self>, _term: &DataExpressionRef<'_>) -> bool {
        false
    }

    #[inline(always)]
    fn enter(&self, _machine: &mut MachineView<'_, '_, '_, Self>, _right_hand_side: TermStackId, _arity: usize) {}

    #[inline(always)]
    fn variable(&self, machine: &mut MachineView<'_, '_, '_, Self>, _scope: (), base: usize, position: &DataPosition) {
        machine.stacks.push_argument(base, position);
    }

    #[inline(always)]
    fn leave(&self, _machine: &mut MachineView<'_, '_, '_, Self>, _scope: ()) {}

    fn hook(&self, _machine: &mut MachineView<'_, '_, '_, Self>, hook: &Infallible, _scope: ()) {
        match *hook {}
    }

    fn resume<'s>(
        &'s self,
        _machine: &mut MachineView<'_, '_, '_, Self>,
        continuation: Infallible,
    ) -> Resumed<'s, Infallible> {
        match continuation {}
    }

    fn matched(
        &self,
        _machine: &mut MachineView<'_, '_, '_, Self>,
        continuation: Infallible,
        _found: Option<MatchResult<'_, Annotation>>,
    ) {
        match continuation {}
    }
}
