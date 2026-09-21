use std::ops::Range;

use itertools::Itertools;
use log::debug;
use log::info;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_lts::TransitionLabel;
use oxidd::BooleanFunction;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::VarNo;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use oxidd::error::DuplicateVarName;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use oxidd::util::OutOfMemory;

use merc_data::DataExpression;
use merc_utilities::MercError;
use oxidd::ldd::LDDFunction;

use crate::SummandGroup;
use crate::SymbolicLTS;
use crate::SymbolicLts;
use crate::TransitionGroup;
use crate::bdd_to_ldd;
use crate::compute_bits;
use crate::compute_highest;
use crate::ldd_to_bdd;
use crate::required_bits;

/// A symbolic LTS that uses BDDs for the symbolic representation, instead of
/// LDDs as done in [crate::SymbolicLts].
pub struct SymbolicLtsBdd<L: TransitionLabel> {
    /// The BDD representing the set of states.
    states: BDDFunction,

    /// The BDD representing the set of initial states.
    initial_state: BDDFunction,

    /// The transition groups representing the disjunctive transition relation.
    transition_groups: Vec<SummandGroupBdd>,

    /// The number of bits used for each state variable.
    state_variable_num_of_bits: Vec<Value>,

    /// The variable numbers used to represent the state variables.
    state_variable_indices: Vec<VarNo>,

    /// The variable numbers used to represent the next state variables.
    next_state_variables_indices: Vec<VarNo>,

    /// The action variable indices used to represent the action labels.
    action_variable_indices: Vec<VarNo>,

    /// The action labels of the LTS, their position corresponds to the LDD values.
    action_labels: Vec<L>,

    /// The possible values for each process parameter, their position
    /// corresponds to the LDD values.
    parameter_values: Vec<Vec<DataExpression>>,
}

impl<L: TransitionLabel> SymbolicLtsBdd<L> {
    /// Converts a symbolic LTS using LDDs into a symbolic LTS using BDDs.
    ///
    /// # Details
    ///
    /// The resulting BDD is assumed to only be valid for the reachable states
    /// of the LDD symbolic LTS, as unreachable states may not be representable
    /// with the number of bits assigned to each state variable.
    ///
    /// # Errors
    ///
    /// Fails if `manager_bdd` already holds variables, since a BDD manager can only hold the
    /// variables for a single symbolic LTS.
    pub fn from_symbolic_lts<LS: SymbolicLTS<Label = L>>(
        manager: &LDDManagerRef,
        manager_bdd: &BDDManagerRef,
        lts: &LS,
    ) -> Result<Self, MercError> {
        info!("Converting symbolic LTS from LDD to BDD representation...");

        // Determine the highest values for every state variable.
        let mut state_highest = compute_highest(manager, lts.states());

        let mut action_label_highest = 0u32;
        for group in lts.transition_groups() {
            let highest = compute_highest(manager, group.relation());

            // Deal with the special empty case.
            if highest.is_empty() {
                continue;
            }

            action_label_highest =
                action_label_highest.max(highest[group.action_label_index().ok_or("Action label index not found")?]);

            // Also consider the highest values read or written for the state
            // variables in the relation. LDD levels follow the sorted merge of
            // read and write indices (read before write at the same variable).
            for (ldd_level, var) in group
                .read_indices()
                .iter()
                .merge(group.write_indices().iter())
                .enumerate()
            {
                let var = *var as usize;
                state_highest[var] = state_highest[var].max(highest[ldd_level]);
            }
        }

        let state_bits = compute_bits(&state_highest);
        debug!("Determined number of bits for state variables: {:?}", state_bits);

        let action_label_bits = required_bits(action_label_highest);
        debug!(
            "Highest action label: {}, bits: {}",
            action_label_highest, action_label_bits
        );

        // Create the state variables, with interleaved primed variables for write parameters
        let mut vars = Vec::new();

        // Keep track of the bits per state variable.
        let mut state_variables_bits: Vec<Vec<VarNo>> = Vec::new();
        let mut next_state_variables_bits: Vec<Vec<VarNo>> = Vec::new();
        for (i, &bits) in state_bits.iter().enumerate() {
            let mut state_var_bits = Vec::new();
            let mut next_state_var_bits = Vec::new();
            for k in 0..bits {
                // Add variable for the state variable
                state_var_bits.push(vars.len() as VarNo);
                vars.push(format!("s{}_{}", i, k));

                // Add a primed version for the write parameters
                next_state_var_bits.push(vars.len() as VarNo);
                vars.push(format!("s{}'_{}", i, k));
            }
            state_variables_bits.push(state_var_bits);
            next_state_variables_bits.push(next_state_var_bits);
        }

        // Create action label variables
        let mut action_labels_vars = Vec::new();
        for k in 0..action_label_bits {
            action_labels_vars.push(vars.len() as VarNo);
            vars.push(format!("a_{}", k));
        }

        // A BDD manager can only hold the variables for a single symbolic LTS.
        if manager_bdd.with_manager_shared(|manager| manager.num_vars()) != 0 {
            return Err("BDD manager must not contain any variables yet".into());
        }

        // Create variables in the BDD manager
        let number_of_vars = vars.len();
        let variables = manager_bdd
            .with_manager_exclusive(|manager| -> Result<Range<VarNo>, DuplicateVarName> {
                manager.add_named_vars(vars)
            })
            .map_err(|e| format!("Failed to create variables: {e}"))?;

        assert!(variables.clone().is_sorted(), "Variables must be added in sorted order");
        assert!(
            variables.len() == number_of_vars,
            "Number of created variables does not match"
        );

        // Convert the states to a BDD representation.
        let bits_dd = manager.with_manager_shared(|m| LDDFunction::singleton(m, &state_bits))?;
        let all_state_variables_bits: Vec<VarNo> = state_variables_bits.iter().flatten().cloned().collect();
        let states = ldd_to_bdd(manager, manager_bdd, lts.states(), &bits_dd, &all_state_variables_bits)?;
        let initial_state = ldd_to_bdd(
            manager,
            manager_bdd,
            lts.initial_state(),
            &bits_dd,
            &all_state_variables_bits,
        )?;

        let mut transition_groups = Vec::new();
        for (index, group) in lts.transition_groups().iter().enumerate() {
            // Determine the number of bits used for each layer.
            let mut relation_bits = Vec::new();

            // Determine all the variables used in this relation.
            let mut variables = Vec::new();

            let mut read_variable_indices: Vec<VarNo> = Vec::new();
            let mut write_variable_indices: Vec<VarNo> = Vec::new();

            for (var, bits) in state_bits.iter().enumerate() {
                if group.read_indices().contains(&(var as VarNo)) {
                    // The transition group reads this state variable
                    relation_bits.push(*bits);
                    variables.extend(state_variables_bits[var].iter());
                    read_variable_indices.extend(state_variables_bits[var].iter())
                }

                if group.write_indices().contains(&(var as VarNo)) {
                    // The transition group writes this state variable
                    relation_bits.push(*bits);
                    variables.extend(next_state_variables_bits[var].iter());
                    write_variable_indices.extend(next_state_variables_bits[var].iter())
                }
            }

            // Append action label bits (between read and write segments) if present
            if let Some(_action_index) = group.action_label_index() {
                // TODO: This currently assumes that action label bits are at the end.
                variables.extend(action_labels_vars.iter());
            }

            // Append action label bits
            relation_bits.push(action_label_bits);
            debug!(
                "Transition group {}, {:?} uses number of bits {:?}, and variables: {:?}",
                index, group, relation_bits, variables
            );

            let bits_dd = manager.with_manager_shared(|m| LDDFunction::singleton(m, &relation_bits))?;
            let relation_bdd = ldd_to_bdd(manager, manager_bdd, group.relation(), &bits_dd, &variables)?;

            transition_groups.push(SummandGroupBdd::new(
                relation_bdd,
                read_variable_indices,
                write_variable_indices,
            ));
        }

        // Compute the BDDs representing the state variables and next state variables.
        let all_next_state_variables_bits = next_state_variables_bits
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<VarNo>>();

        debug!("State bits {all_state_variables_bits:?}, and next state bits {all_next_state_variables_bits:?}");

        info!("Finished conversion.");
        Ok(Self {
            action_variable_indices: action_labels_vars,
            states,
            initial_state,
            transition_groups,
            state_variable_num_of_bits: state_bits,
            state_variable_indices: all_state_variables_bits,
            next_state_variables_indices: all_next_state_variables_bits,
            action_labels: lts.action_labels().to_vec(),
            parameter_values: lts.parameter_values().to_vec(),
        })
    }

    /// Constructs a new symbolic LTS with the given transition groups.
    pub fn with_transition_groups(lts: &Self, transition_groups: Vec<SummandGroupBdd>) -> Self {
        Self {
            states: lts.states.clone(),
            initial_state: lts.initial_state.clone(),
            state_variable_num_of_bits: lts.state_variable_num_of_bits.clone(),
            state_variable_indices: lts.state_variable_indices.clone(),
            next_state_variables_indices: lts.next_state_variables_indices.clone(),
            action_variable_indices: lts.action_variable_indices.clone(),
            action_labels: lts.action_labels.clone(),
            parameter_values: lts.parameter_values.clone(),
            transition_groups,
        }
    }

    /// Constructs a quotient symbolic LTS that reuses the action labels of `lts`
    /// but encodes states and transitions over fresh state and next-state variables
    /// (typically block-index variables produced by sigref).
    pub fn with_quotient_state(
        lts: &Self,
        states: BDDFunction,
        initial_state: BDDFunction,
        transition_groups: Vec<SummandGroupBdd>,
        state_variable_indices: Vec<VarNo>,
        next_state_variables_indices: Vec<VarNo>,
        state_variable_num_of_bits: Vec<Value>,
    ) -> Self {
        Self {
            states,
            initial_state,
            transition_groups,
            state_variable_num_of_bits,
            state_variable_indices,
            next_state_variables_indices,
            action_variable_indices: lts.action_variable_indices.clone(),
            action_labels: lts.action_labels.clone(),
            parameter_values: Vec::new(),
        }
    }

    /// Returns the BDD representing the set of states.
    pub fn states(&self) -> &BDDFunction {
        &self.states
    }

    /// Returns the BDD representing the set of initial states.
    pub fn initial_state(&self) -> &BDDFunction {
        &self.initial_state
    }

    /// Returns the number of bits used for each state variable.
    pub fn state_variable_num_of_bits(&self) -> &[Value] {
        &self.state_variable_num_of_bits
    }

    /// Returns the BDD variables used to represent the state variables.
    pub fn state_variables(&self) -> &[VarNo] {
        &self.state_variable_indices
    }

    /// Returns the variable numbers used to represent the next state variables.
    pub fn next_state_variables(&self) -> &[VarNo] {
        &self.next_state_variables_indices
    }

    /// Returns the transition groups representing the disjunctive transition relation.
    pub fn transition_groups(&self) -> &[SummandGroupBdd] {
        &self.transition_groups
    }

    /// Returns the action variable indices used to represent the action labels.
    pub fn action_variables(&self) -> &[VarNo] {
        &self.action_variable_indices
    }

    /// Returns the action labels of the LTS.
    pub fn action_labels(&self) -> &[L] {
        &self.action_labels
    }

    /// Returns the possible values for each process parameter.
    pub fn parameter_values(&self) -> &[Vec<DataExpression>] {
        &self.parameter_values
    }

    /// Converts this BDD-based symbolic LTS into an LDD-based [SymbolicLts].
    ///
    /// # Details
    ///
    /// This is primarily intended for writing quotient results back to the
    /// mCRL2 `.sym` format through [crate::write_symbolic_lts].
    pub fn to_symbolic_lts(
        &self,
        ldd_manager: &LDDManagerRef,
        bdd_manager: &BDDManagerRef,
    ) -> Result<SymbolicLts<L>, MercError> {
        let state_bits: Vec<Value> = self.state_variable_num_of_bits.clone();
        let state_groups =
            split_variables_by_state_group(&self.state_variable_indices, &self.state_variable_num_of_bits, "state")?;
        let next_state_groups = split_variables_by_state_group(
            &self.next_state_variables_indices,
            &self.state_variable_num_of_bits,
            "next-state",
        )?;

        let states = bdd_to_ldd(
            ldd_manager,
            bdd_manager,
            &self.states,
            &self.state_variable_indices,
            &state_bits,
            0,
            0,
        )?;
        let initial_state = bdd_to_ldd(
            ldd_manager,
            bdd_manager,
            &self.initial_state,
            &self.state_variable_indices,
            &state_bits,
            0,
            0,
        )?;

        let process_parameters: Vec<DataVariable> = (0..self.state_variable_num_of_bits.len())
            .map(|index| DataVariable::new(format!("x{index}").as_str()))
            .collect();

        let mut summand_groups = Vec::with_capacity(self.transition_groups.len());
        for group in &self.transition_groups {
            let read_state_indices =
                decode_state_group_indices(group.read_variables(), &state_groups, "transition group read variables")?;
            let write_state_indices = decode_state_group_indices(
                group.write_variables(),
                &next_state_groups,
                "transition group write variables",
            )?;

            let num_state_variables = self.state_variable_num_of_bits.len();
            let mut reads_state = vec![false; num_state_variables];
            let mut writes_state = vec![false; num_state_variables];
            for &index in &read_state_indices {
                reads_state[index] = true;
            }
            for &index in &write_state_indices {
                writes_state[index] = true;
            }

            let mut relation_variables: Vec<VarNo> = Vec::new();
            let mut relation_bits: Vec<Value> = Vec::new();

            // Keep the same per-layer ordering as `from_symbolic_lts`:
            // iterate state variables in index order and append read segment before write segment.
            for state_index in 0..num_state_variables {
                if reads_state[state_index] {
                    relation_variables.extend_from_slice(&state_groups[state_index]);
                    relation_bits.push(self.state_variable_num_of_bits[state_index]);
                }
                if writes_state[state_index] {
                    relation_variables.extend_from_slice(&next_state_groups[state_index]);
                    relation_bits.push(self.state_variable_num_of_bits[state_index]);
                }
            }

            if self.action_variable_indices.is_empty() {
                return Err("Cannot convert symbolic LTS BDD without action variables".into());
            }

            relation_variables.extend_from_slice(&self.action_variable_indices);
            relation_bits.push(self.action_variable_indices.len() as Value);

            let relation = bdd_to_ldd(
                ldd_manager,
                bdd_manager,
                group.relation(),
                &relation_variables,
                &relation_bits,
                0,
                0,
            )?;

            let read_parameters = read_state_indices
                .iter()
                .map(|&index| process_parameters[index].clone())
                .collect();
            let write_parameters = write_state_indices
                .iter()
                .map(|&index| process_parameters[index].clone())
                .collect();

            summand_groups.push(SummandGroup::new(
                ldd_manager,
                &process_parameters,
                read_parameters,
                write_parameters,
                relation,
            )?);
        }

        let action_labels = self.action_labels.clone();

        let parameter_values = vec![Vec::new(); process_parameters.len()];

        Ok(SymbolicLts::new(
            Mcrl2DataSpecification::default(),
            process_parameters,
            states,
            initial_state,
            summand_groups,
            action_labels,
            parameter_values,
        ))
    }
}

fn split_variables_by_state_group(
    variables: &[VarNo],
    bits_per_state_variable: &[Value],
    kind: &str,
) -> Result<Vec<Vec<VarNo>>, MercError> {
    let expected_num_of_variables: usize = bits_per_state_variable.iter().map(|&bits| bits as usize).sum();
    if variables.len() != expected_num_of_variables {
        return Err(format!(
            "Invalid {kind} variables: expected {expected_num_of_variables}, found {}",
            variables.len()
        )
        .into());
    }

    let mut offset = 0usize;
    let mut groups = Vec::with_capacity(bits_per_state_variable.len());
    for &bits in bits_per_state_variable {
        let end = offset + bits as usize;
        groups.push(variables[offset..end].to_vec());
        offset = end;
    }

    Ok(groups)
}

fn decode_state_group_indices(
    group_variables: &[VarNo],
    all_state_groups: &[Vec<VarNo>],
    description: &str,
) -> Result<Vec<usize>, MercError> {
    let mut result = Vec::new();

    for (state_index, state_bits) in all_state_groups.iter().enumerate() {
        let present_count = state_bits.iter().filter(|bit| group_variables.contains(bit)).count();

        if present_count == 0 {
            continue;
        }

        if present_count != state_bits.len() {
            return Err(format!(
                "Invalid {description}: state variable {state_index} appears with {} out of {} bits",
                present_count,
                state_bits.len()
            )
            .into());
        }

        result.push(state_index);
    }

    Ok(result)
}

pub struct SummandGroupBdd {
    /// The BDD representing the transition relation for this summand group.
    relation: BDDFunction,

    /// The indices of the read variables for this summand group.
    read_variables: Vec<VarNo>,

    /// The indices of the write variables for this summand group.
    write_variables: Vec<VarNo>,
}

impl SummandGroupBdd {
    /// Creates a new summand group with the given transition relation.
    ///
    /// Read variables are current state variables, and write variables are next state variables.
    pub fn new(relation: BDDFunction, read_variables: Vec<VarNo>, write_variables: Vec<VarNo>) -> Self {
        Self {
            relation,
            read_variables,
            write_variables,
        }
    }

    /// Returns the BDD representing the transition relation for this summand group.
    pub fn relation(&self) -> &BDDFunction {
        &self.relation
    }

    /// Returns the indices of the read variables for this summand group.
    pub fn read_variables(&self) -> &[VarNo] {
        &self.read_variables
    }

    /// Returns the indices of the write variables for this summand group.
    pub fn write_variables(&self) -> &[VarNo] {
        &self.write_variables
    }
}

/// Creates BDD of variables for the given variable numbers.
pub fn compute_vars_bdd(
    manager_ref: &BDDManagerRef,
    vars: &[VarNo],
) -> Result<(Vec<BDDFunction>, BDDFunction), OutOfMemory> {
    manager_ref.with_manager_shared(|manager| -> Result<_, OutOfMemory> {
        let mut vector = Vec::new();
        let mut bdd: BDDFunction = BDDFunction::t(manager);

        for var in vars {
            let var = BDDFunction::var(manager, *var)?;
            vector.push(var.clone());
            bdd = bdd.and(&var)?;
        }

        Ok((vector, bdd))
    })
}

#[cfg(test)]
mod tests {
    use merc_lts::LtsBuilderMem;
    use merc_reduction::Equivalence;
    use merc_reduction::compare_lts;
    use merc_utilities::Timing;
    use merc_utilities::random_test;
    use merc_utilities::test_logger;

    use crate::BDD_CACHE_CAPACITY;
    use crate::BDD_NODE_CAPACITY;
    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::SymbolicLtsBdd;
    use crate::convert_symbolic_lts;
    use crate::convert_symbolic_lts_bdd;
    use crate::random_symbolic_lts;
    use crate::read_symbolic_lts;

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_from_symbolic_lts_bdd_abp() {
        test_logger();

        let input = include_bytes!("../../../../examples/lts/abp.sym");

        let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
        let symbolic_lts = read_symbolic_lts(&ldd_manager, &input[..]).unwrap();

        // This only tests that the conversion does not panic.
        SymbolicLtsBdd::from_symbolic_lts(&ldd_manager, &bdd_manager, &symbolic_lts).unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_to_symbolic_lts_conversion() {
        random_test(10, |rng| {
            let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);

            let mut chosen = None;
            for _ in 0..20 {
                let lts = random_symbolic_lts(rng, &ldd_manager, 5, 3).unwrap();
                let lts_bdd = SymbolicLtsBdd::from_symbolic_lts(&ldd_manager, &bdd_manager, &lts).unwrap();

                // `to_symbolic_lts` reconstructs layer order from bit-level variables; zero-bit
                // state variables carry no bit-level representation and are outside this conversion path.
                if lts_bdd.state_variable_num_of_bits().iter().all(|&bits| bits > 0) {
                    chosen = Some((lts, lts_bdd));
                    break;
                }
            }

            let (lts, lts_bdd) = chosen.expect("Failed to generate a random LTS with non-zero state bit-widths");
            let converted_lts = lts_bdd.to_symbolic_lts(&ldd_manager, &bdd_manager).unwrap();

            let mut original_builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_from_original = convert_symbolic_lts(&ldd_manager, &mut original_builder, &lts).unwrap();

            let mut converted_builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_from_converted =
                convert_symbolic_lts(&ldd_manager, &mut converted_builder, &converted_lts).unwrap();

            assert!(
                compare_lts(
                    Equivalence::StrongBisim,
                    explicit_from_original,
                    explicit_from_converted,
                    false,
                    false,
                    &Timing::new()
                )
                .0,
                "Original LTS and LTS converted via SymbolicLtsBdd::to_symbolic_lts should be bisimilar"
            );

            let mut direct_bdd_builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_from_bdd = convert_symbolic_lts_bdd(&bdd_manager, &mut direct_bdd_builder, &lts_bdd).unwrap();

            let mut converted_again_builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_from_roundtrip =
                convert_symbolic_lts(&ldd_manager, &mut converted_again_builder, &converted_lts).unwrap();

            assert!(
                compare_lts(
                    Equivalence::StrongBisim,
                    explicit_from_bdd,
                    explicit_from_roundtrip,
                    false,
                    false,
                    &Timing::new()
                )
                .0,
                "Direct BDD conversion and BDD->LDD->explicit conversion should be bisimilar"
            );
        });
    }
}
