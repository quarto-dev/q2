# Plan 9: `q2 call engine` — Q1-parity engine CLI surface (bd-m1jeqhhz)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `q2 call engine <name> [args...]` with behavior identical to Q1's
`quarto call engine ...` — same subcommand tree, help text, output, error messages,
and exit codes — via a one-shot `call-engine` mode in the Deno engine-host bundle
(vendored cliffy v1.0.0-rc.3) plus a minimal `call_engine_command` hook on
`ExecutionEngine`.

**Architecture:** Rust does registry lookup and gate 1 (`Unknown engine:`), then
dispatches through the new trait method. `TsEngine`'s override spawns
`deno run --allow-all <bundle> call-engine <config-json> <engine-path> <name> <args...>`
with **inherited stdio** and propagates the exit code; the call-engine mode replicates
Q1's `engine-cmd.ts` dispatcher verbatim (import engine → gate 2 → real cliffy
`Command` → `populateCommand` → `parse(args)`). Native engines keep the default
(`NotSupported`) → the CLI prints Q1's exact `Engine <name> does not support
subcommands`. No new wire verbs; the JSONL protocol is untouched.

**Tech Stack:** Rust (typed `CallCommands::Engine` clap subcommand — see the plan1c3 coordination note in Global Constraints — plus `std::process::Command`), Deno 2 +
esbuild 0.28.0 (engine-host bundle), vendored cliffy v1.0.0-rc.3 (MIT).

**References:**
- Research + extracted Q1 spec: `claude-notes/research/2026-07-03-plan9-call-engine-research.md`
- Byte-parity oracle corpus: `claude-notes/research/2026-07-03-plan9-q1-observed/`
- Strand: bd-m1jeqhhz. Epic: ts-engine-extensions (Plan 9); integration branch
  `feature/ts-engine-extensions`.
- Design + naming (`call_engine_command`/`CallEngineOutcome`/`call-engine` mode) and
  Option-1 approach approved by Gordon 2026-07-03. Ratified deviations: research doc
  §8 (D-1..D-10) with §10.4 resolutions (D-2 first-line-only, D-6 Q1 builtin order,
  D-7/D-8/D-9 inherent).

## Global Constraints

- **Q1's observed behavior is the spec.** Every user-visible string, stream
  (stdout vs stderr), and exit code matches the corpus byte-for-byte unless covered
  by a ratified deviation. Never "improve" a wart (e.g. `Usage: COMMAND`, silent
  no-op exit 0).
- Byte-oracle comparisons run with `NO_COLOR` **removed** from the child env
  (cliffy/std colors key off `Deno.noColor`, not tty).
- Path-scoped commits only (`git commit <paths>`); never `git add -A`. Never push.
- Never reference `external-sources/` from compiled code or tests.
- WASM: no changes touch wasm paths, but `traits.rs` is shared — keep the new trait
  method sync (no async, matches existing trait style).
- Integration tests go in `tests/integration/<name>.rs` + registration in `main.rs`
  (never top-level `tests/*.rs`).
- **Runs after plan1c3, which refactors `Call`.** plan1c3 converts `main.rs`'s
  `Call { function, args }` into a typed `Call { command: CallCommands }` group
  (variants `Test` + `build-ts-extension`). This plan therefore adds a **typed
  `Engine` variant to `CallCommands`**, not a bare clap string-dispatch arm — see
  Task 7.4. (Without the typed variant, clap rejects `call engine` as an unknown
  subcommand.) The pre-existing `Some("engine")` arm in `call/mod.rs` still runs:
  the new variant routes through `commands::call::execute(Some("engine"), args)`,
  mirroring how plan1c3 routes `CallCommands::Test`.
- Once a Test Seam row is GREEN its harness/assertions are **frozen** — fix
  production or the spec, never the test. Dated findings are appended to the row,
  never overwritten.
- If a byte-oracle test fails for an *environmental* reason (e.g. help-table width
  differing across Deno versions), do NOT silently relax the assertion: record a
  dated finding in the seam row, capture the actual bytes, and raise it for
  ratification as a new deviation.

## File structure

| Path | Role |
|---|---|
| `ts-packages/quarto-engine-host-deno/vendor/cliffy/` (new, ~59 files) | vendored cliffy v1.0.0-rc.3 (`command/`, `flags/`, `table/`, `_utils/`) |
| `ts-packages/quarto-engine-host-deno/vendor/deno-std/` (new, 6 files) | vendored std@0.196.0 leaves (fmt/colors, console/*, assert/*) |
| `ts-packages/quarto-engine-host-deno/vendor/README.md`, `vendor/LICENSE-cliffy` (new) | provenance, patch list, MIT license |
| `ts-packages/quarto-engine-host-deno/src/call-engine.ts` (new) | the one-shot mode (Q1 dispatcher port) |
| `ts-packages/quarto-engine-host-deno/src/call-engine.deno-test.ts` (new) | deno-tier tests against the built bundle |
| `ts-packages/quarto-engine-host-deno/src/main.ts` (modify) | argv branch into call-engine mode |
| `crates/quarto-core/src/engine/traits.rs` (modify) | `CallEngineOutcome` + `call_engine_command` default |
| `crates/quarto-core/src/engine/mod.rs` (modify) | re-export `CallEngineOutcome` |
| `crates/quarto-core/src/engine/ts_process.rs` (modify) | `extracted_bundle_path` → `pub(crate)`; `TsEngineHost::global_config()` accessor |
| `crates/quarto-core/src/engine/ts_engine.rs` (modify) | `call_engine_command` override + argv builder |
| `crates/quarto/src/commands/call/engine.rs` (new) | CLI arm: gates, help, dispatch, exit-code propagation |
| `crates/quarto/src/main.rs` (modify) | add typed `Engine` variant to plan1c3's `CallCommands` enum + its dispatch arm |
| `crates/quarto/src/commands/call/mod.rs` (modify) | `Some("engine")` arm (reached via the `CallCommands::Engine` dispatch) + usage text |
| `crates/quarto/tests/fixtures/call-engine-oracle/` (new) | committed byte-oracle copies + README |
| `crates/quarto/tests/integration/call_engine_e2e.rs` (new) + `main.rs` (modify) | binary-driving e2e |
| `.github/workflows/ts-test-suite.yml` (modify) | deno test step for call-engine mode |

## Test Seam Spec (frozen — prevalidated 2026-07-03)

One row per test: **tier · real unit (never mocked) · seam → assertion surface ·
mock boundary · named revert hunk → RED**. Tiers: `unit-rs` (pure), `deno`
(deno test driving the **built bundle** as a subprocess), `e2e-rs` (real `q2` binary
via `CARGO_BIN_EXE_q2`, deno-gated skip), `e2e-live` (opt-in env-gated, not CI).
Oracle files live in `crates/quarto/tests/fixtures/call-engine-oracle/` (committed
copies of the research corpus; README records provenance).

| ID | Phase | Tier | Real unit | Seam → assertion surface | Mock boundary | Revert hunk → RED |
|----|-------|------|-----------|--------------------------|---------------|-----------------------|
| CE1 | 3 | unit-rs | `available_engines_list` | registry with builtins + a TS engine → exactly `knitr, jupyter, markdown, julia` (D-6 order) | none (real `EngineRegistry`) | Drop the builtin-reorder (emit `engines_in_order()` raw) → string starts `markdown` → RED |
| CE2 | 2 | unit-rs | trait default `call_engine_command` | struct implementing only `name`/`execute` → call → `Err(NotSupported("call_engine_command"))` | none | Change default to `Ok(CallEngineOutcome{exit_code:0})` → `matches!(… Err(NotSupported(_)))` RED |
| CE3 | 2 | unit-rs | `build_call_engine_argv` | argv vector == `["run","--allow-all",<bundle>,"call-engine",<json>,<engine-path>,<name>,args…]` | none (pure fn) | Remove the `"call-engine"` mode literal → positional equality RED |
| CE4 | 3 | unit-rs | gate-message constants/fns in `call/engine.rs` | `unknown_engine_message("x", …)` == `"Unknown engine: x"` + `"Available engines: …"`; `no_subcommands_message("x")` == `"Engine x does not support subcommands"`; `MISSING_ENGINE_NAME` == `"ERROR: Missing argument(s): engine-name"` (D-2) | none | Alter `"does not support subcommands"` wording → literal equality RED |
| CE5 | 1 | deno | call-engine mode, gate 2 | bundle + **echo-engine fixture** (`dist/echo-engine.js`, really has no `populateCommand`) → stderr == `"Engine echo does not support subcommands\n"`, exit 1, stdout empty | none (real bundle, real fixture) | Remove the `populateCommand` presence check (always proceed) → silent exit 0 → RED |
| CE6 | 1 | deno | call-engine mode + vendored cliffy render | julia fixture bundle, args `["--help"]` → stdout **byte-equal** oracle `call-engine-julia-help.txt`, exit 0 | none | Add `.name("julia")` to the temp `Command` → `Usage:` line differs → RED (binds the unnamed-command wart, D-1 replicate) |
| CE7 | 1 | deno | cliffy error path | julia fixture, args `["frobnicate"]` → exit **2**, output byte-equal oracle `julia-unknown-subcmd.txt` (help + `Unknown command "frobnicate". Did you mean command "close"?`) | none | Wrap `parse(args)` in try/catch returning 1 → exit-code assertion RED |
| CE8 | 1 | deno | bare-invocation quirk | julia fixture, args `[]` → exit 0, stdout AND stderr empty (D-3 replicate) | none | Add a default `showHelp()` action to the temp command → empty-stdout RED |
| CE9 | 1 | deno | `init(quartoAPI)` + runtimeDir plumbing | julia fixture, args `["status"]`, config JSON with `runtimeDir` = fresh tempdir → stderr == `"Julia control server is not running.\n"`, exit 0, **and** `<tempdir>/julia/` was created (proves the action ran against OUR runtimeDir, not `$HOME`) | none (no daemon involved) | Drop `await discovery.init(quartoAPI)` before parse → action throws (engine's `quarto` is undefined) → nonzero exit → RED |
| CE10 | 4 | e2e-rs | full chain: clap arm → registry → TsEngine override → spawn → cliffy | temp project with julia fixture; `q2 call engine julia --help` → stdout byte-equal oracle, exit 0 | none (deno-gated skip) | Delete the `TsEngine` `call_engine_command` override (fall to default) → stderr `does not support subcommands` → RED |
| CE11 | 4 | e2e-rs | gate 1 + gate messages through the binary | (a) `q2 call engine nonexistent foo` in julia project → stderr lines byte-equal `unknown.err.txt` (incl. `Available engines: knitr, jupyter, markdown, julia`), exit 1, stdout empty; (b) `q2 call engine markdown x` → stderr byte-equal `nosupport.err.txt` (name `markdown`), exit 1; (c) `q2 call engine` → stderr first line == `ERROR: Missing argument(s): engine-name`, exit 1 (D-2); (d) `q2 call engine --help` AND `q2 call engine help` → stdout == the `ENGINE_HELP` static text, exit 0 | none | (a) CE1's reorder hunk → RED; (b) route `NotSupported` into anyhow `?` propagation instead of the message+exit(1) → stderr differs → RED; (d) remove the help branch in `execute()` → `--help` falls through to gate 1 (`Unknown engine: --help`) → RED |
| CE12 | 4 | e2e-rs | exit-code propagation | julia project; `q2 call engine julia frobnicate` → exit **2** | none | Replace `std::process::exit(outcome.exit_code)` with `std::process::exit(0)` on success path → RED |
| CE13 | 4 | e2e-rs | Rust-side `HostGlobalConfig` plumbing | temp `HOME`; `q2 call engine julia status` → stderr `Julia control server is not running.`, exit 0, stdout empty, and `<tempHOME>`-derived runtime dir contains `julia/` afterwards | none | In the override, pass a `HostGlobalConfig` with `runtime_dir: String::new()` instead of `host.global_config()` → engine `ensureDir` misbehaves / dir-created assertion fails → RED |
| CE14 | 4 | e2e-live | live daemon round-trip (**opt-in**: `QUARTO_E2E_JULIA_DAEMON=1`, never CI) | temp `HOME` + `SharedTransportSentinel` pattern; render `minimal.qmd` (starts detached server) → `status` stdout starts `QuartoNotebookRunner server status:` → `stop` → stderr `Server stopped.`, transport file gone; QNR process count restored (bd-l9jhy5u0 guard) | none | Environmental confirmation row — binding is carried by CE9/CE10/CE13's named reverts; no separate hunk (logged deliberately, see vacuity notes) |

**Vacuity notes:**
- **CE6/CE7/CE10 byte-oracles** — run children with `NO_COLOR` removed
  (`.env_remove("NO_COLOR")` / deno `env` option) so ANSI matches the corpus; the
  corpus was captured non-tty, tests are non-tty → cliffy width handling matches. If
  a Deno-version width artifact appears, follow the Global Constraints
  environmental-failure protocol (dated finding + ratification, not silent
  normalization).
- **CE9 exercises the path**: without the `<tempdir>/julia/` created-dir assertion,
  a mode that ignored our config and hit the real `$HOME` transport dir could pass
  the same message vacuously (or worse, report a REAL running daemon).
- **CE8 asserts both streams empty** — asserting exit 0 alone would pass a
  help-printing implementation.
- **CE11(a) needs the julia fixture present** so the list is byte-identical to Q1's
  4-engine string; without an extension, the builtins-only list `knitr, jupyter,
  markdown` is asserted in the same test as a second case (binds the reorder for the
  no-extension path too).
- **CE14 is deliberately non-binding**: every hunk it touches is bound elsewhere;
  its value is live-protocol confirmation (HMAC/isready against a real QNR server),
  which no CI tier can give. It follows the julia_engine_e2e temp-HOME +
  transport-sentinel discipline; count `QuartoNotebookRunner` processes before/after
  (worker-leak bd-l9jhy5u0).

**Missing-test pass (accepted-untested, logged):**
- **Deno absent** (`Install Deno from https://deno.land/` path in the override):
  simulating an empty `PATH` cross-platform is flaky (absolute-path deno installs,
  Windows lookup rules). Accepted-untested; the message reuses the existing
  `ensure_started` pattern whose text already has coverage in ts_process tests.
- **Windows behavior** of the julia subcommands (SIGTERM mapping, PowerShell spawn):
  engine-owned code, exercised only on Windows CI where deno-gated legs are
  currently excluded (see `claude-notes/instructions/windows-dev.md`). Logged.
- **Undeclared-name TS engine** (`echo-legacy`) addressable only by extension id in
  gate 1: pre-existing registry semantics (aliases populate only post-LoadEngine),
  not changed by this plan. Logged; a `braid` follow-up is warranted only if a real
  engine hits it.
- **Vendored-cliffy drift**: covered structurally by the existing CI bundle
  freshness gate (rebuild + `git diff --exit-code`), no new test.

---

## Phase 0 — Vendor cliffy v1.0.0-rc.3

### Task 1: vendor tree + bundle proof

**Files:** create `ts-packages/quarto-engine-host-deno/vendor/{cliffy,deno-std}/…`,
`vendor/README.md`, `vendor/LICENSE-cliffy`.

**Interfaces — Produces:** `vendor/cliffy/command/mod.ts` exporting `Command`,
importable from `src/*.ts` as `../vendor/cliffy/command/mod.ts`.

- [ ] **1.1 Mirror the module graph** (from repo root; network required once):

```bash
cd ts-packages/quarto-engine-host-deno
mkdir -p vendor
deno info --json https://deno.land/x/cliffy@v1.0.0-rc.3/command/mod.ts \
  | jq -r '.modules[].specifier' | sort > /tmp/cliffy-graph.txt
while read url; do
  case "$url" in
    https://deno.land/x/cliffy@v1.0.0-rc.3/*)
      rel="vendor/cliffy/${url#https://deno.land/x/cliffy@v1.0.0-rc.3/}" ;;
    https://deno.land/std@0.196.0/*)
      rel="vendor/deno-std/${url#https://deno.land/std@0.196.0/}" ;;
  esac
  mkdir -p "$(dirname "$rel")"; curl -sf "$url" -o "$rel"
done < /tmp/cliffy-graph.txt
find vendor -type f | wc -l   # expect ~65
```

- [ ] **1.2 Apply the three mechanical patches** (spike-proven, research §10.1):

```bash
# absolute std URLs → relative vendored paths (only these two files reference std)
sed -i '' 's|https://deno.land/std@0.196.0/|../../deno-std/|g' \
  vendor/cliffy/command/deps.ts vendor/cliffy/table/deps.ts
# Deno-2-fatal legacy JSON import syntax
sed -i '' 's|assert { type: "json" }|with { type: "json" }|' \
  vendor/deno-std/console/unicode_width.ts
grep -rn 'https://deno.land' vendor/ && echo "FAIL: absolute imports remain" || echo OK
```

- [ ] **1.3 Provenance files.** `vendor/LICENSE-cliffy`: copy the MIT license from
  https://github.com/c4spar/deno-cliffy/blob/v1.0.0-rc.3/LICENSE (curl it).
  `vendor/README.md`:

```markdown
# Vendored: cliffy v1.0.0-rc.3 (+ std@0.196.0 leaves)

Vendored 2026-07 for the `call-engine` host mode (Plan 9, bd-m1jeqhhz) so the
engine-host bundle renders `q2 call engine <name>` help/errors with the EXACT
CLI library Q1 uses (byte parity; see
claude-notes/research/2026-07-03-plan9-call-engine-research.md §9.1/§10.1).

Source: https://deno.land/x/cliffy@v1.0.0-rc.3/command/mod.ts module graph
(65 files). MIT — see LICENSE-cliffy. Local patches (the ONLY edits):
1. command/deps.ts, table/deps.ts: std URLs → ../../deno-std/… relative paths.
2. deno-std/console/unicode_width.ts: `assert { type: "json" }` → `with { … }`
   (legacy syntax is a hard error in Deno 2).
Do not upgrade in place — byte parity is pinned to rc.3 (Q1's import map).
```

- [ ] **1.4 Prove the bundle still builds** (cliffy not yet imported — this is the
  baseline): run `cargo xtask build-engine-host-bundle`. Expected: success,
  `dist/engine-host-deno.js` unchanged (vendored files unreferenced ⇒ not bundled).

- [ ] **1.5 Commit** (path-scoped):

```bash
git add ts-packages/quarto-engine-host-deno/vendor
git commit -m "feat(engine-host): vendor cliffy v1.0.0-rc.3 for call-engine mode (bd-m1jeqhhz)" \
  ts-packages/quarto-engine-host-deno/vendor
```

## Phase 1 — `call-engine` mode in the engine host

### Task 2: the mode + main.ts branch

**Files:** create `src/call-engine.ts`; modify `src/main.ts`
(both under `ts-packages/quarto-engine-host-deno/`).

**Interfaces — Consumes:** `buildQuartoAPI(global, host)` (`src/quarto-api.ts`),
`denoHost` (`src/deno-host.ts`), `HostGlobalConfig` (`src/types.ts`), vendored
`Command`. **Produces:** argv contract
`call-engine <config-json> <engine-path> <engine-name> [engineArgs...]`; process
exit code is the outcome.

- [ ] **2.1 Write `src/call-engine.ts`:**

```ts
/**
 * One-shot `call-engine` mode — the q2 counterpart of Q1's
 * src/command/call/engine-cmd.ts second stage (Plan 9, bd-m1jeqhhz).
 *
 * argv: <config-json> <engine-path> <engine-name> [engineArgs...]
 *
 * Runs with INHERITED stdio: engine actions (julia status/log) write raw
 * bytes to the real stdout; cliffy renders help/errors exactly as Q1.
 * Parity contract: claude-notes/research/2026-07-03-plan9-call-engine-research.md
 * §2.2 + the observed corpus. Deviations there govern; do not "fix" warts
 * (unnamed temp command, silent bare no-op).
 */
import { pathToFileURL } from "node:url";
import { Command } from "../vendor/cliffy/command/mod.ts";
import { buildQuartoAPI } from "./quarto-api.ts";
import { denoHost } from "./deno-host.ts";
import type { HostGlobalConfig } from "./types.ts";

export async function runCallEngine(argv: string[]): Promise<number> {
  const [configJson, enginePath, engineName, ...args] = argv;
  if (!configJson || !enginePath || !engineName) {
    console.error(
      "call-engine: expected <config-json> <engine-path> <engine-name> [args...]",
    );
    return 1;
  }
  const global = JSON.parse(configJson) as HostGlobalConfig;
  const mod = await import(pathToFileURL(enginePath).href);
  const discovery = mod?.default;
  if (!discovery?.populateCommand) {
    // Q1 gate 2, verbatim (engine-cmd.ts:37); exit code 1.
    console.error(`Engine ${engineName} does not support subcommands`);
    return 1;
  }
  // Q1 calls engine.init(quartoAPI) at registration; the julia handlers use
  // the module-level `quarto` captured there. Mirror host.ts loadEngine.
  if (typeof discovery.init === "function") {
    await discovery.init(buildQuartoAPI(global, denoHost));
  }
  // Q1 engine-cmd.ts:41-48 verbatim: fresh UNNAMED command (the `Usage:
  // COMMAND` wart is part of the parity contract), description, populate,
  // second-stage parse. cliffy handles --help (exit 0 via Deno.exit),
  // unknown subcommands (exit 2), and action errors itself.
  const engineSubcommand = new Command()
    .description(
      `Access functionality specific to the ${engineName} rendering engine.`,
    );
  discovery.populateCommand(engineSubcommand);
  await engineSubcommand.parse(args);
  return 0; // bare invocation: silent no-op, exit 0 (deviation D-3: replicate)
}
```

- [ ] **2.2 Modify `src/main.ts`** — insert the branch before `runHost` (keep the
  existing doc comment; add one line to it noting the call-engine mode):

```ts
import { runHost } from "./host.ts";
import { denoHost } from "./deno-host.ts";
import { runCallEngine } from "./call-engine.ts";

if (import.meta.main) {
  if (Deno.args[0] === "call-engine") {
    Deno.exit(await runCallEngine(Deno.args.slice(1)));
  }
  await runHost(Deno.stdin.readable, Deno.stdout, denoHost);
  Deno.exit(0);
}
```

- [ ] **2.3 Typecheck + rebuild the bundle:**

```bash
deno check ts-packages/quarto-engine-host-deno/src/main.ts
cargo xtask build-engine-host-bundle
```
Expected: both succeed; `dist/engine-host-deno.js` grows by roughly the vendored
cliffy size (~56 KB minified; spike-measured).

- [ ] **2.4 Manual smoke (end-to-end through the real bundle):**

```bash
CFG='{"resourceDir":"/tmp","runtimeDir":"/tmp/plan9-smoke-rt","dataDir":"/tmp","pandocPath":null,"isInteractiveSession":false,"runningInCi":true,"quartoVersion":"0.0.0"}'
deno run --allow-all ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js \
  call-engine "$CFG" \
  "$(pwd)/crates/quarto-core/tests/fixtures/extensions/julia-engine/_extensions/julia-engine/julia-engine.js" \
  julia --help
```
Expected: the julia five-command help, `Usage: COMMAND`, ANSI-colored — visually
compare against `claude-notes/research/2026-07-03-plan9-q1-observed/call-engine-julia-help.txt`.
**Record the invocation + a snippet in the strand comment** (end-to-end
verification rule).

- [ ] **2.5 Commit** `src/call-engine.ts src/main.ts dist/` (path-scoped, one
  commit — the dist rebuild belongs with the source that produced it).

### Task 3: deno-tier tests (CE5–CE9)

**Files:** create `ts-packages/quarto-engine-host-deno/src/call-engine.deno-test.ts`.
(Automatically excluded from vitest by the `*.deno-test.ts` glob.)

**Interfaces — Consumes:** the **built bundle** `dist/engine-host-deno.js` (tests
must rebuild first — this is what production embeds) and the fixtures
`crates/quarto-core/tests/fixtures/extensions/{julia-engine,echo-engine}/_extensions/*/{julia-engine,dist/echo-engine}.js`.

- [ ] **3.1 Write the failing tests** — one `Deno.test` per seam row CE5–CE9.
  Skeleton (repeat the runner helper; assertions per row from the Test Seam Spec):

```ts
// Test files are NOT bundled — jsr: specifiers are fine here (the vendored
// deno-std is for the bundle only; don't import it from tests).
import { assert, assertEquals } from "jsr:@std/assert";

const root = new URL("../../../", import.meta.url); // repo root
const bundle = new URL("ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js", root).pathname;
const juliaEngine = new URL(
  "crates/quarto-core/tests/fixtures/extensions/julia-engine/_extensions/julia-engine/julia-engine.js",
  root,
).pathname;
const echoEngine = new URL(
  "crates/quarto-core/tests/fixtures/extensions/echo-engine/_extensions/echo-engine/dist/echo-engine.js",
  root,
).pathname;
const oracle = (name: string) =>
  Deno.readTextFileSync(new URL(`crates/quarto/tests/fixtures/call-engine-oracle/${name}`, root));

function cfg(runtimeDir: string): string {
  return JSON.stringify({
    resourceDir: runtimeDir, runtimeDir, dataDir: runtimeDir, pandocPath: null,
    isInteractiveSession: false, runningInCi: true, quartoVersion: "0.0.0",
  });
}

async function runCallEngine(enginePath: string, name: string, args: string[], runtimeDir: string) {
  const env = { ...Deno.env.toObject() };
  delete env.NO_COLOR; // byte-oracle parity: corpus captured with colors on
  const out = await new Deno.Command("deno", {
    args: ["run", "--allow-all", bundle, "call-engine", cfg(runtimeDir), enginePath, name, ...args],
    env, clearEnv: true, stdout: "piped", stderr: "piped",
  }).output();
  return {
    code: out.code,
    stdout: new TextDecoder().decode(out.stdout),
    stderr: new TextDecoder().decode(out.stderr),
  };
}

Deno.test("CE5: engine without populateCommand → Q1 gate-2 message, exit 1", async () => {
  const r = await runCallEngine(echoEngine, "echo", [], await Deno.makeTempDir());
  assertEquals(r.stderr, "Engine echo does not support subcommands\n");
  assertEquals(r.code, 1);
  assertEquals(r.stdout, "");
});

Deno.test("CE6: julia --help is byte-identical to the Q1 oracle", async () => {
  const r = await runCallEngine(juliaEngine, "julia", ["--help"], await Deno.makeTempDir());
  assertEquals(r.code, 0);
  assertEquals(r.stdout, oracle("call-engine-julia-help.txt"));
});

Deno.test("CE7: unknown subcommand → help + did-you-mean, exit 2", async () => {
  const r = await runCallEngine(juliaEngine, "julia", ["frobnicate"], await Deno.makeTempDir());
  assertEquals(r.code, 2);
  assertEquals(r.stdout + r.stderr, oracle("julia-unknown-subcmd.txt"));
});

Deno.test("CE8: bare invocation is a silent no-op, exit 0", async () => {
  const r = await runCallEngine(juliaEngine, "julia", [], await Deno.makeTempDir());
  assertEquals(r.code, 0);
  assertEquals(r.stdout, "");
  assertEquals(r.stderr, "");
});

Deno.test("CE9: status against empty runtimeDir → not-running info, dir created", async () => {
  const rt = await Deno.makeTempDir();
  const r = await runCallEngine(juliaEngine, "julia", ["status"], rt);
  assertEquals(r.code, 0);
  assertEquals(r.stdout, "");
  assert(r.stderr.includes("Julia control server is not running."));
  const st = await Deno.stat(`${rt}/julia`); // proves OUR runtimeDir was used
  assert(st.isDirectory);
});
```

  Notes for the implementer: import assertions from `jsr:@std/assert` (the comment
  in the skeleton's first line is a deliberate strike-through of the wrong idea —
  test files are not bundled, `jsr:` is fine there); CE7's oracle concatenation
  order (stdout then stderr) must be verified against how cliffy rc.3 splits
  help-vs-error across streams — if the split differs from the single-stream
  corpus capture, byte-compare the *concatenation* as shown and record the split
  as a dated finding on the row. Exit codes in the corpus `*.exit` files are the
  authority.

- [ ] **3.2 Run to verify current state → CE5–CE9 must FAIL before Task 2 is
  merged / PASS after** (TDD ordering across Tasks 2–3: if executing sequentially,
  write 3.1 first, watch it fail with "call-engine: expected …"/module-not-found,
  then land Task 2 and re-run):

```bash
cargo xtask build-engine-host-bundle   # tests drive the ARTIFACT
deno test --allow-all ts-packages/quarto-engine-host-deno/src/call-engine.deno-test.ts
```
Expected after Task 2: 5 passed.

- [ ] **3.3 Prove the named reverts (fail-on-revert pass).** For CE5, CE6, CE8, CE9:
  apply each row's revert hunk to `src/call-engine.ts` (one at a time), rebuild the
  bundle, re-run, confirm the named assertion goes RED, restore byte-identical
  (`git checkout -- src/call-engine.ts`), rebuild, confirm GREEN. Record each RED
  verbatim in the test's doc comment (plan4c convention). CE7's revert (try/catch)
  likewise.

- [ ] **3.4 Commit** the test file + oracle fixtures dir (created in Task 7 step
  7.1 if executing out of order — otherwise create it here, see Task 7.1 for the
  copy commands) path-scoped.

### Task 4: CI wiring

**Files:** modify `.github/workflows/ts-test-suite.yml`.

- [ ] **4.1** Add after the existing wire-parity deno test step (which already runs
  post-bundle-rebuild, so the artifact is fresh):

```yaml
      - name: call-engine mode tests (deno)
        run: deno test --allow-all ts-packages/quarto-engine-host-deno/src/call-engine.deno-test.ts
```

- [ ] **4.2 Commit** path-scoped.

## Phase 2 — Rust trait hook + TsEngine override

### Task 5: `CallEngineOutcome` + trait default (CE2)

**Files:** modify `crates/quarto-core/src/engine/traits.rs`,
`crates/quarto-core/src/engine/mod.rs`.

**Interfaces — Produces:**
`pub struct CallEngineOutcome { pub exit_code: i32 }` and
`fn call_engine_command(&self, args: &[String]) -> Result<CallEngineOutcome, ExecutionError>`
(default `Err(NotSupported("call_engine_command"))`), re-exported from
`quarto_core::engine`.

- [ ] **5.1 Write the failing test** (in `traits.rs` `#[cfg(test)]` module, next to
  the existing default-impl tests):

```rust
struct NoCommandsEngine;
impl ExecutionEngine for NoCommandsEngine {
    fn name(&self) -> &str { "no-commands" }
    fn execute(&self, _input: &str, _ctx: &ExecutionContext)
        -> Result<ExecuteResult, ExecutionError> { unimplemented!() }
}

#[test]
fn call_engine_command_defaults_to_not_supported() {
    let e = NoCommandsEngine;
    let r = e.call_engine_command(&["status".to_string()]);
    assert!(matches!(r, Err(ExecutionError::NotSupported("call_engine_command"))));
}
```

- [ ] **5.2** `cargo nextest run -p quarto-core -E 'test(call_engine_command_defaults)'`
  → FAIL (method does not exist).

- [ ] **5.3 Implement** in `traits.rs` (after `is_alive`, matching the
  `markdown_for_file` default-Err precedent at lines 170–176):

```rust
/// Outcome of an engine-contributed CLI invocation (`q2 call engine <name> …`).
/// Carries the child's exit code so the CLI can propagate it exactly —
/// Q1 parity requires 0/1/2 to survive untouched (bd-m1jeqhhz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallEngineOutcome {
    pub exit_code: i32,
}

    /// Q1 `populateCommand` equivalent: run one of this engine's contributed
    /// CLI subcommands (`q2 call engine <name> <args…>`). Engines without a
    /// command surface keep this default; the CLI layer maps `NotSupported`
    /// to Q1's exact "Engine <name> does not support subcommands" (exit 1).
    fn call_engine_command(
        &self,
        _args: &[String],
    ) -> Result<CallEngineOutcome, ExecutionError> {
        Err(ExecutionError::not_supported("call_engine_command"))
    }
```
Re-export in `engine/mod.rs` alongside the existing trait re-exports:
`pub use traits::CallEngineOutcome;`.

- [ ] **5.4** Re-run the test → PASS. Then CE2's revert proof: change the default to
  `Ok(CallEngineOutcome { exit_code: 0 })`, re-run → RED, restore → GREEN; record in
  the test doc comment.

- [ ] **5.5 Commit** path-scoped.

### Task 6: TsEngine override + spawn plumbing (CE3)

**Files:** modify `crates/quarto-core/src/engine/ts_process.rs`,
`crates/quarto-core/src/engine/ts_engine.rs`.

**Interfaces — Consumes:** Task 5's trait items; `ts_process::is_available()` (pub),
`extracted_bundle_path()` (to be `pub(crate)`), `HostGlobalConfig` serde
(camelCase → matches the TS `HostGlobalConfig` interface). **Produces:**
`pub(crate) fn build_call_engine_argv(bundle: &Path, config_json: &str, engine_path: &Path, engine_name: &str, args: &[String]) -> Vec<std::ffi::OsString>`
in `ts_engine.rs`.

- [ ] **6.1 Visibility bumps in `ts_process.rs`:** change
  `fn extracted_bundle_path()` (line ~136) to `pub(crate) fn`; add to
  `impl TsEngineHost` a config accessor:

```rust
    /// The process-stable global config this host was constructed with.
    /// Used by TsEngine::call_engine_command to give the one-shot
    /// call-engine process the SAME runtime/data dirs as the render path
    /// (same transport files ⇒ same julia daemon).
    pub(crate) fn global_config(&self) -> &HostGlobalConfig {
        &self.global
    }
```
(If `new()` currently consumes the config without storing it, store it in a new
`global: HostGlobalConfig` field at construction — it is `Clone`.)

- [ ] **6.2 Write the failing CE3 test** (in `ts_engine.rs` tests):

```rust
#[test]
fn build_call_engine_argv_shape() {
    let argv = build_call_engine_argv(
        Path::new("/rt/bundles/engine-host-deno-abc.js"),
        r#"{"runtimeDir":"/rt"}"#,
        Path::new("/proj/_extensions/julia-engine/julia-engine.js"),
        "julia",
        &["close".to_string(), "nb.qmd".to_string(), "--force".to_string()],
    );
    let expect: Vec<std::ffi::OsString> = [
        "run", "--allow-all", "/rt/bundles/engine-host-deno-abc.js",
        "call-engine", r#"{"runtimeDir":"/rt"}"#,
        "/proj/_extensions/julia-engine/julia-engine.js", "julia",
        "close", "nb.qmd", "--force",
    ].iter().map(Into::into).collect();
    assert_eq!(argv, expect);
}
```

- [ ] **6.3** Run → FAIL (fn missing). **Implement** in `ts_engine.rs`:

```rust
pub(crate) fn build_call_engine_argv(
    bundle: &Path,
    config_json: &str,
    engine_path: &Path,
    engine_name: &str,
    args: &[String],
) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = vec![
        "run".into(),
        "--allow-all".into(),
        bundle.as_os_str().to_os_string(),
        "call-engine".into(),
        config_json.into(),
        engine_path.as_os_str().to_os_string(),
        engine_name.into(),
    ];
    argv.extend(args.iter().map(Into::into));
    argv
}
```

and the trait override inside `impl ExecutionEngine for TsEngine` (near
`shutdown`/`is_alive`):

```rust
    fn call_engine_command(
        &self,
        args: &[String],
    ) -> Result<CallEngineOutcome, ExecutionError> {
        if !ts_process::is_available() {
            return Err(ExecutionError::Other(
                "Deno is required to run TypeScript engine commands. \
                 Install Deno from https://deno.land/"
                    .to_string(),
            ));
        }
        let bundle = ts_process::extracted_bundle_path()?;
        let config_json = serde_json::to_string(self.host.global_config())
            .map_err(|e| ExecutionError::Other(format!(
                "failed to serialize engine host config: {e}"
            )))?;
        let argv = build_call_engine_argv(
            &bundle, &config_json, &self.engine_path, &self.name, args,
        );
        // Inherited stdio: engine actions own the terminal (raw status/log
        // dumps, cliffy ANSI help). The exit code IS the result.
        let status = std::process::Command::new("deno")
            .args(&argv)
            .status()
            .map_err(|e| ExecutionError::Other(format!(
                "failed to spawn deno for call-engine: {e}"
            )))?;
        Ok(CallEngineOutcome { exit_code: status.code().unwrap_or(1) })
    }
```
(Adjust `ExecutionError::Other` construction to the variant's actual shape in
`error.rs` — it exists per the research; if it is `Other(String)` this compiles
as written.)

- [ ] **6.4** `cargo nextest run -p quarto-core -E 'test(build_call_engine_argv)'`
  → PASS. CE3 revert proof: delete the `"call-engine".into(),` line → RED →
  restore → GREEN; record.

- [ ] **6.5** `cargo build --workspace` clean, then **commit** path-scoped.

## Phase 3 — CLI arm (CE1, CE4)

### Task 7: `q2 call engine` dispatch

**Files:** create `crates/quarto/src/commands/call/engine.rs` and
`crates/quarto/tests/fixtures/call-engine-oracle/`; modify
`crates/quarto/src/commands/call/mod.rs`.

**Interfaces — Consumes:** `ProjectContext::discover(path, runtime)`
(`get_config.rs:69–73` pattern), `project.registry: Arc<EngineRegistry>`,
`registry.get(name)`, `engine.call_engine_command(&args)`, `CallEngineOutcome`,
`ExecutionError::NotSupported`. **Produces:** `pub fn execute(args: Vec<String>) -> Result<()>`
(may `std::process::exit`); pure helpers `available_engines_list`,
`unknown_engine_message`, `no_subcommands_message`, `MISSING_ENGINE_NAME`.

- [ ] **7.1 Create the oracle fixtures dir** (committed byte oracles; provenance
  README):

```bash
mkdir -p crates/quarto/tests/fixtures/call-engine-oracle
for f in call-engine-julia-help.txt julia-unknown-subcmd.txt unknown.err.txt \
         nosupport.err.txt status-nodaemon.err.txt julia-close-help.txt; do
  cp claude-notes/research/2026-07-03-plan9-q1-observed/$f \
     crates/quarto/tests/fixtures/call-engine-oracle/$f
done
cat > crates/quarto/tests/fixtures/call-engine-oracle/README.md <<'EOF'
Byte-parity oracles for `q2 call engine` (Plan 9, bd-m1jeqhhz), copied verbatim
from claude-notes/research/2026-07-03-plan9-q1-observed/ (captured 2026-07-03
from Q1 @ e768e5c2d). NEVER regenerate from q2 output — these are the spec.
EOF
```
Note: `julia-unknown-subcmd.txt` from the corpus contains a trailing
`exit=2` line appended at capture time — strip it into a sibling `.exit`
expectation instead (`tail -1` check, then `sed -i '' '$d'`) so the oracle is
pure output bytes. Verify with `tail -1`.

- [ ] **7.2 Write the failing unit tests** (CE1, CE4) in `engine.rs`'s
  `#[cfg(test)]`:

```rust
#[test]
fn available_engines_list_uses_q1_builtin_order() {
    // real registry: builtins only
    let reg = EngineRegistry::new();
    assert_eq!(available_engines_list(&reg), "knitr, jupyter, markdown");
}

#[test]
fn gate_messages_are_q1_verbatim() {
    assert_eq!(unknown_engine_message("nonexistent"), "Unknown engine: nonexistent");
    assert_eq!(
        no_subcommands_message("markdown"),
        "Engine markdown does not support subcommands"
    );
    assert_eq!(MISSING_ENGINE_NAME, "ERROR: Missing argument(s): engine-name");
}
```
(For the externals-appended case, extend the first test if `EngineRegistry`
exposes a test constructor for registering a dummy engine under `"julia"` —
`registry.rs` has `register`; use it with a `NoCommandsEngine`-style stub named
"julia" and assert `"knitr, jupyter, markdown, julia"`.)

- [ ] **7.3** Run → FAIL (module missing). **Implement `engine.rs`:**

```rust
//! `q2 call engine <engine-name> [args...]` — Q1-parity dispatcher (Plan 9,
//! bd-m1jeqhhz). Q1 reference: src/command/call/engine-cmd.ts; observed
//! contract: claude-notes/research/2026-07-03-plan9-q1-observed/.

use anyhow::{Context, Result};
use quarto_core::engine::{registry::EngineRegistry, ExecutionError};
use quarto_core::project::ProjectContext;
use quarto_core::runtime::{NativeRuntime, SystemRuntime};
use std::sync::Arc;

pub(crate) const MISSING_ENGINE_NAME: &str =
    "ERROR: Missing argument(s): engine-name"; // D-2: first line only, no stack

const BUILTIN_ORDER: [&str; 3] = ["knitr", "jupyter", "markdown"]; // D-6: Q1 order

// Q1 shows this static help for `call engine --help` / `call engine help`
// (corpus call-engine-help.txt / engine-help-subcmd.out.txt), minus the Q1
// log/profile options q2 does not have (deviation D-8) and the Version line.
const ENGINE_HELP: &str = "\
Usage:   q2 call engine <engine-name> [args...]

Description:

  Access functionality specific to quarto's different rendering engines.

Options:

  -h, --help  - Show this help.
";

pub(crate) fn available_engines_list(registry: &EngineRegistry) -> String {
    let mut names: Vec<String> = Vec::new();
    for b in BUILTIN_ORDER {
        if registry.has_engine(b) {
            names.push(b.to_string());
        }
    }
    for name in registry.engines_in_order() {
        if !BUILTIN_ORDER.contains(&name.as_str()) {
            names.push(name.clone());
        }
    }
    names.join(", ")
}

pub(crate) fn unknown_engine_message(name: &str) -> String {
    format!("Unknown engine: {name}")
}

pub(crate) fn no_subcommands_message(name: &str) -> String {
    format!("Engine {name} does not support subcommands")
}

pub fn execute(args: Vec<String>) -> Result<()> {
    let mut it = args.into_iter();
    let engine_name = match it.next() {
        None => {
            eprintln!("{MISSING_ENGINE_NAME}");
            std::process::exit(1);
        }
        Some(h) if h == "--help" || h == "-h" || h == "help" => {
            print!("{ENGINE_HELP}");
            return Ok(());
        }
        Some(name) => name,
    };
    let rest: Vec<String> = it.collect();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let project = ProjectContext::discover(&cwd, runtime.as_ref())
        .context("Failed to discover project context")?;
    let registry = &project.registry;

    let Some(engine) = registry.get(&engine_name) else {
        // Q1 gate 1, verbatim, stderr, exit 1 (engine-cmd.ts:26-33)
        eprintln!("{}", unknown_engine_message(&engine_name));
        eprintln!("Available engines: {}", available_engines_list(registry));
        std::process::exit(1);
    };

    match engine.call_engine_command(&rest) {
        Ok(outcome) => std::process::exit(outcome.exit_code),
        Err(ExecutionError::NotSupported(_)) => {
            // Q1 gate 2 for engines with no command surface (engine-cmd.ts:36-39)
            eprintln!("{}", no_subcommands_message(&engine_name));
            std::process::exit(1);
        }
        Err(e) => Err(e).context(format!(
            "Engine '{engine_name}' command failed"
        )),
    }
}
```
Adjust import paths to the crate's actual re-exports (`quarto_core::engine::…`);
`registry.has_engine` / `engines_in_order()` exist per `registry.rs` (research
§10.3 / earlier survey — `engines_in_order` returns ordered names; if it returns
`Vec<Arc<dyn ExecutionEngine>>`, map through `.name()`).

- [ ] **7.4 Wire the dispatch arm.** Because plan1c3 made `call` a typed
  subcommand group, first add an `Engine` variant to `CallCommands` in `main.rs`
  (mirroring plan1c3's `Test` variant) and route it through the string dispatcher:

```rust
// in enum CallCommands (main.rs), after Test / BuildTsExtension:
    /// Access functionality specific to quarto's different rendering engines
    Engine {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
// in the Commands::Call { command } match arm:
    CallCommands::Engine { args } =>
        commands::call::execute(Some("engine".to_string()), args),
```

Then keep the `commands/call/mod.rs` string arm (now reached via that dispatch)
and update both usage strings:

```rust
mod engine;
mod test;
// in execute():
        Some("engine") => engine::execute(args),
```
with `Available functions:` blocks becoming:

```text
Available functions:
  engine    Access functionality specific to quarto's different rendering engines
  test      Run embedded document tests
```

- [ ] **7.5** `cargo nextest run -p quarto -E 'test(available_engines_list) | test(gate_messages)'`
  → PASS. CE1 revert proof: replace the BUILTIN_ORDER loop with plain
  `engines_in_order()` emission → RED → restore → GREEN; record. CE4 revert:
  wording change → RED → restore.

- [ ] **7.6 Manual end-to-end smoke** (before the e2e suite exists):

```bash
cd crates/quarto-core/tests/fixtures/extensions/julia-engine
cargo run --bin q2 -- call engine julia --help
cargo run --bin q2 -- call engine nonexistent foo; echo "exit=$?"
cargo run --bin q2 -- call engine markdown x; echo "exit=$?"
```
Expected: julia five-command help / `Unknown engine…` + `Available engines:
knitr, jupyter, markdown, julia`, exit 1 / `Engine markdown does not support
subcommands`, exit 1. Inspect output against the corpus; record in strand
comment.

- [ ] **7.7 Commit** path-scoped (`engine.rs`, `mod.rs`, oracle fixtures dir).

## Phase 4 — Binary e2e (CE10–CE14)

### Task 8: `call_engine_e2e.rs`

**Files:** create `crates/quarto/tests/integration/call_engine_e2e.rs`; modify
`crates/quarto/tests/integration/main.rs` (add `pub mod call_engine_e2e;`,
alphabetized).

**Interfaces — Consumes:** `env!("CARGO_BIN_EXE_q2")` (pattern:
`build_ts_extension_e2e.rs:26`), julia/echo fixtures (temp-copied), oracle dir
from Task 7.1, temp-`HOME` isolation (pattern: `julia_engine_e2e.rs`, safe because
nextest is process-per-test).

- [ ] **8.1 Write the tests.** Shared helpers + the CE10–CE13 rows:

```rust
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const Q2: &str = env!("CARGO_BIN_EXE_q2");

fn deno_available() -> bool {
    Command::new("deno").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn oracle(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/call-engine-oracle")
            .join(name),
    )
    .unwrap()
}

/// Copy the julia-engine extension into a fresh temp project dir.
fn setup_julia_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quarto-core/tests/fixtures/extensions/julia-engine/_extensions");
    copy_dir(&src, &dir.path().join("_extensions"));
    dir
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() { copy_dir(&e.path(), &to); }
        else { std::fs::copy(e.path(), &to).unwrap(); }
    }
}

/// Run q2 with isolated HOME (fresh runtime dir ⇒ no ambient julia daemon)
/// and NO_COLOR scrubbed (byte-oracle parity).
fn run_q2(project: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(Q2)
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env_remove("NO_COLOR")
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("XDG_DATA_HOME")
        .output()
        .unwrap()
}

fn s(bytes: &[u8]) -> String { String::from_utf8_lossy(bytes).into_owned() }

#[test]
fn ce10_julia_help_byte_parity() {
    if !deno_available() { eprintln!("SKIP: deno not on PATH — ce10"); return; }
    let proj = setup_julia_project();
    let home = tempfile::tempdir().unwrap();
    let out = run_q2(proj.path(), home.path(), &["call", "engine", "julia", "--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(s(&out.stdout), oracle("call-engine-julia-help.txt"));
}

#[test]
fn ce11_gate_messages_byte_parity() {
    if !deno_available() { eprintln!("SKIP: deno not on PATH — ce11"); return; }
    let proj = setup_julia_project();
    let home = tempfile::tempdir().unwrap();
    // (a) unknown engine, 4-engine list
    let out = run_q2(proj.path(), home.path(), &["call", "engine", "nonexistent", "foo"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(s(&out.stderr), oracle("unknown.err.txt"));
    assert!(out.stdout.is_empty());
    // (a2) builtins-only list outside any extension project
    let bare = tempfile::tempdir().unwrap();
    let out = run_q2(bare.path(), home.path(), &["call", "engine", "nonexistent"]);
    assert!(s(&out.stderr).contains("Available engines: knitr, jupyter, markdown"));
    // (b) engine without commands (native default path — no deno involved)
    let out = run_q2(proj.path(), home.path(), &["call", "engine", "markdown", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(s(&out.stderr), oracle("nosupport.err.txt"));
    // (c) missing engine-name (D-2: first line parity, no stack)
    let out = run_q2(proj.path(), home.path(), &["call", "engine"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        s(&out.stderr).lines().next().unwrap(),
        "ERROR: Missing argument(s): engine-name"
    );
    // (d) static engine help, both spellings (Q1: cliffy help / help subcommand)
    for help_arg in ["--help", "help"] {
        let out = run_q2(proj.path(), home.path(), &["call", "engine", help_arg]);
        assert_eq!(out.status.code(), Some(0));
        assert!(s(&out.stdout).starts_with("Usage:   q2 call engine <engine-name> [args...]"));
        assert!(s(&out.stdout).contains("Access functionality specific to quarto's different rendering engines."));
    }
}

#[test]
fn ce12_exit_code_propagation() {
    if !deno_available() { eprintln!("SKIP: deno not on PATH — ce12"); return; }
    let proj = setup_julia_project();
    let home = tempfile::tempdir().unwrap();
    let out = run_q2(proj.path(), home.path(), &["call", "engine", "julia", "frobnicate"]);
    assert_eq!(out.status.code(), Some(2)); // cliffy's unknown-command code, untouched
}

#[test]
fn ce13_status_no_daemon_isolated_home() {
    if !deno_available() { eprintln!("SKIP: deno not on PATH — ce13"); return; }
    let proj = setup_julia_project();
    let home = tempfile::tempdir().unwrap();
    let out = run_q2(proj.path(), home.path(), &["call", "engine", "julia", "status"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(s(&out.stderr).contains("Julia control server is not running."));
    // path-exercised proof: the isolated HOME's quarto runtime dir gained julia/
    let rt = home.path().join("Library/Caches/quarto/julia"); // macOS layout
    #[cfg(target_os = "macos")]
    assert!(rt.is_dir(), "expected {rt:?} to be created by the status action");
}
```
Implementer notes: on Linux the runtime dir derives from XDG vars (both scrubbed
above, so `$HOME/.local/share/quarto/julia`) — gate the dir assertion per-OS as
shown for macOS and add the Linux path under `#[cfg(target_os = "linux")]`. If
`unknown.err.txt` ends without a trailing newline (captured via
`console.error`), compare with the exact bytes — do not `trim`.

- [ ] **8.2** Register in `main.rs`, run:

```bash
cargo nextest run -p quarto -E 'binary(integration) & test(call_engine_e2e)'
```
Expected: 4 passed (or SKIP lines without deno).

- [ ] **8.3 Fail-on-revert pass** for CE10 (comment out the TsEngine override →
  rebuild → RED → restore), CE11(b) (route NotSupported into `?` → RED → restore),
  CE12 (`exit(0)` → RED → restore), CE13 (empty `runtime_dir` config → RED →
  restore). Record each RED verbatim in doc comments.

- [ ] **8.4 Commit** path-scoped.

### Task 9: opt-in live-daemon test (CE14)

**Files:** append to `crates/quarto/tests/integration/call_engine_e2e.rs`.

- [ ] **9.1** Add the env-gated test (skips unless `QUARTO_E2E_JULIA_DAEMON=1` AND
  deno+julia available). Flow: temp HOME; copy `minimal.qmd` from the julia
  fixture; `q2 render minimal.qmd` (starts the detached server under the isolated
  HOME); `q2 call engine julia status` → stdout starts
  `QuartoNotebookRunner server status:`; `q2 call engine julia stop` → stderr
  contains `Server stopped.`; transport file
  `<home>/Library/Caches/quarto/julia/julia_transport.txt` (macOS) gone within a
  5 s poll. **Worker-leak guard (bd-l9jhy5u0):** count
  `pgrep -f QuartoNotebookRunner` before setup and after stop; assert no net
  increase; if stop fails, fall back to `q2 call engine julia kill` in test
  cleanup so nothing leaks. Reuse `run_q2`; do not touch the real HOME's
  transport file (assert its mtime/existence unchanged, sentinel-style, per
  `julia_engine_e2e.rs:150–195`).
- [ ] **9.2** Run it once locally with the env var set; paste the observed
  status/stop transcript into the test's doc comment (end-to-end verification
  record). Run count of stray processes before/after — must be equal.
- [ ] **9.3 Commit** path-scoped.

## Phase 5 — Verification & wrap-up

### Task 10: full verification + bookkeeping

- [ ] **10.1** `cargo xtask lint` clean.
- [ ] **10.2** Full verify (bundle + ts-packages changed ⇒ no skip flags):
  `cargo xtask verify 2>&1 | tee /tmp/plan9-verify.log`; inspect the tail +
  grep for failures. All legs green.
- [ ] **10.3** Confirm the CI freshness gate will pass: rerun
  `cargo xtask build-engine-host-bundle` and `git diff --exit-code -- ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js`.
- [ ] **10.4** Reconcile THIS plan's checkboxes against reality (repo rule:
  verify each `[x]` actually landed); commit the updated plan.
- [ ] **10.5** Point `claude-notes/plans/CURRENT.md` at this file (worktree
  branch) and update `CLAUDE.local.md`'s Plan line; `braid comment bd-m1jeqhhz`
  with the plan path + e2e evidence snippets. Do NOT close the strand until all
  seams are GREEN and reverts recorded.
- [ ] **10.6** Stop. Merging to `feature/ts-engine-extensions` and any push wait
  for Gordon's explicit approval (epic branch is unpushed pending cumulative
  review).

## Q1-parity acceptance criteria (summary)

1. CE6/CE10: `q2 call engine julia --help` **byte-equal** to Q1 (oracle).
2. CE11: gate messages byte-equal on stderr, exit 1; missing-name first line
   equal (D-2).
3. CE7/CE12: unknown subcommand renders cliffy help + did-you-mean, exit 2.
4. CE8: bare engine invocation silent, exit 0 (D-3).
5. CE9/CE13: `status` with no daemon: stderr info line, exit 0, correct runtime
   dir consulted.
6. CE14 (opt-in): live `status`/`stop` round-trip matches Q1 output shapes.
7. Native engines (`markdown`, `knitr`, `jupyter`) and TS engines without
   `populateCommand` (`echo`) produce Q1's exact no-subcommands error.
