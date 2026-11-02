//! End-to-end tests that the [InnermostRewriter] natively evaluates the
//! machine-word (`@word`) operations, which carry no rewrite rules and must be
//! computed directly from their concrete arguments.
//!
//! Operations are applied through their real, declared function symbol
//! (obtained from [`RewriteSpecification::native_symbols`], which is sourced
//! from a type-checked data specification) rather than a hand-built symbol of
//! unknown sort: the dispatch table is keyed by the symbol's hash-consed
//! identity, which depends on its sort, so a mismatched sort would silently
//! miss the table.

use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::MachineNumber;
use merc_data::MachineWordOp;
use merc_sabre::InnermostRewriter;
use merc_sabre::RewriteEngine;
use merc_sabre::RewriteSpecification;
use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// The rewrite specification for the `MachineWord`-encoded system spec, with
/// no user-declared equations. It still carries every native `@word`
/// operation with its real, declared sort, via
/// [`RewriteSpecification::native_symbols`].
fn rules() -> RewriteSpecification {
    let untyped = UntypedDataSpecification::parse("map q: Nat;").expect("the specification should parse");
    let data_spec = DataSpecification::from_untyped_with(untyped, NumberEncoding::MachineWord, &mut SourceMap::new())
        .expect("the MachineWord encoding should type check");
    RewriteSpecification::from_data_specification(&data_spec.lower_data_specification())
}

/// `op` applied to `args`, through its real, correctly-sorted function symbol.
fn apply(rules: &RewriteSpecification, op: MachineWordOp, args: &[DataExpression]) -> DataExpression {
    let (symbol, _) = rules
        .native_symbols()
        .iter()
        .find(|(_, candidate)| *candidate == op)
        .unwrap_or_else(|| panic!("{} should be a native machine-word operation", op.name()));
    DataApplication::with_args(symbol, args).into()
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_word_valued_operations() {
    let rules = rules();
    let mut rewriter = InnermostRewriter::new(&rules);

    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::AddWord,
            &[
                DataExpression::from(MachineNumber::new(3)),
                DataExpression::from(MachineNumber::new(5))
            ]
        )),
        DataExpression::from(MachineNumber::new(8))
    );
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::SuccWord,
            &[DataExpression::from(MachineNumber::new(41))]
        )),
        DataExpression::from(MachineNumber::new(42))
    );
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::DivWord,
            &[
                DataExpression::from(MachineNumber::new(17)),
                DataExpression::from(MachineNumber::new(5))
            ]
        )),
        DataExpression::from(MachineNumber::new(3))
    );
    // Wrapping semantics.
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::AddWord,
            &[
                DataExpression::from(MachineNumber::new(u64::MAX)),
                DataExpression::from(MachineNumber::new(1))
            ]
        )),
        DataExpression::from(MachineNumber::new(0))
    );
    // (2^64 * 1 + 0) div 2 == 2^63.
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::DivDoubleword,
            &[
                DataExpression::from(MachineNumber::new(1)),
                DataExpression::from(MachineNumber::new(0)),
                DataExpression::from(MachineNumber::new(2))
            ]
        )),
        DataExpression::from(MachineNumber::new(1u64 << 63))
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Miri is too slow to run this test.
fn test_nested_operations_reduce_innermost_first() {
    let rules = rules();
    let mut rewriter = InnermostRewriter::new(&rules);

    // (1 + 2) + (17 mod 5) == 3 + 2 == 5. Exercises the machine-number sub-term
    // short-circuit in the innermost rewriter as well as the native dispatch.
    let nested = apply(
        &rules,
        MachineWordOp::AddWord,
        &[
            apply(
                &rules,
                MachineWordOp::AddWord,
                &[
                    DataExpression::from(MachineNumber::new(1)),
                    DataExpression::from(MachineNumber::new(2)),
                ],
            ),
            apply(
                &rules,
                MachineWordOp::ModWord,
                &[
                    DataExpression::from(MachineNumber::new(17)),
                    DataExpression::from(MachineNumber::new(5)),
                ],
            ),
        ],
    );
    assert_eq!(rewriter.rewrite(&nested), DataExpression::from(MachineNumber::new(5)));
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_shift_right_with_bool_argument() {
    let rules = rules();
    let mut rewriter = InnermostRewriter::new(&rules);

    // @shift_right(false, 0b100) == 0b10.
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::ShiftRight,
            &[
                DataExpression::bool_literal(false),
                DataExpression::from(MachineNumber::new(0b100))
            ]
        )),
        DataExpression::from(MachineNumber::new(0b10))
    );
    // @shift_right(true, 0) inserts a new most-significant bit.
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::ShiftRight,
            &[
                DataExpression::bool_literal(true),
                DataExpression::from(MachineNumber::new(0))
            ]
        )),
        DataExpression::from(MachineNumber::new(1u64 << 63))
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Miri is too slow to run this test.
fn test_bool_valued_operations() {
    let rules = rules();
    let mut rewriter = InnermostRewriter::new(&rules);

    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::Less,
            &[
                DataExpression::from(MachineNumber::new(2)),
                DataExpression::from(MachineNumber::new(5))
            ]
        )),
        DataExpression::bool_literal(true)
    );
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::Less,
            &[
                DataExpression::from(MachineNumber::new(5)),
                DataExpression::from(MachineNumber::new(2))
            ]
        )),
        DataExpression::bool_literal(false)
    );
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::Equal,
            &[
                DataExpression::from(MachineNumber::new(7)),
                DataExpression::from(MachineNumber::new(7))
            ]
        )),
        DataExpression::bool_literal(true)
    );
    assert_eq!(
        rewriter.rewrite(&apply(
            &rules,
            MachineWordOp::EqualsZeroWord,
            &[DataExpression::from(MachineNumber::new(0))]
        )),
        DataExpression::bool_literal(true)
    );
}

/// A lone machine number is already in normal form and rewrites to itself.
#[test]
#[cfg_attr(miri, ignore)] // Miri is too slow to run this test.
fn test_machine_number_is_normal_form() {
    let rules = rules();
    let mut rewriter = InnermostRewriter::new(&rules);
    assert_eq!(
        rewriter.rewrite(&DataExpression::from(MachineNumber::new(42))),
        DataExpression::from(MachineNumber::new(42))
    );
}
