//! Tests for rewriting under a substitution, see [merc_sabre::RewriteEngine::rewrite_with].

use ahash::AHashSet;
use ahash::HashMap;
use ahash::HashMapExt;

use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_sabre::Condition;
use merc_sabre::InnermostRewriter;
use merc_sabre::NaiveRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_sabre::Rule;
use merc_sabre::SabreRewriter;
use merc_sabre::test_utility::create_rewrite_rule;
use merc_sabre::utilities::EmptySubstitution;

/// Parses `input` treating every name in `variables` as a variable.
fn term(input: &str, variables: &[&str]) -> DataExpression {
    let variables: AHashSet<String> = variables.iter().map(|v| v.to_string()).collect();
    DataExpression::from_string_untyped(input, &variables).unwrap()
}

/// Builds a substitution mapping each name to the corresponding parsed closed term.
fn substitution(entries: &[(&str, &str)]) -> HashMap<DataVariable, DataExpression> {
    let mut sigma = HashMap::new();
    for (name, value) in entries {
        sigma.insert(DataVariable::new(*name), term(value, &[]));
    }
    sigma
}

/// Asserts that all three engines rewrite `input` under `sigma` to `expected`.
///
/// The innermost engine consumes the substitution while it decomposes the term, whereas the
/// other two fall back on eagerly substituting first, so this pins the fused path to the simple
/// baseline.
fn assert_all_engines(
    spec: &RewriteSpecification,
    input: &DataExpression,
    sigma: &HashMap<DataVariable, DataExpression>,
    expected: &DataExpression,
) {
    assert_eq!(
        &InnermostRewriter::new(spec).rewrite_with(input, sigma),
        expected,
        "InnermostRewriter result mismatch"
    );
    assert_eq!(
        &SabreRewriter::new(spec).rewrite_with(input, sigma),
        expected,
        "SabreRewriter result mismatch"
    );
    assert_eq!(
        &NaiveRewriter::new(spec).rewrite_with(input, sigma),
        expected,
        "NaiveRewriter result mismatch"
    );
}

/// `plus(x, zero) = x` and `plus(x, s(y)) = s(plus(x, y))`, neither of which observes the first
/// argument, so it may hold a free variable.
fn plus_spec() -> RewriteSpecification {
    RewriteSpecification::new(vec![
        create_rewrite_rule("plus(x, zero)", "x", &["x", "y"]).unwrap(),
        create_rewrite_rule("plus(x, s(y))", "s(plus(x, y))", &["x", "y"]).unwrap(),
    ])
}

/// The empty substitution must leave the plain rewrite path untouched.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_empty_substitution_reproduces_rewrite() {
    let spec = plus_spec();
    let input = term("plus(s(zero), s(s(zero)))", &[]);
    let expected = term("s(s(s(zero)))", &[]);

    assert_eq!(InnermostRewriter::new(&spec).rewrite(&input), expected);
    assert_eq!(
        InnermostRewriter::new(&spec).rewrite_with(&input, &EmptySubstitution),
        expected
    );

    assert_eq!(SabreRewriter::new(&spec).rewrite(&input), expected);
    assert_eq!(
        SabreRewriter::new(&spec).rewrite_with(&input, &EmptySubstitution),
        expected
    );

    assert_eq!(NaiveRewriter::new(&spec).rewrite(&input), expected);
    assert_eq!(
        NaiveRewriter::new(&spec).rewrite_with(&input, &EmptySubstitution),
        expected
    );
}

/// A bare variable input is replaced by its image, which is already a normal form.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bare_variable_input() {
    let spec = plus_spec();

    assert_all_engines(
        &spec,
        &term("x", &["x"]),
        &substitution(&[("x", "s(zero)")]),
        &term("s(zero)", &[]),
    );
}

/// `plus(x, zero)` with `x` mapped to `s(zero)` must yield `s(zero)`, not `plus(s(zero), zero)`:
/// the rule fires after the substitution has been applied.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_rule_fires_across_substitution_boundary() {
    let spec = plus_spec();

    assert_all_engines(
        &spec,
        &term("plus(x, zero)", &["x"]),
        &substitution(&[("x", "s(zero)")]),
        &term("s(zero)", &[]),
    );
}

/// A variable outside the domain of the substitution is its own normal form, including when it
/// ends up as the result.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_variable_outside_domain_is_unchanged() {
    let spec = plus_spec();

    assert_all_engines(
        &spec,
        &term("plus(y, zero)", &["y"]),
        &substitution(&[("x", "s(zero)")]),
        &term("y", &["y"]),
    );
}

/// A variable also blocks a match when it sits at a position a left-hand side observes: it has
/// no head symbol, so no pattern applies and the surrounding term is in normal form.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_variable_at_an_observed_position_blocks_the_match() {
    let spec = plus_spec();

    // The second argument decides between the two rules, so a variable there matches neither.
    assert_all_engines(
        &spec,
        &term("plus(zero, y)", &["y"]),
        &substitution(&[("x", "s(zero)")]),
        &term("plus(zero, y)", &["y"]),
    );
}

/// Open terms are normal forms for a plain rewrite too, without any substitution involved.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_plain_rewrite_leaves_open_terms_alone() {
    let spec = plus_spec();
    let input = term("plus(zero, y)", &["y"]);

    assert_eq!(InnermostRewriter::new(&spec).rewrite(&input), input);
    assert_eq!(SabreRewriter::new(&spec).rewrite(&input), input);
    assert_eq!(NaiveRewriter::new(&spec).rewrite(&input), input);
}

/// The image of a variable is assumed to be in normal form and is spliced in without being
/// rewritten again, which is observable both in the result and in the number of rewrite steps.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_substituted_value_is_not_rewritten() {
    let spec = RewriteSpecification::new(vec![
        create_rewrite_rule("f(x)", "g(x)", &["x"]).unwrap(),
        create_rewrite_rule("a", "b", &[]).unwrap(),
    ]);

    let (substituted, substituted_stats) = InnermostRewriter::new(&spec)
        .rewrite_under_with_statistics(&term("f(x)", &["x"]), &substitution(&[("x", "a")]));
    assert_eq!(substituted, term("g(a)", &[]));

    // Rewriting the same term with `a` spelled out instead normalises it to `b` first, which
    // costs the extra rewrite step that the normal-form assumption saves.
    let (rewritten, rewritten_stats) = InnermostRewriter::new(&spec).rewrite_with_statistics(&term("f(a)", &[]));
    assert_eq!(rewritten, term("g(b)", &[]));

    assert_eq!(substituted_stats.rewrite_steps, 1);
    assert_eq!(rewritten_stats.rewrite_steps, 2);
}

/// A non-linear pattern whose repeated variable is only equal after substituting.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_nonlinear_pattern_with_substituted_variable() {
    let spec = RewriteSpecification::new(vec![create_rewrite_rule("eq(x, x)", "t", &["x"]).unwrap()]);

    assert_all_engines(
        &spec,
        &term("eq(x, a)", &["x"]),
        &substitution(&[("x", "a")]),
        &term("t", &[]),
    );

    // With a different image the equivalence check fails and the term is already in normal form.
    assert_all_engines(
        &spec,
        &term("eq(x, a)", &["x"]),
        &substitution(&[("x", "b")]),
        &term("eq(b, a)", &[]),
    );
}

/// A condition that only holds once the substitution has been applied.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_condition_depends_on_substituted_subterm() {
    let spec = RewriteSpecification::new(vec![Rule::with_condition(
        vec![Condition::new(term("x", &["x"]), term("a", &[]), true)],
        term("f(x)", &["x"]),
        term("t", &["x"]),
    )]);

    assert_all_engines(
        &spec,
        &term("f(y)", &["y"]),
        &substitution(&[("y", "a")]),
        &term("t", &[]),
    );

    // The condition fails for a different image, leaving the substituted term in normal form.
    assert_all_engines(
        &spec,
        &term("f(y)", &["y"]),
        &substitution(&[("y", "b")]),
        &term("f(b)", &[]),
    );
}
