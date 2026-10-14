# Review: plan "move TS engine extensions from Deno to Node"

**Date:** 2026-10-13
**Plan:** `claude-notes/plans/2026-10-13-ts-engines-deno-to-node.md`
**Method:** two Sonnet reviewers (A: q2 side and sequencing; B: extensions and dual-runtime). I re-checked every finding that changes phase order, scope or a locked decision. Those are marked **\[verified\]**. Nothing in the plan, source or any repo was edited; B’s experiments ran in scratch dirs.

Locked decisions (dual runtime, hard cut, Node 24, rename, esbuild, scope exclusions) were not re-litigated. Nothing below contradicts them. The "temporary flag" escape A mentions is rejected in favour of re-ordering.

## Blocking (plan cannot work as written)

### B1. Phase order is not green-able; P2/P3/P6 are circular, and P4/P5 must come before the flip **\[verified\]**
- Only 10 hermetic fixtures exist (`HERMETIC_FIXTURES`, `engine_fixture_build.rs:26`). Their `dist/` is gitignored (`.gitignore:100`) and **rebuilt at test time** via `build_ts_extension`, which shells out to `deno bundle` (`extension/build.rs:345`). Committed bundles exist only for legacy-python, marimo and julia. So P3’s "rebuild committed bundles (P6 builder or deno bundle)" misdescribes the situation, and as long as P6 is undone every hermetic e2e still needs Deno to build, even after P2 moves the host to Node.
- P2’s hard cut turns marimo, julia and the preview-spa e2e specs red (`engine-capture-splice*.spec.ts` skip on `!onPath('deno')`), and none are in P2’s gate list. P3’s gate "all e2e green" cannot be met at P3.
- Key enabler: bundles that use `node:` built-ins run under the current Deno host (B ran `node:fs`, `node:child_process`, `node:net`, `process.kill` under deno 2.9.0 and Node 24.5: same output). Dual-runtime therefore lets ports land *before* the flip.

**Suggested order (every commit green):**
1. P0 marimo reconcile (see B2).
2. P1a: add `node-host.ts`, `node:net` control transport and Node `main.ts` *alongside* the Deno files, with vitest tests. No rename yet.
3. P6: esbuild builder (see S1 for distribution), keep the `deno.json` tiers working until fixtures are ported.
4. P3a: port the 12 fixtures to `node:fs`/`process`.
5. P4, P5: port marimo and julia upstream (repo-local esbuild script is the *only* option, so no dependency on P6), re-sync fixture/subtree.
6. **P2 flip, atomic:** Rust spawn to Node, `DIALBACK_PREAMBLE` rewrite, shared `node_available()`, workflow changes (Node on, Deno off), rename to `engine-host-node`, delete `deno-host.ts`, `*.deno-test.ts`, `deno.jsonc`, `deno.lock`. Gate: the full e2e list including marimo, julia, `capture_driver`, preview-spa specs.
7. P7 docs/cleanup (but see S9: migration note ships with the flip).

Edit the plan: replace the phase order, delete the "land together or temporary compat" hedge (L63) and the "P6 builder or deno bundle until then" hedge (L67).

### B2. P0 is built on wrong premises **\[verified\]**
- `origin` in `~/src/quarto-marimo` is Gordon’s fork; marimo-team is `upstream`. Last fetch was Jul 24, so "latest main" is unknown until `git fetch upstream`. The plan’s "origin/main is well ahead of quarto-2-improvements" is not true of the fetched refs: `quarto-2-improvements` is exactly `upstream/main` (78987bc) + 2 commits (f47802b `claimsFile` dropped, b60cee8 static claims in `_extension.yml`).
- The q2 fixture is a **different lineage**: its `src/`, `lib/`, `extract.py` are identical to `q2-bare-sql-interop` (2a2f312, based on 76d6f1d, 5 commits behind upstream/main). The fixture still has the `containsMarimoFence` content sniff in `claimsFile`; it does **not** have `claimsFile -> false`, and `claims-files: []` is patched in by `marimo_engine_e2e.rs` at test time.
- So P0’s single bullet ("rebase q2-side changes: static claims, claimsFile->false, buildCommand, containsMarimoFence, cellOwnedByMarimo") merges two parallel unmerged branches: `quarto-2-improvements` (claims) and `q2-bare-sql-interop` (LanguageClaim interop, `cellOwnedByMarimo`, `buildCommand`, `extract.py` gate, new tests). Likely conflicts: `src/marimo-engine.ts`, `tests/claims-language.test.ts` (both rewrite it), `_extension.yml`.
- Semantic contradiction: `quarto-2-improvements`’ `_extension.yml` declares `sql: {kind: interop}` statically while its engine’s `claimsLanguage` can’t return interop; the bare-sql branch supplies the runtime half.
- The julia q2 subtree matches the unmerged `q2-static-declarations` branch, not julia `main`; the plan’s "byte-identical to upstream" (L30) holds only against that branch.
- P0’s gate omits `tests/build-command.test.ts`, the new claims tests and `pytest tests/python`.

Edit P0: first `git fetch upstream`, re-measure the gap, then reconcile the two branches; list the decisions in Q1 below; extend the gate.

### B3. marimo’s committed extension file is a Deno-only loader shim, missing from P4 **\[verified\]**
Upstream `_extensions/marimo/marimo-engine.js` is a ~1 KB shim using `Deno.stat`/`Deno.writeTextFile` that downloads `marimo-engine-v${VERSION}.js` (VERSION 0.4.5) from GitHub releases and imports it. A q2 user installing the real extension gets "Deno is not defined" before any engine code runs, and the released v0.4.5 bundle is Deno-only. The q2 fixture works only because it commits a real bundle in place of the shim. Also: marimo commits **no** bundle and has **no** freshness check (CI runs `make build`, which restores the shim); only julia has the committed-bundle freshness gate. P4 needs: port the shim (`node:fs/promises`), and a VERSION/release decision (Q2 below).

### B4. Julia env semantics: naive `Deno.Command` to `node:child_process` swap breaks under Node **\[verified by reviewer experiment\]**
`Deno.Command({env})` merges into the parent environment; plain Node `spawn`/`spawnSync` *replaces* it (`env: {JULIA_LOAD_PATH}` gives no PATH/HOME). julia-engine.ts L376, 428, 441-490 rely on the merge. A naive port works under Deno/Q1 and breaks under Node/q2. Required: `env: {...process.env, JULIA_LOAD_PATH: ...}`, with a test asserting PATH is in the child env. Also: on ENOENT check `.error.code`, not `.status` (null on Node, undefined on Deno).

### B5. Node stdout is truncated on `process.exit` (plan says "unverified") **\[verified\]**
`node -e 'process.stdout.write("y".repeat(5e6)); process.exit(0)' | wc -c` printed 65536 here (reviewer saw 131072; deno prints all 5,000,000). Promote from "unverified" to a required P1 test: write several MB to a piped stdout, exit, assert the Rust reader got it all. Fix: `process.exitCode` plus drain, or `write(buf, cb)` before exit. (A also found the stdio/stdout branch of `main.ts` is dead in production, see S4, which may make this moot for the host itself but not for engines that print.)

### B6. Node 24.0/24.1 hole **\[verified\]**
`MIN_NODE_MAJOR = 24` compares majors only (`node.rs:25,83`), but `import.meta.main` needs >= 24.2 (plan L16; `main.ts:28`). On 24.0/24.1 the guard is falsy, the host exits 0 without dialing back, and Rust reports a confusing "exited before dialing back". Either drop the guard (the bundle is always the entrypoint in control mode) or check minor >= 2. `.nvmrc`=24 and `engines.node ^24.0.0` have the same hole.

## Should-fix

- **S1. esbuild distribution for `q2 call build-ts-extension`.** `run_deno_bundle` shells to `deno`; an installed `q2` has Node but no esbuild. P6 must choose: embed an esbuild-driver bundle run with Node (still needs the native binary or wasm), `npx esbuild` (needs network), or embedded `esbuild-wasm`. This gates all of P6 including the shipped tier. Recommendation: Q3 below.
- **S2. P6 tier mapping is under-specified.** esbuild has no import maps. Define the config (tsconfig `paths` or JSON alias map), a translator for ext-local `deno.json` `imports`, whether `deno.json` is still read, and the `jsr:` policy (shipped template maps `path`, `fs/`, `encoding/`, `log` to jsr; esbuild cannot resolve `jsr:`: fail with an actionable error). `import type` of `@quarto/*` is erased, so tier 4 may need no aliases; verify and say so. State that "no type-check" is preserved. Preserve `build.rs` behaviors: resolution precedence (build.rs:72-100), lazy shipped-tier materialization, `find_workspace_root`, ~17 unit tests, error strings ("Failed to spawn `deno`", "`deno bundle` exited"); list deleted tests with reasons.
- **S3. Node discovery crate.** `quarto-core` -> `quarto-mcp-launcher` has no cycle but is heavy (`include_dir!` of the whole MCP bundle re-embedded by `build.rs`, `fs2`, `sha2`, `dirs`; wasm32 gating needed; MCP-specific error text). Lift `node.rs` (`find_node`, `Discovery`, `probe_version`, `MIN_NODE_MAJOR`) into a small `quarto-node-discovery` crate, re-exported by the launcher, with a purpose string for error wording, under `cfg(not(wasm32))`. Document that `QUARTO_NODE` now governs engines too.
- **S4. P1 host design.** (a) Use `tty.isatty(0)`, never touch `process.stdin` in control mode (it sets O_NONBLOCK, so the later `fs.readSync(0)` can throw EAGAIN; `deno-host.ts:264` precomputes `isInteractive` at load). Windows `readSync` on fd 0 throws EOF where POSIX returns 0. (b) The stdio branch of `main.ts` is dead (Rust stdio transport removed at the Phase-4 cutover); drop `Readable.toWeb(process.stdin)` and the stdout FrameWriter items or delete the branch. (c) Socket: attach an `'error'` handler that rejects pending writes, await the write callback, `end()` before exit. (d) `MuxAsyncIterator` is `jsr:@std/async`; write a small merge. (e) Update tsconfig/vitest excludes, bump `@types/node` to 24, add a test spawning the real bundle with piped stdin.
- **S5. CI.** `test-suite.yml:155-160`: Rust job has "no setup-node", and the free-disk step uses `remove_tool_cache: true` (Deno was installed via curl/brew to dodge that). Specify Node 24 provisioning placed after free-disk (or curl/brew), macOS install, whether `QUARTO_NODE` is needed, and that `deno_available_when_quarto_ci` (`ts_process.rs:~4367`) becomes `node_available_when_quarto_ci`.
- **S6. Rename/Deno scope is wider than listed.** Add: `.gitignore:54-61` and the package’s own `.gitignore`; `ci_test_wiring.rs` (TEST_MARKERS "deno test", test at ~L324); xtask `build_all.rs`, `build_engine_host_bundle.rs`, `main.rs` (~410-457), `dev_setup.rs` (151, 339-362); `ts_process.rs` bundle name `engine-host-deno-<sha8>.js` (L87, test L4291); `ts_protocol.rs:1739`, `project/mod.rs:1400`; `q2-preview-spa/e2e/engine-capture-splice*.spec.ts` and README; `quarto-preview/src/capture_driver.rs` (own `deno_available()`, `deno.workspace.json` path ~L543); `quarto/tests/integration/build_ts_extension_e2e.rs`, `engine_diagnostics_cli.rs`; both workflows; `docs/errors/extension/Q-16-12.qmd:53`, `docs/guides/projects/scripts.qmd:126`. Installer/release/Docker/`scripts/`: no Deno references found, so no packaging change, only "needs Node 24" in user docs.
- **S7. Test-count hygiene is absent.** Most Deno-gated tests `eprintln!("SKIP")` and return, which nextest counts as PASS, so a lost gate is invisible in counts. Add: live baseline before P1 (`cargo nextest list -p quarto-core` etc., vitest count), per-phase delta accounting (tests move between runners), hard-fail under `QUARTO_CI=1` via one shared `node_available()`, and the P6 test list from S2.
- **S8. Julia detached start is mis-described (plan L85).** Unix does *not* detach in TS: it runs `start_quartonotebookrunner_detached.jl` synchronously (`outputSync`), and Julia does `run(detach(cmd), wait=false)`; Windows runs PowerShell `Start-Process`. Keep both paths with `spawnSync`. `spawnSync` blocks until every holder of the stdout pipe closes (observed 2045 ms with a grandchild holding it vs ~50 ms), so the julia script must redirect server stdio. Windows path is not provably safe (quoting, handle inheritance); make a manual Windows checklist item with a pass criterion (call returns, transport file appears). `.cmd` shims may need `shell: true` (EINVAL on Node 20.12+).
- **S9. Julia framing tests.** Current code has a latent bug: on EOF `response=""` then `JSON.parse("")` throws an opaque SyntaxError. Rewrite as a Buffer-accumulating line framer and test: message split across chunks including mid-UTF-8-character; multiple messages in one chunk; chunk ending exactly on `\n`; >512-byte and very large messages; close/error mid-message and before any reply (clear error); HMAC `{hmac,payload}\n` against a fixed vector (`node:crypto` equivalent of `crypto.subtle`+base64); `isready` 10 s timeout destroys the socket; ECONNREFUSED/ECONNRESET surfaced; error/`juliaError` responses; progress-update loop. Pass `host: "127.0.0.1"` explicitly to `net.connect`.
- **S10. Raw status output.** Plan L83 maps `Deno.stdout.writeSync` to `quarto.console.*`, but that routes via `host.log.info` (formatted log, not raw stdout). Use `process.stdout.write` for raw output. (Q1’s `console.info` destination was not checked.)
- **S11. Test port runner: use `node:test` + `node:assert/strict`, not vitest, for marimo/julia extensions.** marimo’s 4 files do use `Deno.test`, `jsr:@std/assert` and `Deno.makeTempDirSync` (plan L76 "no Deno.* in them" is wrong), but the conversion is mechanical, and the same files run under `node --test` and `deno test --no-check -A` (B ran both). The `@quarto/types` mock is a non-issue (all `import type`, erased). marimo’s CI `render` job is already a Q1 leg; julia’s CI already runs Q1 smoke tests with Q1’s bundled Deno, so neither needs a standalone Deno install. Julia’s smoke tests are Q1-native; keep as the Q1 leg, don’t port.
- **S12. Freshness and build story for external repos.** marimo and julia have no `package.json`/lockfile today. Add: `package.json` + lockfile pinning esbuild exactly, `setup-node`, build from repo root (esbuild embeds relative path comments), eol/`.gitattributes` for the Windows CI leg (julia’s freshness check runs on Windows), remove `deno.json`/`deno.lock`/import maps. q2’s committed marimo fixture bundle (deno bundle output with `deno:https://jsr.io/...` banners) has no freshness gate; add one. Q1’s `quarto call build-ts-extension` stays for third parties; sync Q1 `AGENTS.md` build docs as a follow-up.
- **S13. Third-party engines.** A migration note in P7 is not enough: a `Deno.*`-calling engine fails with `ReferenceError: Deno is not defined` at load or first call. Ship with the flip: a host-side catch that rewrites that error into an actionable message (extension path + migration doc), the `node:` equivalents table and dual-runtime recipe (including the env-merge caveat), and Q1 `quarto-required` guidance. Do not detect via static grep of bundles (`deno bundle` output has guarded `globalThis.Deno?.build.os`). A read-only `globalThis.Deno` shim in the host is an option but trades against the hard-cut decision. Note also that P6 removes the `deno.json` import map, so authors using bare `path`/`fs/` specifiers need documented replacements.

## Nice-to-have

- Hidden `Deno.*` in committed bundles: `fs/exists` and `jsr:@std/io` get inlined into today’s bundles (e.g. `Deno.statSync`), so P5’s "21 sites" undercounts; after the port, grep the *built* bundle.
- Import `process` from `node:process` and `Buffer` from `node:buffer` explicitly rather than relying on globals.
- Re-run the dual-runtime check against Deno 2.7.14 (Q1’s pin); reviewers had 2.9.0.
- `esbuild` output was deterministic across two builds and `import.meta.url` still resolves next to the bundle (so `command.py`/`*.jl` lookups keep working, bundle must stay in the extension dir). Add a macOS/Linux byte-stability check to the freshness gate.
- Replace `Deno.makeTempFileSync({suffix})` (marimo) with `mkdtempSync` + fixed filename.

## Questions for Gordon

1. **P0 merge strategy.** Which remote is "upstream" (marimo-team, I assume), and should the two unmerged branches (`quarto-2-improvements`, `q2-bare-sql-interop`) be merged with each other first or both rebased onto fresh `upstream/main`? Does `containsMarimoFence` survive when `claimsFile` is false? Is `~/src/quarto-marimo-main` (an unrelated old line) relevant? *Recommendation:* `git fetch upstream` first, then rebase `quarto-2-improvements` onto `upstream/main`, rebase `q2-bare-sql-interop` on top, drop `containsMarimoFence` if nothing but its own tests uses it, ignore the old worktree.
2. **marimo release/VERSION.** The released v0.4.5 bundle is Deno-only. *Recommendation:* bump VERSION and cut a new release with the Node-safe shim+bundle as part of P4; q2’s fixture keeps committing the real bundle.
3. **esbuild in installed q2 (S1).** *Recommendation:* embed an esbuild-driver bundle run with Node, resolving esbuild’s native binary the way `quarto-mcp-launcher` resolves its Node bundle assets; avoid `npx`. If the native binary can’t be shipped, fall back to `esbuild-wasm`. Needs a short spike before P6.
4. **Phase order.** *Recommendation:* adopt B1’s order, with the flip as one atomic commit/PR.
5. **Shared Node-discovery crate vs depending on the launcher.** *Recommendation:* new tiny crate (S3).
6. **Host-side `Deno is not defined` diagnostic and optional shim (S13).** *Recommendation:* diagnostic yes, shim no (keeps the hard cut honest).

## What was NOT checked

- Whether `process.stdin` is touched by `quarto-api` or other modules at import time (only `deno-host.ts` read in full).
- `blueimp-md5` banner need under `platform: 'node'`; esbuild byte-stability across macOS and Linux.
- Any Windows behaviour (PowerShell quoting, `.cmd` shims, readSync on fd 0, handle inheritance).
- A real Julia server, the HMAC handshake against `quartonotebookrunner.jl`, `start_quartonotebookrunner_detached.jl` internals past L17.
- Q1 under its real Deno 2.7.14, `quarto call build-ts-extension` end to end, Q1’s `console.info` destination, and whether Q1’s CI rebuilds julia subtree bundles (none found).
- What `upstream/main` of marimo-team holds today (no `git fetch` run; refs last fetched Jul 24), and the julia `main` vs `q2-static-declarations` divergence.
- GitHub runner Node versions; esbuild native-binary resolution from an installed q2.
- The 12 fixture engines’ source and q2 host internals beyond what is cited; the exact count of `deno_available` copies.
- No cargo clippy/nextest or workspace run was executed.
