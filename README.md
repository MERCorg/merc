# Overview

The goal of the `MERC` project is to provide a generic set of libraries and tools for (language-agnostic) model checking, written in the Rust language. The name is an acronym for "[**m**CRL2](https://www.mcrl2.org/web/index.html) **e**xcept **R**eliable & **C**oncurrent", which should not be taken literally. The project is developed at the department of Mathematics and Computer Science of the [Technische Universiteit Eindhoven](https://fsa.win.tue.nl/).

We aim to demonstrate efficient and correct implementations using (safe) Rust. Mainly focusing on clean interfaces to allow the libraries to be reused as well. This includes various algorithms on labelled transition systems and parity games for the time being, but in the future it should also be extended with parsing, type checking and state space exploration. The toolset supports and is tested on all main platforms: Linux, macOS and Windows.

## Contributing

The toolset is still in quite early stages, but contributions and ideas are more than welcome so feel free to contact the authors or open discussion. Compilation requires at least rustc version 1.85.0 and we use 2024 edition rust. Then the toolset can be build using `cargo build`, by default this will build in `dev` or debug mode, and a release build can be obtained by passing `--release`. Several tools will be build that can be found in the `target/{debug, release}` directory. See `CONTRIBUTING.md` for more information. Copilot is used for reviewing and occasionally boiler plate code can be written by AI, but slop is strictly forbidden. Extensive (random) testing under various [sanitizers](https://github.com/google/sanitizers/wiki/addresssanitizer) and [miri](https://github.com/rust-lang/miri) is used to gain confidence in the `unsafe` parts of the implementation.

Report bugs in the [issue tracker](https://github.com/MERCorg/merc/issues).

## Tools

Various tools have been implemented so far:
 - `merc-lts` implement various (signature-based) bisimulation algorithms for labelled transition systems in the mCRL2 binary `.lts` format and the Aldebaran `.aut` format.
 - `merc-rewrite` allows rewriting of [REC](https://doi.org/10.1007/978-3-030-17502-3_6) (Rewrite Engine Competition) specifications using [Sabre](https://arxiv.org/abs/2202.08687) (**S**et **A**utomaton **B**ased **RE**writing).
 - `merc-vpg` can be used to solve (variability) parity games in the [PGSolver](https://github.com/tcsprojects/pgsolver) `.(v)pg` format, and to generate parity games for model checking modal mu-calculus on LTSs.
 - `merc-pbes` can identify symmetries in paramerised boolean equation systems [PBES](https://doi.org/10.1016%2Fj.tcs.2005.06.016), located in the `tools/mcrl2` workspace.
 - `merc-ltsgraph` is a GUI tool to visualize LTSs, located in the `tools/GUI` workspace.

## License

The work is licensed under the Boost Software License, see the `LICENSE` for details.

## Related Work

This tool set is inspired by the work on [mCRL2](https://github.com/mCRL2org/mCRL2), and the work on [ltsmin](https://ltsmin.utwente.nl/).
