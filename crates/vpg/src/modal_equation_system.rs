use std::collections::HashSet;
use std::convert::Infallible;
use std::fmt;
use std::ops::ControlFlow;

use log::debug;

use merc_syntax::FixedPointOperator;
use merc_syntax::StateFrm;
use merc_syntax::StateFrmKind;
use merc_syntax::StateVarDecl;
use merc_syntax::Traverse;
use merc_syntax::respan;
use merc_utilities::Span;

/// A fixpoint equation system representing a ranked set of fixpoint equations.
///
/// Each equation is of the shape `{mu, nu} X(args...) = rhs`. Where rhs
/// contains no further fixpoint equations.
pub struct ModalEquationSystem {
    equations: Vec<Equation>,
}

/// A single fixpoint equation of the shape `{mu, nu} X(args...) = rhs`.
#[derive(Clone)]
pub struct Equation {
    operator: FixedPointOperator,
    variable: StateVarDecl,
    rhs: StateFrm,
}

impl Equation {
    /// Returns the operator of the equation.
    pub fn operator(&self) -> FixedPointOperator {
        self.operator
    }

    /// Returns the variable declaration of the equation.
    pub fn variable(&self) -> &StateVarDecl {
        &self.variable
    }

    /// Returns the body of the equation.
    pub fn body(&self) -> &StateFrm {
        &self.rhs
    }
}

impl From<Equation> for StateFrm {
    fn from(val: Equation) -> Self {
        StateFrmKind::FixedPoint {
            operator: val.operator,
            variable: val.variable,
            body: Box::new(val.rhs),
        }
        .into()
    }
}

impl ModalEquationSystem {
    /// Converts a plain state formula into a fixpoint equation system.
    pub fn new(formula: &StateFrm) -> Self {
        let mut equations = Vec::new();
        let mut identifier_generator = FreshStateVarGenerator::new(formula);

        // Ensure that the formula has an outermost fixpoint operator.
        let formula = add_placeholder_operator(formula.clone(), &mut identifier_generator);

        // Apply E to extract all equations from the formula
        apply_e(&mut equations, &formula);

        // Check that there are no duplicate variable names
        let identifiers: HashSet<&String> = HashSet::from_iter(equations.iter().map(|eq| &eq.variable.identifier.node));
        assert_eq!(
            identifiers.len(),
            equations.len(),
            "Duplicate variable names found in fixpoint equation system"
        );

        debug_assert!(
            !equations.is_empty(),
            "At least one fixpoint equation expected in the equation system"
        );

        ModalEquationSystem { equations }
    }

    /// Returns the ith equation in the system.
    pub fn equation(&self, i: usize) -> &Equation {
        &self.equations[i]
    }

    /// Returns the number of equations in the system.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.equations.len()
    }

    /// Returns true if the system contains no equations.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.equations.is_empty()
    }

    /// The alternation depth is a complexity measure of the given formula.
    ///
    /// # Details
    ///
    /// The alternation depth of mu X . psi is defined as the maximum chain X <= X_1 <= ... <= X_n,
    /// where X <= Y iff X appears freely in the corresponding equation sigma Y . phi. And furthermore,
    /// X_0, X_2, ... are bound by mu and X_1, X_3, ... are bound by nu. Similarly, for nu X . psi. Note
    /// that the alternation depth of a formula with a rhs is always 1, since the chain cannot be extended.
    pub fn alternation_depth(&self, i: usize) -> usize {
        let equation = &self.equations[i];
        self.alternation_depth_rec(i, equation.body(), &equation.variable().identifier.node)
    }

    /// Finds an equation by its variable identifier.
    ///
    /// # Details
    ///
    /// This is a linear scan over the equations, and is also called from within
    /// [`Self::alternation_depth`]'s recursion. Equation systems correspond to a
    /// single (modal) formula and are therefore small, so an index map is not
    /// worth its maintenance cost; revisit if very large formulas become common.
    pub fn find_equation_by_identifier(&self, id: &str) -> Option<(usize, &Equation)> {
        self.equations
            .iter()
            .enumerate()
            .find(|(_, eq)| eq.variable.identifier.node == id)
    }

    /// Recursive helper function to compute the alternation depth of equation `i`.
    ///
    /// # Details
    ///
    /// The depth of a formula is the largest depth of the variables occurring in it, so the
    /// traversal only has to look at the [StateFrmKind::Id] leaves. A variable bound by a later
    /// equation continues the chain in that equation's body, which is a different formula and
    /// therefore a nested traversal.
    fn alternation_depth_rec(&self, i: usize, formula: &StateFrm, identifier: &String) -> usize {
        let equation = &self.equations[i];
        let mut depth = 0;

        formula.visit::<(), _>(|formula| {
            match &formula.node {
                StateFrmKind::Id(id, _) => {
                    depth = depth.max(if id.node == *identifier {
                        1
                    } else {
                        let (j, inner_equation) = self
                            .find_equation_by_identifier(id)
                            .expect("Equation not found for identifier");

                        if j > i {
                            self.alternation_depth_rec(j, &inner_equation.rhs, identifier)
                                + usize::from(inner_equation.operator != equation.operator)
                        } else {
                            // Only consider nested equations
                            0
                        }
                    });
                }
                StateFrmKind::Binary { .. }
                | StateFrmKind::Modality { .. }
                | StateFrmKind::True
                | StateFrmKind::False => {}
                _ => {
                    unimplemented!("Cannot determine alternation depth of formula {}", formula)
                }
            }

            ControlFlow::Continue(())
        });

        depth
    }
}

/// If the given formula has no outermost fixpoint operator, adds a placeholder
/// fixpoint operator around it.
fn add_placeholder_operator(formula: StateFrm, identifier_generator: &mut FreshStateVarGenerator) -> StateFrm {
    if matches!(formula.node, StateFrmKind::FixedPoint { .. }) {
        // The outer operator is already a fixpoint
        formula
    } else {
        // Introduce a placeholder.
        StateFrmKind::FixedPoint {
            operator: FixedPointOperator::Least,
            variable: StateVarDecl::new(respan(Span::default(), identifier_generator.generate("X")), Vec::new()),
            body: Box::new(formula),
        }
        .into()
    }
}

/// Applies `E` to the given formula, adding equations to the given vector.
///
/// E(nu X. f) = (nu X = RHS(f)) + E(f)
/// E(mu X. f) = (mu X = RHS(f)) + E(f)
/// E(g) = ... (traverse all the subformulas of g and apply E to them)
fn apply_e(equations: &mut Vec<Equation>, formula: &StateFrm) {
    debug!("Applying E to formula: {}", formula);

    formula.visit::<(), _>(|formula| {
        if let StateFrmKind::FixedPoint {
            operator,
            variable,
            body,
        } = &formula.node
        {
            debug!("Adding equation for variable {}", variable.identifier.node);
            // Add the equation with the renamed variable (the span is the same as the original variable).
            equations.push(Equation {
                operator: *operator,
                variable: variable.clone(),
                rhs: rhs(body),
            });
        }

        ControlFlow::Continue(())
    });
}

/// Applies `RHS` to the given formula.
///
/// ```text
/// RHS(true) = true
/// RHS(false) = false
/// RHS(<a>f) = <a>RHS(f)
/// RHS([a]f) = [a]RHS(f)
/// RHS(f1 && f2) = RHS(f1) && RHS(f2)
/// RHS(f1 || f2) = RHS(f1) || RHS(f2)
/// RHS(X) = X
/// RHS(mu X. f) = X(args)
/// RHS(nu X. f) = X(args)
/// ```
fn rhs(formula: &StateFrm) -> StateFrm {
    let result = formula.clone().apply::<Infallible, _>(|formula| match &formula.node {
        // RHS(mu X. phi) = X(args)
        StateFrmKind::FixedPoint { variable, .. } => Ok(Some(
            StateFrmKind::Id(
                variable.identifier.clone(),
                variable.arguments.iter().map(|arg| arg.expr.clone()).collect(),
            )
            .into(),
        )),
        _ => Ok(None),
    });

    match result {
        Ok(formula) => formula,
        Err(error) => match error {},
    }
}

/// A generator for fresh state variable names.
pub struct FreshStateVarGenerator {
    used: HashSet<String>,
}

impl FreshStateVarGenerator {
    /// Creates a new fresh state variable generator.
    ///
    /// # Details
    ///
    /// Traverses the given formula to collect all used variable names.
    pub fn new(formula: &StateFrm) -> Self {
        let mut used = HashSet::new();
        formula.visit::<(), _>(|subformula| {
            if let StateFrmKind::FixedPoint { variable, .. } = &subformula.node {
                used.insert(variable.identifier.node.clone());
            }

            ControlFlow::Continue(())
        });

        FreshStateVarGenerator { used }
    }

    /// Generates a fresh state variable name based on the given base.
    pub fn generate(&mut self, base: &str) -> String {
        let mut index = 0;
        loop {
            let candidate = format!("{}{}", base, index);
            if !self.used.contains(&candidate) {
                self.used.insert(candidate.clone());
                return candidate;
            }
            index += 1;
        }
    }
}

impl fmt::Display for ModalEquationSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, equation) in self.equations.iter().enumerate() {
            write!(f, "{i}: {} {} = {}", equation.operator, equation.variable, equation.rhs)?;
            if i + 1 < self.equations.len() {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use merc_macros::merc_test;
    use merc_syntax::UntypedStateFrmSpec;

    use super::ModalEquationSystem;

    #[merc_test]
    fn test_fixpoint_equation_system_construction() {
        let formula = UntypedStateFrmSpec::parse("mu X. [a]X && nu Y. <b>true")
            .unwrap()
            .formula;
        let fes = ModalEquationSystem::new(&formula);

        println!("{}", fes);

        assert_eq!(fes.equations.len(), 2);
        assert_eq!(fes.alternation_depth(0), 1);
        assert_eq!(fes.alternation_depth(1), 0);
    }

    #[merc_test]
    fn test_fixpoint_equation_system_example() {
        let formula = UntypedStateFrmSpec::parse(include_str!("../../../examples/vpg/running_example.mcf"))
            .unwrap()
            .formula;
        let fes = ModalEquationSystem::new(&formula);

        println!("{}", fes);

        assert_eq!(fes.equations.len(), 2);
        assert_eq!(fes.alternation_depth(0), 2);
        assert_eq!(fes.alternation_depth(1), 1);
    }

    #[merc_test]
    #[should_panic(expected = "Duplicate variable names found in fixpoint equation system")]
    fn test_fixpoint_equation_system_duplicates() {
        let formula = UntypedStateFrmSpec::parse("mu X. [a]X && (nu Y. <b>true) && (nu Y . <c>X)")
            .unwrap()
            .formula;
        let fes = ModalEquationSystem::new(&formula);

        println!("{}", fes);

        assert_eq!(fes.equations.len(), 3);
    }
}
