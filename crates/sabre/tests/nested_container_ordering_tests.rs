//! Regression tests for `less_total`, the total order that `Set`, `Bag`,
//! `FSet` and `FBag` use internally to keep their element lists canonically
//! sorted (see `crates/syntax/spec/comparison.mcrl2`).
//! 
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

/// Rewrites `expr` (of sort `sort`) to normal form.
///
/// The expression is placed as the right-hand side of an equation for a fresh
/// constant `q`; rewriting the lowered `q` then evaluates it. Going through the
/// specification is what gives the expression its lowered, correctly sorted
/// form — there is no separate entry point for lowering a single expression.
#[track_caller]
fn rewrite(expr: &str, sort: &str) -> String {
    let text = format!("map q: {sort};\neqn q = {expr};");
    let untyped = UntypedDataSpecification::parse(&text).expect("the specification should parse");
    let spec = DataSpecification::from_untyped(untyped)
        .unwrap_or_else(|error| panic!("should type check `{expr}`: {error:?}"));
    let lowered = spec.lower_data_specification();

    let query = lowered
        .equations()
        .iter()
        .find(|equation| equation.lhs().to_string() == "q")
        .expect("the q equation should be lowered")
        .lhs()
        .protect();

    let rules = RewriteSpecification::from_data_specification(&lowered);
    InnermostRewriter::new(&rules).rewrite(&query).to_string()
}

#[track_caller]
fn assert_holds(expr: &str) {
    assert_eq!(rewrite(expr, "Bool"), "true", "`{expr}` should hold");
}

/// `{1, 2}` and `{3}` are incomparable under `FSet(Nat)`'s own `<` (subset
/// inclusion): neither is a subset of the other. Building `FSet(FSet(Nat))`
/// out of them exercises exactly the guard that switched from `<` to
/// `less_total`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_nested_fset_union_reduces_to_a_canonical_normal_form() {
    let forward = rewrite("{ {1, 2} } + { {3} }", "FSet(FSet(Nat))");
    let backward = rewrite("{ {3} } + { {1, 2} }", "FSet(FSet(Nat))");

    // Fully reduced: no leftover `+`/`@fset_insert` node stuck in the term.
    assert!(!forward.contains('+'), "got a stuck normal form: {forward}");
    assert!(!forward.contains("@fset_insert"), "got a stuck normal form: {forward}");

    // Canonical: the same set, inserted in either order, normalises identically.
    assert_eq!(forward, backward);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_nested_fset_cardinality_and_membership_after_union() {
    // `#` on `FSet` does not fully collapse its `succ(...)` chain down to a
    // literal here.
    let card_forward = rewrite("#({ {1, 2} } + { {3} })", "Nat");
    let card_backward = rewrite("#({ {3} } + { {1, 2} })", "Nat");
    assert!(!card_forward.contains('+'), "got a stuck normal form: {card_forward}");
    assert_eq!(card_forward, card_backward);

    assert_holds("{1, 2} in ({ {1, 2} } + { {3} })");
    assert_holds("{3} in ({ {1, 2} } + { {3} })");
}

/// The same non-total-`<` gap exists for `FBag`, whose elements here are
/// themselves `FSet(Nat)` values.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_nested_fbag_union_reduces_to_a_canonical_normal_form() {
    let forward = rewrite("{ {1, 2}: 1 } + { {3}: 1 }", "FBag(FSet(Nat))");
    let backward = rewrite("{ {3}: 1 } + { {1, 2}: 1 }", "FBag(FSet(Nat))");

    assert!(!forward.contains('+'), "got a stuck normal form: {forward}");
    assert!(!forward.contains("@fbag_insert"), "got a stuck normal form: {forward}");
    assert_eq!(forward, backward);

    let card_forward = rewrite("#({ {1, 2}: 1 } + { {3}: 1 })", "Nat");
    let card_backward = rewrite("#({ {3}: 1 } + { {1, 2}: 1 })", "Nat");
    assert!(!card_forward.contains('+'), "got a stuck normal form: {card_forward}");
    assert_eq!(card_forward, card_backward);
}
