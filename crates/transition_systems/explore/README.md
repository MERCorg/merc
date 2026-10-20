# Overview

This crate provides state space exploration algorithms on top of a generic
linear process interface. The central concept is the `LPS` trait, which
describes a transition system implicitly by an initial state vector and a
collection of condition action effect summands (the `Summand` trait). Each
summand enumerates the outgoing transitions it produces for a given source
state. This interface covers linearised process specifications, but also other
implicit state space generators such as PBESs in standard recursive form (SRF).

The `explore` function explores the state space of any `LPS` in either
depth-first or breadth-first order, invoking caller-supplied closures for every
discovered state and transition. The set of discovered states is stored in a
`DiscoveredSet`, which compact stores state vectors in a hash-consed B+-tree
forest to save memory.

Furthermore, the `CacheLPS` wrapper caches the enumeration results of the
summands of an underlying `LPS`. This can save time spent for rewriting or
enumeration, at the cost of increased memory usage.

Finally, the crate contains the `combine_lts` function that computes the
parallel composition `hide(H, allow(A, comm(C, L1 || ... || Ln)))` of a list of
LTSs, where `comm` applies communication expressions, `allow` restricts the
result to a set of allowed multi-actions and `hide` renames the given actions
to the internal action `τ`.

## Features

The `clap` feature flag enables the `clap` dependency to derive
`clap::ValueEnum` for `ExplorationStrategy` and `CachingStrategy`, such that
they can be used directly as command line arguments.

## Changelog

### v2.0.0

Generalised the exploration into the `LPS` and `Summand` traits with
caller-supplied closures for discovered states and transitions, so that state
space generators other than plain LPSs (e.g. PBESs in SRF form) can be
explored as well.

Added the `clap` feature to conditionally enable the `clap` dependency to
derive some convenience traits.

## Safety

This crate contains no `unsafe` code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may
be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the
[LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE)
file in the repository root for more information.
