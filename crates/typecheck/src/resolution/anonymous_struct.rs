use std::ops::ControlFlow;

use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::Span;
use merc_syntax::Traverse;

/// The span of the first anonymous `struct` anywhere within `sort`, if any.
pub(crate) fn find_anonymous_struct(sort: &SortExpression) -> Option<Span> {
    sort.visit(|expr| match &expr.node {
        SortExpressionKind::Struct { .. } => ControlFlow::Break(expr.span.clone()),
        _ => ControlFlow::Continue(()),
    })
}
