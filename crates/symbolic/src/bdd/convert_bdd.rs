use log::info;
use log::trace;
use oxidd::BooleanFunction;
use oxidd::ManagerRef;
use oxidd::VarNo;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use oxidd::util::OptBool;
use rustc_hash::FxBuildHasher;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use merc_collections::IndexedSet;
use merc_io::LargeFormatter;
use merc_io::TimeProgress;
use merc_lts::LtsBuilder;
use merc_lts::StateIndex;
use merc_lts::TransitionLabel;
use merc_utilities::MercError;

use crate::CubeIterAll;
use crate::SatCountCache;
use crate::SymbolicLtsBdd;
use crate::approx_satcount;
use crate::to_value;

/// Converts a symbolic LTS to an explicit LTS.
///
/// # Details
///
/// This basically applies the symbolic transitions to every state in the state
/// space, and constructs the explicit LTS.
pub fn convert_symbolic_lts_bdd<B: LtsBuilder<String>, L: TransitionLabel>(
    manager_ref: &BDDManagerRef,
    output: &mut B,
    lts: &SymbolicLtsBdd<L>,
) -> Result<B::LTS, MercError> {
    // Compute for every read and write index its position in the transition vector.
    let state_variables = lts.state_variables().to_vec();
    let next_state_variables = lts.next_state_variables().to_vec();
    let action_variables = lts.action_variables().to_vec();

    let state_variable_indices = state_variables
        .iter()
        .enumerate()
        .map(|(index, &var)| (var, index))
        .collect::<FxHashMap<VarNo, usize>>();
    let next_state_variable_indices = next_state_variables
        .iter()
        .enumerate()
        .map(|(index, &var)| (var, index))
        .collect::<FxHashMap<VarNo, usize>>();

    let state_group_offsets = {
        let mut offsets = Vec::with_capacity(lts.state_variable_num_of_bits().len());
        let mut offset = 0usize;

        for &num_of_bits in lts.state_variable_num_of_bits() {
            offsets.push(offset);
            offset += num_of_bits as usize;
        }

        offsets
    };

    let mut read_var_positions = Vec::new();
    let mut write_positions = Vec::new();
    let mut enum_variables = Vec::new();
    let mut action_positions = Vec::new();

    for group in lts.transition_groups() {
        // For each read variable, remember its position in the (concrete) state vector so
        // we can build the restriction cube for the current state without hashmap lookups.
        let read_positions: Vec<(VarNo, usize)> = group
            .read_variables()
            .iter()
            .map(|&var| {
                let pos = *state_variable_indices
                    .get(&var)
                    .expect("Read variable was not found in state variables");
                (var, pos)
            })
            .collect();

        let write_group: FxHashSet<usize> = group
            .write_variables()
            .iter()
            .map(|var| {
                *next_state_variable_indices
                    .get(var)
                    .expect("Write variable was not found in next-state variables")
            })
            .collect();

        let mut variables = Vec::new();

        let wpos = compute_positions(
            &next_state_variables,
            &action_variables,
            &state_group_offsets,
            &write_group,
            &mut variables,
        );

        let action_start = variables.len() - action_variables.len();
        action_positions.push(action_start);
        enum_variables.push(variables);

        read_var_positions.push(read_positions);
        write_positions.push(wpos);
    }

    // Total number of states for progress reporting.
    let mut satcount_cache = SatCountCache::new();
    let total_number_of_states =
        approx_satcount(lts.states(), lts.state_variables().len() as VarNo, &mut satcount_cache);
    info!(
        "Converting symbolic LTS to explicit LTS with {} states",
        total_number_of_states
    );

    let total_states_f64 = total_number_of_states.as_f64();
    let state_progress = TimeProgress::new(
        move |number_of_states| {
            info!(
                "Added {} states to discovered ({}%)",
                LargeFormatter(number_of_states),
                (number_of_states as f64 * 100.0 / total_states_f64) as usize
            );
        },
        1,
    );

    // All states have been explored, so add them to the discovered set immediately.
    let mut discovered: IndexedSet<Vec<OptBool>, FxBuildHasher> = IndexedSet::new();
    for cube in CubeIterAll::with_variables(lts.states(), &state_variables) {
        let cube = cube?;

        let (_, inserted) = discovered.insert(cube);
        debug_assert!(inserted, "State space contains duplicate states");
        state_progress.print(discovered.len())
    }

    // Total number of states for progress reporting.
    let progress = TimeProgress::new(
        move |(number_of_states, number_of_transitions)| {
            info!(
                "Explored {} states and {} transitions ({}%)",
                LargeFormatter(number_of_states),
                LargeFormatter(number_of_transitions),
                (number_of_states as f64 * 100.0 / total_states_f64) as usize
            );
        },
        1,
    );

    // Keep track of outgoing transitions to avoid duplicates.
    let mut outgoing = FxHashSet::default();

    // Avoid reallocations.
    let mut target = vec![OptBool::False; state_variables.len()];

    for (index, cube) in CubeIterAll::with_variables(lts.states(), &state_variables).enumerate() {
        let cube = cube?;

        // Find the index of this state, it was already added before.
        let state_index = discovered
            .index(&cube)
            .ok_or("Found state that was not in the state set")?;

        // Apply every transition group to this state.
        for (group_index, group) in lts.transition_groups().iter().enumerate() {
            // Restrict the transition relation to the current state's read-variable assignment.
            // This directly yields the transitions enabled in this state, so no per-transition
            // read-parameter matching is required and transitions of other states are never
            // enumerated.
            let restriction = build_state_restriction(manager_ref, &read_var_positions[group_index], &cube)?;
            let relation = group.relation().restrict(&restriction)?;

            for transition in CubeIterAll::with_variables(&relation, &enum_variables[group_index]) {
                let transition = transition?;

                // Apply the transition writes to the state vector.
                target.clone_from_slice(&cube);
                trace!("transition {:?}", transition);
                for (&write_variable, &write_position) in
                    group.write_variables().iter().zip(write_positions[group_index].iter())
                {
                    let state_pos = *next_state_variable_indices
                        .get(&write_variable)
                        .expect("Write variable was not found in next-state variables");
                    target[state_pos] = transition[write_position];
                }

                // Find the action label.
                let action_bits = &transition[action_positions[group_index]..];
                let action_value = to_value(action_bits);
                let label = lts
                    .action_labels()
                    .get(action_value as usize)
                    .ok_or("Found transition with unknown action label")?
                    .to_string();

                // Find the target state index.
                let target_index = discovered
                    .index(&target)
                    .ok_or("Found state that was not in the state set")?;
                // Include the action label in the dedup key: the same source/target pair can be
                // connected by transitions with different labels, and dropping any of them would
                // silently lose behavior.
                if outgoing.insert((*state_index, action_value, *target_index)) {
                    trace!(
                        " Found transition in {group_index} from {:?} to {:?} with label {:?}",
                        cube, target, label
                    );

                    output.add_transition(StateIndex::new(*state_index), &label, StateIndex::new(*target_index))?;
                }
            }
        }

        progress.print((index, output.num_of_transitions()));

        // Clear the outgoing set for the next state.
        outgoing.clear();
    }

    // Find the initial state.
    let mut initial_state = CubeIterAll::with_variables(lts.initial_state(), &state_variables);
    let initial_state = initial_state.next().ok_or("Symbolic LTS has no initial state")??;

    let initial_state_index = discovered
        .index(&initial_state)
        .ok_or("Initial state was not found in the discovered state set")?;

    output.finish(StateIndex::new(*initial_state_index))
}

/// Builds a BDD restriction cube fixing every read variable to its concrete value
/// in the given state, suitable for use with [`BooleanFunction::restrict`].
fn build_state_restriction(
    manager_ref: &BDDManagerRef,
    read_positions: &[(VarNo, usize)],
    state: &[OptBool],
) -> Result<BDDFunction, MercError> {
    manager_ref.with_manager_shared(|manager| {
        let mut result = BDDFunction::t(manager);

        for &(var, pos) in read_positions {
            let literal = BDDFunction::var(manager, var)?;
            result = match state[pos] {
                OptBool::True => result.and(&literal)?,
                // Concrete states never contain don't cares, so treat None as false.
                OptBool::False | OptBool::None => result.and(&literal.not()?)?,
            };
        }

        Ok(result)
    })
}

/// Computes the positions of the write indices in the enumeration vector.
///
/// The enumeration vector consists of the write variables (in state-group order) followed
/// by the action variables. The returned vector maps each pushed write variable to its
/// position in the enumeration vector.
fn compute_positions(
    next_state_variables: &[VarNo],
    action_variables: &[VarNo],
    state_group_offsets: &[usize],
    write_group: &FxHashSet<usize>,
    enum_variables: &mut Vec<VarNo>,
) -> Vec<usize> {
    let mut wpos = Vec::new();

    for (state_group, &offset) in state_group_offsets.iter().enumerate() {
        let end = state_group_offsets
            .get(state_group + 1)
            .copied()
            .unwrap_or(next_state_variables.len());

        if write_group.contains(&offset) {
            for bit in next_state_variables.iter().take(end).skip(offset) {
                wpos.push(enum_variables.len());
                enum_variables.push(*bit);
            }
        }
    }

    enum_variables.extend(action_variables.iter().copied());

    wpos
}

#[cfg(test)]
mod tests {
    use merc_lts::LTS;
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
    fn test_convert_symbolic_lts_bdd_abp() {
        test_logger();

        let input = include_bytes!("../../../../examples/lts/abp.sym");

        let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
        let symbolic_lts = read_symbolic_lts(&ldd_manager, &input[..]).unwrap();
        let symbolic_lts_bdd = SymbolicLtsBdd::from_symbolic_lts(&ldd_manager, &bdd_manager, &symbolic_lts).unwrap();

        let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
        let lts = convert_symbolic_lts_bdd(&bdd_manager, &mut builder, &symbolic_lts_bdd).unwrap();

        assert_eq!(lts.num_of_states(), 74);
        assert_eq!(lts.num_of_transitions(), 92);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_convert_symbolic_lts_bdd() {
        random_test(100, |rng| {
            let ldd_manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);

            let lts = random_symbolic_lts(rng, &ldd_manager, 10, 5).unwrap();
            let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_lts = convert_symbolic_lts(&ldd_manager, &mut builder, &lts).unwrap();

            let bdd_manager = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
            let lts_bdd = SymbolicLtsBdd::from_symbolic_lts(&ldd_manager, &bdd_manager, &lts).unwrap();

            let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
            let explicit_lts_bdd = convert_symbolic_lts_bdd(&bdd_manager, &mut builder, &lts_bdd).unwrap();

            assert!(
                compare_lts(
                    Equivalence::StrongBisim,
                    explicit_lts,
                    explicit_lts_bdd,
                    false,
                    false,
                    &Timing::new()
                )
                .0,
                "Both the explicit LTS and the one converted from the symbolic LTS should be bisimilar"
            );
        });
    }
}
