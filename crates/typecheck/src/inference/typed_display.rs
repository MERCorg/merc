//! Renders a checked equation's expressions annotated with every sub-expression's resolved
//! sort, for a stricter regression signal than the plain (unannotated) equation text gives —
//! see [`crate::DataSpecification::to_typed_string`]'s doc comment for why this exists.

use std::fmt;

use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::EqnDecl;
use merc_syntax::UntypedDataSpecification;

use crate::DisplaySortContext;
use crate::EquationTyping;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;

/// A displayable wrapper around one expression, rendered in the same prefix notation
/// `Display for DataExpr` uses except every sub-expression is suffixed with `: <sort>` — its
/// own resolved sort.
pub(crate) struct TypedExpr<'a> {
    expr: &'a DataExpr,
    ctx: &'a TypeCheckContext,
    spec: &'a UntypedDataSpecification,
    typing: &'a EquationTyping,
}

impl<'a> TypedExpr<'a> {
    pub(crate) fn new(
        expr: &'a DataExpr,
        ctx: &'a TypeCheckContext,
        spec: &'a UntypedDataSpecification,
        typing: &'a EquationTyping,
    ) -> Self {
        Self {
            expr,
            ctx,
            spec,
            typing,
        }
    }

    /// Renders just this node's shape, without the trailing `: <sort>` annotation `Display`
    /// appends — what an `Application` renders its callee with in place, and what an
    /// `Application` with no arguments collapses to.
    fn fmt_shape(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let TypedExpr {
            expr,
            ctx,
            spec,
            typing,
        } = self;
        match &expr.node {
            DataExprKind::EmptyList => write!(f, "[]"),
            DataExprKind::EmptyBag => write!(f, "{{:}}"),
            DataExprKind::EmptySet => write!(f, "{{}}"),
            DataExprKind::Id(name) | DataExprKind::Resolved(name, _) => write!(f, "{name}"),
            DataExprKind::Number(value) => write!(f, "{value}"),
            DataExprKind::Bool(value) => write!(f, "{value}"),
            DataExprKind::Set(members) => {
                write!(f, "{{ ")?;
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", TypedExpr::new(member, ctx, spec, typing))?;
                }
                write!(f, " }}")
            }
            DataExprKind::Bag(members) => {
                write!(f, "{{ ")?;
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(
                        f,
                        "{}: {}",
                        TypedExpr::new(&member.expr, ctx, spec, typing),
                        TypedExpr::new(&member.multiplicity, ctx, spec, typing)
                    )?;
                }
                write!(f, " }}")
            }
            DataExprKind::SetBagComp { variable, predicate } => {
                // The bound variable has no `ExprId` of its own — see `visit`'s own comment —
                // so only the predicate is annotated.
                write!(f, "{{ {variable} | {} }}", TypedExpr::new(predicate, ctx, spec, typing))
            }
            DataExprKind::Application { function, arguments } => {
                TypedExpr::new(function, ctx, spec, typing).fmt_shape(f)?;

                // A defensively-made fallback for a callee taking no arguments at all (in
                // practice every parsed `Application` has at least one) collapses to exactly
                // the callee alone, matching the plain, unannotated `Display for DataExpr`.
                if arguments.is_empty() {
                    return Ok(());
                }
                write!(f, "(")?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", TypedExpr::new(argument, ctx, spec, typing))?;
                }
                write!(f, ")")
            }
            DataExprKind::Lambda { variables, body } => {
                let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
                write!(
                    f,
                    "(lambda {} . {})",
                    variables.join(", "),
                    TypedExpr::new(body, ctx, spec, typing)
                )
            }
            DataExprKind::Quantifier { op, variables, body } => {
                let variables: Vec<String> = variables.iter().map(ToString::to_string).collect();
                write!(
                    f,
                    "({op} {} . {})",
                    variables.join(", "),
                    TypedExpr::new(body, ctx, spec, typing)
                )
            }
            DataExprKind::Whr { expr, assignments } => {
                let mut parts = Vec::with_capacity(assignments.len());
                for assignment in assignments {
                    parts.push(format!(
                        "{} = {}",
                        assignment.identifier,
                        TypedExpr::new(&assignment.expr, ctx, spec, typing)
                    ));
                }
                write!(
                    f,
                    "{} whr {} end",
                    TypedExpr::new(expr, ctx, spec, typing),
                    parts.join(", ")
                )
            }
            DataExprKind::List(_)
            | DataExprKind::Unary { .. }
            | DataExprKind::Binary { .. }
            | DataExprKind::FunctionUpdate { .. } => {
                unreachable!("typed-display requires a lowered expression, exactly like inference itself")
            }
        }
    }
}

impl fmt::Display for TypedExpr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // An `Application`'s trailing annotation is the *applied* (base) callee's sort — a full
        // domain `#`-separated `-> range` arrow — rather than this node's own (just the range),
        // so `f(x)` reads as `f(x: S): (S -> T)` instead of the more redundant
        // `f: (S -> T)(x: S): T`. For a spine like `b(i)(j)` the base callee is `b` itself —
        // the inner `b(i)` node's sort is only the arrow's prefix — reached by descending each
        // `Application`'s own `function`.
        let sort = match &self.expr.node {
            DataExprKind::Application { function, .. } => {
                let mut base = function;
                while let DataExprKind::Application { function: inner, .. } = &base.node {
                    base = inner;
                }
                node_sort(base, self.typing)
            }
            _ => node_sort(self.expr, self.typing),
        };
        self.fmt_shape(f)?;
        let display = DisplaySortContext::new(self.ctx, self.spec, sort);
        if matches!(self.ctx.sorts.get(sort), ResolvedSort::Function { .. }) {
            write!(f, ": ({display})")
        } else {
            write!(f, ": {display}")
        }
    }
}

/// A displayable equation — `condition -> lhs = rhs`, or plain `lhs = rhs` with no condition —
/// with every sub-expression suffixed with its resolved sort (see [`TypedExpr`]).
pub(crate) struct TypedEquation<'a> {
    eqn: &'a EqnDecl,
    ctx: &'a TypeCheckContext,
    spec: &'a UntypedDataSpecification,
    typing: &'a EquationTyping,
}

impl<'a> TypedEquation<'a> {
    pub(crate) fn new(
        eqn: &'a EqnDecl,
        ctx: &'a TypeCheckContext,
        spec: &'a UntypedDataSpecification,
        typing: &'a EquationTyping,
    ) -> Self {
        Self { eqn, ctx, spec, typing }
    }
}

impl fmt::Display for TypedEquation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let TypedEquation { eqn, ctx, spec, typing } = self;
        let lhs = TypedExpr::new(&eqn.lhs, ctx, spec, typing);
        let rhs = TypedExpr::new(&eqn.rhs, ctx, spec, typing);
        match &eqn.condition {
            Some(condition) => write!(
                f,
                "{} -> {} = {}",
                TypedExpr::new(condition, ctx, spec, typing),
                lhs,
                rhs
            ),
            None => write!(f, "{} = {}", lhs, rhs),
        }
    }
}

/// The `ExprId` `typing` recorded for `expr` (see [`EquationTyping::node_ids`]), looked up by
/// `expr`'s own address rather than by replaying `ConstraintGenerator::visit`'s traversal
/// order — `expr` must come from the same tree `typing` was computed against.
fn node_sort(expr: &DataExpr, typing: &EquationTyping) -> ResolvedSortId {
    let &id = typing
        .node_ids
        .get(&(expr as *const DataExpr as usize))
        .expect("typed-display only ever visits nodes of the tree `typing` was computed against");
    typing.sorts[id]
}
