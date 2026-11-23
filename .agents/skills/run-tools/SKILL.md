---
name: run-tools
description: Build and run the merc command-line tools (merc-lts, merc-sym, merc-rewrite, merc-vpg, merc-lps, merc-pbes, merc-ltsgraph) against the bundled example inputs. Use when asked to run or demo a tool, reproduce behaviour end-to-end, or check that a change works on real input.
---

# Run the merc tools

Every tool is a cargo binary; run it with `cargo run --release --bin <tool> -- <args>`. Use `--release` for anything beyond trivial inputs — state-space exploration and reduction are orders of magnitude slower in debug mode. Ready-made inputs live in `examples/`.

| Tool | Workspace | Purpose | Example inputs |
|---|---|---|---|
| `merc-lts` | root | LTS reduction, comparison, refinement, conversion, composition | `examples/lts/*.aut`, `*.lts` |
| `merc-sym` | root | symbolic (LDD/BDD) reachability and reduction | `examples/ldd/*.ldd`, `examples/lts/*.sym` |
| `merc-rewrite` | root | rewriting with the naive, innermost, jitty and Sabre rewriters, for REC files and mCRL2 data specifications; `check` prints the stages of parsing and type checking | `examples/REC/rec/*.rec`, `examples/mCRL2/` |
| `merc-vpg` | root | (variability) parity game solving | `examples/vpg/*.pg`, `*.vpg` |
| `merc-lps` | `tools/mcrl2` | mCRL2 linear process specification exploration | `examples/mCRL2/` |
| `merc-pbes` | `tools/mcrl2` | PBES symmetry identification | `examples/pbes/` |
| `merc-ltsgraph` | `tools/gui` | GUI visualization of LTSs (needs a display; skip in headless environments) | `examples/lts/` |

For the `tools/mcrl2` and `tools/gui` binaries, run cargo from that workspace directory. Both workspaces already point their `target-dir` at the shared root `target/` (via `.cargo/config.toml`), so never pass an explicit `--target-dir` — doing so builds a separate, duplicate copy of everything instead of reusing the shared target directory.

Functionality that shells out to the upstream mCRL2 toolset (e.g. symbolic exploration in `merc-lps`, refinement comparisons) reads the `MCRL2_PATH` environment variable, which must point at a built mCRL2 `build/stage/bin` directory. When it is unset those code paths are skipped or unavailable — say so in your report instead of concluding they work.

## Instructions

### Step 1: Discover the exact CLI

Subcommands and flags change; always confirm with `--help` before constructing a command:

```bash
cargo run --release --bin merc-lts -- --help
cargo run --release --bin merc-lts -- reduce --help
```

All tools share the global flags `--timings` and `-q`/`-v`/`-d`/`--trace` for verbosity, which come from `merc_tools`.

### Step 2: Run against an example

Known-good smoke tests:

```bash
# Print information about an LTS
cargo run --release --bin merc-lts -- info examples/lts/abp.aut

# Reduce an LTS modulo an equivalence (see reduce --help for the list)
cargo run --release --bin merc-lts -- reduce branching-bisim examples/lts/abp.aut --output /tmp/reduced.aut

# Count the reachable states of a symbolic LTS (29,641 for this one)
cargo run --release --bin merc-sym -- reachability examples/ldd/anderson.4.ldd

# Rewrite the terms of a REC specification
cargo run --release --bin merc-rewrite -- rewrite innermost examples/REC/rec/fibonacci05.rec
```

The `examples/ldd` models are sorted by size in their filename suffix (`anderson.4` is small, `anderson.8` is large) — start small.

### Step 3: Verify the outcome

Check the exit code and that the reported numbers (states, transitions, timing) are plausible; when a change should alter behaviour, run the same command on `main` or with the old flag for comparison rather than eyeballing a single run. A tool that exits 0 has not necessarily done the right thing — inspect the output file or reported statistics before claiming success.
