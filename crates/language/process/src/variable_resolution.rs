//! Variable resolution for process specifications: ties every free variable occurrence in a
//! `proc` body or `init` to the declaration (a global, a process parameter, or a `sum`/`dist`
//! binder) it refers to.

use merc_syntax::ProcessExpr;
use merc_syntax::ProcessExprKind;
use merc_syntax::UntypedProcessSpecification;
use merc_syntax::VarIdAllocator;
use merc_typecheck::internal::VariableScope as Scope;
use merc_typecheck::internal::resolve_in_data_expr;

/// Resolves every free variable reference in `spec`'s `proc` bodies and `init`.
pub(crate) fn resolve_process_variables(spec: &mut UntypedProcessSpecification) {
    let mut ids = VarIdAllocator::default();
    let globals = Scope::from_declarations(&mut spec.global_variables, &mut ids);

    for proc_decl in &mut spec.process_declarations {
        // A process's own parameters shadow a global variable of the same name.
        let mut scope = globals.clone();
        scope.push_declarations(&mut proc_decl.params, &mut ids);
        resolve_in_process_expr(&mut proc_decl.body, &mut scope, &mut ids);
    }

    if let Some(init) = &mut spec.init {
        // `init` sits outside every process's own parameter scope — only globals apply.
        let mut scope = globals.clone();
        resolve_in_process_expr(init, &mut scope, &mut ids);
    }
}

fn resolve_in_process_expr(expr: &mut ProcessExpr, scope: &mut Scope, ids: &mut VarIdAllocator) {
    match &mut expr.node {
        ProcessExprKind::Delta | ProcessExprKind::Tau => {}
        ProcessExprKind::Action(_, args) => {
            for arg in args {
                resolve_in_data_expr(arg, scope, ids);
            }
        }
        ProcessExprKind::Id(_, assignments) => {
            // Only the assignment's *value* is a free variable read.
            for assignment in assignments {
                resolve_in_data_expr(&mut assignment.expr, scope, ids);
            }
        }
        ProcessExprKind::Sum { variables, operand } => {
            let pushed = scope.push_declarations(variables, ids);
            resolve_in_process_expr(operand, scope, ids);
            scope.pop(pushed);
        }
        ProcessExprKind::Dist {
            variables,
            expr: weight,
            operand,
        } => {
            let pushed = scope.push_declarations(variables, ids);
            // `dist`'s weight is resolved with its own bound variables already in scope.
            resolve_in_data_expr(weight, scope, ids);
            resolve_in_process_expr(operand, scope, ids);
            scope.pop(pushed);
        }
        ProcessExprKind::Binary { lhs, rhs, .. } => {
            resolve_in_process_expr(lhs, scope, ids);
            resolve_in_process_expr(rhs, scope, ids);
        }
        ProcessExprKind::Hide { operand, .. }
        | ProcessExprKind::Rename { operand, .. }
        | ProcessExprKind::Allow { operand, .. }
        | ProcessExprKind::Block { operand, .. }
        | ProcessExprKind::Comm { operand, .. } => resolve_in_process_expr(operand, scope, ids),
        ProcessExprKind::Condition { condition, then, else_ } => {
            resolve_in_data_expr(condition, scope, ids);
            resolve_in_process_expr(then, scope, ids);
            if let Some(else_) = else_ {
                resolve_in_process_expr(else_, scope, ids);
            }
        }
        ProcessExprKind::At { expr, operand } => {
            resolve_in_process_expr(expr, scope, ids);
            resolve_in_data_expr(operand, scope, ids);
        }
    }
}

#[cfg(test)]
mod tests {
    use merc_syntax::DataExprKind;
    use merc_syntax::ProcessExprKind;
    use merc_syntax::UntypedProcessSpecification;

    use super::resolve_process_variables;

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_action_argument_resolves_to_process_parameter() {
        let text = "act a: Nat; proc P(n: Nat) = a(n); init P(1);";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let declared = spec.process_declarations[0].params[0]
            .var_id
            .expect("the parameter was assigned a VarId");
        let ProcessExprKind::Action(_, args) = &spec.process_declarations[0].body.node else {
            panic!("expected an Action body");
        };
        assert!(matches!(
            &args[0].node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_sum_bound_variable_resolves_to_its_own_binder() {
        let text = "act a: Nat; proc P = sum x: Nat . a(x); init P;";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let ProcessExprKind::Sum { variables, operand } = &spec.process_declarations[0].body.node else {
            panic!("expected a Sum body");
        };
        let declared = variables[0].var_id.expect("the binder was assigned a VarId");
        let ProcessExprKind::Action(_, args) = &operand.node else {
            panic!("expected an Action operand");
        };
        assert!(matches!(
            &args[0].node,
            DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_dist_weight_sees_its_own_bound_variable() {
        let text = "act a; proc P = dist x: Pos[1/x] . a; init P;";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let ProcessExprKind::Dist {
            variables,
            expr: weight,
            ..
        } = &spec.process_declarations[0].body.node
        else {
            panic!("expected a Dist body");
        };
        let declared = variables[0].var_id.expect("the binder was assigned a VarId");
        let DataExprKind::Binary { rhs, .. } = &weight.node else {
            panic!("expected a Binary (division) weight, got {:?}", weight.node);
        };
        assert!(matches!(
            &rhs.node,
            DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_assignment_value_resolves_but_key_does_not() {
        let text = "proc P(n: Nat) = P(n = n); init P(0);";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let declared = spec.process_declarations[0].params[0]
            .var_id
            .expect("the parameter was assigned a VarId");
        let ProcessExprKind::Id(_, assignments) = &spec.process_declarations[0].body.node else {
            panic!("expected an Id (instantiation) body");
        };
        // The key `n` is a plain `String` field (`AssignmentData::identifier`), never touched by
        // this pass; only the value `n` (the expression) is a `DataExpr` and gets resolved.
        assert_eq!(assignments[0].identifier, "n");
        assert!(matches!(
            &assignments[0].expr.node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_nested_data_binder_inside_process_action_argument_resolves() {
        let text = "act a: Bool; proc P(n: Nat) = a(exists x: Nat . x == n); init P(0);";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let n_declared = spec.process_declarations[0].params[0]
            .var_id
            .expect("the parameter was assigned a VarId");
        let ProcessExprKind::Action(_, args) = &spec.process_declarations[0].body.node else {
            panic!("expected an Action body");
        };
        let DataExprKind::Quantifier { variables, body, .. } = &args[0].node else {
            panic!("expected a Quantifier argument");
        };
        let x_declared = variables[0].var_id.expect("the binder was assigned a VarId");
        let DataExprKind::Binary { lhs, rhs, .. } = &body.node else {
            panic!("expected a Binary (==) body");
        };
        assert!(matches!(
            &lhs.node,
            DataExprKind::Resolved(name, var_id) if name == "x" && **var_id == x_declared
        ));
        assert!(matches!(
            &rhs.node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == n_declared
        ));
        // The process parameter and the nested quantifier binder are distinct binders and must
        // never share an id, even though each is the "first" binder of its own construct.
        assert_ne!(n_declared, x_declared);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_undeclared_name_is_left_unresolved() {
        let text = "act a: Nat; proc P = a(m); init P;";
        let mut spec = UntypedProcessSpecification::parse(text).unwrap();
        resolve_process_variables(&mut spec);

        let ProcessExprKind::Action(_, args) = &spec.process_declarations[0].body.node else {
            panic!("expected an Action body");
        };
        // `m` isn't declared anywhere; this pass leaves it as a plain `Id` for the existing
        // `UndeclaredName` inference error to reject later.
        assert!(matches!(&args[0].node, DataExprKind::Id(name) if name == "m"));
    }
}
