# AGENTS.md

Guidance for coding agents working in this repository. Tool-specific files (such as `CLAUDE.md`) only import this file.

## Project

MERC is a Rust toolset and library collection for model checking — LTS reduction/refinement, term rewriting, symbolic state spaces, and parity games — inspired by mCRL2, LTSmin, and CADP. Tools are thin CLI wrappers; shared logic always lives in library crates under `crates/`.

## Critical invariants

- **Three independent Cargo workspaces**: the root, `tools/mcrl2`, and `tools/gui`. Cargo commands operate on one workspace at a time — run them from the corresponding directory. Each sub-workspace has its own `AGENTS.md`.
- **Tests run with `cargo nextest run`, not `cargo test`** (doc tests separately via `cargo test --doc`).
- **Format with `cargo +nightly fmt --all`** — `rustfmt.toml` uses nightly-only options. Imports are one `use` per item, never grouped with `{}`. No glob imports (`use path::*;`), including `use super::*;` in `#[cfg(test)]` modules — import each item explicitly. Exceptions: `pub use module::*;` re-exports that flatten a crate's own modules (in `lib.rs` or a `mod.rs`), and the `use super::*;` / `pub use inner::*;` scaffolding inside a `#[merc_derive_terms] mod inner` block. Because a flattened (private `mod` + glob re-export) module is not a public doc target, avoid module-level `//!` docs on those — they never render in `cargo doc`, so document the items instead. Only the crate root's `//!`/`#![doc]` and `pub mod` modules render their module docs.
- **Every `unsafe` block needs a `// SAFETY:` comment** explaining the invariant being upheld. Unsafe code is concentrated in `merc_aterm`, `merc_unsafety` and `merc_sharedmutex`, behind safe interfaces, and checked by miri, sanitizers, loom, and Kani in CI. Crates that are `#![forbid(unsafe_code)]` stay that way.
- **No new dependencies without clear justification**; `cargo deny` gates licenses. The minimum Rust version (`rust-version` in each workspace's `Cargo.toml`) is checked in CI, so do not use newer language or library features.
- **Reuse before writing**: shared helpers, collections, traversals and term types already exist in the `core/` crates. Read the `write-code` skill before adding code.
- Errors use the crate's existing `thiserror`-based type (or `MercError` from `merc_utilities`); CLI tools use `clap` derive with the shared `merc_tools` helpers.

## Common commands

```bash
cargo build [--release]
cargo +nightly fmt --all
cargo clippy --all-targets
cargo nextest run --no-fail-fast -- --include-ignored   # full suite, as CI runs it
cargo nextest run -E 'test(name_substring)'             # single test
cargo doc --no-deps
cargo xtask --help        # automation: sanitizers, coverage, packaging, tool tests
```

## Architecture

Crates are grouped by layer under `crates/`: `core/` (terms, collections, I/O, numbers, unsafe primitives, utilities), `language/` (syntax and type checking), `rewriter/` (data terms, Sabre, enumeration) and `transition_systems/` (LTS, exploration, reduction, refinement, symbolic, parity games).

Everything in the data/rewriting stack is built on `merc_aterm`: a thread-safe global term pool with maximal sharing, so structural equality implies pointer equality and term identity *is* pointer equality; garbage collection tracks live roots via thread-local protection sets. On top sit data expressions and parsing (`merc_data`, `merc_syntax`), the Sabre rewriter (`merc_sabre`, plus a runtime-compiling variant), and the LTS/symbolic layers (`merc_lts`, `merc_explore`, `merc_reduction`, `merc_refinement`, `merc_symbolic`, `merc_vpg`). The untyped `merc_syntax` tree ends at the type checker: algorithms work on typed terms (e.g. `merc_pbes::PbesExpression`, matched through its borrowed `kind()` view), obtained by lowering a checked specification or reading a binary file, and never consume `merc_syntax::DataExpr`. Terms give constant-time equality and hashing and no recursive drop, so deep expressions such as enumerated quantifiers are safe. `TagIndex` newtypes keep state/action/priority indices apart at compile time — never cast between them. The `merc_derive_terms` macro of `merc_macros` generates term-subtype boilerplate; use it instead of hand-rolling conversions. Terms and syntax trees are traversed with the existing visitors, which use an explicit stack, rather than with recursive functions.

## Skills and roles

Task-specific instructions are skills in the Agent Skills format (YAML frontmatter with `name` and `description`, then Markdown). Before starting a task listed below, read the skill file and follow it rather than re-deriving its content. When a skill or this file refers to "the `X` skill", it means `.agents/skills/X/SKILL.md`.

| Skill file | Read when |
|---|---|
| `.agents/skills/write-code/SKILL.md` | implementing a feature, fixing a bug, refactoring, or adding a public item: where code belongs, what to reuse, terms, visitors, indices, API shape, errors and tests |
| `.agents/skills/check/SKILL.md` | before committing or after finishing a code change: build, tests, formatting, clippy and `cargo deny` across the three workspaces, as CI runs them |
| `.agents/skills/benchmark/SKILL.md` | benchmarking, measuring performance, or checking for regressions with criterion baselines |
| `.agents/skills/run-tools/SKILL.md` | running or demoing a merc tool on the bundled examples, or checking a change end to end |
| `.agents/skills/unsafe-verify/SKILL.md` | a change touches `unsafe`, atomics, locks, allocators, or the aterm, sharedmutex, unsafety or collections crates: miri, loom, Kani and sanitizers |
| `.agents/skills/review/SKILL.md` | reviewing a diff, PR, commit or design, including your own: correctness first, then design against the `write-code` skill |
| `.agents/skills/documentation/SKILL.md` | writing doc comments, a crate README or a design explanation, or changing behaviour that a website page describes |

Role prompts for a review loop between two separate agents live in `.agents/agents/`: `review-adversary.md` tries to break a change with failing tests, and `implementor.md` answers each finding with evidence. Give each role its own agent or context when your tool supports that, with the role file as its instructions.

Theory, algorithms and design decisions are documented on the website (`~/merc-website`, served at https://mercorg.github.io/merc-website/), not in doc comments. Lean formalisations live in `~/merc-verified`.

## Change hygiene

- Prefer small, focused patches that follow nearby patterns; no placeholders or speculative abstractions.
- Add or update tests for bug fixes and new behavior; grammar changes in `merc_syntax` require updating the related pest rule tests.
- Keep documentation concise: explain **why** a choice was made rather than repeating what the code shows. Avoid duplication, excessive linking to other structs, and internal/implementation details. Read the `documentation` skill for where an explanation belongs.
- Occasional AI-generated boilerplate is acceptable, but slop is strictly forbidden (see `README.md`).
