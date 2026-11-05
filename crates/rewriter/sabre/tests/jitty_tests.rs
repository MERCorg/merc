//! The [JittyRewriter] on mCRL2 data specifications: it reaches the same normal forms as the
//! [InnermostRewriter], but rewrites only the arguments its strategies need. The specifications
//! that merc copies from mCRL2 rely on that, for instance for `if` in the comparison of binary
//! numbers and in `exp` on machine words.

use merc_data::DataExpression;
use merc_sabre::InnermostRewriter;
use merc_sabre::JittyRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// Returns the rewrite specification of `spec` with `goal` as the right-hand side of a constant
/// `b: Bool`, and the term `b`, lowered under `encoding`.
fn lower(spec: &str, goal: &str, encoding: NumberEncoding) -> (RewriteSpecification, DataExpression) {
    let untyped = UntypedDataSpecification::parse(&format!("{spec}\nmap b: Bool;\neqn b = {goal};")).unwrap();
    let lowered = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .unwrap()
        .lower_data_specification();
    let b = lowered
        .equations()
        .iter()
        .find(|equation| equation.lhs().to_string() == "b")
        .expect("no equation for b")
        .lhs()
        .protect();
    (RewriteSpecification::from_data_specification(&lowered), b)
}

/// Asserts that `goal` rewrites to `true` with the jitty rewriter, and returns its rewrite steps.
#[track_caller]
fn jitty_steps(spec: &str, goal: &str, encoding: NumberEncoding) -> usize {
    let (rules, b) = lower(spec, goal, encoding);
    let mut rewriter = JittyRewriter::new(&rules);
    let result = rewriter.rewrite(&b);
    assert_eq!(result, DataExpression::bool_literal(true), "{goal} under {encoding:?}");
    rewriter.metrics().rewrite_steps
}

const UPTO: &str = "
map upto: Nat # Nat -> Nat;
var n, k: Nat;
eqn upto(n, k) = if(n < k, upto(n + 1, k), n);
";

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_agrees_with_innermost() {
    let goals = [
        "3 + 4 == 7",
        "15 < 16 && !(16 < 15)",
        "max(3, 7) == 7 && min(3, 7) == 3",
        "17 div 5 == 3 && 17 mod 5 == 2",
        "sqrt(17) == 4",
        "[3, 1, 2] . 1 == 1",
    ];

    for encoding in [NumberEncoding::Binary, NumberEncoding::MachineWord] {
        for goal in goals {
            let (rules, b) = lower("", goal, encoding);
            let mut innermost_rewriter = InnermostRewriter::new(&rules);
            let innermost = innermost_rewriter.rewrite(&b);
            let innermost_stats = innermost_rewriter.metrics();
            let mut jitty_rewriter = JittyRewriter::new(&rules);
            let jitty = jitty_rewriter.rewrite(&b);
            let jitty_stats = jitty_rewriter.metrics();

            assert_eq!(
                innermost,
                DataExpression::bool_literal(true),
                "{goal} under {encoding:?}"
            );
            assert_eq!(jitty, innermost, "{goal} under {encoding:?}");
            assert!(
                jitty_stats.rewrite_steps <= innermost_stats.rewrite_steps,
                "{goal} under {encoding:?}: jitty took {} steps, innermost {}",
                jitty_stats.rewrite_steps,
                innermost_stats.rewrite_steps
            );
        }
    }
}

/// `@cDub(b,p) < @cDub(c,q) = if(c => b, p < q, p <= q)` recurses into both branches under
/// innermost rewriting, which takes `2v - 2` steps for `v - 1 < v`. The jitty rewriter only
/// rewrites the selected branch, so the steps grow with the number of bits.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_binary_comparison_is_linear_in_the_number_of_bits() {
    let steps = |bits: u32| {
        let v = 1u64 << bits;
        jitty_steps("", &format!("{} < {v}", v - 1), NumberEncoding::Binary)
    };

    let (small, large) = (steps(4), steps(20));
    assert!(large < 6 * small, "{small} steps for 4 bits, {large} for 20 bits");
}

/// The else branch of `@exp_aux3n(false, n, w) = if(@equals_zero_word(w), .., @exp_aux3n(..,
/// @shift_right(false, w)))` calls itself again once `w` is zero, so innermost rewriting does
/// not terminate.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_exp_terminates_under_machine_words() {
    jitty_steps("", "exp(3, 5) == 243", NumberEncoding::MachineWord);
    jitty_steps("", "exp(3, 400) mod 1000003 == 313558", NumberEncoding::MachineWord);
}

/// A tail call in a branch stays in the loop of the stack machine, so a long loop does not
/// grow the system stack.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_long_loop_does_not_overflow_the_stack() {
    // The consistency checks of debug builds are quadratic in the stack size.
    let iterations = if cfg!(debug_assertions) { 20_000 } else { 1_000_000 };
    jitty_steps(
        UPTO,
        &format!("upto(0, {iterations}) == {iterations}"),
        NumberEncoding::MachineWord,
    );
}
