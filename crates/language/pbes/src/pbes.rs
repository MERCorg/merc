use std::fmt;

use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;

use crate::FixpointSymbol;
use crate::PbesExpression;
use crate::PropositionalVariable;
use crate::PropositionalVariableInstantiation;
use crate::fmt_variables;

/// An equation `σ X(d: D) = φ`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PbesEquation {
    pub symbol: FixpointSymbol,
    pub variable: PropositionalVariable,
    pub formula: PbesExpression,
}

/// A parameterised boolean equation system, whose data is typed and lowered into the terms that a
/// rewriter is built from.
#[derive(Clone)]
pub struct Pbes {
    pub data_specification: Mcrl2DataSpecification,
    /// Variables that may occur free in the equations, standing for arbitrary values.
    pub global_variables: Vec<DataVariable>,
    pub equations: Vec<PbesEquation>,
    pub initial_state: PropositionalVariableInstantiation,
}

impl fmt::Display for PbesEquation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} = {};", self.symbol, self.variable, self.formula)
    }
}

impl fmt::Display for Pbes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.data_specification)?;
        if !self.global_variables.is_empty() {
            write!(f, "glob ")?;
            fmt_variables(f, &self.global_variables)?;
            writeln!(f, ";")?;
        }
        writeln!(f, "pbes")?;
        for equation in &self.equations {
            writeln!(f, "  {equation}")?;
        }
        writeln!(f, "init {};", self.initial_state)
    }
}
