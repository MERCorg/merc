//! Whole-state-formula type-checking tests:.

use merc_syntax::UntypedStateFrmSpec;
use merc_typecheck::ModalError;
use merc_typecheck::ModalSpecification;
use merc_typecheck::ValSort;

/// Type checks `text`, asserting it is accepted.
#[track_caller]
fn check_ok(text: &str) {
    check_val_sort(text);
}

/// Type checks `text`, asserting it is accepted, and returns the [`ValSort`] its `val(...)`
/// occurrences fixed on.
#[track_caller]
fn check_val_sort(text: &str) -> ValSort {
    let spec = UntypedStateFrmSpec::parse(text).expect("the specification should parse");
    match ModalSpecification::from_untyped(spec) {
        Ok(spec) => spec.val_sort(),
        Err(error) => panic!("expected the specification to type check:\n{text}\nerror: {error}"),
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
fn test_a_formula_without_any_val_expr_has_an_unknown_val_sort() {
    assert_eq!(check_val_sort("true"), ValSort::Unknown);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_real_is_accepted() {
    // `1` fits `Real` (tried first), fixating the formula's `val(...)` sort to `Real`.
    assert_eq!(check_val_sort("val(1)"), ValSort::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_bool_is_accepted() {
    // `true` doesn't fit `Real`; the first `val(...)` in a formula falls back to `Bool`.
    assert_eq!(check_val_sort("val(true)"), ValSort::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_rejects_a_value_matching_neither_real_nor_bool() {
    // `undeclared` fails identically against both `Real` and `Bool`; the `Real` attempt's error
    // (tried first) is the one reported.
    let error = check_err("val(undeclared)");
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_every_val_expr_in_a_formula_shares_the_same_fixed_sort() {
    assert_eq!(check_val_sort("val(1) && val(2)"), ValSort::Real);
    assert_eq!(check_val_sort("val(true) && val(false)"), ValSort::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_a_later_val_expr_must_match_the_sort_the_first_one_fixed() {
    // The first `val(...)` (`1`) fixates `Real`; the second (`true`) doesn't fit `Real`, even
    // though it would fit `Bool` on its own.
    let error = check_err("val(1) && val(true)");
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
fn test_undeclared_action_lists_declared_actions_as_candidates() {
    let error = check_err("act b: Nat; form <a(1)>true;");
    let ModalError::UndeclaredAction { candidates, .. } = &error else {
        panic!("expected ModalError::UndeclaredAction, got {error:?}");
    };
    assert_eq!(candidates, &["b".to_string()]);
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
