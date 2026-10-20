# Overview

The `merc_typecheck` crate type checks mCRL2 data specifications, following the
definitions in *Modeling and Analysis of Communicating Systems* (Groote &
Mousavi, MIT Press 2014). It turns the loosely-structured syntax tree produced
by the `merc_syntax` parser into a fully typed specification: it resolves every
name, decides the sort of every expression, chooses between overloaded
operators, and inserts the implicit coercions the surface language leaves out
(such as reading a natural number where a real number is expected).

Type checking runs as a pipeline of phases — sort resolution, desugaring,
signature building, constraint-based sort inference, then lowering to the
aterm representation used by the rest of merc (`merc_data`, `merc_sabre`,
`merc_explore`). For the full design — the query-based architecture, the sort
lattice, and the ranked backtracking search that drives inference — see the
[Type Checking](https://MERCorg.github.io/merc/developer/typechecking/) page
on the documentation site.

## Usage

The entry point is `DataSpecification::from_untyped`, which takes an
`UntypedDataSpecification` (produced by `merc_syntax`) and returns a type
checked specification, or a `WellTypedError` describing the first problem
found. Calling `lower_data_specification` on the result produces the
`merc_data::Mcrl2DataSpecification` (aterm, fully typed) consumed by rewriting.

```rust
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
let lowered = spec.lower_data_specification();
```

`DataSpecification::typing_info` (and its per-equation counterpart,
`equation_typing_info`) returns a `TypingInfo`: the sort and name resolution of
every checked expression node, keyed by source `Span` rather than any internal
id, so a consumer such as an editor integration can look up hover text or a
go-to-definition target by byte offset via `TypingInfo::at_offset` without
needing to know anything about how inference numbers its nodes internally.

### Process and PBES specifications

`ProcessSpecification::from_untyped` extends `DataSpecification` to also check a
whole `UntypedProcessSpecification`. Before checking runs,
`ProcessSpecification` disambiguates the specification to undo a handful of
grammar-level ambiguities mCRL2's concrete syntax has between the process
algebra and the data language — `.`, `+`, and `||` all lex the same in both, and
a single-action `hide`/`block`/`allow` application can appear in the same
ambiguous position. `ProcessSpecification::typing_info` exposes the same
span-keyed `TypingInfo` as `DataSpecification`, merged over every checked
process-body expression.

A related grammar quirk survives disambiguation rather than being undone by it: a process
reference with no arguments and no parentheses (`proc P = a . P;`'s recursive `P`) is not parsed
as `ProcessExprKind::Id` — that variant requires parentheses, even empty ones (`P()`). A bare `P`
instead falls through to the same `Action` rule a real action instantiation uses, landing as
`ProcessExprKind::Action`. Type checking resolves this by looking such a name up against both the
declared action names and the declared process names (`check_action_or_process`); an `act` and a
`proc` are never allowed to share a name in the first place (`ActionAndProcessConflict`), so this
never resolves a genuine action-vs-process ambiguity — only an arity overload within whichever one
of the two the name actually belongs to.

`PbesSpecification::from_untyped` similarly extends `DataSpecification` to check
a whole PBES This covers *sorts and names* only; PBES well-formedness properties
such as monotonicity or alternation depth are a separate semantic analysis and
are out of scope here.

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
