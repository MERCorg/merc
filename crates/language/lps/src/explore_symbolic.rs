use std::ops::ControlFlow;

use log::debug;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;

use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_lts::LtsAction;
use merc_lts::LtsMultiAction;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::ReachabilityResult;
use merc_symbolic::SummandGroup;
use merc_symbolic::SymbolicLPS;
use merc_symbolic::SymbolicLps;
use merc_symbolic::SymbolicLpsOptions;
use merc_symbolic::SymbolicLts;
use merc_symbolic::TransitionGroup;
use merc_symbolic::reachability_with_callback;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::explore_explicit::ExplicitLinearProcessSpecification;
use crate::transformations::TransformedLps;

/// Explore the linear process specification using symbolic reachability.
///
/// The summand machinery (read/write positions, condition enumeration) is reused
/// from the explicit [`ExplicitLinearProcessSpecification`] via the generic
/// [`SymbolicLps`] adapter, so LPS and PBES symbolic exploration share one
/// implementation. The `encoding` decides how the summands are distributed over
/// the transition groups and in which order their parameters are stored,
/// mirroring the `--groups` and `--reorder` options of mCRL2's `lpsreach`.
///
/// The LPS is explored as given: any preprocessing (see [`preprocess`](crate::transformations::preprocess))
/// is the caller's responsibility.
///
/// This only reports the reachable state (and, when requested, deadlock)
/// count; it does not retain the transition relation or action labels
/// discovered along the way. Use [`explore_lps_symbolic_to_sym`] to keep those
/// and write a real symbolic LTS.
///
/// `on_iteration` is called after every iteration of the exploration with the number of iterations that
/// have completed and the states found so far, and stops the exploration early by returning
/// [`ControlFlow::Break`], see [`merc_symbolic::reachability_with_callback`].
pub fn explore_lps_symbolic<F: FnMut(usize, &LDDFunction) -> ControlFlow<()>>(
    storage: &LDDManagerRef,
    lps: TransformedLps,
    encoding: &SymbolicLpsOptions,
    options: &ReachabilityOptions,
    timing: &Timing,
    on_iteration: F,
) -> Result<ReachabilityResult, MercError> {
    let lps = ExplicitLinearProcessSpecification::new(lps)?;
    let mut symbolic = SymbolicLps::with_options(storage, lps, encoding)?;

    debug!("{symbolic:?}");

    let mut context = symbolic.create_context();
    reachability_with_callback(storage, &mut symbolic, &mut context, options, timing, on_iteration)
}

/// Explore the linear process specification using symbolic reachability and
/// assemble the result into a [`SymbolicLts`] carrying the real data
/// specification, process parameters, parameter values, action labels and
/// transition relation — suitable for [`merc_symbolic::write_symbolic_lts`].
///
/// `on_iteration` is called after every iteration of the exploration with the number of iterations that
/// have completed and the states found so far, and stops the exploration early by returning
/// [`ControlFlow::Break`], see [`merc_symbolic::reachability_with_callback`].
pub fn explore_lps_symbolic_to_sym<F: FnMut(usize, &LDDFunction) -> ControlFlow<()>>(
    storage: &LDDManagerRef,
    lps: TransformedLps,
    encoding: &SymbolicLpsOptions,
    options: &ReachabilityOptions,
    timing: &Timing,
    on_iteration: F,
) -> Result<(SymbolicLts<LtsMultiAction<LtsAction>>, Option<LDDFunction>), MercError> {
    let lps = ExplicitLinearProcessSpecification::new(lps)?;
    let mut symbolic = SymbolicLps::with_options(storage, lps, encoding)?;

    debug!("{symbolic:?}");

    let mut context = symbolic.create_context();
    let result = reachability_with_callback(storage, &mut symbolic, &mut context, options, timing, on_iteration)?;

    let permuted = symbolic.lps();
    let inner = permuted.inner();
    let order = permuted.order();

    // The data specification never changes during exploration, so it is only
    // converted once here rather than tracked incrementally.
    let data_specification = inner.user_data_specification();

    // `order[i]` is the position of `inner`'s (unpermuted) state vector stored
    // at position `i` of the diagrams `symbolic` builds, so the process
    // parameters in diagram order are `inner.parameters()[order[i]]`.
    let inner_parameters = inner.parameters();
    let process_parameters: Vec<DataVariable> = order
        .iter()
        .map(|&position| inner_parameters[position].clone())
        .collect();

    // The values observed for each process parameter, in the dense order the
    // diagrams use, i.e. matching the LDD values stored at that position.
    let mut parameter_values: Vec<Vec<DataExpression>> = Vec::with_capacity(process_parameters.len());
    for column in context.columns() {
        let mut values = Vec::with_capacity(column.len());
        for i in 0..column.len() {
            let index = *column.get_by_index(i).expect("dense index must be present");
            values.push(inner.value(index));
        }
        parameter_values.push(values);
    }

    // The action labels observed during exploration, again in dense order.
    let labels = context.labels();
    let mut action_labels: Vec<LtsMultiAction<LtsAction>> = Vec::with_capacity(labels.len());
    for i in 0..labels.len() {
        let label = labels.get_by_index(i).expect("dense index must be present");
        action_labels.push(label.to_lts_multi_action()?);
    }

    // Every transition group, translated from diagram positions to the
    // process parameters they read/write.
    let mut summand_groups = Vec::with_capacity(symbolic.transition_groups().len());
    for group in symbolic.transition_groups() {
        let read_parameters: Vec<DataVariable> = group
            .read_indices()
            .iter()
            .map(|&position| process_parameters[position as usize].clone())
            .collect();
        let write_parameters: Vec<DataVariable> = group
            .write_indices()
            .iter()
            .map(|&position| process_parameters[position as usize].clone())
            .collect();

        summand_groups.push(SummandGroup::new(
            storage,
            &process_parameters,
            read_parameters,
            write_parameters,
            group.relation().clone(),
        )?);
    }

    let lts = SymbolicLts::new(
        data_specification,
        process_parameters,
        result.states,
        symbolic.initial_state().clone(),
        summand_groups,
        action_labels,
        parameter_values,
    );

    Ok((lts, result.deadlocks))
}
