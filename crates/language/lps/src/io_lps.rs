use std::collections::HashMap;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;

use merc_aterm::ATerm;
use merc_aterm::ATermList;
use merc_aterm::ATermRead;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermReader;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Term;
use merc_aterm::is_list_term;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::DataWhrDecl;
use merc_data::Mcrl2DataSpecification;
use merc_data::is_undefined_real;
use merc_data::undefined_real;
use merc_lts::MCRL2Action;
use merc_lts::MCRL2ActionLabel;
use merc_lts::MCRL2ActionLabelRef;
use merc_utilities::MercError;

use crate::Action;
use crate::ActionLabel;
use crate::ActionSummand;
use crate::DeadlockSummand;
use crate::LinearProcessSpecification;
use crate::MultiAction;
use crate::lps_terms::Distribution;
use crate::lps_terms::DistributionRef;
use crate::lps_terms::LPS_SYMBOLS;
use crate::lps_terms::LinearProcessInit;

/// Reads a linear process specification in the binary `.lps` format of the mCRL2 toolset.
///
/// # Errors
///
/// Fails when the stream is not a well-formed LPS, and for a stochastic LPS, which is not
/// supported.
pub fn read_lps<R: Read>(reader: R) -> Result<LinearProcessSpecification, MercError> {
    let mut reader = BinaryATermReader::new(BufReader::new(reader))?;
    LinearProcessSpecification::read(&mut reader)
}

/// Writes a linear process specification in the binary `.lps` format of the mCRL2 toolset.
pub fn write_lps<W: Write>(lps: &LinearProcessSpecification, writer: W) -> Result<(), MercError> {
    let mut writer = BinaryATermWriter::new(BufWriter::new(writer))?;
    lps.write(&mut writer)?;
    writer.flush()
}

/// The LPS binary encoding that the mCRL2 toolset also uses.
/// 
/// Terms are written as is, and containers that are not terms as a count
/// followed by their elements. The linear process is its parameters, its action
/// summands and its deadlock summands. An action summand is its distribution,
/// summation variables, condition, multi-action (the actions and the time) and
/// assignments, and a deadlock summand is its summation variables, condition
/// and time.
impl ATermStreamable for LinearProcessSpecification {
    fn write<W: ATermWrite>(&self, writer: &mut W) -> Result<(), MercError> {
        // Write the LPS marker to indicate the start of a linear process specification.
        writer.write_aterm(&LPS_SYMBOLS.with(|symbols| ATerm::constant(&*symbols.marker)))?;

        // Write the data specification, action labels (as list) and global variables.
        self.data_specification.write(writer)?;
        writer.write_aterm(&ATermList::from_double_iter(self.action_labels.iter().map(action_label_term)).into())?;
        writer.write_aterm_iter(self.global_variables.iter().cloned().map(ATerm::from))?;

        writer.write_aterm(
            &ATermList::from_double_iter(self.process_parameters.iter().cloned().map(ATerm::from)).into(),
        )?;

        // Write the action summands.
        writer.write_aterm_int(self.action_summands.len())?;
        for summand in &self.action_summands {
            writer.write_aterm(&Distribution::none().into())?;
            writer.write_aterm(
                &ATermList::from_double_iter(summand.summation_variables.iter().cloned().map(ATerm::from)).into(),
            )?;
            writer.write_aterm(&summand.condition.clone().into())?;
            writer.write_aterm(&ATermList::from_double_iter(summand.action.actions.iter().map(action_term)).into())?;
            writer.write_aterm(&summand.action.time.clone().unwrap_or_else(undefined_real).into())?;
            writer.write_aterm(&assignments_term(self, &summand.next_state))?;
        }

        // Write the deadlock summands.
        writer.write_aterm_int(self.deadlock_summands.len())?;
        for summand in &self.deadlock_summands {
            writer.write_aterm(
                &ATermList::from_double_iter(summand.summation_variables.iter().cloned().map(ATerm::from)).into(),
            )?;
            writer.write_aterm(&summand.condition.clone().into())?;
            writer.write_aterm(&summand.time.clone().unwrap_or_else(undefined_real).into())?;
        }

        // Finally, write the initial state of the LPS.
        let initial_state = ATermList::from_double_iter(self.initial_state.iter().cloned());
        writer.write_aterm(&LinearProcessInit::new(initial_state, Distribution::none()).into())
    }

    fn read<R: ATermRead>(reader: &mut R) -> Result<Self, MercError> {
        let marker = next_term(reader, "LPS marker")?;
        if !LPS_SYMBOLS.with(|symbols| marker.get_head_symbol() == symbols.marker.copy()) {
            return Err("Stream does not contain a linear process specification (LPS).".into());
        }

        let data_specification = Mcrl2DataSpecification::read(reader)?;
        let action_labels = list_elements(&next_term(reader, "action labels")?, "action labels")?
            .into_iter()
            .map(|term| MCRL2ActionLabel::try_from_term(term).map(|label| read_action_label(label.copy())))
            .collect::<Result<Vec<_>, _>>()?;
        let global_variables = reader
            .read_aterm_iter()?
            .map(|term| Ok(DataVariable::try_from_term(term?)?))
            .collect::<Result<Vec<_>, MercError>>()?;

        let process_parameters = data_variables(&next_term(reader, "process parameters")?)?;
        let parameter_index: HashMap<DataVariable, usize> = process_parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| (parameter.clone(), index))
            .collect();
        if parameter_index.len() != process_parameters.len() {
            return Err("The process parameters of the LPS are not distinct".into());
        }

        let count = reader.read_aterm_int()?;
        let mut action_summands = Vec::with_capacity(count);
        for _ in 0..count {
            check_no_distribution(Distribution::try_from_term(next_term(reader, "distribution")?)?.copy())?;
            let summation_variables = data_variables(&next_term(reader, "summation variables")?)?;
            let condition = DataExpression::try_from_term(next_term(reader, "condition")?)?;
            let actions = list_elements(&next_term(reader, "actions")?, "actions")?
                .into_iter()
                .map(|action| read_action(action, &action_labels))
                .collect::<Result<Vec<_>, _>>()?;
            let time = read_time(next_term(reader, "time")?)?;

            let mut next_state: Vec<DataExpression> =
                process_parameters.iter().cloned().map(DataExpression::from).collect();
            let mut assigned = vec![false; process_parameters.len()];
            for assignment in list_elements(&next_term(reader, "assignments")?, "assignments")? {
                let assignment = DataWhrDecl::try_from_term(assignment)?;
                let (variable, value) = (assignment.variable().protect(), assignment.expression().protect());
                let index = *parameter_index
                    .get(&variable)
                    .ok_or_else(|| format!("Assignment to {variable}, which is not a process parameter"))?;
                if std::mem::replace(&mut assigned[index], true) {
                    return Err(format!("Process parameter {variable} is assigned twice in a summand").into());
                }
                next_state[index] = value;
            }

            action_summands.push(ActionSummand {
                summation_variables,
                condition,
                action: MultiAction { actions, time },
                next_state,
            });
        }

        let count = reader.read_aterm_int()?;
        let mut deadlock_summands = Vec::with_capacity(count);
        for _ in 0..count {
            deadlock_summands.push(DeadlockSummand {
                summation_variables: data_variables(&next_term(reader, "summation variables")?)?,
                condition: DataExpression::try_from_term(next_term(reader, "condition")?)?,
                time: read_time(next_term(reader, "time")?)?,
            });
        }

        let initializer = LinearProcessInit::try_from_term(next_term(reader, "initial process")?)?;
        check_no_distribution(initializer.distribution())?;
        let initial_state = initializer.initial_state().to_vec();
        if initial_state.len() != process_parameters.len() {
            return Err(format!(
                "The initial state has {} values, but there are {} process parameters",
                initial_state.len(),
                process_parameters.len()
            )
            .into());
        }

        Ok(LinearProcessSpecification {
            data_specification,
            action_labels,
            global_variables,
            process_parameters,
            action_summands,
            deadlock_summands,
            initial_state,
        })
    }
}

/// The assignments `x := e` of a summand, including the ones that keep the parameter unchanged.
fn assignments_term(lps: &LinearProcessSpecification, next_state: &[DataExpression]) -> ATerm {
    debug_assert_eq!(
        next_state.len(),
        lps.process_parameters.len(),
        "The next state must have a value for every process parameter in the LPS"
    );

    ATermList::from_double_iter(
        lps.process_parameters
            .iter()
            .cloned()
            .zip(next_state.iter().cloned())
            .map(|(parameter, value)| DataWhrDecl::new(parameter, value)),
    )
    .into()
}

fn next_term<R: ATermRead>(reader: &mut R, what: &str) -> Result<ATerm, MercError> {
    reader
        .read_aterm()?
        .ok_or_else(|| format!("Unexpected end of stream, expected the {what}").into())
}

/// The elements of the list `term`.
fn list_elements(term: &ATerm, what: &str) -> Result<Vec<ATerm>, MercError> {
    // `is_list_term` only recognises a non-empty list.
    if !is_list_term(term) && *term != ATerm::from(ATermList::<ATerm>::empty()) {
        return Err(format!("Expected a list of {what}, found {term}").into());
    }
    Ok(ATermList::<ATerm>::from(term.clone()).to_vec())
}

fn data_variables(term: &ATerm) -> Result<Vec<DataVariable>, MercError> {
    Ok(list_elements(term, "data variables")?
        .into_iter()
        .map(DataVariable::try_from_term)
        .collect::<Result<Vec<_>, _>>()?)
}

fn read_action_label(label: MCRL2ActionLabelRef<'_>) -> ActionLabel {
    ActionLabel {
        name: label.name().protect(),
        sorts: label.sorts().to_vec(),
    }
}

pub(crate) fn action_label_term(label: &ActionLabel) -> MCRL2ActionLabel {
    MCRL2ActionLabel::new(
        label.name.copy(),
        ATermList::from_double_iter(label.sorts.iter().cloned()),
    )
}

/// Reads an `Action(label, arguments)` term, whose label must be declared.
fn read_action(term: ATerm, action_labels: &[ActionLabel]) -> Result<Action, MercError> {
    let action = MCRL2Action::try_from_term(term)?;
    let label = read_action_label(action.label());
    if !action_labels.contains(&label) {
        return Err(format!("Action label {label} is not declared").into());
    }
    let arguments = action.arguments().to_vec();
    if arguments.len() != label.sorts.len() {
        return Err(format!("Action {label} is applied to {} arguments", arguments.len()).into());
    }
    Ok(Action { label, arguments })
}

fn action_term(action: &Action) -> MCRL2Action {
    MCRL2Action::new(
        action_label_term(&action.label).copy(),
        ATermList::from_double_iter(action.arguments.iter().cloned()),
    )
}

/// Reads a time stamp, which mCRL2 writes as the undefined real when there is none.
fn read_time(term: ATerm) -> Result<Option<DataExpression>, MercError> {
    if is_undefined_real(&term) {
        Ok(None)
    } else {
        Ok(Some(DataExpression::try_from_term(term)?))
    }
}

/// Checks that `distribution` has no variables, i.e. is not stochastic.
fn check_no_distribution(distribution: DistributionRef<'_>) -> Result<(), MercError> {
    if !distribution.variables().is_empty() {
        return Err("Stochastic linear process specifications are not supported".into());
    }
    Ok(())
}
