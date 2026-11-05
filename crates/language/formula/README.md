# Overview

The `merc_formula` crate type checks modal (mu-calculus) state formulas.
`ModalSpecification::from_untyped` type checks the embedded data specification with
`merc_typecheck::DataSpecification`, flattens the formula's nested fixpoints into a
`ModalEquationSystem`, and checks every action, fixpoint variable, and data binder against it.
`FormulaType` selects whether `val(...)` occurrences are Boolean (plain mu-calculus) or
real-valued (quantitative).

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
