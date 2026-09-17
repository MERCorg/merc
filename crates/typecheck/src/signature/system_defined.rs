use std::collections::BTreeSet;
use std::collections::HashSet;

use merc_syntax::SortExpressionKind;
use merc_syntax::UntypedDataSpecification;

use crate::SortCollectionMode;
use crate::WellTypedError;
use crate::collect_system_sorts_in_spec;
use crate::is_system_generated_name;
use crate::polymorphic_operator_names;

/// Any user `cons`/`map` declaration whose name collides with a system-defined
/// function is rejected.
///
/// Also rejects `@`-prefixed names outright.
pub(crate) fn check_no_system_function_redeclaration(
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
) -> Result<(), WellTypedError> {
    // Gather the constructors and mappings from system, and added the polymorphic operator names.
    let mut reserved: HashSet<&str> = HashSet::new();
    reserved.extend(
        system
            .constructor_declarations
            .iter()
            .map(|decl| decl.identifier.as_str()),
    );
    reserved.extend(system.map_declarations.iter().map(|decl| decl.identifier.as_str()));
    let reserved_polymorphic: HashSet<&'static str> = polymorphic_operator_names().collect();

    for decl in &spec.constructor_declarations {
        if reserved.contains(decl.identifier.as_str())
            || reserved_polymorphic.contains(decl.identifier.as_str())
            || is_system_generated_name(&decl.identifier)
        {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }

    for decl in &spec.map_declarations {
        if reserved.contains(decl.identifier.as_str())
            || reserved_polymorphic.contains(decl.identifier.as_str())
            || is_system_generated_name(&decl.identifier)
        {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }
    Ok(())
}

/// Every distinct function-update arity `spec` needs.
pub(crate) fn function_update_arities(spec: &UntypedDataSpecification) -> BTreeSet<usize> {
    let mut worklist = Vec::new();
    
    collect_system_sorts_in_spec(spec, &mut worklist, SortCollectionMode::ContainersAndFunctions);
    worklist
        .into_iter()
        .filter_map(|sort| match sort.node {
            SortExpressionKind::Function { .. } => Some(1),
            SortExpressionKind::FlattenedFunction { domain, .. } => Some(domain.len()),
            _ => None,
        })
        .collect()
}


/// Collects every container sort — every simple/resolved (basic or
/// user-declared) sort too, in [SortCollectionMode::Every] — and, unless
/// [SortCollectionMode::ContainersOnly], every single-argument function sort,
/// occurring in the specification into `out`, including the sorts on binders
/// inside the equation expressions.
pub(crate) fn collect_system_sorts_in_spec(
    spec: &UntypedDataSpecification,
    out: &mut Vec<SortExpression>,
    mode: SortCollectionMode,
) {
    for declaration in &spec.sort_declarations {
        if let Some(expr) = &declaration.expr {
            collect_system_sorts(expr, out, mode);
        }
    }

    for constructor in &spec.constructor_declarations {
        collect_system_sorts(&constructor.sort, out, mode);
    }

    for map in &spec.map_declarations {
        collect_system_sorts(&map.sort, out, mode);
    }

    for equation in &spec.equation_declarations {
        collect_system_sorts_in_equation(equation, out, mode);
    }
}

/// Collects the system-defined sorts occurring in a single `var ... eqn ...`
/// block: its declared variable sorts and its equations' conditions, left- and
/// right-hand sides (including binder sorts inside those expressions).
fn collect_system_sorts_in_equation(equation: &EqnSpec, out: &mut Vec<SortExpression>, mode: SortCollectionMode) {
    for variable in &equation.variables {
        collect_system_sorts(&variable.sort, out, mode);
    }
    
    for eqn in &equation.equations {
        if let Some(condition) = &eqn.condition {
            collect_system_sorts_in_expr(condition, out, mode);
        }

        collect_system_sorts_in_expr(&eqn.lhs, out, mode);
        collect_system_sorts_in_expr(&eqn.rhs, out, mode);
    }
}

/// Collects the system-defined sorts mentioned syntactically inside a data
/// expression: the sorts on binders, and around a set/bag comprehension's
/// element sort also `Set(S)` and `Bag(S)` — the comprehension denotes one of
/// the two, which reading applies is only decided by sort inference, so the
/// operators of both are provided. The element sorts of enumeration literals
/// (`{1, 2}`) are not syntactically apparent and are not collected.
///
/// Binder sorts that are not valid variable sorts (see
/// [is_supported_binder_sort]) are skipped: inference rejects the constructs
/// that bind them, so their operators are never looked up.
fn collect_system_sorts_in_expr(expr: &DataExpr, out: &mut Vec<SortExpression>, mode: SortCollectionMode) {
    expr.visit::<(), _>(|expr| {
        match &expr.node {
            DataExprKind::SetBagComp { variable, predicate: _ } => {
                if is_supported_binder_sort(&variable.sort) {
                    collect_system_sorts(&variable.sort, out, mode);
                    out.push(SortExpressionKind::Complex(ComplexSort::Set, Box::new(variable.sort.clone())).into());
                    out.push(SortExpressionKind::Complex(ComplexSort::Bag, Box::new(variable.sort.clone())).into());
                }
            }
            DataExprKind::Lambda { variables, body: _ }
            | DataExprKind::Quantifier {
                op: _,
                variables,
                body: _,
            } => {
                for variable in variables {
                    if is_supported_binder_sort(&variable.sort) {
                        collect_system_sorts(&variable.sort, out, mode);
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}
