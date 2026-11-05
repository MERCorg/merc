use std::fmt;

use itertools::Itertools;
use merc_aterm::ATermString;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_data::SortExpression;
use merc_data::is_bool_literal;
use merc_lowering::Pretty;

/// An action label declaration `a: D₁ # … # Dₙ`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActionLabel {
    pub name: ATermString,
    pub sorts: Vec<SortExpression>,
}

/// An action `a(e₁, …, eₙ)`, where the arguments match the sorts of the label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Action {
    pub label: ActionLabel,
    pub arguments: Vec<DataExpression>,
}

/// A multi-action `a₁(…) | … | aₙ(…)`, optionally at time `t`. The empty multi-action is `tau`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MultiAction {
    pub actions: Vec<Action>,
    pub time: Option<DataExpression>,
}

/// An action summand `Σ d: D . c → α . P(next)` of a linear process.
///
/// In every state `s` that satisfies the condition for some values of the summation variables,
/// the multi-action can occur, after which the process continues in the state given by the
/// next-state expressions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActionSummand {
    pub summation_variables: Vec<DataVariable>,
    pub condition: DataExpression,
    pub action: MultiAction,
    /// The value of every process parameter after the action, parallel to
    /// [`LinearProcessSpecification::process_parameters`]. A parameter that the summand leaves
    /// unchanged is the parameter itself.
    pub next_state: Vec<DataExpression>,
}

/// A deadlock summand `Σ d: D . c → δ`, optionally at time `t`. It only restricts the passing of
/// time, and has no effect on the state space.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeadlockSummand {
    pub summation_variables: Vec<DataVariable>,
    pub condition: DataExpression,
    pub time: Option<DataExpression>,
}

/// A linear process specification (LPS): a single process whose body is a choice between summands,
/// each of which performs at most one multi-action before continuing as the process itself.
///
/// Unlike the term representation of the mCRL2 toolset, only the data expressions are terms; the
/// structure around them is plain Rust. Stochastic linear processes are not supported.
#[derive(Clone)]
pub struct LinearProcessSpecification {
    pub data_specification: Mcrl2DataSpecification,
    pub action_labels: Vec<ActionLabel>,
    /// Variables that may occur free in the process, standing for arbitrary values.
    pub global_variables: Vec<DataVariable>,
    pub process_parameters: Vec<DataVariable>,
    pub action_summands: Vec<ActionSummand>,
    pub deadlock_summands: Vec<DeadlockSummand>,
    /// The initial value of every process parameter, parallel to [`Self::process_parameters`].
    pub initial_state: Vec<DataExpression>,
}

impl fmt::Display for ActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name.value())?;
        if !self.sorts.is_empty() {
            write!(f, ": {}", self.sorts.iter().format(" # "))?;
        }
        Ok(())
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label.name.value())?;
        if !self.arguments.is_empty() {
            write!(f, "({})", self.arguments.iter().map(Pretty::from).format(", "))?;
        }
        Ok(())
    }
}

impl fmt::Display for MultiAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.actions.is_empty() {
            write!(f, "tau")?;
        } else {
            write!(f, "{}", self.actions.iter().format(" | "))?;
        }
        write!(f, "{}", Time(self.time.as_ref()))
    }
}

impl fmt::Display for DeadlockSummand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}delta{}",
            Guard {
                summation_variables: &self.summation_variables,
                condition: &self.condition,
            },
            Time(self.time.as_ref())
        )
    }
}

impl fmt::Display for LinearProcessSpecification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.data_specification)?;
        if !self.action_labels.is_empty() {
            writeln!(f, "act {};", self.action_labels.iter().format(", "))?;
        }
        if !self.global_variables.is_empty() {
            writeln!(f, "glob {};", Variables(&self.global_variables))?;
        }

        writeln!(f, "proc P({}) =", Variables(&self.process_parameters))?;
        let summands = self
            .action_summands
            .iter()
            .map(|summand| {
                SummandWithParameters {
                    summand,
                    parameters: &self.process_parameters,
                }
                .to_string()
            })
            .chain(self.deadlock_summands.iter().map(DeadlockSummand::to_string))
            .collect::<Vec<_>>();
        if summands.is_empty() {
            writeln!(f, "    delta;")?;
        } else {
            writeln!(f, "    {};", summands.iter().format("\n  + "))?;
        }

        writeln!(
            f,
            "init P({});",
            self.initial_state.iter().map(Pretty::from).format(", ")
        )
    }
}

/// Returns true iff `value` is the parameter itself, i.e. the parameter keeps its value.
pub(crate) fn is_parameter(parameter: &DataVariable, value: &DataExpression) -> bool {
    DataExpression::from(parameter.clone()) == *value
}

/// An action summand `sum d: D. c -> a . P(x = e)`, which shows only the parameters that change.
struct SummandWithParameters<'a> {
    summand: &'a ActionSummand,
    parameters: &'a [DataVariable],
}

impl fmt::Display for SummandWithParameters<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let assignments = self
            .parameters
            .iter()
            .zip(&self.summand.next_state)
            .filter(|(parameter, value)| !is_parameter(parameter, value))
            .format_with(", ", |(parameter, value), f| {
                f(&format_args!("{} = {}", parameter.name(), Pretty::from(value)))
            });
        write!(
            f,
            "{}{} . P({assignments})",
            Guard {
                summation_variables: &self.summand.summation_variables,
                condition: &self.summand.condition,
            },
            self.summand.action
        )
    }
}

/// The `sum d: D. c ->` in front of a summand, where the sum is left out when there are no
/// summation variables and the condition when it is `true`.
struct Guard<'a> {
    summation_variables: &'a [DataVariable],
    condition: &'a DataExpression,
}

impl fmt::Display for Guard<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.summation_variables.is_empty() {
            write!(f, "sum {}. ", Variables(self.summation_variables))?;
        }
        if !is_bool_literal(self.condition, true) {
            write!(f, "{} -> ", Pretty::from(self.condition))?;
        }
        Ok(())
    }
}

/// The time stamp ` @ t` of a multi-action or deadlock, if any.
struct Time<'a>(Option<&'a DataExpression>);

impl fmt::Display for Time<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(time) => write!(f, " @ {}", Pretty::from(time)),
            None => Ok(()),
        }
    }
}

/// A list of variable declarations `x: S, y: T`.
struct Variables<'a>(&'a [DataVariable]);

impl fmt::Display for Variables<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.0.iter().format_with(", ", |variable, f| f(&format_args!(
                "{}: {}",
                variable.name(),
                variable.sort()
            )))
        )
    }
}
