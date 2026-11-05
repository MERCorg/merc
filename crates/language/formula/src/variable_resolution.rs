//! Variable resolution for state formulas: ties every free variable occurrence to the data
//! binder it refers to, and every fixpoint variable occurrence to its `mu`/`nu`.

use std::convert::Infallible;
use std::ops::ControlFlow;

use merc_syntax::ActFrm;
use merc_syntax::ActFrmKind;
use merc_syntax::RegFrm;
use merc_syntax::RegFrmKind;
use merc_syntax::StateFrm;
use merc_syntax::StateFrmKind;
use merc_syntax::StateVarId;
use merc_syntax::StateVarIdAllocator;
use merc_syntax::Traverse;
use merc_syntax::VarIdAllocator;
use merc_typecheck::internal::NameStack;
use merc_typecheck::internal::VariableScope as Scope;
use merc_typecheck::internal::resolve_in_data_expr;
use merc_utilities::Step;

/// Resolves every free variable reference in `formula`, and returns the [`StateVarIdAllocator`]
/// used.
///
/// This pass only decides *which* enclosing binder a name refers to; a fixpoint variable's own
/// *parameter sorts* still aren't known here.
pub(crate) fn resolve_modal_variables(formula: &mut StateFrm) -> StateVarIdAllocator {
    let mut ids = VarIdAllocator::default();
    let mut state_var_ids = StateVarIdAllocator::default();
    resolve_in_state_frm(formula, &mut ids, &mut state_var_ids);
    state_var_ids
}

/// The fixpoint-variable names currently in scope, in the second, [`StateVarId`]-keyed namespace.
type FixpointScope = NameStack<StateVarId>;

/// What a matching `enter` call pushed onto `StateFrmScope::scope`/`state_vars` for one node,
/// popped by `exit` once that node's whole subtree is resolved.
enum Undo {
    /// This node pushed nothing.
    None,
    /// A `Quantifier`/`Bound`'s own bound variables: pop this many entries off `scope`.
    Scope(usize),
    /// A `FixedPoint`'s own parameters and its own variable name: pop this many entries off
    /// `scope`, then one off `state_vars` (in that order, mirroring the push order below).
    ScopeAndStateVar(usize),
}

/// The scoped mutable state threaded through [`resolve_in_state_frm`]'s [`Traverse::apply_scoped`]
/// walk.
#[derive(Default)]
struct StateFrmScope {
    scope: Scope,
    state_vars: FixpointScope,
    undo: Vec<Undo>,
}

/// Resolves every free variable reference in `formula`, rewriting each `Id` naming an enclosing
/// `mu`/`nu` into `Resolved` in place.
fn resolve_in_state_frm(formula: &mut StateFrm, ids: &mut VarIdAllocator, state_var_ids: &mut StateVarIdAllocator) {
    let mut state = StateFrmScope::default();
    let _: Result<Option<Infallible>, Infallible> = formula.apply_scoped(
        (),
        &mut state,
        |formula, (), state| {
            match &mut formula.node {
                StateFrmKind::True | StateFrmKind::False => state.undo.push(Undo::None),

                StateFrmKind::Delay(time) | StateFrmKind::Yaled(time) => {
                    if let Some(time) = time {
                        resolve_in_data_expr(time, &mut state.scope, ids);
                    }
                    state.undo.push(Undo::None);
                }

                StateFrmKind::Id(name, arguments) => {
                    for argument in arguments.iter_mut() {
                        resolve_in_data_expr(argument, &mut state.scope, ids);
                    }
                    if let Some(declaration) = state.state_vars.resolve(name) {
                        formula.node = StateFrmKind::Resolved(name.clone(), std::mem::take(arguments), declaration);
                    }
                    state.undo.push(Undo::None);
                }

                // Already resolved: this pass never runs twice on the same tree.
                StateFrmKind::Resolved(_, arguments, _) => {
                    for argument in arguments.iter_mut() {
                        resolve_in_data_expr(argument, &mut state.scope, ids);
                    }
                    state.undo.push(Undo::None);
                }

                StateFrmKind::DataValExpr(data_expr) => {
                    resolve_in_data_expr(data_expr, &mut state.scope, ids);
                    state.undo.push(Undo::None);
                }

                StateFrmKind::DataValExprLeftMult(constant, _) | StateFrmKind::DataValExprRightMult(_, constant) => {
                    resolve_in_data_expr(constant, &mut state.scope, ids);
                    state.undo.push(Undo::None);
                }

                StateFrmKind::Modality { formula: reg, .. } => {
                    resolve_in_reg_frm(reg, &mut state.scope, ids);
                    state.undo.push(Undo::None);
                }

                StateFrmKind::Unary { .. } | StateFrmKind::Binary { .. } => state.undo.push(Undo::None),

                StateFrmKind::Quantifier { variables, .. } | StateFrmKind::Bound { variables, .. } => {
                    let pushed = state.scope.push_declarations(variables, ids);
                    state.undo.push(Undo::Scope(pushed));
                }

                StateFrmKind::FixedPoint { variable, .. } => {
                    // Each parameter's own initial value is a free read of the *outer* scope.
                    for argument in &mut variable.arguments {
                        resolve_in_data_expr(&mut argument.expr, &mut state.scope, ids);
                    }
                    let declarations = variable.arguments.iter_mut().map(|argument| &mut argument.decl);
                    let pushed = state.scope.push_declarations(declarations, ids);
                    // The fixpoint variable's own name is in scope for its body only (it may
                    // itself shadow an outer variable of the same name, `mu X. nu X. ...`).
                    let state_var_id = state_var_ids.alloc();
                    variable.id = Some(state_var_id);
                    state.state_vars.push(variable.identifier.node.clone(), state_var_id);
                    state.undo.push(Undo::ScopeAndStateVar(pushed));
                }
            }
            Ok(ControlFlow::Continue(Step::Into(())))
        },
        |(), state| match state.undo.pop().expect("enter pushes exactly one Undo per exit") {
            Undo::None => {}
            Undo::Scope(pushed) => state.scope.pop(pushed),
            Undo::ScopeAndStateVar(pushed) => {
                state.state_vars.pop(1);
                state.scope.pop(pushed);
            }
        },
    );
}

fn resolve_in_reg_frm(formula: &mut RegFrm, scope: &mut Scope, ids: &mut VarIdAllocator) {
    match &mut formula.node {
        RegFrmKind::Action(action) => resolve_in_act_frm(action, scope, ids),
        RegFrmKind::Iteration(inner) | RegFrmKind::Plus(inner) => resolve_in_reg_frm(inner, scope, ids),
        RegFrmKind::Sequence { lhs, rhs } | RegFrmKind::Choice { lhs, rhs } => {
            resolve_in_reg_frm(lhs, scope, ids);
            resolve_in_reg_frm(rhs, scope, ids);
        }
    }
}

/// The scoped state for [`resolve_in_act_frm`]'s own [`Traverse::apply_scoped`] walk: `scope` is
/// borrowed from the enclosing [`resolve_in_state_frm`] walk (an `ActFrm`'s own `Quantifier` binder
/// shares the same namespace an enclosing `StateFrm` binder does), while `undo` is local to this
/// walk, the only one of the two `exit` needs to unwind.
struct ActFrmScope<'a> {
    scope: &'a mut Scope,
    /// One entry per node entered, matching [`Undo`]'s two relevant cases here: `None` (pushed
    /// nothing) or `Some(pushed)` (a `Quantifier`'s own bound variables to pop).
    undo: Vec<Option<usize>>,
}

/// Resolves every free variable reference in `formula`, sharing `scope`/`ids` with whichever
/// `resolve_in_state_frm`/`resolve_in_reg_frm` call reached this `ActFrm` through a `Modality`.
///
/// A single [`Traverse::apply_scoped`] walk, like [`resolve_in_state_frm`] above; only `Quantifier`
/// pushes anything, so `ActFrmScope::undo` only ever needs `Option<usize>`, not the fuller [`Undo`]
/// enum.
fn resolve_in_act_frm(formula: &mut ActFrm, scope: &mut Scope, ids: &mut VarIdAllocator) {
    let mut state = ActFrmScope {
        scope,
        undo: Vec::new(),
    };
    let _: Result<Option<Infallible>, Infallible> = formula.apply_scoped(
        (),
        &mut state,
        |formula, (), state| {
            match &mut formula.node {
                ActFrmKind::True | ActFrmKind::False | ActFrmKind::Negation(_) | ActFrmKind::Binary { .. } => {
                    state.undo.push(None);
                }

                ActFrmKind::MultAct(multi_action) => {
                    for action in &mut multi_action.actions {
                        for argument in &mut action.args {
                            resolve_in_data_expr(argument, state.scope, ids);
                        }
                    }
                    state.undo.push(None);
                }

                ActFrmKind::DataExprVal(data_expr) => {
                    resolve_in_data_expr(data_expr, state.scope, ids);
                    state.undo.push(None);
                }

                ActFrmKind::Quantifier { variables, .. } => {
                    let pushed = state.scope.push_declarations(variables, ids);
                    state.undo.push(Some(pushed));
                }

                ActFrmKind::At { operand, .. } => {
                    resolve_in_data_expr(operand, state.scope, ids);
                    state.undo.push(None);
                }
            }
            Ok(ControlFlow::Continue(Step::Into(())))
        },
        |(), state| {
            if let Some(pushed) = state.undo.pop().expect("enter pushes exactly one Undo per exit") {
                state.scope.pop(pushed);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use merc_syntax::ActFrmKind;
    use merc_syntax::DataExprKind;
    use merc_syntax::RegFrmKind;
    use merc_syntax::StateFrmKind;
    use merc_syntax::UntypedStateFrmSpec;

    use super::resolve_modal_variables;

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_fixed_point_variable_resolves_to_its_own_binder() {
        let text = "mu X(n: Nat = 0) . val(n) || X(n)";
        let mut spec = UntypedStateFrmSpec::parse(text).unwrap();
        resolve_modal_variables(&mut spec.formula);

        let StateFrmKind::FixedPoint { variable, body, .. } = &spec.formula.node else {
            panic!("expected a FixedPoint formula");
        };
        let declared = variable.id.expect("the fixpoint variable was assigned a StateVarId");
        let StateFrmKind::Binary { rhs, .. } = &body.node else {
            panic!("expected a Binary (||) body");
        };
        assert!(matches!(
            &rhs.node,
            StateFrmKind::Resolved(name, _, id) if name.node == "X" && *id == declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_nested_fixed_point_variables_of_the_same_name_do_not_share_an_id() {
        // The inner, parameter-less `X` refers to the *inner* `nu X`, shadowing the outer `mu X`
        // of the same name — the two binders must never share a StateVarId.
        let text = "mu X(n: Nat = 0) . [true](nu X. X)";
        let mut spec = UntypedStateFrmSpec::parse(text).unwrap();
        resolve_modal_variables(&mut spec.formula);

        let StateFrmKind::FixedPoint {
            variable: outer, body, ..
        } = &spec.formula.node
        else {
            panic!("expected an outer FixedPoint formula");
        };
        let outer_declared = outer.id.expect("the outer fixpoint variable was assigned a StateVarId");
        let StateFrmKind::Modality { expr, .. } = &body.node else {
            panic!("expected a Modality body");
        };
        let StateFrmKind::FixedPoint {
            variable: inner,
            body: inner_body,
            ..
        } = &expr.node
        else {
            panic!("expected a nested FixedPoint formula");
        };
        let inner_declared = inner.id.expect("the inner fixpoint variable was assigned a StateVarId");
        assert_ne!(outer_declared, inner_declared);
        assert!(matches!(
            &inner_body.node,
            StateFrmKind::Resolved(name, _, id) if name.node == "X" && *id == inner_declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_action_formula_quantifier_resolves_to_its_own_binder_and_can_see_an_outer_state_formula_binder() {
        // Exercises `resolve_in_act_frm`'s own scoping specifically: `m` is bound by the action
        // formula's own `exists`, `n` by the *enclosing* state formula's `forall`, reached by
        // crossing through the modality's `RegFrm` -- both must resolve, to two different ids.
        let text = "act a: Nat # Nat; form forall n: Nat . [exists m: Nat . a(m, n)] true;";
        let mut spec = UntypedStateFrmSpec::parse(text).unwrap();
        resolve_modal_variables(&mut spec.formula);

        let StateFrmKind::Quantifier { variables, body, .. } = &spec.formula.node else {
            panic!("expected a Quantifier formula");
        };
        let n_declared = variables[0].var_id.expect("the outer binder was assigned a VarId");

        let StateFrmKind::Modality { formula: reg, .. } = &body.node else {
            panic!("expected a Modality body");
        };
        let RegFrmKind::Action(act_frm) = &reg.node else {
            panic!("expected an Action regular formula");
        };
        let ActFrmKind::Quantifier {
            variables: act_vars,
            body: act_body,
            ..
        } = &act_frm.node
        else {
            panic!("expected a Quantifier action formula");
        };
        let m_declared = act_vars[0]
            .var_id
            .expect("the action formula binder was assigned a VarId");
        assert_ne!(n_declared, m_declared);

        let ActFrmKind::MultAct(multi_action) = &act_body.node else {
            panic!("expected a MultAct body");
        };
        assert!(matches!(
            &multi_action.actions[0].args[0].node,
            DataExprKind::Resolved(name, var_id) if name == "m" && **var_id == m_declared
        ));
        assert!(matches!(
            &multi_action.actions[0].args[1].node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == n_declared
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_action_formula_quantifier_shadows_an_outer_state_formula_binder_of_the_same_name() {
        // `n` is bound twice: the outer `forall` (`Bool`) and the action formula's own `exists`
        // (`Nat`). Inside the action formula, `n` must resolve to the *inner* binder -- the two
        // must never share a VarId.
        let text = "act a: Nat; form forall n: Bool . [exists n: Nat . a(n)] true;";
        let mut spec = UntypedStateFrmSpec::parse(text).unwrap();
        resolve_modal_variables(&mut spec.formula);

        let StateFrmKind::Quantifier { variables, body, .. } = &spec.formula.node else {
            panic!("expected a Quantifier formula");
        };
        let outer_declared = variables[0].var_id.expect("the outer binder was assigned a VarId");

        let StateFrmKind::Modality { formula: reg, .. } = &body.node else {
            panic!("expected a Modality body");
        };
        let RegFrmKind::Action(act_frm) = &reg.node else {
            panic!("expected an Action regular formula");
        };
        let ActFrmKind::Quantifier {
            variables: act_vars,
            body: act_body,
            ..
        } = &act_frm.node
        else {
            panic!("expected a Quantifier action formula");
        };
        let inner_declared = act_vars[0].var_id.expect("the inner binder was assigned a VarId");
        assert_ne!(outer_declared, inner_declared);

        let ActFrmKind::MultAct(multi_action) = &act_body.node else {
            panic!("expected a MultAct body");
        };
        assert!(matches!(
            &multi_action.actions[0].args[0].node,
            DataExprKind::Resolved(name, var_id) if name == "n" && **var_id == inner_declared
        ));
    }
}
