//! Whole-PRES-specification type-checking tests: global variables, propositional-variable
//! equations, and `init`, on top of the data-specification checking `data_specification_test.rs`
//! already covers. Mirrors `pbes_specification_test.rs`, adapted for a PRES's real-valued
//! embedded data expressions and its extra constructs (`eqinf`/`eqninf`, `condsm`/`condeq`,
//! constant multiplication, `inf`/`sup`/`sum`).

use merc_syntax::UntypedPres;
use merc_typecheck::PresError;
use merc_typecheck::PresSpecification;

/// Type checks `text`, asserting it is accepted.
#[track_caller]
fn check_ok(text: &str) {
    let spec = UntypedPres::parse(text).expect("the specification should parse");
    if let Err(error) = PresSpecification::from_untyped(spec) {
        panic!("expected the specification to type check:\n{text}\nerror: {error}");
    }
}

/// Type checks `text`, returning the error for the caller to match on the specific variant.
#[track_caller]
fn check_err(text: &str) -> PresError {
    let spec = UntypedPres::parse(text).expect("the specification should parse");
    match PresSpecification::from_untyped(spec) {
        Err(error) => error,
        Ok(_) => panic!("expected the specification to be rejected:\n{text}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_true_and_false_are_accepted() {
    check_ok("pres mu X = true; init X;");
    check_ok("pres mu X = false; init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_val_expr_against_real_is_accepted() {
    check_ok("pres mu X = val(1); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_multiple_equations_and_cross_reference_are_accepted() {
    check_ok("pres mu X = Y; nu Y = X; init Y;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_eqinf_and_eqninf_are_accepted() {
    check_ok("pres mu X = eqinf(val(1)); init X;");
    check_ok("pres mu X = eqninf(val(1)); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_condsm_and_condeq_are_accepted() {
    check_ok("pres mu X = condsm(val(1), val(2), val(3)); init X;");
    check_ok("pres mu X = condeq(val(1), val(2), val(3)); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_is_accepted_on_both_sides() {
    check_ok("pres mu X = val(2) * X; init X;");
    check_ok("pres mu X = X * val(2); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_constant_multiply_rejects_a_non_real_constant() {
    let error = check_err("pres mu X = val(true) * X; init X;");
    assert!(matches!(error, PresError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_inf_sup_sum_binders_are_accepted() {
    check_ok("pres mu X = inf n: Nat . val(n); init X;");
    check_ok("pres mu X = sup n: Nat . val(n); init X;");
    check_ok("pres mu X = sum n: Nat . val(n); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_addition_between_pres_expressions_is_accepted() {
    check_ok("pres mu X(n: Nat) = val(n) + X(n); init X(0);");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_self_recursive_prop_var_inst_is_accepted() {
    check_ok("pres nu X(n: Nat) = val(n) || X(n); init X(0);");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_prop_var_inst_argument_upcasts_like_any_other_expression() {
    check_ok("pres mu X(n: Nat) = val(n); init X(1);");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_propositional_variable_is_rejected() {
    let error = check_err("pres mu X = true; init Y;");
    assert!(
        matches!(error, PresError::UndeclaredPropositionalVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_propositional_variable_lists_declared_names_as_candidates() {
    let error = check_err("pres mu X = true; init Y;");
    let PresError::UndeclaredPropositionalVariable { candidates, .. } = &error else {
        panic!("expected PresError::UndeclaredPropositionalVariable, got {error:?}");
    };
    assert_eq!(candidates, &["X".to_string()]);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_undeclared_propositional_variable_inside_a_formula_is_rejected() {
    let error = check_err("pres mu X = Y; init X;");
    assert!(
        matches!(error, PresError::UndeclaredPropositionalVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_prop_var_inst_arity_mismatch_is_rejected() {
    let error = check_err("pres mu X(n: Nat) = true; init X(1, 2);");
    assert!(matches!(error, PresError::ArityMismatch { .. }), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_prop_var_inst_argument_sort_mismatch_is_rejected() {
    let error = check_err("pres mu X(n: Nat) = true; init X(true);");
    assert!(matches!(error, PresError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_is_in_scope_for_its_body() {
    check_ok("pres mu X = sum n: Nat . val(n); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bound_variable_is_out_of_scope_outside_it() {
    let error = check_err("pres mu X = (sum n: Nat . val(n)) + val(n); init X;");
    assert!(matches!(error, PresError::Inference(_)), "got {error:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_equation_parameter_is_in_scope_for_its_own_formula() {
    check_ok("pres mu X(n: Nat) = val(n); init X(1);");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_global_variable_is_in_scope_in_a_formula_and_init() {
    check_ok("glob n: Nat; pres mu X = val(n); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_equation_parameter_shadows_a_global_of_the_same_name() {
    // `X`'s own `n: Bool` parameter shadows the global `n: Nat`.
    check_ok("glob n: Nat; pres mu X(n: Bool) = condsm(val(1), val(2), val(3)); init X(true);");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_equation_parameter_is_rejected() {
    let error = check_err("pres mu X(n: Nat, n: Bool) = true; init X(1, true);");
    assert!(
        matches!(error, PresError::DuplicateEquationParameter { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_global_variable_is_rejected() {
    let error = check_err("glob n, n: Nat; pres mu X = true; init X;");
    assert!(
        matches!(error, PresError::DuplicateGlobalVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_duplicate_propositional_variable_is_rejected() {
    let error = check_err("pres mu X = true; mu X = false; init X;");
    assert!(
        matches!(error, PresError::DuplicatePropositionalVariable { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_anonymous_struct_in_global_variable_is_rejected() {
    let error = check_err("glob g: struct x | y; pres mu X = true; init X;");
    assert!(
        matches!(error, PresError::AnonymousStructInDeclaration { .. }),
        "got {error:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_anonymous_struct_in_equation_parameter_is_rejected() {
    let error = check_err("pres mu X(g: struct x | y) = true; init X(x);");
    assert!(
        matches!(error, PresError::AnonymousStructInDeclaration { .. }),
        "got {error:?}"
    );
}
