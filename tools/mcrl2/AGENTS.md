# Agent instructions: `tools/mcrl2`

This is a separate cargo workspace for `merc-lps`, `merc-pbes` and the Rust bindings to the mCRL2
C++ toolset. The repository-wide instructions in [../../AGENTS.md](../../AGENTS.md) also apply;
this file only adds what differs here. Run cargo from this directory.

## Layout

| Path | Contents |
|---|---|
| `crates/mcrl2` | safe wrappers over `mcrl2-sys` (aterms, data, LPS, PBES, rewriter) |
| `crates/mcrl2-macros` | proc-macro that derives the term wrappers |
| `crates/merc_lps`, `lps` | LPS exploration library and the `merc-lps` binary |
| `crates/merc_pbes`, `pbes` | PBES exploration, symmetry reduction and solving; the `merc-pbes` binary |
| `crates/xtask` | the sanitizer tasks for this workspace (`cargo +nightly xtask address-sanitizer test`) |

## Things to know

- The C++ code comes from the `mcrl2-sys` git dependency, pinned by `rev` in
  [Cargo.toml](Cargo.toml). There is no `build.rs` in this workspace: `mcrl2-sys` compiles mCRL2 with
  `cc` and `cxx` (C++20), so it needs a C++ compiler, and cmake only for the `cpptrace` feature. To
  change the bindings, clone mCRL2-sys into `crates/mCRL2-sys` and enable the commented-out
  `[patch]` in `Cargo.toml`. Do not commit that patch.
- The first build and every `cargo clean` recompile the template-heavy mCRL2 sources, which takes
  several minutes. Avoid `cargo clean`. Use `RUSTC_WRAPPER=sccache` when it is available.
- The workspace has its own `Cargo.lock`, `deny.toml` and MSRV (`rust-version` in `Cargo.toml`),
  separate from the root ones. `oxidd` is patched to a fork.
- Many `merc_*` dependencies are path dependencies on `../../crates`. After changing those crates,
  build this workspace as well.
- Tests read their inputs from the root `examples/` directory (`examples/mCRL2`, `examples/pbes`).
- Tests that compare with upstream mCRL2 tools pass silently when `MCRL2_PATH` is unset; see the
  check skill. Miri does not run on this workspace (FFI); the address and thread sanitizers do.
- `merc-pbes graph-symmetry` shells out to GAP (`gap` on `PATH`).

## Skills

Read the matching skill before you start one of these tasks. Each one covers this workspace.

| Skill | Use for |
|---|---|
| [check](../../.agents/skills/check/SKILL.md) | build, tests, fmt, clippy, deny as CI runs them, including the `MCRL2_PATH` tests |
| [run-tools](../../.agents/skills/run-tools/SKILL.md) | running `merc-lps` and `merc-pbes` on the bundled examples |
| [unsafe-verify](../../.agents/skills/unsafe-verify/SKILL.md) | FFI or `unsafe` changes: sanitizers (miri does not apply here) |
| [review](../../.agents/skills/review/SKILL.md) | reviewing a change |
