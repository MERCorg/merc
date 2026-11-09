# Overview

The `merc_process` crate type checks mCRL2 process specifications.
`ProcessSpecification::from_untyped` type checks the embedded data specification with
`merc_typecheck::DataSpecification`, and then checks every action declaration, process
declaration, and the `init` process against it. Before checking runs,
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

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
