# Overview

The goal of the `merc` project is to provide a generic set of libraries and tools for (specification language-agnostic) model checking, written in the [Rust](https://rust-lang.org/) language. The name is an acronym for "[**m**CRL2](https://www.mcrl2.org/web/index.html) **e**xcept **R**eliable & **C**oncurrent", which is just a backronym for the name. 

We aim to demonstrate efficient and correct implementations using Rust. The main focus is on clean interfaces to allow the libraries to be reused as well. The toolset supports and is tested on all major platforms: Linux, macOS and Windows.

## Installation

Every release provides installers and portable binaries on the [releases page](https://github.com/MERCorg/merc/releases):
 - Linux (x86-64): a `.deb` package for Ubuntu 24.04 and Debian 13 or newer, and an `.rpm` package for the supported Fedora releases. Both install the tools into `/opt/merc` and link them into `/usr/bin`. The portable binaries run on any distribution with glibc 2.39 or newer.
 - macOS (Apple Silicon, macOS 11 or newer): a `.dmg` disk image containing the tools.
 - Windows (x86-64, Windows 10 or newer): an `.msi` installer that adds the tools to the `PATH`.

## Contributing

The toolset is still in quite early stages, but contributions and ideas are more than welcome so feel free to contact the authors or open discussions. The toolset can be built using `cargo build`. By default this will build in `dev` or debug mode, and a release build can be obtained by passing the `--release` flag. Several tools will be built that can be found in the `target/debug` (or `release`) directory. See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for more information on the testing and formatting of code. LLMs are used for reviewing and occasionally code can be written by AI, but slop is strictly forbidden. Extensive (random) testing under various [sanitizers](https://github.com/google/sanitizers/wiki/addresssanitizer) and [miri](https://github.com/rust-lang/miri) is used to gain confidence in the `unsafe` parts of the implementation.

Bugs and issues can be reported in the [issue tracker](https://github.com/MERCorg/merc/issues).

## Tools

Various tools have been implemented so far:
 - `merc-lts` implements various algorithms for labelled transition systems in the mCRL2 binary [`.lts`](https://www.mcrl2.org/web/user_manual/tools/lts.html) format and the AUTomaton (or ALDEBARAN) [`.aut`](https://cadp.inria.fr/man/aut.html) format. Using [CADP](https://cadp.inria.fr/) it can also read and write the [`.bcg`](https://cadp.inria.fr/man/bcg.html) format. It can do (signature-based) bisimulation algorithms for reduction and comparison, and also supports various refinement preorders. Furthermore, it can now also compute compositions of LTSs.
 - `merc-rewrite` allows rewriting of Rewrite Engine Competition specifications ([REC](https://doi.org/10.1007/978-3-030-17502-3_6)) using [Sabre](https://arxiv.org/abs/2202.08687) (**S**et **A**utomaton **B**ased **RE**writing). It can also rewrite mCRL2 data expressions using the data specification contained in an mCRL2 specification (`.mcrl2`, `.dataspec`, `.pbes`, `.pres` or `.mcf`), and type check such specifications.
 - `merc-vpg` can be used to solve (variability) parity games in the [PGSolver](https://github.com/tcsprojects/pgsolver) `.pg` format, and a slightly extended variability parity game `.vpg` format. Furthermore, it can generate variability parity games for model checking modal mu-calculus on LTSs.
 - `merc-lps` explores the state space of linear process specifications of mCRL2, either symbolically into a `.sym` file or explicitly, using multiple threads, into an `.lts` file. It can also detect deadlocks. Located in the `tools/mcrl2` workspace.
 - `merc-pbes` solves parameterised boolean equation systems [PBES](https://doi.org/10.1016%2Fj.tcs.2005.06.016), either explicitly by exploring them into a parity game, optionally using multiple threads and symmetry reduction, or symbolically using LDD-based exploration. It can also identify symmetries in a PBES. Located in the `tools/mcrl2` workspace.
 - `merc-ltsgraph` is a GUI tool to visualize LTSs, rendering either on the CPU or the GPU, located in the `tools/gui` workspace.
 - `merc-sym` computes the reachable states of a symbolic state space given in Sylvan's binary `.ldd` format, or the mCRL2 symbolic binary `.sym` format, using either LDDs or BDDs. It can also reduce symbolic LTSs, convert them to explicit LTSs, and compute orderings with `kahypar` using the MINCE algorithm.

Various crates are also published on [crates.io](https://crates.io/users/mlaveaux), see the [crates](./crates) directory for an overview.

## License

The work is licensed under the Boost Software License, see the [`LICENSE`](./LICENSE) for details. Furthermore, `cargo deny` is used to ensure that no `crates.io` dependencies with incompatible licenses are added.

## Related Work

This tool set is inspired by the work on the [mCRL2](https://github.com/mCRL2org/mCRL2) toolset, the work on a specification language agnostic toolset [ltsmin](https://ltsmin.utwente.nl/) and the work on [CADP](https://cadp.inria.fr/).

This project is developed at the department of Mathematics and Computer Science of the [Technische Universiteit Eindhoven](https://fsa.win.tue.nl/).