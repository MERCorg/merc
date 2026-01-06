use crate::DataAbstractionRef;
use crate::DataApplicationRef;
use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataFunctionSymbolRef;
use crate::DataMachineNumberRef;
use crate::DataUntypedIdentifierRef;
use crate::DataVariableRef;
use crate::DataWhereClauseRef;
use crate::PbesAndRef;
use crate::PbesExistsRef;
use crate::PbesExpression;
use crate::PbesForallRef;
use crate::PbesImpRef;
use crate::PbesNotRef;
use crate::PbesOrRef;
use crate::PbesPropositionalVariableInstantiationRef;
use crate::is_abstraction;
use crate::is_application;
use crate::is_function_symbol;
use crate::is_machine_number;
use crate::is_pbes_and;
use crate::is_pbes_exists;
use crate::is_pbes_forall;
use crate::is_pbes_imp;
use crate::is_pbes_not;
use crate::is_pbes_or;
use crate::is_pbes_propositional_variable_instantiation;
use crate::is_untyped_identifier;
use crate::is_variable;
use crate::is_where_clause;

pub trait DataExpressionVisitor {
    fn visit_variable(&mut self, var: &DataVariableRef<'_>) -> DataExpression {
        DataExpression::from(var.protect())
    }

    fn visit_application(&mut self, appl: &DataApplicationRef<'_>) -> DataExpression {
        DataExpression::from(appl.protect())
    }

    fn visit_abstraction(&mut self, abstraction: &DataAbstractionRef<'_>) -> DataExpression {
        DataExpression::from(abstraction.protect())
    }

    fn visit_function_symbol(&mut self, function_symbol: &DataFunctionSymbolRef<'_>) -> DataExpression {
        DataExpression::from(function_symbol.protect())
    }

    fn visit_where_clause(&mut self, where_: &DataWhereClauseRef<'_>) -> DataExpression {
        DataExpression::from(where_.protect())
    }

    fn visit_machine_number(&mut self, number: &DataMachineNumberRef<'_>) -> DataExpression {
        DataExpression::from(number.protect())
    }

    fn visit_untyped_identifier(&mut self, identifier: &DataUntypedIdentifierRef<'_>) -> DataExpression {
        DataExpression::from(identifier.protect())
    }

    fn visit(&mut self, expr: &DataExpressionRef<'_>) -> DataExpression {
        if is_variable(&expr.copy()) {
            self.visit_variable(&DataVariableRef::from(expr.copy()))
        } else if is_application(&expr.copy()) {
            self.visit_application(&DataApplicationRef::from(expr.copy()))
        } else if is_abstraction(&expr.copy()) {
            self.visit_abstraction(&DataAbstractionRef::from(expr.copy()))
        } else if is_function_symbol(&expr.copy()) {
            self.visit_function_symbol(&DataFunctionSymbolRef::from(expr.copy()))
        } else if is_where_clause(&expr.copy()) {
            self.visit_where_clause(&DataWhereClauseRef::from(expr.copy()))
        } else if is_machine_number(&expr.copy()) {
            self.visit_machine_number(&DataMachineNumberRef::from(expr.copy()))
        } else if is_untyped_identifier(&expr.copy()) {
            self.visit_untyped_identifier(&DataUntypedIdentifierRef::from(expr.copy()))
        } else {
            unreachable!("Unknown data expression type");
        }
    }
}

pub trait PbesExpressionVisitor {
    fn visit_propositional_variable_instantiation(
        &mut self,
        inst: &PbesPropositionalVariableInstantiationRef<'_>,
    ) -> PbesExpression {
        PbesExpression::from(inst.protect())
    }

    fn visit_not(&mut self, or: &PbesNotRef<'_>) -> PbesExpression {
        PbesExpression::from(or.protect())
    }

    fn visit_and(&mut self, and: &PbesAndRef<'_>) -> PbesExpression {
        PbesExpression::from(and.protect())
    }

    fn visit_or(&mut self, or: &PbesOrRef<'_>) -> PbesExpression {
        PbesExpression::from(or.protect())
    }

    fn visit_imp(&mut self, imp: &PbesImpRef<'_>) -> PbesExpression {
        PbesExpression::from(imp.protect())
    }

    fn visit_forall(&mut self, forall: &PbesForallRef<'_>) -> PbesExpression {
        PbesExpression::from(forall.protect())
    }

    fn visit_exists(&mut self, exists: &PbesExistsRef<'_>) -> PbesExpression {
        PbesExpression::from(exists.protect())
    }

    fn visit(&mut self, expr: &PbesExpression) -> PbesExpression {
        if is_pbes_propositional_variable_instantiation(&expr.copy()) {
            self.visit_propositional_variable_instantiation(&PbesPropositionalVariableInstantiationRef::from(
                expr.copy(),
            ))
        } else if is_pbes_not(&expr.copy()) {
            self.visit_not(&PbesNotRef::from(expr.copy()))
        } else if is_pbes_and(&expr.copy()) {
            self.visit_and(&PbesAndRef::from(expr.copy()))
        } else if is_pbes_or(&expr.copy()) {
            self.visit_or(&PbesOrRef::from(expr.copy()))
        } else if is_pbes_imp(&expr.copy()) {
            self.visit_imp(&PbesImpRef::from(expr.copy()))
        } else if is_pbes_forall(&expr.copy()) {
            self.visit_forall(&PbesForallRef::from(expr.copy()))
        } else if is_pbes_exists(&expr.copy()) {
            self.visit_exists(&PbesExistsRef::from(expr.copy()))
        } else {
            unreachable!("Unknown pbes expression type");
        }
    }
}

/// Replaces data variables in the given data expression according to the
/// provided substitution function.
pub fn data_expression_replace_variables<F>(expr: &DataExpressionRef<'_>, f: &F) -> DataExpression
where
    F: Fn(&DataVariableRef<'_>) -> DataExpression,
{
    struct ReplaceVariableBuilder<'a, F> {
        apply: &'a F,
    }

    impl<'a, F> DataExpressionVisitor for ReplaceVariableBuilder<'a, F>
    where
        F: Fn(&DataVariableRef<'_>) -> DataExpression,
    {
        fn visit_variable(&mut self, var: &DataVariableRef<'_>) -> DataExpression {
            (*self.apply)(var)
        }
    }

    let mut builder = ReplaceVariableBuilder { apply: f };
    builder.visit(expr)
}


pub fn pbes_expression_pvi(expr: &PbesExpression) -> Vec<PbesPropositionalVariableInstantiation> {
    struct PviChecker;

    impl PbesExpressionVisitor for PviChecker {
        fn visit_propositional_variable_instantiation(
            &mut self,
            _inst: &PbesPropositionalVariableInstantiationRef<'_>,
        ) -> PbesExpression {
            // Found a propositional variable instantiation, return true.
            PbesExpression::from(_inst.protect())
        }
    }

    let mut checker = PviChecker;
    let result = checker.visit(expr);
    is_pbes_propositional_variable_instantiation(&result.copy())
}