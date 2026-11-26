# Overview

This crate provides algorithms for working with symbolic data structures. This
includes List Decision Diagrams and Binary Decision Diagrams, both using the
[OxiDD](https://oxidd.net) crate.

In the following example we show that we can load symbolic LTSs stored in
Sylvan's binary format. These can then be explored using the reachability
function.

```rust
use std::fs::File;

use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::read_sylvan;
use merc_symbolic::reachability;
use merc_utilities::Timing;

let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
let mut file = File::open("../../../examples/ldd/anderson.4.ldd").expect("File could not be opened");
let lts = read_sylvan(&manager, &mut file).expect("Should not fail");

let states = reachability(&manager, &lts, &Timing::new()).expect("Should not fail");
```

Furthermore, this crate can also compute variable orders using the MINCE
algorithm for a given dependency graph, or apply a user defined order. MINCE
requires the [kahypar](https://github.com/kahypar/kahypar) tool.

The summands of a linear process can be distributed over the transition groups
used for symbolic reachability, using the `SummandGrouping` strategies of the
mCRL2 toolset (`none`, `used`, `simple` or a user defined partition). The
grouping does not change the reachable set, only the number and shape of the
transition relations.

## Changelog

### 3.0.0

The `merc_ldd` crate has been removed, and List Decision Diagrams are now
provided by [OxiDD](https://github.com/oxidd/oxidd).

Symbolic reachability can be configured using `reachability_with_options`, which
supports the strategies of `ExplorationStrategy` (including chaining and
Ciardo's saturation) and can detect deadlocks, the latter is also required for
the `PBES` explorers.

Transition relations are learned during reachability with the `GroupLearner` that
a `SymbolicLPS` provides through `group_learner`, or not at all when the
relations are fixed. Exploration takes the LTS by shared reference, keeps the
learned relations itself and returns them in `ReachabilityResult::groups`; the
`SymbolicLps` keeps the interning of values and labels, see
`SymbolicLps::interning`.

Added `SymbolicLps` to explore a linear process symbolically, where the
summands are grouped into transition groups according to a `SummandGrouping`,
and the state vector is permuted according to a `VariableOrder`, which is
either user defined or computed by MINCE.

The `SymbolicLts` type is now generic in its action labels, and can be written
with `write_symbolic_lts`. The `convert_symbolic_lts` and the new
`convert_symbolic_lts_bdd` functions accept any `LtsBuilder`, so symbolic LTSs
can also be converted to mCRL2 `.lts` files.

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