//! The relations of the transition groups during one exploration run, see [`LearnedRelations`].

use std::fmt;

use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use parking_lot::Mutex;

use merc_utilities::MercError;

use crate::GroupLearner;
use crate::SymbolicLPS;
use crate::TransitionGroup;

/// The relations of the transition groups of a [`SymbolicLPS`] during one exploration run, learned
/// with its [`GroupLearner`] if it has one.
///
/// This is the only place where transitions are learned, for every strategy. Every method takes
/// `&self` and can be called from every worker, also concurrently.
pub(super) struct LearnedRelations<'a, T> {
    /// The relational product meta of every group.
    metas: Vec<LDDFunction>,
    state: State<'a, T>,
}

enum State<'a, T> {
    /// There is no learner: the relations of the groups as given, read without a lock.
    Fixed(Vec<LDDFunction>),

    /// The relations are learned with `learner`.
    Learning {
        learner: &'a T,

        /// Whether the domains are cached: then only the projections that were not learned from
        /// before are learned.
        cached: bool,

        groups: Vec<GroupState>,
    },
}

/// The learning state of one group.
struct GroupState {
    /// The projection meta that selects the read indices of the group from a full state.
    projection: LDDFunction,

    /// The relation and its domain, the projected states whose transitions are in the relation.
    /// They grow together. The domain stays empty when the domains are not cached.
    learned: Mutex<(LDDFunction, LDDFunction)>,
}

impl<'a, T: GroupLearner> LearnedRelations<'a, T> {
    /// Starts from the relations of the groups of `lts`, learning with `learner` if there is one.
    ///
    /// When `cached` is set, a group remembers the projections of the states it has already learned
    /// from (its *domain*) and only learns the successors of the projections it has not seen before.
    /// Since the relation is projected on the read parameters, a state whose read projection is
    /// already in the domain cannot contribute a transition that is not in the relation yet, so this
    /// does not change the learned relation; it only trades memory for enumeration work.
    pub fn new<L: SymbolicLPS>(
        storage: &LDDManagerRef,
        lts: &L,
        learner: Option<&'a T>,
        cached: bool,
    ) -> Result<Self, MercError> {
        let groups = lts.transition_groups();
        let metas = groups.iter().map(|group| group.meta().clone()).collect();

        let state = match learner {
            None => State::Fixed(groups.iter().map(|group| group.relation().clone()).collect()),
            Some(learner) => State::Learning {
                learner,
                cached,
                groups: groups
                    .iter()
                    .map(|group| -> Result<GroupState, MercError> {
                        let (projection, domain) = storage.with_manager_shared(|m| -> Result<_, MercError> {
                            Ok((
                                LDDFunction::projection_meta(m, group.read_indices())?,
                                LDDFunction::empty_set(m),
                            ))
                        })?;
                        Ok(GroupState {
                            projection,
                            learned: Mutex::new((group.relation().clone(), domain)),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            },
        };

        Ok(Self { metas, state })
    }

    /// Returns whether the relations are learned, i.e. whether there is a learner.
    pub fn is_learning(&self) -> bool {
        matches!(self.state, State::Learning { .. })
    }

    /// Returns the current relation of `group`.
    pub fn relation(&self, group: usize) -> LDDFunction {
        match &self.state {
            State::Fixed(relations) => relations[group].clone(),
            State::Learning { groups, .. } => groups[group].learned.lock().0.clone(),
        }
    }

    /// Returns the relational product meta of `group`.
    pub fn meta(&self, group: usize) -> &LDDFunction {
        &self.metas[group]
    }

    /// Returns the projection meta that selects the read indices of `group` from a full state, or
    /// `None` when nothing is learned.
    pub fn projection(&self, group: usize) -> Option<&LDDFunction> {
        match &self.state {
            State::Fixed(_) => None,
            State::Learning { groups, .. } => Some(&groups[group].projection),
        }
    }

    /// Learns the transitions of `group` from the full states `states`, see [`Self::learn_projected`].
    pub fn learn(&self, group: usize, states: &LDDFunction) -> Result<bool, MercError> {
        let Some(projection) = self.projection(group) else {
            return Ok(false);
        };
        self.learn_projected(group, &states.project(projection)?)
    }

    /// Learns the transitions of `group` from the projected states `projected` (over the read
    /// indices of the group) and unites them into its relation. With caching, only the projected
    /// states that are not in the domain are learned. Returns whether the relation grew; without a
    /// learner this does nothing and returns `false`.
    pub fn learn_projected(&self, group: usize, projected: &LDDFunction) -> Result<bool, MercError> {
        let State::Learning {
            learner,
            cached,
            groups,
        } = &self.state
        else {
            return Ok(false);
        };
        let state = &groups[group];

        let new = if *cached {
            let domain = state.learned.lock().1.clone();
            projected.diff(&domain)?
        } else {
            projected.clone()
        };
        if new.is_empty() {
            return Ok(false);
        }
        let transitions = learner.learn(group, &new)?;

        // No lock is held during an LDD operation, which can wait for the pool. When another
        // worker extended the relation in the meantime, unite again with its result. The domain is
        // only extended once the learner succeeded, so a failed call hides no states.
        loop {
            let (relation, domain) = state.learned.lock().clone();
            let grown_relation = relation.union(&transitions)?;
            let grown_domain = if *cached { domain.union(&new)? } else { domain.clone() };

            let mut learned = state.learned.lock();
            if learned.0 == relation && learned.1 == domain {
                let grew = grown_relation != relation;
                *learned = (grown_relation, grown_domain);
                return Ok(grew);
            }
        }
    }

    /// Returns the groups of `lts` with the relations as they are now.
    pub fn into_groups<L: SymbolicLPS>(self, lts: &L) -> Vec<ExploredGroup> {
        let relations: Vec<LDDFunction> = match self.state {
            State::Fixed(relations) => relations,
            State::Learning { groups, .. } => groups.into_iter().map(|state| state.learned.into_inner().0).collect(),
        };

        lts.transition_groups()
            .iter()
            .zip(relations)
            .zip(self.metas)
            .map(|((group, relation), meta)| ExploredGroup {
                relation,
                meta,
                read_indices: group.read_indices().to_vec(),
                write_indices: group.write_indices().to_vec(),
                action_label_index: group.action_label_index(),
            })
            .collect()
    }
}

/// A transition group of a [`SymbolicLPS`] with the relation that exploration learned for it, see
/// [`crate::ReachabilityResult::groups`].
#[derive(Clone)]
pub struct ExploredGroup {
    relation: LDDFunction,
    meta: LDDFunction,
    read_indices: Vec<u32>,
    write_indices: Vec<u32>,
    action_label_index: Option<usize>,
}

impl TransitionGroup for ExploredGroup {
    fn relation(&self) -> &LDDFunction {
        &self.relation
    }

    fn read_indices(&self) -> &[u32] {
        &self.read_indices
    }

    fn write_indices(&self) -> &[u32] {
        &self.write_indices
    }

    fn action_label_index(&self) -> Option<usize> {
        self.action_label_index
    }

    fn meta(&self) -> &LDDFunction {
        &self.meta
    }
}

// `LDDFunction` does not implement `Debug`, so this cannot be derived.
impl fmt::Debug for ExploredGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} -> {:?}", self.read_indices, self.write_indices)
    }
}
