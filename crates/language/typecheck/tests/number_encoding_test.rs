//! Tests for the [`NumberEncoding`] option, which selects the system specification pulled in for
//! the numeric sorts. How numeric literals are lowered under each encoding is tested in
//! `merc_lowering`.

use merc_syntax::SourceMap;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::NumberEncoding;

/// Type checks `text` under `encoding`.
#[track_caller]
fn typed(text: &str, encoding: NumberEncoding) -> DataSpecification {
    let untyped = UntypedDataSpecification::parse(text).expect("the specification should parse");
    DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new())
        .unwrap_or_else(|error| panic!("{encoding:?} should type check:\n{text}\nerror: {error:?}"))
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_binary_is_the_default() {
    assert_eq!(NumberEncoding::default(), NumberEncoding::Binary);
    assert!(!NumberEncoding::Binary.is_machine_word());
    assert!(NumberEncoding::MachineWord.is_machine_word());

    // `from_untyped` keeps using the default encoding.
    let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Nat;").unwrap()).unwrap();
    assert_eq!(spec.number_encoding(), NumberEncoding::Binary);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_encoding_is_recorded() {
    for encoding in [NumberEncoding::Binary, NumberEncoding::MachineWord] {
        assert_eq!(typed("map f: Nat;", encoding).number_encoding(), encoding);
    }
}

/// The machine-word encoding pulls in `machine_word.mcrl2`, which declares the
/// `@word` digit sort; the binary encoding has no such sort.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_system_specification_differs_per_encoding() {
    let declares_word = |encoding| {
        typed("map f: Nat;", encoding)
            .system_defined_specification()
            .sort_declarations
            .iter()
            .any(|declaration| declaration.identifier == "@word")
    };

    assert!(
        !declares_word(NumberEncoding::Binary),
        "the binary encoding has no @word"
    );
    assert!(
        declares_word(NumberEncoding::MachineWord),
        "the machine-word encoding declares @word"
    );
}

/// The machine-word system specification is strictly larger: the `*64`
/// templates define the digit operations on top of the same interface.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_machine_word_specification_has_more_equations() {
    let count = |encoding| {
        typed("map f: Nat;", encoding)
            .system_defined_specification()
            .equation_declarations
            .iter()
            .map(|block| block.equations.len())
            .sum::<usize>()
    };

    assert!(
        count(NumberEncoding::MachineWord) > count(NumberEncoding::Binary),
        "expected the machine-word specification to define more equations"
    );
}
