# Overview

This crate provides algorithms for working with symbolic data structures. This includes List Decision Diagrams using the `merc_ldd` crate and Binary Decision Diagrams using the [OxiDD](https://oxidd.net) crate. 



```rust


let lts = read_sylvan(&File::open("../../examples/ldd/anderson.4.ldd"));





```

## Safety

This crate contains no `unsafe` code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.