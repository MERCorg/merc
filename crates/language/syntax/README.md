# Overview

This crate contains a complete [Pest](https://pest.rs/) grammar for the
[mCRL2](https://www.mcrl2.org/) specification language, along with functionality
to consume the resulting parse tree into an abstract syntax tree (AST). As
opposed to the mCRL2 toolset where the AST is represented by terms, we store the
AST in a Rust-idiomatic way using enums and structs. This means that we
potentially use more memory because there is no term sharing of expressions, but
manipulating the AST is much easier.

## Usage

The mCRL2 specifications can simply be parsed using the untyped variants of the
various objects: `UntypedProcessSpecification`, `UntypedDataSpecification`, etc.

```rust
use merc_syntax::UntypedProcessSpecification;

let mcrl2_spec = UntypedProcessSpecification::parse("
    act
        a, b: Nat;

    proc
        P(n: Nat) = a(n).P(n + 1) + b(n).P(n - 1);

    init
        P(0);

").unwrap();
```

## Changelog

### 3.0.0

The AST now carries source spans. The expression types, such as `DataExpr`,
`SortExpression`, `ProcessExpr` and `StateFrm`, are now a `Spanned` node of
which the variants moved to the corresponding kind, e.g. `DataExprKind`.

The visitors and builders, such as `visit_statefrm` and `apply_sort_expression`,
have been replaced by the `Traverse` trait. These traversals, and dropping an
AST, no longer use recursion, avoiding stack overflows on large expressions.

Avoided quadratic parsing time of nested if-then-else expressions, and fixed
several parsing and printing issues.

### 2.0

Added various builders and visitors to manipulate the ASTs.

Removed the `arbitrary` dependency since generating these expressions completely arbitrarily is not that useful.

## Safety

This crate contains no unsafe code.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.