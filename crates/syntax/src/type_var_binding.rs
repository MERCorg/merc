//! Rewrites every sort-position [`SortExpressionKind::Reference`] naming one of a
//! specification's own `type_var` declarations into a [`SortExpressionKind::TypeVar`].
//!
//! This runs as part of parsing (see the `type_var`-handling call sites in `consume.rs`), before
//! any type-checking-specific name resolution: a `type_var` declaration is purely syntactic
//! information (which names are bound, where), so by the time an
//! [`UntypedDataSpecification`][crate::UntypedDataSpecification] leaves the parser, a `type_var`
//! block's names are already told apart from ordinary sort references. Name resolution later
//! rewrites [`SortExpressionKind::TypeVar`] into [`SortExpressionKind::ResolvedTypeVar`], the same
//! way it rewrites [`SortExpressionKind::Reference`] into [`SortExpressionKind::Resolved`]. See
//! `docs/polymorphism.md`.

use std::collections::HashSet;

use crate::DataExpr;
use crate::DataExprKind;
use crate::SortExpression;
use crate::SortExpressionKind;
use crate::Traverse;
use crate::UntypedDataSpecification;

/// Rewrites every [`SortExpressionKind::Reference`] naming one of `spec`'s own `type_var`
/// declarations into a [`SortExpressionKind::TypeVar`], throughout the specification: sort
/// aliases, constructor, map and equation-variable sorts, and binder sorts inside equation bodies
/// (a quantifier, lambda, or set/bag comprehension). A no-op when the spec declares no type
/// variables.
pub(crate) fn bind_type_vars(spec: &mut UntypedDataSpecification) {
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
            bind_type_var(expr, &names);
        }
    }

    for constructor in &mut spec.constructor_declarations {
        bind_type_var(&mut constructor.sort, &names);
    }

    for map in &mut spec.map_declarations {
        bind_type_var(&mut map.sort, &names);
    }

    for equation in &mut spec.equation_declarations {
        for var in &mut equation.variables {
            bind_type_var(&mut var.sort, &names);
        }

        for eqn in &mut equation.equations {
            if let Some(condition) = &mut eqn.condition {
                bind_type_vars_in_expr(condition, &names);
            }
            bind_type_vars_in_expr(&mut eqn.lhs, &names);
            bind_type_vars_in_expr(&mut eqn.rhs, &names);
        }
    }
}

/// Rewrites every `Reference` in `sort` naming one of `names` into a `TypeVar`.
fn bind_type_var(sort: &mut SortExpression, names: &HashSet<&str>) {
    sort.transform(|expr| {
        if let SortExpressionKind::Reference(name) = &expr.node
            && names.contains(name.as_str())
        {
            expr.node = SortExpressionKind::TypeVar(name.clone());
        }
    });
}

/// See [bind_type_var]; applied to every binder sort (lambda, quantifier and set/bag
/// comprehension variables) inside a data expression.
fn bind_type_vars_in_expr(expr: &mut DataExpr, names: &HashSet<&str>) {
    expr.transform(|expr| match &mut expr.node {
        DataExprKind::Lambda { variables, body: _ }
        | DataExprKind::Quantifier {
            op: _,
            variables,
            body: _,
        } => {
            for variable in variables {
                bind_type_var(&mut variable.sort, names);
            }
        }
        DataExprKind::SetBagComp { variable, predicate: _ } => {
            bind_type_var(&mut variable.sort, names);
        }
        _ => {}
    });
}
