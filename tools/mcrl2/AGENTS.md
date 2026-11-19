# AGENTS.md

The `tools/mcrl2` workspace: merc functionality that links against the mCRL2 C++ toolset. The repository-wide guidance in `../../AGENTS.md` applies here as well, including its skills: "the `X` skill" means `../../.agents/skills/X/SKILL.md`; this file only adds what differs.

## Layout

| Path | Contents |
|---|---|
| `crates/mcrl2` | Rust wrappers around the FFI, e.g. aterms, LPSs and PBESs from mCRL2 |
| `crates/mcrl2-macros` | proc macros for the wrappers |
| `crates/merc_lps`, `crates/merc_pbes` | LPS exploration and PBES algorithms built on the wrappers. This `merc_pbes` is a different crate from the one in `../../crates/language/pbes`, which has the same name and works on merc's own terms |
| `lps`, `pbes` | the `merc-lps` and `merc-pbes` command-line tools |
| `crates/xtask` | sanitizer runs for this workspace |

The FFI itself is the `mcrl2-sys` crate (`cxx` bindings plus the mCRL2 sources), pinned by git revision in `Cargo.toml`. To work on it locally, clone it to `crates/mCRL2-sys` and enable the commented `[patch]` section in `Cargo.toml`; it is excluded from the workspace members.

Crates from the root workspace (`merc_aterm`, `merc_data`, ...) are path dependencies, so a change under `../../crates` can break this workspace. Check it whenever such a change touches an API used here.

The wrapped mCRL2 terms are not `merc_aterm` terms, so the root workspace's visitors do not apply to them. `crates/mcrl2/src/visitor.rs` has visitors of the same shape for the wrapped data and PBES expressions; use those rather than recursing by hand. New algorithms that do not need the C++ toolset belong in the root workspace, on merc's own typed terms.

## Building

- Run every cargo command from this directory. `.cargo/config.toml` shares the root `target/` directory; never pass `--target-dir` or set `CARGO_TARGET_DIR`, which would rebuild everything a second time.
- A C++ compiler is required (`CC`/`CXX` select it on Linux). The first build compiles a large part of mCRL2 and takes several minutes; `RUSTC_WRAPPER=sccache` caches it.
- The `cpptrace` feature of the `mcrl2` crate adds stack traces for mCRL2 exceptions and assertions and needs `cmake` on `PATH`.
- The minimum Rust version is set in this workspace's `Cargo.toml` and differs from the root workspace.

## Testing

```bash
cargo nextest run --no-fail-fast -- --include-ignored
cargo +nightly xtask address-sanitizer test --no-fail-fast
```

Tests that compare with the mCRL2 command-line tools need `MCRL2_PATH` pointing at a built mCRL2 `build/stage/bin`, and are skipped when it is unset. A local run without it does not cover them; say so when reporting. CI builds mCRL2 with `-DMCRL2_ENABLE_EXPERIMENTAL=ON -DMCRL2_ENABLE_GUI_TOOLS=OFF` and runs these tests with the filter `mcrl2`.

Formatting, clippy, `cargo deny` and doc tests run per workspace, so repeat them here (see the `check` skill).
