use std::collections::HashMap;
use std::sync::Arc;

use merc_syntax::Span;
use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;

use crate::BUILTIN_SCHEME_TEMPLATE;
use crate::CONTAINER_TEMPLATES;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;
use crate::WellTypedError;
use crate::build_polymorphic_schemes;
use crate::check_products_within_domains;
use crate::query_sort_of_constructor;
use crate::query_sort_of_map;
use crate::target_sort;

/// A polymorphic overload: `sort` is a [ResolvedSortId] built by [`resolve_sort`](crate::resolve_sort)
/// from a template's own declaration, so it may mention [`ResolvedSort::Var`]
/// at any depth wherever the declaration mentions one of `vars`. Two
/// occurrences of the same bound variable within `sort` share the same
/// [TypeVarId] and so the same `Var` node — this is what makes `S` mean "the
/// same `S`" on both sides of a scheme like `in: S # List(S) -> Bool`.
///
/// Not a ground overload: using one requires instantiating it
/// (`ConstraintGenerator::instantiate_scheme`), substituting each variable in
/// `vars` for a fresh unification variable, shared across its occurrences
/// within that one instantiation.
#[derive(Clone, Debug)]
pub(crate) struct PolySortScheme {
    /// Not yet read anywhere: instantiation (`ConstraintGenerator::instantiate_scheme`)
    /// currently discovers a scheme's bound variables structurally, by
    /// walking `sort` and instantiating every `Var` it finds, rather than by
    /// consulting this list. It is kept for the next step of
    /// `docs/polymorphism.md`'s migration plan (checking each template's own
    /// equations once, with these variables held rigid), which does need it.
    #[allow(dead_code)]
    pub(crate) vars: Vec<TypeVarId>,
    pub(crate) sort: ResolvedSortId,
}

/// The (S, C, M) signature of a specification (Definition 15.1.5): the resolved
/// overload set of every constructor and mapping name, the lookup table for
/// overload resolution.
///
/// A symbol is a name together with its sort, so a name maps to one
/// [ResolvedSortId] per overload; duplicate declarations of the same symbol
/// collapse into one entry.
///
/// `schemes` is a separate, name-keyed table of polymorphic overloads
/// (containers, function-update, comparisons/`if`) — not split by
/// constructor/mapping, since nothing downstream needs that distinction for a
/// scheme (there is no [`merc_syntax::ConstructorId`]/[`merc_syntax::MapId`]
/// for synthesized template content to carry). Empty for every `Signature`
/// except the one merged into `ctx.signature` and the small per-role table
/// built for a system equation's own comparison/`if` lookup — see
/// `build_polymorphic_schemes`.
#[derive(Default)]
pub(crate) struct Signature {
    pub(crate) constructors: HashMap<String, Vec<ResolvedSortId>>,
    pub(crate) mappings: HashMap<String, Vec<ResolvedSortId>>,
    pub(crate) schemes: HashMap<String, Vec<PolySortScheme>>,
}

/// Computes the signature of `spec` and stores it on `ctx`, running the
/// signature-layer well-typedness checks of 15.1.7. Idempotent: a second call
/// is a no-op that returns the already-stored result.
///
/// Runs *before* alias normalization, so the errors refer to sorts as the user
/// wrote them (`D` rather than its alias expansion `Nat`).
///
/// # Requirements
///
/// Requires names to be resolved and structured sorts to be desugared.
pub(crate) fn build_signature<'a>(
    ctx: &'a mut TypeCheckContext,
    spec: &UntypedDataSpecification,
) -> Result<&'a Signature, WellTypedError> {
    if ctx.signature.is_none() {
        let signature = compute_signature(ctx, spec)?;
        ctx.signature = Some(Arc::new(signature));
    }

    Ok(ctx.signature.as_deref().expect("the signature was just computed"))
}

fn compute_signature(ctx: &mut TypeCheckContext, spec: &UntypedDataSpecification) -> Result<Signature, WellTypedError> {
    // resolve_sort has no meaning for (and panics on) a product sort outside a
    // function domain, so every sort this query resolves is checked first: the
    // constructor and mapping sorts, and the alias bodies reachable from them
    // through query_sort_of_def.
    for sort in spec.sort_declarations.iter().filter_map(|decl| decl.expr.as_ref()) {
        check_products_within_domains(sort)?;
    }
    for sort in spec
        .constructor_declarations
        .iter()
        .map(|decl| &decl.sort)
        .chain(spec.map_declarations.iter().map(|decl| &decl.sort))
    {
        check_products_within_domains(sort)?;
    }

    let mut signature = Signature::default();

    // Zero-arity constructors/mappings are keyed by *name* only, so a second
    // declaration under any different sort is rejected.
    let mut constants: HashMap<String, ResolvedSortId> = HashMap::new();

    for decl in &spec.constructor_declarations {
        // Resolve through the memoized query so lowering can later read the
        // interned constructor sort straight from the context.
        let constructor_id = decl.id.expect("assign_declaration_ids ran before build_signature");
        let id = query_sort_of_constructor(ctx, spec, constructor_id);

        // The constructor targets the range of its (function) sort. The check
        // is semantic — an alias of `Nat` is rejected like `Nat` itself — but
        // the error reports the target as written. When the whole constructor
        // sort is an alias of a function sort, the written sort itself is the
        // closest the user came to writing the target.
        let target = match ctx.sorts.get(id) {
            ResolvedSort::Function { domain: _, range } => *range,
            _ => id,
        };
        match ctx.sorts.get(target) {
            ResolvedSort::Primitive(_) => {
                return Err(WellTypedError::ConstructorForBasicSort {
                    constructor: decl.identifier.node.clone(),
                    sort: target_sort(&decl.sort).to_string(),
                    span: decl.identifier.span.clone(),
                });
            }
            ResolvedSort::Function { .. } => {
                return Err(WellTypedError::ConstructorForFunctionSort {
                    constructor: decl.identifier.node.clone(),
                    sort: target_sort(&decl.sort).to_string(),
                    span: decl.identifier.span.clone(),
                });
            }
            _ => {}
        }

        check_constant_name(&mut constants, ctx, &decl.identifier, decl.identifier.span.clone(), id)?;
        push_overload(
            signature.constructors.entry(decl.identifier.node.clone()).or_default(),
            id,
        );
    }

    for decl in &spec.map_declarations {
        let map_id = decl.id.expect("assign_declaration_ids ran before build_signature");
        let id = query_sort_of_map(ctx, spec, map_id);

        // The constructors and mappings must be disjoint *as symbols*: the same
        // name under both `cons` and `map` conflicts exactly when the resolved
        // sorts coincide, which also catches sorts that only differ through an
        // alias. Overloading the name with a different sort remains allowed.
        if signature
            .constructors
            .get(&decl.identifier.node)
            .is_some_and(|overloads| overloads.contains(&id))
        {
            return Err(WellTypedError::ConstructorAndMappingConflict {
                constructor: decl.identifier.node.clone(),
                map: decl.identifier.node.clone(),
                span: decl.identifier.span.clone(),
            });
        }

        check_constant_name(&mut constants, ctx, &decl.identifier, decl.identifier.span.clone(), id)?;
        push_overload(signature.mappings.entry(decl.identifier.node.clone()).or_default(), id);
    }

    // The polymorphic built-ins — containers, function-update, comparisons
    // and `if` — join the same one signature as real scheme entries, so
    // inference has exactly one table to look a name up in. User
    // declarations never carry a `type_var` block (see
    // `docs/polymorphism.md`'s "Open questions"), so this never collides
    // with the loops above; it only *adds* names.
    signature.schemes = build_polymorphic_schemes(
        ctx,
        CONTAINER_TEMPLATES.all().into_iter().chain([&*BUILTIN_SCHEME_TEMPLATE]),
    );

    Ok(signature)
}

/// Rejects a second zero-arity declaration of `name` under a different sort
/// than a previous one (see the comment on `constants` in
/// [compute_signature]). Symbols with a function sort are not zero-arity and
/// pass through untouched.
fn check_constant_name(
    constants: &mut HashMap<String, ResolvedSortId>,
    ctx: &TypeCheckContext,
    name: &str,
    span: Span,
    id: ResolvedSortId,
) -> Result<(), WellTypedError> {
    if matches!(ctx.sorts.get(id), ResolvedSort::Function { .. }) {
        return Ok(());
    }
    match constants.get(name) {
        Some(&existing) if existing != id => Err(WellTypedError::DuplicateConstantDifferentSort {
            name: name.to_string(),
            span,
        }),
        _ => {
            constants.insert(name.to_string(), id);
            Ok(())
        }
    }
}

/// Appends `id` unless it is already an overload, so duplicate declarations of
/// the same symbol collapse into one entry.
pub(crate) fn push_overload(overloads: &mut Vec<ResolvedSortId>, id: ResolvedSortId) {
    if !overloads.contains(&id) {
        overloads.push(id);
    }
}

#[cfg(test)]
mod tests {
    use merc_syntax::UntypedDataSpecification;

    use crate::DataSpecification;
    use crate::Signature;
    use crate::TypeCheckContext;
    use crate::WellTypedError;
    use crate::build_signature;

    fn typecheck(text: &str) -> DataSpecification {
        DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap()
    }

    fn typecheck_err(text: &str) -> WellTypedError {
        match DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()) {
            Err(err) => err,
            Ok(_) => panic!("expected the specification to be rejected"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_signature_collects_overloads() {
        let spec = typecheck("map f: Nat; f: Bool -> Bool;");
        assert_eq!(spec.signature().mappings["f"].len(), 2);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_signature_matches_declaration_sorts() {
        // The signature is resolved before normalization and the declaration
        // sorts after; both must agree on the interned ids, also through an
        // alias chain onto a struct representative.
        let spec = typecheck("sort D; A = B; B = struct s; cons c: A; map g: D -> Bool;");
        let signature = spec.signature();
        assert_eq!(
            signature.constructors["c"],
            vec![spec.sort_of_constructor(merc_syntax::ConstructorId::new(0))]
        );
        assert_eq!(
            signature.mappings["g"],
            vec![spec.sort_of_map(merc_syntax::MapId::new(0))]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_duplicate_declaration_is_one_symbol() {
        let spec = typecheck("map f: Nat; f: Nat;");
        assert_eq!(spec.signature().mappings["f"].len(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_and_mapping_conflict() {
        match typecheck_err("sort D; cons c: D; map c: D;") {
            WellTypedError::ConstructorAndMappingConflict { constructor, map, .. } => {
                assert_eq!(constructor, "c");
                assert_eq!(map, "c");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_and_mapping_overload_is_allowed() {
        // The name `c` is shared, but the sorts differ, so these are distinct
        // symbols to be disambiguated by overload resolution.
        let spec = typecheck("sort D; cons c: Bool -> D; d: D; map c: Nat -> D;");
        let signature = spec.signature();
        assert_eq!(signature.constructors["c"].len(), 1);
        assert_eq!(signature.mappings["c"].len(), 1);
        assert_ne!(signature.constructors["c"], signature.mappings["c"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_conflict_through_alias_is_detected() {
        // `A` and `(Nat -> Bool)` denote the same sort, so the constructor and
        // mapping `c` are the same symbol even though their written sorts
        // differ; the conflict is decided on the interned sort ids.
        match typecheck_err("sort A = Nat -> Bool; sort D; cons c: A -> D; d: D; map c: (Nat -> Bool) -> D;") {
            WellTypedError::ConstructorAndMappingConflict { constructor, .. } => assert_eq!(constructor, "c"),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_for_alias_of_basic_sort_reports_written_name() {
        // The target of `c` denotes the built-in `Nat` and is rejected, but the
        // error refers to the sort as the user wrote it, not to its expansion.
        match typecheck_err("sort D = Nat; cons c: D;") {
            WellTypedError::ConstructorForBasicSort { constructor, sort, .. } => {
                assert_eq!(constructor, "c");
                assert_eq!(sort, "D");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_for_function_sort_is_rejected() {
        // The higher-order target is written directly, without alias indirection.
        match typecheck_err("cons c: Bool -> (Nat -> Bool);") {
            WellTypedError::ConstructorForFunctionSort { constructor, sort, .. } => {
                assert_eq!(constructor, "c");
                assert_eq!(sort, "(Nat -> Bool)");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_for_alias_of_function_sort_reports_written_name() {
        match typecheck_err("sort A = Nat -> Bool; cons c: Bool -> A;") {
            WellTypedError::ConstructorForFunctionSort { constructor, sort, .. } => {
                assert_eq!(constructor, "c");
                assert_eq!(sort, "A");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_whose_whole_sort_is_a_function_alias() {
        // `c: A` with `A = Nat -> Bool` makes `c` a constructor for the basic
        // sort `Bool`; the error reports the written sort `A`, the closest the
        // user came to writing the target.
        match typecheck_err("sort A = Nat -> Bool; cons c: A;") {
            WellTypedError::ConstructorForBasicSort { constructor, sort, .. } => {
                assert_eq!(constructor, "c");
                assert_eq!(sort, "A");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_constructor_whose_function_alias_targets_a_declared_sort() {
        // As above, but the aliased function sort ranges over the declared sort
        // `D`, so `c` is a valid (unary) constructor for `D`.
        let spec = typecheck("sort D; A = Nat -> D; cons c: A; d: D;");
        assert_eq!(spec.signature().constructors["c"].len(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_build_signature_is_idempotent() {
        let spec = typecheck("sort D; cons c: D; map f: D -> Bool;");

        let mut ctx = TypeCheckContext::new();
        let first: *const Signature = build_signature(&mut ctx, spec.data_specification()).unwrap();
        let second: *const Signature = build_signature(&mut ctx, spec.data_specification()).unwrap();
        assert!(
            std::ptr::eq(first, second),
            "the second call must return the already-stored signature"
        );
    }
}
