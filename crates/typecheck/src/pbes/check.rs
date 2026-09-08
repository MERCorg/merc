//! The scoped walk over every PBES equation's formula and `init`: checks each `val(...)`
//! expression against `Bool`, resolves each `PropVarInst` against the equation table (name, arity,
//! and each argument's sort), and pushes/pops quantifier binders — accumulating each checked
//! expression's [`TypingInfo`] along the way.

use merc_syntax::PbesExpr;
use merc_syntax::PbesExprKind;
use merc_syntax::PropVarInst;
use merc_syntax::Span;
use merc_syntax::UntypedPbes;
use merc_syntax::VarId;

use crate::DataSpecification;
use crate::ResolvedName;
use crate::ResolvedSortId;
use crate::TypingInfo;
use crate::VariableSpans;
use crate::checking::Scope;
use crate::checking::check_expression_against;
use crate::checking::collect_binder_sorts;
use crate::declared_span;
use crate::typing_info;

use super::PbesError;
use super::pbes_specification::DeclarationTables;
use super::pbes_specification::resolve_declared_sort;

/// Checks a PBES specification against its declaration tables, returning typing
/// information for all expressions.
pub(super) fn check_pbes_specification(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    variable_spans: &VariableSpans,
    spec: &UntypedPbes,
) -> Result<TypingInfo, PbesError> {
    let mut typing = TypingInfo::default();
    let mut sort_references = Vec::new();

    for decl in &spec.global_variables {
        typing_info::collect_sort_name_references(&decl.sort, &mut sort_references);
    }
    for eqn in &spec.equations {
        for param in &eqn.variable.parameters {
            typing_info::collect_sort_name_references(&param.sort, &mut sort_references);
        }
    }

    let globals: Vec<(VarId, ResolvedSortId)> = spec
        .global_variables
        .iter()
        .zip(&tables.global_sorts)
        .map(|(decl, &sort)| (decl.var_id.expect("resolve_pbes_variables ran before checking"), sort))
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
        scope.extend(eqn.variable.parameters.iter().zip(params).map(|(decl, &(_, sort))| {
            (decl.var_id.expect("resolve_pbes_variables ran before checking"), sort)
        }));
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
        check_pbes_expr(data, tables, &scope, variable_spans, &eqn.formula, &mut typing)?;
    }

    // `init` is a bare `PropVarInst`, checked the same way as one appearing inside a formula —
    // scope = globals only, since it sits outside every equation's own parameter scope.
    check_prop_var_inst(data, tables, &globals, variable_spans, &spec.init, &mut typing)?;

    typing_info::push_sort_references(data, &sort_references, &mut typing);
    Ok(typing)
}

/// Resolves the declared sort of every `Quantifier` binder in `expr`.
fn collect_scope(
    data: &mut DataSpecification,
    expr: &PbesExpr,
    scope: &mut Vec<(VarId, ResolvedSortId)>,
    sort_references: &mut Vec<(Span, String)>,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    match &expr.node {
        PbesExprKind::True | PbesExprKind::False | PbesExprKind::DataValExpr(_) | PbesExprKind::PropVarInst(_) => {
            Ok(())
        }
        PbesExprKind::Negation(inner) => collect_scope(data, inner, scope, sort_references, typing),
        PbesExprKind::Binary { lhs, rhs, .. } => {
            collect_scope(data, lhs, scope, sort_references, typing)?;
            collect_scope(data, rhs, scope, sort_references, typing)
        }
        PbesExprKind::Quantifier { variables, body, .. } => {
            collect_binder_sorts(data, scope, sort_references, typing, variables, resolve_declared_sort)?;
            collect_scope(data, body, scope, sort_references, typing)
        }
    }
}

fn check_pbes_expr(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    variable_spans: &VariableSpans,
    expr: &PbesExpr,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    match &expr.node {
        PbesExprKind::True | PbesExprKind::False => Ok(()),

        PbesExprKind::DataValExpr(data_expr) => {
            let bool_sort = data.context().sorts.bool_sort();
            check_expression_against::<PbesError>(data, scope, variable_spans, data_expr, bool_sort, typing)
        }

        PbesExprKind::PropVarInst(inst) => check_prop_var_inst(data, tables, scope, variable_spans, inst, typing),

        PbesExprKind::Negation(inner) => check_pbes_expr(data, tables, scope, variable_spans, inner, typing),

        PbesExprKind::Binary { lhs, rhs, .. } => {
            check_pbes_expr(data, tables, scope, variable_spans, lhs, typing)?;
            check_pbes_expr(data, tables, scope, variable_spans, rhs, typing)
        }

        PbesExprKind::Quantifier { body, .. } => check_pbes_expr(data, tables, scope, variable_spans, body, typing),
    }
}

/// Resolves `inst.identifier` against the equation table.
fn check_prop_var_inst(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    variable_spans: &VariableSpans,
    inst: &PropVarInst,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    let Some(&index) = tables.equations_by_name.get(&inst.identifier.node) else {
        return Err(PbesError::UndeclaredPropositionalVariable {
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
        return Err(PbesError::ArityMismatch {
            name: inst.identifier.node.clone(),
            expected: params.len(),
            found: inst.arguments.len(),
            span: inst.span.clone(),
        });
    }

    for (arg, (_, sort)) in inst.arguments.iter().zip(params) {
        check_expression_against::<PbesError>(data, scope, variable_spans, arg, *sort, typing)?;
    }
    Ok(())
}
