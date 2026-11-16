use std::collections::HashSet;

use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_sabre::utilities::SliceSubstitution;
use merc_sabre::utilities::apply_substitution;

use crate::LinearProcessSpecification;
use crate::transformations::map_summand::map_action_summand;
use crate::transformations::map_summand::map_deadlock_summand;

/// Renames the summation variables with the name of a process parameter, which the enumerator
/// would otherwise confuse with that parameter. The fresh names do not occur in `lps`, so this can
/// be applied at any point.
///
/// Returns the `(new, old)` variable pairs for every action summand, parallel to the action
/// summands of the returned LPS.
pub fn resolve_name_clashes(
    mut lps: LinearProcessSpecification,
) -> (LinearProcessSpecification, Vec<Vec<(DataVariable, DataVariable)>>) {
    let parameters: HashSet<String> = lps
        .process_parameters
        .iter()
        .map(|parameter| parameter.name().to_string())
        .collect();
    let mut used = lps.variable_names();

    let mut fresh = |variable: &DataVariable| {
        let mut index = 0usize;
        loop {
            let name = format!("{}_{index}", variable.name());
            if used.insert(name.clone()) {
                return DataVariable::with_sort(name.as_str(), variable.sort());
            }
            index += 1;
        }
    };

    let mut renamings = Vec::with_capacity(lps.action_summands.len());
    for summand in &mut lps.action_summands {
        let (old, new) = renaming(&mut summand.summation_variables, &parameters, &mut fresh);
        if !old.is_empty() {
            let images: Vec<DataExpression> = new.iter().cloned().map(DataExpression::from).collect();
            let sigma = SliceSubstitution::new(&old, &images);
            map_action_summand(summand, &|expression| apply_substitution(expression, &sigma));
        }
        renamings.push(new.into_iter().zip(old).collect());
    }
    for summand in &mut lps.deadlock_summands {
        let (old, new) = renaming(&mut summand.summation_variables, &parameters, &mut fresh);
        if !old.is_empty() {
            let images: Vec<DataExpression> = new.iter().cloned().map(DataExpression::from).collect();
            let sigma = SliceSubstitution::new(&old, &images);
            map_deadlock_summand(summand, &|expression| apply_substitution(expression, &sigma));
        }
    }

    debug_assert!(!lps.has_name_clashes(), "Resolving the name clashes left a clash");
    (lps, renamings)
}

/// Renames every variable of `variables` whose name is in `parameters`, returning the old and the
/// new variables.
fn renaming<F>(
    variables: &mut [DataVariable],
    parameters: &HashSet<String>,
    fresh: &mut F,
) -> (Vec<DataVariable>, Vec<DataVariable>)
where
    F: FnMut(&DataVariable) -> DataVariable,
{
    let mut old = Vec::new();
    let mut new = Vec::new();
    for variable in variables {
        if parameters.contains(variable.name()) {
            let renamed = fresh(variable);
            old.push(std::mem::replace(variable, renamed.clone()));
            new.push(renamed);
        }
    }
    (old, new)
}
