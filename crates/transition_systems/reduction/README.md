# Overview

This crate provides various algorithms for reducing labeled transition systems
(LTS) modulo various equivalence relations, see `merc_lts`. These algorithms can
also be used to compare LTS for equivalence. For now the equivalences that are
supported are strong bisimulation, branching bisimulation, and weak
bisimulation.

## Usage

The main functionalities of this crate are provided by the `reduce_lts` and
`compare_lts` functions, which can be used to reduce and compare LTS using all
available equivalence relations. These functions, and many functions in this crate,
accept the inputs by value, since these reductions often need to preprocess the given
LTS (e.g.,)

```rust
use merc_lts::LTS;
use merc_lts::read_aut;
use merc_reduction::reduce_lts;
use merc_reduction::Equivalence;
use merc_utilities::Timing;

let lts = read_aut(b"des(0, 6, 7)
(0, a, 1)
(0, a, 2)
(1, b, 3)
(1, c, 4)
(2, b, 5)
(2, c, 6)
" as &[u8]).unwrap();

let mut timings = Timing::new();
assert_eq!(lts.num_of_states(), 7); // The original has 7 states
let reduced = reduce_lts(lts, Equivalence::StrongBisim, false, &mut timings);
assert_eq!(reduced.num_of_states(), 3);
```

## Changelog

### 2.0.0

Fixed an issue with the compare function for branching bisimulation, which
was not properly adapting the `initial_rhs` state to the processed LTS.

Added various weak bisimulation algorithms, both using signatures and
Kanellakis-Smolka inspired ones, which can use branching bisimulation as a
preprocessing step.

Added divergence-preserving variants of the branching and weak bisimulation
algorithms. Iterative SCC decomposition implemented to avoid stack overflows on
large examples.

Added the `clap` feature to conditionally enable the `clap` dependency to derive
some convenience traits.

## Authors

This crate was developed by Maurice Laveaux and Jan Martens. The main
signature based branching bisimulation algorithm is described in the paper:

> Jan J.M. Martens and Maurice Laveaux. Faster Signature Refinement for Branching Bisimilarity Minimization. TACAS 2026.

## Safety

This crate contains minimal `unsafe` code, but modules that don't use `unsafe` code
are clearly marked as such.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.