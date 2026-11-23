---
name: write-code
description: Conventions for writing or changing Rust code in merc — where code belongs, which shared crates to reuse, how to handle terms, traversals and typed indices, and how to shape a public API, errors and tests. Use before implementing a feature, fixing a bug, refactoring, or adding a public item.
---

# Write code for merc

The repository is small enough that most things exist once, and it stays that way because a new piece of code first looks for what is already there. A change that works but duplicates a helper, recurses over a term or widens an API for one caller is not finished.

## Step 1: Find what already exists

Grep for the concept before writing it, in the crate you are in and in the shared crates below. If two crates need the same thing, it moves to the lowest crate that both depend on; it is not copied.

| Crate | Reuse it for |
|---|---|
| `merc_utilities` | `MercError`, `TagIndex`, the traversal protocol `Visit`/`Step`, `random_test`, `test_logger`, `Timing`, `debug_trace!`, `FixedSizeCache`, `FreshNameGenerator`, `SourceMap`/`Span`, chains of binary operators (`BinaryChain`), simplifying Boolean connectives (`optimized_and`, ...) |
| `merc_collections` | `IndexedSet`, `VecSet`, `VecBag`, `BlockPartition`, `IndexedPartition`, `ByteCompressedVec`, `scc_decomposition`, `Graph` |
| `merc_aterm` | terms, `Protected` containers, `TermBuilder`, the binary term streams |
| `merc_io` | bit streams, `LineIterator`, `TimeProgress`, `DumpFiles` for test inputs |
| `merc_macros` | `merc_derive_terms`, `merc_term`, `merc_ignore`, `merc_test` |
| `merc_tools` | the command-line flags and reporting every tool shares: `VersionFlag`, `VerbosityFlag`, `report_error`, `Console` |

The `lib.rs` of a crate lists its whole public interface as explicit `pub use` lines, so reading it is the fastest way to see what a crate offers.

## Step 2: Put the code in the right place

- **Tools are thin.** A binary under `tools/` parses arguments with `clap` derive, calls a library function and reports the result. Anything that could be tested without a command line belongs in a crate under `crates/`.
- **Layers point one way**: `core/` knows nothing of the others; `language/` and `rewriter/` build on it; `transition_systems/` builds on those. A dependency in the other direction means the code is in the wrong crate.
- **Typed terms only after the type checker.** Algorithms work on typed terms such as `DataExpression` or `PbesExpression`, obtained by lowering a checked specification or reading a binary file. The untyped `merc_syntax` trees end at `merc_typecheck`.
- **Modules are private and flattened**: `mod foo;` plus `pub use foo::Item;` in `lib.rs`, one line per item. Items that only the crate uses are `pub(crate)`.
- **Unsafe stays where it is.** Most crates are `#![forbid(unsafe_code)]`. If a change seems to need `unsafe`, look for the safe interface in `merc_aterm` or `merc_unsafety` first, and read the `unsafe-verify` skill if it really does.

## Step 3: Use terms as they are meant to be used

Terms live in a global pool with maximal sharing and are garbage collected ([Terms](https://mercorg.github.io/merc-website/developer/rewriting/aterms/)). That gives the following rules.

- **Owned and borrowed.** Every term type comes as a pair: `DataExpression` is a garbage-collection root, `DataExpressionRef<'a>` is a free copy of the index that borrows from a root. Take `&..Ref<'_>` (or a generic `Term`) in parameters, read with the accessors, which return `..Ref`, and call `protect()` only for what must outlive the borrow, usually the return value. `copy()` borrows an owned term.
- **Do not protect in a loop.** Creating and dropping an owned term touches the protection set. Many terms that live together go into one `Protected<Vec<..>>`.
- **Equality is identity.** Comparing and hashing a term is constant time, so use terms as keys of maps and sets directly. Never compare terms by walking them, and never by their printed form.
- **Terms are deep and shared.** A list of a million elements is a term nested a million deep, and a subterm can be reachable along exponentially many paths. Code that recurses over a term overflows the stack on the first, and code that does not remember what it has seen never finishes on the second. Use the traversals of Step 4.
- **New term types are generated.** A struct with a single `term: ATerm` field, marked `#[merc_term(predicate)]` in a module marked `#[merc_derive_terms]`, gets its `..Ref` type, the conversions and the accessors on both. Do not write those by hand. Mark constructors `#[merc_ignore]`.
- **Match on `kind()`.** `DataExpression::kind()` and `PbesExpression::kind()` return an enum of borrowed views (`DataExpressionKind`, `PbesKind`). A `match` on it is exhaustive, which a chain of `is_data_application(..)` checks is not.

## Step 4: Traverse with the existing visitors

All of these use an explicit stack, stop early, and can skip subterms. Pick one; do not write a recursive function next to them.

| To do this | Use |
|---|---|
| inspect a data expression top-down | `merc_data::visit_data_expr` with a closure, or implement `DataExpressionVisitor` when node kinds differ |
| inspect a PBES expression top-down | `merc_pbes::visit_pbes_expr` / `PbesExpressionVisitor` |
| compute a value bottom-up over a PBES expression | `merc_pbes::fold_pbes_expr`, which memoises per distinct subexpression |
| build a new term from an old one | `merc_aterm::TermBuilder` |
| walk or rewrite an untyped syntax tree | the `Traverse` trait of `merc_syntax` |
| wrapped mCRL2 terms in `tools/mcrl2` | the visitors in `crates/mcrl2/src/visitor.rs` |

They share one protocol from `merc_utilities`: a callback returns a `Visit`, which is an error, `ControlFlow::Break(result)` to stop the whole traversal, or `ControlFlow::Continue` with a `Step`: `Into(context)` to descend, `Prune` to skip the children, `Replace(node)` where the traversal rewrites. Three things follow from it:

- The context is `Copy` and is handed from a node to its children. State that grows along a path, such as the variables bound above the current node, lives in the visitor itself.
- A visitor whose work per node is not trivial keeps a set of the nodes it has seen and returns `Step::Prune` for repeats.
- A new kind of term or tree gets a visitor of this shape, with its tests, in the crate that defines it. Callers then never need their own traversal.

## Step 5: Keep indices and ids apart

- States, labels, blocks, priorities and the like are `TagIndex<usize, SomeTag>` aliases (`StateIndex`, `LabelIndex`, `BlockIndex`). Take and return those, never a bare `usize`, and never cast one kind to another.
- An id into a table that only the table can hand out is a newtype with a private field, so that holding one proves it is valid.

## Step 6: Shape the interface

- **Smallest surface.** Start `pub(crate)`. An item becomes `pub` when something outside the crate uses it, and then it has a contract in its doc comment. Breaking a published crate's API fails the semver check in CI.
- **Names say what the thing is** in the vocabulary of the domain or of the website page that defines it, not how it is implemented. Constructors are `new`, `from_*` or `with_*`.
- **Parameters cannot be confused.** Two `usize` or `bool` arguments in a row become a struct with named fields, an enum, or typed indices.
- **Closures are named type parameters** with a `where` clause when the function is part of an interface, which is the prevailing style here.
- **One concept, one place.** If callers must do two things in the right order, that is one function. If two values only make sense together, such as an index and the table it points into, they are one type.
- **Nothing speculative.** No trait with one implementation "for later", no option nobody sets, no generic parameter with one instantiation, no placeholder.
- **Performance changes come with a measurement** (`benchmark` skill). That includes a small-vector or a cache added "to be safe".

## Step 7: Errors and panics

- A library function that can fail on its input returns a `Result` with the crate's `thiserror` enum; add a variant rather than a second error type. `MercError` is the catch-all at the tool boundary and in tests.
- A panic states a broken invariant, never bad input: `expect("why this cannot fail")` rather than `unwrap()`, and `unreachable!("...")` with the reason.
- `debug_assert!` checks invariants that are too expensive for release. Remember that release builds also drop overflow checks.
- Log with the `log` macros; tracing in hot paths goes through `debug_trace!`, which compiles to nothing unless its feature is on.

## Step 8: Test next to the code

- Unit tests go in a `#[cfg(test)] mod tests` of the file, integration tests in the crate's existing `tests/*.rs` files. Extend a file that covers the area before creating a new one. Import each item explicitly, not `use super::*`.
- A bug fix comes with a test that fails without it.
- Prefer `merc_utilities::random_test`, which states the postcondition over arbitrary inputs and prints a seed to reproduce with `MERC_SEED=<seed>`. Compare against a simple reference implementation where one exists, as the rewriters do against each other.
- `#[merc_test]` is `#[test]` with the test logger set up; `test_case` parameterises over example files.
- Mark what is too slow for miri `#[cfg_attr(miri, ignore)]`.
- Do not commit binary fixtures, and do not add a test that runs a tool as a subprocess, or a compile-fail harness, for something a library-level test can show.

## Step 9: Document and check

- A doc comment gives the contract: what the item does, what it requires and guarantees, when it panics. It describes the code as it is, not how it got there. Algorithms, design decisions and measurements go on the website; the `documentation` skill says what goes where.
- A comment inside a function says why, when that is not visible from the code.
- Format with `cargo +nightly fmt --all`, with one `use` per item and no glob imports, then run the `check` skill.

## Before you call it done

- [ ] Nothing in the change exists elsewhere under another name.
- [ ] No recursion over a term or syntax tree, and shared subterms are handled once.
- [ ] No owned term created per iteration of a hot loop.
- [ ] No bare `usize` where a typed index exists, and no casts between index types.
- [ ] Every new `pub` item has a user outside its crate and a documented contract.
- [ ] The tool, if any, only parses arguments and calls the library.
- [ ] New behaviour and every bug fix has a test, in an existing test file where one fits.
- [ ] No new dependency, no `unsafe` in a crate that forbids it, nothing newer than the minimum Rust version.
