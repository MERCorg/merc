//! Whole-state-formula type-checking tests:.

use merc_syntax::UntypedStateFrmSpec;
use merc_typecheck::ModalError;
use merc_typecheck::ModalSpecification;

/// Type checks `text`, asserting it is accepted.
#[track_caller]
fn check_ok(text: &str) {
    let spec = UntypedStateFrmSpec::parse(text).expect("the specification should parse");
    if let Err(error) = ModalSpecification::from_untyped(spec) {
        panic!("expected the specification to type check:\n{text}\nerror: {error}");
    }
}

/// Type checks `text`, returning the error for the caller to match on the specific variant.
#[track_caller]
fn check_err(text: &str) -> ModalError {
    let spec = UntypedStateFrmSpec::parse(text).expect("the specification should parse");
    match ModalSpecification::from_untyped(spec) {
        Err(error) => error,
        Ok(_) => panic!("expected the specification to be rejected:\n{text}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_true_and_false_are_accepted() {
    check_ok("true");
    check_ok("false");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_real_is_accepted() {
    check_ok("val(1)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_rejects_a_non_real_value() {
    // `true` is `Bool`, not upcastable to `Real`.
    let error = check_err("val(true)");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_delay_and_yaled_with_and_without_a_time_are_accepted() {
    check_ok("delay");
    check_ok("yaled");
    check_ok("delay@(1)");
    check_ok("yaled@(1)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_is_accepted_on_both_sides() {
    check_ok("val(2) * (mu X. X)");
    check_ok("(mu X. X) * val(2)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_rejects_a_non_real_constant() {
    let error = check_err("val(true) * (mu X. X)");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_forall_exists_and_inf_sup_sum_binders_are_accepted() {
    check_ok("forall n: Nat . val(n)");
    check_ok("exists n: Nat . val(n)");
    check_ok("inf n: Nat . val(n)");
    check_ok("sup n: Nat . val(n)");
    check_ok("sum n: Nat . val(n)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_is_out_of_scope_outside_it() {
    let error = check_err("(exists n: Nat . val(n)) && val(n)");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_declared_action_inside_a_modality_is_accepted() {
    check_ok("act a: Nat; form <a(1)>true;");
    check_ok("act a: Nat; form [a(1)]true;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_action_is_rejected() {
    let error = check_err("form <a>true;");
    assert!(matches!(error, ModalError::UndeclaredAction { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_argument_sort_mismatch_is_rejected() {
    let error = check_err("act a: Nat; form <a(true)>true;");
    assert!(matches!(error, ModalError::NoMatchingOverload { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_data_val_expr_against_bool_is_accepted() {
    check_ok("act a: Nat; form exists x: Nat . <a(x) || val(x > 0)>true;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_data_val_expr_rejects_a_non_bool_value() {
    let error = check_err("[val(1)]true");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_self_recursive_fixpoint_variable_is_accepted() {
    check_ok("mu X. [true]X");
    check_ok("nu X. [true]X");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_with_parameters_is_accepted() {
    check_ok("mu X(n: Nat = 0) . val(n) || X(n)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_parameter_initial_value_upcasts_like_any_other_expression() {
    check_ok("mu X(n: Nat = 1) . val(n)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_fixpoint_variable_is_rejected() {
    let error = check_err("mu X. Y");
    assert!(
        matches!(error, ModalError::UndeclaredStateVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_out_of_scope_outside_its_body_is_rejected() {
    let error = check_err("(mu X. true) && X");
    assert!(
        matches!(error, ModalError::UndeclaredStateVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_arity_mismatch_is_rejected() {
    let error = check_err("mu X(n: Nat = 0) . X(n, n)");
    assert!(matches!(error, ModalError::ArityMismatch { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_parameter_argument_sort_mismatch_is_rejected() {
    let error = check_err("mu X(n: Nat = 0) . X(true)");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_fixpoint_parameter_is_rejected() {
    let error = check_err("mu X(n: Nat = 0, n: Bool = true) . true");
    assert!(
        matches!(error, ModalError::DuplicateFixedPointParameter { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_nested_fixpoint_variable_shadows_the_outer_one() {
    // The inner `X` (arity 0) shadows the outer `X(n: Nat)`; referencing the bare `X` inside the
    // inner scope must resolve to the inner declaration, not fail an arity check against the
    // outer one.
    check_ok("mu X(n: Nat = 0) . [true](nu X. X)");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_anonymous_struct_in_action_declaration_is_rejected() {
    let error = check_err("act a: struct x | y; form <a(x)>true;");
    assert!(
        matches!(error, ModalError::AnonymousStructInDeclaration { .. }),
        "got {error:?}"
    );
}
