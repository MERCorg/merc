use std::rc::Rc;

use merc_data::DataExpression;
use merc_data::Mcrl2DataSpecification;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::Enumerator;
use merc_enumerate::FreshVariableGenerator;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

/// Parses and type-checks `map b: Bool; eqn b = <goal>;`, returning the lowered spec.
fn lower(goal: &str) -> Mcrl2DataSpecification {
    let source = format!("map b: Bool;\neqn b = {goal};");
    let untyped = UntypedDataSpecification::parse(&source).unwrap();
    let data_spec = DataSpecification::from_untyped(untyped).unwrap();
    data_spec.lower_data_specification()
}

/// Returns the right-hand side of `b`'s (nullary) defining equation.
fn goal_body(spec: &Mcrl2DataSpecification) -> DataExpression {
    spec.equations()
        .iter()
        .find(|eq| eq.lhs().data_function_symbol().name().value() == "b")
        .expect("no equation for b")
        .rhs()
        .protect()
}

fn rewriter_with_enumerator(data_spec: &Mcrl2DataSpecification) -> InnermostRewriter<Enumerator> {
    let spec = RewriteSpecification::from_data_specification(data_spec);
    let plans = Rc::new(EnumerationPlans::build(data_spec));
    let generator = FreshVariableGenerator::new("v", std::iter::empty());
    InnermostRewriter::with_enumerator(&spec, Enumerator::new(plans, generator))
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_exists_over_nat_is_decided_true() {
    let data_spec = lower("exists n: Nat . n == 5");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, DataExpression::bool_literal(true));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_forall_over_nat_is_decided_false() {
    let data_spec = lower("forall n: Nat . n == 5");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, DataExpression::bool_literal(false));
}

/// A quantifier nested directly in another's body.
#[test]
#[cfg_attr(miri, ignore)]
fn test_a_directly_nested_quantifier_is_also_decided() {
    let data_spec = lower("exists n: Nat . n == 0 && forall m: Nat . m == m");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, DataExpression::bool_literal(true));
}

/// A nested quantifier over the outer quantifier's variable, which can only be
/// decided once the outer enumeration has substituted a value for it.
#[test]
#[cfg_attr(miri, ignore)]
fn test_a_dependent_nested_quantifier_is_decided() {
    for (goal, expected) in [
        ("forall x: Bool . exists y: Bool . x != y", true),
        ("exists x: Bool . forall y: Bool . x == y", false),
        ("exists n: Nat . forall b: Bool . b || n == 3", true),
    ] {
        let data_spec = lower(goal);
        let body = goal_body(&data_spec);

        let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
        assert_eq!(result, DataExpression::bool_literal(expected), "{goal}");
    }
}

/// Lowers `spec` extended with `b = <expr>` and `e = <expected>`, and checks
/// that rewriting `b` with an [Enumerator] yields `e`'s normal form.
fn assert_rewrites_to(spec: &str, expr: &str, expected: &str) {
    let source = format!("{spec}\nmap b, e: Bool;\neqn b = {expr};\n    e = {expected};");
    let untyped = UntypedDataSpecification::parse(&source).unwrap();
    let data_spec = DataSpecification::from_untyped(untyped)
        .unwrap()
        .lower_data_specification();

    let rhs = |name: &str| {
        data_spec
            .equations()
            .iter()
            .find(|eq| eq.lhs().data_function_symbol().name().value() == name)
            .unwrap_or_else(|| panic!("no equation for {name}"))
            .rhs()
            .protect()
    };

    let mut rewriter = rewriter_with_enumerator(&data_spec);
    let result = rewriter.rewrite(&rhs("b"));
    let expected_result = rewriter.rewrite(&rhs("e"));
    assert_eq!(result, expected_result, "{expr} should rewrite to {expected}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_quantifiers_over_nat_and_pos() {
    assert_rewrites_to("", "exists x: Nat. x in {1, 2, 25, 600} && 25 == x", "true");

    // Only small solutions are found within the enumeration bound.
    assert_rewrites_to("", "exists n: Nat. n div 2 == 1", "true");
    assert_rewrites_to("", "forall n: Nat. n div 2 == 1", "false");

    assert_rewrites_to("", "forall x: Nat. x == 3", "false");
    assert_rewrites_to("", "exists x: Nat. x == 3", "true");
    assert_rewrites_to("", "forall x: Pos. exists y: Pos. x == y + 1", "false");
    assert_rewrites_to("", "forall x: Pos. exists y1, y2: Pos. x == y1 + y2", "false");

    assert_rewrites_to("", "forall n: Nat. n < 2", "false");
    assert_rewrites_to("", "exists n: Nat. n < 2", "true");
}
