use merc_syntax::PbesExpr;
use merc_syntax::PbesExprKind;
use merc_syntax::UntypedPbes;
use merc_syntax::VarIdAllocator;
use merc_typecheck::internal::VariableScope as Scope;
use merc_typecheck::internal::resolve_in_data_expr;
use merc_typecheck::internal::resolve_in_prop_var_inst;

/// Resolves every free variable reference in `pbes`'s equation bodies and `init`.
pub(crate) fn resolve_pbes_variables(pbes: &mut UntypedPbes) {
    let mut ids = VarIdAllocator::default();
    let globals = Scope::from_declarations(&mut pbes.global_variables, &mut ids);

    for equation in &mut pbes.equations {
        let mut scope = globals.clone();
        scope.push_declarations(&mut equation.variable.parameters, &mut ids);
        resolve_in_pbes_expr(&mut equation.formula, &mut scope, &mut ids);
    }

    // `init` sits outside every equation's own parameter scope — only globals apply.
    let mut scope = globals.clone();
    resolve_in_prop_var_inst(&mut pbes.init, &mut scope, &mut ids);
}

fn resolve_in_pbes_expr(expr: &mut PbesExpr, scope: &mut Scope, ids: &mut VarIdAllocator) {
    match &mut expr.node {
        PbesExprKind::True | PbesExprKind::False => {}
        PbesExprKind::DataValExpr(data_expr) => resolve_in_data_expr(data_expr, scope, ids),
        PbesExprKind::PropVarInst(inst) => resolve_in_prop_var_inst(inst, scope, ids),
        PbesExprKind::Negation(inner) => resolve_in_pbes_expr(inner, scope, ids),
        PbesExprKind::Binary { lhs, rhs, .. } => {
            resolve_in_pbes_expr(lhs, scope, ids);
            resolve_in_pbes_expr(rhs, scope, ids);
        }
        PbesExprKind::Quantifier { variables, body, .. } => {
            let pushed = scope.push_declarations(variables, ids);
            resolve_in_pbes_expr(body, scope, ids);
            scope.pop(pushed);
        }
    }
}

#[cfg(test)]
mod tests {
    use merc_syntax::DataExprKind;
    use merc_syntax::PbesExprKind;
    use merc_syntax::UntypedPbes;

    use super::resolve_pbes_variables;

    #[test]
    fn test_prop_var_inst_argument_resolves_to_equation_parameter() {
        let text = "pbes nu X(n: Nat) = val(n == 0) || X(n); init X(0);";
        let mut pbes = UntypedPbes::parse(text).unwrap();
        resolve_pbes_variables(&mut pbes);

        let declared = pbes.equations[0].variable.parameters[0]
            .var_id
            .expect("the parameter was assigned a VarId");
        let PbesExprKind::Binary { rhs, .. } = &pbes.equations[0].formula.node else {
            panic!("expected a Binary (||) formula");
        };
        let PbesExprKind::PropVarInst(inst) = &rhs.node else {
            panic!("expected a PropVarInst");
        };
        assert!(matches!(
            &inst.arguments[0].node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == declared
        ));
    }

    #[test]
    fn test_quantifier_bound_variable_resolves_to_its_own_binder() {
        let text = "pbes mu X = forall n: Nat . val(n == n); init X;";
        let mut pbes = UntypedPbes::parse(text).unwrap();
        resolve_pbes_variables(&mut pbes);

        let PbesExprKind::Quantifier { variables, body, .. } = &pbes.equations[0].formula.node else {
            panic!("expected a Quantifier formula");
        };
        let declared = variables[0].var_id.expect("the binder was assigned a VarId");
        let PbesExprKind::DataValExpr(data_expr) = &body.node else {
            panic!("expected a DataValExpr body");
        };
        let DataExprKind::Binary { lhs, .. } = &data_expr.node else {
            panic!("expected a Binary (==) expression");
        };
        assert!(matches!(
            &lhs.node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == declared
        ));
    }

    #[test]
    fn test_pbes_init_only_sees_globals_not_any_equations_own_parameters() {
        let text = "glob g: Nat; pbes nu X(n: Nat) = val(n == n); init X(g);";
        let mut pbes = UntypedPbes::parse(text).unwrap();
        resolve_pbes_variables(&mut pbes);

        let declared = pbes.global_variables[0]
            .var_id
            .expect("the global was assigned a VarId");
        assert!(matches!(
            &pbes.init.arguments[0].node,
            DataExprKind::Resolved(name, var_id) if name == "g" && **var_id == declared
        ));
    }
}
