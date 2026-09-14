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
use crate::comparison_operator_equations_with_provenance;
use crate::is_supported_binder_sort;
use crate::lower_data_expressions;
use crate::polymorphic_operator_names;
use crate::standard_sort;
use crate::standard_sort_with_provenance;

/// Which template (bundled or generic, named the same way
/// `ctx.template_typings` keys it — see `standard_sort_with_provenance`/
/// `comparison_operator_equations_with_provenance`) produced one contiguous
/// range of `system.equation_declarations` (`EqnSpecId` block indices), and
/// the concrete sort(s) substituted for that template's own `type_var`
/// declaration(s), in declaration order.
///
/// Used to specialize each generated equation's typing from the template's
/// own proven, rigid typing (`ctx.template_typings`) by substitution, instead
/// of re-checking it: two instantiations of the same container template
/// (`Bag(Nat)`, `Bag(D)`) each carry a copy of its equations, checked once as
/// the template's own — see `docs/typecheck.md`.
pub(crate) struct TemplateInstantiation {
    pub(crate) template: String,
    pub(crate) substitution: Vec<SortExpression>,
    pub(crate) equation_range: Range<usize>,
}

/// Which sort-expression nodes [collect_system_sorts]/[collect_system_sorts_in_expr]/
/// [collect_system_sorts_in_spec] collect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortCollectionMode {
    /// Container sorts only — used to re-scan already-generated container
    /// content, where a growing function sort like
    /// `@is_not_an_update: (S -> T) -> Bool` would otherwise diverge (see
    /// [collect_system_sorts]'s doc comment).
    ContainersOnly,
    /// Container sorts and single-/multi-argument function sorts — used to
    /// seed the container worklist from the user's own specification.
    ContainersAndFunctions,
    /// Every sort — used to seed and re-scan the comparison-operator
    /// worklist: `==`/`<`/`if` apply uniformly to any sort, not just
    /// containers and functions.
    Every,
}

/// Drains `worklist` to a fixpoint like [expand_sorts], merging each popped
/// sort's generated batch into `result` directly via `generate`. Unlike an
/// earlier version of this function, batches are no longer partitioned by
/// element sort before merging: the container/function-update/comparison
/// operations are looked up as schemes in one pooled signature regardless of
/// which concrete instantiation an equation came from (see
/// `docs/typecheck.md`), so there is nothing left for two instantiations'
/// equations to collide over. Records a [TemplateInstantiation] for each
/// batch, so its equations can be specialized from the template's own proven
/// typing rather than re-checked.
fn merge_generated(
    sources: &mut SourceMap,
    result: &mut UntypedDataSpecification,
    mut worklist: Vec<SortExpression>,
    seen: &HashSet<SortExpression>,
    scan_mode: SortCollectionMode,
    mut generate: impl FnMut(&mut SourceMap, &SortExpression) -> (UntypedDataSpecification, (String, Vec<SortExpression>)),
) -> Vec<TemplateInstantiation> {
    let mut seen = seen.clone();
    let mut instantiations = Vec::new();
    while let Some(sort) = worklist.pop() {
        if !seen.insert(sort.clone()) {
            continue;
        }
        let (mut generated, (template, substitution)) = generate(sources, &sort);
        collect_system_sorts_in_spec(&generated, &mut worklist, scan_mode);
        lower_data_expressions(&mut generated);

        let start = result.equation_declarations.len();
        result.merge(&generated);
        let end = result.equation_declarations.len();

        instantiations.push(TemplateInstantiation {
            template,
            substitution,
            equation_range: start..end,
        });
    }
    instantiations
}

/// Builds the system-defined part of a specification: the Appendix-B
/// definitions (constructors, mappings and equations) for every basic sort,
/// container sort and single-argument function sort that occurs in `spec`,
/// plus the reflexive/derived comparison-operator equations (`==`, `<`, `if`,
/// …) for *every* sort occurring in `spec`.
///
/// The five basic sorts are always included. A container sort pulls in the
/// containers it is defined in terms of — a `Set(S)` needs `FSet(S)`, a
/// `Bag(S)` needs `FBag(S)`, `FSet(S)` and `Set(S)` — which the fixpoint below
/// discovers by re-scanning each generated specification.
///
/// A function sort `D_0 # ... # D_{n-1} -> T` contributes the function-update
/// operators for its declared arity — the bundled single-argument template when
/// `n == 1`, otherwise [standard_sort] generalizes it to the flattened domain.
/// Structured-sort equations are generated separately from the desugared
/// declarations and merged in by `DataSpecification::from_untyped`.
///
/// The comparison-operator pass runs independently, over its own worklist and
/// `seen` set: unlike containers/functions, `==`/`<`/`if` apply uniformly to
/// any sort, so a sort can legitimately need both a container instantiation
/// and a comparison instantiation, and the two passes must not block each
/// other.
///
/// The result is deliberately left unresolved: it uses the built-in `Simple`
/// sorts and the Appendix-B operator names, and is not re-checked against the
/// user-oriented well-typedness rules.
///
/// `basics` is the
/// [`basic_sort_data_specification`](crate::basic_sort_data_specification),
/// passed in because the caller also needs it separately for the system
/// signature.
///
/// Returns the merged specification alongside the [TemplateInstantiation]s its
/// content was generated in.
pub(crate) fn build_system_defined_specification(
    sources: &mut SourceMap,
    spec: &UntypedDataSpecification,
    basics: UntypedDataSpecification,
    encoding: NumberEncoding,
) -> (UntypedDataSpecification, Vec<TemplateInstantiation>) {
    let mut result = basics;

    let mut container_worklist = Vec::new();
    // Seed from the user specification, including its function sorts.
    collect_system_sorts_in_spec(
        spec,
        &mut container_worklist,
        SortCollectionMode::ContainersAndFunctions,
    );
    let mut instantiations = merge_generated(
        sources,
        &mut result,
        container_worklist,
        &HashSet::new(),
        SortCollectionMode::ContainersOnly,
        |sources, sort| standard_sort_with_provenance(sources, sort, encoding),
    );

    let mut comparison_worklist = Vec::new();
    collect_system_sorts_in_spec(spec, &mut comparison_worklist, SortCollectionMode::Every);
    instantiations.extend(merge_generated(
        sources,
        &mut result,
        comparison_worklist,
        &HashSet::new(),
        SortCollectionMode::Every,
        comparison_operator_equations_with_provenance,
    ));

    (result, instantiations)
}

/// Drains `worklist` to a fixpoint: for every sort popped that has not already
/// been `seen`, generates its Appendix-B specification via `generate` and
/// passes it to `on_generated`, then re-scans the generated content (in
/// `scan_mode`) for further sorts it in turn depends on (a container is
/// defined in terms of other containers, e.g. `Set(S)` needs `FSet(S)`) and
/// pushes those too.
///
/// `scan_mode` should be [SortCollectionMode::ContainersOnly] when `generate`
/// produces container content: function sorts are not re-collected from
/// generated container content (only from the initial `worklist`), since the
/// function-update operators introduce ever-larger function sorts
/// (`@is_not_an_update: (S -> T) -> Bool`), which would not terminate here.
/// Comparison-operator content has no such concern — a generated
/// instantiation only ever mentions the sort itself and `Bool` — so
/// [SortCollectionMode::Every] is safe there.
fn expand_sorts(
    sources: &mut SourceMap,
    mut worklist: Vec<SortExpression>,
    seen: &mut HashSet<SortExpression>,
    scan_mode: SortCollectionMode,
    mut generate: impl FnMut(&mut SourceMap, &SortExpression) -> UntypedDataSpecification,
    mut on_generated: impl FnMut(&UntypedDataSpecification),
) {
    while let Some(sort) = worklist.pop() {
        if !seen.insert(sort.clone()) {
            continue;
        }

        let generated = generate(sources, &sort);
        collect_system_sorts_in_spec(&generated, &mut worklist, scan_mode);
        on_generated(&generated);
    }
}

/// Extends `system` with the Appendix-B declarations of every container sort
/// and comparison-operator instantiation that is discovered only through
/// Phase-3 inference rather than appearing in the textual declarations: the
/// element sort of a `List`/`Set`/`Bag` enumeration literal (`[1, 2]`,
/// `{1, 2}`, `{1: 2}`), or of a bare numeral, is not written down anywhere —
/// it is entirely a product of its elements' inferred sorts (see
/// [collect_system_sorts_in_expr]'s doc comment) — so
/// [build_system_defined_specification]'s syntactic scan misses it whenever
/// the same sort does not also occur, spelled out, elsewhere in the
/// specification.
///
/// `ctx` must be the context [crate::check_equations] populated: every sort
/// reachable from a successfully typed equation's per-node sorts is a
/// candidate. Which of those `system` already covers is not recorded anywhere
/// (containers are structural, not named, so `system` carries no direct list
/// of them), so the syntactic scan is replayed here to reconstruct that set
/// before diffing against it — once for containers/functions, once
/// independently for comparisons, mirroring
/// [build_system_defined_specification]'s own two independent passes.
///
/// Returns a new specification plus the [TemplateInstantiation]s of the newly
/// added content; `system` itself is left untouched, so calling this repeatedly (as
/// [crate::DataSpecification::lower_data_specification] may be) keeps
/// producing the same result from the same inputs.
pub(crate) fn extend_system_with_inferred_sorts(
    sources: &mut SourceMap,
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    encoding: NumberEncoding,
) -> (UntypedDataSpecification, Vec<TemplateInstantiation>) {
    let mut result = system.clone();

    // Reconstruct the set of container sorts `system` already covers.
    let mut container_seen: HashSet<SortExpression> = HashSet::new();
    let mut container_covered = Vec::new();
    collect_system_sorts_in_spec(spec, &mut container_covered, SortCollectionMode::ContainersAndFunctions);
    expand_sorts(
        sources,
        container_covered,
        &mut container_seen,
        SortCollectionMode::ContainersOnly,
        |sources, sort| standard_sort(sources, sort, encoding),
        |_| {},
    );

    // Every container sort that shows up as the inferred sort of some
    // expression node in a well-typed equation, not already covered above.
    let mut container_worklist = Vec::new();
    for typing in ctx.equation_typing.values().filter_map(|typing| typing.as_ref().ok()) {
        for &id in &typing.sorts {
            if matches!(ctx.sorts.get(id), ResolvedSort::Generic { .. })
                && let Some(sort) = resolved_sort_to_syntax(ctx, spec, id)
            {
                container_worklist.push(sort);
            }
        }
    }
    // `equation_typing` is a HashMap, so its iteration order (hence the push
    // order above) varies between runs; sort so `merge_generated` below sees
    // a fixed order and the generated equations end up in the same order
    // every time.
    container_worklist.sort();

    let mut instantiations = merge_generated(
        sources,
        &mut result,
        container_worklist,
        &container_seen,
        SortCollectionMode::ContainersOnly,
        |sources, sort| standard_sort_with_provenance(sources, sort, encoding),
    );

    // The comparison-operator counterpart: reconstruct the set of sorts
    // `system` already covers for comparisons (every sort, not just
    // containers/functions)...
    let mut comparison_seen: HashSet<SortExpression> = HashSet::new();
    let mut comparison_covered = Vec::new();
    collect_system_sorts_in_spec(spec, &mut comparison_covered, SortCollectionMode::Every);
    expand_sorts(
        sources,
        comparison_covered,
        &mut comparison_seen,
        SortCollectionMode::Every,
        |sources, sort| comparison_operator_equations_with_provenance(sources, sort).0,
        |_| {},
    );

    // ...then every sort that shows up as the inferred sort of some
    // expression node in a well-typed equation — `resolved_sort_to_syntax`
    // already returns `None` for the two `ResolvedSort` variants that never
    // denote a comparable data sort (`Var`, `Unit`), so no extra filter is
    // needed here beyond that.
    let mut comparison_worklist = Vec::new();
    for typing in ctx.equation_typing.values().filter_map(|typing| typing.as_ref().ok()) {
        for &id in &typing.sorts {
            if let Some(sort) = resolved_sort_to_syntax(ctx, spec, id) {
                comparison_worklist.push(sort);
            }
        }
    }
    comparison_worklist.sort();

    instantiations.extend(merge_generated(
        sources,
        &mut result,
        comparison_worklist,
        &comparison_seen,
        SortCollectionMode::Every,
        comparison_operator_equations_with_provenance,
    ));

    // The freshly generated content still carries the raw `Binary`/`Unary`/
    // `List` nodes the templates are written with (mirroring what
    // `DataSpecification::from_untyped` does for the syntactically-collected
    // part); already-lowered content passes through unchanged since lowering
    // is idempotent.
    lower_data_expressions(&mut result);
    (result, instantiations)
}

/// Converts an inferred sort back into the `merc_syntax` sort-expression form
/// the Appendix-B templates are written in — the mirror of
/// `mcrl2_lowering::lower_sort`, but targeting the syntax tree rather than the
/// aterm schema, since [standard_sort] substitutes into syntax-tree templates.
/// Returns `None` for [ResolvedSort::Unit] (never a data sort) or a
/// [ResolvedSort::Def] whose declaration cannot be named (out of range of
/// `spec`, which does not happen for a sort that inference actually produced).
fn resolved_sort_to_syntax(
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
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
            let sub = resolved_sort_to_syntax(ctx, spec, *subsort)?;
            Some(SortExpressionKind::Complex(*op, Box::new(sub)).into())
        }
        ResolvedSort::Function { domain, range } => {
            let domain = domain
                .iter()
                .map(|&sort| resolved_sort_to_syntax(ctx, spec, sort))
                .collect::<Option<Vec<_>>>()?;
            let range = resolved_sort_to_syntax(ctx, spec, *range)?;
            Some(
                SortExpressionKind::FlattenedFunction {
                    domain,
                    range: Box::new(range),
                }
                .into(),
            )
        }
        ResolvedSort::Def(def) => {
            let name = ctx.sort_name(spec, *def)?;
            Some(SortExpressionKind::Resolved(name.to_string(), *def).into())
        }
    }
}

/// Any user `cons`/`map` declaration whose name collides with a system-defined
/// function is rejected, regardless of the user's declared sort — as is any
/// declaration under an `@`-prefixed name outright, the reserved-name
/// convention every system-generated symbol uses (`@c0`, `@cPair`, `@zero_`,
/// …), whether or not it happens to collide with one that exists today; only
/// a *trusted* declaration (Appendix B's own) may use one — see
/// `docs/typecheck.md`'s trusted-signature milestone.
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
    // source.
    let reserved_polymorphic: HashSet<&'static str> = polymorphic_operator_names().collect();

    for decl in &spec.constructor_declarations {
        if reserved.contains(decl.identifier.as_str())
            || reserved_polymorphic.contains(decl.identifier.as_str())
            || decl.identifier.starts_with('@')
        {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }
    for decl in &spec.map_declarations {
        if reserved.contains(decl.identifier.as_str())
            || reserved_polymorphic.contains(decl.identifier.as_str())
            || decl.identifier.starts_with('@')
        {
            return Err(WellTypedError::SystemFunctionRedeclared {
                name: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }
    }
    Ok(())
}

/// Collects every container sort — every simple/resolved (basic or
/// user-declared) sort too, in [SortCollectionMode::Every] — and, unless
/// [SortCollectionMode::ContainersOnly], every single-argument function sort,
/// occurring in the specification into `out`, including the sorts on binders
/// inside the equation expressions.
fn collect_system_sorts_in_spec(
    spec: &UntypedDataSpecification,
    out: &mut Vec<SortExpression>,
    mode: SortCollectionMode,
) {
    for declaration in &spec.sort_declarations {
        if let Some(expr) = &declaration.expr {
            collect_system_sorts(expr, out, mode);
        }
    }
    for constructor in &spec.constructor_declarations {
        collect_system_sorts(&constructor.sort, out, mode);
    }
    for map in &spec.map_declarations {
        collect_system_sorts(&map.sort, out, mode);
    }
    for equation in &spec.equation_declarations {
        for variable in &equation.variables {
            collect_system_sorts(&variable.sort, out, mode);
        }
        for eqn in &equation.equations {
            if let Some(condition) = &eqn.condition {
                collect_system_sorts_in_expr(condition, out, mode);
            }
            collect_system_sorts_in_expr(&eqn.lhs, out, mode);
            collect_system_sorts_in_expr(&eqn.rhs, out, mode);
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
fn collect_system_sorts_in_expr(expr: &DataExpr, out: &mut Vec<SortExpression>, mode: SortCollectionMode) {
    expr.visit::<(), _>(|expr| {
        match &expr.node {
            DataExprKind::SetBagComp { variable, predicate: _ } => {
                if is_supported_binder_sort(&variable.sort) {
                    collect_system_sorts(&variable.sort, out, mode);
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
                        collect_system_sorts(&variable.sort, out, mode);
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
/// collected unless [SortCollectionMode::ContainersOnly] — see the call in
/// [`build_system_defined_specification`] for why generated container content
/// is re-scanned without them. A single-argument domain is converted to the
/// nested `Function` form [`standard_sort`]'s single-argument branch expects;
/// a multi-argument domain is passed through as `FlattenedFunction`, which
/// `standard_sort`'s multi-argument branch consumes directly. In
/// [SortCollectionMode::Every], every `Simple`/`Resolved` leaf sort is
/// collected too — `sort.visit` already recurses into every child regardless
/// of whether the current node was pushed, so a compound sort like
/// `List(Nat)` yields both itself and `Nat` with no extra recursion needed
/// here.
fn collect_system_sorts(sort: &SortExpression, out: &mut Vec<SortExpression>, mode: SortCollectionMode) {
    sort.visit::<(), _>(|expr| {
        match &expr.node {
            SortExpressionKind::Complex(_, _) => out.push(expr.clone()),
            // A user specification carries flattened function sorts; the
            // generated Appendix-B specifications carry the un-flattened
            // `Function` form.
            SortExpressionKind::Function { domain, .. } => {
                if mode != SortCollectionMode::ContainersOnly
                    && !matches!(domain.node, SortExpressionKind::Product { .. })
                {
                    out.push(expr.clone());
                }
            }
            SortExpressionKind::FlattenedFunction { domain, range } if mode != SortCollectionMode::ContainersOnly => {
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
            SortExpressionKind::Simple(_) | SortExpressionKind::Resolved(_, _) if mode == SortCollectionMode::Every => {
                out.push(expr.clone());
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

    use super::SortCollectionMode;
    use super::build_system_defined_specification;
    use super::collect_system_sorts_in_spec;
    use crate::DataSpecification;
    use crate::NumberEncoding;
    use crate::basic_sort_data_specification;

    /// The distinct container constructors that occur in a specification.
    fn container_ops(spec: &UntypedDataSpecification) -> Vec<ComplexSort> {
        let mut sorts = Vec::new();
        collect_system_sorts_in_spec(spec, &mut sorts, SortCollectionMode::ContainersAndFunctions);
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

    /// Whether the *lowered* spec of `text` declares the function-update
    /// operators, checked through the full `from_untyped`/`lower_data_specification`
    /// path (which flattens function sorts and, since the monomorphization-to-
    /// lowering milestone, is also where a container/function-update
    /// instantiation is generated at all — `system_defined_specification()`
    /// itself no longer carries one, see `docs/typecheck.md`).
    fn has_function_update(text: &str) -> bool {
        let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();
        spec.lower_data_specification()
            .mappings()
            .iter()
            .any(|map| map.name().value().contains("func_update"))
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

    /// Whether `spec` includes the generic `if(true, x, y) = x;` reduction
    /// instantiated for `sort_name` — a `var`/`eqn` block whose declared
    /// variable has sort `sort_name` and whose equations reduce a generic
    /// `if`.
    fn spec_has_comparison_equations_for(spec: &UntypedDataSpecification, sort_name: &str) -> bool {
        spec.equation_declarations.iter().any(|eqn_spec| {
            eqn_spec.variables.iter().any(|var| var.sort.to_string() == sort_name)
                && eqn_spec.equations.iter().any(|eqn| eqn.lhs.to_string().contains("if("))
        })
    }

    /// As [spec_has_comparison_equations_for], but over the *lowered* spec,
    /// checked through the full `from_untyped`/`lower_data_specification` path
    /// (which flattens function sorts, desugars structs, drives Phase-3
    /// inference, and — since the monomorphization-to-lowering milestone — is
    /// also where a comparison-operator instantiation is generated at all).
    ///
    /// Compares by aterm equality rather than `Display`: `SortCons`'s own
    /// `Display` renders only the element sort (`"Nat"`, not `"List(Nat)"`) —
    /// a binary-aterm-format quirk unrelated to this milestone, since the
    /// container *kind* is a separate structural tag there, not part of a
    /// name — so a `sort_name` like `"List(Nat)"` is instead parsed and
    /// lowered through the same [`crate::lower_syntax_sort`] every other
    /// declaration sort goes through, and compared against that.
    fn has_comparison_equations_for(text: &str, sort_name: &str) -> bool {
        let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();
        let lowered = spec.lower_data_specification();

        let sort_spec = UntypedDataSpecification::parse(&format!("map q: {sort_name};")).unwrap();
        let expected_sort = crate::lower_syntax_sort(&sort_spec.map_declarations[0].sort);

        lowered.equations().iter().any(|eqn| {
            eqn.variables()
                .into_iter()
                .any(|var| var.sort().protect() == expected_sort)
                && eqn.lhs().to_string().contains("if(")
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_container_sort_gets_comparison_equations_direct() {
        // As `test_container_sort_gets_comparison_equations`, but exercises
        // only the worklist/generation layer directly
        // (`build_system_defined_specification`), independent of the rest of
        // the type-checking pipeline.
        assert!(spec_has_comparison_equations_for(
            &system_spec("map f: List(Nat);"),
            "List(Nat)"
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_unused_basic_sort_has_no_comparison_equations_direct() {
        // As `test_unused_basic_sort_has_no_comparison_equations`, checked
        // directly against `build_system_defined_specification`'s output.
        assert!(!spec_has_comparison_equations_for(&system_spec("map f: Bool;"), "Real"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_container_sort_gets_comparison_equations() {
        // Closes a real gap: `if(b, xs, ys)` for `List(Nat)` used to
        // type-check (the polymorphic scheme accepts any sort) but had no
        // equation to rewrite it with, since `list.mcrl2` defines its own
        // structural `==`/`<` but never a generic `if`.
        assert!(has_comparison_equations_for("map f: List(Nat);", "List(Nat)"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_struct_sort_gets_comparison_equations() {
        // A `struct` gets its own componentwise `==`/`<`/`<=` from
        // `structured_sort_equations`, but never `if` — that still has to
        // come from the generic scheme.
        assert!(has_comparison_equations_for("sort D = struct c1 | c2; map f: D;", "D"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_unused_basic_sort_has_no_comparison_equations() {
        // The comparison-operator instantiation is lazy, like a container's:
        // a basic sort that never occurs in the specification gets no
        // comparison equations, even though its own sort/arithmetic
        // declarations are still unconditionally present.
        assert!(!has_comparison_equations_for("map f: Bool;", "Real"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_sort_inferred_only_from_a_literal_gets_comparison_equations() {
        // The inference-driven pass (`extend_system_with_inferred_sorts`)
        // must catch a sort that is never spelled out anywhere in the
        // specification's own text — the comparison-operator counterpart of
        // the enumeration-literal container gap.
        assert!(has_comparison_equations_for("map f: Bool; eqn f = (1 == 1);", "Pos"));
    }
}
