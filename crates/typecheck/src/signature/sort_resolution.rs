use merc_syntax::ConstructorId;
use merc_syntax::DefId;
use merc_syntax::MapId;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::UntypedDataSpecification;
use merc_syntax::VarId;

use crate::ResolvedSortId;
use crate::TypeCheckContext;

/// Returns the resolved sort of the constructor with the given [ConstructorId].
///
/// Covers the user specification only; the system-defined specification is
/// still unresolved content.
pub(crate) fn query_sort_of_constructor(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    id: ConstructorId,
) -> ResolvedSortId {
    ctx.get_or_compute(
        |ctx| &mut ctx.sort_of_constructor,
        id,
        |ctx| resolve_sort(ctx, spec, &spec.constructor_declarations[id].sort),
    )
    .expect("constructor sort has no cyclic dependency")
}

/// Returns the resolved sort of the map with the given [MapId], memoized on
/// [TypeCheckContext::sort_of_map]. Requires `id` to originate from
/// `assign_declaration_ids` on `spec`.
///
/// Covers the user specification only; the system-defined specification is
/// still unresolved content.
pub(crate) fn query_sort_of_map(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    id: MapId,
) -> ResolvedSortId {
    ctx.get_or_compute(
        |ctx| &mut ctx.sort_of_map,
        id,
        |ctx| resolve_sort(ctx, spec, &spec.map_declarations[id].sort),
    )
    .expect("map sort has no cyclic dependency")
}

/// Returns the resolved sort of the equation `var`-block variable declared by `sort`, identified
/// by its own `var_id`, memoized on [TypeCheckContext::sort_of_equation_var]. Requires `var_id` to
/// originate from `resolve_data_specification_variables` on `spec`.
///
/// Covers the user specification only; the system-defined specification is
/// still unresolved content.
pub(crate) fn query_sort_of_equation_var(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    var_id: VarId,
    sort: &SortExpression,
) -> ResolvedSortId {
    ctx.get_or_compute(
        |ctx| &mut ctx.sort_of_equation_var,
        var_id,
        |ctx| resolve_sort(ctx, spec, sort),
    )
    .expect("equation variable sort has no cyclic dependency")
}

///
/// Requires names resolved and structured sorts desugared; alias indirection
/// need not be expanded, since a `Resolved` sort goes through
/// [query_sort_of_def], which resolves the alias body lazily. Note that
/// flattening does not recurse into a substituted function sort, so a nested
/// higher-order sort still appears as `Function` with a `Product` domain spine;
/// both forms resolve to the same interned function sort.
pub(crate) fn resolve_sort(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    sort: &SortExpression,
) -> ResolvedSortId {
    match &sort.node {
        SortExpressionKind::Simple(sort) => ctx.sorts.primitive(*sort),
        SortExpressionKind::Complex(op, subsort) => {
            let subsort = resolve_sort(ctx, spec, subsort);
            ctx.sorts.generic(*op, subsort)
        }
        SortExpressionKind::FlattenedFunction { domain, range } => {
            let domain = domain.iter().map(|sort| resolve_sort(ctx, spec, sort)).collect();
            let range = resolve_sort(ctx, spec, range);
            ctx.sorts.function(domain, range)
        }
        // Kept so the resolver accepts any well-formed sort expression, such
        // as binder sorts built during inference.
        SortExpressionKind::Function { domain, range } => {
            let mut resolved_domain = Vec::new();
            resolve_function_domain(ctx, spec, domain, &mut resolved_domain);
            let range = resolve_sort(ctx, spec, range);
            ctx.sorts.function(resolved_domain, range)
        }
        SortExpressionKind::Resolved(_, id) => query_sort_of_def(ctx, spec, *id),
        SortExpressionKind::Reference(_) => unreachable!("Names must have been resolved"),
        SortExpressionKind::TypeVar(_) => unreachable!("Names must have been resolved"),
        // Interned as a genuine lattice element, the same `S` is considered
        // identical wherever it appears.
        SortExpressionKind::ResolvedTypeVar(id) => ctx.sorts.var(*id),
        SortExpressionKind::Struct { .. } => unreachable!("Structured sorts must have been desugared"),
        SortExpressionKind::Product { .. } => {
            unreachable!("product sorts outside a function domain were rejected before resolution")
        }
    }
}

/// Resolves the leaves of a `Product` domain spine in declaration order, the
/// resolution counterpart of `flatten_function_domain_rec`.
fn resolve_function_domain(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    sort: &SortExpression,
    domain: &mut Vec<ResolvedSortId>,
) {
    match &sort.node {
        SortExpressionKind::Product { lhs, rhs } => {
            resolve_function_domain(ctx, spec, lhs, domain);
            resolve_function_domain(ctx, spec, rhs, domain);
        }
        _ => domain.push(resolve_sort(ctx, spec, sort)),
    }
}

/// Returns the resolved sort denoted by a sort declaration: the nominal sort
/// for an abstract sort or struct representative (no alias body), or the
/// resolved body for an alias. Memoized on [TypeCheckContext::sort_of_def].
///
/// Requires `def` to originate from name resolution of `spec`, so it indexes
/// `sort_declarations`. Cyclic aliases were rejected by `check_aliases`, so the
/// query cannot re-enter itself, whether alias bodies are normalized or not.
pub(crate) fn query_sort_of_def(
    ctx: &mut TypeCheckContext,
    spec: &UntypedDataSpecification,
    def: DefId,
) -> ResolvedSortId {
    debug_assert!(
        spec.sort_declarations
            .get(*def)
            .is_some_and(|decl| decl.id == Some(def)),
        "DefId {def:?} does not originate from name resolution of this specification"
    );

    ctx.get_or_compute(
        |ctx| &mut ctx.sort_of_def,
        def,
        |ctx| match &spec.sort_declarations[*def].expr {
            None => ctx.sorts.def(def),
            Some(expr) => resolve_sort(ctx, spec, expr),
        },
    )
    .expect("check_aliases rejected cyclic aliases")
}

#[cfg(test)]
mod tests {
    use merc_syntax::ComplexSort;
    use merc_syntax::ConstructorId;
    use merc_syntax::DefId;
    use merc_syntax::MapId;
    use merc_syntax::Sort;
    use merc_syntax::UntypedDataSpecification;

    use crate::DataSpecification;
    use crate::ResolvedSort;
    use crate::ResolvedSortId;
    use crate::TypeCheckContext;
    use crate::query_sort_of_def;

    /// Type checks `text`; the returned specification carries the resolved
    /// declaration sorts and the context that interned them.
    fn typecheck(text: &str) -> DataSpecification {
        DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap()
    }

    /// The resolved sort of the `index`-th map declaration.
    fn mapping(spec: &DataSpecification, index: usize) -> ResolvedSortId {
        spec.sort_of_map(MapId::new(index))
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_basic_sort() {
        let spec = typecheck("map f: Nat;");
        assert_eq!(mapping(&spec, 0), spec.context().sorts.primitive(Sort::Nat));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_alias_inside_container() {
        // `D = Nat` is inlined by normalization, so `List(D)` resolves to `List(Nat)`.
        let spec = typecheck("sort D = Nat; map f: List(D);");
        let sorts = &spec.context().sorts;
        let ResolvedSort::Generic { op, subsort } = sorts.get(mapping(&spec, 0)) else {
            panic!("expected a container sort");
        };
        assert_eq!(*op, ComplexSort::List);
        assert_eq!(*subsort, sorts.primitive(Sort::Nat));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_function_sort() {
        let spec = typecheck("map f: Nat # Bool -> Real;");
        let sorts = &spec.context().sorts;
        let ResolvedSort::Function { domain, range } = sorts.get(mapping(&spec, 0)) else {
            panic!("expected a function sort");
        };
        assert_eq!(*domain, vec![sorts.primitive(Sort::Nat), sorts.primitive(Sort::Bool)]);
        assert_eq!(*range, sorts.primitive(Sort::Real));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_higher_order_function_sort() {
        // Flattening does not recurse into the substituted sort, so the inner
        // `Nat -> Bool` is still an un-flattened `Function`; both forms must
        // resolve to the same interned function sort.
        let spec = typecheck("map f: (Nat -> Bool) -> Bool; g: Nat -> Bool;");
        let sorts = &spec.context().sorts;
        let ResolvedSort::Function { domain, range } = sorts.get(mapping(&spec, 0)) else {
            panic!("expected a function sort");
        };
        assert_eq!(*range, sorts.primitive(Sort::Bool));
        assert_eq!(*domain, vec![mapping(&spec, 1)]);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_struct_sort_is_nominal() {
        // A structured sort resolves to the nominal sort of its declaration,
        // and its desugared constructors target that same sort.
        let spec = typecheck("sort D = struct a | b; map f: D;");
        let def = DefId::new(*spec.sorts().index("D").expect("D should be declared"));
        assert_eq!(*spec.context().sorts.get(mapping(&spec, 0)), ResolvedSort::Def(def));
        assert_eq!(spec.sort_of_constructor(ConstructorId::new(0)), mapping(&spec, 0));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_interned_sorts_are_shared() {
        let spec = typecheck("map f: List(Nat); g: List(Nat);");
        assert_eq!(mapping(&spec, 0), mapping(&spec, 1));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_resolve_equation_variables() {
        let spec = typecheck("map f: Nat -> Bool; var n: Nat; eqn f(n) = true;");
        let var_id = spec.data_specification().equation_declarations[0].variables[0]
            .var_id
            .expect("resolve_data_specification_variables ran");
        assert_eq!(
            spec.sort_of_equation_var(var_id),
            spec.context().sorts.primitive(Sort::Nat)
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_query_sort_of_def_expands_alias_and_memoizes() {
        // A directly-queried alias resolves to its expanded definition; the
        // second query is answered from the cache and yields the same id.
        let spec = typecheck("sort D = List(Nat); map f: D;");
        let def = DefId::new(*spec.sorts().index("D").expect("D should be declared"));

        let mut ctx = TypeCheckContext::new();
        let first = query_sort_of_def(&mut ctx, spec.data_specification(), def);
        assert_eq!(first, mapping(&spec, 0));
        assert_eq!(ctx.sort_of_def.get(&def), Some(&first));
        assert_eq!(query_sort_of_def(&mut ctx, spec.data_specification(), def), first);
    }
}
