use std::cmp::Reverse;

use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::SortEnumerability;

use crate::LinearProcessSpecification;

/// Where a summation variable goes in the enumeration order, from first to last.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum EnumerationPriority {
    /// A sort whose elements are all constants, where the sorts with the most elements come first.
    Enumerated { elements: Reverse<usize> },
    /// Another finite sort, whose constructors have arguments.
    Finite,
    /// A sort that is not known to be finite.
    Unbounded,
}

/// Puts the summation variables of enumerated sorts with the most elements first, followed by
/// those of other finite sorts, followed by the rest.
pub fn order_summand_variables(
    mut lps: LinearProcessSpecification,
    data_specification: &Mcrl2DataSpecification,
) -> LinearProcessSpecification {
    let plans = EnumerationPlans::build(data_specification);
    let priority = |variable: &DataVariable| {
        let Some(plan) = plans.get(&variable.sort()).map(|id| plans.plan(id)) else {
            return EnumerationPriority::Unbounded;
        };
        if !matches!(plan.enumerability(), SortEnumerability::Finite) {
            EnumerationPriority::Unbounded
        } else if plan.constructors().iter().all(|constructor| constructor.arity() == 0) {
            EnumerationPriority::Enumerated {
                elements: Reverse(plan.constructors().len()),
            }
        } else {
            EnumerationPriority::Finite
        }
    };

    for summand in &mut lps.action_summands {
        summand.summation_variables.sort_by_key(priority);
    }
    lps
}
