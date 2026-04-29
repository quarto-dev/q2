# CI disk space and the `[profile.ci]` Cargo profile

Date: 2026-04-28

This note explains why the q2 workspace has a `[profile.ci]` Cargo profile that is **only** used in `.github/workflows/test-suite.yml`, what's different from the default `dev` profile, and how to recover the full debug info when reproducing a CI failure locally.

If you stumbled here from a `target/` directory growing unexpectedly, or wondered why CI uses `--cargo-profile ci`, this is the right page.

## TL;DR

- CI on `ubuntu-latest` has very little disk space (~14 GB free after the runner image, partially recovered by a cleanup action).
- The default `dev` profile emits full debuginfo, which roughly **doubles** `target/` size on a workspace this big.
- We added a `[profile.ci]` profile (inherits from `dev`, strips most debuginfo) and the CI workflow uses it via `cargo nextest run --cargo-profile ci`.
- We also removed the redundant `cargo build` step from CI — `cargo nextest run --tests` already builds everything `cargo build` does, plus the test artifacts. (We initially tried `--all-targets` but that pulls in `harness = false` benches which nextest can't enumerate as tests; `--tests` is the correct flag.)
- We freed ~10 GB more on the runner by enabling `remove_tool_cache: true` on the existing free-disk-space step (no step uses `/opt/hostedtoolcache/`) and by pruning Docker images right after that step.
- **Locally, nothing changed.** `cargo build`, `cargo test`, and `cargo nextest run` (without `--cargo-profile ci`) still use the default `dev` profile with full debuginfo.

## What triggered this

Run [25062065055](https://github.com/quarto-dev/q2/actions/runs/25062065055/job/73418747660?pr=139) (PR #139, ubuntu-latest) failed with `No space left on device` during `cargo nextest run`, even though the existing `endersonmenezes/free-disk-space` step had already freed **18.8 GB** (Android 10.4 + .NET 4.6 + Haskell 3.7).

The previous mitigation (PR #55, 2026-03-17) bought us several months of headroom; the workspace has since grown past it.

## The two changes that need explaining

The runner-side cleanup (tool_cache + docker prune) is mechanical — see the workflow file. The two changes worth documenting in detail are the Cargo profile and the workflow build/test consolidation.

### Change 1: `[profile.ci]` Cargo profile

Added to root `Cargo.toml`:

```toml
[profile.ci]
inherits = "dev"
debug = "line-tables-only"

[profile.ci.package."*"]
debug = false
```

What this does:

- `inherits = "dev"` — start from the `dev` profile, including the existing `opt-level = "s"` wasm-bindgen workaround.
- `debug = "line-tables-only"` — emit only enough debug info for backtraces to show **file and line numbers**. Variable values and function-parameter info are dropped.
- `[profile.ci.package."*"] debug = false` — strip debug info entirely from **all dependencies**. Deps dominate `target/` size on this workspace, so this is where the bulk of the saving comes from.

#### What's lost in CI panic logs

Backtraces still show file/line per frame. They no longer show:

- Variable values
- Function-parameter values
- The full type names of generic instantiations beyond what rustc records as part of the symbol

In practice, panic output in CI logs only ever showed file/line anyway — variables aren't included unless you're attached with `gdb`/`lldb`. So this is essentially zero observable change for CI-log-only debugging.

#### How to get full debuginfo back locally

Just **don't pass `--cargo-profile ci`**. Every default cargo command uses the `dev` profile, which is unchanged:

```bash
# Full debuginfo (default behavior, unchanged)
cargo build
cargo test
cargo nextest run

# CI profile (matches what GitHub Actions runs)
cargo nextest run --cargo-profile ci
```

If you want to **reproduce a CI failure** under a debugger or with `RUST_BACKTRACE=full` and need rich frame info, run with the default `dev` profile. The `[profile.ci]` block does not affect any other profile.

#### Why `line-tables-only` instead of `debug = 0`

`line-tables-only` keeps file:line info in panic backtraces — readable in CI logs, no debugger needed.
`debug = 0` would drop that too, leaving only hex addresses, which makes any unexpected CI failure much harder to triage.

The disk savings between `line-tables-only` and `debug = 0` are minor (line tables are tiny relative to full debug info). The big saving is on dependencies, where we use `debug = false` because deps are usually irrelevant to triaging q2-specific failures.

### Change 2: Removed the redundant `cargo build` step in CI

The old workflow had:

```yaml
- name: Build
  run: cargo build
  env:
    RUSTFLAGS: "-D warnings"

- name: Test Rust code
  run: cargo nextest run
  env:
    RUSTFLAGS: "-D warnings"
```

Per the [nextest design docs](https://nexte.st/docs/design/how-it-works/):

> "cargo-nextest first builds all test binaries with `cargo test --no-run`"

And per the [Cargo book on `cargo build`](https://doc.rust-lang.org/cargo/commands/cargo-build.html#target-selection), `--tests` builds "all targets that have the `test = true` manifest flag set. By default this includes the library and binaries built as unittests, and integration tests. Be aware that this will also build any required dependencies, so the lib target may be built twice (once as a unittest, and once as a dependency for binaries, integration tests, etc.)."

That last clause is the key one — it confirms test-mode and non-test-mode rlibs are separate artifacts, which is what `cargo nextest run --tests` produces in one shot:

```yaml
- name: Test Rust code
  run: cargo nextest run --tests --cargo-profile ci
  env:
    RUSTFLAGS: "-D warnings"
```

#### Why `--tests`, not `--all-targets`

`--all-targets` is officially defined as `--lib --bins --tests --benches --examples`. We tried it first, but it caused CI to fail on `quarto-yaml`'s benches: `crates/quarto-yaml/benches/{memory_overhead,scaling_overhead}.rs` are declared `harness = false` and print prose reports rather than libtest output. Nextest enumerates each bench binary with `--list --format terse` and errors on the unrecognized output (`line "..." did not end with the string ": test" or ": benchmark"`).

The previous CI never built benches anyway — plain `cargo build` excludes them by default — so `--tests` matches the prior coverage exactly while still consolidating compilation into one nextest-driven step.

#### Why this is safe (verified against Cargo docs)

1. **Compilation coverage matches what `cargo build` was producing.** `cargo nextest run --tests` builds the lib (both as unittest and as a non-test dep for bins/integration tests), all bins (also as unittests), and all integration tests. That is a strict superset of plain `cargo build`'s default targets (lib + bins, non-test). Examples without `test = true` and benches were not built before either.
2. **`-D warnings` still fires.** `RUSTFLAGS` is a rustc env var, applied to every rustc invocation regardless of which cargo subcommand drives the build. Nextest invokes `cargo test --no-run` internally, which picks up `RUSTFLAGS` exactly as `cargo build` would.
3. **The redundancy that disappears:** `cargo build` and `cargo nextest run` share `target/debug/` (or in our case `target/ci/`) but **not artifacts** — library crates compiled with `--cfg test` have a different fingerprint and produce **separate `.rlib`s** alongside the dev-build ones. With ~35 crates, that duplication ran into multi-GB at peak disk.

#### Edge case to be aware of

`--tests` skips any target whose manifest sets `test = false`. The only such target in this repo is `crates/pampa/fuzz` (libfuzzer-sys is Linux/macOS only, so the crate is `exclude`d from the workspace — `cargo build` at workspace level wasn't building it either). If a future bin is added with `test = false` and is expected to be compile-checked in CI, either drop the `test = false`, add a separate `cargo build` step for that bin, or revisit this decision.

### Change 3: Profile-aware binary discovery in `quarto-lsp` integration test

`crates/quarto-lsp/tests/integration_test.rs` spawns the `q2` binary as a subprocess. The previous code hardcoded `target/debug/q2`, which broke under `--cargo-profile ci` (binary lands at `target/ci/q2`).

#### Why this isn't a misuse of profiles

`[profile.ci]` is a fully-supported Cargo pattern. The test was simply unaware of profiles. Cargo's `CARGO_BIN_EXE_<name>` env var would normally provide the production-bin path, but it's **package-scoped** — set only for integration tests in the same package as the bin. Our LSP test lives in `quarto-lsp` while the bin is defined in `quarto`, so that env var is never set here.

#### The fix

Derive the profile directory from the test binary's own location:

```rust
let binary_path = std::env::current_exe()
    .unwrap()
    .parent().unwrap()   // target/<profile>/deps
    .parent().unwrap()   // target/<profile>
    .join("q2")
    .with_extension(std::env::consts::EXE_EXTENSION);
```

Cargo always places the test binary in `target/<profile>/deps/`, so backing up two parents lands in the same `target/<profile>/` directory where the production `q2` is built. Profile-correct on Linux/macOS/Windows without env-var coupling or mtime heuristics.

#### Why not `assert_cmd`

`assert_cmd::cargo::cargo_bin("q2")` is the idiomatic crate-based answer and does exactly the same thing internally (with friendlier error handling and Windows `.exe` suffix logic). We chose the stdlib version for this fix because:

- The LSP test does **not** use any of `assert_cmd`'s value — no `.assert()`, no stdout/stderr matchers, no exit-code checks. It speaks JSON-RPC over stdio.
- The only function we'd touch is `cargo_bin()`, replacing 4 lines of stdlib with one dev-dep.

If a future test wants `cargo run --` style command-driving with output assertions, **switch to `assert_cmd` then** — `cargo_bin()` is its standard binary-discovery helper and pays for itself once `.assert()` joins the picture.

## All the cleanup levers we used

| Lever | Disk saving | Status |
|---|---|---|
| A. `[profile.ci]` debuginfo strip | ~30–50% of `target/` | Done. Biggest single lever, preserves panic backtraces. |
| B. Drop redundant `cargo build` | Multi-GB peak reduction | Done via `cargo nextest run --tests`. |
| C. `remove_tool_cache: true` on free-disk-space | ~6 GB | Done. Safe — no step uses `/opt/hostedtoolcache/`. |
| D. `docker image prune` + `docker builder prune` | ~3–8 GB | Done. Pre-pulled docker images aren't used by this job. |
| E. `remove_swap: true` on free-disk-space | ~4 GB | Held in reserve — small OOM risk for heavy linkers (deno_core, large LTO). |
| F. `df -h` diagnostic step | 0 GB | Held in reserve — adds log noise; useful for next failure triage if disk pressure returns. |
| G. Larger runner | 150 GB / 45 GB | Held in reserve — Posit `ubuntu-latest-4x` ($0.012/min) or `ubuntu-24.04-arm` (free for public repos, requires arm64 Rust toolchain). |

## Cross-references

- Previous CI cleanup: [PR #55](https://github.com/quarto-dev/q2/pull/55) (Mar 2026)
- Failed run that triggered this work: [run 25062065055](https://github.com/quarto-dev/q2/actions/runs/25062065055/job/73418747660?pr=139)
- Local Windows precedent (same pattern via `~/.cargo/config.toml`): see `memory://main/docs/rust-disk-space-strategy-for-windows-with-worktrees`
- Open follow-up tracking: `memory://main/tasks/optimize-q2-ci-workflow-disk-usage`

## External sources

- nextest: [How it works](https://nexte.st/docs/design/how-it-works/) — confirms nextest delegates compilation to `cargo test --no-run`.
- Cargo book: [`cargo test` target selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection) — what gets built by default.
- Cargo book: [Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) — `inherits`, `debug` levels, per-package overrides.
- Kobzol, June 2025: [Reducing Cargo target directory size with `-Zno-embed-metadata`](https://kobzol.github.io/rust/rustc/2025/06/02/reduce-cargo-target-dir-size-with-z-no-embed-metadata.html) — measured ~2× target size from debuginfo on a comparable workspace.
- `endersonmenezes/free-disk-space` action documentation — option semantics for `tool_cache`, `swap_storage`, etc.
