# Contributing

Compilation requires at least rustc version 1.85.0 and we use 2024 edition rust. By default this will build in `dev` or debug mode, and a release build can be obtained by passing `--release`. Source code documentation can be found at Github [pages](https://mlaveaux.github.io/merc/merc/index.html), and more detailed documentation can be found in `doc`.

## Formatting

All source code should be formatted using `cargo fmt`, which can installed using `rustup component add rustfmt`. Individual source files can then be formatted using `cargo +nightly fmt`.

## Third party libraries

We generally strive for using high quality third party dependencies, we use `cargo deny check`, installed with `cargo install cargo-deny` to check the license of third party libraries and to compare them to the `RustSec` advisory db. In general unmaintained dependencies should either be vendored or replaced by own code if possible. However, using third party libraries where applicable is generally not discouraged.