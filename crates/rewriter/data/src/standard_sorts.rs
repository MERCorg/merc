use std::mem::ManuallyDrop;

use crate::BasicSort;
use crate::SortExpression;

thread_local! {
    /// Canonical terms for the standard sorts.
    static STANDARD_SORTS: StandardSorts = StandardSorts::new();
}

/// The standard sorts of mCRL2, built once per thread so that they are not recreated, which
/// requires a string and a term lookup, every time one is needed.
///
/// All fields are wrapped in `ManuallyDrop` so that their destructors never run at thread exit: a
/// term must never be dropped after its owning thread-local term pool has already been torn down,
/// and thread-local destruction order is unspecified (see [`crate::data_terms::DataSymbols`]).
struct StandardSorts {
    bool: ManuallyDrop<SortExpression>,
    pos: ManuallyDrop<SortExpression>,
    nat: ManuallyDrop<SortExpression>,
    int: ManuallyDrop<SortExpression>,
    real: ManuallyDrop<SortExpression>,
    machine_word: ManuallyDrop<SortExpression>,
}

impl StandardSorts {
    fn new() -> Self {
        let sort = |name: &str| ManuallyDrop::new(SortExpression::from(BasicSort::new(name)));
        StandardSorts {
            bool: sort("Bool"),
            pos: sort("Pos"),
            nat: sort("Nat"),
            int: sort("Int"),
            real: sort("Real"),
            machine_word: sort("@word"),
        }
    }
}

impl SortExpression {
    /// The sort `Bool`.
    pub fn bool() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.bool))
    }

    /// The sort `Pos`.
    pub fn pos() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.pos))
    }

    /// The sort `Nat`.
    pub fn nat() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.nat))
    }

    /// The sort `Int`.
    pub fn int() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.int))
    }

    /// The sort `Real`.
    pub fn real() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.real))
    }

    /// The sort `@word` of a machine word, the digit sort of the machine word number encoding
    /// (declared by `crates/syntax/spec/machine_word.mcrl2`).
    pub fn machine_word() -> SortExpression {
        STANDARD_SORTS.with(|sorts| SortExpression::clone(&sorts.machine_word))
    }
}
