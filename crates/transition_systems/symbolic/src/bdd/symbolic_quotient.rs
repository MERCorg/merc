use std::ops::Range;

use oxidd::BooleanFunctionQuant;
use oxidd::BooleanOperator;
use oxidd::FunctionSubst;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::Subst;
use oxidd::VarNo;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use oxidd::error::DuplicateVarName;
use oxidd::ldd::Value;

use merc_lts::TransitionLabel;
use merc_utilities::MercError;

use crate::SummandGroupBdd;
use crate::SymbolicLtsBdd;
use crate::compute_vars_bdd;
use crate::variable_rename_reverse;

/// Computes the symbolic quotient of the given partition.
///
/// # Details
///
/// The returned LTS encodes its states as block indices over the supplied
/// `block_vars` (used as the source-block bits) and over a freshly allocated
/// set of "next-block" variables (used as the target-block bits).
///
/// The action variables and action labels are inherited from `lts`.
pub fn quotient_symbolic<L: TransitionLabel>(
    manager_ref: &BDDManagerRef,
    lts: &SymbolicLtsBdd<L>,
    partition: &BDDFunction,
    block_vars: &[VarNo],
) -> Result<SymbolicLtsBdd<L>, MercError> {
    let num_block_bits = block_vars.len();

    // Allocate next-block variables in the manager (one per source-block bit).
    let next_block_names = (0..num_block_bits)
        .map(|i| format!("nb_{}", i))
        .collect::<Vec<String>>();
    let next_block_vars: Vec<VarNo> = manager_ref
        .with_manager_exclusive(|manager| -> Result<Range<VarNo>, DuplicateVarName> {
            manager.add_named_vars(next_block_names)
        })
        .map_err(|e| format!("Failed to create next-block variables: {e}"))?
        .collect();

    // P_source(s, b) = P(s', b)[s' <- s]
    let s_prime_to_s_pairs: Vec<(VarNo, VarNo)> = lts
        .state_variables()
        .iter()
        .zip(lts.next_state_variables().iter())
        .map(|(s, s_prime)| (*s_prime, *s))
        .collect();
    let partition_source = variable_rename_reverse(manager_ref, partition, &s_prime_to_s_pairs)?;

    // P_target(s', b') = P(s', b)[b <- b']
    let (next_block_vars_bdd, _) = compute_vars_bdd(manager_ref, &next_block_vars)?;
    let partition_target = partition.substitute(&Subst::new(block_vars, &next_block_vars_bdd))?;

    // Cubes to quantify over.
    let (_, state_vars_bdd) = compute_vars_bdd(manager_ref, lts.state_variables())?;
    let (_, next_state_vars_bdd) = compute_vars_bdd(manager_ref, lts.next_state_variables())?;

    // States_q(b) = ∃ s. states(s) ∧ P_source(s, b)
    let states_q = lts
        .states()
        .apply_exists(BooleanOperator::And, &partition_source, &state_vars_bdd)?;

    // InitialState_q(b) = ∃ s. initial_state(s) ∧ P_source(s, b)
    let initial_state_q = lts
        .initial_state()
        .apply_exists(BooleanOperator::And, &partition_source, &state_vars_bdd)?;

    // For each transition group, compute T_q(b, b', a) = ∃ s, s'. T_ext(s, s', a) ∧ P_source(s, b) ∧ P_target(s', b').
    //
    // The relation is extended with x = x' for non-written state variables so the existential
    // quantification does not leave those variables unconstrained — otherwise non-written next-state
    // bits would be free and we would conflate transitions with different target states.
    let mut quotient_groups = Vec::with_capacity(lts.transition_groups().len());
    for group in lts.transition_groups() {
        let extended = crate::extend_relation(
            manager_ref,
            group.relation(),
            lts.state_variables(),
            lts.next_state_variables(),
            group.write_variables(),
        )?;

        let tmp = extended.apply_exists(BooleanOperator::And, &partition_target, &next_state_vars_bdd)?;
        let quotient = tmp.apply_exists(BooleanOperator::And, &partition_source, &state_vars_bdd)?;

        quotient_groups.push(SummandGroupBdd::new(
            quotient,
            block_vars.to_vec(),
            next_block_vars.clone(),
        ));
    }

    Ok(SymbolicLtsBdd::with_quotient_state(
        lts,
        states_q,
        initial_state_q,
        quotient_groups,
        block_vars.to_vec(),
        next_block_vars,
        vec![num_block_bits as Value],
    ))
}
