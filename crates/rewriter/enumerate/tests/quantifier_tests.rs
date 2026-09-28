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
