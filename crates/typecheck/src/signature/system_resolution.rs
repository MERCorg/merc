use std::collections::HashMap;
use std::sync::Arc;

use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::DefId;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;

use crate::BUILTIN_SCHEME_TEMPLATE;
use crate::CONTAINER_TEMPLATES;
use crate::PolySortScheme;
use crate::ResolvedSortId;
use crate::Signature;
use crate::SystemEquationGroup;
use crate::TypeCheckContext;
use crate::WellTypedError;
use crate::is_basic_sort_name;
use crate::push_overload;
use crate::query_sort_of_def;
use crate::resolve_sort;

/// Resolves the constructor and mapping declarations of the *basic-sort* part
/// of the system-defined specification onto the interned sort lattice.
///
/// `system` must be the *basic-sort* specification ([`basic_sort_data_specification`](crate::basic_sort_data_specification)),
/// not the full system-defined specification `build_system_defined_specification`
/// produces: the container operations are looked up polymorphically instead
/// (`POLYMORPHIC_SIGNATURE`), because resolving their per-sort instantiations
/// here as well would misreport ambiguity (a name would have both a concrete
/// and a polymorphic candidate for the same sort).
///
/// Unlike `build_signature` this runs no well-typedness checks here — not
/// because the system specification is trusted, but because `build_signature`'s
/// checks would misfire on it: it legitimately declares things a user cannot,
/// such as constructors for the basic sorts (`@c0: Nat`). The system
/// specification's own well-formedness is instead verified separately and
/// extensively by `check_system_specification`, unconditionally.
pub(crate) fn resolve_system_signature(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
) -> Result<(), WellTypedError> {
    let sort_ids = build_system_sort_ids(ctx, user_spec, system);

    let mut signature = Signature::default();

    for decl in &system.constructor_declarations {
        let id = resolve_system_sort(ctx, user_spec, &sort_ids, &decl.sort)?;
        push_overload(
            signature.constructors.entry(decl.identifier.node.clone()).or_default(),
            id,
        );
        ctx.system_symbol_spans
            .insert((decl.identifier.node.clone(), id), decl.identifier.span.clone());
    }
    for decl in &system.map_declarations {
        let id = resolve_system_sort(ctx, user_spec, &sort_ids, &decl.sort)?;
        push_overload(signature.mappings.entry(decl.identifier.node.clone()).or_default(), id);
        ctx.system_symbol_spans
            .insert((decl.identifier.node.clone(), id), decl.identifier.span.clone());
    }

    ctx.system_signature = Some(Arc::new(signature));
    Ok(())
}

/// Resolves the system-defined specification's declarations onto the interned
/// sort lattice, group by group (see [SystemEquationGroup]), populating
/// `ctx.system_equation_signature_by_group`.
///
/// Also eagerly resolves every equation- and binder-variable sort and persists
/// `ctx.system_sort_ids`, so the per-equation Phase-3 pass can treat sort
/// resolution there as infallible rather than thread a second fallible path.
pub(crate) fn resolve_system_signature_full(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    groups: &[SystemEquationGroup],
) -> Result<(), WellTypedError> {
    let sort_ids = build_system_sort_ids(ctx, user_spec, system);

    for eqn_spec in &system.equation_declarations {
        for variable in &eqn_spec.variables {
            resolve_system_sort(ctx, user_spec, &sort_ids, &variable.sort)?;
        }
        for equation in &eqn_spec.equations {
            if let Some(condition) = &equation.condition {
                validate_system_binder_sorts(ctx, user_spec, &sort_ids, condition)?;
            }
            validate_system_binder_sorts(ctx, user_spec, &sort_ids, &equation.lhs)?;
            validate_system_binder_sorts(ctx, user_spec, &sort_ids, &equation.rhs)?;
        }
    }

    // An ungrouped equation is a basic-sort template's own, never at risk of the
    // cross-instantiation collision and never referencing a user declaration, so
    // the basic-sort signature alone suffices.
    let basics = ctx
        .system_signature
        .as_deref()
        .expect("resolve_system_signature ran earlier");
    let ambient = Arc::new(Signature {
        constructors: basics.constructors.clone(),
        mappings: basics.mappings.clone(),
        schemes: basics.schemes.clone(),
    });

    let mut by_group = vec![Arc::clone(&ambient); system.equation_declarations.len()];
    for group in groups {
        let mut signature = Signature::default();
        for decl in &group.declarations.constructor_declarations {
            let id = resolve_system_sort(ctx, user_spec, &sort_ids, &decl.sort)?;
            push_overload(
                signature.constructors.entry(decl.identifier.node.clone()).or_default(),
                id,
            );
            ctx.system_symbol_spans
                .insert((decl.identifier.node.clone(), id), decl.identifier.span.clone());
        }
        for decl in &group.declarations.map_declarations {
            let id = resolve_system_sort(ctx, user_spec, &sort_ids, &decl.sort)?;
            push_overload(signature.mappings.entry(decl.identifier.node.clone()).or_default(), id);
            ctx.system_symbol_spans
                .insert((decl.identifier.node.clone(), id), decl.identifier.span.clone());
        }
        let group_signature = Arc::new(merge_signatures(&signature, &ambient));
        for slot in &mut by_group[group.equation_range.clone()] {
            *slot = Arc::clone(&group_signature);
        }
    }

    ctx.system_equation_signature_by_group = by_group;
    ctx.system_sort_ids = Some(Arc::new(sort_ids));
    Ok(())
}

/// Builds the system-internal sort name table; the re-declared basic sorts
/// (`sort Bool;`) already resolve as primitives and are skipped.
///
/// Each entry gets a fresh `DefId` continuing the user sorts' numbering,
/// `user_spec.sort_declarations.len() + decl_index` — the layout
/// `TypeCheckContext::sort_name` relies on to recover the name again.
fn build_system_sort_ids(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
) -> HashMap<String, ResolvedSortId> {
    let mut sort_ids: HashMap<String, ResolvedSortId> = HashMap::new();
    for (decl_index, decl) in system.sort_declarations.iter().enumerate() {
        if is_basic_sort_name(&decl.identifier) || sort_ids.contains_key(&decl.identifier) {
            continue;
        }
        debug_assert!(
            decl.expr.is_none(),
            "system-defined sorts are nominal, but '{}' has a body",
            decl.identifier
        );

        let def = DefId::new(user_spec.sort_declarations.len() + decl_index);
        sort_ids.insert(decl.identifier.clone(), ctx.sorts.def(def));
    }
    sort_ids
}

/// Recursively resolves the sort declared on every binder inside `expr`.
/// `expr` is assumed already lowered by `lower_data_expressions`.
fn validate_system_binder_sorts(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    sort_ids: &HashMap<String, ResolvedSortId>,
    expr: &DataExpr,
) -> Result<(), WellTypedError> {
    match &expr.node {
        // `Resolved` never occurs in the system-defined specification's own equations, but is
        // grouped with `Id` for exhaustiveness.
        DataExprKind::Id(_)
        | DataExprKind::Resolved(_, _)
        | DataExprKind::Number(_)
        | DataExprKind::Bool(_)
        | DataExprKind::EmptyList
        | DataExprKind::EmptySet
        | DataExprKind::EmptyBag => Ok(()),
        DataExprKind::Application { function, arguments } => {
            validate_system_binder_sorts(ctx, user_spec, sort_ids, function)?;
            for argument in arguments {
                validate_system_binder_sorts(ctx, user_spec, sort_ids, argument)?;
            }
            Ok(())
        }
        DataExprKind::Set(members) => {
            for member in members {
                validate_system_binder_sorts(ctx, user_spec, sort_ids, member)?;
            }
            Ok(())
        }
        DataExprKind::Bag(members) => {
            for member in members {
                validate_system_binder_sorts(ctx, user_spec, sort_ids, &member.expr)?;
                validate_system_binder_sorts(ctx, user_spec, sort_ids, &member.multiplicity)?;
            }
            Ok(())
        }
        DataExprKind::SetBagComp { variable, predicate } => {
            resolve_system_sort(ctx, user_spec, sort_ids, &variable.sort)?;
            validate_system_binder_sorts(ctx, user_spec, sort_ids, predicate)
        }
        DataExprKind::Lambda { variables, body } | DataExprKind::Quantifier { op: _, variables, body } => {
            for variable in variables {
                resolve_system_sort(ctx, user_spec, sort_ids, &variable.sort)?;
            }
            validate_system_binder_sorts(ctx, user_spec, sort_ids, body)
        }
        DataExprKind::Whr { expr, assignments } => {
            for assignment in assignments {
                validate_system_binder_sorts(ctx, user_spec, sort_ids, &assignment.expr)?;
            }
            validate_system_binder_sorts(ctx, user_spec, sort_ids, expr)
        }
        DataExprKind::List(_)
        | DataExprKind::Unary { .. }
        | DataExprKind::Binary { .. }
        | DataExprKind::FunctionUpdate { .. } => {
            unreachable!("lower_data_expressions already rewrote this expression form before this pass runs")
        }
    }
}

/// The subset of `signature` naming exactly `constructor_names` and
/// `mapping_names`, used to scope a struct's equations to its own symbols.
///
/// Two separate name sets, not one checked against both maps: a struct's
/// constructor can share a name with an unrelated struct's projection (`a` as
/// both a constant and a projection in `struct.mcrl2`), and only the
/// constructor/mapping distinction tells the two apart.
pub(crate) fn filter_signature(
    signature: &Signature,
    constructor_names: &std::collections::HashSet<String>,
    mapping_names: &std::collections::HashSet<String>,
) -> Signature {
    let mut filtered = Signature::default();
    for name in constructor_names {
        if let Some(overloads) = signature.constructors.get(name) {
            filtered.constructors.insert(name.clone(), overloads.clone());
        }
    }
    for name in mapping_names {
        if let Some(overloads) = signature.mappings.get(name) {
            filtered.mappings.insert(name.clone(), overloads.clone());
        }
    }
    filtered
}

/// The union of `a` and `b`'s overload sets, per name — ground overloads
/// deduplicated by id, scheme overloads simply concatenated (two schemes
/// never denote the same overload the way a ground redeclaration can).
pub(crate) fn merge_signatures(a: &Signature, b: &Signature) -> Signature {
    let mut merged = Signature {
        constructors: a.constructors.clone(),
        mappings: a.mappings.clone(),
        schemes: a.schemes.clone(),
    };
    for (name, overloads) in &b.constructors {
        let entry = merged.constructors.entry(name.clone()).or_default();
        for &id in overloads {
            push_overload(entry, id);
        }
    }
    for (name, overloads) in &b.mappings {
        let entry = merged.mappings.entry(name.clone()).or_default();
        for &id in overloads {
            push_overload(entry, id);
        }
    }
    for (name, schemes) in &b.schemes {
        merged
            .schemes
            .entry(name.clone())
            .or_default()
            .extend(schemes.iter().cloned());
    }
    merged
}

/// Builds one [PolySortScheme] per constructor/mapping declaration of each
/// `template` in `templates`, keyed by name, via [`resolve_sort`] against the
/// template's own (self-contained) spec — legal because every occurrence of
/// the template's own `type_var` block interns to the same [ResolvedSort::Var],
/// on the same footing as any other lattice element.
///
/// Safe to call with any of [CONTAINER_TEMPLATES]/[BUILTIN_SCHEME_TEMPLATE]:
/// none of them contains a `Resolved(_, DefId)` node or a nominal `sort X;`
/// declaration (only `type_var`, primitive, container and function sorts), so
/// there is no `DefId` to resolve and hence no risk of it being looked up
/// against the wrong spec's `sort_declarations`.
///
/// This is the one shared mechanism behind both `ctx.signature`'s `schemes`
/// (containers, function-update and the comparison/`if` builtins, for the
/// user-facing lookup — see `build_signature`) and
/// [`build_builtin_scheme_signature`]'s narrower table (the comparison/`if`
/// builtins only, for a system equation's own lookup).
pub(crate) fn build_polymorphic_schemes<'a>(
    ctx: &mut TypeCheckContext,
    templates: impl IntoIterator<Item = &'a UntypedDataSpecification>,
) -> HashMap<String, Vec<PolySortScheme>> {
    let mut schemes: HashMap<String, Vec<PolySortScheme>> = HashMap::new();
    for template in templates {
        let vars: Vec<TypeVarId> = template
            .type_var_declarations
            .iter()
            .filter_map(|decl| decl.id)
            .collect();
        for (identifier, sort) in template
            .constructor_declarations
            .iter()
            .map(|decl| (&decl.identifier, &decl.sort))
            .chain(
                template
                    .map_declarations
                    .iter()
                    .map(|decl| (&decl.identifier, &decl.sort)),
            )
        {
            let resolved = resolve_sort(ctx, template, sort);
            schemes
                .entry(identifier.node.clone())
                .or_default()
                .push(PolySortScheme {
                    vars: vars.clone(),
                    sort: resolved,
                });
        }
    }
    schemes
}

/// The narrow scheme table a system equation's own body is checked against:
/// the comparison operators and `if` only, built once and cached on `ctx`.
/// Deliberately excludes the container/function-update templates — a system
/// equation's primary signature (`ctx.system_equation_signature_by_group`)
/// already covers the container operations concretely for its own group, so
/// re-adding them here as a polymorphic fallback would misreport ambiguity.
pub(crate) fn build_builtin_scheme_signature(ctx: &mut TypeCheckContext) -> Arc<HashMap<String, Vec<PolySortScheme>>> {
    if ctx.builtin_scheme_signature.is_none() {
        let schemes = build_polymorphic_schemes(ctx, std::iter::once(&*BUILTIN_SCHEME_TEMPLATE));
        ctx.builtin_scheme_signature = Some(Arc::new(schemes));
    }
    Arc::clone(
        ctx.builtin_scheme_signature
            .as_ref()
            .expect("just computed above if it wasn't already"),
    )
}

/// The reserved names of every polymorphic built-in operator (containers,
/// function-update, comparisons/`if`) — a user `cons`/`map` declaration may
/// not redeclare any of them, regardless of its own sort. Derived directly
/// from the templates rather than from `ctx.signature`'s schemes, since this
/// check runs early in the pipeline, well before a `TypeCheckContext` (and so
/// a `Signature`) exists.
pub(crate) fn polymorphic_operator_names() -> impl Iterator<Item = &'static str> {
    CONTAINER_TEMPLATES
        .all()
        .into_iter()
        .flat_map(|template| {
            template
                .constructor_declarations
                .iter()
                .map(|decl| decl.identifier.as_str())
                .chain(template.map_declarations.iter().map(|decl| decl.identifier.as_str()))
        })
        .chain(crate::builtin_scheme_names())
}

/// The system-defined counterpart of `resolve_sort`. It differs in two ways:
/// `Reference` nodes are looked up among the system-internal sorts first (the
/// system specification never went through name resolution) and, failing that,
/// among the user specification's sort declarations — `structured_sort_equations`
/// generates fresh source text that is re-parsed, so a user sort it mentions
/// stays a bare `Reference` rather than a `Resolved` node. Unknown references
/// are a clean error rather than a panic, so a template mistake in a
/// `spec/*.mcrl2` file cannot crash the checker.
pub(crate) fn resolve_system_sort(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    sort_ids: &HashMap<String, ResolvedSortId>,
    sort: &SortExpression,
) -> Result<ResolvedSortId, WellTypedError> {
    match &sort.node {
        SortExpressionKind::Simple(sort) => Ok(ctx.sorts.primitive(*sort)),
        SortExpressionKind::Complex(op, subsort) => {
            let subsort = resolve_system_sort(ctx, user_spec, sort_ids, subsort)?;
            Ok(ctx.sorts.generic(*op, subsort))
        }
        SortExpressionKind::FlattenedFunction { domain, range } => {
            let domain = domain
                .iter()
                .map(|sort| resolve_system_sort(ctx, user_spec, sort_ids, sort))
                .collect::<Result<_, _>>()?;
            let range = resolve_system_sort(ctx, user_spec, sort_ids, range)?;
            Ok(ctx.sorts.function(domain, range))
        }
        // The system specification is parsed directly and never flattened, so
        // function sorts appear with a `Product` domain spine.
        SortExpressionKind::Function { domain, range } => {
            let mut resolved_domain = Vec::new();
            resolve_system_function_domain(ctx, user_spec, sort_ids, domain, &mut resolved_domain)?;
            let range = resolve_system_sort(ctx, user_spec, sort_ids, range)?;
            Ok(ctx.sorts.function(resolved_domain, range))
        }
        // A sort substituted into an Appendix-B template comes from the
        // normalized user specification, so its `DefId` indexes `user_spec`.
        SortExpressionKind::Resolved(_, id) => Ok(query_sort_of_def(ctx, user_spec, *id)),
        SortExpressionKind::Reference(name) => {
            if let Some(id) = sort_ids.get(name) {
                return Ok(*id);
            }
            match user_spec.sort_declarations.iter().find(|decl| decl.identifier == *name) {
                Some(decl) => Ok(query_sort_of_def(
                    ctx,
                    user_spec,
                    decl.id
                        .expect("name resolution assigned every user sort declaration an id"),
                )),
                None => Err(WellTypedError::Custom(
                    format!("the system-defined specification references the undeclared sort '{name}'").into(),
                )),
            }
        }
        SortExpressionKind::TypeVar(_) => {
            unreachable!("a template's `type_var` block is resolved once, up front, before it is ever cached")
        }
        SortExpressionKind::ResolvedTypeVar(_) => unreachable!(
            "a container/function-update template does declare its own sort variable(s) with a \
             `type_var` block now (see the unifying-polymorphism design), but `replace_sort` always \
             substitutes every ResolvedTypeVar node for a concrete sort before the result is ever \
             merged into `system` — resolve_system_sort only ever runs on that already-substituted copy"
        ),
        SortExpressionKind::Struct { .. } => unreachable!("the system-defined specification has no structured sorts"),
        SortExpressionKind::Product { .. } => {
            unreachable!("product sorts cannot occur outside a function domain")
        }
    }
}

/// Resolves the leaves of a `Product` domain spine in declaration order.
fn resolve_system_function_domain(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    sort_ids: &HashMap<String, ResolvedSortId>,
    sort: &SortExpression,
    domain: &mut Vec<ResolvedSortId>,
) -> Result<(), WellTypedError> {
    match &sort.node {
        SortExpressionKind::Product { lhs, rhs } => {
            resolve_system_function_domain(ctx, user_spec, sort_ids, lhs, domain)?;
            resolve_system_function_domain(ctx, user_spec, sort_ids, rhs, domain)?;
        }
        _ => domain.push(resolve_system_sort(ctx, user_spec, sort_ids, sort)?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use merc_syntax::ComplexSort;
    use merc_syntax::DefId;
    use merc_syntax::Sort;
    use merc_syntax::SourceMap;
    use merc_syntax::UntypedDataSpecification;

    use crate::DataSpecification;
    use crate::NumberEncoding;
    use crate::ResolvedSort;
    use crate::ResolvedSortId;
    use crate::Signature;
    use crate::TypeCheckContext;
    use crate::WellTypedError;
    use crate::basic_sort_data_specification;
    use crate::merge_signatures;
    use crate::resolve_system_signature;
    use crate::resolve_system_signature_full;

    /// Type checks `text` and resolves the basic-sort system signature in a
    /// fresh context, as `DataSpecification::from_untyped` does.
    fn resolve(text: &str) -> (DataSpecification, TypeCheckContext) {
        let mut sources = SourceMap::new();
        let spec = DataSpecification::from_untyped_with(
            UntypedDataSpecification::parse(text).unwrap(),
            NumberEncoding::default(),
            &mut sources,
        )
        .unwrap();
        let mut ctx = TypeCheckContext::new();
        let basics = basic_sort_data_specification(&mut sources, NumberEncoding::Binary);
        resolve_system_signature(&mut ctx, spec.data_specification(), &basics).unwrap();
        (spec, ctx)
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_boolean_operators_are_resolved() {
        let (_, ctx) = resolve("map f: Bool;");
        let signature = ctx.system_signature.as_ref().unwrap();

        let bool_sort = ctx.sorts.primitive(Sort::Bool);
        let conjunction = ctx.sorts.get(signature.mappings["&&"][0]).clone();
        assert_eq!(
            conjunction,
            ResolvedSort::Function {
                domain: vec![bool_sort, bool_sort],
                range: bool_sort,
            }
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_overloads_are_collected() {
        // Appendix B declares `max` for Pos # Nat, Nat # Pos and Nat # Nat
        // (and more through Int), all collected as one overloaded name.
        let (_, ctx) = resolve("map f: Nat;");
        let signature = ctx.system_signature.as_ref().unwrap();
        assert!(signature.mappings["max"].len() >= 3);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_template_instantiation_carries_user_sorts() {
        // `resolve_system_sort`'s handling of `Resolved` nodes, exercised
        // directly: production only ever feeds `resolve_system_signature` the
        // basic-sort spec (see its doc comment), so this instantiates the
        // full system-defined spec — containers included — in an isolated
        // context to check the substitution logic itself. The list template
        // instantiated with the user sort `D` should resolve `|>` to
        // `D # List(D) -> List(D)`.
        let spec = DataSpecification::from_untyped(
            UntypedDataSpecification::parse("sort D = struct s; map f: List(D);").unwrap(),
        )
        .unwrap();
        let mut ctx = TypeCheckContext::new();
        resolve_system_signature(&mut ctx, spec.data_specification(), spec.system_defined_specification()).unwrap();

        let def = DefId::new(*spec.sorts().index("D").unwrap());
        let d = ctx.sorts.def(def);
        let d_list = ctx.sorts.generic(ComplexSort::List, d);
        let expected = ctx.sorts.function(vec![d, d_list], d_list);

        let signature = ctx.system_signature.as_ref().unwrap();
        assert!(signature.constructors["|>"].contains(&expected));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_system_internal_sort_gets_fresh_def() {
        // `@NatPair` exists only in the system specification; it gets a nominal
        // DefId past the user declarations, and its name is recovered by
        // `sort_name`, which derives it from the system specification's
        // declarations on demand rather than from a stored table.
        let (spec, ctx) = resolve("sort D; map f: D;");
        let signature = ctx.system_signature.as_ref().unwrap();

        let pair_constructor = signature.constructors["@cPair"][0];
        let ResolvedSort::Function { domain: _, range } = ctx.sorts.get(pair_constructor) else {
            panic!("expected a function sort");
        };
        let ResolvedSort::Def(def) = ctx.sorts.get(*range) else {
            panic!("expected a nominal sort");
        };
        let user_len = spec.data_specification().sort_declarations.len();
        assert!(**def >= user_len);
        assert_eq!(
            ctx.sort_name(spec.data_specification(), spec.system_defined_specification(), *def),
            Some("@NatPair")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_unknown_reference_is_a_clean_error() {
        // A system specification referencing an undeclared sort must error
        // rather than panic; parse one directly to simulate a template mistake.
        let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Bool;").unwrap()).unwrap();
        let broken = UntypedDataSpecification::parse("map f: Unknown;").unwrap();

        let mut ctx = TypeCheckContext::new();
        match resolve_system_signature(&mut ctx, spec.data_specification(), &broken) {
            Err(WellTypedError::Custom(err)) => assert!(err.to_string().contains("Unknown")),
            other => panic!("expected a custom error, got {other:?}"),
        }
    }

    /// Type checks `text` through the full pipeline.
    fn resolve_full(text: &str) -> DataSpecification {
        DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_full_signature_covers_containers() {
        let spec = resolve_full("map f: Set(Nat);");
        let ctx = spec.context();
        assert!(
            ctx.system_equation_signature_by_group.iter().any(|signature| {
                signature.mappings.contains_key("in") && signature.mappings.contains_key("@setfset")
            }),
            "some group must resolve 'in'/'@setfset' for a spec using Set(Nat)"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_full_signature_validates_equation_binder_sorts() {
        // `Set` pulls in the `forall c:S. ...` extensionality equation, whose
        // binder sort must resolve; `from_untyped` fails otherwise.
        let spec = resolve_full("map f: Set(Nat);");
        assert!(
            !spec.system_defined_specification().equation_declarations.is_empty(),
            "the Set template should contribute equations to walk"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_full_signature_rejects_unresolvable_binder_sort() {
        let mut user_spec = UntypedDataSpecification::parse("map f: Bool;").unwrap();
        crate::assign_declaration_ids(&mut user_spec);
        let broken =
            UntypedDataSpecification::parse("map g: Bool -> Bool; eqn g(b) = forall s: S. b;").unwrap_or_else(|err| {
                panic!("the broken fixture spec should parse even though it doesn't type check: {err}")
            });

        let mut ctx = TypeCheckContext::new();
        crate::build_signature(&mut ctx, &user_spec).unwrap();
        let basics = crate::basic_sort_data_specification(&mut SourceMap::new(), crate::NumberEncoding::Binary);
        resolve_system_signature(&mut ctx, &user_spec, &basics).unwrap();
        match resolve_system_signature_full(&mut ctx, &user_spec, &broken, &[]) {
            Err(WellTypedError::Custom(err)) => assert!(err.to_string().contains('S'), "{err}"),
            other => panic!("expected a custom error, got {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_merge_signatures_unions_overloads_by_name() {
        let a = Signature {
            constructors: HashMap::from([("c".to_string(), vec![ResolvedSortId::new(0)])]),
            ..Signature::default()
        };
        let b = Signature {
            constructors: HashMap::from([("@cPair".to_string(), vec![ResolvedSortId::new(1)])]),
            ..Signature::default()
        };
        let merged = merge_signatures(&a, &b);
        assert!(merged.constructors.contains_key("c"));
        assert!(merged.constructors.contains_key("@cPair"));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_struct_desugared_symbols_resolve_in_their_own_group_signature() {
        // `c1`/`is_c1` are declared on the user spec by struct desugaring, not
        // on `system`, yet must still resolve in their own group's signature.
        let spec = resolve_full("sort D = struct c1(pr1: Nat)?is_c1; map f: Set(D);");
        let ctx = spec.context();
        assert!(
            !ctx.system_equation_signature_by_group.is_empty(),
            "Set(D) should produce at least one group"
        );
        assert!(
            ctx.system_equation_signature_by_group
                .iter()
                .any(|signature| signature.mappings.contains_key("is_c1")),
            "is_c1's own struct group should see it"
        );
    }
}
