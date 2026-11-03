# corgi as a build cache for q2: research notes (round 1)

**Date:** 2026-11-03. **Strand:** bd-corgi-build-8fb3mzki.
**Plan / working log:** `claude-notes/plans/2026-11-03-corgi-build-exploration.md`
has the step-by-step account. This note records the findings and a
baseline, so that a later round can be compared with this one.
**Branch:** `2026-11-corgi-build-caching-research` on origin. It contains
`corgi.toml`, the generator, and the q2-side changes. It was **not
merged** and is not meant to be.
**Verdict:** don't adopt yet. Re-evaluate in a few months (target:
early 2027 or later).

## Environment

| | |
|---|---|
| Machine | Apple M5 Max, 18 cores, 128 GB RAM, macOS 26.6.2 |
| q2 | `main` at `f4a385337`, workspace v0.39.0, 59 packages in `cargo metadata --no-deps` |
| Rust | `nightly-2026-04-28` (from `rust-toolchain.toml`; corgi downloads its own copy) |
| corgi | `corgi-build` 0.1.99 (upstream commit `49254b6`), plus the 5 local commits (6 changes) in `2026-11-03-corgi-build-caching/corgi-patches/` |
| corgi store | `/Users/Shared/corgi` (default); C toolchain from corgi's pinned Zig 0.16.0 and macOS SDK 15.0 |

## What corgi is (one paragraph)

[corgi](https://github.com/ConradIrwin/corgi) is a cargo-compatible
build driver. Cargo still resolves dependencies; corgi runs rustc and
build scripts itself. It provides:

- one content-addressed store per machine, shared by every checkout and
  worktree;
- workspace-wide feature unification;
- rustc, build scripts and proc macros sandboxed with no network, a
  whitelisted env, and reads limited to the package's own `.rs` files
  plus declared `[extra-inputs]`;
- rejection of any artifact that contains the workspace path;
- nextest-style parallel tests with cached passes;
- an optional CI-populated R2 remote cache.

## Findings

### F1. It works, with patches on both sides

With the patches below, on macOS:

- `corgi check --workspace` passes.
- `corgi build -p quarto` passes, and that `q2` renders.
- `corgi test --workspace` builds all 1,518 units and runs 15,729
  tests. 25 fail (F6).

### F2. corgi needed 6 changes (5 commits, ~150 lines)

They are saved as `git format-patch` output in
`2026-11-03-corgi-build-caching/corgi-patches/` and apply to upstream
`49254b6`.

| Patch | Change | Kind |
|---|---|---|
| 0001 | Accept `target.<t>.runner` in `.cargo/config.toml` (we set it for wasm32) | missing feature |
| 0001 | Support `[lints.rust.unexpected_cfgs] check-cfg = [...]` | missing feature |
| 0002 | `?`-prefixed optional extra-inputs, for npm `dist/` trees that build scripts embed only when present | design question |
| 0003 | Pass transitive build-script `rustc-link-search` to test harness links (only bins got them; 24 harnesses failed `-llua5.4`) | **bug** |
| 0004 | Set `CARGO_MANIFEST_DIR` when running tests, as cargo and nextest do | **bug / parity** |
| 0005 | Drop git-ignored files from extra-input globs (tests writing outputs into `tests/` invalidated their own cache) | design question |

Also seen, but not patched:

- `default-members` is ignored; `--workspace` is required at our
  virtual root.
- `CARGO_TARGET_TMPDIR` is `/tmp/corgi/target-tmp` (see F6).
- `corgi test --force` (alias `--no-cache`) exists but is hidden from
  `--help`.

### F3. q2 needed hygiene fixes, all cargo-neutral

Most are worth landing even without corgi. They make our artifacts
location-free and reproducible.

1. **Path-locating `lua-src` fork.** It used `env!("CARGO_MANIFEST_DIR")`
   to find its C sources, which bakes the path into an rlib. It now
   embeds them with `include_dir!`.
2. **rust-embed in quarto-citeproc.** Without `debug-embed`, debug
   builds bake the absolute `locales/` path into the binary.
   `deterministic-timestamps` is also needed.
3. **`quarto-trace/build.rs`** shells out to git, which the sandbox
   can't run. It now accepts a precomputed `QUARTO_GIT_HASH`. corgi
   computes the value for release builds only, so dev builds no longer
   rebuild quarto-trace and everything above it on every commit.
4. **Tests baked in their own path** via `env!("CARGO_MANIFEST_DIR")`:
   98 sites, plus 11 `concat!(env!(..), "/x")` consts. They now read it
   at runtime or use package-root-relative literals. The branch uses a
   crude `&*std::env::var(..).unwrap().leak()` rewrite; a real change
   should use a `manifest_dir()` helper.
5. **insta macros** bake `env!("CARGO_MANIFEST_DIR")` into every
   snapshot test, which made 13 harnesses non-shareable. Setting a
   compile-time `INSTA_WORKSPACE_ROOT="."` fixes it under corgi only,
   because corgi compiles with package-relative `file!()` and runs tests
   from the package root.

`cargo nextest run --workspace` stays green with all of these:
17,631 passed and 200 skipped.

### F4. Configuration cost: about 28 packages need `[extra-inputs]`

Every non-`.rs` file a crate reads at compile time must be declared.
That covers `include_str!`/`include_bytes!`/`include_dir!`, rust-embed
folders, proc-macro reads, build-script reads, and `[tools.*]` such as
`cmake` for `wasmtime-c-api-impl`. To keep test-pass caching sound,
runtime test data must be declared too.

On the branch, `scripts/corgi/scan-extra-inputs.py` generates
`corgi.toml` from `scripts/corgi/corgi.base.toml` plus a source scan.
Adoption would need it as an xtask, plus a staleness check in `verify`.

### F5. Cross-checkout sharing works

This is the feature that matters for a worktree-per-task workflow.

| Measurement | Wall |
|---|---|
| `cargo build -p quarto`, fresh target dir | **2 m 05 s**, `target/` 9.3 GB |
| `corgi build -p quarto`, fresh worktree, store warm from another worktree (29 of 1,246 units genuinely new) | **43 s**, `target/` 3.3 GB (exported binaries only) |
| `corgi build -p quarto`, nothing changed, either worktree | **~3 s** |
| `corgi check --workspace`, after a small fix | 9 s |
| `corgi test --workspace`, after the test-path migration | 2 m 35 s |
| `corgi test --workspace`, nothing changed, either worktree (only the failing harness re-runs) | **~31 s** |
| `cargo nextest run --workspace` (incremental build + run) | 4 m 33 s |

Keys converged across the two worktrees: no ping-pong rebuilds in
either direction. The store reached **59 GB** after a day of
iterating; GC (`corgi clean`) was not evaluated.

### F6. Remaining test failures: 25, one cause

The quarto-core `pandoc_request_books`/`_projects`/`_exports` tests
need a scratch dir outside `/tmp`, because the pandoc-request mount
validator reserves `/tmp`. They use `env!("CARGO_TARGET_TMPDIR")`,
which is `target/tmp` under cargo and `/tmp/corgi/target-tmp` under
corgi. The fix belongs in our tests and was not made.

### F7. Test-pass caching is only as sound as the declared inputs

A cached pass is keyed on the harness's compile inputs. Tests run
unsandboxed, so these are invisible unless declared:

- runtime-read fixtures from *other* packages;
- external tools (pandoc, node, R, Python, typst);
- the environment.

The scanner declares each package's own test-data dirs. Anything beyond
that needs `corgi test --force`. This is the main correctness risk of
relying on `corgi test`.

## Why not adopt yet

- **Platforms.** corgi supports macOS and NixOS only. Our CI is Ubuntu,
  so the CI-populated remote cache, the big win, is unavailable, and
  Windows is unsupported. corgi could only be an opt-in local accelerator
  beside cargo.
- **Maturity.** We needed 6 corgi changes, including 2 bugs in paths
  any test-heavy workspace hits.
- **Soundness.** Of cached test passes, per F7.
- **Maintenance.** Of `[extra-inputs]` and the hand-written base config,
  per F4.

## Re-evaluation protocol (round 2)

1. **Check upstream.** For each row in F2, is it fixed in corgi (read
   the changelog or `src/config.rs` / `src/build.rs`)? Does corgi
   support Ubuntu yet? Windows? Has `CARGO_TARGET_TMPDIR` or
   `default-members` handling changed?
2. **Set up.** `cargo install corgi-build` (or build upstream).
   Cherry-pick the q2-side commits from
   `origin/2026-11-corgi-build-caching-research` onto current `main`,
   and regenerate `corgi.toml`:
   `python3 -I scripts/corgi/scan-extra-inputs.py .`
3. **Re-apply only the patches still needed.** Use
   `git am claude-notes/research/2026-11-03-corgi-build-caching/corgi-patches/*.patch`
   in a corgi checkout. Build it **outside** the q2 tree, because the
   q2 workspace glob claims `external-sources/corgi`. Note which patches
   were needed.
4. **Run, recording the same rows as F5.** Run `corgi clean` first, or
   use a fresh `CORGI_STORE`, for cold numbers:
   - `corgi check --workspace` (cold, then after touching one leaf crate);
   - `corgi build -p quarto` (cold; then in a second, fresh worktree;
     then with nothing changed);
   - `corgi test --workspace` (first run, then with nothing changed, in
     both worktrees);
   - the cargo baseline:
     `CARGO_TARGET_DIR=<fresh> cargo build -p quarto` and
     `cargo nextest run --workspace`;
   - store size (`du -sh /Users/Shared/corgi`), and whether `corgi clean`
     brings it back down.
5. **Count failures by cause.** Path leaks, sandbox reads, link errors,
   test failures. If upstream has Linux support: try `corgi cache build`
   / `cache test` in a CI job with an R2 bucket, and measure a fresh
   worktree on a cold machine.
6. **Compare.** Re-check the "Why not adopt yet" bullets against the
   new results.
