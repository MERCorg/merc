use merc_data::DataVariable;
use merc_data::Mcrl2DataSpecification;
use merc_utilities::optimized_and;
use merc_utilities::optimized_conjunction;
use merc_utilities::optimized_disjunction;
use merc_utilities::optimized_or;

use crate::FixpointSymbol;
use crate::Pbes;
use crate::PbesEquation;
use crate::PbesExpression;
use crate::PropositionalVariable;
use crate::PropositionalVariableInstantiation;
use crate::distribute_not_over_and;

/// A single summand of an [SrfEquation]: `exists/forall parameters . condition <> variable`, where
/// `<>` is `&&` for a disjunctive equation and `||` (with `condition` negated) for a conjunctive
/// one; see `SrfSummand::to_pbes_expression`.
#[derive(Clone, Debug)]
pub struct SrfSummand {
    pub(crate) parameters: Vec<DataVariable>,
    pub(crate) condition: PbesExpression,
    pub(crate) variable: PropositionalVariableInstantiation,
}

impl SrfSummand {
    pub(crate) fn new(
        parameters: Vec<DataVariable>,
        condition: PbesExpression,
        variable: PropositionalVariableInstantiation,
    ) -> Self {
        SrfSummand {
            parameters,
            condition,
            variable,
        }
    }

    pub fn parameters(&self) -> &[DataVariable] {
        &self.parameters
    }

    pub fn condition(&self) -> &PbesExpression {
        &self.condition
    }

    pub fn variable(&self) -> &PropositionalVariableInstantiation {
        &self.variable
    }

    pub(crate) fn add_variables(&mut self, variables: &[DataVariable]) {
        let mut combined = variables.to_vec();
        combined.append(&mut self.parameters);
        self.parameters = combined;
    }

    pub(crate) fn add_condition(&mut self, extra: &PbesExpression) {
        self.condition = optimized_and(extra.clone(), self.condition.clone());
    }

    pub(crate) fn to_pbes_expression(&self, conjunctive: bool) -> PbesExpression {
        let variable = PbesExpression::from(self.variable.clone());
        let body = if conjunctive {
            optimized_or(distribute_not_over_and(&self.condition.copy()), variable)
        } else {
            optimized_and(self.condition.clone(), variable)
        };

        if self.parameters.is_empty() {
            body
        } else if conjunctive {
            PbesExpression::forall(&self.parameters, &body)
        } else {
            PbesExpression::exists(&self.parameters, &body)
        }
    }
}

/// A PBES equation in standard recursive form: either purely conjunctive or purely disjunctive,
/// never a mix.
#[derive(Clone, Debug)]
pub struct SrfEquation {
    pub(crate) symbol: FixpointSymbol,
    pub(crate) variable: PropositionalVariable,
    pub(crate) summands: Vec<SrfSummand>,
    pub(crate) conjunctive: bool,
}

impl SrfEquation {
    pub(crate) fn new(
        symbol: FixpointSymbol,
        variable: PropositionalVariable,
        summands: Vec<SrfSummand>,
        conjunctive: bool,
    ) -> Self {
        SrfEquation {
            symbol,
            variable,
            summands,
            conjunctive,
        }
    }

    pub fn symbol(&self) -> FixpointSymbol {
        self.symbol
    }

    pub fn variable(&self) -> &PropositionalVariable {
        &self.variable
    }

    pub fn summands(&self) -> &[SrfSummand] {
        &self.summands
    }

    pub fn is_conjunctive(&self) -> bool {
        self.conjunctive
    }

    /// Joins this equation's summands back into a plain PBES equation.
    pub fn to_pbes_equation(&self) -> PbesEquation {
        let summands = self
            .summands
            .iter()
            .map(|summand| summand.to_pbes_expression(self.conjunctive));
        let formula = if self.conjunctive {
            optimized_conjunction(summands)
        } else {
            optimized_disjunction(summands)
        };
        PbesEquation::new(self.symbol, self.variable.clone(), formula)
    }

    /// Adds the summand that lets this equation escape to `true`/`false`, so that every parameter
    /// valuation has at least one applicable summand. `true_summand`/`false_summand` must be the
    /// `X_true`/`X_false` equations' own self-referencing summand (see [SrfPbes::make_total]).
    fn make_total(&mut self, true_summand: &SrfSummand, false_summand: &SrfSummand) {
        if self.conjunctive {
            self.summands.push(true_summand.clone());
        } else {
            self.summands.push(false_summand.clone());
        }
    }
}

/// A PBES in standard recursive form, as produced by [crate::pbes_to_pre_srf]/[crate::pbes_to_srf].
#[derive(Clone)]
pub struct SrfPbes {
    pub(crate) data_specification: Mcrl2DataSpecification,
    pub(crate) global_variables: Vec<DataVariable>,
    /// The equations of the input PBES and those spawned from them, without the escape equations.
    pub(crate) equations: Vec<SrfEquation>,
    /// The generated `mu X_false = false || X_false` equation.
    pub(crate) false_equation: SrfEquation,
    /// The generated `nu X_true = X_true` equation.
    pub(crate) true_equation: SrfEquation,
    pub(crate) initial_state: PropositionalVariableInstantiation,
}

impl SrfPbes {
    pub(crate) fn new(
        data_specification: Mcrl2DataSpecification,
        global_variables: Vec<DataVariable>,
        equations: Vec<SrfEquation>,
        false_equation: SrfEquation,
        true_equation: SrfEquation,
        initial_state: PropositionalVariableInstantiation,
    ) -> Self {
        SrfPbes {
            data_specification,
            global_variables,
            equations,
            false_equation,
            true_equation,
            initial_state,
        }
    }

    pub fn data_specification(&self) -> &Mcrl2DataSpecification {
        &self.data_specification
    }

    pub fn global_variables(&self) -> &[DataVariable] {
        &self.global_variables
    }

    /// Returns all equations, ending with the generated `X_false`/`X_true` equations in that order.
    pub fn equations(&self) -> impl Iterator<Item = &SrfEquation> {
        self.equations.iter().chain([&self.false_equation, &self.true_equation])
    }

    pub fn false_equation(&self) -> &SrfEquation {
        &self.false_equation
    }

    pub fn true_equation(&self) -> &SrfEquation {
        &self.true_equation
    }

    pub fn initial_state(&self) -> &PropositionalVariableInstantiation {
        &self.initial_state
    }

    /// Joins every equation's summands back into a plain PBES.
    pub fn to_pbes(&self) -> Pbes {
        Pbes::new(
            self.data_specification.clone(),
            self.global_variables.clone(),
            self.equations().map(SrfEquation::to_pbes_equation).collect(),
            self.initial_state.clone(),
        )
    }

    /// Adds the escape summands of `SrfEquation::make_total` to every equation but the
    /// `X_false`/`X_true` ones, so that every parameter valuation has at least one applicable
    /// summand.
    pub fn make_total(&mut self) {
        let false_summand = &self.false_equation.summands[0];
        let true_summand = &self.true_equation.summands[0];
        for equation in &mut self.equations {
            equation.make_total(true_summand, false_summand);
        }
    }
}
