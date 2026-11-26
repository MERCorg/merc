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

use merc_io::TimeProgress;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::GroupLearner;
use crate::LddLenCache;
use crate::SymbolicLPS;
use crate::TransitionGroup;
use crate::ldd_len;

use super::Explored;
use super::LearnedRelations;
use super::ReachabilityOptions;
use super::remove_states_with_successor;

/// The epoch of the next saturation call. The results of a call are memoized under its epoch and
/// depend on the relations, so every call gets a new one; the counter is shared by the whole process,
/// so that explorations sharing a manager (with different relations) never share an epoch either.
static NEXT_EPOCH: AtomicU32 = AtomicU32::new(0);

/// Performs reachability with saturation, see the module documentation.
///
/// Saturation runs on the manager's worker pool and learns from every worker through `relations`,
/// which must cache the domains: without it, a group would enumerate the same projections at every
/// node. One iteration of `on_iteration` is one pass.
pub(super) fn saturation_reachability<L, T, F>(
    storage: &LDDManagerRef,
    lts: &L,
    relations: &LearnedRelations<'_, T>,
    options: &ReachabilityOptions,
    timing: &Timing,
    mut on_iteration: F,
) -> Result<Explored, MercError>
where
    L: SymbolicLPS,
    T: GroupLearner + Sync,
    F: FnMut(usize, &LDDFunction) -> ControlFlow<()>,
{
    let progress = TimeProgress::new(
        |(pass, num_of_states)| {
            info!("explored {} state(s) after {} pass(es)", num_of_states, pass);
        },
        1,
    );

    let (events, event_groups) = saturation_events(lts, relations);

    // The groups without an event read and write nothing. They cannot add states, but their
    // transitions (self-loops) belong to the relation, so they are learned outside saturation.
    let without_event: Vec<usize> = (0..lts.transition_groups().len())
        .filter(|group| !event_groups.iter().any(|event| event.group == *group))
        .collect();

    timing.measure("reachability", || {
        let mut states = lts.initial_state().clone();
        let mut pass = 0;
        let mut complete = true;
        let mut len_cache = LddLenCache::new();

        loop {
            // Such a group projects every state onto the empty vector, so with the cached domain it
            // learns at most once.
            let mut grew = false;
            for &group in &without_event {
                grew |= relations.learn(group, &states)?;
            }

            let (next, grew_saturating) = saturation_pass(relations, &events, &event_groups, &states)?;
            let grew = grew || grew_saturating;
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
            for group in 0..lts.transition_groups().len() {
                candidates = remove_states_with_successor(
                    &states,
                    &relations.relation(group),
                    relations.meta(group),
                    &candidates,
                )?;
            }
            Some(candidates)
        };

        Ok(Explored {
            states,
            deadlocks,
            complete,
        })
    })
}

/// Saturates `states` once, learning on the fly. Returns the result and whether a relation grew.
fn saturation_pass<T: GroupLearner + Sync>(
    relations: &LearnedRelations<'_, T>,
    events: &[SaturationEvent],
    event_groups: &[EventGroup],
    states: &LDDFunction,
) -> Result<(LDDFunction, bool), MercError> {
    let epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);

    // Saturation asks for the relations from every worker of the pool.
    let on_the_fly = OnTheFly {
        relations,
        events: event_groups,
        error: OnceLock::new(),
        grew: AtomicBool::new(false),
    };
    let result = states.saturate(events, epoch, |event, set| on_the_fly.relation_for(event, set));

    if let Some(error) = on_the_fly.error.into_inner() {
        return Err(error);
    }
    Ok((result?, on_the_fly.grew.into_inner()))
}

/// The group of a saturation event, and what it learns with.
struct EventGroup {
    /// The index of the group of the event.
    group: usize,

    /// The projection of a set at the event's top position onto the group's read positions, or
    /// `None` when the group reads nothing or nothing is learned.
    projection_at_top: Option<LDDFunction>,
}

/// Returns the saturation events of `lts`'s transition groups, and for every event its group. A
/// group that neither reads nor writes any position is skipped: its relation is the identity, so it
/// cannot add states.
fn saturation_events<L: SymbolicLPS, T: GroupLearner>(
    lts: &L,
    relations: &LearnedRelations<'_, T>,
) -> (Vec<SaturationEvent>, Vec<EventGroup>) {
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
            let projection_at_top = relations
                .projection(group)
                .filter(|_| !transition.read_indices().is_empty())
                .map(|projection| descend(projection, top));
            Some((
                event,
                EventGroup {
                    group,
                    projection_at_top,
                },
            ))
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
/// relations of their groups for the nodes the events are about to fire on, see
/// [`Self::relation_for`]. It is `Sync` when the group learner is.
struct OnTheFly<'r, 'a, T> {
    relations: &'r LearnedRelations<'a, T>,
    events: &'r [EventGroup],

    /// The first error of a learning call. After an error nothing more is learned.
    error: OnceLock<MercError>,

    /// Whether a relation grew during the call.
    grew: AtomicBool,
}

impl<T: GroupLearner> OnTheFly<'_, '_, T> {
    /// Learns the transitions of `event`'s group for the vectors of `set`, which start at the
    /// event's top position, and returns the group's relation. Called by saturation right before
    /// the event fires on `set`; an error is kept for the caller and stops further learning.
    fn relation_for(&self, event: usize, set: &LDDFunction) -> AllocResult<LDDFunction> {
        let state = &self.events[event];
        if self.relations.is_learning() && self.error.get().is_none() {
            match self.try_learn(state, set) {
                Ok(true) => self.grew.store(true, Ordering::Relaxed),
                Ok(false) => {}
                Err(error) => {
                    let _ = self.error.set(error);
                }
            }
        }
        Ok(self.relations.relation(state.group))
    }

    /// Learns the transitions of the group of `state` for the vectors of `set`, which start at the
    /// event's top position. Returns whether the relation grew.
    fn try_learn(&self, state: &EventGroup, set: &LDDFunction) -> Result<bool, MercError> {
        let projected = match &state.projection_at_top {
            Some(projection) => set.project(projection)?,
            None if set.is_empty() => return Ok(false),
            None => set.with_manager_shared(|manager, _| LDDFunction::empty_vector(manager)),
        };
        self.relations.learn_projected(state.group, &projected)
    }
}
