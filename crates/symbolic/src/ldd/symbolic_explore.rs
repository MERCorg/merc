use log::debug;
use log::info;
use log::trace;
use merc_io::LargeFormatter;
#[cfg(feature = "metrics")]
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::SaturationEvent;

use merc_data::DataExpression;
use merc_io::TimeProgress;
use merc_utilities::MercError;
use merc_utilities::Timing;

use merc_lts::TransitionLabel;

use crate::LddDisplay;
use crate::SymbolicLPS;
use crate::TransitionGroup;

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
    /// Ciardo-style node-wise saturation. Every LDD node is brought to a fixed point under the events
    /// confined to its level and below, bottom-up, before it is used as anyone's child.
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
    pub deadlocks: Option<LDDFunction>,
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
    if options.strategy == ExplorationStrategy::Saturation {
        return saturation_reachability(storage, lts, context, options, timing);
    }

    let mut todo = lts.initial_state().clone();
    let mut states = lts.initial_state().clone();
    let mut deadlocks: Option<LDDFunction> = if options.detect_deadlocks {
        Some(storage.with_manager_shared(|m| LDDFunction::empty_set(m))?)
    } else {
        None
    };
    let mut iteration = 0;

    trace!("states = {}", LddDisplay::new(&states));
    let progress = TimeProgress::new(
        |(iteration, num_of_states)| {
            info!(
                "explored {} state(s) after {} iteration(s)",
                LargeFormatter(num_of_states),
                iteration
            );
        },
        1,
    );

    // The chaining and saturation strategies compute an entire fixpoint inside a single step, so the
    // progress above can stay silent for a very long time. This one reports the frontier as it grows.
    let step_progress = TimeProgress::new(
        |(group, num_of_states)| {
            info!(
                "found {} todo state(s) up to transition group {}",
                LargeFormatter(num_of_states),
                group
            );
        },
        10,
    );

    timing.measure("reachability", || {
        while !todo.is_empty() {
            debug!("Iteration {}: todo size = {}", iteration, todo.len());

            let (todo1, step_deadlocks) = step(storage, lts, context, &todo, options, timing, &step_progress)?;

            trace!("todo1 = {}", LddDisplay::new(&todo1));

            todo = todo1.minus(&states)?;
            states = states.union(&todo)?;

            if let Some(accumulated) = &mut deadlocks {
                *accumulated = accumulated.union(&step_deadlocks)?;
            }

            #[cfg(feature = "metrics")]
            {
                let nodes = storage.with_manager_shared(|m| m.num_inner_nodes());
                info!("iteration {iteration}: manager has {} inner node(s)", LargeFormatter(nodes));
                oxidd::ldd::print_stats();
            }

            if progress.is_due() {
                progress.print((iteration, states.len()));
            }

            iteration += 1;
        }

        Ok(ReachabilityResult { states, deadlocks })
    })
}

/// Performs reachability via repeated [`LDDFunction::saturate`] calls over the
/// growing state set, rather than the whole-set fixpoint schedules the other
/// strategies use.
///
/// **On-the-fly relation learning.** 
/// 
/// Inside a single `saturate` call no new local value can appear: firing only
/// ever installs values already present on the write side of an already-learned
/// relation:
///
/// ```text
/// states = initial; epoch = 0
/// loop:
///   changed = for each group: group.learn_successors(context, storage, &states, options.cached)
///   next = states.saturate(&events, num_levels, epoch)   // epoch: see below
///   if changed: epoch += 1
///   if next == states: break
///   states = next
/// ```
///
/// This is a coarse-grained version of the paper's per-local-state `Confirm`
/// (§4): learning is triggered per whole state set per group rather than per
/// single newly-touched local value, so it does strictly more enumeration work
/// than necessary. It is correct and terminating regardless.
///
/// **`epoch`.** 
/// 
/// `saturate`'s `Saturate`/`SatRecFire` cache entries are keyed on node
/// identity, which does not change when a group's relation grows — a node
/// cached as saturated under an earlier, smaller relation would otherwise be
/// silently (and wrongly) reused as if it still were, so `epoch` must be bumped
/// whenever any group's relation actually grew since the last round.
fn saturation_reachability<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    options: &ReachabilityOptions,
    timing: &Timing,
) -> Result<ReachabilityResult, MercError> {
    let progress = TimeProgress::new(
        |(iteration, num_of_states)| {
            info!(
                "explored {} state(s) after {} iteration(s)",
                LargeFormatter(num_of_states),
                iteration
            );
        },
        1,
    );

    timing.measure("reachability", || {
        let mut states = lts.initial_state().clone();
        let mut epoch: u32 = 0;
        let mut round: u32 = 0;

        loop {
            // Only bump `epoch` when a group's relation actually grew this round.
            let mut relation_changed = false;
            for group in lts.transition_groups_mut() {
                let before = group.relation().clone();
                group.learn_successors(context, storage, &states, options.cached)?;
                if *group.relation() != before {
                    relation_changed = true;
                }
            }

            let events = saturation_events(lts);
            let num_levels = vector_length(&states);

            let next = states.saturate(&events, num_levels, epoch)?;
            if relation_changed {
                epoch += 1;
            }
            round += 1;

            progress.print((round, states.len()));

            #[cfg(feature = "metrics")]
            oxidd::ldd::print_stats();

            let fixpoint = next == states;
            states = next;
            if fixpoint {
                break;
            }
        }

        let deadlocks = if options.detect_deadlocks {
            let mut candidates = states.clone();
            for group in lts.transition_groups() {
                candidates = remove_states_with_successor(&states, group, &candidates)?;
            }
            Some(candidates)
        } else {
            None
        };

        Ok(ReachabilityResult { states, deadlocks })
    })
}

/// Builds the [`SaturationEvent`]s used by [`node_saturation_reachability`] from `lts`'s transition
/// groups, in the same order as [`SymbolicLPS::transition_groups`]. A group that neither reads nor
/// writes any position is skipped: its relation is the identity and it has no well-defined
/// `top`/`bot`, so it contributes nothing to saturation.
fn saturation_events<L: SymbolicLPS>(lts: &L) -> Vec<SaturationEvent> {
    let graph = lts.dependency_graph();

    lts.transition_groups()
        .iter()
        .zip(graph.relations())
        .filter_map(|(group, relation)| {
            let top = relation.top()?;
            let bot = relation.bot()?;

            // `meta_at_top` is the group's meta LDD descended `top` times, so that its own root
            // describes state position `top` (see [`LDDFunction::saturate`]).
            let mut meta_at_top = group.meta().clone();
            for _ in 0..top {
                let (_, down, _) = meta_at_top
                    .node()
                    .expect("a group's meta must have at least `top + 1` levels");
                meta_at_top = down;
            }

            Some(SaturationEvent {
                relation: group.relation().clone(),
                meta_at_top,
                top: top as u32,
                bot: bot as u32,
            })
        })
        .collect()
}

/// Returns the length of the vectors in `set`, by walking down its leftmost spine. Every vector in
/// `set` is assumed to have the same length, as required by [`SymbolicLPS`].
fn vector_length(set: &LDDFunction) -> u32 {
    let mut level = set.clone();
    let mut length = 0;
    while let Some((_, down, _)) = level.node() {
        length += 1;
        level = down;
    }
    length
}

/// Performs a single exploration step from the frontier `todo`.
///
/// Returns `(todo1, deadlocks)`: the states reachable this step (the caller subtracts the already
/// visited states) and, when [ReachabilityOptions::detect_deadlocks] is set, the subset of `todo`
/// with no outgoing transition in any group. The transition relations are learned on the fly.
///
/// For the chaining and saturation strategies a single step is a fixpoint computation that can take
/// arbitrarily long, so `progress` reports `(group, frontier size)` while that fixpoint is computed.
fn step<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    todo: &LDDFunction,
    options: &ReachabilityOptions,
    timing: &Timing,
    progress: &TimeProgress<(usize, usize)>,
) -> Result<(LDDFunction, LDDFunction), MercError> {
    // We only print a message when this step takes a significant amount of time.
    progress.reset();

    let chaining = matches!(
        options.strategy,
        ExplorationStrategy::Chaining | ExplorationStrategy::FixpointChaining
    );
    let fixpoint = matches!(
        options.strategy,
        ExplorationStrategy::Fixpoint | ExplorationStrategy::FixpointChaining
    );
    let detect_deadlocks = options.detect_deadlocks;

    let groups = lts.transition_groups_mut();

    // Potential deadlocks start as the whole frontier; a state is removed as soon as a group is found
    // that takes it to a successor. Only tracked when requested.
    let mut deadlocks = if detect_deadlocks {
        todo.clone()
    } else {
        storage.with_manager_shared(|m| LDDFunction::empty_set(m))?
    };

    if !fixpoint {
        // Regular breadth-first, or chaining where successors found by earlier groups feed later groups.
        let mut todo1 = if chaining {
            todo.clone()
        } else {
            storage.with_manager_shared(|m| LDDFunction::empty_set(m))?
        };

        for (i, transition) in groups.iter_mut().enumerate() {
            trace!("Learning successors for transition group {}:", i);
            let source = if chaining { todo1.clone() } else { todo.clone() };
            timing.measure(&format!("learn_successors_{}", i), || {
                transition.learn_successors(context, storage, &source, options.cached)
            })?;

            let result = source.relational_product(transition.relation(), transition.meta())?;
            todo1 = todo1.union(&result)?;

            if detect_deadlocks {
                deadlocks = remove_states_with_successor(&todo1, transition, &deadlocks)?;
            }

            // Only chaining accumulates the frontier across groups; for breadth-first every group
            // starts from `todo` again and the per-iteration progress already covers it.
            if chaining && progress.is_due() {
                progress.print((i, todo1.len()));
            }
        }

        Ok((todo1, deadlocks))
    } else {
        // Fixpoint: apply each group to a fixpoint before the next, optionally re-applying earlier
        // groups (chaining) after every group.
        let mut todo1 = todo.clone();

        for i in 0..groups.len() {
            trace!("Learning successors for transition group {}:", i);
            timing.measure(&format!("learn_successors_{}", i), || {
                groups[i].learn_successors(context, storage, &todo1, options.cached)
            })?;

            // Apply group i repeatedly until it no longer adds new states.
            loop {
                let old = todo1.clone();
                let result = todo1.relational_product(groups[i].relation(), groups[i].meta())?;
                todo1 = todo1.union(&result)?;
                if todo1 == old {
                    break;
                }

                if progress.is_due() {
                    progress.print((i, todo1.len()));
                }
            }

            if detect_deadlocks {
                deadlocks = remove_states_with_successor(&todo1, &groups[i], &deadlocks)?;
            }

            // Apply all previously learned groups repeatedly until a fixpoint.
            if chaining {
                loop {
                    let old = todo1.clone();
                    for group in groups.iter().take(i + 1) {
                        let result = todo1.relational_product(group.relation(), group.meta())?;
                        todo1 = todo1.union(&result)?;
                    }
                    if todo1 == old {
                        break;
                    }

                    if progress.is_due() {
                        progress.print((i, todo1.len()));
                    }
                }
            }
        }

        Ok((todo1, deadlocks))
    }
}

/// Removes from `deadlocks` every state that has a `group` transition into `todo1`, i.e. every state
/// that is not actually a deadlock with respect to `group`. Uses the relational predecessor (the
/// inverse of the relational product) restricted to the current deadlock candidates.
fn remove_states_with_successor(
    todo1: &LDDFunction,
    group: &impl TransitionGroup,
    deadlocks: &LDDFunction,
) -> Result<LDDFunction, MercError> {
    let with_successor = todo1.relational_predecessor(group.relation(), group.meta(), deadlocks)?;
    Ok(deadlocks.minus(&with_successor)?)
}

#[cfg(test)]
mod test {
    use merc_utilities::Timing;
    use oxidd::ManagerRef;
    use oxidd::ldd::LDDFunction;
    use oxidd::ldd::RelationProductMeta;
    use oxidd::ldd::Value;

    use crate::ExplorationStrategy;
    use crate::ReachabilityOptions;
    use crate::SylvanTransitionGroup;
    use crate::SymbolicLPS;
    use crate::from_iter;
    use crate::reachability_with_options;
    use crate::read_sylvan;

    /// Explores the `anderson.4` fixture with the given strategy and returns the reachable state count.
    fn explored_count(strategy: ExplorationStrategy) -> usize {
        let ldd_manager = oxidd::ldd::new_manager(2048, 1024, 1);
        let bytes = include_bytes!("../../../../examples/ldd/anderson.4.ldd");
        let mut lts = read_sylvan(&ldd_manager, &mut &bytes[..]).expect("Loading should work correctly");

        let options = ReachabilityOptions {
            strategy,
            detect_deadlocks: false,
            // The groups of a Sylvan fixture are fully explored, so there is nothing to cache.
            cached: false,
            ..ReachabilityOptions::default()
        };
        let mut context = lts.create_context();
        reachability_with_options(&ldd_manager, &mut lts, &mut context, &options, &Timing::new())
            .expect("Reachability should work correctly")
            .states
            .len()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_reachability_strategies_agree() {
        // All strategies must compute the same reachable set, only the convergence speed differs.
        let expected = explored_count(ExplorationStrategy::BreadthFirst);
        assert_eq!(expected, explored_count(ExplorationStrategy::Chaining));
        assert_eq!(expected, explored_count(ExplorationStrategy::Fixpoint));
        assert_eq!(expected, explored_count(ExplorationStrategy::FixpointChaining));
        assert_eq!(expected, explored_count(ExplorationStrategy::Saturation));
    }

    /// Minimal hand-built LTS over a single parameter with transitions `0 -> 1 -> 2`, so the only
    /// reachable deadlock is state `2`.
    struct LineLts {
        initial: LDDFunction,
        groups: Vec<SylvanTransitionGroup>,
    }

    impl SymbolicLPS for LineLts {
        type Group = SylvanTransitionGroup;

        fn initial_state(&self) -> &LDDFunction {
            &self.initial
        }

        fn transition_groups(&self) -> &[Self::Group] {
            &self.groups
        }

        fn transition_groups_mut(&mut self) -> &mut [Self::Group] {
            &mut self.groups
        }

        fn create_context(&self) {}
    }

    fn line_lts(manager: &oxidd::ldd::LDDManagerRef) -> LineLts {
        // One read+write of parameter 0; relation short vectors place the read/write values at the
        // positions reported by `relation_product_meta`.
        let RelationProductMeta {
            meta,
            read_positions,
            write_positions,
        } = manager
            .with_manager_shared(|m| LDDFunction::relation_product_meta(m, &[0], &[0]))
            .expect("meta");
        let transition = |from: Value, to: Value| {
            let mut vector: Vec<Value> = vec![0; read_positions.len() + write_positions.len()];
            vector[read_positions[0]] = from;
            vector[write_positions[0]] = to;
            vector
        };

        let relation = from_iter(manager, [transition(0, 1), transition(1, 2)].iter());
        let group = SylvanTransitionGroup::new(relation, meta, vec![0], vec![0]);

        LineLts {
            initial: from_iter(manager, std::iter::once(&vec![0])),
            groups: vec![group],
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_reachability_detect_deadlocks() {
        for strategy in [
            ExplorationStrategy::BreadthFirst,
            ExplorationStrategy::Chaining,
            ExplorationStrategy::Fixpoint,
            ExplorationStrategy::FixpointChaining,
            ExplorationStrategy::Saturation,
        ] {
            let manager = oxidd::ldd::new_manager(2048, 1024, 1);
            let mut lts = line_lts(&manager);

            let options = ReachabilityOptions {
                strategy,
                detect_deadlocks: true,
                cached: false,
                ..ReachabilityOptions::default()
            };
            let mut context = lts.create_context();
            let result = reachability_with_options(&manager, &mut lts, &mut context, &options, &Timing::new())
                .expect("Reachability should work correctly");

            // States 0, 1, 2 are reachable and only state 2 has no outgoing transition.
            assert_eq!(result.states.len(), 3, "{strategy:?}");
            let deadlocks = result.deadlocks.expect("detect_deadlocks was requested");
            assert_eq!(deadlocks.len(), 1, "{strategy:?}");
        }
    }
}
