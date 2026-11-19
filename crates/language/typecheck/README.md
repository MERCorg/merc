# Overview

This crate type checks data specifications of the
[mCRL2](https://www.mcrl2.org/) toolset, following the definitions in
*Modeling and Analysis of Communicating Systems* (Groote & Mousavi, MIT Press
2014). Type checking resolves every name, determines the sort of every
expression, chooses between overloaded functions and inserts the implicit
coercions, for example where a natural number is used as a real number.

The result can be lowered to the term representation used by the rewriters
with `merc_lowering`. Process specifications, modal formulas, PBESs and PRESs
are type checked by the `merc_process`, `merc_formula`, `merc_pbes` and
`merc_pres` crates respectively, which build on this crate. The design of the
type checker is described on the
[Type Checking](https://MERCorg.github.io/merc/developer/typechecking/) page of
the website.

## Usage

```rust
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

let text = "
    sort D = struct c(pr: Nat, other: Bool)?is_c | d;
    map f: D -> Nat;
    var x: D;
    eqn f(x) = pr(x);
";

let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse(text).unwrap()).unwrap();

// The typing information gives the sort of every expression by its position in the input.
let offset = text.find("pr(x)").unwrap();
let typing_info = spec.typing_info();
let node = typing_info.at_offset(offset).unwrap();
println!("{} has sort {:?}", &text[offset..offset + 2], node.sort);

// The structured sort declares the projection `pr: D -> Nat`, so applying it to
// the natural number `1` instead of a `D` results in a type error.
let untyped = UntypedDataSpecification::parse("
    sort D = struct c(pr: Nat);
    map g: Nat;
    eqn g = pr(1);
").unwrap();
assert!(DataSpecification::from_untyped(untyped).is_err());
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
