//! Shared building blocks for a `TypingInfo`-accumulating walk over an
//! expression tree that lives *outside* the data specification proper — a
//! process body (`crate::process::check`), a PBES equation, or a PRES equation.

use merc_syntax::DataExpr;
use merc_syntax::IdDecl;
use merc_syntax::SortExpression;
use merc_syntax::Span;

use crate::DataSpecification;
use crate::InferenceError;
use crate::ResolvedSortId;
use crate::TypingInfo;
use crate::WellTypedError;
use crate::infer_expression_in_scope;
use crate::lower_data_expr;
use crate::lsp_info;

/// Every declaration reachable from the `proc` body/PBES equation currently being checked —
/// global variables, that declaration's own parameters, and every `sum`/`dist`/quantifier binder
/// anywhere in it — keyed by each declaration's own span.
pub(crate) type Scope = [(Span, ResolvedSortId)];

/// Prepares a raw expression for inference: resolves its embedded binder sorts (see
/// [`DataSpecification::resolve_expression_binder_sorts`]) and lowers it, exactly as
/// [`DataSpecification::typecheck_expression`] does for a standalone expression.
pub(crate) fn prepare_expression<E>(data: &mut DataSpecification, expr: &DataExpr) -> Result<DataExpr, E>
where
    E: From<WellTypedError>,
{
    let mut expr = expr.clone();
    data.resolve_expression_binder_sorts(&mut expr)?;
    Ok(lower_data_expr(expr))
}

/// Checks `expr` (a `sum`/`dist` condition or time bound, an assignment-form instantiation
/// argument, a `PropVarInst` argument, …) against `expected`, merging its `TypingInfo` into
/// `typing` on success.
///
/// Generic over the caller's own error type `E` (`ProcessError`, `PbesError`, …), which must
/// convert from [`WellTypedError`] and [`InferenceError`].
pub(crate) fn check_expression_against<E>(
    data: &mut DataSpecification,
    scope: &Scope,
    expr: &DataExpr,
    expected: ResolvedSortId,
    typing: &mut TypingInfo,
) -> Result<(), E>
where
    E: From<WellTypedError> + From<InferenceError>,
{
    let lowered = prepare_expression::<E>(data, expr)?;
    let (ctx, spec, system) = data.context_and_specs_mut();
    let equation_typing = infer_expression_in_scope(ctx, spec, system, &lowered, scope, Some(expected))?;
    typing.merge(lsp_info::build(data, &equation_typing));
    Ok(())
}

/// Collects the sorts of the given binder variables, extending the current
/// scope and recording sort references, and records each variable's own declaration occurrence so
/// it can be hovered/go-to-definition'd the same as a use of it (see
/// [`lsp_info::push_binder_declaration`]).
pub(crate) fn collect_binder_sorts<E>(
    data: &mut DataSpecification,
    scope: &mut Vec<(Span, ResolvedSortId)>,
    sort_references: &mut Vec<(Span, String)>,
    typing: &mut TypingInfo,
    variables: &[IdDecl],
    mut resolve: impl FnMut(&mut DataSpecification, &SortExpression) -> Result<ResolvedSortId, E>,
) -> Result<(), E> {
    for var in variables {
        lsp_info::collect_sort_name_references(&var.sort, sort_references);
        let sort = resolve(data, &var.sort)?;
        lsp_info::push_binder_declaration(
            data,
            typing,
            var.identifier.span.clone(),
            var.identifier.node.clone(),
            sort,
        );
        scope.push((var.identifier.span.clone(), sort));
    }
    Ok(())
}
