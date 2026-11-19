# Overview

This crate lowers a type checked data specification, as produced by
`merc_typecheck`, to the term representation of `merc_data` that is used by
the rewriters in `merc_sabre`. During lowering the implicit coercions are made
explicit, numbers are encoded according to the chosen `NumberEncoding`, and the
system defined sorts, such as lists, sets and function updates, are
instantiated for the sorts that the specification uses.

## Usage

```rust
use merc_lowering::lower_data_specification;
use merc_lowering::lower_expression;
use merc_syntax::DataExpr;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

let untyped = UntypedDataSpecification::parse("
    map f: Nat -> Nat;
    var n: Nat;
    eqn f(n) = n + 1;
").unwrap();
let mut spec = DataSpecification::from_untyped(untyped).unwrap();

// Lowers the whole specification, which can be used to construct a rewriter.
let lowered = lower_data_specification(&spec);

// Expressions are first type checked against the specification and then lowered.
let typed = spec.typecheck_expression(&DataExpr::parse("f(1)").unwrap()).unwrap();
let term = lower_expression(&spec, &typed);
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
