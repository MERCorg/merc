use std::collections::HashSet;
use std::ops::ControlFlow;

use crate::StateFrm;
use crate::StateFrmKind;
use crate::Traverse;

/// A generator for fresh state variable *names*, not colliding with any fixpoint variable already
/// declared in a formula. Shared by `merc_typecheck::ModalEquationSystem::new`'s synthetic
/// placeholder fixpoint (a display name only — the id that actually matters for correctness comes
/// from a `StateVarIdAllocator`, not from this) and `merc_vpg::translate_regular_formulas`'s `*`/`+`
/// desugaring, which mints a genuinely fresh recursion variable for each rewritten modality.
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
