//! Saturation (van Dijk, Meijer, van de Pol, *Multi-core on-the-fly saturation*, TACAS 2019), as
//! `lpsreach --recursive-saturation` does, via [`LDDFunction::saturate`], with the transition
//! relations learned inside the saturation call.
//!
//! Before saturation fires a group's relation on a node at the group's top position, the group
//! learns the transitions of the projections of that node that it has not learned yet, see
//! [`OnTheFly`]. A result that saturation memoized before a relation grew misses the new
//! transitions, so the call is repeated, with a new epoch, until a pass neither changes the states
//! nor learns a transition.

use std::ops::ControlFlow;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use log::info;
#[cfg(feature = "metrics")]
use merc_io::LargeFormatter;
use oxidd::Function;
use oxidd::LevelNo;
#[cfg(feature = "metrics")]
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::SaturationEvent;
use oxidd::util::AllocResult;
use parking_lot::Mutex;

use merc_io::TimeProgress;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::GroupLearner;
use crate::LddLenCache;
use crate::SymbolicLPS;
use crate::TransitionGroup;
use crate::ldd_len;

use super::ReachabilityOptions;
use super::ReachabilityResult;
use super::remove_states_with_successor;

/// The epoch of the next saturation call. The results of a call are memoized under its epoch and
/// depend on the relations, so every call gets a new one; the counter is shared by the whole process,
/// so that explorations sharing a manager (with different relations) never share an epoch either.
static NEXT_EPOCH: AtomicU32 = AtomicU32::new(0);

/// Performs reachability with saturation, see the module documentation.
///
/// When the LTS offers a learner that can be used from every worker
/// ([`SymbolicLPS::parallel_group_learner`]), saturation runs on the manager's worker pool and learns
/// from every worker. Otherwise it runs on the calling thread. Groups with fixed relations are
/// saturated without learning.
///
/// One iteration of `on_iteration` is one pass. Learning always caches the domains: without it, a
/// group would enumerate the same projections at every node.
pub(super) fn saturation_reachability<L: SymbolicLPS, F: FnMut(usize, &LDDFunction) -> ControlFlow<()>>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    options: &ReachabilityOptions,
    timing: &Timing,
    mut on_iteration: F,
) -> Result<ReachabilityResult, MercError> {
    let progress = TimeProgress::new(
        |(pass, num_of_states)| {
            info!("explored {} state(s) after {} pass(es)", num_of_states, pass);
        },
        1,
    );

    timing.measure("reachability", || {
        let mut states = lts.initial_state().clone();
        let mut pass = 0;
        let mut complete = true;
        let mut len_cache = LddLenCache::new();

        loop {
            let (next, grew) = saturation_pass(storage, lts, context, &states)?;
            pass += 1;

            let changed = next != states;
            states = next;
            if !changed && !grew && pass > 1 {
                break;
            }

            if progress.is_due() {
                progress.print((pass, ldd_len(&states, &mut len_cache)));
            }

            #[cfg(feature = "metrics")]
            {
                let nodes = storage.with_manager_shared(|m| m.num_inner_nodes());
                info!("pass {pass}: manager has {} inner node(s)", LargeFormatter(nodes));
                oxidd::ldd::print_stats();
            }

            if on_iteration(pass, &states).is_break() {
                // Whether `states` is closed is only known after a pass that learns nothing.
                complete = false;
                break;
            }
        }

        info!("saturation finished after {pass} pass(es)");

        let deadlocks = if !options.detect_deadlocks {
            None
        } else if !complete {
            // The relations only cover the states that were learned from so far, so every state
            // that was not would show up as a deadlock. Nothing is reported instead.
            Some(storage.with_manager_shared(|m| LDDFunction::empty_set(m)))
        } else {
            // The last pass learned at every group's top position for all reachable states, so the
            // relations are complete for them.
            let mut candidates = states.clone();
            for group in lts.transition_groups() {
                candidates = remove_states_with_successor(&states, group, &candidates)?;
            }
            Some(candidates)
        };

        Ok(ReachabilityResult {
            states,
            deadlocks,
            complete,
        })
    })
}

/// Saturates `states` once, learning on the fly, and hands the learned relations back to the groups
/// of `lts`. Returns the result and whether a relation grew.
fn saturation_pass<L: SymbolicLPS>(
    storage: &LDDManagerRef,
    lts: &mut L,
    context: &mut <L::Group as TransitionGroup>::Context,
    states: &LDDFunction,
) -> Result<(LDDFunction, bool), MercError> {
    let (events, groups) = saturation_events(lts);
    let epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);

    let (result, learned) = {
        let lts: &L = lts;
        if let Some(learner) = lts.parallel_group_learner(context, storage) {
            let on_the_fly = OnTheFly::new(&*learner, lts, &events, &groups);
            let result = states.saturate(&events, epoch, |event, set| on_the_fly.relation_for(event, set));
            (result, Some(on_the_fly.finish()))
        } else if let Some(learner) = lts.group_learner(context, storage) {
            let on_the_fly = OnTheFly::new(&*learner, lts, &events, &groups);
            let result = states.saturate_sequential(&events, epoch, |event, set| on_the_fly.relation_for(event, set));
            (result, Some(on_the_fly.finish()))
        } else {
            let relations: Vec<LDDFunction> = groups
                .iter()
                .map(|&group| lts.transition_groups()[group].relation().clone())
                .collect();
            (
                states.saturate(&events, epoch, |event, _| Ok(relations[event].clone())),
                None,
            )
        }
    };

    let mut grew = false;
    if let Some(learned) = learned {
        // Hand back what was learned before reporting an error, so that nothing is lost.
        for (group, relation, domain) in learned.groups {
            lts.transition_groups_mut()[group].set_learned(relation, domain);
        }
        if let Some(error) = learned.error {
            return Err(error);
        }
        grew = learned.grew;
    }

    Ok((result?, grew))
}

/// Returns the saturation events of `lts`'s transition groups, and for every event the index of its
/// group. A group that neither reads nor writes any position is skipped: its relation is the
/// identity, so it cannot add states.
fn saturation_events<L: SymbolicLPS>(lts: &L) -> (Vec<SaturationEvent>, Vec<usize>) {
    let graph = lts.dependency_graph();
    lts.transition_groups()
        .iter()
        .zip(graph.relations())
        .enumerate()
        .filter_map(|(group, (transition, relation))| {
            let top = relation.top()?;
            let event = SaturationEvent {
                meta_at_top: descend(transition.meta(), top),
                top: top as LevelNo,
            };
            Some((event, group))
        })
        .unzip()
}

/// Returns `ldd` descended `levels` times along its first `down` edges, which for a meta LDD is the
/// meta from position `levels` on.
fn descend(ldd: &LDDFunction, levels: usize) -> LDDFunction {
    let mut result = ldd.clone();
    for _ in 0..levels {
        let (_, down, _) = result
            .node()
            .expect("a meta LDD covers every position up to its last one");
        result = down;
    }
    result
}

/// Provides the relations of the saturation events to [`LDDFunction::saturate`], learning the
/// relations of their groups with a [`GroupLearner`] for the nodes the events are about to fire on,
/// see [`Self::relation_for`].
///
/// It keeps the relation and the domain of every event's group during a saturation call, shared by
/// all workers, and hands them back with [`Self::finish`]. It is `Sync` when the group learner is.
struct OnTheFly<'a, G: ?Sized> {
    learner: &'a G,
    events: Vec<EventState>,

    /// The first error of a learning call. After an error nothing more is learned.
    error: OnceLock<MercError>,

    /// Whether a relation grew during the call.
    grew: AtomicBool,
}

/// What an event learns with.
struct EventState {
    /// The index of the group of the event.
    group: usize,

    /// The projection of a set at the event's top position onto the group's read positions, or
    /// `None` when the group reads nothing.
    projection_at_top: Option<LDDFunction>,

    /// The relation and the domain of the group. Both grow together, under the lock.
    learned: Mutex<(LDDFunction, LDDFunction)>,
}

/// What [`OnTheFly::finish`] returns.
struct Learned {
    /// The relation and domain of the group of every event, by group index.
    groups: Vec<(usize, LDDFunction, LDDFunction)>,

    /// Whether a relation grew.
    grew: bool,

    /// The first error of a learning call.
    error: Option<MercError>,
}

impl<'a, G: ?Sized + GroupLearner> OnTheFly<'a, G> {
    /// Learns the groups `groups` of the `events` with `learner`, starting from their current
    /// relations and domains in `lts`.
    fn new<L: SymbolicLPS>(learner: &'a G, lts: &L, events: &[SaturationEvent], groups: &[usize]) -> Self {
        let events = events
            .iter()
            .zip(groups)
            .map(|(event, &index)| {
                let group = &lts.transition_groups()[index];
                let projection = group.projection().expect("only learning groups get a learner");
                let domain = group.domain().expect("only learning groups get a learner");
                EventState {
                    group: index,
                    projection_at_top: (!group.read_indices().is_empty())
                        .then(|| descend(projection, event.top as usize)),
                    learned: Mutex::new((group.relation().clone(), domain.clone())),
                }
            })
            .collect();

        Self {
            learner,
            events,
            error: OnceLock::new(),
            grew: AtomicBool::new(false),
        }
    }

    /// Learns the transitions of `event`'s group for the vectors of `set`, which start at the
    /// event's top position.
    fn try_learn(&self, event: usize, set: &LDDFunction) -> Result<(), MercError> {
        let state = &self.events[event];
        let domain = state.learned.lock().1.clone();
        let projected = match &state.projection_at_top {
            Some(projection) => set.project(projection)?,
            None if set.is_empty() => return Ok(()),
            None => set.with_manager_shared(|manager, _| LDDFunction::empty_vector(manager))?,
        };

        let new = projected.diff(&domain)?;
        if new.is_empty() {
            return Ok(());
        }
        let transitions = self.learner.learn(state.group, &new)?;

        // No lock is held during an LDD operation, which can wait for the pool. When another
        // worker extended the relation in the meantime, unite again with its result.
        loop {
            let (relation, domain) = state.learned.lock().clone();
            let grown_relation = relation.union(&transitions)?;
            let grown_domain = domain.union(&new)?;

            let mut learned = state.learned.lock();
            if learned.0 == relation && learned.1 == domain {
                if grown_relation != relation {
                    self.grew.store(true, Ordering::Relaxed);
                }
                *learned = (grown_relation, grown_domain);
                return Ok(());
            }
        }
    }

    /// Returns the learned relations and domains, whether they grew, and the first error.
    fn finish(self) -> Learned {
        Learned {
            groups: self
                .events
                .into_iter()
                .map(|state| {
                    let (relation, domain) = state.learned.into_inner();
                    (state.group, relation, domain)
                })
                .collect(),
            grew: self.grew.into_inner(),
            error: self.error.into_inner(),
        }
    }
}

impl<G: ?Sized + GroupLearner> OnTheFly<'_, G> {
    /// Learns the transitions of `event`'s group for the vectors of `set`, which start at the
    /// event's top position, and returns the group's relation. Called by saturation right before
    /// the event fires on `set`; an error is kept for [`Self::finish`] and stops further learning.
    fn relation_for(&self, event: usize, set: &LDDFunction) -> AllocResult<LDDFunction> {
        if self.error.get().is_none()
            && let Err(error) = self.try_learn(event, set)
        {
            let _ = self.error.set(error);
        }
        Ok(self.events[event].learned.lock().0.clone())
    }
}
