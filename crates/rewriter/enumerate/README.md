# Overview

Enumerates ground constructor instances of a (possibly infinite) data sort.

Neither `merc_sabre` nor `merc_explore` can close an open data expression: pick
concrete constructor instances for a set of free variables of a sort. This
crate owns that single mechanism, which is needed both to eliminate `exists`/
`forall` quantifiers during rewriting and to instantiate `sum d:D` variables
while generating LPS successors.

Enumeration always proceeds over a sort's *constructors*, normalising each
candidate with a `merc_sabre::RewriteEngine` rather than ever descending into
a native machine-word representation.

The definitions, the search algorithm and the choices behind it are described
on the [Data enumeration](https://mercorg.github.io/merc-website/developer/rewriting/enumeration/) page of the website, and its use for
quantifiers on [Quantifiers in the rewriter](https://mercorg.github.io/merc-website/developer/rewriting/quantifiers/).
