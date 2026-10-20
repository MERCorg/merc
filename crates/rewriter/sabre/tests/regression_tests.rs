//! Regression tests for the Sabre rewriter that exercise rules whose delayed
//! equivalence/condition checks fail. A failing delayed check used to leave the
//! configuration untouched, causing `SabreRewriter` to loop forever while
//! `InnermostRewriter` returned immediately.

use ahash::AHashSet;

use merc_aterm::ATerm;
use merc_data::DataExpression;
use merc_data::to_untyped_data_expression;
use merc_sabre::Condition;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_sabre::Rule;
use merc_sabre::SabreRewriter;
use merc_sabre::test_utility::create_rewrite_rule;

/// Parses `input` into an untyped data expression, treating `variables` as variables.
fn term(input: &str, variables: &[&str]) -> DataExpression {
    let vars: AHashSet<String> = variables.iter().map(|v| v.to_string()).collect();
    to_untyped_data_expression(ATerm::from_string(input).unwrap(), Some(&vars))
}

/// Asserts that both rewrite engines rewrite `input` to `expected`.
fn assert_both_engines(spec: &RewriteSpecification, input: &str, expected: &str) {
    let input = term(input, &[]);
    let expected = term(expected, &[]);

    let mut inner = InnermostRewriter::new(spec);
    assert_eq!(inner.rewrite(&input), expected, "InnermostRewriter result mismatch");

    let mut sabre = SabreRewriter::new(spec);
    assert_eq!(sabre.rewrite(&input), expected, "SabreRewriter result mismatch");
}

/// A non-linear rule `eq(x, x) -> t` whose equivalence-class check fails on
/// `eq(a, b)`; the term is already in normal form.
#[test]
fn test_failing_equivalence_check_terminates() {
    let spec = RewriteSpecification::new(vec![create_rewrite_rule("eq(x, x)", "t", &["x"]).unwrap()]);

    assert_both_engines(&spec, "eq(a, b)", "eq(a, b)");
}

/// A conditional rule `f(x) -> t` (with condition `x == a`) whose condition
/// check fails on `f(b)`; the term is already in normal form.
#[test]
fn test_failing_condition_check_terminates() {
    let rule = Rule::with_condition(
        vec![Condition::new(term("x", &["x"]), term("a", &["x"]), true)],
        term("f(x)", &["x"]),
        term("t", &["x"]),
    );
    let spec = RewriteSpecification::new(vec![rule]);

    assert_both_engines(&spec, "f(b)", "f(b)");
}
