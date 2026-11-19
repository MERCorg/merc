# Overview

This crate type checks process specifications of the
[mCRL2](https://www.mcrl2.org/) toolset. This checks the data specification,
the action and process declarations and the initial process. Note that the
syntax of process expressions and data expressions overlaps, for example `.`,
`+` and `||` occur in both, so before type checking the parsed specification is
disambiguated based on the declared actions and processes.

## Usage

```rust
use merc_process::ProcessSpecification;
use merc_syntax::UntypedProcessSpecification;

let untyped = UntypedProcessSpecification::parse("
    act
        a, b: Nat;

    proc
        P(n: Nat) = a(n) . P(n + 1) + (n > 0) -> b(n) . P(Int2Nat(n - 1));

    init
        P(0);
").unwrap();

let spec = ProcessSpecification::from_untyped(untyped).unwrap();
assert_eq!(spec.process_declarations().len(), 1);

// Using an undeclared action results in a type error.
let untyped = UntypedProcessSpecification::parse("init c;").unwrap();
assert!(ProcessSpecification::from_untyped(untyped).is_err());
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
