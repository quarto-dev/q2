# Porting TS engine extensions from Deno to Node — preliminary research

**Date:** 2026-10-09
**Status:** preliminary; nothing implemented. Based on q2 `main` @ 15bb0d54f and the
local checkouts of `~/src/quarto-marimo` (b60cee8) and `~/src/quarto-julia-engine` (0a2b98f).

## Summary

Most of the machinery is already runtime-neutral. The q2-side port is **small and
mechanical** (one spawn site, one host adapter, one transport, one bundler, tests/CI).
The real cost is the **extensions**: both call `Deno.*` directly, and their bundles must
presumably keep running under Quarto 1 (Deno) too. The recommended path is to move the
extensions to `node:` built-ins, which run under both Deno 2 and Node, rather than
shimming a `Deno` global in the Node host.

Rough size: q2 ≈ 1–2 days of focused work + CI; marimo ≈ 0.5 day; julia ≈ 1 day
(more surface, plus Windows detached-process behaviour that needs real testing).
This is **a long plan, not an epic**: every step is mechanical and the test
coverage to gate each step already exists (§7). `@quarto/api` needs no code
changes (§1, "Already neutral"); the bundler is **esbuild**, already in use in q2
(§4.2).

## 1. q2 changes

### Already neutral (no change)
- `ts-packages/quarto-engine-host-deno/src/host.ts` (protocol loop), `framing.ts`,
  `mapped-source.ts`, `metadata-as-format.ts`, `quarto-api.ts`, and `@quarto/api`
  all state "no `Deno.*`". Their vitest suites already run under **Node**.
- `engine-loader.ts` already uses `node:url` + dynamic `import()`.
- The Rust protocol layer (`ts_protocol.rs`, the `EngineTransport` traits, the
  loopback-TCP dial-back with stdin token) is runtime-agnostic. Only the command line differs.

### Must change

| Area | File | Change |
|---|---|---|
| Entry point | `src/main.ts` | `Deno.args`, `Deno.stdin.readable`, `Deno.stdout`, `Deno.exit`, `import.meta.main`. Node: `process.argv`, `process.stdin` (adapt to a `ReadableStream`/`Readable.toWeb`), a `FrameWriter` over `process.stdout`, `process.exit`. `import.meta.main` is `import.meta.filename === process.argv[1]` (or just drop the guard — it is always the entry). |
| Control transport | `src/control-transport.ts` | `Deno.connect` → `node:net` `createConnection`; `Deno.stdin.read` byte-at-a-time token read → read fd 0 synchronously (`fs.readSync`) so no bytes past `\n` are consumed (the existing H-READER hazard applies unchanged). The socket writer must keep the short-write-safe semantics (`socket.write` + drain). |
| PlatformHost | `src/deno-host.ts` → `node-host.ts` | Straight mapping to `node:fs` (`readFileSync`, `writeFileSync`, `statSync`, `mkdirSync`, `mkdtempSync`, `rmSync`, `realpathSync`), `process.env`, `process.cwd()`, `process.stdin.isTTY`. `walkSync` from `jsr:@std/fs` → small hand-written recursive `readdirSync` (or `readdirSync({recursive:true})`). `MuxAsyncIterator` from `jsr:@std/async` → merge two `Readable`s (or use `child.stdout/stderr` events). `Deno.Command` → `node:child_process.spawn`; port `exec`’s timeout/mergeOutput/stderrFilter/respectStreams knobs. `onExit` (`unload` event) → `process.on("exit")`. |
| Mktemp semantics | same | `Deno.makeTempFileSync({prefix,suffix,dir})` has no Node twin; write `mkdtemp` + file, or open with `wx` and random name. Suffix support matters (marimo uses `.html`). |
| Bundler | `esbuild.config.mjs` | `platform: 'neutral'` → `platform: 'node'`, `format: 'esm'`, `target: node24`; `external: ['node:*']` only (drop `jsr:*`). Needs a `createRequire`/`import.meta` banner only if any CJS dep leaks in (`blueimp-md5` — check). Output name `engine-host-deno.js` → `engine-host-node.js` (touch `ts_process.rs` `include_str!`, `cargo xtask build-engine-host-bundle`, the CI freshness gate). Consider renaming the package `@quarto/engine-host-node`; this is optional churn — a rename touches ~40 references (plans, CI, `deno.jsonc`, workspace deps). |
| Spawn site | `crates/quarto-core/src/engine/ts_process.rs` (`is_available`, `ensure_started` ~L150, ~L897) | `Command::new("deno") run --allow-all <bundle> --control …` → `Command::new(node) <bundle> --control …`. Drop `--allow-all` (and the security-posture comment, which becomes "Node has no permission model by default; same accepted v1 trust model"). Error text "Deno is required…" → Node. |
| Node discovery | reuse `crates/quarto-mcp-launcher/src/node.rs` | Already has `QUARTO_NODE` override, PATH lookup, well-known/fnm/nvm/volta paths, version floor (24). Either depend on it or lift `find_node` into a shared crate (`quarto-util`?). **Decision needed:** the floor is 24 for `q2 mcp`; the engine host may only need Node ≥22 (native TS type stripping is not required for bundled `.js`). Keeping one floor is simpler. |
| One-shot mode | same file / `ts_engine.rs` L757–830 | `deno_is_available()` wrappers and the Plan 9 one-shot `deno` process → Node discovery. |
| Extension build | `crates/quarto-core/src/extension/build.rs` (`deno bundle`, tier-1..4 config resolution), `resources/extension-build/deno.json`, `deno.workspace.json`, `q2 build-ts-extension` in `crates/quarto/src/main.rs` | The largest q2-side design question — see §4. |
| Test helper | `ts_process.rs` `deno_dialback_child` (JS preamble using `Deno.connect/stdin/exit`) | Rewrite preamble with `node:net`/`process`. Mechanical. |
| e2e gates | 8 files under `crates/quarto-core/tests/integration/*_e2e.rs`, `engine_fixture_build.rs`, `crates/quarto/tests/integration/build_ts_extension_e2e.rs`, `engine_diagnostics_cli.rs`, `crates/quarto-preview/src/capture_driver.rs` | Each has its own `deno_available()` copy (`Command::new("deno")`). Consolidate into one shared `node_available()` test helper while at it. |
| Fixtures | `crates/quarto-core/tests/fixtures/extensions/*/src/*.ts` (alpha, beta, behave, echo-\*, marimo, interop-r, …) | Fixture engines that call `Deno.*` need porting; the ones that only use `quarto.*` need nothing. Grep each. |
| CI / dev setup | `.github/workflows/ts-test-suite.yml` (`DENO_VERSION`, `setup-deno`, 3 `deno test` steps), `crates/xtask/src/dev_setup.rs::check_deno`, root `deno.jsonc` | Remove Deno install; the three `*.deno-test.ts` files (`deno-host`, `control-transport`, `wire-parity`) become vitest tests — a real improvement, they currently need a separate toolchain. `wire-parity` already reads a JSON fixture and needs no Deno API beyond `Deno.test`. |
| Docs | `claude-notes/plans/2026-04-16-ts-engine-extensions-subprocess.md` and Plan 1b/9/10 text | Update `Deno must be on PATH`. |

### Not in scope (false positives from grep)
- `jupyter/text_execute.rs` maps `ts`/`js` → the **Jupyter kernel named "deno"**.
  Unrelated to the engine host.
- `quarto-system-runtime` deno_core removal (`2026-06-12-remove-deno-core-rusty-v8.md`) —
  separate, already underway.
- `.ts` render scripts (pre/post-render): see the `q2-port-analysis:deno-to-node-porting` skill.
  Distinct feature, same underlying theme.

### Behavioural differences to test
1. **TypeScript**: Deno executes `.ts` engine modules directly; `loadEngineModule`
   accepts "`.mjs`, `.ts`, `.js`". Node 24 strips types natively but rejects `enum`,
   parameter properties, and extensionless/`.js→.ts` imports. Decide whether q2 supports
   un-bundled `.ts` engines at all (the `_extension.yml` convention is a bundled `.js`).
2. **Detached child outliving the host** (julia server): Deno’s `Command` keeps the
   event loop alive; Node needs `spawn(..., {detached:true, stdio:'ignore'}).unref()`.
3. **stdout is the old protocol channel**; with TCP dial-back that footgun is gone, but
   Node’s `process.stdout` writes to a pipe asynchronously on POSIX, so engine
   `console.log` + `process.exit` can lose output. Use `process.exitCode` or flush before exit.
4. **Unhandled rejections** crash Node by default; Deno behaves similarly but not
   identically — check engine-loader error surfacing (`recent_stderr` plumbing).
5. **Windows**: argv quoting/`.cmd` shims for `julia`/`python` via `spawn` (Deno’s `Command`
   had its own resolution). Cross-platform rules in `.claude/rules/cross-platform.md` apply.

## 2. quarto-marimo

Files: `src/marimo-engine.ts` (345 lines), `lib/*.ts` (pure, no Deno), `deno.json`,
`Makefile`, `.github/workflows/ci.yml`, `tests/*.test.ts`, bundled
`_extensions/marimo/marimo-engine*.js`.

Deno surface is small (5 sites):
- `new Deno.Command(...).spawn()` + piped stdin/stdout/stderr (`executePython`) →
  `node:child_process.spawn` (or `quarto.system.execProcess`, which the host already
  provides and which would remove the need for Node/Deno APIs entirely — preferred).
- `Deno.readTextFileSync`, `Deno.makeTempFileSync`, `Deno.writeTextFileSync` →
  `node:fs` (`readFileSync`, `mkdtempSync` + `writeFileSync`).
- Imports: `path` (import-mapped to `deno.land/std@0.224.0/path`) → `node:path`
  (`fromFileUrl` → `node:url` `fileURLToPath`); `https://deno.land/std…/async/delay.ts`
  → `setTimeout` promise from `node:timers/promises`.
- Tests: `deno test --no-check` with an import map aliasing `@quarto/types` to a mock.
  Port to vitest/`node --test`; `tests/mocks/quarto-types.ts` can stay. 4 test files,
  pure logic, easy.
- Build: `make build` uses `quarto call build-ts-extension` (Deno). Replace by q2’s
  new Node build (§4) or a repo-local `esbuild` script. CI drops `setup-deno`.
- `_extension.yml` already has the static `claims`/`claims-files` for q2; no change.
- `quarto-required: ">=1.9.20"` implies Q1 compatibility is promised. If using `node:`
  built-ins, Q1/Deno 2 still runs it. **Verify** Q1’s bundled Deno version supports
  `node:child_process`/`node:fs` (Deno ≥1.40 does; Q1 1.9 ships Deno 2.x).

## 3. quarto-julia-engine

Files: `src/julia-engine.ts` (1142 lines), `src/constants.ts`, `tests/smoke/**`
(Deno tests using `jsr:` imports), `tests/run-tests.sh` (uses the Deno bundled with
Quarto), CI pinned to a quarto-cli rev.

Deno surface (~21 call sites):
- `Deno.build.os` → `process.platform === "win32"`.
- `Deno.Command` (4 sites incl. PowerShell `Start-Process` for the detached server,
  `outputSync`) → `spawnSync`/`spawn`. The Windows detached-start path and the Unix
  `start_quartonotebookrunner_detached.jl` path should be preserved as-is and re-tested.
- `Deno.connect({port})` → `node:net`; `Deno.TcpConn`/`Deno.Conn` type usage and the
  hand-rolled `writeAll` (`jsr:@std/io`) → `socket.write` with a promise wrapper. The
  length/framing read loop over the conn (L837+) needs rewriting against Node’s
  event-based sockets (or `socket[Symbol.asyncIterator]`). **This is the riskiest piece.**
- `Deno.readTextFileSync/writeTextFileSync/readFileSync/writeFileSync/removeSync` → `node:fs`.
- `Deno.env.get` → `process.env`. `Deno.kill(pid, "SIGTERM")` → `process.kill`.
- `Deno.consoleSize()` (terminal width for Julia) → `process.stdout.columns`
  — but under q2 the host’s stdout is not a TTY; likely returns `undefined` there
  (already `?? null`). Confirm intended behaviour.
- `Deno.stdout.writeSync(...)` (status + server log dump) → prefer `quarto.console.*`
  since stdout in the q2 host is not user-visible.
- Imports: `path`, `fs/exists`, `encoding/base64` (import-mapped) → `node:path`,
  `node:fs` `existsSync`, `Buffer.from(x).toString("base64")`.
- Tests: `tests/smoke/**` are Deno tests that shell out to `quarto render`; they would
  mostly keep working if only the runner changes, but `run-tests.sh` depends on
  Quarto’s bundled Deno. Port to vitest, run `q2 render`, and keep a Q1 leg if Q1 remains supported.
- Extension has `.jl` percent-script `claims-files` processor and static claims already (q2-ready).

## 4. Cross-cutting design decisions (need Gordon’s call)

1. **Dual runtime for extensions.** Do extension bundles have to keep working in Q1
   (Deno)? Both repos say so today (`quarto-required`, julia vendored into quarto-cli as a
   git subtree). If yes: port to `node:` built-ins (works in both) — **recommended**.
   If q2-only: either works, but still prefer `node:`.
   *Alternative:* a `globalThis.Deno` compat shim in the Node host
   (`Command`, `connect`, `readTextFileSync`, …). Fastest, zero extension changes, but it
   is a lie-by-implementation surface that will grow; a stopgap at best.
2. **Extension build tool — decided: esbuild.** `q2 call build-ts-extension` currently
   shells out to `deno bundle` with a tiered `deno.json` (`build.rs`). Switch it to
   esbuild, which q2 already uses (engine-host bundle, `quarto-hub-mcp`), pinned exactly.
   Earlier investigation found the port trivial. What remains is replacing the
   4-tier config resolution (`--config`, ext-local `deno.json`, workspace, shipped
   template) with esbuild equivalents (alias map for `@quarto/*` in workspace mode),
   and the `resources/extension-build/deno*.json` files.
   **Publishing is a pre-existing, orthogonal gap, not part of this port:** the
   shipped tier-4 template resolves `jsr:@quarto/api` / `jsr:@quarto/types`, and
   neither is published to jsr.io today (404), so the installed-binary path doesn’t
   work now and a Node/esbuild version would be equally unpublished on npm. The
   workspace tier (in-repo builds) works today and its esbuild equivalent will too.
   The port neither fixes nor worsens this.
3. **Node floor and discovery**: reuse `quarto-mcp-launcher/src/node.rs` (floor 24)?
4. **Package/crate naming**: rename `engine-host-deno` → `engine-host-node` now or leave.
5. **Transition**: support both runtimes behind a flag for a release (host bundle is
   already a separate artifact; two thin entry points + one esbuild config each), or hard cut.
6. **Julia is a vendored subtree in q2** (`resources/extension-subtrees/julia-engine/`,
   pulled from `quarto-julia-engine`). Julia changes land in that repo first and are
   re-subtreed into q2; q2 e2e tests exercise the vendored copy. Order the julia
   work accordingly (§5 step 4).

## 5. Suggested order of work

1. q2: `node-host.ts` + `main.ts`/`control-transport.ts` Node variants; convert the three
   `.deno-test.ts` to vitest; Node bundle; `ts_process.rs` spawn + discovery. Gate with the
   existing MockTransport tests plus `echo_engine_e2e`.
2. q2: port fixture extensions; consolidate `deno_available()` → `node_available()`; CI.
3. marimo (smallest, validates the end-to-end path incl. `marimo_engine_e2e`).
4. julia, in `quarto-julia-engine` first, then re-subtree into
   `resources/extension-subtrees/julia-engine/` (socket framing rewrite; Windows
   detached start). Gate: `julia_engine_e2e.rs` j1–j7.
5. q2 `build-ts-extension`: `deno bundle` → esbuild (decision 2); gate:
   `engine_fixture_build`, `build_ts_extension_e2e`.
6. Docs/plan updates; final grep for stray `deno`.

## 6. Open risks
- Node `net` sockets for julia’s protocol: backpressure and partial reads vs Deno’s `Conn.read`.
- Windows process spawning and detachment (no Windows CI per `cross-platform.md`).
- Engines in the wild that call `Deno.*` directly will break; needs a migration note
  (and possibly the shim in decision 1 as a compat layer for third-party engines).
- Bundle size/startup: Node startup is comparable to Deno; Plan 5 warm-pooling unaffected.

## 7. Existing tests (the gates for each step)

### q2 — Rust integration tests (`crates/quarto-core/tests/integration/`)
All are gated by `deno_available()` (+ tool gates) and early-return with `SKIP:`;
each needs the gate swapped to Node (consolidate into one shared helper).

| File | Gate | What it covers |
|---|---|---|
| `marimo_engine_e2e.rs` (8 tests: sc8, p4cb2, sc9, sc10, sc13, sc14, sc16, sc18) | deno + `uv` (+ R for sc16) | Full render through the host: static + dynamic claims parity, bare-SQL interop, widget header include, tagged SQL, knitr coexistence, error marker not crash |
| `marimo_resolution.rs` (sc11–sc17) | none (pure resolver, no subprocess) | Static claims resolution — **runtime-independent, no change needed** |
| `julia_engine_e2e.rs` (j1–j7, pc4a) | deno + `julia` (+ `QUARTO_JULIA_PROJECT`) | Minimal render, echo:false, exeflags/env, error doesn’t wedge host, website figure files, one host per project, abandoned-worker close, no worker leak on failed run |
| `echo_engine_e2e`, `behave_engine_e2e`, `synth_engines_e2e`, `synth_extension_subtree_e2e`, `capture_splice_seam`, `engine_registry_build` | deno | Synthetic engines — validate the host/transport with no Python/Julia dependency; **the first gate to turn green in step 1** |
| `engine_fixture_build.rs` (`build_helper_produces_bundle`) | deno | In-process `build_ts_extension` against `deno.workspace.json`; regenerates hermetic fixture bundles at test time — becomes the esbuild-build test |
| `crates/quarto/tests/integration/build_ts_extension_e2e.rs`, `engine_diagnostics_cli.rs` | deno | `q2 call build-ts-extension` CLI and diagnostics |
| `crates/quarto-core/src/engine/ts_process.rs` unit tests | mostly none (MockTransport); `deno_dialback_child` ones need deno | Transport/demux/respawn logic |
| `crates/quarto-preview/src/capture_driver.rs` | deno | Preview capture path |

Fixtures: `crates/quarto-core/tests/fixtures/extensions/marimo/` (a **copy** with its own
`src/` and `lib/` plus a committed `_extensions/marimo/marimo-engine.js`),
`.../julia-website/` (project only; the engine is the vendored subtree), and the
synthetic ones (alpha, beta, behave, echo-\*, …). The marimo fixture’s `Deno.*` calls
need the same port as the upstream repo, or be re-synced from it.

### q2 — TS (`ts-packages/quarto-engine-host-deno`)
`host.test.ts` (~3.4k lines), `framing`, `engine-loader`, `mapped-source`,
`metadata-as-format`, `quarto-api` tests run under **vitest/Node already**.
Deno-only: `deno-host.deno-test.ts`, `control-transport.deno-test.ts`,
`wire-parity.deno-test.ts` → convert to vitest (new `node-host`/Node control-transport
tests replace the first two).

### quarto-marimo (`~/src/quarto-marimo/tests/`)
- TS (Deno test, `--no-check`; 4 files, ~560 lines, ~67 cases): `cell-execution-regex`,
  `claims-language`, `is-marimo-cell`, `render-output`, with `tests/mocks/quarto-types.ts`.
  Pure logic over `lib/*.ts` and the claims function; **none exercise `Deno.*`**, so they
  port by swapping the runner (vitest or `node --test`) and replacing the
  `deno.json` import map (`@quarto/types` → mock, `path` → `node:path`).
- Python (`tests/python/`: `test_command.py`, `test_extract.py`, pytest via `uv`) —
  runtime-independent, unchanged.
- No end-to-end render test lives in this repo; the e2e coverage is q2’s
  `marimo_engine_e2e.rs`. CI (`ci.yml`) installs Deno only for the TS tests.
  **Gap:** nothing in the repo tests `executePython` or the temp-file header include
  (the ported `Deno.*` code) except through q2’s e2e — worth a unit test with a fake
  python when porting.

### quarto-julia-engine (`~/src/quarto-julia-engine/tests/`)
- `smoke/julia-engine/render.test.ts` (5 tests) and `julia.test.ts` (1, ~245 lines):
  Deno tests that shell out to `quarto render` over `tests/docs/julia-engine/*`
  (keep-ipynb, sleep, source-ranges, engine-reordering, error). They use `jsr:@std/*`
  and `Deno.Command` **only as a test driver**, not engine code. These are
  Q1-targeted smoke tests (`run-tests.sh` uses Quarto’s bundled Deno); they keep
  working against Q1 unchanged if the engine stays dual-runtime, so porting the *runner* is optional
  unless the goal is to drop Deno from this repo’s dev loop.
- CI runs on Linux/macOS/Windows against a pinned quarto-cli rev and verifies the
  bundled JS matches the TS source (so the esbuild switch changes the bundle bytes
  and the freshness check must use the same bundler as the committed artifact).
- The vendored copy in `resources/extension-subtrees/julia-engine/` carries these tests too.
- q2’s `julia_engine_e2e.rs` is the Node-runtime coverage (j1–j7 above); it is the
  main gate for the socket-framing rewrite and detached-server behaviour.

### Coverage gaps to close during the port
1. Unit tests for the new `node-host.ts` (fs/mktemp suffix, exec knobs, walk, onExit).
2. Julia socket framing against a fake TCP server (no Julia needed).
3. Marimo `executePython` with a stub interpreter.
4. A Windows-path check for detached julia start (no Windows CI; manual).
