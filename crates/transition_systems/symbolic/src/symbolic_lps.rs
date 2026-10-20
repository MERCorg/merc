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
pub trait SymbolicLPS {
    /// The transition group type backing this LTS.
    type Group: TransitionGroup;

    /// Returns the LDD representing the initial state(s).
    fn initial_state(&self) -> &LDDFunction;

    /// Returns the transition groups.
    fn transition_groups(&self) -> &[Self::Group];

    /// Returns the transition groups (mutable), needed for on-the-fly learning.
    fn transition_groups_mut(&mut self) -> &mut [Self::Group];

    /// Creates a fresh learning context, threaded through [TransitionGroup::learn_successors].
    ///
    /// The context owns the per-run mutable state shared by all groups (e.g. an enumeration backend
    /// and value interning), so the groups themselves need no interior mutability. It is created once
    /// per reachability run and must outlive every learning call.
    fn create_context(&self) -> <Self::Group as TransitionGroup>::Context;

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

/// A single LDD-based transition group for a set of summands (short vector encoding).
pub trait TransitionGroup: fmt::Debug {
    /// Per-run mutable state threaded into [Self::learn_successors]; `()` when nothing is learned.
    type Context;

    /// Returns the transition relation T' -> U' for this summand group.
    fn relation(&self) -> &LDDFunction;

    /// Returns the read indices for this summand group.
    fn read_indices(&self) -> &[u32];

    /// Returns the write indices for this summand group.
    fn write_indices(&self) -> &[u32];

    /// Returns the action label index for this summand group, if any.
    fn action_label_index(&self) -> Option<usize>;

    /// Returns the meta LDD required to perform the relational product.
    fn meta(&self) -> &LDDFunction;

    /// Learns the successors of `todo` and incorporates them into [Self::relation].
    ///
    /// When `cached` is set the group remembers the projections of the states it has already learned
    /// from (its *domain*) and only learns the successors of the projections it has not seen before.
    /// Since the relation is projected on the read parameters, a state whose read projection is
    /// already in the domain cannot contribute a transition that is not in the relation yet, so this
    /// does not change the learned relation — it only trades memory for enumeration work. Groups that
    /// have nothing to learn ignore the flag.
    fn learn_successors(
        &mut self,
        context: &mut Self::Context,
        manager: &LDDManagerRef,
        todo: &LDDFunction,
        cached: bool,
    ) -> Result<(), MercError>;
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
            .with_manager_shared(|m| {
                LDDFunction::relation_product_meta(m, &read_parameter_indices, &write_parameter_indices)
            })?
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
    type Context = ();

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

    fn learn_successors(
        &mut self,
        _context: &mut (),
        _storage: &LDDManagerRef,
        _todo: &LDDFunction,
        _cached: bool,
    ) -> Result<(), MercError> {
        Ok(())
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
