# Overview

This crate provides parameterised Boolean equation systems (PBESs) as used in
the [mCRL2](https://www.mcrl2.org/) toolset. It contains two representations.
The `PbesSpecification` is a type checked PBES in its textual syntax, as parsed
by `merc_syntax`. The `Pbes` is the term-based representation that the mCRL2
toolset uses, which can be read from and written to mCRL2's binary `.pbes`
format, and transformed into standard recursive form (SRF).

## Usage

A PBES in textual form is parsed by `merc_syntax` and then type checked. Type
errors are reported with the location in the input.

```rust
use merc_pbes::PbesSpecification;
use merc_syntax::UntypedPbes;

let untyped = UntypedPbes::parse("
    pbes
        nu X(n: Nat) = val(n < 10) && X(n + 1);
    init
        X(0);
").unwrap();

let pbes = PbesSpecification::from_untyped(untyped).unwrap();
assert_eq!(pbes.equations().len(), 1);
```

A PBES in mCRL2's binary format can be read with `read_pbes`, which also checks
that it is well typed. It can then be converted into SRF, in which every
equation is a disjunction (or conjunction) of summands of the shape
`exists e. c && X(f)`.

```rust,no_run
use std::fs::File;

use merc_pbes::pbes_to_srf;
use merc_pbes::read_pbes;

let pbes = read_pbes(File::open("example.pbes").unwrap()).unwrap();
let srf = pbes_to_srf(&pbes, true).unwrap();

for equation in srf.equations() {
    println!("{} has {} summands", equation.variable(), equation.summands().len());
}
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
