//! Raising lowered data expressions back to mCRL2 text, see `merc_lowering::Pretty`.

use merc_lowering::Pretty;
use merc_lowering::lower_expression;
use merc_syntax::DataExpr;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;
use test_case::test_case;

/// Type checks and lowers `expr_text` against `spec_text`, and pretty prints the result.
#[track_caller]
fn lower_and_raise(spec_text: &str, expr_text: &str, encoding: NumberEncoding) -> String {
    let untyped = UntypedDataSpecification::parse(spec_text).expect("the specification should parse");
    let mut spec = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .expect("the specification should type check");
    let expr = DataExpr::parse(expr_text).expect("the expression should parse");
    let typed = spec
        .typecheck_expression(&expr)
        .unwrap_or_else(|err| panic!("'{expr_text}' should type check: {err}"));
    Pretty::from(&lower_expression(&spec, &typed)).to_string()
}

const SPEC: &str = "map f: Nat -> Nat; g: Int # Int -> Bool;";

/// Lowering and then raising an expression prints it as it was written.
#[test_case("1" ; "one")]
#[test_case("0" ; "zero")]
#[test_case("12345" ; "number")]
#[test_case("18446744073709551617" ; "beyond one machine word")]
#[test_case("123456789012345678901234567890" ; "beyond two machine words")]
#[test_case("-5 < 3" ; "negative")]
#[test_case("f(2) + 3 * f(4)" ; "precedence")]
#[test_case("(f(2) + 3) * f(4)" ; "parentheses kept")]
#[test_case("!(true && false) || g(1, -2)" ; "booleans")]
#[test_case("forall m: Nat. f(m) > 0" ; "quantifier")]
#[test_case("[1, 2] ++ [3]" ; "lists")]
#[test_case("x + 1 whr x = 2 end" ; "where clause")]
#[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
fn test_raise(input: &str) {
    for encoding in [NumberEncoding::Binary, NumberEncoding::MachineWord] {
        assert_eq!(lower_and_raise(SPEC, input, encoding), input, "{encoding:?}");
    }
}
