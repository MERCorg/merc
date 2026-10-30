use std::collections::HashMap;
use std::convert::Infallible;
use std::hash::BuildHasher;
use std::ops::ControlFlow;

use merc_aterm::ATermList;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_utilities::Step;
use merc_utilities::Visit;

use crate::PbesExpression;
use crate::PbesExpressionRef;
use crate::PbesKind;
use crate::PropositionalVariableInstantiationRef;

/// A top-down traversal over PBES expressions that threads a context down the expression.
///
/// There is one method per [PbesKind], so a visitor only overrides the kinds it cares about and
/// the remaining ones keep descending. Every method returns a [Visit], so a traversal that has
/// found what it was looking for stops without walking the rest of the expression, and
/// subexpressions that cannot contribute are skipped with [Step::Prune]. The context is passed
/// from a node to its operands, which lets a visitor track where it is without maintaining a
/// stack of its own.
///
/// Expressions are maximally shared, so the same subexpression is typically reachable along many
/// paths. A visitor whose work per node is not trivial should remember the nodes it has seen and
/// prune the repeats, or compute its result with [fold_pbes_expr] instead.
///
/// The traversal only inspects expressions; [Step::Replace] is uninhabited here. Data expressions
/// are leaves: use a `merc_data::DataExpressionVisitor` from [PbesExpressionVisitor::visit_data]
/// to look inside them. The arguments of an instantiation are not visited either.
pub trait PbesExpressionVisitor {
    /// Threaded from a node to its operands.
    type Context: Copy;

    /// Produced when the traversal stops early.
    type Break;

    type Error;

    /// The returned [Step] is ignored, since a constant has no operands.
    fn visit_constant(
        &mut self,
        _value: bool,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    /// The returned [Step] is ignored, since a data expression is a leaf of the PBES expression.
    fn visit_data(
        &mut self,
        _expr: &DataExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    /// The returned [Step] is ignored, since an instantiation has no PBES operands.
    fn visit_instantiation(
        &mut self,
        _instantiation: &PropositionalVariableInstantiationRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_not(
        &mut self,
        _body: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_and(
        &mut self,
        _lhs: &PbesExpressionRef<'_>,
        _rhs: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_or(
        &mut self,
        _lhs: &PbesExpressionRef<'_>,
        _rhs: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_imp(
        &mut self,
        _lhs: &PbesExpressionRef<'_>,
        _rhs: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_forall(
        &mut self,
        _variables: &ATermList<DataVariable>,
        _body: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    fn visit_exists(
        &mut self,
        _variables: &ATermList<DataVariable>,
        _body: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Visit<Infallible, Self::Context, Self::Break, Self::Error> {
        Ok(ControlFlow::Continue(Step::Into(context)))
    }

    /// Visits `expr` and, unless a visit method breaks or prunes, its operands, left to right.
    fn try_visit(
        &mut self,
        expr: &PbesExpressionRef<'_>,
        context: Self::Context,
    ) -> Result<Option<Self::Break>, Self::Error> {
        traverse(expr, context, |_expr, kind, context| match kind {
            PbesKind::True => self.visit_constant(true, context),
            PbesKind::False => self.visit_constant(false, context),
            PbesKind::Data(data) => self.visit_data(data, context),
            PbesKind::Instantiation(instantiation) => self.visit_instantiation(instantiation, context),
            PbesKind::Not(body) => self.visit_not(body, context),
            PbesKind::And(lhs, rhs) => self.visit_and(lhs, rhs, context),
            PbesKind::Or(lhs, rhs) => self.visit_or(lhs, rhs, context),
            PbesKind::Imp(lhs, rhs) => self.visit_imp(lhs, rhs, context),
            PbesKind::Forall(variables, body) => self.visit_forall(variables, body, context),
            PbesKind::Exists(variables, body) => self.visit_exists(variables, body, context),
        })
    }

    /// See [PbesExpressionVisitor::try_visit], for visitors that cannot fail.
    fn visit(&mut self, expr: &PbesExpressionRef<'_>, context: Self::Context) -> Option<Self::Break>
    where
        Self: PbesExpressionVisitor<Error = Infallible>,
    {
        match self.try_visit(expr, context) {
            Ok(result) => result,
            Err(error) => match error {},
        }
    }
}

/// Visits `expr` and its subexpressions top-down, calling `visitor` on every node.
///
/// See [PbesExpressionVisitor] for the meaning of the context and the return value; implement
/// that trait directly when the node kinds need to be treated differently.
pub fn try_visit_pbes_expr<'a, C, T, E, F>(
    expr: &PbesExpressionRef<'a>,
    context: C,
    mut visitor: F,
) -> Result<Option<T>, E>
where
    C: Copy,
    F: FnMut(&PbesExpressionRef<'a>, C) -> Visit<Infallible, C, T, E>,
{
    traverse(expr, context, |expr, _kind, context| visitor(expr, context))
}

/// See [try_visit_pbes_expr], for visitors that cannot fail.
pub fn visit_pbes_expr<'a, C, T, F>(expr: &PbesExpressionRef<'a>, context: C, mut visitor: F) -> Option<T>
where
    C: Copy,
    F: FnMut(&PbesExpressionRef<'a>, C) -> ControlFlow<T, Step<Infallible, C>>,
{
    match try_visit_pbes_expr::<C, T, Infallible, _>(expr, context, |expr, context| Ok(visitor(expr, context))) {
        Ok(result) => result,
        Err(error) => match error {},
    }
}

/// The single traversal behind [PbesExpressionVisitor] and [try_visit_pbes_expr]. It keeps its
/// own worklist rather than using the call stack, so long chains such as those produced by
/// quantifier enumeration are traversed without overflowing it.
fn traverse<'a, C, T, E, F>(expr: &PbesExpressionRef<'a>, context: C, mut visitor: F) -> Result<Option<T>, E>
where
    C: Copy,
    F: FnMut(&PbesExpressionRef<'a>, &PbesKind<'a>, C) -> Visit<Infallible, C, T, E>,
{
    let mut stack = vec![(expr.copy(), context)];
    while let Some((expr, context)) = stack.pop() {
        let kind = expr.kind();
        let context = match visitor(&expr, &kind, context)? {
            ControlFlow::Break(result) => return Ok(Some(result)),
            ControlFlow::Continue(Step::Prune) => continue,
            ControlFlow::Continue(Step::Replace(replacement)) => match replacement {},
            ControlFlow::Continue(Step::Into(context)) => context,
        };

        // Pushed in reverse, so that the operands are visited left to right.
        let [lhs, rhs] = operands(kind);
        stack.extend(rhs.map(|rhs| (rhs, context)));
        stack.extend(lhs.map(|lhs| (lhs, context)));
    }

    Ok(None)
}

/// The shape of a PBES expression whose operands have been replaced by the values folded from
/// them; see [fold_pbes_expr].
pub enum PbesFold<'a, T> {
    True,
    False,
    /// A Boolean data expression other than the constants `true` and `false`.
    Data(DataExpressionRef<'a>),
    Instantiation(PropositionalVariableInstantiationRef<'a>),
    Not(T),
    And(T, T),
    Or(T, T),
    Imp(T, T),
    Forall(ATermList<DataVariable>, T),
    Exists(ATermList<DataVariable>, T),
}

/// Computes a value for `expr` bottom-up: `fold` is called once for every distinct subexpression,
/// with that subexpression and its shape with the operands replaced by their values.
///
/// The values are memoised in `cache`, so a subexpression that is shared, within `expr` or with
/// the expressions of earlier calls that used the same cache, is folded only once. This keeps the
/// work linear in the size of the term graph rather than of the tree it unfolds to.
pub fn fold_pbes_expr<T, S, F>(
    expr: &PbesExpressionRef<'_>,
    cache: &mut HashMap<PbesExpression, T, S>,
    mut fold: F,
) -> T
where
    T: Clone,
    S: BuildHasher,
    F: FnMut(&PbesExpressionRef<'_>, PbesFold<'_, T>) -> T,
{
    // A post-order traversal with an explicit stack: a node is pushed again, marked as expanded,
    // below its operands, so that it is folded once all of them have been.
    let mut stack = vec![(expr.copy(), false)];
    while let Some((expr, expanded)) = stack.pop() {
        let key = expr.protect();
        if cache.contains_key(&key) {
            continue;
        }

        // A leaf has no operands to wait for, so it is folded right away.
        if !expanded && let [Some(lhs), rhs] = operands(expr.kind()) {
            stack.push((expr, true));
            stack.extend(rhs.map(|rhs| (rhs, false)));
            stack.push((lhs, false));
            continue;
        }

        let value = |operand: PbesExpressionRef<'_>| cache[&operand.protect()].clone();
        let node = match expr.kind() {
            PbesKind::True => PbesFold::True,
            PbesKind::False => PbesFold::False,
            PbesKind::Data(data) => PbesFold::Data(data),
            PbesKind::Instantiation(instantiation) => PbesFold::Instantiation(instantiation),
            PbesKind::Not(body) => PbesFold::Not(value(body)),
            PbesKind::And(lhs, rhs) => PbesFold::And(value(lhs), value(rhs)),
            PbesKind::Or(lhs, rhs) => PbesFold::Or(value(lhs), value(rhs)),
            PbesKind::Imp(lhs, rhs) => PbesFold::Imp(value(lhs), value(rhs)),
            PbesKind::Forall(variables, body) => PbesFold::Forall(variables, value(body)),
            PbesKind::Exists(variables, body) => PbesFold::Exists(variables, value(body)),
        };
        let value = fold(&expr, node);
        cache.insert(key, value);
    }

    cache[&expr.protect()].clone()
}

/// The PBES operands directly below a node of the given shape, left to right.
fn operands(kind: PbesKind<'_>) -> [Option<PbesExpressionRef<'_>>; 2] {
    match kind {
        PbesKind::True | PbesKind::False | PbesKind::Data(_) | PbesKind::Instantiation(_) => [None, None],
        PbesKind::Not(body) | PbesKind::Forall(_, body) | PbesKind::Exists(_, body) => [Some(body), None],
        PbesKind::And(lhs, rhs) | PbesKind::Or(lhs, rhs) | PbesKind::Imp(lhs, rhs) => [Some(lhs), Some(rhs)],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::ops::ControlFlow;

    use merc_utilities::Step;
    use merc_utilities::Visit;

    use crate::PbesExpression;
    use crate::PbesExpressionRef;
    use crate::PbesExpressionVisitor;
    use crate::PbesFold;
    use crate::PropositionalVariableInstantiation;
    use crate::PropositionalVariableInstantiationRef;
    use crate::fold_pbes_expr;
    use crate::visit_pbes_expr;

    fn instantiation(name: &str) -> PbesExpression {
        PropositionalVariableInstantiation::new(name, &[]).into()
    }

    /// `(X && Y) || !Z`
    fn example() -> PbesExpression {
        PbesExpression::or(
            &PbesExpression::and(&instantiation("X"), &instantiation("Y")),
            &PbesExpression::not(&instantiation("Z")),
        )
    }

    /// Collects the name of every instantiation in the order in which it is visited.
    #[derive(Default)]
    struct InstantiationNames {
        names: Vec<String>,
    }

    impl PbesExpressionVisitor for InstantiationNames {
        type Context = ();
        type Break = Infallible;
        type Error = Infallible;

        fn visit_instantiation(
            &mut self,
            instantiation: &PropositionalVariableInstantiationRef<'_>,
            context: (),
        ) -> Visit<Infallible, (), Infallible, Infallible> {
            self.names.push(instantiation.name().value().to_string());
            Ok(ControlFlow::Continue(Step::Into(context)))
        }
    }

    #[test]
    fn test_visit_order_is_left_to_right() {
        let mut visitor = InstantiationNames::default();
        assert!(visitor.visit(&example().copy(), ()).is_none());
        assert_eq!(visitor.names, ["X", "Y", "Z"]);
    }

    #[test]
    fn test_visit_breaks_early() {
        let mut visited = 0;
        let found = visit_pbes_expr(&example().copy(), (), |expr, context| {
            visited += 1;
            if format!("{expr}") == "Y" {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(Step::Into(context))
            }
        });

        assert!(found.is_some());
        assert_eq!(visited, 4, "expected ||, &&, X and Y to be visited");
    }

    #[test]
    fn test_visit_prunes_and_threads_context() {
        // The context is the depth of the node; everything below the `&&` is skipped.
        let mut depths = Vec::new();
        let result: Option<Infallible> =
            visit_pbes_expr(&example().copy(), 0, |expr: &PbesExpressionRef<'_>, depth| {
                depths.push((format!("{expr}"), depth));
                if format!("{expr}") == "X && Y" {
                    ControlFlow::Continue(Step::Prune)
                } else {
                    ControlFlow::Continue(Step::Into(depth + 1))
                }
            });

        assert!(result.is_none());
        assert_eq!(
            depths,
            [
                ("(X && Y) || !Z".to_string(), 0),
                ("X && Y".to_string(), 1),
                ("!Z".to_string(), 1),
                ("Z".to_string(), 2),
            ]
        );
    }

    /// Every distinct subexpression is folded once, operands before the node that contains them,
    /// and the cache is reused by later folds.
    #[test]
    fn test_fold_is_bottom_up_and_memoised() {
        let shared = PbesExpression::and(&instantiation("X"), &instantiation("Y"));
        let expr = PbesExpression::or(&shared, &PbesExpression::not(&shared));

        let mut folded = Vec::new();
        let mut cache = HashMap::new();
        let size = |expr: &PbesExpressionRef<'_>, node: PbesFold<'_, usize>, folded: &mut Vec<String>| {
            folded.push(format!("{expr}"));
            match node {
                PbesFold::True | PbesFold::False | PbesFold::Data(_) | PbesFold::Instantiation(_) => 1,
                PbesFold::Not(body) | PbesFold::Forall(_, body) | PbesFold::Exists(_, body) => body + 1,
                PbesFold::And(lhs, rhs) | PbesFold::Or(lhs, rhs) | PbesFold::Imp(lhs, rhs) => lhs + rhs + 1,
            }
        };

        // The size of the tree, even though `shared` is folded only once.
        let result = fold_pbes_expr(&expr.copy(), &mut cache, |expr, node| size(expr, node, &mut folded));
        assert_eq!(result, 8);
        assert_eq!(folded, ["X", "Y", "X && Y", "!(X && Y)", "(X && Y) || !(X && Y)"]);

        folded.clear();
        let result = fold_pbes_expr(&shared.copy(), &mut cache, |expr, node| size(expr, node, &mut folded));
        assert_eq!(result, 3);
        assert!(folded.is_empty());
    }
}
