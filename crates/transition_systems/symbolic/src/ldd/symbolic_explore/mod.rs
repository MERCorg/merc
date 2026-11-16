use std::ops::ControlFlow;

use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;

use merc_data::DataExpression;
use merc_lts::TransitionLabel;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::SymbolicLPS;
use crate::TransitionGroup;

mod saturation;
mod step;

/// A symbolic LTS — extends [SymbolicLPS] with LTS-specific metadata.
pub trait SymbolicLTS: SymbolicLPS {
    /// The label type for transitions in this LTS.
    type Label: TransitionLabel;

    /// Returns the LDD representing the set of states.
    fn states(&self) -> &LDDFunction;

    /// Returns the action labels for the LTS.
    fn action_labels(&self) -> &[Self::Label];

    /// Returns the possible values for each process parameter.
    fn parameter_values(&self) -> &[Vec<DataExpression>];
}

/// The order in which transition groups are applied during reachability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum ExplorationStrategy {
    /// Plain breadth-first: every group computes successors of the original frontier.
    #[default]
    BreadthFirst,
    /// Successors found by earlier groups feed into later groups within the same iteration.
    Chaining,
    /// Each group is applied to a fixpoint before moving on to the next on.
    Fixpoint,
    /// Like [Self::Fixpoint], but after each group all earlier groups are re-applied to a fixpoint.
    FixpointChaining,
    /// Saturation (van Dijk, Meijer, van de Pol, TACAS 2019), as `lpsreach --recursive-saturation`:
    /// every node is closed under the groups that start at its level and below, bottom-up, and the
    /// groups learn their transitions inside the saturation, at their top position. Always caches
    /// the learned domains.
    Saturation,
}

/// Options controlling [reachability_with_options].
///
/// Build with the `metrics` cargo feature to also log manager node counts and oxidd's own per-op
/// apply-cache counters (calls/queries/hits) at `info` level once per outer iteration/round,
/// unconditionally — there is no separate runtime flag for this.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReachabilityOptions {
    /// The strategy used to apply the transition groups.
    pub strategy: ExplorationStrategy,

    /// Whether to detect and report deadlock states (states without outgoing transitions).
    pub detect_deadlocks: bool,

    /// Whether every transition group caches the domain of its learned relation, so that it only
    /// learns the successors of read projections it has not seen before.
    pub cached: bool,
}

/// The result of a reachability run.
pub struct ReachabilityResult {
    /// The set of reachable states.
    pub states: LDDFunction,

    /// The deadlock states (reachable states with no outgoing transition), or `None` when
    /// [ReachabilityOptions::detect_deadlocks] was not requested.
    ///
    /// If the exploration was stopped early (see [ReachabilityResult::complete]), these are only the
    /// deadlocks among the states that were explored, and saturation finds none.
    pub deadlocks: Option<LDDFunction>,

    /// Whether the fixpoint was reached. This is `false` if the callback of
    /// [reachability_with_callback] stopped the exploration first, in which case
    /// [ReachabilityResult::states] is only an under-approximation of the reachable states.
    pub complete: bool,
}

/// Performs reachability analysis using the given initial state and transitions.
///
/// Uses the default [ReachabilityOptions]; see [reachability_with_options] for strategies and
/// deadlock detection. Returns only the reachable states.
pub fn reachability<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    timing: &Timing,
) -> Result<LDDFunction, MercError> {
    let mut context = lts.create_context();
    Ok(reachability_with_options(storage, lts, &mut context, &ReachabilityOptions::default(), timing)?.states)
}

/// Performs reachability analysis using the given initial state, transitions and [ReachabilityOptions].
///
/// `context` is created once by the caller (via [`SymbolicLPS::create_context`]) and threaded through
/// every learning call, so that its interned state — e.g. the value/label interning some
/// implementations use — is still available to the caller once reachability finishes.
pub fn reachability_with_options<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    options: &ReachabilityOptions,
    timing: &Timing,
) -> Result<ReachabilityResult, MercError> {
    reachability_with_callback(storage, lts, context, options, timing, |_, _| ControlFlow::Continue(()))
}

/// Like [reachability_with_options], but calls `on_iteration` after every iteration of the outer loop.
///
/// The arguments are the number of iterations that have completed so far, starting at one, and the
/// states that have been found up to and including that iteration. What an iteration is depends on
/// [ExplorationStrategy]: for the breadth-first, chaining and fixpoint strategies it is one step over
/// the frontier, and for saturation it is one round of learning relations and saturating.
///
/// If `on_iteration` returns [ControlFlow::Break] the exploration stops, and the states found so far
/// are returned with [ReachabilityResult::complete] set to `false`, unless that iteration had
/// already reached the fixpoint. This allows, for example, to limit the number of iterations or the
/// time spent, or to report on the progress.
pub fn reachability_with_callback<L: SymbolicLPS, F: FnMut(usize, &LDDFunction) -> ControlFlow<()>>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    options: &ReachabilityOptions,
    timing: &Timing,
    on_iteration: F,
) -> Result<ReachabilityResult, MercError> {
    match options.strategy {
        ExplorationStrategy::Saturation => {
            saturation::saturation_reachability(storage, lts, context, options, timing, on_iteration)
        }
        ExplorationStrategy::BreadthFirst
        | ExplorationStrategy::Chaining
        | ExplorationStrategy::Fixpoint
        | ExplorationStrategy::FixpointChaining => {
            step::step_reachability(storage, lts, context, options, timing, on_iteration)
        }
    }
}

/// Removes from `deadlocks` every state that has a `group` transition into `todo1`, i.e. every state
/// that is not actually a deadlock with respect to `group`. Uses the relational predecessor (the
/// inverse of the relational product) restricted to the current deadlock candidates.
pub(crate) fn remove_states_with_successor(
    todo1: &LDDFunction,
    group: &impl TransitionGroup,
    deadlocks: &LDDFunction,
) -> Result<LDDFunction, MercError> {
    let with_successor = todo1.relational_predecessors(group.relation(), group.meta(), deadlocks)?;
    Ok(deadlocks.diff(&with_successor)?)
}

#[cfg(test)]
mod tests;
