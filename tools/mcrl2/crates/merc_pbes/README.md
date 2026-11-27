# Overview

This crate provides exploration, symmetry detection and symmetry reduction for
parameterised boolean equation systems (PBESs) of the mCRL2 toolset. A PBES can
be instantiated into a parity game either directly via its structure graph, or
after conversion to standard recursive form (SRF), both sequentially and in
parallel. There is also an LDD-based symbolic reachability that explores the
PBES into a symbolic parity game.

Symmetries of a PBES are permutations of its parameters that leave the equation
system invariant, and can be used to explore only one representative per orbit.
Two ways of obtaining them are implemented. The `SymmetryAlgorithm` checks
candidate parameter permutations directly on the equations, whereas
`graph_symmetries` constructs the Symmetry Detection Graph (SDG) of the PBES and
calls an external [GAP](https://www.gap-system.org/) process to compute its
automorphism group. The resulting generators are turned into a stabilizer chain
by `Bsgs`, which `QuotientLps` uses to canonicalize every next state to its
orbit representative during exploration.

The [`merc-pbes`](https://MERCorg.github.io/merc-website/) binary is a thin
command line wrapper around this crate: it parses the arguments, reads the PBES
and then calls into the algorithms defined here.

## Usage

Computing the symmetries of a PBES with the graph-based algorithm requires a
GAP installation, see below. The generators it returns are permutations of the
parameter indices.

```rust ignore
use merc_pbes::GapConfig;
use merc_pbes::graph_symmetries;

let result = graph_symmetries(&pbes, &GapConfig::default())?;
println!("|Sym(pbes)| = {}", result.symmetry_group_order);

for generator in &result.generators {
    println!("{generator}");
}
```

`GapConfig::executable` selects the GAP binary (`gap` on `$PATH` by default) and
`GapConfig::dump_script` writes the generated GAP script to a file for
debugging. The `Sdg` in the result can be written to a Graphviz DOT file with
`write_dot`, see the vertex legend below.

## Installing GAP

Download and install GAP 4 from <https://www.gap-system.org/>. The `gap` binary
must be on `$PATH`, or its location passed via `GapConfig::executable` (the
`--gap-path` flag of `merc-pbes`). Verified against **GAP 4.12.1**.

The symmetry detection additionally needs the
[Digraphs](https://digraphs.github.io/Digraphs/) package. When the GAP
installation includes the `PackageManager` package it can be installed by
running the following inside a GAP session:

```gap
LoadPackage("PackageManager");
InstallPackage("digraphs");
```

To verify that the package loads correctly:

```gap
LoadPackage("digraphs");
```

If this returns `fail`, `graph_symmetries` reports an error pointing to the
Digraphs website.

### Installing Digraphs on Debian and Ubuntu

The `InstallPackage` route does not work on the distribution packages of Debian
and Ubuntu. Their `gap-core` package does not ship `PackageManager`, and neither
`PackageManager` nor `Digraphs` is available as a `gap-*` package from the
archive, so `LoadPackage("PackageManager")` returns `fail` on a fresh install.
Digraphs must therefore be built from source, together with its `orb` and
`datastructures` dependencies which are not packaged either. GAP looks for
packages in `~/.gap/pkg`, so no root privileges are needed:

```sh
sudo apt install gap gap-dev build-essential
mkdir -p ~/.gap/pkg && cd ~/.gap/pkg

curl -sSLO https://github.com/gap-packages/datastructures/releases/download/v0.4.3/datastructures-0.4.3.tar.gz
curl -sSLO https://github.com/gap-packages/orb/releases/download/v5.1.0/orb-5.1.0.tar.gz
curl -sSLO https://github.com/digraphs/Digraphs/releases/download/v1.15.0/digraphs-1.15.0.tar.gz
for f in *.tar.gz; do tar xzf "$f"; done && rm -f *.tar.gz

for d in datastructures-0.4.3 orb-5.1.0 digraphs-1.15.0; do
  (cd "$d" && ./configure --with-gaproot=/usr/lib/gap && make -j"$(nproc)")
done
```

The `gap-dev` package provides the `gac` compiler and the GAP headers that these
three packages need, and `--with-gaproot` points them at the system GAP
installation. Newer releases can be used as long as they support the installed
GAP version.

## DOT output

`write_dot` (the `--dot <file.dot>` flag of `merc-pbes graph-symmetry`) writes
the SDG as a Graphviz DOT file for visualization. If the `dot` binary (part of
[Graphviz](https://graphviz.org/)) is on `$PATH`, a PDF is generated
automatically alongside it. To convert manually:

```sh
dot -Tpdf file.dot -o file.pdf
dot -Tsvg file.dot -o file.svg
```

The vertices of the resulting graph are drawn as follows.

| Shape | Fill colour | Label | Meaning |
|---|---|---|---|
| Rectangle | Blue | parameter name | PBES parameter |
| Tiny diamond (unlabelled) | Grey | — | Update vertex `X_{i,k}` |
| Hexagon | Light blue | `X` | Propositional variable instantiation of `X` |
| Parallelogram | Purple | `forall x:D,…` | Quantifier |
| Ellipse | Yellow | function name | Data function application |
| Ellipse | Orange | number | Machine-number constant |
| Ellipse | Red | `&&` / `\|\|` / `!` / `=>` | Boolean connective |
| Ellipse | Green | `x` | Bound (quantifier-scoped) variable |

Edges are directed from a formula to its direct subformulas, from a PVI to its
update vertices, and from an update vertex to its data argument and parameter.
An argument of a non-commutative operator carries its 1-based positions, and an
argument of a commutative operator its multiplicity `xN` when it occurs more than
once. The edges leaving an update vertex are dashed and labelled `dat`, `pos`, or
`dat,pos` when the parameter is passed unchanged.

For a detailed description of the SDG construction and its relation to the
technical report, see the
[merc-pbes graph-symmetry](https://MERCorg.github.io/merc-website/tools/merc-pbes-graph-symmetry/)
page on the MERC website.

## Safety

This crate contains `unsafe` code in the exploration modules. The parallel
explorers manually implement `Send` and `Sync` for their exploration contexts,
which is sound because these are read-only after construction apart from the
thread-safe concurrent indexed sets, and they extend the lifetime of `ATerm`
references that are interned in those sets, which is sound because the
referenced terms stay alive for the duration of the exploration. Every other
module is free of `unsafe` code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.
