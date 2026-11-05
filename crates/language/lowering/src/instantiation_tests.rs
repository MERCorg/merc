//! End-to-end tests of the template instantiations that `lower_data_specification` adds: a
//! container, function-update, or comparison instantiation is only generated there.

use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

use crate::lower_data_specification;
use crate::lowering::lower_syntax_sort;

/// Whether the *lowered* spec of `text` declares the function-update
/// operators, checked through the full `from_untyped`/`lower_data_specification`
/// path: that's where a container/function-update instantiation is
/// generated at all — `system_defined_specification()` itself does not
/// carry one.
fn has_function_update(text: &str) -> bool {
    let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();
    lower_data_specification(&spec)
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

/// As `merc_typecheck`'s `spec_has_comparison_equations_for`, but over the *lowered* spec,
/// checked through the full `from_untyped`/`lower_data_specification` path
/// (which flattens function sorts, desugars structs, drives sort
/// inference, and — since the monomorphization-to-lowering milestone — is
/// also where a comparison-operator instantiation is generated at all).
///
/// Compares by aterm equality rather than `Display`: `SortCons`'s own
/// `Display` renders only the element sort (`"Nat"`, not `"List(Nat)"`) —
/// a binary-aterm-format quirk unrelated to this milestone, since the
/// container *kind* is a separate structural tag there, not part of a
/// name — so a `sort_name` like `"List(Nat)"` is instead parsed and
/// lowered through the same [`lower_syntax_sort`] every other
/// declaration sort goes through, and compared against that.
fn has_comparison_equations_for(text: &str, sort_name: &str) -> bool {
    let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();
    let lowered = lower_data_specification(&spec);

    let sort_spec = UntypedDataSpecification::parse(&format!("map q: {sort_name};")).unwrap();
    let expected_sort = lower_syntax_sort(&sort_spec.map_declarations[0].sort);

    lowered.equations().iter().any(|eqn| {
        eqn.variables()
            .into_iter()
            .any(|var| var.sort().protect() == expected_sort)
            && eqn.lhs().to_string().contains("if(")
    })
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
