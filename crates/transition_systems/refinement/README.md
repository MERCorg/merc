# Overview

This crate provides refinement algorithms to check various preorder relations
between labelled transition systems (LTSs). The main functionality is checking
whether an implementation LTS is refined by a specification LTS with respect to
the failures-divergence refinement, which is the preorder relation widely used by
the [FDR4](https://cocotec.io/fdr/); the CSP refinement checker.

```rust
use merc_lts::read_aut;
use merc_utilities::Timing;

use merc_refinement::ExplorationStrategy;
use merc_refinement::RefinementType;
use merc_refinement::refines;

let impl_lts = read_aut(b"des (0,8,6)                                        
(0,newday,1)
(1,tau,2)
(1,tau,3)
(2,teach,4)
(3,lindyhop,0)
(3,tau,5)
(4,newday,2)
(5,teach,0)
" as &[u8]).unwrap();

let spec_lts = read_aut(b"des (0,5,4)                                        
(0,newday,1)
(1,tau,2)
(1,tau,3)
(2,teach,0)
(3,lindyhop,0)
" as &[u8]).unwrap();

// Note that this is strong trace refinement, and we request no counter example.
let (result, counter_example) = refines(impl_lts, spec_lts, RefinementType::Trace, ExplorationStrategy::DFS, true, false, &mut Timing::new());
assert!(!result);
assert!(counter_example.is_none());
```

## Changelog

### 2.0

Added various refinement algorithms, which can return counter examples when the
refinement does not hold. These counter examples can be various shapes, but can
also be translated to modal mu-calculus formulas by the `merc_syntax` crate. In
the future we could also support translations to other formalisms, such as LTL
and CTL when applicable.

Added the `clap` feature to conditionally enable the `clap` dependency to derive
some convenience traits.

## Related Work

The original implementation as part of the mCRL2 toolset was done by Jan Friso
Groote, and this has been adapted to Rust by Maurice Laveaux. The
failure-divergences refinement algorithms are based on the following article:

 > Maurice Laveaux, Jan Friso Groote and Tim A.C. Willemse. "Correct and efficient antichain algorithms for refinement checking". Logical Methods in Computer Science, 2021. [DOI](https://doi.org/10.23638/LMCS-17(1:8)2021).

And the impossible futures algorithm is based on:

 > Maurice Laveaux and Tim A.C. Willemse. "Deciding Impossible Futures". SMLXV 2026.

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may
be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the
[LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE)
file in the repository root for more information.