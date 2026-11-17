//! The breadth-first, chaining and fixpoint strategies: an outer loop over the frontier, where every
//! iteration applies the transition groups once ([`step`]).

use std::ops::ControlFlow;

use log::debug;
use log::info;
use log::trace;
#[cfg(feature = "metrics")]
use merc_io::LargeFormatter;
use oxidd::HasWorkers;
#[cfg(feature = "metrics")]
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::WorkerPool;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;

use merc_io::TimeProgress;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::GroupLearner;
use crate::LddDisplay;
use crate::LddLenCache;
use crate::SatCount;
use crate::SymbolicLPS;
use crate::TransitionGroup;
use crate::ldd_len;
use crate::parallel_map;
use crate::parallel_map_reduce;

use super::ExplorationStrategy;
use super::ReachabilityOptions;
use super::ReachabilityResult;
use super::remove_states_with_successor;

/// Performs reachability with the breadth-first, chaining or fixpoint strategies, see
/// [`super::reachability_with_callback`].
pub(super) fn step_reachability<L: SymbolicLPS, F: FnMut(usize, &LDDFunction) -> ControlFlow<()>>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    options: &ReachabilityOptions,
    timing: &Timing,
    mut on_iteration: F,
) -> Result<ReachabilityResult, MercError> {
    let mut todo = lts.initial_state().clone();
    let mut states = lts.initial_state().clone();
    let mut deadlocks: Option<LDDFunction> = if options.detect_deadlocks {
        Some(storage.with_manager_shared(|m| LDDFunction::empty_set(m)))
    } else {
        None
    };
    let mut iteration = 0;
    let mut complete = true;
    let mut len_cache = LddLenCache::new();

    trace!("states = {}", LddDisplay::new(&states));
    let progress = TimeProgress::new(
        |(iteration, num_of_states)| {
            info!("explored {} state(s) after {} iteration(s)", num_of_states, iteration);
        },
        1,
    );

    // The chaining and saturation strategies compute an entire fixpoint inside a single step, so the
    // progress above can stay silent for a very long time. This one reports the frontier as it grows.
    let step_progress = TimeProgress::new(
        |(group, num_of_states)| {
            info!("found {} todo state(s) up to transition group {}", num_of_states, group);
        },
        10,
    );

    timing.measure("reachability", || {
        while !todo.is_empty() {
            debug!(
                "Iteration {}: todo size = {}",
                iteration,
                ldd_len(&todo, &mut len_cache)
            );

            let (todo1, step_deadlocks) = step(storage, lts, context, &todo, options, timing, &step_progress)?;

            trace!("todo1 = {}", LddDisplay::new(&todo1));

            todo = timing.measure("bf_diff", || todo1.diff(&states))?;
            states = timing.measure("bf_union_states", || states.union(&todo))?;

            if let Some(accumulated) = &mut deadlocks {
                *accumulated = accumulated.union(&step_deadlocks)?;
            }

            #[cfg(feature = "metrics")]
            {
                let nodes = storage.with_manager_shared(|m| m.num_inner_nodes());
                info!(
                    "iteration {iteration}: manager has {} inner node(s)",
                    LargeFormatter(nodes)
                );
                oxidd::ldd::print_stats();
            }

            if progress.is_due() {
                progress.print((iteration, ldd_len(&states, &mut len_cache)));
            }

            iteration += 1;

            if on_iteration(iteration, &states).is_break() {
                // The iteration that is stopped can also be the one that found nothing new.
                complete = todo.is_empty();
                break;
            }
        }

        Ok(ReachabilityResult {
            states,
            deadlocks,
            complete,
        })
    })
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
    progress: &TimeProgress<(usize, SatCount)>,
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

    // The order in which the groups are applied; it only matters when groups feed each other.
    let order = if chaining {
        chaining_order(groups)
    } else {
        (0..groups.len()).collect()
    };

    // Potential deadlocks start as the whole frontier; a state is removed as soon as a group is found
    // that takes it to a successor. Only tracked when requested.
    let mut deadlocks = if detect_deadlocks {
        todo.clone()
    } else {
        storage.with_manager_shared(|m| LDDFunction::empty_set(m))
    };

    if !fixpoint && !chaining && !detect_deadlocks {
        return step_parallel_groups(storage, lts, context, todo, options, timing);
    }

    if !fixpoint {
        // Regular breadth-first, or chaining where successors found by earlier groups feed later groups.
        let mut todo1 = if chaining {
            todo.clone()
        } else {
            storage.with_manager_shared(|m| LDDFunction::empty_set(m))
        };

        for &i in &order {
            let transition = &mut groups[i];
            trace!("Learning successors for transition group {}:", i);
            let source = if chaining { todo1.clone() } else { todo.clone() };
            timing.measure(&format!("learn_successors_{}", i), || {
                transition.learn_successors(context, storage, &source, options.cached)
            })?;

            let result = timing.measure("bf_relprod", || {
                source.relational_successors(transition.relation(), transition.meta())
            })?;
            todo1 = timing.measure("bf_union_todo", || todo1.union(&result))?;

            if detect_deadlocks {
                deadlocks = remove_states_with_successor(&todo1, transition, &deadlocks)?;
            }

            // Only chaining accumulates the frontier across groups; for breadth-first every group
            // starts from `todo` again and the per-iteration progress already covers it.
            if chaining && progress.is_due() {
                progress.print((i, ldd_len(&todo1, &mut LddLenCache::new())));
            }
        }

        Ok((todo1, deadlocks))
    } else {
        // Fixpoint: apply each group to a fixpoint before the next, optionally re-applying earlier
        // groups (chaining) after every group.
        let mut todo1 = todo.clone();

        for (applied, &i) in order.iter().enumerate() {
            trace!("Learning successors for transition group {}:", i);
            timing.measure(&format!("learn_successors_{}", i), || {
                groups[i].learn_successors(context, storage, &todo1, options.cached)
            })?;

            // Apply group i repeatedly until it no longer adds new states.
            loop {
                let old = todo1.clone();
                let result = todo1.relational_successors(groups[i].relation(), groups[i].meta())?;
                todo1 = todo1.union(&result)?;
                if todo1 == old {
                    break;
                }

                if progress.is_due() {
                    progress.print((i, ldd_len(&todo1, &mut LddLenCache::new())));
                }
            }

            if detect_deadlocks {
                deadlocks = remove_states_with_successor(&todo1, &groups[i], &deadlocks)?;
            }

            // Apply all previously learned groups repeatedly until a fixpoint.
            if chaining {
                loop {
                    let old = todo1.clone();
                    for group in order[..=applied].iter().map(|&j| &groups[j]) {
                        let result = todo1.relational_successors(group.relation(), group.meta())?;
                        todo1 = todo1.union(&result)?;
                    }
                    if todo1 == old {
                        break;
                    }

                    if progress.is_due() {
                        progress.print((i, ldd_len(&todo1, &mut LddLenCache::new())));
                    }
                }
            }
        }

        Ok((todo1, deadlocks))
    }
}

/// Returns the order in which chaining applies the `groups`: by their top, the first position they
/// read or write, from the deepest to the highest, as saturation fires them bottom-up. The sort is
/// stable, so groups with the same top keep their order. Groups that read and write nothing come
/// last; they cannot add states.
///
/// With chaining every group is applied to the states found by the groups before it. A group only
/// changes the positions from its top on, so applying the groups that start deepest first lets the
/// groups above them work on states that are already closed under the changes below.
fn chaining_order<G: TransitionGroup>(groups: &[G]) -> Vec<usize> {
    let top = |group: &G| group.read_indices().iter().chain(group.write_indices()).min().copied();
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(top(&groups[i])));
    order
}

/// A breadth-first step that applies the groups in parallel. Every group reads
/// the same `todo`, so the projections of all groups are computed in parallel,
/// then the groups learn, and then the relational products of all groups and
/// their unions run as a balanced tree of `join`s ([`parallel_map_reduce`]).
///
/// The groups learn at the same time through the
/// [`SymbolicLPS::group_learner`], as saturation does, or one after
/// another (each in parallel itself) if the backend has none.
fn step_parallel_groups<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    todo: &LDDFunction,
    options: &ReachabilityOptions,
    timing: &Timing,
) -> Result<(LDDFunction, LDDFunction), MercError> {
    let workers = storage.workers();
    let cached = options.cached;
    let empty = storage.with_manager_shared(|m| LDDFunction::empty_set(m));

    // The projections of all groups that learn and the projections
    // that are not in the domain yet, in parallel.
    let inputs: Vec<(Option<LDDFunction>, Option<LDDFunction>)> = lts
        .transition_groups()
        .iter()
        .map(|g| (g.projection().cloned(), g.domain().cloned()))
        .collect();
    let projections: Vec<Option<(LDDFunction, LDDFunction)>> = timing.measure("parallel_project", || {
        workers.install(|| {
            parallel_map(workers, inputs.len(), &|i| -> Result<_, oxidd::util::OutOfMemory> {
                let (Some(projection_ldd), domain) = &inputs[i] else {
                    return Ok(None);
                };
                let projection = todo.project(projection_ldd)?;
                let new = match domain {
                    Some(domain) if cached => projection.diff(domain)?,
                    _ => projection.clone(),
                };
                Ok(Some((projection, new)))
            })
        })
    })?;

    let learned = {
        let lts_ref: &L = lts;
        let current: Vec<(LDDFunction, Option<LDDFunction>)> = lts_ref
            .transition_groups()
            .iter()
            .map(|g| (g.relation().clone(), g.domain().cloned()))
            .collect();
        match lts_ref.group_learner(context, storage) {
            Some(learner) => Some(timing.measure("pg_learn", || {
                workers.install(|| {
                    parallel_map(workers, projections.len(), &|i| -> Result<
                        Option<(LDDFunction, LDDFunction)>,
                        MercError,
                    > {
                        let (Some((projection, new)), Some(domain)) = (&projections[i], &current[i].1) else {
                            return Ok(None);
                        };
                        if new.is_empty() {
                            return Ok(None);
                        }
                        let transitions = learner.learn(i, new)?;
                        let relation = current[i].0.union(&transitions)?;
                        let domain = if cached {
                            domain.union(projection)?
                        } else {
                            domain.clone()
                        };
                        Ok(Some((relation, domain)))
                    })
                })
            })?),
            None => None,
        }
    };

    match learned {
        Some(learned) => {
            for (i, learned) in learned.into_iter().enumerate() {
                if let Some((relation, domain)) = learned {
                    lts.transition_groups_mut()[i].set_learned(relation, domain);
                }
            }
        }
        None => {
            for (i, (group, projection)) in lts.transition_groups_mut().iter_mut().zip(projections).enumerate() {
                timing.measure(&format!("learn_successors_{i}"), || {
                    group.learn_successors_projected(context, storage, todo, projection.map(|p| p.0), cached)
                })?;
            }
        }
    }

    // The relational products of all groups and their union, as a balanced tree.
    let relations: Vec<(LDDFunction, LDDFunction)> = lts
        .transition_groups()
        .iter()
        .map(|g| (g.relation().clone(), g.meta().clone()))
        .collect();
    let todo1 = timing.measure("pg_relprod_union", || {
        workers.install(|| {
            parallel_map_reduce(
                workers,
                0..relations.len(),
                &|i| {
                    let (relation, meta) = &relations[i];
                    todo.relational_successors(relation, meta)
                },
                &|a: LDDFunction, b| a.union(&b),
            )
        })
    })?;

    Ok((todo1, empty))
}
