# Agent instructions: `tools/gui`

This is a separate cargo workspace for `merc-ltsgraph`, the Slint-based LTS visualiser. It is
separate because Slint adds about 600 dependencies. The repository-wide instructions in
[../../AGENTS.md](../../AGENTS.md) also apply; this file only adds what differs here. Run cargo from
this directory.

## Layout

| Path | Contents |
|---|---|
| `ltsgraph` | the `merc-ltsgraph` binary: CLI, Slint app, wgpu setup |
| `ltsgraph/ui/application.slint` | the only UI definition; `build.rs` compiles it via `slint-project.json` |
| `ltsgraph/data` | window icon, the embedded Noto Sans font and its licence |
| `ltsgraph-lib` | graph layout, viewer state, and renderer backends (tiny-skia, femtovg, vello, vello_cpu) |
| `ltsgraph-lib/benchmarks` | criterion benchmarks |

## Things to know

- On Linux, building requires `libfontconfig1-dev` and `libfreetype6-dev`.
- The workspace has its own `Cargo.lock`, `deny.toml` and MSRV (`rust-version` in `Cargo.toml`).
  `deny.toml` ignores several advisories pulled in through Slint and femtovg; check whether an entry
  is still needed before adding another.
- Keep the `femtovg` version in step with the one Slint uses (see the comment in `Cargo.toml`).
- The `merc_*` dependencies are path dependencies on `../../crates`. After changing those crates,
  build this workspace as well.
- The tests run headless: renderers are tested on the CPU, and the wgpu test skips itself when no GPU
  adapter is available. Running the app itself needs a display.
- Pick a renderer with `--viewer cpu|gpu|vello-cpu|vello-gpu` (the default is `cpu`). The vello
  renderers reject the Windows WARP software adapter on purpose.
- The installers (including the Windows MSI with WiX) are built from the root workspace by
  `cargo xtask package`. Their templates are in `crates/xtask`, not here.

## Skills

Read the matching skill before you start one of these tasks. Each one covers this workspace.

| Skill | Use for |
|---|---|
| [check](../../.agents/skills/check/SKILL.md) | build, tests, fmt, clippy, deny as CI runs them |
| [run-tools](../../.agents/skills/run-tools/SKILL.md) | running `merc-ltsgraph` on `examples/lts` |
| [benchmark](../../.agents/skills/benchmark/SKILL.md) | the layout and rendering benchmarks |
| [review](../../.agents/skills/review/SKILL.md) | reviewing a change |
