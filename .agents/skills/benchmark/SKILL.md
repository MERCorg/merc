---
name: benchmark
description: Build and run the criterion benchmarks in the crates/*/*/benchmarks crates, and compare performance before and after a change using saved baselines. Use when asked to benchmark, measure performance, or check for regressions.
---

# Benchmarks

Benchmarks are criterion-based and live in dedicated sub-crates so they don't burden normal builds:

| Package | Measures |
|---|---|
| `benchmarks_aterm` | term library (`crates/core/aterm`) |
| `benchmarks_sharedmutex` | `bf_sharedmutex` and lock implementations (`crates/core/sharedmutex`) |
| `benchmarks_unsafety` | sharded hashmap and other unsafe primitives (`crates/core/unsafety`) |
| `benchmarks_utilities` | counters and utility types (`crates/core/utilities`) |
| `benchmarks_enumerate` | data enumerator (`crates/rewriter/enumerate`) |
| `benchmarks_sabre` | the rewriters (`crates/rewriter/sabre`), in three targets: `benchmark_sabre`, `benchmark_machine_word`, `benchmark_number_encoding` |
| `benchmarks_lts` | LTS algorithms (`crates/transition_systems/lts`) |
| `benchmarks_symbolic` | symbolic state spaces (`crates/transition_systems/symbolic`) |
| `benchmarks_vpg` | parity game solving (`crates/transition_systems/vpg`) |

The GUI workspace (`tools/gui`) has `benchmarks_ltsgraph`; run it from that directory.

## Instructions

### Step 1: Pick scope

Never run all benchmarks for a scoped change — a full run takes very long. Select the package matching the changed crate:

```bash
cargo bench -p benchmarks_sharedmutex
# narrow further to one criterion filter:
cargo bench -p benchmarks_sharedmutex -- <name-substring>
```

CI only verifies benchmarks compile; you can do the same quickly with `cargo bench --no-run`.

### Step 2: Compare against a baseline for before/after claims

A single run proves nothing. Save a baseline on the **unmodified** code, then compare.

Build the unmodified code in a separate `git worktree` of the base commit, run the benchmark there with `--save-baseline before`, and run it on the change with `--baseline before`. Criterion keeps baselines under `target/criterion`, so both runs must see the same directory. A worktree leaves the working tree alone. Do not `git stash` or switch branches to get the base when the tree holds changes that are not yours: other uncommitted work is stashed along with it.

Criterion prints the relative change and whether it is statistically significant. HTML reports land in `target/criterion/`. Make sure both runs use the same benchmark filter and machine conditions (no parallel builds or tests running).

### Step 3: Rule out the two known sources of noise

Criterion's verdict only covers run-to-run variance. Two effects in this repository are larger than most optimisations and are not variance, so they survive any number of samples:

- **Code generation.** The release profile uses thin LTO, and an unrelated edit can move a hot function across a codegen unit and change a benchmark by 10–40%. For an A/B comparison build both sides with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_LTO=fat`.
- **Term pool state.** The term pool is global, so a benchmark inherits the pool that earlier benchmarks in the same process filled. Run one benchmark per process (one filter per `cargo bench` invocation) and alternate the two builds instead of running all of one and then all of the other.

When a difference matters and is below that range, repeat the A/B and report the spread.

Benchmarks of the rewriters default to the binary encoding of numbers. Compare with mCRL2 only under the machine-word encoding, which is what mCRL2 uses.

Large benchmarks (state space exploration, symbolic reachability) can use many gigabytes. On a machine where running out of memory takes down more than the benchmark, cap it, for example `systemd-run --user --scope -p MemoryMax=12G cargo bench ...`.

### Step 4: Report honestly

Quote criterion's own change estimate and significance verdict, not just the raw times, and say how the two sides were built and run. Mention that results from a shared/dev machine (codespace) are noisy — treat small differences (< ~5%) as noise unless they repeat across runs. If the change does not measurably help, say that; a result that belongs on the website's benchmark pages goes there (`documentation` skill).
