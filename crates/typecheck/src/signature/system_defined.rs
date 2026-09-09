use std::collections::BTreeMap;
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::ops::Range;

use merc_syntax::ComplexSort;
use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::SourceMap;
use merc_syntax::Traverse;
use merc_syntax::UntypedDataSpecification;

use crate::NumberEncoding;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;
use crate::WellTypedError;
use crate::is_supported_binder_sort;
use crate::lower_data_expressions;
use crate::polymorphic_operator_names;
use crate::standard_sort;

/// One element-sort-scoped group of generated Appendix-B content, and the
/// range of `equation_declarations` indices it occupies.
///
/// Two instantiations of the same container template (`Bag(Nat)`, `Bag(D)`)
/// each carry a copy of its equations, and some of those (`@zero_ == @one_` in
/// `bag.mcrl2`) mention no argument pinning down which copy they belong to, so
/// one pooled signature would make them genuinely ambiguous.
pub(crate) struct SystemEquationGroup {
    pub(crate) declarations: UntypedDataSpecification,
    pub(crate) equation_range: Range<usize>,
}

/// The grouping key of a concrete container sort: its element one level down,
/// so a template and its transitive dependencies share a key
/// (`Bag(Nat)`/`FSet(Nat)`/`Set(Nat)` all key on `Nat`).
///
/// Deliberately not recursive: recursing would key `FSet(Set(Nat))` on `Nat`,
/// the same as an unrelated `Set(Nat)`, reintroducing the ambiguity
/// [SystemEquationGroup] exists to prevent.
fn container_group_key(sort: &SortExpression) -> SortExpression {
    match &sort.node {
        SortExpressionKind::Complex(_, subsort) => (**subsort).clone(),
        _ => sort.clone(),
    }
}

/// Drains `worklist` to a fixpoint like [expand_container_sorts], but keeps
/// each popped sort's generated batch separate, then partitions the batches by
/// [container_group_key] and merges each partition into `result` as its own
/// [SystemEquationGroup]. The shared `seen` keeps grouping from changing *what*
/// is generated, only how it is partitioned. Partitions are kept in a
/// `BTreeMap`, not a `HashMap`, so the group (and so equation) order in
/// `result` is deterministic across runs; callers must also pass `worklist`
/// in a deterministic order for the same reason.
fn group_and_merge(
    sources: &mut SourceMap,
    result: &mut UntypedDataSpecification,
    mut worklist: Vec<SortExpression>,
    seen: &HashSet<SortExpression>,
    encoding: NumberEncoding,
) -> Vec<SystemEquationGroup> {
    let mut seen = seen.clone();
    let mut generated_by_sort: Vec<(SortExpression, UntypedDataSpecification)> = Vec::new();
    while let Some(sort) = worklist.pop() {
        if !seen.insert(sort.clone()) {
            continue;
        }
        let generated = standard_sort(sources, &sort, encoding);
        collect_system_sorts_in_spec(&generated, &mut worklist, false);
        generated_by_sort.push((sort, generated));
    }

    let mut by_key: BTreeMap<SortExpression, UntypedDataSpecification> = BTreeMap::new();
    for (sort, generated) in &generated_by_sort {
        by_key.entry(container_group_key(sort)).or_default().merge(generated);
    }

    let mut groups = Vec::with_capacity(by_key.len());
    for (_, mut declarations) in by_key {
        lower_data_expressions(&mut declarations);

        let start = result.equation_declarations.len();
        result.merge(&declarations);
        let end = result.equation_declarations.len();

        groups.push(SystemEquationGroup {
            declarations,
            equation_range: start..end,
        });
    }
    groups
}

/// Builds the system-defined part of a specification: the Appendix-B
/// definitions (constructors, mappings and equations) for every basic sort,
/// container sort and single-argument function sort that occurs in `spec`.
///
/// The five basic sorts are always included. A container sort pulls in the
/// containers it is defined in terms of — a `Set(S)` needs `FSet(S)`, a `Bag(S)`
/// needs `FBag(S)`, `FSet(S)` and `Set(S)` — which the fixpoint below discovers
/// by re-scanning each generated specification. A function sort
/// `D_0 # ... # D_{n-1} -> T` contributes the function-update operators for
/// its declared arity — the bundled single-argument template when `n == 1`,
/// otherwise [standard_sort] generalizes it to the flattened domain.
/// Structured-sort equations are generated separately from the desugared
/// declarations and merged in by `DataSpecification::from_untyped`.
///
/// The result is deliberately left unresolved: it uses the built-in `Simple`
/// sorts and the Appendix-B operator names, and is not re-checked against the
/// user-oriented well-typedness rules.
///
/// `basics` is the [`basic_sort_data_specification`](crate::basic_sort_data_specification), passed in because the
/// caller also needs it separately for the system signature.
///
/// Returns the merged specification alongside the [SystemEquationGroup]s its
/// content was generated in.
pub(crate) fn build_system_defined_specification(
    sources: &mut SourceMap,
    spec: &UntypedDataSpecification,
    basics: UntypedDataSpecification,
    encoding: NumberEncoding,
) -> (UntypedDataSpecification, Vec<SystemEquationGroup>) {
    let mut result = basics;

    let mut worklist = Vec::new();
    // Seed from the user specification, including its function sorts.
    collect_system_sorts_in_spec(spec, &mut worklist, true);

    let groups = group_and_merge(sources, &mut result, worklist, &HashSet::new(), encoding);

    (result, groups)
}

/// Drains `worklist` to a fixpoint: for every sort popped that has not already
/// been `seen`, generates its Appendix-B specification and passes it to
/// `on_generated`, then re-scans the generated content for further container
/// sorts it in turn depends on (a container is defined in terms of other
/// containers, e.g. `Set(S)` needs `FSet(S)`) and pushes those too.
///
/// Function sorts are not re-collected from generated content (only from the
/// initial `worklist`): the function-update operators introduce ever-larger
/// function sorts (`@is_not_an_update: (S -> T) -> Bool`), which would not
/// terminate here.
fn expand_container_sorts(
    sources: &mut SourceMap,
    mut worklist: Vec<SortExpression>,
    seen: &mut HashSet<SortExpression>,
    encoding: NumberEncoding,
    mut on_generated: impl FnMut(&UntypedDataSpecification),
) {
    while let Some(sort) = worklist.pop() {
        if !seen.insert(sort.clone()) {
            continue;
        }

        let generated = standard_sort(sources, &sort, encoding);
        collect_system_sorts_in_spec(&generated, &mut worklist, false);
        on_generated(&generated);
    }
}

/// Extends `system` with the Appendix-B declarations of every container sort
/// that is discovered only through Phase-3 inference rather than appearing in
/// the textual declarations: the element sort of a `List`/`Set`/`Bag`
/// enumeration literal (`[1, 2]`, `{1, 2}`, `{1: 2}`) is not written down
/// anywhere — it is entirely a product of its elements' inferred sorts (see
/// [collect_system_sorts_in_expr]'s doc comment) — so [build_system_defined_specification]'s
/// syntactic scan misses it whenever the same container sort does not also
/// occur, spelled out, elsewhere in the specification.
///
/// `ctx` must be the context [crate::check_equations] populated: every
/// container sort reachable from a successfully typed equation's per-node
/// sorts is a candidate. Which of those `system` already covers is not
/// recorded anywhere (containers are structural, not named, so `system`
/// carries no direct list of them), so the syntactic scan is replayed here to
/// reconstruct that set before diffing against it.
///
/// Returns a new specification plus the [SystemEquationGroup]s of the newly
/// added content; `system` itself is left untouched, so calling this repeatedly (as
/// [crate::DataSpecification::lower_data_specification] may be) keeps
/// producing the same result from the same inputs.
pub(crate) fn extend_system_with_inferred_sorts(
    sources: &mut SourceMap,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    encoding: NumberEncoding,
) -> (UntypedDataSpecification, Vec<SystemEquationGroup>) {
    let mut result = system.clone();

    // Reconstruct the set of container sorts `system` already covers.
    let mut seen: HashSet<SortExpression> = HashSet::new();
    let mut covered = Vec::new();
    collect_system_sorts_in_spec(spec, &mut covered, true);
    expand_container_sorts(sources, covered, &mut seen, encoding, |_| {});

    // Every container sort that shows up as the inferred sort of some
    // expression node in a well-typed equation, not already covered above.
    let mut worklist = Vec::new();
    for typing in ctx.equation_typing.values().filter_map(|typing| typing.as_ref().ok()) {
        for &id in &typing.sorts {
            if matches!(ctx.sorts.get(id), ResolvedSort::Generic { .. })
                && let Some(sort) = resolved_sort_to_syntax(ctx, spec, system, id)
            {
                worklist.push(sort);
            }
        }
    }
    // `equation_typing` is a HashMap, so its iteration order (hence the push
    // order above) varies between runs; sort so `group_and_merge` below sees
    // a fixed order and the generated equations end up in the same order
    // every time.
    worklist.sort();

    // The freshly generated content still carries the raw `Binary`/`Unary`/
    // `List` nodes the templates are written with (mirroring what
    // `DataSpecification::from_untyped` does for the syntactically-collected
    // part); already-lowered content passes through unchanged since lowering
    // is idempotent.
    let groups = group_and_merge(sources, &mut result, worklist, &seen, encoding);
    lower_data_expressions(&mut result);
    (result, groups)
}

/// Converts an inferred sort back into the `merc_syntax` sort-expression form
/// the Appendix-B templates are written in — the mirror of
/// `mcrl2_lowering::lower_sort`, but targeting the syntax tree rather than the
/// aterm schema, since [standard_sort] substitutes into syntax-tree templates.
/// Returns `None` for [ResolvedSort::Unit] (never a data sort) or a
/// [ResolvedSort::Def] whose declaration cannot be named (out of range of both
/// `spec` and `system`, which does not happen for a sort that inference
/// actually produced).
fn resolved_sort_to_syntax(
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    id: ResolvedSortId,
) -> Option<SortExpression> {
    match ctx.sorts.get(id) {
        // Never a data sort, like `Unit`: a bound type variable is always
        // instantiated to a fresh unification variable before Phase-3
        // solving produces a node's final ResolvedSortId, so this case does
        // not happen for a sort inference actually produced either.
        ResolvedSort::Var(_) => None,
        ResolvedSort::Unit => None,
        ResolvedSort::Primitive(sort) => Some(SortExpressionKind::Simple(*sort).into()),
        ResolvedSort::Generic { op, subsort } => {
            let sub = resolved_sort_to_syntax(ctx, spec, system, *subsort)?;
            Some(SortExpressionKind::Complex(*op, Box::new(sub)).into())
        }
        ResolvedSort::Function { domain, range } => {
            let domain = domain
                .iter()
                .map(|&sort| resolved_sort_to_syntax(ctx, spec, system, sort))
                .collect::<Option<Vec<_>>>()?;
            let range = resolved_sort_to_syntax(ctx, spec, system, *range)?;
            Some(
                SortExpressionKind::FlattenedFunction {
                    domain,
                    range: Box::new(range),
                }
                .into(),
            )
        }
        ResolvedSort::Def(def) => {
            let name = ctx.sort_name(spec, system, *def)?;
            Some(SortExpressionKind::Resolved(name.to_string(), *def).into())
        }
    }
}

/// Any user `cons`/`map` declaration whose name collides with a system-defined
/// function is rejected, regardless of the user's declared sort.
pub(crate) fn check_no_system_function_redeclaration(
    spec: &UntypedDataSpecification,
    basics: &UntypedDataSpecification,
) -> Result<(), WellTypedError> {
    let mut reserved: HashSet<&str> = HashSet::new();
    reserved.extend(
        basics
            .constructor_declarations
            .iter()
            .map(|decl| decl.identifier.as_str()),
    );
    reserved.extend(basics.map_declarations.iter().map(|decl| decl.identifier.as_str()));
    // The container/function-update operations *and* the comparison operators
    // and `if` are all polymorphic built-ins, so they share one reserved-name
    // source. Kept as its own set (rather than merged into `reserved`): its
    // names are `'static` (drawn from the bundled templates), while
    // `reserved`'s are borrowed from `basics`, and unifying the two into one
    // `HashSet` type would force every borrow in this function to be
    // `'static` too.
    let reserved_polymorphic: HashSet<&'static str> = polymorphic_operator_names().collect();

    for decl in &spec.constructor_declarations {
        if reserved.contains(decl.identifier.as_str()) || reserved_polymorphic.contains(decl.identifier.as_str()) {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }
    for decl in &spec.map_declarations {
        if reserved.contains(decl.identifier.as_str()) || reserved_polymorphic.contains(decl.identifier.as_str()) {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }
    Ok(())
}

/// Collects every container sort — and, when `include_functions`, every
/// single-argument function sort — occurring in the specification into `out`,
/// including the sorts on binders inside the equation expressions.
fn collect_system_sorts_in_spec(
    spec: &UntypedDataSpecification,
    out: &mut Vec<SortExpression>,
    include_functions: bool,
) {
    for declaration in &spec.sort_declarations {
        if let Some(expr) = &declaration.expr {
            collect_system_sorts(expr, out, include_functions);
        }
    }
    for constructor in &spec.constructor_declarations {
        collect_system_sorts(&constructor.sort, out, include_functions);
    }
    for map in &spec.map_declarations {
        collect_system_sorts(&map.sort, out, include_functions);
    }
    for equation in &spec.equation_declarations {
        for variable in &equation.variables {
            collect_system_sorts(&variable.sort, out, include_functions);
        }
        for eqn in &equation.equations {
            if let Some(condition) = &eqn.condition {
                collect_system_sorts_in_expr(condition, out, include_functions);
            }
            collect_system_sorts_in_expr(&eqn.lhs, out, include_functions);
            collect_system_sorts_in_expr(&eqn.rhs, out, include_functions);
        }
    }
}

/// Collects the system-defined sorts mentioned syntactically inside a data
/// expression: the sorts on binders, and around a set/bag comprehension's
/// element sort also `Set(S)` and `Bag(S)` — the comprehension denotes one of
/// the two, which reading applies is only decided by sort inference, so the
/// operators of both are provided. The element sorts of enumeration literals
/// (`{1, 2}`) are not syntactically apparent and are not collected.
///
/// Binder sorts that are not valid variable sorts (see
/// [is_supported_binder_sort]) are skipped: inference rejects the constructs
/// that bind them, so their operators are never looked up.
fn collect_system_sorts_in_expr(expr: &DataExpr, out: &mut Vec<SortExpression>, include_functions: bool) {
    expr.visit::<(), _>(|expr| {
        match &expr.node {
            DataExprKind::SetBagComp { variable, predicate: _ } => {
                if is_supported_binder_sort(&variable.sort) {
                    collect_system_sorts(&variable.sort, out, include_functions);
                    out.push(SortExpressionKind::Complex(ComplexSort::Set, Box::new(variable.sort.clone())).into());
                    out.push(SortExpressionKind::Complex(ComplexSort::Bag, Box::new(variable.sort.clone())).into());
                }
            }
            DataExprKind::Lambda { variables, body: _ }
            | DataExprKind::Quantifier {
                op: _,
                variables,
                body: _,
            } => {
                for variable in variables {
                    if is_supported_binder_sort(&variable.sort) {
                        collect_system_sorts(&variable.sort, out, include_functions);
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// Collects the system-defined sorts in a single sort expression, recursing
/// through element, function, product and structured sorts.
///
/// Container sorts are always collected. Function sorts of any arity are
/// collected only when `include_functions` — see the call in
/// [`build_system_defined_specification`] for why generated specifications are
/// scanned without them. A single-argument domain is converted to the nested
/// `Function` form [`standard_sort`]'s single-argument branch expects; a
/// multi-argument domain is passed through as `FlattenedFunction`, which
/// `standard_sort`'s multi-argument branch consumes directly.
fn collect_system_sorts(sort: &SortExpression, out: &mut Vec<SortExpression>, include_functions: bool) {
    sort.visit::<(), _>(|expr| {
        match &expr.node {
            SortExpressionKind::Complex(_, _) => out.push(expr.clone()),
            // A user specification carries flattened function sorts; the
            // generated Appendix-B specifications carry the un-flattened
            // `Function` form.
            SortExpressionKind::Function { domain, .. } => {
                if include_functions && !matches!(domain.node, SortExpressionKind::Product { .. }) {
                    out.push(expr.clone());
                }
            }
            SortExpressionKind::FlattenedFunction { domain, range } if include_functions => {
                if let [single] = domain.as_slice() {
                    out.push(
                        SortExpressionKind::Function {
                            domain: Box::new(single.clone()),
                            range: range.clone(),
                        }
                        .into(),
                    );
                } else {
                    out.push(expr.clone());
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

#[cfg(test)]
mod tests {
    use merc_syntax::ComplexSort;
    use merc_syntax::SortExpressionKind;
    use merc_syntax::SourceMap;
    use merc_syntax::UntypedDataSpecification;

    use super::build_system_defined_specification;
    use super::collect_system_sorts_in_spec;
    use crate::DataSpecification;
    use crate::NumberEncoding;
    use crate::basic_sort_data_specification;

    /// The distinct container constructors that occur in a specification.
    fn container_ops(spec: &UntypedDataSpecification) -> Vec<ComplexSort> {
        let mut sorts = Vec::new();
        collect_system_sorts_in_spec(spec, &mut sorts, true);
        let mut ops: Vec<ComplexSort> = sorts
            .into_iter()
            .filter_map(|sort| match sort.node {
                SortExpressionKind::Complex(op, _) => Some(op),
                _ => None,
            })
            .collect();
        ops.sort();
        ops.dedup();
        ops
    }

    fn system_spec(text: &str) -> UntypedDataSpecification {
        let mut sources = SourceMap::new();
        let basics = basic_sort_data_specification(&mut sources, NumberEncoding::Binary);
        build_system_defined_specification(
            &mut sources,
            &UntypedDataSpecification::parse(text).unwrap(),
            basics,
            NumberEncoding::Binary,
        )
        .0
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_basic_sorts_are_always_present() {
        let spec = system_spec("map f: Bool;");
        for basic in ["Bool", "Pos", "Nat", "Int", "Real"] {
            assert!(
                spec.sort_declarations.iter().any(|decl| decl.identifier == basic),
                "the basic sort {basic} should always be included"
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_set_pulls_in_finite_set() {
        // A `Set(S)` is defined in terms of `FSet(S)`, so both must be present.
        let ops = container_ops(&system_spec("map f: Set(Nat);"));
        assert!(ops.contains(&ComplexSort::Set));
        assert!(ops.contains(&ComplexSort::FSet));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_comprehension_contributes_set_and_bag() {
        // A comprehension may denote a set or a bag; the equations of both are
        // provided for its element sort even though no declaration mentions a
        // container.
        let spec = UntypedDataSpecification::parse("map b: Bool; eqn b = 1 in { n: Pos | n < 3 };").unwrap();
        let ops = container_ops(&spec);
        for op in [ComplexSort::Set, ComplexSort::Bag] {
            assert!(ops.contains(&op), "a comprehension should contribute {op:?}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_quantifier_binder_sort_is_collected() {
        // The `List(Nat)` mentioned only on the quantifier binder still gets
        // its Appendix-B equations.
        let spec = UntypedDataSpecification::parse("map b: Bool; eqn b = forall l: List(Nat). l == [];").unwrap();
        assert!(container_ops(&spec).contains(&ComplexSort::List));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_bag_pulls_in_all_related_containers() {
        // A `Bag(S)` transitively needs `FBag(S)`, `FSet(S)` and `Set(S)`.
        let ops = container_ops(&system_spec("map f: Bag(Nat);"));
        for op in [ComplexSort::Bag, ComplexSort::FBag, ComplexSort::FSet, ComplexSort::Set] {
            assert!(ops.contains(&op), "using Bag should pull in {op:?}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_nested_container_element_is_included() {
        // `List(Set(Nat))` needs both the list and the (transitive) set defs.
        let ops = container_ops(&system_spec("map f: List(Set(Nat));"));
        assert!(ops.contains(&ComplexSort::List));
        assert!(ops.contains(&ComplexSort::Set));
        assert!(ops.contains(&ComplexSort::FSet));
    }

    /// Whether the system-defined spec of `text` declares the function-update
    /// operators, checked through the full `from_untyped` path (which flattens
    /// function sorts).
    fn has_function_update(text: &str) -> bool {
        let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();
        spec.system_defined_specification()
            .map_declarations
            .iter()
            .any(|map| map.identifier.contains("func_update"))
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_single_argument_function_gets_update_operators() {
        assert!(has_function_update("map f: Nat -> Bool;"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_multi_argument_function_gets_update_operators() {
        // `Nat # Bool -> Nat` has a product domain; `standard_sort` generalizes
        // the Appendix-B template to it instead of deferring it.
        assert!(has_function_update("map f: Nat # Bool -> Nat;"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_function_over_containers_terminates() {
        // Regression: re-scanning generated function-update specs for further
        // function sorts diverged, because `@is_not_an_update: (S -> T) -> Bool`
        // is itself a single-argument function, growing the sort without bound.
        assert!(has_function_update("map f: List(Nat) -> List(Nat);"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_multi_argument_function_over_containers_terminates() {
        // The same regression as above, but seeded from a multi-argument
        // function so the fixpoint also terminates when it re-scans a
        // generated multi-argument `@func_update`/`@is_not_an_update`/
        // `@if_always_else` specification.
        assert!(has_function_update("map f: List(Nat) # Bool -> List(Nat);"));
    }
}
