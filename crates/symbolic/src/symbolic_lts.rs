use merc_aterm::ATerm;
use merc_data::DataSpecification;
use merc_ldd::Ldd;

/// Represents a symbolic LTS encoded by a disjunctive transition relation and a set of states.
pub struct SymbolicLts {
    data_specification: DataSpecification,

    states: Ldd,

    /// A singleton LDD representing the initial state.
    initial_state: Ldd,

    summand_groups: Vec<SummandGroup>,
}

impl SymbolicLts {
    /// Creates a new symbolic LTS.
    pub fn new(
        data_specification: DataSpecification,
        states: Ldd,
        initial_state: Ldd,
        summand_groups: Vec<SummandGroup>,
    ) -> Self {
        Self {
            data_specification,
            states,
            initial_state,
            summand_groups,
        }
    }

    /// Returns the LDD representing the set of states.
    pub fn states(&self) -> &Ldd {
        &self.states
    }

    /// Returns the LDD representing the initial state.
    pub fn initial_state(&self) -> &Ldd {
        &self.initial_state
    }

    /// Returns an iterator over the summand groups.
    pub fn summand_groups(&self) -> &[SummandGroup] {
        &self.summand_groups
    }
}

/// Represents a short vector transition relation for a group of summands.
pub struct SummandGroup {
    read_parameters: Vec<ATerm>,
    write_parameters: Vec<ATerm>,

    /// The transition relation T -> U for this summand group, such that T are the original parameters projected on the read_parameters and U the ones projected on the write_parameters.
    relation: Ldd,
}

impl SummandGroup {
    /// Creates a new summand group.
    pub fn new(
        read_parameters: Vec<ATerm>,
        write_parameters: Vec<ATerm>,
        relation: Ldd,
    ) -> Self {
        Self {
            read_parameters,
            write_parameters,
            relation,
        }
    }

    /// Returns the transition relation LDD for this summand group.
    pub fn relation(&self) -> &Ldd {
        &self.relation
    }
}