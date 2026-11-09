use std::ops::ControlFlow;

use merc_utilities::FreshNameGenerator;

use crate::StateFrm;
use crate::StateFrmKind;
use crate::Traverse;

/// A generator for fresh state variable *names*, not colliding with any fixpoint variable already
/// declared in `formula`.
pub fn fresh_state_var_generator(formula: &StateFrm) -> FreshNameGenerator {
    let mut used = Vec::new();
    formula.visit::<(), _>(|subformula| {
        if let StateFrmKind::FixedPoint { variable, .. } = &subformula.node {
            used.push(variable.identifier.node.clone());
        }

        ControlFlow::Continue(())
    });

    FreshNameGenerator::new(used)
}
