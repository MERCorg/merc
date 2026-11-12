---
name: check
description: Run the merc validation suite (build, tests, formatting, clippy, cargo deny) the same way CI does, across all three cargo workspaces. Use before committing, after finishing a code change, or when asked to run the tests, lints, or checks.
---

# Validate changes

The repository contains **three independent cargo workspaces**. A change is only fully checked when the affected workspaces all pass:

| Workspace | Directory | Contents |
|---|---|---|
| root | repository root | all `crates/*` and the `merc-lts`, `merc-rewrite`, `merc-sym`, `merc-vpg` tools |
| mCRL2 | `tools/mcrl2` | `merc-lps`, `merc-pbes` and their crates |
| GUI | `tools/gui` | `merc-ltsgraph` (needs `libfontconfig1-dev`, `libfreetype6-dev` on Linux) |

For a change scoped to one crate, run tests for that crate first (`-p <crate>`), then the workspace-wide checks before committing.

The `tools/mcrl2` and `tools/gui` workspaces each have a `.cargo/config.toml` setting `target-dir = "../../target"`, so their builds already land in the shared root `target/` directory. Never pass an explicit `--target-dir` (or set `CARGO_TARGET_DIR`) when building in those workspaces — it overrides that shared location and produces a second, duplicate build of every dependency.

## Instructions

### Step 1: Tests

CI uses nextest and includes ignored tests:

```bash
cargo nextest run --no-fail-fast -- --include-ignored
```

If `cargo-nextest` is not installed, either install it (`cargo install cargo-nextest`) or fall back to `cargo test -- --include-ignored`. Repeat inside `tools/mcrl2` and `tools/gui` when those workspaces are affected. When debugging a failure, rerun with `RUST_BACKTRACE=full RUST_LOG=debug`.

A nightly CI job runs the same suite in **release mode** with `RUST_MIN_STACK=104857600` — release builds disable `debug_assertions` and overflow checks, so a debug-only pass can still fail there, and nobody sees it until the next day. Run `RUST_MIN_STACK=104857600 cargo nextest run --release --no-fail-fast -- --include-ignored` when a change touches arithmetic, assertions, or deep recursion.

### Step 2: Formatting

Formatting **requires nightly rustfmt** — `rustfmt.toml` uses `imports_granularity = "Item"`, which stable rustfmt silently ignores:

```bash
cargo +nightly fmt --all               # apply
cargo +nightly fmt --all -- --check    # what CI verifies
```

Run in each affected workspace directory (rustfmt is per-workspace).

### Step 3: Clippy

```bash
cargo clippy --all-targets
```

CI reports clippy warnings on the pull request. Introduce no new ones, and fix them rather than `#[allow]`-ing them; run in each affected workspace. Warnings that were already there in code you did not touch are not part of your change: leave them, and compare against the base commit if you are unsure which are yours.

### Step 4: Documentation (when doc comments or public APIs changed)

CI runs doc tests and builds documentation with rustdoc warnings promoted to errors:

```bash
cargo test --doc
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items
```

CI documents private items too, so a broken intra-doc link in the docs of a private item, or a link from a public item to a private one, fails CI even when everything else passes.

### Step 5: Dependency policy (only when Cargo.toml changed)

License and dependency checks are enforced with `cargo deny` (config in `deny.toml`, one per workspace):

```bash
cargo deny check
```

### Reporting

Say which workspaces and which steps you ran, quote the summary line of each, and name what you skipped and why. A failure that also occurs on the unmodified base commit is reported as pre-existing, with the command that shows it, not silently ignored.

### Additional CI-only gates to keep in mind

- MSRV is pinned by `rust-version` in each workspace's `Cargo.toml` (the GUI workspace is one release ahead) and verified with `cargo msrv verify`; avoid language and standard library features newer than it.
- `cargo semver-checks` runs on the published crates (the exclusions are listed in `.github/workflows/semver.yml`) — breaking a public API of one of those requires a version bump, which is one more reason to keep new items `pub(crate)` until something outside the crate needs them.
- miri runs on the whole root workspace in CI, so a new test that is slow or does file or process I/O needs `#[cfg_attr(miri, ignore)]`.
- Tests that compare against the upstream mCRL2 toolset read `MCRL2_PATH` (pointing at mCRL2's `build/stage/bin`) and skip when it is unset — a locally green run does not cover them. CI runs them with the filter `mcrl2`.
- Changes to `unsafe` or concurrent code need the deeper verification described in the `unsafe-verify` skill (miri, loom, sanitizers, kani).
