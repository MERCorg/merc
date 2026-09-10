//! Rewrites every sort-position [`SortExpressionKind::Reference`] naming one of a
//! specification's own `type_var` declarations into a [`SortExpressionKind::TypeVar`].
//!
//! Runs during parsing (see the `type_var` call sites in `consume.rs`), before any
//! type-checking-specific name resolution: which names a `type_var` block binds is purely
//! syntactic, so an [`UntypedDataSpecification`] already tells
//! them apart from ordinary sort references by the time it leaves the parser. Name resolution
//! later resolves [`SortExpressionKind::TypeVar`] into [`SortExpressionKind::ResolvedTypeVar`],
//! mirroring how it resolves a plain [`SortExpressionKind::Reference`]. See `docs/typecheck.md`.

use std::collections::HashSet;

use crate::DataExpr;
use crate::DataExprKind;
use crate::SortExpression;
use crate::SortExpressionKind;
use crate::Traverse;
use crate::UntypedDataSpecification;

/// Resolves every [`SortExpressionKind::Reference`] naming one of `spec`'s own `type_var`
/// declarations into a [`SortExpressionKind::TypeVar`], throughout the specification: sort
/// aliases, constructor, map and equation-variable sorts, and binder sorts inside equation bodies
/// (a quantifier, lambda, or set/bag comprehension). A no-op when the spec declares no type
/// variables.
pub(crate) fn resolve_type_vars(spec: &mut UntypedDataSpecification) {
    if spec.type_var_declarations.is_empty() {
        return;
    }

    let names: HashSet<&str> = spec
        .type_var_declarations
        .iter()
        .map(|decl| decl.identifier.as_str())
        .collect();

    for sort in &mut spec.sort_declarations {
        if let Some(expr) = &mut sort.expr {
            resolve_type_var(expr, &names);
        }
    }

    for constructor in &mut spec.constructor_declarations {
        resolve_type_var(&mut constructor.sort, &names);
    }

    for map in &mut spec.map_declarations {
        resolve_type_var(&mut map.sort, &names);
    }

    for equation in &mut spec.equation_declarations {
        for var in &mut equation.variables {
            resolve_type_var(&mut var.sort, &names);
        }

        for eqn in &mut equation.equations {
            if let Some(condition) = &mut eqn.condition {
                resolve_type_vars_in_expr(condition, &names);
            }
            resolve_type_vars_in_expr(&mut eqn.lhs, &names);
            resolve_type_vars_in_expr(&mut eqn.rhs, &names);
        }
    }
}

/// Rewrites every `Reference` in `sort` naming one of `names` into a `TypeVar`.
fn resolve_type_var(sort: &mut SortExpression, names: &HashSet<&str>) {
    sort.transform(|expr| {
        if let SortExpressionKind::Reference(name) = &expr.node
            && names.contains(name.as_str())
        {
            expr.node = SortExpressionKind::TypeVar(name.clone());
        }
    });
}

/// See [resolve_type_var]; applied to every binder sort (lambda, quantifier and set/bag
/// comprehension variables) inside a data expression.
fn resolve_type_vars_in_expr(expr: &mut DataExpr, names: &HashSet<&str>) {
    expr.transform(|expr| match &mut expr.node {
        DataExprKind::Lambda { variables, body: _ }
        | DataExprKind::Quantifier {
            op: _,
            variables,
            body: _,
        } => {
            for variable in variables {
                resolve_type_var(&mut variable.sort, names);
            }
        }
        DataExprKind::SetBagComp { variable, predicate: _ } => {
            resolve_type_var(&mut variable.sort, names);
        }
        _ => {}
    });
}
