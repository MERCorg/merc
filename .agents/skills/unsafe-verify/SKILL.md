---
name: unsafe-verify
description: Verify unsafe or concurrent code with miri, loom, kani, and the address/thread sanitizers, matching what merc CI runs. Use when a change touches unsafe blocks, atomics, locks, allocators, or the aterm, sharedmutex, unsafety, or collections crates.
---

# Verify unsafe and concurrent code

Most `unsafe` code is in `merc_aterm`, `merc_unsafety` and `merc_sharedmutex`; a few other crates (the LTS, rewriter and reduction crates) contain some, mostly to store terms in a protected container. Before verifying new `unsafe`, ask whether it is needed: an existing safe interface (`Protected`, the `merc_unsafety` containers) usually covers the case, and a crate that is `#![forbid(unsafe_code)]` stays that way.

Plain tests are not sufficient evidence for `unsafe` or lock-free code in this repository — CI additionally runs miri, loom, kani, and both sanitizers. Run the relevant subset locally before committing; scope to the affected crate with `-p <crate>` because these runs are slow.

Which verifier applies:

- **miri** — any `unsafe` code (undefined behaviour, aliasing, leaks).
- **loom** — concurrency permutations; the loom-gated tests live in `crates/core/sharedmutex`. A new lock-free structure elsewhere needs loom tests of its own, with `loom` in their names so that the CI filter picks them up.
- **sanitizers** — ASan for memory errors, TSan for data races, on the real (non-interpreted) build.
- **kani** — proof harnesses in `crates/core/unsafety` and `crates/core/number`.

## Instructions

### Miri

Needs nightly with the `miri` and `rust-src` components (`rustup +nightly component add miri rust-src`):

```bash
MIRIFLAGS="-Zmiri-disable-isolation --cfg chacha20_force_soft" \
  cargo +nightly miri nextest run --no-fail-fast -p <crate>
```

(`cargo +nightly miri test -p <crate>` works without nextest.) CI runs miri on the whole root workspace except the proc-macro crate `merc_macros`. Note that CI deliberately does **not** pass `--include-ignored` to miri — long-running ignored tests stay excluded — so do not add it here either. Miri is very slow; do not run it workspace-wide unless asked.

When writing tests for unsafe code, prefer tests that state the postcondition explicitly as an assertion rather than comparing against one memorized output: randomized tests (`merc_utilities::random_test`, seed reproducible via `MERC_SEED=<seed>`) where possible, or small deterministic tests for miri — randomized stress tests are far too slow under miri and are marked `#[cfg_attr(miri, ignore)]`, so give miri its own small tests of the same postcondition (see `crates/core/aterm/tests/miri_aterm.rs` for the pattern).

### Loom

Loom tests are behind `--cfg loom` and named with `loom`; CI runs them in release mode:

```bash
RUSTFLAGS="--cfg loom" cargo nextest run --no-fail-fast --release -- --include-ignored loom
```

### Sanitizers (via xtask)

The `xtask` crate wraps the RUSTFLAGS/target setup. Needs nightly with `rust-src`:

```bash
cargo +nightly xtask address-sanitizer test --no-fail-fast   # ASan; also run in tools/mcrl2 if affected
cargo +nightly xtask thread-sanitizer test --no-fail-fast    # TSan
```

Everything after `address-sanitizer`/`thread-sanitizer` is passed through to cargo, so `-p <crate>` works. The sanitizers only run on Linux.

### Kani

CI proves the harnesses in `crates/core/unsafety` and `crates/core/number`. Locally, only if [Kani](https://model-checking.github.io/kani/) is installed; run in the crate directory:

```bash
cd crates/core/unsafety && cargo kani   # likewise crates/core/number
```

### The `SAFETY:` comment

Every `unsafe` block and `unsafe impl` states the invariant that makes it sound, and where that invariant is established: "`term` is pushed into the container on the next line", not "this is safe". An `unsafe fn` documents its contract under `# Safety`. If the invariant cannot be written down in a sentence or two, the abstraction boundary is in the wrong place.

### Reporting

State explicitly which verifiers were run and which were skipped (and why, e.g. tool not installed) — a green `cargo test` alone does not validate an unsafe change here.
