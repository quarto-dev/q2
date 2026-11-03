# corgi as a Rust build driver: exploration

**Strand:** bd-corgi-build-8fb3mzki
**Branch:** `braid/bd-corgi-build-8fb3mzki-explore-corgi-corgibuild-alternative`
**corgi:** 0.1.99 (`external-sources/corgi`), plus six small patches on a
local `q2-compat` branch in that checkout (listed below).
**Status:** exploratory, macOS only. On this branch:

- `corgi check --workspace` passes.
- `corgi build -p quarto` passes, and the resulting `q2` renders.
- `corgi test --workspace` builds every test harness and runs
  15,729 tests. The only failures are 25 tests in one quarto-core
  harness (`pandoc_request_books`); the cause is known and is a test-side
  assumption (see "Tests").

## TL;DR

corgi can build this workspace. Its rules are strict by design: no
location-dependent artifacts, every non-`.rs` input declared, sandboxed
build scripts. Those rules turned up real hygiene issues in our tree, most
of which are cheap to fix and harmless under cargo. corgi is young,
though: we needed six local patches, two of them for corgi bugs we hit.
Linux support is NixOS-only and there is no Windows support. So for us it
can only be an opt-in local accelerator on macOS for now, next to cargo,
which stays the source of truth and the CI driver.

Recommendation, open for revisiting: **don't adopt yet.**

1. Land the cargo-neutral hygiene changes on their own merits (listed
   under "q2-side changes").
2. File the upstream issues.
3. Re-evaluate when corgi supports Debian/Ubuntu Linux. That is what
   would unlock the CI-populated remote cache, the biggest win for a
   worktree-heavy workflow.

## What corgi is

[corgi](https://github.com/ConradIrwin/corgi) is a cargo-compatible
build driver. Cargo still resolves dependencies (`Cargo.lock`), but corgi
plans and runs the rustc/build-script actions itself:

- **One content-addressed store per machine** (`/Users/Shared/corgi`),
  shared by every checkout and worktree. Worktrees are what we use most
  (`.worktrees/*`, `create-worktree`), and each one currently pays for a
  cold `target/` of 12+ GB.
- **workspace-hack-style feature unification.** Features are unified
  across the whole workspace by default, so `-p foo` does not rebuild
  dependencies with different features. `[roots.*]` defines separate
  feature universes.
- **Sandboxed rustc, build.rs and proc macros** (macOS `sandbox-exec`).
  They get no network, a whitelisted env, and read access only to the
  package's own `.rs` files plus declared `[extra-inputs]`. corgi rejects
  any artifact whose bytes contain the workspace path, which is what
  makes cross-checkout sharing safe.
- **Hermetic toolchain.** corgi downloads its own pinned Zig-based C
  toolchain, macOS SDK and rust toolchain (it reads our
  `rust-toolchain.toml`) and any declared `[tools.*]`.
- **Tests run one process per test and in parallel, like nextest, and
  passing runs are cached.**
- Optional remote cache (Cloudflare R2) populated from CI.

## What it took

### corgi-side patches (`external-sources/corgi`, branch `q2-compat`)

| # | Gap | Our trigger | Patch | Kind |
|---|-----|-------------|-------|------|
| 1 | `.cargo/config.toml` `target.<t>.runner` is a hard error | `target.wasm32-unknown-unknown.runner = "wasm-bindgen-test-runner"` | Ignore `runner`. It only changes how a binary is launched. | missing feature |
| 2 | `[lints]` `check-cfg` is a hard error ("not supported yet") | workspace `unexpected_cfgs` check-cfg `cfg(coverage_nightly)`, and the same in `wasm-bindgen-futures-patch` | Emit `--check-cfg <v>` for each entry, as cargo does | missing feature |
| 3 | An `[extra-inputs]` pattern that matches nothing fails the build | Five build scripts embed an npm-built tree (`q2-preview-spa/dist`, `hub-client/dist-preview-embed`, `ts-packages/quarto-hub-mcp/dist-bundle`, `trace-viewer/dist`, `agents-docs-dist`) when it exists and a placeholder otherwise | A leading `?` marks a pattern optional: matching nothing is a valid, hashed state | design question |
| 4 | Test harnesses are linked without the transitive build-script `rustc-link-search` paths (only `Kind::Bin` got them) | 24 test binaries reaching mlua: `library not found for -llua5.4` | Treat `Kind::Test` like `Kind::Bin` | **bug** |
| 5 | `CARGO_MANIFEST_DIR` is not set when running tests (cargo test and nextest set it) | The portable replacement for compile-time `env!("CARGO_MANIFEST_DIR")` in tests is a runtime `std::env::var` | Set it to the package root (runtime only, not part of any key) | **bug / parity** |
| 6 | Broad extra-inputs sweep in git-ignored outputs that tests write | `tests/**` on `quarto` includes the rendered `smoke-all` outputs, so every run invalidates the next and nothing caches | Filter expansions through `git check-ignore` | design question |

All six patches are small; together they are about 150 lines. They are
good upstream issues/PRs. #4 and #5 are clear bugs; #3 and #6 are design
questions for corgi.

`external-sources/` is gitignored, so the patches are also saved as
`git format-patch` output in
`claude-notes/research/2026-11-03-corgi-build-caching/corgi-patches/`
(against upstream `49254b6`). The findings summary and the
re-evaluation protocol are in
`claude-notes/research/2026-11-03-corgi-build-caching.md`. Build the
patched binary outside the q2 tree, because the q2 workspace glob
otherwise claims `external-sources/corgi`:

```bash
git clone https://github.com/ConradIrwin/corgi /tmp/corgi-q2
git -C /tmp/corgi-q2 checkout 49254b6
git -C /tmp/corgi-q2 am "$PWD"/claude-notes/research/2026-11-03-corgi-build-caching/corgi-patches/*.patch
cargo build --release --manifest-path /tmp/corgi-q2/Cargo.toml
/tmp/corgi-q2/target/release/corgi check --workspace
```

### q2-side changes (on this branch)

**corgi config (inert under cargo):**

1. `corgi.toml`, generated by `scripts/corgi/scan-extra-inputs.py` from
   the hand-written `scripts/corgi/corgi.base.toml`. The scanner walks
   every workspace member from `cargo metadata`, which picks up nested
   members such as `quarto-math/vendor/mitex-*`. It finds:
   - literal paths in `include_str!` / `include_bytes!` /
     `include_dir!` / `concat!(env!("CARGO_MANIFEST_DIR"), …)`;
   - rust-embed `#[folder]`;
   - our own `include_error_table!` proc macro;
   - each package's own test-data dirs (`tests/`, `snapshots/`,
     `test-data/`, …).

   It turns each into a package-relative glob. The base file holds what
   a scan can't see:
   - `[tools.cmake]`: pinned 3.31.6 macOS universal tarball for
     `wasmtime-c-api-impl`, which tree-sitter's `wasm` feature pulls in
     for quarto-highlight's native user-grammar loader.
   - Build-script reads: `resources/scss`, highlight styles, the CSL
     suite, and tree-sitter C sources.
   - The optional (`?`) npm-built trees.
   - `[env.QUARTO_GIT_HASH]`, release builds only.
   - `[env.INSTA_WORKSPACE_ROOT] = "."` for every insta user (see 6 below).

   About 28 packages need declarations, which is why a generator exists.
   A real adoption would make it an xtask and add a verify check that
   fails when `corgi.toml` is stale.

**Hygiene changes, cargo-neutral, worth landing regardless:**

2. **`crates/lua-src-wasm`** (our `[patch]` fork of `lua-src`) embeds
   its Lua 5.4 sources with `include_dir!` and extracts them to
   `OUT_DIR`. Before, it located them through
   `env!("CARGO_MANIFEST_DIR")`, which bakes an absolute path into an
   rlib.
3. **`quarto-citeproc`**: rust-embed gains `debug-embed` and
   `deterministic-timestamps`. Without `debug-embed`, debug builds bake
   the absolute `locales/` path into the binary and read the files at
   runtime. corgi's README calls out rust-embed for exactly this.
4. **`quarto-trace/build.rs`** uses a precomputed `QUARTO_GIT_HASH`
   when one is set, because git can't run in the sandbox. Under corgi,
   dev builds report `unknown`, so a new commit no longer rebuilds
   quarto-trace and everything above it.
5. **Tests stop baking in their own path.** 98 sites go from
   `env!("CARGO_MANIFEST_DIR")` to
   `&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak()`, and 11
   `concat!(env!(..), "/x")` consts become package-root-relative
   literals. Both work under cargo test, nextest and (patched) corgi,
   since all three set the variable at runtime and run tests from the
   package root. The `.leak()` spelling is a mechanical-migration
   artifact. A real change should add a small `manifest_dir()` test
   helper instead.
6. **insta**: its `assert_*_snapshot!` macros expand
   `env!("CARGO_MANIFEST_DIR")` into every test that uses them, which
   alone made 13 harnesses non-shareable. insta reads a compile-time
   `INSTA_WORKSPACE_ROOT` instead when it is set. corgi compiles from
   the package root with package-relative `file!()`, so `"."` resolves
   snapshots correctly at runtime. This only applies to corgi builds,
   via `corgi.toml`.

### Things that just worked

- The pinned nightly from `rust-toolchain.toml` and the
  `[patch.crates-io]` entries.
- C builds via `cc` (tree-sitter grammars, Lua, mimalloc, jemalloc, …)
  through corgi's Zig toolchain.
- CMake-driven `wasmtime-c-api-impl`, once cmake was declared.
- Build scripts that `watch_dir` a missing tree. `quarto-build-support`
  tries to `create_dir` it, the sandbox denies the write, and the helper
  falls back to watching `build.rs`. corgi ignores `rerun-if-changed`
  anyway: inputs are hashed.

## Tests

`corgi test --workspace` builds every harness (1,518 units) and runs
15,729 tests. Of those, 25 fail, all in quarto-core's
`integration::pandoc_request_books`, `…_projects` and `…_exports`.

**Cause.** These tests need a scratch dir *outside* `/tmp`, because the
pandoc-request mount validator reserves `/tmp`, so they use
`env!("CARGO_TARGET_TMPDIR")`. Cargo sets that to `target/tmp`. corgi
sets a location-free `/tmp/corgi/target-tmp` instead. The fix belongs
in our tests: pick a scratch root that isn't under `/tmp` at runtime.
That fix is not done on this branch.

**Test-pass caching works, with caveats.** Running again with no changes
re-executes only the harness that still fails (31 s); every other harness
is a cache hit. Caveats:

- A cached pass is keyed on the harness's *compile* inputs, i.e. its
  sources plus extra-inputs. Tests run unsandboxed, so files they read
  at runtime are hashed only when declared. The scanner declares each
  package's own test-data dirs. It does **not** cover:
  - tests that read files from other packages (e.g. `quarto` reading
    `quarto-core/tests/fixtures`);
  - external tools (pandoc, node, R, Python, typst);
  - environment variables.

  A cached pass can therefore be stale. `corgi test --force` (alias
  `--no-cache`, hidden from `--help`) re-runs everything.
- Tests that write into their own input dirs defeated caching until
  patch #6.

## Numbers (M-series Mac, this worktree)

| Step | Wall |
|------|------|
| `corgi check --workspace`, cold store (1302 units) | ~32 s to the first error (most units done) |
| `corgi check --workspace` after fixes | 9 s (17 executed, 1252 cached) |
| `corgi build -p quarto`, first (cold codegen) | 1 m 45 s |
| `corgi build -p quarto`, after a fix low in the graph | 35 s |
| `corgi test --workspace`, after the test-path migration | 2 m 35 s |
| `corgi test --workspace`, no changes (only the failing harness re-runs) | 32 s |

For comparison, under cargo on the same branch:

| Step | Wall | `target/` |
|------|------|-----------|
| `cargo build -p quarto`, fresh target dir | 2 m 05 s | 9.3 GB |
| `cargo nextest run --workspace`, incremental: 17,631 passed, 200 skipped | 4 m 33 s | (this worktree: 30 GB, shared with corgi exports) |

The test-path migration is therefore green under cargo/nextest too.
corgi counts fewer tests (15,729) because it doesn't descend into the
failing harness's skipped dependents and counts `#[ignore]` differently;
I didn't reconcile the two counts exactly.

## Cross-worktree reuse

Method: a throwaway detached worktree (`.worktrees/corgi-probe`) at the
same commit, with no `target/`, built against the same store.

- `corgi build -p quarto`: **43 s**. 1,217 of 1,246 units came from the
  store. The 29 misses were genuinely new keys: the original worktree
  had last built `-p quarto` before the test-path commit, and its last
  `corgi test` predated a `cargo fmt`. Built again afterwards, both
  worktrees are fully cached (**~3 s**, 0 executed).
- `corgi test --workspace`, run in both worktrees afterwards: identical,
  0 compiled, and only the one failing harness re-run (**~31 s each**).
  Test passes are shared across checkouts.
- Disk: the fresh worktree's `target/` holds only exported binaries
  (3.3 GB) versus 9.3 GB for a cold cargo `build -p quarto`. The shared
  store at `/Users/Shared/corgi` grew to **59 GB** over this session,
  including toolchains and every intermediate iteration. `corgi clean`
  exists, but its GC policy wasn't evaluated.

The cross-checkout model works as advertised once artifacts are
location-free. For a worktree-per-task workflow this is the headline
win: a new worktree's first build costs seconds instead of minutes.

## Candidate upstream issues for corgi

1. Accept `target.<triple>.runner` in `.cargo/config.toml` (patch 1).
   Ideally `corgi test --target wasm32-…` would use it.
2. Support `[lints.rust.unexpected_cfgs] check-cfg = [...]` (patch 2).
3. Optional extra-inputs for build scripts that embed a generated tree
   only when it exists (patch 3).
4. **Bug:** test harnesses miss transitive native link-search paths
   (patch 4).
5. **Parity:** set `CARGO_MANIFEST_DIR` (and the other runtime
   `CARGO_*` vars) when running tests, as cargo and nextest do
   (patch 5).
6. Ignore git-ignored files in extra-input globs, or support exclude
   patterns (patch 6).
7. `default-members` is ignored: bare `corgi check` at a virtual
   workspace root fails with "no root package". `--workspace` is
   required.
8. Document or reconsider `CARGO_TARGET_TMPDIR=/tmp/corgi/target-tmp`.
   Code that treats `/tmp` specially behaves differently under corgi.
9. Expose `--force` in `corgi test --help`, and consider declaring
   runtime test inputs separately from compile inputs.

## Out of scope / not attempted

- **WASM** (`crates/wasm-quarto-hub-client`, `wasm-qmd-parser`): these
  are separate workspaces built with wasm-pack and their own
  `.cargo/config.toml`. corgi has `--target`, but wasm-pack and
  wasm-bindgen post-processing are outside its model. `cargo xtask`
  keeps owning these.
- **xtask / verify integration.** `cargo xtask verify` still drives
  cargo + nextest. Nothing here changes CI.
- **Linux CI / remote cache.** Linux support upstream is NixOS-only and
  our CI runs on Ubuntu, so an R2-backed shared cache isn't available to
  us yet.
- **Windows.** Unsupported upstream.
- **clippy** under corgi (`corgi clippy`) was not tried.

## Follow-ups if we continue

- Land hygiene items 2–5 as a normal PR, with a `manifest_dir()` helper
  instead of `.leak()`. They make our artifacts reproducible regardless
  of corgi.
- Fix the `/tmp` scratch assumption in the quarto-core pandoc-request
  tests.
- File upstream issues 1–9, with patches for 1, 2, 4 and 5.
- If corgi gains Ubuntu support: try `corgi cache build` / `cache test`
  in CI with an R2 bucket and measure fresh-worktree build times.
