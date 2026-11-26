use std::fmt;

use merc_data::DataVariable;
use merc_utilities::MercError;
use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;

use crate::DependencyGraph;
use crate::Relation;

/// A generic trait for symbolic LDD-based state-space exploration.
///
/// Any type with an initial state and transition groups can be explored via [crate::reachability].
/// The relations of the groups are either given up front ([`TransitionGroup::relation`]), or learned
/// during exploration with the [`GroupLearner`] of [`Self::group_learner`]. Exploration keeps the
/// learned relations itself and returns them in [`crate::ReachabilityResult::groups`], so the LTS
/// is never changed by exploring it.
pub trait SymbolicLPS {
    /// The transition group type backing this LTS.
    type Group: TransitionGroup;

    /// The learner returned by [`Self::group_learner`]; [`NoLearner`] when the relations are fixed.
    type Learner<'a>: GroupLearner + Sync
    where
        Self: 'a;

    /// Returns the LDD representing the initial state(s).
    fn initial_state(&self) -> &LDDFunction;

    /// Returns the transition groups.
    fn transition_groups(&self) -> &[Self::Group];

    /// Returns a learner for the transition groups, or `None` when [`TransitionGroup::relation`] is
    /// already the complete relation of every group.
    ///
    /// Called once per exploration run; the learner keeps its per-run state (such as an enumeration
    /// context per worker) for that run. It can be used from every worker of `storage`, also
    /// concurrently and from inside an operation such as saturation.
    fn group_learner<'a>(&'a self, storage: &'a LDDManagerRef) -> Option<Self::Learner<'a>>;

    /// Computes the dependency graph from the read/write indices of each transition group.
    fn dependency_graph(&self) -> DependencyGraph {
        let relations = self
            .transition_groups()
            .iter()
            .map(|group| {
                Relation::new(
                    group.read_indices().iter().map(|i| *i as usize).collect(),
                    group.write_indices().iter().map(|i| *i as usize).collect(),
                )
            })
            .collect();

        DependencyGraph::new(relations)
    }
}

/// Learns the transitions of the transition groups of a [`SymbolicLPS`] for projected states, see
/// [`SymbolicLPS::group_learner`].
pub trait GroupLearner {
    /// Returns the transitions of group `group` from the projected states `projected`, which are
    /// short vectors over the read indices of the group, in the layout of its relation.
    ///
    /// Can be called from inside saturation, on any worker of the manager's pool.
    fn learn(&self, group: usize, projected: &LDDFunction) -> Result<LDDFunction, MercError>;
}

/// The [`SymbolicLPS::Learner`] of an LTS whose relations are fixed; it has no values, so
/// [`SymbolicLPS::group_learner`] can only return `None`.
pub enum NoLearner {}

impl GroupLearner for NoLearner {
    fn learn(&self, _group: usize, _projected: &LDDFunction) -> Result<LDDFunction, MercError> {
        match *self {}
    }
}

/// A single LDD-based transition group for a set of summands (short vector encoding).
pub trait TransitionGroup: fmt::Debug {
    /// Returns the transition relation T' -> U' for this summand group, as known before
    /// exploration: the complete relation for a group with a fixed relation, and the empty set for
    /// a group that is learned with a [`GroupLearner`].
    fn relation(&self) -> &LDDFunction;

    /// Returns the read indices for this summand group.
    fn read_indices(&self) -> &[u32];

    /// Returns the write indices for this summand group.
    fn write_indices(&self) -> &[u32];

    /// Returns the action label index for this summand group, if any.
    fn action_label_index(&self) -> Option<usize>;

    /// Returns the meta LDD required to perform the relational product.
    fn meta(&self) -> &LDDFunction;
}

/// A short vector transition relation for a group of summands.
///
/// A short transition vector is part of a transition relation T -> U, where we
/// store T' -> U' with T' being the projection of T on the read parameters and
/// U' the projection of U on the write parameters, as a LDD. Formally,
///
/// (t, u) in (T -> U)  iff  (t', u') in (T' -> U') where t' and u' are the projections
///     of t and u on the read and write parameters respectively.
pub struct SummandGroup {
    read_parameters: Vec<DataVariable>,
    read_parameter_indices: Vec<Value>,

    write_parameters: Vec<DataVariable>,
    write_parameter_indices: Vec<Value>,

    relation: LDDFunction,
    meta: LDDFunction,
    action_label_index: usize,
}

impl SummandGroup {
    /// Creates a new summand group.
    ///
    /// Fails if a read or write parameter is not found in `parameters`.
    pub fn new(
        manager: &LDDManagerRef,
        parameters: &[DataVariable],
        read_parameters: Vec<DataVariable>,
        write_parameters: Vec<DataVariable>,
        relation: LDDFunction,
    ) -> Result<Self, MercError> {
        // Build (position-in-parameters, variable) pairs, then sort by position so
        // read_parameters[i] always corresponds to read_parameter_indices[i].
        let mut read_pairs: Vec<(Value, DataVariable)> = read_parameters
            .into_iter()
            .map(|var| {
                parameters
                    .iter()
                    .position(|p| p == &var)
                    .ok_or(format!("Cannot find read parameter {var:?}"))
                    .map(|pos| (pos as Value, var))
            })
            .collect::<Result<_, _>>()?;

        let mut write_pairs: Vec<(Value, DataVariable)> = write_parameters
            .into_iter()
            .map(|var| {
                parameters
                    .iter()
                    .position(|p| p == &var)
                    .ok_or(format!("Cannot find write parameter {var:?}"))
                    .map(|pos| (pos as Value, var))
            })
            .collect::<Result<_, _>>()?;

        read_pairs.sort_unstable_by_key(|(idx, _)| *idx);
        write_pairs.sort_unstable_by_key(|(idx, _)| *idx);

        let (read_parameter_indices, read_parameters): (Vec<Value>, Vec<DataVariable>) = read_pairs.into_iter().unzip();
        let (write_parameter_indices, write_parameters): (Vec<Value>, Vec<DataVariable>) =
            write_pairs.into_iter().unzip();

        let meta = manager
            .with_manager_shared(|m| LDDFunction::relation_meta(m, &read_parameter_indices, &write_parameter_indices))?
            .meta;
        let action_label_index = read_parameters.len() + write_parameters.len();

        Ok(Self {
            read_parameters,
            read_parameter_indices,
            write_parameters,
            write_parameter_indices,
            relation,
            meta,
            action_label_index,
        })
    }

    /// Returns the read parameters for this summand group.
    pub fn read_parameters(&self) -> &[DataVariable] {
        &self.read_parameters
    }

    /// Returns the write parameters for this summand group.
    pub fn write_parameters(&self) -> &[DataVariable] {
        &self.write_parameters
    }
}

impl TransitionGroup for SummandGroup {
    fn relation(&self) -> &LDDFunction {
        &self.relation
    }

    fn meta(&self) -> &LDDFunction {
        &self.meta
    }

    fn read_indices(&self) -> &[u32] {
        &self.read_parameter_indices
    }

    fn write_indices(&self) -> &[u32] {
        &self.write_parameter_indices
    }

    fn action_label_index(&self) -> Option<usize> {
        Some(self.action_label_index)
    }
}

impl fmt::Debug for SummandGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} -> {:?}",
            self.read_parameter_indices, self.write_parameter_indices
        )
    }
}
