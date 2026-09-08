//! Renders a checked equation's expressions annotated with every sub-expression's resolved
//! sort, for a stricter regression signal than the plain (unannotated) equation text gives —
//! see [`crate::DataSpecification::to_typed_string`]'s doc comment for why this exists.

use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::EqnDecl;
use merc_syntax::UntypedDataSpecification;

use crate::DisplaySortContext;
use crate::EquationTyping;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;

/// As [`typed_expr_string`], but returns the node's own resolved sort alongside its text instead
/// of appending it — the building block [`typed_expr_string`] wraps, and what an `Application`
/// uses to show the *applied function's* sort (a full domain `#`-separated `-> range` arrow)
/// rather than its own (just the range) as the call's trailing annotation, so `f(x)` reads as
/// `f(x: S): (S -> T)` instead of the more redundant `f: (S -> T)(x: S): T`.
fn typed_expr_shape(
    expr: &DataExpr,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    typing: &EquationTyping,
    cursor: &mut usize,
) -> (String, ResolvedSortId) {
    let id = *cursor;
    *cursor += 1;
    debug_assert_eq!(
        typing.spans[id], expr.span,
        "typed-display traversal drifted out of sync with ConstraintGenerator::visit's ExprId order"
    );
    let sort = typing.sorts[id];

    let shape = match &expr.node {
        DataExprKind::EmptyList => "[]".to_string(),
        DataExprKind::EmptyBag => "{:}".to_string(),
        DataExprKind::EmptySet => "{}".to_string(),
        DataExprKind::Id(name) | DataExprKind::Resolved(name, _) => name.clone(),
        DataExprKind::Number(value) => value.clone(),
        DataExprKind::Bool(value) => value.to_string(),
        DataExprKind::Set(members) => {
            let mut parts = Vec::with_capacity(members.len());
            for member in members {
                parts.push(typed_expr_string(member, ctx, spec, system, typing, cursor));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        DataExprKind::Bag(members) => {
            let mut parts = Vec::with_capacity(members.len());
            for member in members {
                // Mirrors `visit`: each member's own expression is consumed before its
                // multiplicity.
                let element = typed_expr_string(&member.expr, ctx, spec, system, typing, cursor);
                let count = typed_expr_string(&member.multiplicity, ctx, spec, system, typing, cursor);
                parts.push(format!("{element}: {count}"));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        DataExprKind::SetBagComp { variable, predicate } => {
            // The bound variable has no `ExprId` of its own — see `visit`'s own comment — so
            // only the predicate is annotated.
            let predicate = typed_expr_string(predicate, ctx, spec, system, typing, cursor);
            format!("{{ {variable} | {predicate} }}")
        }
        DataExprKind::Application { function, arguments } => {
            // Mirrors `visit`: arguments are consumed (and so numbered) before the applied
            // function.
            let args: Vec<String> = arguments
                .iter()
                .map(|argument| typed_expr_string(argument, ctx, spec, system, typing, cursor))
                .collect();
            let (function_shape, function_sort) = typed_expr_shape(function, ctx, spec, system, typing, cursor);

            // The whole call's own trailing annotation is the *applied function's* sort (its
            // full arrow), not this `Application` node's own (just the arrow's range) — see this
            // function's own doc comment. A defensive fallback for a callee taking no arguments
            // at all (in practice every parsed `Application` has at least one) collapses to
            // exactly the callee alone, matching the plain, unannotated `Display for DataExpr`.
            if args.is_empty() {
                return (function_shape, function_sort);
            }
            return (format!("{function_shape}({})", args.join(", ")), function_sort);
        }
        DataExprKind::Lambda { variables, body } => {
            let body = typed_expr_string(body, ctx, spec, system, typing, cursor);
            let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
            format!("(lambda {} . {body})", variables.join(", "))
        }
        DataExprKind::Quantifier { op, variables, body } => {
            let body = typed_expr_string(body, ctx, spec, system, typing, cursor);
            let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
            format!("({op} {} . {body})", variables.join(", "))
        }
        DataExprKind::Whr { expr, assignments } => {
            // Mirrors `visit`: every assignment's own value is consumed before the body.
            let mut parts = Vec::with_capacity(assignments.len());
            for assignment in assignments {
                let value = typed_expr_string(&assignment.expr, ctx, spec, system, typing, cursor);
                parts.push(format!("{} = {value}", assignment.identifier));
            }
            let body = typed_expr_string(expr, ctx, spec, system, typing, cursor);
            format!("{body} whr {} end", parts.join(", "))
        }
        DataExprKind::List(_)
        | DataExprKind::Unary { .. }
        | DataExprKind::Binary { .. }
        | DataExprKind::FunctionUpdate { .. } => {
            unreachable!("typed-display requires a lowered expression, exactly like inference itself")
        }
    };

    (shape, sort)
}

/// Renders `expr` in the same prefix notation `Display for DataExpr` uses, except every
/// sub-expression is suffixed with `: <sort>` — its own resolved sort, read off `typing` (an
/// applied function's own arrow sort in place of the call's — see [`typed_expr_shape`]'s doc
/// comment), parenthesized when it is itself an arrow (matching how a `map`/`cons` declaration's
/// own function sort is parenthesized in this same file's header).
///
/// `cursor` walks `typing.sorts`/`typing.spans` (both `ExprId`-indexed) one entry per recursive
/// call, advancing in exactly the order `ConstraintGenerator::visit` assigned `ExprId`s in:
/// parents before children, and within an `Application` the arguments before the applied
/// function (see that function's own doc comment). Each call `debug_assert`s that the span it
/// consumes matches `expr`'s own, so if a future change to `visit`'s traversal order drifts out
/// of sync with this mirror, a debug build catches it immediately rather than silently
/// mislabeling sorts.
pub(crate) fn typed_expr_string(
    expr: &DataExpr,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    typing: &EquationTyping,
    cursor: &mut usize,
) -> String {
    let (shape, sort) = typed_expr_shape(expr, ctx, spec, system, typing, cursor);
    let display = DisplaySortContext::new(ctx, spec, system, sort);
    if matches!(ctx.sorts.get(sort), ResolvedSort::Function { .. }) {
        format!("{shape}: ({display})")
    } else {
        format!("{shape}: {display}")
    }
}

/// As [`typed_expr_string`], for a whole equation: `condition -> lhs = rhs`, or plain `lhs = rhs`
/// with no condition. Uses one shared `cursor`, starting at `0`, across the condition (if any),
/// then the left-hand side, then the right-hand side — the same order
/// `ConstraintGenerator::generate` visits them in for one equation's own `EquationTyping`.
pub(crate) fn typed_equation_string(
    eqn: &EqnDecl,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    typing: &EquationTyping,
) -> String {
    let mut cursor = 0;
    let condition = eqn
        .condition
        .as_ref()
        .map(|condition| typed_expr_string(condition, ctx, spec, system, typing, &mut cursor));
    let lhs = typed_expr_string(&eqn.lhs, ctx, spec, system, typing, &mut cursor);
    let rhs = typed_expr_string(&eqn.rhs, ctx, spec, system, typing, &mut cursor);

    match condition {
        Some(condition) => format!("{condition} -> {lhs} = {rhs}"),
        None => format!("{lhs} = {rhs}"),
    }
}
