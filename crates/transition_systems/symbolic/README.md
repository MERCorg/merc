# Overview

This crate provides algorithms for working with symbolic data structures. This
includes List Decision Diagrams using the `merc_ldd` crate and Binary Decision
Diagrams using the [OxiDD](https://oxidd.net) crate. 

In the following example we show that we can load symbolic LTSs stored in
Sylvan's binary format. These can then be explored using the reachability
function.

```rust
use std::fs::File;

use merc_ldd::Storage;
use merc_symbolic::read_sylvan;
use merc_symbolic::reachability;

let mut storage = Storage::new();
let lts = read_sylvan(&mut storage, &File::open("../../examples/ldd/anderson.4.ldd").expect("File could not be opened"));

reachability(&mut storage, &lts).expect("Should not fail");
```

Furthermore, this crate can also compute variable ordering, for now only using
the MINCE algorithm for a given dependency graph. This requires the
[kahypar](https://github.com/kahypar/kahypar) tool.

The summands of a linear process can be distributed over the transition groups
used for symbolic reachability, using the `SummandGrouping` strategies of the
mCRL2 toolset (`none`, `used`, `simple` or a user defined partition). The
grouping does not change the reachable set, only the number and shape of the
transition relations.

## Changelog

### 3.0.0

Fixed the implementation of the signature-based refinement algorithm for strong
bisimulation, both the regular and the split signature variants should now yield
the correct results. Furthermore, the implementation of the naive quotienting has
also been fixed.

## Safety

This crate contains no `unsafe` code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may
be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the
[LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE)
file in the repository root for more information.