use std::mem::ManuallyDrop;

use merc_data::ContainerSortKind;
use merc_data::DataFunctionSymbol;
use merc_data::SortArrow;
use merc_data::SortCons;
use merc_data::SortExpression;

/// `@c0: Nat`, the binary encoding of zero (`nat.mcrl2`, `nat64.mcrl2`).
pub const C0: &str = "@c0";
/// `@c1: Pos`, the binary encoding of one (`pos.mcrl2`).
pub const C1: &str = "@c1";
/// `@cDub: Bool # Pos -> Pos`, where `@cDub(b, p)` denotes `2 * p + b` (`pos.mcrl2`).
pub const C_DUB: &str = "@cDub";
/// `@cNat: Pos -> Nat`, embeds a `Pos` into `Nat` in the binary encoding (`nat.mcrl2`).
pub const C_NAT: &str = "@cNat";
/// `@cInt: Nat -> Int`, embeds a `Nat` into `Int` (`int.mcrl2`, `int64.mcrl2`).
pub const C_INT: &str = "@cInt";
/// `@cReal: Int # Pos -> Real`, the fraction of an `Int` and a `Pos` (`real.mcrl2`, `real64.mcrl2`).
pub const C_REAL: &str = "@cReal";
/// `Pos2Nat: Pos -> Nat`, embeds a `Pos` into `Nat` in the machine-word encoding, which has no
/// `@cNat` constructor (`nat64.mcrl2`).
pub const POS2NAT: &str = "Pos2Nat";
/// `@most_significant_digit: @word -> Pos`, the leading digit of a `Pos` digit chain
/// (`pos64.mcrl2`).
pub const MOST_SIGNIFICANT_DIGIT: &str = "@most_significant_digit";
/// `@most_significant_digitNat: @word -> Nat`, the leading digit of a `Nat` digit chain
/// (`nat64.mcrl2`).
pub const MOST_SIGNIFICANT_DIGIT_NAT: &str = "@most_significant_digitNat";
/// `@concat_digit: Pos # @word -> Pos` and `@concat_digit: Nat # @word -> Nat`, where
/// `@concat_digit(p, w)` denotes `2^64 * p + w` (`pos64.mcrl2`, `nat64.mcrl2`).
pub const CONCAT_DIGIT: &str = "@concat_digit";

/// `[]: List(S)`, the empty list (`list.mcrl2`, `list64.mcrl2`).
pub const EMPTY_LIST: &str = "[]";
/// `{}: FSet(S)`, the empty finite set (`fset.mcrl2`, `fset64.mcrl2`).
pub const EMPTY_FSET: &str = "{}";
/// `{:}: FBag(S)`, the empty finite bag (`fbag.mcrl2`, `fbag64.mcrl2`).
pub const EMPTY_FBAG: &str = "{:}";
/// `@fset_insert: S # FSet(S) -> FSet(S)` (`fset.mcrl2`, `fset64.mcrl2`).
pub const FSET_INSERT: &str = "@fset_insert";
/// `@fbag_cinsert: S # Nat # FBag(S) -> FBag(S)` (`fbag.mcrl2`, `fbag64.mcrl2`).
pub const FBAG_CINSERT: &str = "@fbag_cinsert";
/// `@false_: S -> Bool`, the characteristic function of the empty set (`set.mcrl2`, `set64.mcrl2`).
pub const FALSE_FUNCTION: &str = "@false_";
/// `@zero_: S -> Nat`, the multiplicity function of the empty bag (`bag.mcrl2`, `bag64.mcrl2`).
pub const ZERO_FUNCTION: &str = "@zero_";
/// `@set: (S -> Bool) # FSet(S) -> Set(S)` (`set.mcrl2`, `set64.mcrl2`).
pub const SET_CONSTRUCTOR: &str = "@set";
/// `@bag: (S -> Nat) # FBag(S) -> Bag(S)` (`bag.mcrl2`, `bag64.mcrl2`).
pub const BAG_CONSTRUCTOR: &str = "@bag";

thread_local! {
    static SYSTEM_SYMBOLS: SystemSymbols = SystemSymbols::new();
}

/// The monomorphic symbols, built once per thread.
///
/// The symbols are wrapped in `ManuallyDrop` so that their destructors never run at thread exit.
struct SystemSymbols {
    c0: ManuallyDrop<DataFunctionSymbol>,
    c1: ManuallyDrop<DataFunctionSymbol>,
    c_dub: ManuallyDrop<DataFunctionSymbol>,
    c_nat: ManuallyDrop<DataFunctionSymbol>,
    c_int: ManuallyDrop<DataFunctionSymbol>,
    c_real: ManuallyDrop<DataFunctionSymbol>,
    pos2nat: ManuallyDrop<DataFunctionSymbol>,
    most_significant_digit: ManuallyDrop<DataFunctionSymbol>,
    most_significant_digit_nat: ManuallyDrop<DataFunctionSymbol>,
    concat_digit_pos: ManuallyDrop<DataFunctionSymbol>,
    concat_digit_nat: ManuallyDrop<DataFunctionSymbol>,
}

impl SystemSymbols {
    fn new() -> Self {
        let bool = SortExpression::bool();
        let pos = SortExpression::pos();
        let nat = SortExpression::nat();
        let int = SortExpression::int();
        let real = SortExpression::real();
        let word = SortExpression::machine_word();

        let symbol = |name: &str, domain: &[SortExpression], codomain: &SortExpression| {
            ManuallyDrop::new(if domain.is_empty() {
                DataFunctionSymbol::with_sort(name, codomain.copy())
            } else {
                DataFunctionSymbol::with_signature(name, domain, codomain)
            })
        };

        SystemSymbols {
            c0: symbol(C0, &[], &nat),
            c1: symbol(C1, &[], &pos),
            c_dub: symbol(C_DUB, &[bool, pos.clone()], &pos),
            c_nat: symbol(C_NAT, std::slice::from_ref(&pos), &nat),
            c_int: symbol(C_INT, std::slice::from_ref(&nat), &int),
            c_real: symbol(C_REAL, &[int, pos.clone()], &real),
            pos2nat: symbol(POS2NAT, std::slice::from_ref(&pos), &nat),
            most_significant_digit: symbol(MOST_SIGNIFICANT_DIGIT, std::slice::from_ref(&word), &pos),
            most_significant_digit_nat: symbol(MOST_SIGNIFICANT_DIGIT_NAT, std::slice::from_ref(&word), &nat),
            concat_digit_pos: symbol(CONCAT_DIGIT, &[pos.clone(), word.clone()], &pos),
            concat_digit_nat: symbol(CONCAT_DIGIT, &[nat.clone(), word], &nat),
        }
    }
}

/// Returns a clone of the monomorphic symbol selected by `field`.
fn cached(field: impl FnOnce(&SystemSymbols) -> &ManuallyDrop<DataFunctionSymbol>) -> DataFunctionSymbol {
    SYSTEM_SYMBOLS.with(|symbols| DataFunctionSymbol::clone(field(symbols)))
}

/// See [C0].
pub fn c0() -> DataFunctionSymbol {
    cached(|s| &s.c0)
}

/// See [C1].
pub fn c1() -> DataFunctionSymbol {
    cached(|s| &s.c1)
}

/// See [C_DUB].
pub fn c_dub() -> DataFunctionSymbol {
    cached(|s| &s.c_dub)
}

/// See [C_NAT].
pub fn c_nat() -> DataFunctionSymbol {
    cached(|s| &s.c_nat)
}

/// See [C_INT].
pub fn c_int() -> DataFunctionSymbol {
    cached(|s| &s.c_int)
}

/// See [C_REAL].
pub fn c_real() -> DataFunctionSymbol {
    cached(|s| &s.c_real)
}

/// See [POS2NAT].
pub fn pos2nat() -> DataFunctionSymbol {
    cached(|s| &s.pos2nat)
}

/// See [MOST_SIGNIFICANT_DIGIT].
pub fn most_significant_digit() -> DataFunctionSymbol {
    cached(|s| &s.most_significant_digit)
}

/// See [MOST_SIGNIFICANT_DIGIT_NAT].
pub fn most_significant_digit_nat() -> DataFunctionSymbol {
    cached(|s| &s.most_significant_digit_nat)
}

/// The `Pos` overload of [CONCAT_DIGIT].
pub fn concat_digit_pos() -> DataFunctionSymbol {
    cached(|s| &s.concat_digit_pos)
}

/// The `Nat` overload of [CONCAT_DIGIT].
pub fn concat_digit_nat() -> DataFunctionSymbol {
    cached(|s| &s.concat_digit_nat)
}

/// See [EMPTY_LIST], instantiated for `element`.
pub fn empty_list(element: &SortExpression) -> DataFunctionSymbol {
    let list: SortExpression = SortCons::new(ContainerSortKind::List, element.clone()).into();
    DataFunctionSymbol::with_sort(EMPTY_LIST, list.copy())
}

/// See [EMPTY_FSET], instantiated for `element`.
pub fn empty_fset(element: &SortExpression) -> DataFunctionSymbol {
    DataFunctionSymbol::with_sort(EMPTY_FSET, fset(element).copy())
}

/// See [EMPTY_FBAG], instantiated for `element`.
pub fn empty_fbag(element: &SortExpression) -> DataFunctionSymbol {
    DataFunctionSymbol::with_sort(EMPTY_FBAG, fbag(element).copy())
}

/// See [FSET_INSERT], instantiated for `element`.
pub fn fset_insert(element: &SortExpression) -> DataFunctionSymbol {
    let fset = fset(element);
    DataFunctionSymbol::with_signature(FSET_INSERT, &[element.clone(), fset.clone()], &fset)
}

/// See [FBAG_CINSERT], instantiated for `element`.
pub fn fbag_cinsert(element: &SortExpression) -> DataFunctionSymbol {
    let fbag = fbag(element);
    DataFunctionSymbol::with_signature(
        FBAG_CINSERT,
        &[element.clone(), SortExpression::nat(), fbag.clone()],
        &fbag,
    )
}

/// See [FALSE_FUNCTION], instantiated for `element`.
pub fn false_function(element: &SortExpression) -> DataFunctionSymbol {
    DataFunctionSymbol::with_signature(FALSE_FUNCTION, std::slice::from_ref(element), &SortExpression::bool())
}

/// See [ZERO_FUNCTION], instantiated for `element`.
pub fn zero_function(element: &SortExpression) -> DataFunctionSymbol {
    DataFunctionSymbol::with_signature(ZERO_FUNCTION, std::slice::from_ref(element), &SortExpression::nat())
}

/// See [SET_CONSTRUCTOR], instantiated for `element`.
pub fn set_constructor(element: &SortExpression) -> DataFunctionSymbol {
    let set: SortExpression = SortCons::new(ContainerSortKind::Set, element.clone()).into();
    let predicate: SortExpression = SortArrow::new(std::slice::from_ref(element), &SortExpression::bool()).into();
    DataFunctionSymbol::with_signature(SET_CONSTRUCTOR, &[predicate, fset(element)], &set)
}

/// See [BAG_CONSTRUCTOR], instantiated for `element`.
pub fn bag_constructor(element: &SortExpression) -> DataFunctionSymbol {
    let bag: SortExpression = SortCons::new(ContainerSortKind::Bag, element.clone()).into();
    let multiplicity: SortExpression = SortArrow::new(std::slice::from_ref(element), &SortExpression::nat()).into();
    DataFunctionSymbol::with_signature(BAG_CONSTRUCTOR, &[multiplicity, fbag(element)], &bag)
}

/// `FSet(element)`.
fn fset(element: &SortExpression) -> SortExpression {
    SortCons::new(ContainerSortKind::FSet, element.clone()).into()
}

/// `FBag(element)`.
fn fbag(element: &SortExpression) -> SortExpression {
    SortCons::new(ContainerSortKind::FBag, element.clone()).into()
}

#[cfg(test)]
mod tests {
    use merc_data::BasicSort;
    use merc_data::DataFunctionSymbol;
    use merc_data::SortExpression;
    use merc_syntax::UntypedDataSpecification;

    use crate::system_symbols::C_NAT;
    use crate::lowering::lower_syntax_sort;

    use super::bag_constructor;
    use super::c_dub;
    use super::c_int;
    use super::c_nat;
    use super::c_real;
    use super::c0;
    use super::c1;
    use super::concat_digit_nat;
    use super::concat_digit_pos;
    use super::empty_fbag;
    use super::empty_fset;
    use super::empty_list;
    use super::false_function;
    use super::fbag_cinsert;
    use super::fset_insert;
    use super::most_significant_digit;
    use super::most_significant_digit_nat;
    use super::pos2nat;
    use super::set_constructor;
    use super::zero_function;

    /// Declares the bundled specification files by name, so a failure names the file.
    macro_rules! spec {
        ($name:literal) => {
            ($name, include_str!(concat!("../../syntax/spec/", $name)))
        };
    }

    /// Asserts that every file in `files` declares a constructor or mapping with the name and sort
    /// of `symbol`. The type variable `S` of a container template is matched by the basic sort
    /// `S`, which is what [lower_syntax_sort] makes of an unresolved template reference.
    #[track_caller]
    fn assert_declared(symbol: DataFunctionSymbol, files: &[(&str, &str)]) {
        for (file, text) in files {
            let spec = UntypedDataSpecification::parse(text)
                .unwrap_or_else(|error| panic!("the bundled specification {file} should parse: {error}"));
            let declared = spec
                .constructor_declarations
                .iter()
                .map(|decl| (decl.identifier.as_str(), &decl.sort))
                .chain(
                    spec.map_declarations
                        .iter()
                        .map(|decl| (decl.identifier.as_str(), &decl.sort)),
                )
                .any(|(name, sort)| DataFunctionSymbol::with_sort(name, lower_syntax_sort(sort).copy()) == symbol);
            assert!(declared, "{file} does not declare {symbol}: {}", symbol.sort());
        }
    }

    fn template_variable() -> SortExpression {
        BasicSort::new("S").into()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_numeric_symbols_match_their_declarations() {
        assert_declared(c0(), &[spec!("nat.mcrl2"), spec!("nat64.mcrl2")]);
        assert_declared(c1(), &[spec!("pos.mcrl2"), spec!("pos64.mcrl2")]);
        assert_declared(c_dub(), &[spec!("pos.mcrl2")]);
        assert_declared(c_nat(), &[spec!("nat.mcrl2")]);
        assert_declared(c_int(), &[spec!("int.mcrl2"), spec!("int64.mcrl2")]);
        assert_declared(c_real(), &[spec!("real.mcrl2"), spec!("real64.mcrl2")]);
        assert_declared(pos2nat(), &[spec!("nat.mcrl2"), spec!("nat64.mcrl2")]);
        assert_declared(most_significant_digit(), &[spec!("pos64.mcrl2")]);
        assert_declared(most_significant_digit_nat(), &[spec!("nat64.mcrl2")]);
        assert_declared(concat_digit_pos(), &[spec!("pos64.mcrl2")]);
        assert_declared(concat_digit_nat(), &[spec!("nat64.mcrl2")]);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_container_symbols_match_their_declarations() {
        let s = template_variable();
        assert_declared(empty_list(&s), &[spec!("list.mcrl2"), spec!("list64.mcrl2")]);
        assert_declared(empty_fset(&s), &[spec!("fset.mcrl2"), spec!("fset64.mcrl2")]);
        assert_declared(empty_fbag(&s), &[spec!("fbag.mcrl2"), spec!("fbag64.mcrl2")]);
        assert_declared(fset_insert(&s), &[spec!("fset.mcrl2"), spec!("fset64.mcrl2")]);
        assert_declared(fbag_cinsert(&s), &[spec!("fbag.mcrl2"), spec!("fbag64.mcrl2")]);
        assert_declared(false_function(&s), &[spec!("set.mcrl2"), spec!("set64.mcrl2")]);
        assert_declared(zero_function(&s), &[spec!("bag.mcrl2"), spec!("bag64.mcrl2")]);
        assert_declared(set_constructor(&s), &[spec!("set.mcrl2"), spec!("set64.mcrl2")]);
        assert_declared(bag_constructor(&s), &[spec!("bag.mcrl2"), spec!("bag64.mcrl2")]);
    }

    /// A symbol that the specification files do not declare with that sort must be rejected, so
    /// the tests above cannot pass vacuously.
    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    #[should_panic(expected = "does not declare")]
    fn test_a_mismatching_sort_is_rejected() {
        let wrong = DataFunctionSymbol::with_signature(C_NAT, &[SortExpression::nat()], &SortExpression::nat());
        assert_declared(wrong, &[spec!("nat.mcrl2")]);
    }
}
