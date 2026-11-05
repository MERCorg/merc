# Overview

The `merc_lowering` crate lowers a type checked mCRL2 data specification, a
`merc_typecheck::DataSpecification`, to the aterm representation of `merc_data`
that the rewriters and the rest of merc consume. It reads the sorts and name
resolutions that type checking inferred for every expression, makes the implicit
numeric and container coercions explicit, encodes numeric literals according to
the selected `NumberEncoding`, and monomorphizes the system-defined templates
(containers, function updates, comparisons) that the specification uses.

## Usage

```rust
use merc_lowering::lower_data_specification;
use merc_lowering::lower_expression;
use merc_syntax::DataExpr;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

let untyped = UntypedDataSpecification::parse("map f: Nat -> Nat; var n: Nat; eqn f(n) = n + 1;").unwrap();
let mut spec = DataSpecification::from_untyped(untyped).unwrap();

// The whole specification, as a `merc_data::Mcrl2DataSpecification`.
let lowered = lower_data_specification(&spec);

// A standalone expression, type checked against the specification first.
let typed = spec.typecheck_expression(&DataExpr::parse("f(1)").unwrap()).unwrap();
let term = lower_expression(&spec, &typed);
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
