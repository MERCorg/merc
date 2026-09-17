//! Whole-state-formula type-checking tests:.

use merc_syntax::UntypedStateFrmSpec;
use merc_typecheck::ModalError;
use merc_typecheck::ModalSpecification;
use merc_typecheck::FormulaType;

/// Type checks `text` under `val_sort`, asserting it is accepted.
#[track_caller]
fn check_ok(text: &str, val_sort: FormulaType) {
    let spec = UntypedStateFrmSpec::parse(text).expect("the specification should parse");
    if let Err(error) = ModalSpecification::from_untyped(spec, val_sort) {
        panic!("expected the specification to type check under {val_sort:?}:\n{text}\nerror: {error}");
    }
}

/// Type checks `text` under `val_sort`, returning the error for the caller to match on the
/// specific variant.
#[track_caller]
fn check_err(text: &str, val_sort: FormulaType) -> ModalError {
    let spec = UntypedStateFrmSpec::parse(text).expect("the specification should parse");
    match ModalSpecification::from_untyped(spec, val_sort) {
        Err(error) => error,
        Ok(_) => panic!("expected the specification to be rejected under {val_sort:?}:\n{text}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_true_and_false_are_accepted() {
    check_ok("true", FormulaType::Bool);
    check_ok("false", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_a_formula_without_any_val_expr_accepts_either_val_sort() {
    check_ok("true", FormulaType::Real);
    check_ok("true", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_real_is_accepted() {
    check_ok("val(1)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_real_only_val_expr_is_rejected_under_bool_val_sort() {
    // `1` doesn't fit `Bool`, and `Bool` never falls back to trying `Real`.
    let error = check_err("val(1)", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_bool_is_accepted_under_either_val_sort() {
    // `true` doesn't fit `Real` on its own, but `Real` is a superset of `Bool` (see `ValSort`'s
    // doc comment), so a `Bool`-valued `val(...)` is accepted under `Real` too.
    check_ok("val(true)", FormulaType::Bool);
    check_ok("val(true)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_rejects_a_value_matching_neither_real_nor_bool() {
    // Under `Real`, `undeclared` fails identically against both `Real` and `Bool`; both attempts'
    // causes are reported together via `NoMatchingValSort`.
    let error = check_err("val(undeclared)", FormulaType::Real);
    assert!(matches!(error, ModalError::NoMatchingValSort { .. }), "got {error:?}");

    // Under `Bool`, only `Bool` is ever tried, so the underlying mismatch is reported directly.
    let error = check_err("val(undeclared)", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_every_val_expr_in_a_formula_must_fit_the_declared_val_sort() {
    check_ok("val(1) && val(2)", FormulaType::Real);
    check_ok("val(true) && val(false)", FormulaType::Bool);
    // Both `Bool`-valued, so also accepted under `Real` (see `ValSort`'s doc comment).
    check_ok("val(true) && val(false)", FormulaType::Real);

    // `1`/`2` don't fit `Bool`, and `Bool` never falls back to trying `Real`.
    let error = check_err("val(1) && val(2)", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bool_val_expr_is_accepted_alongside_a_real_one_under_real_val_sort() {
    // `val(true)` only fits `Bool` on its own, but that's still accepted under `Real` — verified
    // against `lps2pres` (see `ValSort`'s doc comment).
    check_ok("val(1) && val(true)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_real_val_expr_is_rejected_alongside_a_bool_one_under_bool_val_sort() {
    let error = check_err("val(true) && val(1)", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_delay_and_yaled_with_and_without_a_time_are_accepted() {
    check_ok("delay", FormulaType::Bool);
    check_ok("yaled", FormulaType::Bool);
    check_ok("delay@(1)", FormulaType::Bool);
    check_ok("yaled@(1)", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_is_accepted_regardless_of_val_sort() {
    check_ok("val(2) * (mu X. X)", FormulaType::Real);
    check_ok("(mu X. X) * val(2)", FormulaType::Real);
    // The multiplier's own constant is always checked against `Real`, independently of the
    // formula's declared `val_sort`.
    check_ok("val(2) * (mu X. X)", FormulaType::Bool);
    check_ok("(mu X. X) * val(2)", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_rejects_a_non_real_constant() {
    let error = check_err("val(true) * (mu X. X)", FormulaType::Real);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_after_a_bool_val_expr_is_accepted() {
    // `val(true)` fits the declared `Bool` sort, but a later multiplier is still accepted —
    // verified against `lps2pres`, which embeds `true` as the top of the real-valued PRES lattice
    // rather than rejecting the mix (see `ValSort`'s doc comment).
    check_ok("val(true) && val(2) * (mu X. X)", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_a_val_expr_matching_both_sorts_is_accepted_under_either_val_sort() {
    // `f`'s two overloads make `f(0)` fit both `Real` and `Bool`.
    check_ok("map f: Nat -> Real; map f: Nat -> Bool; form val(f(0));", FormulaType::Real);
    check_ok("map f: Nat -> Real; map f: Nat -> Bool; form val(f(0));", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_forall_exists_and_inf_sup_sum_binders_are_accepted() {
    // `n: Nat` upcasts to `Real`, not `Bool`.
    check_ok("forall n: Nat . val(n)", FormulaType::Real);
    check_ok("exists n: Nat . val(n)", FormulaType::Real);
    check_ok("inf n: Nat . val(n)", FormulaType::Real);
    check_ok("sup n: Nat . val(n)", FormulaType::Real);
    check_ok("sum n: Nat . val(n)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_is_out_of_scope_outside_it() {
    // `n` is out of scope regardless of which sort it's checked against, so both the `Real` and
    // `Bool` attempts fail identically with the same `UndeclaredName`, surfaced together via
    // `NoMatchingValSort`.
    let error = check_err("(exists n: Nat . val(n)) && val(n)", FormulaType::Real);
    let ModalError::NoMatchingValSort {
        real_cause, bool_cause, ..
    } = &error
    else {
        panic!("expected ModalError::NoMatchingValSort, got {error:?}");
    };
    assert!(matches!(**real_cause, ModalError::Inference(_)), "got {real_cause:?}");
    assert!(matches!(**bool_cause, ModalError::Inference(_)), "got {bool_cause:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_declared_action_inside_a_modality_is_accepted() {
    check_ok("act a: Nat; form <a(1)>true;", FormulaType::Bool);
    check_ok("act a: Nat; form [a(1)]true;", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_action_is_rejected() {
    let error = check_err("form <a>true;", FormulaType::Bool);
    assert!(matches!(error, ModalError::UndeclaredAction { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_action_lists_declared_actions_as_candidates() {
    let error = check_err("act b: Nat; form <a(1)>true;", FormulaType::Bool);
    let ModalError::UndeclaredAction { candidates, .. } = &error else {
        panic!("expected ModalError::UndeclaredAction, got {error:?}");
    };
    assert_eq!(candidates, &["b".to_string()]);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_argument_sort_mismatch_is_rejected() {
    let error = check_err("act a: Nat; form <a(true)>true;", FormulaType::Bool);
    assert!(matches!(error, ModalError::NoMatchingOverload { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_data_val_expr_against_bool_is_accepted() {
    check_ok(
        "act a: Nat; form exists x: Nat . <a(x) || val(x > 0)>true;",
        FormulaType::Bool,
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_data_val_expr_rejects_a_non_bool_value() {
    let error = check_err("[val(1)]true", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_at_is_accepted() {
    check_ok("act a: Nat; form <a(1)@3>true;", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_formula_at_rejects_a_non_real_time() {
    let error = check_err("act a: Nat; form <a(1)@true>true;", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_self_recursive_fixpoint_variable_is_accepted() {
    check_ok("mu X. [true]X", FormulaType::Bool);
    check_ok("nu X. [true]X", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_with_parameters_is_accepted() {
    check_ok("mu X(n: Nat = 0) . val(n) || X(n)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_parameter_initial_value_upcasts_like_any_other_expression() {
    check_ok("mu X(n: Nat = 1) . val(n)", FormulaType::Real);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_fixpoint_variable_is_rejected() {
    let error = check_err("mu X. Y", FormulaType::Bool);
    assert!(
        matches!(error, ModalError::UndeclaredStateVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_out_of_scope_outside_its_body_is_rejected() {
    let error = check_err("(mu X. true) && X", FormulaType::Bool);
    assert!(
        matches!(error, ModalError::UndeclaredStateVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_variable_arity_mismatch_is_rejected() {
    let error = check_err("mu X(n: Nat = 0) . X(n, n)", FormulaType::Bool);
    assert!(matches!(error, ModalError::ArityMismatch { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_fixpoint_parameter_argument_sort_mismatch_is_rejected() {
    let error = check_err("mu X(n: Nat = 0) . X(true)", FormulaType::Bool);
    assert!(matches!(error, ModalError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_fixpoint_parameter_is_rejected() {
    let error = check_err("mu X(n: Nat = 0, n: Bool = true) . true", FormulaType::Bool);
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
    check_ok("mu X(n: Nat = 0) . [true](nu X. X)", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_action_declaration_with_the_same_domain_collapses_to_one() {
    // Same name and domain restated is harmless, like a repeated `map`/`cons` declaration — not an
    // error, and not left as two identical overload candidates either, which would make any use of
    // `a` spuriously ambiguous.
    check_ok("act a: Nat; a: Nat; form [a(0)]false;", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_overloaded_action_declaration_with_a_different_domain_is_accepted() {
    check_ok("act a: Nat; a: Bool; form true;", FormulaType::Bool);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_anonymous_struct_in_action_declaration_is_rejected() {
    let error = check_err("act a: struct x | y; form <a(x)>true;", FormulaType::Bool);
    assert!(
        matches!(error, ModalError::AnonymousStructInDeclaration { .. }),
        "got {error:?}"
    );
}
