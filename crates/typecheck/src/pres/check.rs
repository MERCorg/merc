//! The scoped walk over every PRES equation's formula and `init`: checks each
//! data expression embedded via `val(...)` against `Real`, and each
//! `PropVarInst` against the equation table. Resolves the declared sorts of all
//! bound variables.

use merc_syntax::PresExpr;
use merc_syntax::PresExprKind;
use merc_syntax::PropVarInst;
use merc_syntax::Span;
use merc_syntax::UntypedPres;

use crate::DataSpecification;
use crate::ResolvedName;
use crate::ResolvedSortId;
use crate::TypingInfo;
use crate::checking::Scope;
use crate::checking::check_expression_against;
use crate::checking::collect_binder_sorts;
use crate::declared_span;
use crate::lsp_info;

use super::PresError;
use super::pres_specification::DeclarationTables;
use super::pres_specification::resolve_declared_sort;

/// Checks a PRES specification against the declared sorts, returning the merged
/// typing information.
pub(super) fn check_pres_specification(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    spec: &UntypedPres,
) -> Result<TypingInfo, PresError> {
    let mut typing = TypingInfo::default();
    let mut sort_references = Vec::new();

    for decl in &spec.global_variables {
        lsp_info::collect_sort_name_references(&decl.sort, &mut sort_references);
    }
    for eqn in &spec.equations {
        for param in &eqn.variable.parameters {
            lsp_info::collect_sort_name_references(&param.sort, &mut sort_references);
        }
    }

    let globals: Vec<(Span, ResolvedSortId)> = spec
        .global_variables
        .iter()
        .zip(&tables.global_sorts)
        .map(|(decl, &sort)| (decl.identifier.span.clone(), sort))
        .collect();
    for (decl, &sort) in spec.global_variables.iter().zip(&tables.global_sorts) {
        lsp_info::push_binder_declaration(
            data,
            &mut typing,
            decl.identifier.span.clone(),
            decl.identifier.node.clone(),
            sort,
        );
    }

    for (eqn, params) in spec.equations.iter().zip(&tables.equation_params) {
        let mut scope = globals.clone();
        // An equation's own parameters are in scope throughout its formula.
        scope.extend(
            eqn.variable
                .parameters
                .iter()
                .zip(params)
                .map(|(decl, &(_, sort))| (decl.identifier.span.clone(), sort)),
        );
        for (decl, &(_, sort)) in eqn.variable.parameters.iter().zip(params) {
            lsp_info::push_binder_declaration(
                data,
                &mut typing,
                decl.identifier.span.clone(),
                decl.identifier.node.clone(),
                sort,
            );
        }
        collect_scope(data, &eqn.formula, &mut scope, &mut sort_references, &mut typing)?;
        check_pres_expr(data, tables, &scope, &eqn.formula, &mut typing)?;
    }

    // `init` is a bare `PropVarInst`, checked the same way as one appearing inside a formula —
    // scope = globals only, since it sits outside every equation's own parameter scope.
    check_prop_var_inst(data, tables, &globals, &spec.init, &mut typing)?;

    lsp_info::push_sort_references(data, &sort_references, &mut typing);
    Ok(typing)
}

/// Collects the scope for a PRES expression, resolving the declared sorts of
/// all `Bound` binders
fn collect_scope(
    data: &mut DataSpecification,
    expr: &PresExpr,
    scope: &mut Vec<(Span, ResolvedSortId)>,
    sort_references: &mut Vec<(Span, String)>,
    typing: &mut TypingInfo,
) -> Result<(), PresError> {
    match &expr.node {
        PresExprKind::True | PresExprKind::False | PresExprKind::DataValExpr(_) | PresExprKind::PropVarInst(_) => {
            Ok(())
        }
        PresExprKind::Negation(inner) => collect_scope(data, inner, scope, sort_references, typing),
        PresExprKind::Binary { lhs, rhs, .. } => {
            collect_scope(data, lhs, scope, sort_references, typing)?;
            collect_scope(data, rhs, scope, sort_references, typing)
        }
        PresExprKind::Equal { body, .. } => collect_scope(data, body, scope, sort_references, typing),
        PresExprKind::Condition { lhs, then, else_, .. } => {
            collect_scope(data, lhs, scope, sort_references, typing)?;
            collect_scope(data, then, scope, sort_references, typing)?;
            collect_scope(data, else_, scope, sort_references, typing)
        }
        PresExprKind::RightConstantMultiply { expr, .. } | PresExprKind::LeftConstantMultiply { expr, .. } => {
            collect_scope(data, expr, scope, sort_references, typing)
        }
        PresExprKind::Bound { variables, expr, .. } => {
            collect_binder_sorts(data, scope, sort_references, typing, variables, resolve_declared_sort)?;
            collect_scope(data, expr, scope, sort_references, typing)
        }
    }
}

fn check_pres_expr(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    expr: &PresExpr,
    typing: &mut TypingInfo,
) -> Result<(), PresError> {
    match &expr.node {
        PresExprKind::True | PresExprKind::False => Ok(()),

        PresExprKind::DataValExpr(data_expr) => {
            let real_sort = data.context().sorts.real_sort();
            check_expression_against::<PresError>(data, scope, data_expr, real_sort, typing)
        }

        PresExprKind::PropVarInst(inst) => check_prop_var_inst(data, tables, scope, inst, typing),

        PresExprKind::Negation(inner) => check_pres_expr(data, tables, scope, inner, typing),

        PresExprKind::Binary { lhs, rhs, .. } => {
            check_pres_expr(data, tables, scope, lhs, typing)?;
            check_pres_expr(data, tables, scope, rhs, typing)
        }

        PresExprKind::Equal { body, .. } => check_pres_expr(data, tables, scope, body, typing),

        PresExprKind::Condition { lhs, then, else_, .. } => {
            check_pres_expr(data, tables, scope, lhs, typing)?;
            check_pres_expr(data, tables, scope, then, typing)?;
            check_pres_expr(data, tables, scope, else_, typing)
        }

        PresExprKind::RightConstantMultiply { expr, constant }
        | PresExprKind::LeftConstantMultiply { expr, constant } => {
            let real_sort = data.context().sorts.real_sort();
            check_expression_against::<PresError>(data, scope, constant, real_sort, typing)?;
            check_pres_expr(data, tables, scope, expr, typing)
        }

        PresExprKind::Bound { expr, .. } => check_pres_expr(data, tables, scope, expr, typing),
    }
}

/// Resolves `inst.identifier` against the equation table (`UndeclaredPropositionalVariable` if
/// missing), checks its argument count against the declared parameter count (`ArityMismatch`), and
/// checks each argument against its parameter's sort. On success, also pushes a
/// [`ResolvedName::PropositionalVariable`] at `inst.identifier`'s own span (not `inst.span`, the
/// whole `name(args)` node) — see [`docs/name_resolution.md`](../../../../docs/name_resolution.md):
/// unlike an action/process name, a PRES equation is never overloaded, so the equation table's
/// single match is the answer. Mirrors `crate::pbes::check::check_prop_var_inst`.
fn check_prop_var_inst(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    inst: &PropVarInst,
    typing: &mut TypingInfo,
) -> Result<(), PresError> {
    let Some(&index) = tables.equations_by_name.get(&inst.identifier.node) else {
        return Err(PresError::UndeclaredPropositionalVariable {
            name: inst.identifier.node.clone(),
            span: inst.span.clone(),
        });
    };
    typing.push(
        inst.identifier.span.clone(),
        ResolvedName::PropositionalVariable {
            name: inst.identifier.node.clone(),
            declaration: declared_span(&tables.equation_decl_spans[index]),
        },
    );

    let params = &tables.equation_params[index];
    if inst.arguments.len() != params.len() {
        return Err(PresError::ArityMismatch {
            name: inst.identifier.node.clone(),
            expected: params.len(),
            found: inst.arguments.len(),
            span: inst.span.clone(),
        });
    }

    for (arg, (_, sort)) in inst.arguments.iter().zip(params) {
        check_expression_against::<PresError>(data, scope, arg, *sort, typing)?;
    }
    Ok(())
}
