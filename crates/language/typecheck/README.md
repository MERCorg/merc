# Overview

The `merc_typecheck` crate type checks mCRL2 data specifications, following the
definitions in *Modeling and Analysis of Communicating Systems* (Groote &
Mousavi, MIT Press 2014). It turns the loosely-structured syntax tree produced
by the `merc_syntax` parser into a fully typed specification: it resolves every
name, decides the sort of every expression, chooses between overloaded
operators, and inserts the implicit coercions the surface language leaves out
(such as reading a natural number where a real number is expected).

Type checking runs as a pipeline of phases — sort resolution, desugaring,
signature building, then constraint-based sort inference. Lowering the result
to the aterm representation used by the rest of merc (`merc_data`,
`merc_sabre`, `merc_explore`) is done by the separate `merc_lowering` crate. For the full design — the query-based architecture, the sort
lattice, and the ranked backtracking search that drives inference — see the
[Type Checking](https://MERCorg.github.io/merc/developer/typechecking/) page
on the documentation site.

## Usage

The entry point is `DataSpecification::from_untyped`, which takes an
`UntypedDataSpecification` (produced by `merc_syntax`) and returns a type
checked specification, or a `WellTypedError` describing the first problem
found. `merc_lowering::lower_data_specification` turns the result into the
`merc_data::Mcrl2DataSpecification` (aterm, fully typed) consumed by rewriting.

```rust
use merc_lowering::lower_data_specification;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

let untyped = UntypedDataSpecification::parse(
    "sort D = struct c(pr: Nat, other: Bool)?is_c | d;
     map f: D -> Nat;
     var d: D;
     eqn f(d) = pr(d);",
)
.unwrap();

let spec = DataSpecification::from_untyped(untyped).unwrap();
let lowered = lower_data_specification(&spec);
```

`DataSpecification::typing_info` (and its per-equation counterpart,
`equation_typing_info`) returns a `TypingInfo`: the sort and name resolution of
every checked expression node, keyed by source `Span` rather than any internal
id, so a consumer such as an editor integration can look up hover text or a
go-to-definition target by byte offset via `TypingInfo::at_offset` without
needing to know anything about how inference numbers its nodes internally.

### Other specifications

The process specifications, state formulas, and PBES/PRES specifications that embed data
expressions are checked by their own crates, `merc_process`, `merc_formula`, `merc_pbes`, and
`merc_pres`, on top of a `DataSpecification`. These build on the scoped checking and variable
resolution building blocks this crate exposes behind the (unstable) `internal` feature.

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
