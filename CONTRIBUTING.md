# Contributing

Source code documentation can be found at the Github
[pages](https://mercorg.github.io/merc/index.html), and more general
documentation can be found on our
[website](https://mercorg.github.io/merc-website/). The basic tests can be run
locally using `cargo test` in the root directory, or in a specific crate using
`-p <crate_name>`, but more extensive information can be found on the website on
the whole CI overview. 

Unless you explicitly state otherwise, any contribution you make to this project
will be licensed under the same license as the project.

## Formatting

All source code should be formatted using `cargo fmt`, which can installed using
`rustup component add rustfmt`. Source files can then be formatted using `cargo
+nightly fmt`, or a single crate with `-p <crate_name>`.

## Third party libraries

We generally strive for using high quality third party dependencies. For this
purpose we use `cargo deny check`, installed with `cargo install cargo-deny` to
check the licenses of third party libraries, and also to check them against the
`RustSec` advisory db. In general unmaintained dependencies should either be
vendored or replaced by own code. However, using third party libraries where
applicable is generally not discouraged.
