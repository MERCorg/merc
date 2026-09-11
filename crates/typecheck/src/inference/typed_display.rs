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

/// The `ExprId` `typing` recorded for `expr` (see [`EquationTyping::node_ids`]), looked up by
/// `expr`'s own address rather than by replaying `ConstraintGenerator::visit`'s traversal order —
/// `expr` must come from the same `spec`/`system` tree `typing` was computed against.
fn node_sort(expr: &DataExpr, typing: &EquationTyping) -> ResolvedSortId {
    let &id = typing
        .node_ids
        .get(&(expr as *const DataExpr as usize))
        .expect("typed-display only ever visits nodes of the tree `typing` was computed against");
    typing.sorts[id]
}

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
) -> (String, ResolvedSortId) {
    let sort = node_sort(expr, typing);

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
                parts.push(typed_expr_string(member, ctx, spec, system, typing));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        DataExprKind::Bag(members) => {
            let mut parts = Vec::with_capacity(members.len());
            for member in members {
                let element = typed_expr_string(&member.expr, ctx, spec, system, typing);
                let count = typed_expr_string(&member.multiplicity, ctx, spec, system, typing);
                parts.push(format!("{element}: {count}"));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        DataExprKind::SetBagComp { variable, predicate } => {
            // The bound variable has no `ExprId` of its own — see `visit`'s own comment — so
            // only the predicate is annotated.
            let predicate = typed_expr_string(predicate, ctx, spec, system, typing);
            format!("{{ {variable} | {predicate} }}")
        }
        DataExprKind::Application { function, arguments } => {
            let args: Vec<String> = arguments
                .iter()
                .map(|argument| typed_expr_string(argument, ctx, spec, system, typing))
                .collect();
            let (function_shape, function_sort) = typed_expr_shape(function, ctx, spec, system, typing);

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
            let body = typed_expr_string(body, ctx, spec, system, typing);
            let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
            format!("(lambda {} . {body})", variables.join(", "))
        }
        DataExprKind::Quantifier { op, variables, body } => {
            let body = typed_expr_string(body, ctx, spec, system, typing);
            let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
            format!("({op} {} . {body})", variables.join(", "))
        }
        DataExprKind::Whr { expr, assignments } => {
            let mut parts = Vec::with_capacity(assignments.len());
            for assignment in assignments {
                let value = typed_expr_string(&assignment.expr, ctx, spec, system, typing);
                parts.push(format!("{} = {value}", assignment.identifier));
            }
            let body = typed_expr_string(expr, ctx, spec, system, typing);
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
/// sub-expression is suffixed with `: <sort>` — its own resolved sort, read off `typing` by node
/// identity (see [`node_sort`]; an applied function's own arrow sort in place of the call's — see
/// [`typed_expr_shape`]'s doc comment), parenthesized when it is itself an arrow (matching how a
/// `map`/`cons` declaration's own function sort is parenthesized in this same file's header).
pub(crate) fn typed_expr_string(
    expr: &DataExpr,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    typing: &EquationTyping,
) -> String {
    let (shape, sort) = typed_expr_shape(expr, ctx, spec, system, typing);
    let display = DisplaySortContext::new(ctx, spec, system, sort);
    if matches!(ctx.sorts.get(sort), ResolvedSort::Function { .. }) {
        format!("{shape}: ({display})")
    } else {
        format!("{shape}: {display}")
    }
}

/// As [`typed_expr_string`], for a whole equation: `condition -> lhs = rhs`, or plain `lhs = rhs`
/// with no condition.
pub(crate) fn typed_equation_string(
    eqn: &EqnDecl,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    typing: &EquationTyping,
) -> String {
    let condition = eqn
        .condition
        .as_ref()
        .map(|condition| typed_expr_string(condition, ctx, spec, system, typing));
    let lhs = typed_expr_string(&eqn.lhs, ctx, spec, system, typing);
    let rhs = typed_expr_string(&eqn.rhs, ctx, spec, system, typing);

    match condition {
        Some(condition) => format!("{condition} -> {lhs} = {rhs}"),
        None => format!("{lhs} = {rhs}"),
    }
}
