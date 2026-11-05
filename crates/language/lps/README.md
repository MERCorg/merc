# Overview

The `merc_lps` crate defines the linear process specifications (LPS) of the
mCRL2 toolset, and reads and writes them in its binary `.lps` format.

A linear process is a single process `P(x: D)` whose body is a choice between
summands. An action summand `sum e: E . c -> a(f) . P(g)` is represented by its
summation variables, its condition `c`, its multi-action `a(f)` and the next
state `g`, which has a value for every process parameter. Only these data
expressions are terms of `merc_data`; unlike in mCRL2 the summands, actions and
the process itself are plain Rust values. Stochastic linear processes are not
supported.

## Usage

```rust,no_run
use std::fs::File;

use merc_lps::read_lps;
use merc_lps::write_lps;

let lps = read_lps(File::open("abp.lps").unwrap()).unwrap();
for summand in &lps.action_summands {
    println!("{} -> {}", summand.condition, summand.action);
}
write_lps(&lps, File::create("copy.lps").unwrap()).unwrap();
```

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it
may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the
[LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE)
file in the repository root for more information.
