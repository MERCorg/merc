use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use rand::Rng;
use rand::RngExt;
use rand::seq::IndexedRandom;
use rand::seq::IteratorRandom;

use merc_aterm::ATermString;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_lts::LtsAction;
use merc_lts::LtsMultiAction;
use merc_lts::TransitionLabel;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::SummandGroup;
use crate::SymbolicLPS;
use crate::SymbolicLts;
use crate::from_iter;
use crate::random_vector_set;
use crate::reachability;

/// Generates random symbolic LTSs for testing purposes.
pub fn random_symbolic_lts<R: Rng>(
    rng: &mut R,
    manager: &LDDManagerRef,
    num_state_variables: usize,
    num_action_labels: usize,
) -> Result<SymbolicLts<LtsMultiAction<LtsAction>>, MercError> {
    let num_of_values = 5usize;

    let states = random_vector_set(rng, 100, num_state_variables, num_of_values as u32);
    let initial_state = states.iter().choose(rng).ok_or("at least one state generated")?;

    let mut action_labels = Vec::new();
    for i in 0..num_action_labels {
        action_labels.push(LtsMultiAction::from_index(i));
    }

    let mut parameter_values = Vec::new();
    let value = DataExpression::from_string("1")?;
    for _ in 0..num_state_variables {
        parameter_values.push(Vec::from_iter(std::iter::repeat_n(value.clone(), num_of_values)));
    }

    let parameters = (0..num_state_variables)
        .map(|i| DataVariable::new(ATermString::new(format!("p{i}"))))
        .collect::<Vec<_>>();

    let mut summand_groups = Vec::new();
    for _ in 0..5 {
        // random_range panics on an empty range, so guard against zero parameters.
        let num_of_read_variables = if parameters.is_empty() {
            0
        } else {
            rng.random_range(0..parameters.len())
        };

        let read_parameters = parameters
            .sample(rng, num_of_read_variables)
            .cloned()
            .collect::<Vec<_>>();

        let num_of_write_variables = if parameters.is_empty() {
            0
        } else {
            rng.random_range(0..parameters.len())
        };

        let write_parameters = parameters
            .sample(rng, num_of_write_variables)
            .cloned()
            .collect::<Vec<_>>();

        // The last position of every relation vector is the action label, which must
        // stay within the declared action labels; the state positions range over the
        // parameter values.
        let mut relation = random_vector_set(rng, 10, read_parameters.len() + write_parameters.len(), 5);
        let relation: Vec<Vec<Value>> = relation
            .drain()
            .map(|mut vector| {
                vector.push(rng.random_range(0..num_action_labels as Value));
                vector
            })
            .collect();
        let relation_bdd = from_iter(manager, relation.iter());

        summand_groups.push(SummandGroup::new(
            manager,
            &parameters,
            read_parameters,
            write_parameters,
            relation_bdd,
        )?)
    }

    let initial_state_ldd = from_iter(manager, std::iter::once(initial_state));

    // Explore via a lightweight wrapper so SymbolicLts is only constructed at the end
    // with the actual reachable state set.
    let mut lps = ExplorationLps {
        initial_state: initial_state_ldd.clone(),
        summand_groups,
    };
    let reachable = reachability(manager, &mut lps, &Timing::new())?;

    Ok(SymbolicLts::new(
        Mcrl2DataSpecification::default(),
        parameters,
        reachable,
        initial_state_ldd,
        lps.summand_groups,
        action_labels,
        parameter_values,
    ))
}

/// Minimal [SymbolicLPS] wrapper used during exploration before the final [SymbolicLts] is built.
struct ExplorationLps {
    initial_state: LDDFunction,
    summand_groups: Vec<SummandGroup>,
}

impl SymbolicLPS for ExplorationLps {
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
