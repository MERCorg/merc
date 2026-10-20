use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_lts::TransitionLabel;
use oxidd::ldd::LDDFunction;

use crate::SummandGroup;
use crate::SymbolicLPS;
use crate::SymbolicLTS;

/// Represents a symbolic LTS encoded by a disjunctive transition relation and a set of states.
pub struct SymbolicLts<L: TransitionLabel> {
    data_specification: Mcrl2DataSpecification,

    /// The process parameters, in the order used to index the LDD vectors.
    process_parameters: Vec<DataVariable>,
    states: LDDFunction,

    /// A singleton LDD representing the initial state.
    initial_state: LDDFunction,
    summand_groups: Vec<SummandGroup>,

    /// The action labels of the LTS, their position corresponds to the LDD values.
    action_labels: Vec<L>,

    /// The possible values for each process parameter, their position
    /// corresponds to the LDD values.
    parameter_values: Vec<Vec<DataExpression>>,
}

impl<L: TransitionLabel> SymbolicLts<L> {
    /// Creates a new symbolic LTS.
    ///
    /// `states` is the known state space (pass `initial_state.clone()` when the full set is not
    /// yet known, or provide the reachable set computed by [crate::reachability]).
    ///
    /// # Panics
    ///
    /// Panics if `parameter_values` does not have exactly one entry per process parameter.
    pub fn new(
        data_specification: Mcrl2DataSpecification,
        process_parameters: Vec<DataVariable>,
        states: LDDFunction,
        initial_state: LDDFunction,
        summand_groups: Vec<SummandGroup>,
        action_labels: Vec<L>,
        parameter_values: Vec<Vec<DataExpression>>,
    ) -> Self {
        assert_eq!(
            parameter_values.len(),
            process_parameters.len(),
            "parameter_values must have one entry per process parameter"
        );

        Self {
            data_specification,
            process_parameters,
            states,
            initial_state,
            summand_groups,
            action_labels,
            parameter_values,
        }
    }

    /// Returns the data specification of the LTS.
    pub fn data_specification(&self) -> &Mcrl2DataSpecification {
        &self.data_specification
    }

    /// Returns the process parameters, in the order used to index the LDD vectors.
    pub fn process_parameters(&self) -> &[DataVariable] {
        &self.process_parameters
    }

    pub fn parameter_values(&self) -> &[Vec<DataExpression>] {
        &self.parameter_values
    }

    /// Replaces the state set with `states`, typically the reachable set from
    /// [crate::reachability].
    pub fn set_states(&mut self, states: LDDFunction) {
        self.states = states;
    }
}

impl<L: TransitionLabel> SymbolicLPS for SymbolicLts<L> {
    type Group = SummandGroup;

    fn initial_state(&self) -> &LDDFunction {
        &self.initial_state
    }

    fn transition_groups(&self) -> &[Self::Group] {
        &self.summand_groups
    }

    fn transition_groups_mut(&mut self) -> &mut [Self::Group] {
        &mut self.summand_groups
    }

    fn create_context(&self) {}
}

impl<L: TransitionLabel> SymbolicLTS for SymbolicLts<L> {
    type Label = L;

    fn states(&self) -> &LDDFunction {
        &self.states
    }

    fn action_labels(&self) -> &[L] {
        &self.action_labels
    }

    fn parameter_values(&self) -> &[Vec<DataExpression>] {
        &self.parameter_values
    }
}
