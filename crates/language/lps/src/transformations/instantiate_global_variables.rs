use log::debug;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_sabre::utilities::SliceSubstitution;
use merc_sabre::utilities::apply_substitution;
use merc_utilities::MercError;

use crate::LinearProcessSpecification;
use crate::transformations::map_summand::map_action_summand;
use crate::transformations::map_summand::map_deadlock_summand;
use crate::transformations::representative_generator::RepresentativeGenerator;

/// Replaces every global variable by a representative of its sort, returning the value that was
/// chosen for every variable.
///
/// # Errors
///
/// Fails when a global variable has a sort without a representative.
pub fn instantiate_global_variables(
    mut lps: LinearProcessSpecification,
    data_specification: &Mcrl2DataSpecification,
) -> Result<(LinearProcessSpecification, Vec<(DataVariable, DataExpression)>), MercError> {
    if lps.global_variables.is_empty() {
        return Ok((lps, Vec::new()));
    }

    let mut generator = RepresentativeGenerator::new(data_specification);
    let mut values = Vec::new();
    for variable in &lps.global_variables {
        let sort = variable.sort().protect();
        let value = generator.representative(&sort).ok_or_else(|| {
            MercError::from(format!(
                "Could not instantiate global variable {}: its sort has no representative",
                variable.name()
            ))
        })?;
        debug!("Instantiating global variable {} with {value}", variable.name());
        values.push(value);
    }

    let variables = std::mem::take(&mut lps.global_variables);
    let sigma = SliceSubstitution::new(&variables, &values);
    for summand in &mut lps.action_summands {
        map_action_summand(summand, &|expression| apply_substitution(expression, &sigma));
    }
    for summand in &mut lps.deadlock_summands {
        map_deadlock_summand(summand, &|expression| apply_substitution(expression, &sigma));
    }
    for value in &mut lps.initial_state {
        *value = apply_substitution(value, &sigma);
    }

    let assignments = variables.iter().cloned().zip(values.iter().cloned()).collect();
    Ok((lps, assignments))
}
