//! The scoped walk over every PBES equation's formula and `init`: checks each `val(...)`
//! expression against `Bool`, resolves each `PropVarInst` against the equation table (name, arity,
//! and each argument's sort), and pushes/pops quantifier binders — accumulating each checked
//! expression's [`TypingInfo`] along the way.

use std::convert::Infallible;
use std::ops::ControlFlow;

use merc_syntax::PbesExpr;
use merc_syntax::PbesExprKind;
use merc_syntax::PropVarInst;
use merc_syntax::Traverse;
use merc_syntax::UntypedPbes;
use merc_syntax::VarId;

use merc_typecheck::DataSpecification;
use merc_typecheck::ResolvedName;
use merc_typecheck::TypingInfo;
use merc_typecheck::internal::ResolvedSortId;
use merc_typecheck::internal::SortReference;
use merc_typecheck::internal::bool_sort;
use merc_typecheck::internal::checking::Scope;
use merc_typecheck::internal::checking::check_expression_against;
use merc_typecheck::internal::checking::collect_binder_sorts;
use merc_typecheck::internal::checking::declare_variables;
use merc_typecheck::internal::collect_sort_name_references;
use merc_typecheck::internal::declared_span;
use merc_typecheck::internal::push_resolved_name;
use merc_typecheck::internal::push_sort_references;

use crate::PbesError;
use crate::pbes_specification::DeclarationTables;
use crate::pbes_specification::resolve_declared_sort;

/// Checks a PBES specification against its declaration tables, returning typing
/// information for all expressions.
pub(crate) fn check_pbes_specification(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    spec: &UntypedPbes,
) -> Result<TypingInfo, PbesError> {
    let mut typing = TypingInfo::default();
    let mut sort_references = Vec::new();

    for decl in &spec.global_variables {
        collect_sort_name_references(&decl.sort, &mut sort_references);
    }

    for eqn in &spec.equations {
        for param in &eqn.variable.parameters {
            collect_sort_name_references(&param.sort, &mut sort_references);
        }
    }

    let mut globals = Vec::new();
    declare_variables(
        data,
        &mut globals,
        &mut typing,
        &spec.global_variables,
        tables.global_sorts.iter().copied(),
    );

    for (eqn, params) in spec.equations.iter().zip(&tables.equation_params) {
        // An equation's own parameters are in scope throughout its formula.
        let mut scope = globals.clone();
        let sorts = params.iter().map(|&(_, sort)| sort);
        declare_variables(data, &mut scope, &mut typing, &eqn.variable.parameters, sorts);
        collect_scope(data, &eqn.formula, &mut scope, &mut sort_references, &mut typing)?;
        check_pbes_expr(data, tables, &scope, &eqn.formula, &mut typing)?;
    }

    // `init` is a bare `PropVarInst`, checked the same way as one appearing inside a formula —
    // scope = globals only, since it sits outside every equation's own parameter scope.
    check_prop_var_inst(data, tables, &globals, &spec.init, &mut typing)?;

    push_sort_references(data, &sort_references, &mut typing);
    Ok(typing)
}

/// Resolves the declared sort of every `Quantifier` binder in `expr`, extending `scope`.
fn collect_scope(
    data: &mut DataSpecification,
    expr: &PbesExpr,
    scope: &mut Vec<(VarId, ResolvedSortId)>,
    sort_references: &mut Vec<SortReference>,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    expr.try_visit(|expr| {
        if let PbesExprKind::Quantifier { variables, .. } = &expr.node {
            collect_binder_sorts(data, scope, sort_references, typing, variables, resolve_declared_sort)?;
        }
        Ok(ControlFlow::Continue(()))
    })
    .map(|_: Option<Infallible>| ())
}

/// Type-checks a PBES expression: every `val(...)` against `Bool` and every `PropVarInst` against
/// the equation table.
fn check_pbes_expr(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    expr: &PbesExpr,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    expr.try_visit(|expr| {
        match &expr.node {
            PbesExprKind::True
            | PbesExprKind::False
            | PbesExprKind::Negation(_)
            | PbesExprKind::Binary { .. }
            | PbesExprKind::Quantifier { .. } => {}

            PbesExprKind::DataValExpr(data_expr) => {
                let bool_sort = bool_sort(data);
                check_expression_against::<PbesError>(data, scope, data_expr, bool_sort, typing)?;
            }

            PbesExprKind::PropVarInst(inst) => check_prop_var_inst(data, tables, scope, inst, typing)?,
        }
        Ok(ControlFlow::Continue(()))
    })
    .map(|_: Option<Infallible>| ())
}

/// Resolves `inst.identifier` against the equation table (`UndeclaredPropositionalVariable` if
/// missing), checks its argument count against the declared parameter count (`ArityMismatch`), and
/// checks each argument against its parameter's sort. On success, also pushes a
/// [`ResolvedName::PropositionalVariable`] at `inst.identifier`'s own span: a PBES equation is
/// never overloaded, so the equation table's single match is the answer.
fn check_prop_var_inst(
    data: &mut DataSpecification,
    tables: &DeclarationTables,
    scope: &Scope,
    inst: &PropVarInst,
    typing: &mut TypingInfo,
) -> Result<(), PbesError> {
    let Some(&index) = tables.equations_by_name.get(&inst.identifier.node) else {
        return Err(PbesError::UndeclaredPropositionalVariable {
            name: inst.identifier.node.clone(),
            span: inst.span.clone(),
            candidates: tables.equations_by_name.keys().cloned().collect(),
        });
    };
    push_resolved_name(
        typing,
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
        check_expression_against::<PbesError>(data, scope, arg, *sort, typing)?;
    }
    Ok(())
}
