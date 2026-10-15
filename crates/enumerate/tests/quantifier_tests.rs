use std::rc::Rc;

use merc_data::DataExpression;
use merc_data::Mcrl2DataSpecification;
use merc_data::bool_literal;
use merc_enumerate::EnumerationPlans;
use merc_enumerate::Enumerator;
use merc_enumerate::FreshVariableGenerator;
use merc_enumerate::QuantifierEnumerator;
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

/// Builds an [InnermostRewriter] with a fresh [QuantifierEnumerator] wired in, so it decides
/// `exists`/`forall` quantifiers over `data_spec`'s sorts.
fn rewriter_with_enumerator(data_spec: &Mcrl2DataSpecification) -> InnermostRewriter<QuantifierEnumerator> {
    let spec = RewriteSpecification::from_data_specification(data_spec);
    let plans = Rc::new(EnumerationPlans::build(data_spec));
    let enumerator = Enumerator::new(plans);
    let generator = FreshVariableGenerator::new("v", std::iter::empty());
    InnermostRewriter::with_enumerator(&spec, QuantifierEnumerator::new(enumerator, generator))
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_exists_over_nat_is_decided_true() {
    let data_spec = lower("exists n: Nat . n == 5");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, bool_literal(true));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_forall_over_nat_is_decided_false() {
    let data_spec = lower("forall n: Nat . n == 5");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, bool_literal(false));
}

/// A quantifier nested directly in another's body.
#[test]
#[cfg_attr(miri, ignore)]
fn test_a_directly_nested_quantifier_is_also_decided() {
    let data_spec = lower("exists n: Nat . n == 0 && forall m: Nat . m == m");
    let body = goal_body(&data_spec);

    let result = rewriter_with_enumerator(&data_spec).rewrite(&body);
    assert_eq!(result, bool_literal(true));
}

// The tests below are ported from mCRL2's `libraries/data/test/quantifier_test.cpp`.

/// Lowers `spec` extended with `b = <expr>` and `e = <expected>`, and checks
/// that rewriting `b` with a [QuantifierEnumerator] yields `e`'s normal form.
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
#[ignore = "ported from mCRL2"]
fn test_quantifiers_over_bool() {
    assert_rewrites_to("", "exists x: Bool. x == false", "true");
    assert_rewrites_to("", "forall x: Bool. x == false", "false");
    assert_rewrites_to("", "exists x: Bool. x == true", "true");
    assert_rewrites_to("", "forall x: Bool. x == true", "false");
    assert_rewrites_to("", "forall x: Bool. x == true && x == false", "false");
    assert_rewrites_to("", "exists x: Bool. x == true && x == false", "false");
    assert_rewrites_to("", "forall x: Bool. x == true || x == false", "true");
    assert_rewrites_to("", "exists x: Bool. x == true || x == false", "true");
    assert_rewrites_to("", "forall x: Bool. exists y: Bool. x == y", "true");
    assert_rewrites_to("", "exists x: Bool. forall y: Bool. x == y", "false");
    assert_rewrites_to("", "forall b: Bool. b", "false");
}

#[test]
#[ignore = "ported from mCRL2"]
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

/// Quantifiers occurring as subterms are eliminated as well.
#[test]
#[ignore = "ported from mCRL2"]
fn test_quantifiers_inside_a_term() {
    assert_rewrites_to("", "(exists x_0: Bool. false) && (forall x_0: Nat. true)", "false");
    assert_rewrites_to("", "(forall x_0: Pos. true) || (exists x_0: Bool. false)", "true");
}

/// `Real` is not enumerable, so these must be left in place rather than decided.
#[test]
#[ignore = "ported from mCRL2"]
fn test_quantifiers_over_real_are_left_undecided() {
    assert_rewrites_to("", "forall r: Real. r < 5", "forall r: Real. r < 5");
    assert_rewrites_to("", "exists r: Real. r >= 10", "exists r: Real. r >= 10");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_quantifiers_over_structs_and_lists() {
    let spec = "sort S = struct s1?is_s1 | s2?is_s2;
                sort T = struct t;
                sort L = List(T);";

    assert_rewrites_to(spec, "exists s: S. is_s1(s) && is_s2(s)", "false");
    assert_rewrites_to(spec, "exists s: S. s == s2 && is_s2(s)", "true");
    assert_rewrites_to(spec, "forall y: T. y in [t]", "true");
}

/// A trivial predicate over a non-enumerable sort is decided by the rewriter
/// alone, without enumerating the bound variable.
#[test]
#[ignore = "ported from mCRL2"]
fn test_trivial_quantifiers_over_set() {
    let spec = "sort A = Set(Bool);";

    assert_rewrites_to(spec, "exists x: Set(Bool). x == x", "true");
    assert_rewrites_to(spec, "forall x: Set(Bool). x == x", "true");
    assert_rewrites_to(spec, "exists x: Set(Bool). x != x", "false");
    assert_rewrites_to(spec, "forall x: Set(Bool). x != x", "false");
}

/// Substituting `y` for `x` in `g`'s body must not capture `g`'s own bound
/// `y`, which would turn `x != y` into `y != y` and make `f` false.
#[test]
#[ignore = "ported from mCRL2"]
fn test_quantifier_in_rewrite_rules_avoids_capture() {
    let spec = "map f: Bool;
                    g: Bool -> Bool;
                var x: Bool;
                eqn f = forall y: Bool. g(y);
                    g(x) = exists y: Bool. x != y;";

    assert_rewrites_to(spec, "f", "true");
}

#[test]
#[ignore = "ported from mCRL2"]
fn test_mixed_enumerable_and_non_enumerable_variables() {
    assert_rewrites_to("", "exists x: Real, p: Pos. p == 1 && x > 4", "exists x: Real. x > 4");
    assert_rewrites_to("", "exists x: Real, p: Pos. p == 1", "true");
    assert_rewrites_to("", "exists x: Real, p: Pos. x > 4", "exists x: Real. x > 4");
}
