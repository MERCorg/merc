use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::Span;
use merc_syntax::UntypedDataSpecification;

use crate::BUILTIN_SCHEME_TEMPLATE;
use crate::CONTAINER_TEMPLATES;
use crate::DisplaySortContext;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;
use crate::WellTypedError;
use crate::build_polymorphic_schemes;
use crate::check_products_within_domains;
use crate::query_sort_of_constructor;
use crate::query_sort_of_map;
use crate::resolve_sort;

/// A polymorphic overload: `sort` is a [ResolvedSortId] built by
/// [`resolve_sort`](crate::resolve_sort) from a template's own declaration.
#[derive(Clone, Debug)]
pub(crate) struct PolySortScheme {
    pub(crate) sort: ResolvedSortId,
}

/// The (S, C, M) signature of a specification (Definition 15.1.5): the resolved
/// overload set of every constructor and mapping name, the lookup table for
/// overload resolution.
///
/// A symbol is a name together with its sort, so a name maps to one
/// [ResolvedSortId] per overload; duplicate declarations of the same symbol
/// collapse into one entry.
#[derive(Default)]
pub(crate) struct Signature {
    pub(crate) constructors: HashMap<String, Vec<ResolvedSortId>>,
    pub(crate) mappings: HashMap<String, Vec<ResolvedSortId>>,
    pub(crate) schemes: HashMap<String, Vec<PolySortScheme>>,
}

impl Signature {
    /// Renders `self` with every overload's [`ResolvedSortId`] resolved to its
    /// printable sort (via [`DisplaySortContext`]), sorted by name so the
    /// output is deterministic.
    pub(crate) fn display<'a>(
        &'a self,
        ctx: &'a TypeCheckContext,
        spec: &'a UntypedDataSpecification,
    ) -> DisplaySignature<'a> {
        DisplaySignature {
            signature: self,
            ctx,
            spec,
        }
    }
}

/// See [`Signature::display`].
pub(crate) struct DisplaySignature<'a> {
    signature: &'a Signature,
    ctx: &'a TypeCheckContext,
    spec: &'a UntypedDataSpecification,
}

impl fmt::Display for DisplaySignature<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let write_overloads =
            |f: &mut fmt::Formatter<'_>, keyword: &str, overloads: &HashMap<String, Vec<ResolvedSortId>>| {
                if overloads.is_empty() {
                    return Ok(());
                }

                let mut names: Vec<&String> = overloads.keys().collect();
                names.sort();

                writeln!(f, "{keyword}")?;
                for name in names {
                    for &sort in &overloads[name] {
                        writeln!(f, "   {name}: {};", DisplaySortContext::new(self.ctx, self.spec, sort))?;
                    }
                }
                Ok(())
            };

        write_overloads(f, "cons", &self.signature.constructors)?;
        write_overloads(f, "map", &self.signature.mappings)?;

        if !self.signature.schemes.is_empty() {
            let mut names: Vec<&String> = self.signature.schemes.keys().collect();
            names.sort();

            writeln!(f, "schemes")?;
            for name in names {
                for scheme in &self.signature.schemes[name] {
                    writeln!(
                        f,
                        "   {name}: {};",
                        DisplaySortContext::new(self.ctx, self.spec, scheme.sort)
                    )?;
                }
            }
        }

        Ok(())
    }
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
    let mut signature = Signature::default();
    let mut constants: HashMap<String, ResolvedSortId> = HashMap::new();
    push_declarations(ctx, spec, spec, false, &mut signature, &mut constants)?;

    // The polymorphic built-ins.
    signature.schemes = build_polymorphic_schemes(
        ctx,
        CONTAINER_TEMPLATES.all().into_iter().chain([&*BUILTIN_SCHEME_TEMPLATE]),
    );

    Ok(signature)
}

/// Checks and collects `decl_spec`'s own constructor/mapping declarations into `signature`,
/// running every signature-level well-typedness rule of Definition 15.1.5/15.1.7 `is_well_typed`
/// doesn't already cover post-normalization:
///
///     - no product sort outside a function domain,
///     - no constructor for a function sort,
///     - constructor/mapping disjointness, and
///     - no zero-arity symbol declared twice under different sorts (`constants`, shared across both declaration kinds and,
/// when called again for a second spec, across that call too — see `resolve_system_signature`).
///
/// `resolve_spec` is the specification whose `sort_declarations` table a `Resolved(name, SortId)`
/// node in `decl_spec` indexes into.
///
/// `trusted` skips the one rule the system-defined specification's own basic-sort constructors
/// (`@c0: Nat`, `@cNat`, ...) legitimately break: no constructor for a basic sort.
pub(crate) fn push_declarations(
    ctx: &mut TypeCheckContext,
    decl_spec: &UntypedDataSpecification,
    resolve_spec: &UntypedDataSpecification,
    trusted: bool,
    signature: &mut Signature,
    constants: &mut HashMap<String, ResolvedSortId>,
) -> Result<(), WellTypedError> {
    // resolve_sort has no meaning for (and panics on) a product sort outside a
    // function domain, so every sort this query resolves is checked first.
    for sort in decl_spec
        .sort_declarations
        .iter()
        .filter_map(|decl| decl.expr.as_ref())
        .chain(decl_spec.constructor_declarations.iter().map(|decl| &decl.sort))
        .chain(decl_spec.map_declarations.iter().map(|decl| &decl.sort))
    {
        check_products_within_domains(sort)?;
    }

    for decl in &decl_spec.constructor_declarations {
        // Resolve through the memoized, `ConstructorId`-keyed query for the user's own
        // specification, so lowering can later read the interned constructor sort straight from
        // the context. `trusted` content resolves directly against `resolve_spec` instead.
        let sort_id = if trusted {
            resolve_sort(ctx, resolve_spec, &decl.sort)
        } else {
            let constructor_id = decl.id.expect("assign_declaration_ids ran before build_signature");
            query_sort_of_constructor(ctx, decl_spec, constructor_id)
        };

        // The constructor targets the range of its (function) sort. The check
        // is semantic — an alias of `Nat` is rejected like `Nat` itself — but
        // the error reports the target as written. When the whole constructor
        // sort is an alias of a function sort, the written sort itself is the
        // closest the user came to writing the target.
        let target = match ctx.sorts.get(sort_id) {
            ResolvedSort::Function { domain: _, range } => *range,
            _ => sort_id,
        };
        match ctx.sorts.get(target) {
            ResolvedSort::Primitive(_) if !trusted => {
                return Err(WellTypedError::ConstructorForBasicSort {
                    constructor: decl.identifier.node.clone(),
                    sort: written_target_sort(&decl.sort).to_string(),
                    span: decl.identifier.span.clone(),
                });
            }
            ResolvedSort::Function { .. } => {
                return Err(WellTypedError::ConstructorForFunctionSort {
                    constructor: decl.identifier.node.clone(),
                    sort: written_target_sort(&decl.sort).to_string(),
                    span: decl.identifier.span.clone(),
                });
            }
            _ => {}
        }

        check_constant_name(constants, ctx, &decl.identifier, decl.identifier.span.clone(), sort_id)?;
        push_overload(
            signature.constructors.entry(decl.identifier.node.clone()).or_default(),
            sort_id,
        );
    }

    for decl in &decl_spec.map_declarations {
        let id = if trusted {
            resolve_sort(ctx, resolve_spec, &decl.sort)
        } else {
            let map_id = decl.id.expect("assign_declaration_ids ran before build_signature");
            query_sort_of_map(ctx, decl_spec, map_id)
        };

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

        check_constant_name(constants, ctx, &decl.identifier, decl.identifier.span.clone(), id)?;
        push_overload(signature.mappings.entry(decl.identifier.node.clone()).or_default(), id);
    }

    Ok(())
}

/// The range of a written (function) sort, or the sort itself otherwise — for rendering the
/// "sort as written" half of a [`WellTypedError::ConstructorForBasicSort`]/
/// [`WellTypedError::ConstructorForFunctionSort`] message.
///
/// Unlike [`crate::target_sort`], this tolerates a plain `Function` node, not just `FlattenedFunction`:
/// the user's own declarations are always already flattened by the time `push_declarations` sees
/// them, but a `trusted` specification's are not (`resolve_system_signature` never runs
/// `flatten_function_sorts` over `system`/`basics` — nothing needed it to, since no real system
/// content has ever hit this error path before). Asserting the precondition here, the way
/// `target_sort` does, would turn a `trusted` equation's *rejection* into a panic instead of the
/// `WellTypedError` this whole check exists to produce in the first place.
fn written_target_sort(sort: &SortExpression) -> &SortExpression {
    match &sort.node {
        SortExpressionKind::Function { range, .. } | SortExpressionKind::FlattenedFunction { range, .. } => range,
        _ => sort,
    }
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
    fn test_signature_display_renders_resolved_sorts() {
        let spec = typecheck("sort D; cons c: D; map f: D -> Bool;");
        let signature = spec.signature();
        let text = signature.display(spec.context(), spec.data_specification()).to_string();

        assert!(text.contains("c: D;"), "expected a rendered constructor: {text}");
        assert!(text.contains("f: D -> Bool;"), "expected a rendered mapping: {text}");
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
