use std::collections::HashMap;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::mem::ManuallyDrop;

use merc_aterm::ATerm;
use merc_aterm::ATermList;
use merc_aterm::ATermRead;
use merc_aterm::ATermStreamable;
use merc_aterm::ATermString;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermReader;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_aterm::is_list_term;
use merc_data::BasicSort;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortExpression;
use merc_data::is_data_application;
use merc_data::is_data_binder;
use merc_data::is_data_function_symbol;
use merc_data::is_data_machine_number;
use merc_data::is_data_variable;
use merc_data::is_data_where_clause;
use merc_data::is_undefined_real;
use merc_data::undefined_real;
use merc_utilities::MercError;

use crate::Action;
use crate::ActionLabel;
use crate::ActionSummand;
use crate::DeadlockSummand;
use crate::LinearProcessSpecification;
use crate::MultiAction;
use crate::linear_process::is_parameter;

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

thread_local! {
    static LPS_SYMBOLS: LpsSymbols = LpsSymbols::new();
}

/// The function symbols of the mCRL2 terms that make up an LPS in its binary format, see
/// `libraries/core/include/mcrl2/core/detail/function_symbols.h` of the mCRL2 toolset.
///
/// The symbols are wrapped in `ManuallyDrop` so that their destructors never run at thread exit.
struct LpsSymbols {
    marker: ManuallyDrop<Symbol>,
    action_label: ManuallyDrop<Symbol>,
    action: ManuallyDrop<Symbol>,
    assignment: ManuallyDrop<Symbol>,
    distribution: ManuallyDrop<Symbol>,
    initializer: ManuallyDrop<Symbol>,
}

impl LpsSymbols {
    fn new() -> Self {
        LpsSymbols {
            marker: ManuallyDrop::new(Symbol::new("linear_process_specification", 0)),
            action_label: ManuallyDrop::new(Symbol::new("ActId", 2)),
            action: ManuallyDrop::new(Symbol::new("Action", 2)),
            assignment: ManuallyDrop::new(Symbol::new("DataVarIdInit", 2)),
            distribution: ManuallyDrop::new(Symbol::new("Distribution", 2)),
            initializer: ManuallyDrop::new(Symbol::new("LinearProcessInit", 2)),
        }
    }
}

/// The layout follows `lps_io.cpp` of the mCRL2 toolset: a marker, the data specification, the
/// action labels, the global variables, the linear process and the initial process.
///
/// Terms are written as is, and containers that are not terms as a count followed by their
/// elements. The linear process is its parameters, its action summands and its deadlock summands.
/// An action summand is its distribution, summation variables, condition, multi-action (the
/// actions and the time) and assignments, and a deadlock summand is its summation variables,
/// condition and time.
impl ATermStreamable for LinearProcessSpecification {
    fn write<W: ATermWrite>(&self, writer: &mut W) -> Result<(), MercError> {
        writer.write_aterm(&LPS_SYMBOLS.with(|symbols| ATerm::constant(&*symbols.marker)))?;
        self.data_specification.write(writer)?;
        writer.write_aterm(&list(self.action_labels.iter().map(action_label_term)))?;
        writer.write_aterm_iter(self.global_variables.iter().cloned().map(ATerm::from))?;

        writer.write_aterm(&list(self.process_parameters.iter().cloned().map(ATerm::from)))?;

        writer.write_aterm_int(self.action_summands.len())?;
        for summand in &self.action_summands {
            writer.write_aterm(&no_distribution())?;
            writer.write_aterm(&list(summand.summation_variables.iter().cloned().map(ATerm::from)))?;
            writer.write_aterm(&summand.condition.clone().into())?;
            writer.write_aterm(&list(summand.action.actions.iter().map(action_term)))?;
            writer.write_aterm(&time_term(summand.action.time.as_ref()))?;
            writer.write_aterm(&self.assignments_term(&summand.next_state))?;
        }

        writer.write_aterm_int(self.deadlock_summands.len())?;
        for summand in &self.deadlock_summands {
            writer.write_aterm(&list(summand.summation_variables.iter().cloned().map(ATerm::from)))?;
            writer.write_aterm(&summand.condition.clone().into())?;
            writer.write_aterm(&time_term(summand.time.as_ref()))?;
        }

        let initial_state = list(self.initial_state.iter().cloned().map(ATerm::from));
        writer.write_aterm(
            &LPS_SYMBOLS
                .with(|symbols| ATerm::with_args(&*symbols.initializer, &[initial_state, no_distribution()]).protect()),
        )
    }

    fn read<R: ATermRead>(reader: &mut R) -> Result<Self, MercError> {
        let marker = next_term(reader, "LPS marker")?;
        if !LPS_SYMBOLS.with(|symbols| marker.get_head_symbol() == symbols.marker.copy()) {
            return Err("Stream does not contain a linear process specification (LPS).".into());
        }

        let data_specification = Mcrl2DataSpecification::read(reader)?;
        let action_labels = list_elements(&next_term(reader, "action labels")?, "action labels")?
            .iter()
            .map(read_action_label)
            .collect::<Result<Vec<_>, _>>()?;
        let global_variables = reader
            .read_aterm_iter()?
            .map(|term| data_variable(term?))
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
            check_no_distribution(&next_term(reader, "distribution")?)?;
            let summation_variables = data_variables(&next_term(reader, "summation variables")?)?;
            let condition = data_expression(next_term(reader, "condition")?)?;
            let actions = list_elements(&next_term(reader, "actions")?, "actions")?
                .iter()
                .map(|action| read_action(action, &action_labels))
                .collect::<Result<Vec<_>, _>>()?;
            let time = read_time(next_term(reader, "time")?)?;

            let mut next_state: Vec<DataExpression> =
                process_parameters.iter().cloned().map(DataExpression::from).collect();
            let mut assigned = vec![false; process_parameters.len()];
            for assignment in list_elements(&next_term(reader, "assignments")?, "assignments")? {
                let (variable, value) = read_assignment(&assignment)?;
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
                condition: data_expression(next_term(reader, "condition")?)?,
                time: read_time(next_term(reader, "time")?)?,
            });
        }

        let initializer = next_term(reader, "initial process")?;
        if !LPS_SYMBOLS.with(|symbols| initializer.get_head_symbol() == symbols.initializer.copy()) {
            return Err(format!("Expected an initial process, found {initializer}").into());
        }
        check_no_distribution(&initializer.arg(1).protect())?;
        let initial_state = list_elements(&initializer.arg(0).protect(), "initial state")?
            .into_iter()
            .map(data_expression)
            .collect::<Result<Vec<_>, _>>()?;
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

impl LinearProcessSpecification {
    /// The assignments `x := e` of a summand, leaving out those that keep the parameter unchanged,
    /// as mCRL2 does.
    fn assignments_term(&self, next_state: &[DataExpression]) -> ATerm {
        debug_assert_eq!(
            next_state.len(),
            self.process_parameters.len(),
            "The next state must have a value for every process parameter"
        );
        LPS_SYMBOLS.with(|symbols| {
            list(
                self.process_parameters
                    .iter()
                    .zip(next_state)
                    .filter(|(parameter, value)| !is_parameter(parameter, value))
                    .map(|(parameter, value)| {
                        ATerm::with_args(
                            &*symbols.assignment,
                            &[ATerm::from(parameter.clone()), ATerm::from(value.clone())],
                        )
                        .protect()
                    }),
            )
        })
    }
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

/// The list term of the given elements.
fn list(elements: impl Iterator<Item = ATerm>) -> ATerm {
    let elements: Vec<ATerm> = elements.collect();
    ATermList::from_double_iter(elements.into_iter()).into()
}

fn data_variable(term: ATerm) -> Result<DataVariable, MercError> {
    if !is_data_variable(&term) {
        return Err(format!("Expected a data variable, found {term}").into());
    }
    Ok(term.into())
}

fn data_variables(term: &ATerm) -> Result<Vec<DataVariable>, MercError> {
    list_elements(term, "data variables")?
        .into_iter()
        .map(data_variable)
        .collect()
}

/// Checks that `term` is a data expression; only its head symbol is inspected.
fn data_expression(term: ATerm) -> Result<DataExpression, MercError> {
    let is_data_expression = is_data_variable(&term)
        || is_data_function_symbol(&term)
        || is_data_application(&term)
        || is_data_binder(&term)
        || is_data_where_clause(&term)
        || is_data_machine_number(&term);
    if !is_data_expression {
        return Err(format!("Expected a data expression, found {term}").into());
    }
    Ok(term.into())
}

/// Reads an `ActId(name, sorts)` term.
fn read_action_label(term: &ATerm) -> Result<ActionLabel, MercError> {
    if !LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.action_label.copy()) {
        return Err(format!("Expected an action label, found {term}").into());
    }
    let sorts = list_elements(&term.arg(1).protect(), "sorts")?
        .into_iter()
        .map(SortExpression::from)
        .collect();
    Ok(ActionLabel {
        name: ATermString::from(term.arg(0).protect()),
        sorts,
    })
}

fn action_label_term(label: &ActionLabel) -> ATerm {
    let sorts = list(label.sorts.iter().cloned().map(ATerm::from));
    LPS_SYMBOLS
        .with(|symbols| ATerm::with_args(&*symbols.action_label, &[ATerm::from(label.name.clone()), sorts]).protect())
}

/// Reads an `Action(label, arguments)` term, whose label must be declared.
fn read_action(term: &ATerm, action_labels: &[ActionLabel]) -> Result<Action, MercError> {
    if !LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.action.copy()) {
        return Err(format!("Expected an action, found {term}").into());
    }
    let label = read_action_label(&term.arg(0).protect())?;
    if !action_labels.contains(&label) {
        return Err(format!("Action label {label} is not declared").into());
    }
    let arguments = list_elements(&term.arg(1).protect(), "action arguments")?
        .into_iter()
        .map(data_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if arguments.len() != label.sorts.len() {
        return Err(format!("Action {label} is applied to {} arguments", arguments.len()).into());
    }
    Ok(Action { label, arguments })
}

fn action_term(action: &Action) -> ATerm {
    let arguments = list(action.arguments.iter().cloned().map(ATerm::from));
    LPS_SYMBOLS
        .with(|symbols| ATerm::with_args(&*symbols.action, &[action_label_term(&action.label), arguments]).protect())
}

/// Reads a `DataVarIdInit(variable, value)` term.
fn read_assignment(term: &ATerm) -> Result<(DataVariable, DataExpression), MercError> {
    if !LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.assignment.copy()) {
        return Err(format!("Expected an assignment, found {term}").into());
    }
    Ok((
        data_variable(term.arg(0).protect())?,
        data_expression(term.arg(1).protect())?,
    ))
}

/// Reads a time stamp, which mCRL2 writes as the undefined real when there is none.
fn read_time(term: ATerm) -> Result<Option<DataExpression>, MercError> {
    if is_undefined_real(&term) {
        Ok(None)
    } else {
        data_expression(term).map(Some)
    }
}

fn time_term(time: Option<&DataExpression>) -> ATerm {
    time.cloned().unwrap_or_else(undefined_real).into()
}

/// The distribution of a non-stochastic summand or initial process: `Distribution([], @NoValue)`.
fn no_distribution() -> ATerm {
    let no_value = DataVariable::with_sort("@NoValue", SortExpression::from(BasicSort::new("@NoValue")).copy());
    LPS_SYMBOLS.with(|symbols| {
        ATerm::with_args(
            &*symbols.distribution,
            &[ATermList::<ATerm>::empty().into(), ATerm::from(no_value)],
        )
        .protect()
    })
}

/// Checks that `term` is a distribution without variables, i.e. not stochastic.
fn check_no_distribution(term: &ATerm) -> Result<(), MercError> {
    if !LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.distribution.copy()) {
        return Err(format!("Expected a distribution, found {term}").into());
    }
    if !list_elements(&term.arg(0).protect(), "distribution variables")?.is_empty() {
        return Err("Stochastic linear process specifications are not supported".into());
    }
    Ok(())
}
