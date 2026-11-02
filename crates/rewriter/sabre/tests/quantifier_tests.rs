//! Tests for `InnermostRewriter`'s handling of binder terms (`exists`, `forall`, `lambda`),
//! independent of any [merc_sabre::Enumerator] — see `crates/rewriter/enumerate/tests` for tests that
//! wire one in and have quantifiers decided.

use ahash::AHashSet;
use ahash::HashMap;
use ahash::HashMapExt;

use merc_data::BinderType;
use merc_data::DataAbstraction;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::is_data_binder;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_sabre::test_utility::create_rewrite_rule;
use merc_sabre::utilities::apply_substitution;

/// Parses `input` treating every name in `variables` as a variable.
fn term(input: &str, variables: &[&str]) -> DataExpression {
    let variables: AHashSet<String> = variables.iter().map(|v| v.to_string()).collect();
    DataExpression::from_string_untyped(input, &variables).unwrap()
}

/// `f(x) = x`, so a rewritten body of `f(n)` (`n` free) collapses to `n`, making it easy to
/// tell whether a binder's body was rewritten at all.
fn identity_spec() -> RewriteSpecification {
    RewriteSpecification::new(vec![create_rewrite_rule("f(x)", "x", &["x"]).unwrap()])
}

/// `InnermostRewriter::new` builds a rewriter with no [merc_sabre::Enumerator] wired in
/// ([merc_sabre::NoQuantifierEnumerator]), so a quantifier it reaches is left in place, over
/// its rewritten body — it must not panic the way an unhandled binder used to.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_exists_without_an_enumerator_is_left_unevaluated_over_its_rewritten_body() {
    let spec = identity_spec();
    let n = DataVariable::new("n");
    let input: DataExpression = DataAbstraction::new(BinderType::Exists, &[n.clone()], term("f(n)", &["n"])).into();
    let expected: DataExpression = DataAbstraction::new(BinderType::Exists, &[n], term("n", &["n"])).into();

    assert_eq!(InnermostRewriter::new(&spec).rewrite(&input), expected);
}

/// A `lambda` (or `set`/`bag` comprehension) is reconstructed over its rewritten body rather
/// than reduced — β-reduction is separate, unimplemented work — but, like `exists`/`forall`,
/// no longer panics.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_lambda_no_longer_panics_and_rewrites_its_body() {
    let spec = identity_spec();
    let n = DataVariable::new("n");
    let input: DataExpression = DataAbstraction::new(BinderType::Lambda, &[n.clone()], term("f(n)", &["n"])).into();
    let expected: DataExpression = DataAbstraction::new(BinderType::Lambda, &[n], term("n", &["n"])).into();

    let result = InnermostRewriter::new(&spec).rewrite(&input);
    assert!(is_data_binder(&result), "expected a binder, got {result}");
    assert_eq!(result, expected);
}

/// A binder's own bound variable shadows an enclosing substitution of the same name: rewriting
/// `exists n. f(n)` under `{n := g}` must leave the bound `n` alone, not splice `g` in.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_shadows_an_enclosing_substitution_of_the_same_name() {
    let spec = identity_spec();
    let n = DataVariable::new("n");
    let input: DataExpression = DataAbstraction::new(BinderType::Exists, &[n.clone()], term("f(n)", &["n"])).into();
    let expected: DataExpression = DataAbstraction::new(BinderType::Exists, &[n.clone()], term("n", &["n"])).into();

    let mut sigma = HashMap::new();
    sigma.insert(n, term("g", &[]));

    let result = InnermostRewriter::new(&spec).rewrite_with(&input, &sigma);
    assert_eq!(
        result, expected,
        "the bound `n` must shadow the substitution, not be replaced by `g`"
    );
}

/// Returns the single bound variable and the body of `binder`.
fn single_bound_variable(binder: &DataExpression) -> (DataVariable, DataExpression) {
    assert!(is_data_binder(binder), "expected a binder, got {binder}");
    let abstraction = DataAbstraction::from(binder.clone());
    let variables: Vec<DataVariable> = abstraction.variables().into_iter().collect();
    assert_eq!(variables.len(), 1, "expected one bound variable in {binder}");
    (variables[0].clone(), abstraction.body().protect())
}

/// Returns `g(y, renamed)`; the name of `renamed` cannot be parsed.
fn renamed_body(renamed: &DataVariable) -> DataExpression {
    let mut sigma = HashMap::new();
    sigma.insert(DataVariable::new("z"), DataExpression::from(renamed.clone()));
    apply_substitution(&term("g(y, z)", &["y", "z"]), &sigma)
}

/// Rewriting `exists y. g(x, y)` under `{x := y}` must rename the bound `y` apart, since the
/// substituted `y` would otherwise be captured.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_is_renamed_apart_from_a_substituted_image() {
    let spec = identity_spec();
    let y = DataVariable::new("y");
    let input: DataExpression = DataAbstraction::new(
        BinderType::Exists,
        std::slice::from_ref(&y),
        term("g(x, y)", &["x", "y"]),
    )
    .into();

    let mut sigma = HashMap::new();
    sigma.insert(DataVariable::new("x"), DataExpression::from(y.clone()));

    let result = InnermostRewriter::new(&spec).rewrite_with(&input, &sigma);
    let (renamed, body) = single_bound_variable(&result);
    assert_ne!(renamed, y, "the bound `y` must be renamed in {result}");
    assert_eq!(body, renamed_body(&renamed));
}

/// The same for a binder in a right-hand side: instantiating `h(x) = exists y. g(x, y)` for
/// `h(y)` must not capture the matched `y`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_right_hand_side_binder_is_renamed_apart_from_a_matched_value() {
    let y = DataVariable::new("y");
    let mut rule = create_rewrite_rule("h(x)", "x", &["x"]).unwrap();
    rule.rhs = DataAbstraction::new(
        BinderType::Exists,
        std::slice::from_ref(&y),
        term("g(x, y)", &["x", "y"]),
    )
    .into();
    let spec = RewriteSpecification::new(vec![rule]);

    let result = InnermostRewriter::new(&spec).rewrite(&term("h(y)", &["y"]));
    let (renamed, body) = single_bound_variable(&result);
    assert_ne!(renamed, y, "the bound `y` must be renamed in {result}");
    assert_eq!(body, renamed_body(&renamed));
}
