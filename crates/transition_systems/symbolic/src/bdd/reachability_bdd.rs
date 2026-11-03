use log::info;
use log::trace;
use oxidd::BooleanFunction;
use oxidd::BooleanFunctionQuant;
use oxidd::BooleanOperator;
use oxidd::ManagerRef;
use oxidd::VarNo;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use oxidd::util::OutOfMemory;

use merc_io::TimeProgress;
use merc_lts::TransitionLabel;
use merc_utilities::MercError;
use oxidd_dump::Visualizer;

use crate::SymbolicLtsBdd;
use crate::compute_vars_bdd;
use crate::minus;
use crate::variable_rename_reverse;

/// Performs reachability analysis on the given symbolic LTS represented using BDDs.
///
/// # Details
///
/// When `visualize` has been set to true, intermediate BDDs are visualized using `oxidd-vis`.
pub fn reachability_bdd<L: TransitionLabel>(
    manager_ref: &BDDManagerRef,
    lts: &SymbolicLtsBdd<L>,
    visualize: bool,
) -> Result<BDDFunction, MercError> {
    let mut todo = lts.initial_state().clone();
    let mut states = lts.initial_state().clone(); // The state space.
    let mut iteration = 0;

    let progress = TimeProgress::new(
        |iteration: usize| {
            info!("Iteration {}", iteration);
        },
        1,
    );

    // Substitution to replace next state variables with current state variables: [s' <- s]
    let next_state_substitution: Vec<(VarNo, VarNo)> = lts
        .next_state_variables()
        .iter()
        .cloned()
        .zip(lts.state_variables().iter().cloned())
        .collect();

    // Determine the write variables BDDs for all transition groups.
    let relation_vars_bdd = lts
        .transition_groups()
        .iter()
        .map(|group| -> Result<_, OutOfMemory> {
            let bits = group
                .write_variables()
                .iter()
                .map(|var| {
                    // Find the index of the current state variable corresponding to this next state variable.
                    let index = lts
                        .next_state_variables()
                        .iter()
                        .position(|next_var| next_var == var)
                        .expect("write variable not found in next-state variables");
                    lts.state_variables()[index]
                })
                .collect::<Vec<VarNo>>();
            compute_vars_bdd(manager_ref, &bits)?
                .1
                .and(&compute_vars_bdd(manager_ref, lts.action_variables())?.1)
        })
        .collect::<Result<Vec<BDDFunction>, OutOfMemory>>()?;

    while todo.satisfiable() {
        trace!("iteration {}", iteration);

        // Apply the transition relations to the todo set.
        let mut todo1 = manager_ref.with_manager_shared(|manager| BDDFunction::f(manager));
        for (transition, relation_vars) in lts.transition_groups().iter().zip(relation_vars_bdd.iter()) {
            // We explicitly do not quantify over state variables that are not
            // written by the transition group. Otherwise, these state variables
            // would become unconstrained and then after substituting next state
            // variables with current state variables, they would lead to
            // spurious states.
            //
            // This can be seen from the following: `exists a. (todo(s) ∧ R(s, s', a))`
            // is equal to `todo(s)` if `support(R) = a`, where quantifying over `s` would
            // result in `T`. And `todo(s)[s' <- s]` is equal to `todo(s)`.
            let tmp = todo.apply_exists(BooleanOperator::And, transition.relation(), relation_vars)?;

            // Substitute next state variables with current state variables.
            let tmp = variable_rename_reverse(manager_ref, &tmp, &next_state_substitution)?;

            todo1 = todo1.or(&tmp)?;
        }

        if visualize {
            // Visualize the current partition.
            manager_ref.with_manager_shared(|manager| {
                Visualizer::new()
                    .add(&format!("todo1_{iteration}"), manager, [&todo1])
                    .serve()
            })?;
        }

        // Keep todo states that have not been discovered yet, and add them to the set of discovered states.
        todo = minus(&todo1, &states)?;
        states = states.or(&todo)?;
        progress.print(iteration);
        iteration += 1;
    }

    Ok(states)
}

#[cfg(test)]
mod tests {
    use oxidd::BooleanFunction;
    use oxidd::util::SatCountCache;
    use rustc_hash::FxBuildHasher;

    use merc_utilities::Timing;
    use merc_utilities::random_test;

    use crate::BDD_CACHE_CAPACITY;
    use crate::BDD_NODE_CAPACITY;
    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::LddLenCache;
    use crate::SymbolicLtsBdd;
    use crate::ldd_len;
    use crate::random_symbolic_lts;
    use crate::reachability;
    use crate::reachability_bdd;

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_reachability() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);

            // We don't really check anything here, just ensure that reachability runs without errors.
            let mut lts = random_symbolic_lts(rng, &manager, 10, 5).unwrap();
            let reachable_states = reachability(&manager, &mut lts, &Timing::new()).unwrap();
            let num_reachable_states = ldd_len(&reachable_states, &mut LddLenCache::new())
                .exact()
                .expect("the random test sets are small") as usize;

            let manager_ref = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);
            let lts_bdd = SymbolicLtsBdd::from_symbolic_lts(&manager, &manager_ref, &lts).unwrap();

            let reachable_states_bdd = reachability_bdd(&manager_ref, &lts_bdd, false).unwrap();
            let num_reachable_states_bdd = reachable_states_bdd
                .sat_count::<u64, FxBuildHasher>(lts_bdd.state_variables().len() as u32, &mut SatCountCache::default())
                as usize;

            assert_eq!(
                num_reachable_states, num_reachable_states_bdd,
                "Number of reachable states does not match between BDD and LDD-based reachability."
            );
        });
    }
}
