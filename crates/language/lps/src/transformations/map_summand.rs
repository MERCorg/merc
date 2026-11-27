use merc_data::DataExpression;

use crate::Action;
use crate::ActionSummand;
use crate::DeadlockSummand;
use crate::MultiAction;

/// Applies `f` to every data expression of `summand`.
pub(super) fn map_action_summand<F>(summand: &mut ActionSummand, f: &F)
where
    F: Fn(&DataExpression) -> DataExpression,
{
    summand.condition = f(&summand.condition);
    let MultiAction { actions, time } = &mut summand.action;
    for Action { arguments, .. } in actions {
        for argument in arguments {
            *argument = f(argument);
        }
    }
    if let Some(time) = time {
        *time = f(time);
    }
    for value in summand.next_state.iter_mut().flatten() {
        *value = f(value);
    }
}

/// Applies `f` to every data expression of `summand`.
pub(super) fn map_deadlock_summand<F>(summand: &mut DeadlockSummand, f: &F)
where
    F: Fn(&DataExpression) -> DataExpression,
{
    summand.condition = f(&summand.condition);
    if let Some(time) = &mut summand.time {
        *time = f(time);
    }
}
