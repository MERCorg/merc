# AGENTS.md

The `tools/gui` workspace: graphical tools, kept separate because the Slint GUI framework adds several hundred dependencies that the rest of merc should not build. The repository-wide guidance in `../../AGENTS.md` applies here as well, including its skills: "the `X` skill" means `../../.agents/skills/X/SKILL.md`; this file only adds what differs.

## Layout

| Path | Contents |
|---|---|
| `ltsgraph` | the `merc-ltsgraph` tool; its user interface is written in Slint (`ui/*.slint`) and compiled by `build.rs` |
| `ltsgraph-lib` | the `merc_ltsgraph_lib` crate: layout and rendering of an LTS, independent of the GUI |
| `ltsgraph-lib/benchmarks` | criterion benchmarks for the library |

Crates from the root workspace (`merc_lts`, `merc_io`, ...) are path dependencies, so a change under `../../crates` can break this workspace. Check it whenever such a change touches an API used here.

## Building

- Run every cargo command from this directory. `.cargo/config.toml` shares the root `target/` directory; never pass `--target-dir` or set `CARGO_TARGET_DIR`, which would rebuild everything a second time.
- On Linux the build needs `libfontconfig1-dev` and `libfreetype6-dev`.
- The minimum Rust version is set in this workspace's `Cargo.toml` and is one release ahead of the root workspace.
- Put logic that can be tested without a display in `ltsgraph-lib`. Running `merc-ltsgraph` needs a display, so in a headless environment only build it and run the tests.

## Testing

```bash
cargo nextest run --no-fail-fast -- --include-ignored
cargo bench --no-run      # what CI checks for the benchmarks
```

Formatting, clippy, `cargo deny` and doc tests run per workspace, so repeat them here (see the `check` skill).
