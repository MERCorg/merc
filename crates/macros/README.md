# Overview

 > ⚠️ **important** This is an internal crate and is not intended for public use.

This internal crate defines procedural macros for the MERC project. The main
macro provided is the `merc_derive_terms` macro which generates the necessary
boilerplate for `ATerm` data types.

In Rust there is no inheritance mechanism like in object-oriented languages such
as `C++` or `Java` in which the ATerm library has been implemented originally.
However, one typically wants to add additional structure on top of the basic
`ATerm` type, for example to represent (typed) data expressions, variables,
sorts etc. 

The `merc_derive_terms` macro automatically generates the necessary boilerplate
code to convert between the custom data types and the underlying `ATerm`
representation, as well as implementing common traits such as `Clone`, `Debug`,
`PartialEq` and `Eq`. Furthermore, the (arguably) most important feature is that
it implements the `Ref<'_>` variant, similarly to `ATermRef`, which allows for
references without taking ownership (and as such incurring a protection) of the
underlying data. This avoids the need for `UB` casts as done in the original ATerm
library. See the `merc_aterm` crate for documentation of this macro since it uses
types from that crate and these cannot be circularly referenced.

There is also a small utility macro called `merc_test` that can be used in place
of `#[test]` to define unit tests that automatically enable the logging
infrastructure used throughout MERC.

## Debugging

Working with procedural macros is typically difficult, but `cargo-expand` can be
installed using `cargo install cargo-expand` to make it easier. Running the
command `cargo expand` in `merc_aterm` crate can be used to print the Rust code
with the macros expanded for debugging purposes.

## Safety

This crate does not use unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.