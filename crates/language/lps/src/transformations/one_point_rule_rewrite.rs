use log::info;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::is_bool_literal;
use merc_enumerate::QuantifierKind;
use merc_enumerate::simplify_one_point;
use merc_sabre::DataRewriter;
use merc_sabre::RewriteEngine;

use crate::ActionSummand;
use crate::LinearProcessSpecification;

/// What [one_point_rule_rewrite] removed from an LPS.
#[derive(Clone)]
pub struct OnePointEliminations {
    /// The index that every remaining action summand had before the transformation.
    pub summand_origin: Vec<usize>,

    /// The eliminated summation variables and their values, for every action summand before the
    /// transformation. The values only depend on the process parameters, since the LPS has no
    /// global variables.
    pub eliminated: Vec<Vec<(DataVariable, DataExpression)>>,
}

/// Eliminates summation variables whose value is determined by a conjunct `d == e` of the
/// condition, and drops the summands whose condition is false.
///
/// The global variables of `lps` must be instantiated first, see
/// [instantiate_global_variables](crate::transformations::instantiate_global_variables).
pub fn one_point_rule_rewrite(
    mut lps: LinearProcessSpecification,
    rewriter: &mut DataRewriter,
) -> (LinearProcessSpecification, OnePointEliminations) {
    debug_assert!(
        lps.global_variables.is_empty(),
        "The one point rule requires the global variables to be instantiated"
    );
    let mut eliminations = OnePointEliminations {
        summand_origin: Vec::new(),
        eliminated: Vec::with_capacity(lps.action_summands.len()),
    };

    lps.action_summands.retain_mut(|summand| {
        let index = eliminations.eliminated.len();
        let keep = if summand.summation_variables.is_empty() {
            eliminations.eliminated.push(Vec::new());
            true
        } else {
            eliminations.eliminated.push(rewrite_summand(summand, rewriter));
            !is_bool_literal(&summand.condition, false)
        };

        if keep {
            eliminations.summand_origin.push(index);
        }
        keep
    });

    let eliminated = eliminations.eliminated.iter().map(Vec::len).sum::<usize>();
    info!("The one point rule eliminated {eliminated} summation variable(s)");
    (lps, eliminations)
}

/// Applies the one point rule to `summand`, returning the eliminated variables and their values.
fn rewrite_summand(summand: &mut ActionSummand, rewriter: &mut DataRewriter) -> Vec<(DataVariable, DataExpression)> {
    let mut condition = rewriter.rewrite(&summand.condition);
    let mut rest: Vec<DataExpression> = summand.next_state.iter().flatten().cloned().collect();
    for action in &summand.action.actions {
        rest.extend(action.arguments.iter().cloned());
    }
    rest.extend(summand.action.time.iter().cloned());

    let (remaining, eliminated) = simplify_one_point(
        rewriter,
        QuantifierKind::Exists,
        &summand.summation_variables,
        &mut condition,
        &mut rest,
    );

    let mut rest = rest.into_iter();
    for value in summand.next_state.iter_mut().flatten() {
        *value = rest.next().expect("one value was pushed for every assignment");
    }
    for action in &mut summand.action.actions {
        for argument in &mut action.arguments {
            *argument = rest.next().expect("one value was pushed for every action argument");
        }
    }
    if let Some(time) = &mut summand.action.time {
        *time = rest.next().expect("one value was pushed for the time");
    }

    summand.summation_variables = remaining;
    summand.condition = condition;
    eliminated
}
