//! End-to-end rewriting tests across both [`NumberEncoding`]s.
//!
//! Each test builds a rewriter from a type-checked mCRL2 data specification and
//! evaluates a closed expression. The two encodings represent numbers
//! differently — a `@c1`/`@cDub` bit chain versus a chain of `@word` machine-number
//! digits — so results are compared through a `Bool` query wherever both
//! encodings must agree, and against the encoding's own literal form when the
//! representation itself is under test.
//!
//! The identities in [`test_multiplication_and_addition`], [`test_div_mod`] and
//! [`test_square_root`] are ported from mCRL2's
//! `libraries/data/test/rewrite_large_numbers_test.cpp`.

use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// Both encodings, for tests that must agree across them.
const ENCODINGS: [NumberEncoding; 2] = [NumberEncoding::Binary, NumberEncoding::MachineWord];

/// Rewrites `expr` (of sort `sort`) to normal form under `encoding`.
///
/// The expression is placed as the right-hand side of an equation for a fresh
/// constant `q`; rewriting the lowered `q` then evaluates it. Going through the
/// specification is what gives the expression its lowered, correctly sorted
/// form — there is no separate entry point for lowering a single expression.
#[track_caller]
fn rewrite(expr: &str, sort: &str, encoding: NumberEncoding) -> String {
    let text = format!("map q: {sort};\neqn q = {expr};");
    let untyped = UntypedDataSpecification::parse(&text).expect("the specification should parse");
    let spec = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .unwrap_or_else(|error| panic!("{encoding:?} should type check `{expr}`: {error:?}"));
    let lowered = spec.lower_data_specification();

    let query = lowered
        .equations()
        .iter()
        .find(|equation| equation.lhs().to_string() == "q")
        .expect("the q equation should be lowered")
        .lhs()
        .protect();

    let rules = RewriteSpecification::from_data_specification(&lowered);
    InnermostRewriter::new(&rules).rewrite(&query).to_string()
}

/// Asserts that the `Bool` expression `expr` rewrites to `true` under both
/// encodings. Booleans have the same representation either way, so this
/// compares the encodings on equal terms.
#[track_caller]
fn assert_holds(expr: &str) {
    for encoding in ENCODINGS {
        assert_holds_under(expr, encoding);
    }
}

/// Asserts that `expr` rewrites to `true` under one specific `encoding`.
///
/// Used for values large enough that the binary encoding, which does arithmetic
/// one bit at a time, becomes impractically slow: evaluating
/// `sqrt(65535) * sqrt(65535) <= 65535` takes ~20 s under `Binary` against
/// ~0.4 s under `MachineWord`, where the native `@word` operations do the work
/// in a single step. Avoiding that blow-up is the point of the machine-word
/// encoding, so these cases are asserted only where they are meaningful.
#[track_caller]
fn assert_holds_under(expr: &str, encoding: NumberEncoding) {
    assert_eq!(
        rewrite(expr, "Bool", encoding),
        "true",
        "`{expr}` should hold under {encoding:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_arithmetic_agrees_across_encodings() {
    assert_holds("1 + 1 == 2");
    assert_holds("2 * 3 == 6");
    assert_holds("10 div 3 == 3");
    assert_holds("10 mod 3 == 1");
    assert_holds("Int2Nat(7 - 2) == 5");
    assert_holds("3 - 10 == -7");
    assert_holds("succ(41) == 42");
    assert_holds("max(3, 9) == 9");
    assert_holds("min(3, 9) == 3");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_comparisons_agree_across_encodings() {
    assert_holds("3 < 5");
    assert_holds("!(5 < 3)");
    assert_holds("5 > 3");
    assert_holds("7 >= 7");
    assert_holds("7 <= 7");
    assert_holds("10 == 10");
    assert_holds("2 != 3");
}

/// `(x+y)*(x-y) == x*x - y*y`, mirroring mCRL2's `multiplication_and_addition_test`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_multiplication_and_addition() {
    for (x, y) in [(107u64, 10u64), (500, 37)] {
        assert_holds(&format!("({x} + {y}) * ({x} - {y}) == {x} * {x} - {y} * {y}"));
    }
}

/// The same identity on numbers too large for the binary encoding to evaluate
/// in reasonable time.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_multiplication_and_addition_large() {
    for (x, y) in [(99999u64, 12345u64), (123456789, 987654)] {
        assert_holds_under(
            &format!("({x} + {y}) * ({x} - {y}) == {x} * {x} - {y} * {y}"),
            NumberEncoding::MachineWord,
        );
    }
}

/// `x == (x div y)*y + x mod y`, mirroring mCRL2's `mod_and_div_test`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_div_mod() {
    for (x, y) in [(235u64, 78u64), (1000, 7)] {
        assert_holds(&format!("{x} == ({x} div {y}) * {y} + {x} mod {y}"));
    }
}

/// The div/mod identity on larger numbers, machine-word only.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_div_mod_large() {
    for (x, y) in [(123456789u64, 1000u64), (98765432109876, 12345)] {
        assert_holds_under(
            &format!("{x} == ({x} div {y}) * {y} + {x} mod {y}"),
            NumberEncoding::MachineWord,
        );
    }
}

/// `r*r <= x` and `x < (r+1)*(r+1)` for `r = sqrt(x)`, mirroring mCRL2's
/// `square_root_test`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_square_root() {
    for x in [0u64, 1, 17, 831] {
        assert_holds(&format!("sqrt({x}) * sqrt({x}) <= {x}"));
        assert_holds(&format!("{x} < (sqrt({x}) + 1) * (sqrt({x}) + 1)"));
    }
}

/// The square-root bounds on larger numbers, machine-word only.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_square_root_large() {
    for x in [65535u64, 1000000, 4294967295] {
        assert_holds_under(&format!("sqrt({x}) * sqrt({x}) <= {x}"), NumberEncoding::MachineWord);
        assert_holds_under(
            &format!("{x} < (sqrt({x}) + 1) * (sqrt({x}) + 1)"),
            NumberEncoding::MachineWord,
        );
    }
}

/// Numbers that need more than one 64-bit digit exercise the multi-digit chains
/// and the native carry/borrow word operations. These are far past what the
/// binary encoding can evaluate in reasonable time.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_numbers_spanning_multiple_machine_words() {
    let machine_word = NumberEncoding::MachineWord;

    // 2^64, the first value needing two digits, and 2^64 - 1, the largest single digit.
    assert_holds_under("18446744073709551615 + 1 == 18446744073709551616", machine_word);
    assert_holds_under("18446744073709551616 - 1 == 18446744073709551615", machine_word);
    assert_holds_under("18446744073709551616 == 4294967296 * 4294967296", machine_word);
    assert_holds_under("18446744073709551616 div 4294967296 == 4294967296", machine_word);
    assert_holds_under("sqrt(18446744073709551616) == 4294967296", machine_word);
    // 2^128, needing three digits.
    assert_holds_under(
        "340282366920938463463374607431768211456 == 18446744073709551616 * 18446744073709551616",
        machine_word,
    );
}

/// The normal form itself differs: the same value is a bit chain in one encoding
/// and a machine-word digit chain in the other.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_normal_forms_use_the_selected_representation() {
    // 4 == 0b100.
    assert_eq!(
        rewrite("2 + 2", "Nat", NumberEncoding::Binary),
        "@cNat(@cDub(false, @cDub(false, @c1)))"
    );
    assert_eq!(
        rewrite("2 + 2", "Nat", NumberEncoding::MachineWord),
        "@most_significant_digitNat(4)"
    );

    // sqrt(17) == 4 in both, still in each encoding's own representation.
    assert_eq!(
        rewrite("sqrt(17)", "Nat", NumberEncoding::Binary),
        "@cNat(@cDub(false, @cDub(false, @c1)))"
    );
    assert_eq!(
        rewrite("sqrt(17)", "Nat", NumberEncoding::MachineWord),
        "@most_significant_digitNat(4)"
    );
}

/// A value larger than one machine word is a two-digit chain whose digits are
/// the base-2^64 decomposition.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_multi_digit_normal_form() {
    // 2^64 + 5 == 1 * 2^64 + 5. The literal lowers through `Pos2Nat`, which the
    // `nat64.mcrl2` equations push into the chain, leaving a pure `Nat` chain.
    assert_eq!(
        rewrite("18446744073709551621", "Nat", NumberEncoding::MachineWord),
        "@concat_digit(@most_significant_digitNat(1), 5)"
    );
}
