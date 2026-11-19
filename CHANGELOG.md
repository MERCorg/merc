# v3.0 (December 2026)

See the `README.md` of the individual crates for their own changelogs. The
crates are now grouped into `core`, `language`, `rewriter` and
`transition_systems` directories.

This release adds a native type checker for the mCRL2 specification language.
The new `merc_typecheck` crate type checks data specifications, including
overload resolution, implicit numeric coercions, structured sorts and the
polymorphic system-defined containers. The syntax tree now carries span
information, which gives precise error messages and is the basis for a language
server. Parsing of deeply nested if-then-else expressions is no longer
quadratic.

The new `merc_explore` crate provides a generic explicit state space
exploration framework, with optional caching of summand enumeration, a
control flow graph analysis and parallel (work stealing) exploration. It is
used by `merc-lps explore-explicit`, by the PBES exploration and by
`merc-lts combine`. The latter now caches enabled actions and shares action
labels, which makes it considerably faster.

The `merc_ldd` crate has been removed in favour of the LDD implementation of
[OxiDD](https://oxidd.net). Symbolic reachability in `merc-symbolic` now
supports chaining and saturation (following Ciardo) and summand grouping. The
`merc-pbes` tool has been extended considerably. It can now explore PBES
explicitly or symbolically into parity games, and solve them (`solve`,
`solve-symbolic`). 

The `merc-rewrite` tool can now parse full mCRL2 specifications and has a new
`check` command to type check data specifications. `merc-lts` supports the
binary LTS formats where sensible, no longer converts `i` to `tau` when
reading AUT files, and requires an output for `convert`. The explicit parity
games and LTSs use compressed vectors to reduce memory usage.

The `merc_aterm` crate has been reviewed to ensure that safe code cannot lead
to undefined behaviour. In particular, `Protected` containers are now written
through a scoped closure. Terms and function symbols have a more compact
layout, the term parser no longer depends on pest, and garbage collection
marking uses pointer tagging instead of a hash set.

**Full Changelog**:  https://github.com/MERCorg/merc/compare/v2.0...main

# v2.0 (June 2026)

See the `README.md` of the individual crates for their own changelogs. The
`merc-lps` tool can be used to explore mCRL2 linear process specifications
symbolically. This is currently not very efficient, but will be improved in
future releases.

**Full Changelog**:  https://github.com/MERCorg/merc/compare/v1.1...v2.0

# v1.1 (January 2026)

This release was primarily made to force a Zenodo upload.

Added support for reading and writing LTSs in the `.bcg` format used by the
[CADP](https://cadp.inria.fr/) toolset, this is gated behind the `cadp` feature
flag.

Made the AUT format compliant with the original specification. In particular,
the internal action is now represented by `i` instead of `tau`. This means that
`merc-lts` now requires the `--tau=tau` flag to read AUT files that use `tau` as
the internal action, as is the case for mCRL2.

This release introduces the `merc_refinement` crate, which can be used to check
whether two labelled transition systems are related by various pre-order
relations, which are (weak) trace, failure refinement and failures-divergence
refinement and impossible futures.

A new tool `merc-sym` can explore a symbolic state space given in Sylvan's
binary `.ldd` format, or the mCRL2 symbolic binary `.sym` format. It can also
compute orderings using MINCE when the `kahypar` feature is enabled.

See the `README.md` of the individual crates for their own changelogs.


**Full Changelog**: https://github.com/MERCorg/merc/compare/v1.0...v1.1

# v1.0 (December 2025)

The initial release of the MERC toolset. Although nowhere near feature complete we opted to produce a `1.0` release to mark the initial milestone of the toolset instead of staying in zero version forever. We generally expect to release a single major version per year, without focusing too much on avoiding breaking changes to libraries for foreseeable future.

This release comes with a set of five tools:
  - `merc-lts` implements branching, strong and weak (signature-based) bisimulation reduction and comparison for labelled transition systems in the mCRL2 binary [`.lts`](https://www.mcrl2.org/web/user_manual/tools/lts.html) format and the **AUT**omaton (or ALDEBARAN) [`.aut`](https://cadp.inria.fr/man/aut.html) format.
  - `merc-rewrite` allows rewriting of Rewrite Engine Competition specifications ([REC](https://doi.org/10.1007/978-3-030-17502-3_6)) using [Sabre](https://arxiv.org/abs/2202.08687) (Set Automaton Based Rewrite Engine).
  - `merc-vpg` can be used to solve (variability) parity games in the [PGSolver](https://github.com/tcsprojects/pgsolver) `.pg` format, and a slightly extended variability parity game `.vpg` format. Furthermore, it can generate variability parity games for model checking modal mu-calculus on LTSs.
  - `merc-pbes` can identify symmetries in parameterised boolean equation systems [PBES](https://doi.org/10.1016%2Fj.tcs.2005.06.016).
  - `merc-ltsgraph` is a GUI tool to visualize LTSs.

This release also comes with various crates that can be used, these are also published on [crates.io](https://crates.io/users/mlaveaux):
  - `merc_aterm`: A feature complete thread-safe re-implementation of the mCRL2 aterm library in Rust. Can read and write aterms in (streamable) binary and text formats.
  - `merc_data`: Defines data expressions mimicking the mCRL2 data expressions, demonstrates the use of Rust macros (defined in `merc_macros`) to generate the required boilerplate code.    
  - `merc_lts`: library for manipulating labelled transition systems, including I/O for mCRL2's `.lts` files and Aldebaran `.aut` files. Can also generate random LTSs and (synchronous) products for testing purposes.
  - `merc_sharedmutex` is a crate implementing the busy-forbidden protocol, a read-efficient readers-writer lock described in the [preprint](https://arxiv.org/pdf/2111.02706).
  - `merc_syntax` defines a [Pest](https://pest.rs/) grammar for the mCRL2 specification, modal formula and PBES languages, and defines an AST for these languages.
  - `merc_vpg` defines functionality for manipulating (variability) parity games, including I/O for the PGSolver `.pg` format and a slightly extended variability parity game `.vpg` format.

**Full Changelog**: https://github.com/MERCorg/merc/commits/v1.0