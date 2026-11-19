# Overview

This crate provides type checking for parameterised real equation systems
(PRESs) as used in the [mCRL2](https://www.mcrl2.org/) toolset. A PRES is the
quantitative counterpart of a PBES: its equations range over the real numbers
instead of the Booleans, and can additionally contain operators such as `+`,
`inf`, `sup` and multiplication by a constant. The type checking itself is
shared with the PBESs of `merc_pbes`.

## Usage

A PRES in textual form is parsed by `merc_syntax` and then type checked. Type
errors are reported with the location in the input.

```rust
use merc_pres::PresSpecification;
use merc_syntax::UntypedPres;

let untyped = UntypedPres::parse("
    pres
        mu X(n: Nat) = sup m: Nat . (val(n + m) + X(m));
    init
        X(3);
").unwrap();

let pres = PresSpecification::from_untyped(untyped).unwrap();
assert_eq!(pres.equations().len(), 1);
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
