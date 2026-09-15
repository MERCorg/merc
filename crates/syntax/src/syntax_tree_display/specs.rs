use std::fmt;

use itertools::Itertools;

use crate::ActDecl;
use crate::Action;
use crate::ActionName;
use crate::CommExpr;
use crate::EqnDecl;
use crate::EqnSpec;
use crate::MultiAction;
use crate::MultiActionLabel;
use crate::PbesEquation;
use crate::PresEquation;
use crate::ProcDecl;
use crate::PropVarDecl;
use crate::PropVarInst;
use crate::Rename;
use crate::SortDecl;
use crate::UntypedDataSpecification;
use crate::UntypedPbes;
use crate::UntypedPres;
use crate::UntypedProcessSpecification;
use crate::UntypedStateFrmSpec;

impl fmt::Display for UntypedProcessSpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;

        if !self.action_declarations.is_empty() {
            writeln!(f, "act")?;
            for act_decl in &self.action_declarations {
                writeln!(f, "   {act_decl};")?;
            }

            writeln!(f)?;
        }

        if !self.process_declarations.is_empty() {
            writeln!(f, "proc")?;
            for proc_decl in &self.process_declarations {
                writeln!(f, "   {proc_decl};")?;
            }

            writeln!(f)?;
        }

        if !self.global_variables.is_empty() {
            writeln!(f, "glob")?;
            for var_decl in &self.global_variables {
                writeln!(f, "   {var_decl};")?;
            }

            writeln!(f)?;
        }

        if let Some(init) = &self.init {
            writeln!(f, "init {init};")?;
        }
        Ok(())
    }
}

impl fmt::Display for UntypedDataSpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.type_var_declarations.is_empty() {
            writeln!(f, "type_var")?;
            for decl in &self.type_var_declarations {
                writeln!(f, "   {};", decl.identifier)?;
            }

            writeln!(f)?;
        }

        if !self.sort_declarations.is_empty() {
            writeln!(f, "sort")?;
            for decl in &self.sort_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        if !self.constructor_declarations.is_empty() {
            writeln!(f, "cons")?;
            for decl in &self.constructor_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        if !self.map_declarations.is_empty() {
            writeln!(f, "map")?;
            for decl in &self.map_declarations {
                writeln!(f, "   {decl};")?;
            }

            writeln!(f)?;
        }

        for decl in &self.equation_declarations {
            writeln!(f, "{decl}")?;
        }
        Ok(())
    }
}

impl fmt::Display for UntypedPbes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;
        writeln!(f)?;
        if !self.global_variables.is_empty() {
            writeln!(f, "glob")?;
            for var_decl in &self.global_variables {
                writeln!(f, "   {var_decl};")?;
            }

            writeln!(f)?;
        }
        writeln!(f)?;

        if !self.equations.is_empty() {
            writeln!(f, "pbes")?;
            for equation in &self.equations {
                writeln!(f, "   {equation};")?;
            }
        }

        writeln!(f, "init {};", self.init)
    }
}

impl fmt::Display for PropVarInst {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.arguments.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.arguments.iter().format(", "))
        }
    }
}

impl fmt::Display for PbesEquation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} = {}", self.operator, self.variable, self.formula)
    }
}

impl fmt::Display for PropVarDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.parameters.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}({})", self.identifier, self.parameters.iter().format(", "))
        }
    }
}

impl fmt::Display for UntypedPres {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;
        writeln!(f)?;
        if !self.global_variables.is_empty() {
            writeln!(f, "glob")?;
            for var_decl in &self.global_variables {
                writeln!(f, "   {var_decl};")?;
            }

            writeln!(f)?;
        }
        writeln!(f)?;

        if !self.equations.is_empty() {
            writeln!(f, "pres")?;
            for equation in &self.equations {
                writeln!(f, "   {equation};")?;
            }
        }

        writeln!(f, "init {};", self.init)
    }
}

impl fmt::Display for PresEquation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} = {}", self.operator, self.variable, self.formula)
    }
}

impl fmt::Display for EqnSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // The grammar requires at least one declaration after `var`, so only
        // emit the section when there are variables to declare.
        if !self.variables.is_empty() {
            writeln!(f, "var")?;
            for decl in &self.variables {
                writeln!(f, "   {decl};")?;
            }
        }

        writeln!(f, "eqn")?;
        for decl in &self.equations {
            writeln!(f, "   {decl};")?;
        }
        Ok(())
    }
}

impl fmt::Display for SortDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.identifier)?;

        if let Some(expr) = &self.expr {
            write!(f, " = {expr}")?;
        }

        Ok(())
    }
}

impl fmt::Display for ActDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // An action declaration is `id: sort # sort # ...`, matching the
        // `IdList ~ ":" ~ SortProduct` grammar rule.
        if self.args.is_empty() {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "{}: {}", self.identifier, self.args.iter().format(" # "))
        }
    }
}

impl fmt::Display for EqnDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.condition {
            Some(condition) => write!(f, "{} -> {} = {}", condition, self.lhs, self.rhs),
            None => write!(f, "{} = {}", self.lhs, self.rhs),
        }
    }
}

impl fmt::Display for UntypedStateFrmSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.data_specification)?;

        // Wrap the formula in a `form ...;` section: the bare-formula grammar
        // alternative is only valid when no specification elements precede it.
        writeln!(f, "form {};", self.formula)
    }
}

impl fmt::Display for MultiAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.actions.is_empty() {
            write!(f, "tau")
        } else {
            write!(f, "{}", self.actions.iter().format("|"))
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.id)
        } else {
            write!(f, "{}({})", self.id, self.args.iter().format(", "))
        }
    }
}

impl fmt::Display for ProcDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.params.is_empty() {
            write!(f, "{} = {}", self.identifier, self.body)
        } else {
            write!(
                f,
                "{}({}) = {}",
                self.identifier,
                self.params.iter().format(", "),
                self.body
            )
        }
    }
}

impl fmt::Display for CommExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)
    }
}

impl fmt::Display for Rename {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.node)
    }
}

impl fmt::Display for MultiActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.actions.is_empty() {
            write!(f, "tau")
        } else {
            write!(f, "{}", self.actions.iter().format("|"))
        }
    }
}
