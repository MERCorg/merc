# Overview

This crate type checks modal mu-calculus formulas as used in the
[mCRL2](https://www.mcrl2.org/) toolset, together with the data specification
and action declarations that they refer to. A type checked formula is also
available as a `ModalEquationSystem`, in which every fixpoint of the formula is
a separate equation.

The `FormulaType` determines whether `val(...)` occurrences in the formula are
Boolean, as in the plain mu-calculus, or real-valued, as in the quantitative
mu-calculus.

## Usage

```rust
use merc_formula::FormulaType;
use merc_formula::ModalSpecification;
use merc_syntax::UntypedStateFrmSpec;

let untyped = UntypedStateFrmSpec::parse("
    act
        send, receive: Nat;

    form
        nu X(n: Nat = 0) . [send(n)] mu Y . <receive(n)> true || <true> Y && X(n + 1);
").unwrap();

let formula = ModalSpecification::from_untyped(untyped, FormulaType::Bool).unwrap();
assert_eq!(formula.equation_system().len(), 2);
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
