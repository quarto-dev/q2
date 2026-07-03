# Plan 10 research — engine `checkInstallation` in q2 (`q2 check` integration)

**Strand:** bd-4qflzhwh (P3, feature). Related: bd-m1jeqhhz (populateCommand — sibling
session, separate worktree).
**Epic:** ts-engine-extensions (`feature/ts-engine-extensions`).
**Status:** research in progress (2026-07-03).

## Binding constraint (Gordon, 2026-07-03)

**Q1 is the spec.** The goal is IDENTICAL user-visible behavior to Quarto 1:

- Engine check semantics — when `checkInstallation` fires, what it verifies per
  engine, what success/failure looks like — must match Q1 exactly.
- The engine section of `q2 check`'s report should mirror Q1's `quarto check`
  structure and wording as closely as q2's existing report format allows.
- Where `q2 check`'s current output already diverges from Q1's overall report,
  do **not** invent new UX to bridge it — document the divergence as an explicit,
  **numbered decision point** for Gordon. Deviations are ratified, never silent.
- Part 2 (Rust-engine trait method) sits beneath this surface and must not
  change the user-facing contract.

## Overview

`checkInstallation` is an optional method on the TS `ExecutionEngine` interface
that Q1's `quarto check` invokes. In q2 it is **inert**: the `ToEngine` wire enum
(`crates/quarto-core/src/engine/ts_protocol.rs:33-101`) has 11 variants and no
check-installation; the only traces are the optional TS type declaration and two
fixture engines that implement stubs. This research establishes (Part 1) exactly
how Q1's `checkInstallation` + `quarto check` behave, corner by corner, and what
q2's existing `q2 check` and TS-engine protocol look like; and (Part 2) how an
optional native counterpart on the Rust `ExecutionEngine` trait should reconcile
with q2's existing runtime-availability checks.

## Findings so far (direct reading)

### TS type declaration (q2, vendored parity types)

`ts-packages/quarto-types/src/execution-engine.ts:209`:

```ts
/**
 * Check installation and capabilities for this engine (optional)
 * Used by `quarto check <engine-name>` command
 *
 * Engines implementing this method will automatically be available as targets
 * for the check command (e.g., `quarto check jupyter`, `quarto check knitr`).
 *
 * @param conf - Check configuration with output settings and services
 */
checkInstallation?: (conf: CheckConfiguration) => Promise<void>;
```

It lives on the engine **discovery** object (module level, like `populateCommand`
and `quartoRequired`), not the launched instance — so invoking it does not require
`launchEngine`.

`CheckConfiguration` is already vendored at `ts-packages/quarto-types/src/check.ts`
("parity: vendored from external-sources/quarto-cli/packages/quarto-types"):

```ts
export interface CheckConfiguration {
  strict: boolean;                                  // whether to run strict checks
  target: string;                                   // e.g. "jupyter", "knitr", "all"
  output: string | undefined;                       // optional JSON results path
  services: CheckRenderServiceWithLifetime;         // temp-file mgmt + cleanup()
  jsonResult: Record<string, unknown> | undefined;  // mutated in place when JSON output
}
```

**Wire-design wrinkle:** `services` (functions, temp context) and the in-place
mutable `jsonResult` are not wire-serializable. Q1 passes this object by reference
in-process. Over q2's wire protocol, the host (ts-packages/quarto-api side) must
synthesize `services` locally, and any `jsonResult` contribution has to come back
as response payload rather than by mutation.

### Fixture engine implementations (both stubs)

- **marimo** (`~/src/quarto-marimo`, branch `q2-bare-sql-interop`,
  `src/marimo-engine.ts:210`; mirrored in
  `crates/quarto-core/tests/fixtures/extensions/marimo/src/marimo-engine.ts:210`):
  `quarto.console.withSpinner({message: "Checking Marimo installation..."},
  async () => { await delay(2000); })` — checks **nothing**, returns void.
  Confirms the plan-4c finding (migration guide §4, compat doc §16).
- **julia fixture** (`crates/quarto-core/tests/fixtures/extensions/julia-engine/src/julia-engine.ts:127`):
  same shape — spinner "Checking Julia installation...", `delay(3000)`, checks
  nothing. (Contrary to the strand's "julia's may do real checks" hedge — it does
  not. Both fixtures exercise only the invocation path, not real verification.)

So the two available fixture engines give: two no-op stubs with different spinner
messages/delays. Real-check depth will have to come from Q1's built-in engine
implementations (jupyter/knitr) if we port any of them (Part 2), or from a fixture
extended for the plan's test seams.

### Prior record (plan-4c)

- Migration guide §4 (`claude-notes/research/2026-07-03-marimo-migration-guide.md`):
  `checkInstallation` inert in q2; `ToEngine` has exactly 11 variants (`init`,
  `loadEngine`, `launchEngine`, `shutdown`, `claimsLanguage`, `claimsFile`,
  `markdownForFile`, `execute`, `intermediateFiles`, `dependencies`, `cancel`);
  zero occurrences of `checkInstallation`/`check_installation` in
  `ts_protocol.rs`/`ts_engine.rs`/`ts_process.rs`.
- Compat doc §16 (`claude-notes/research/2026-07-02-marimo-engine-q2-compat.md:915`):
  same finding, disposition "inert in q2, no call site exists".

## Part 1a — Q1 `checkInstallation` contract + call sites

All paths under `external-sources/quarto-cli` (READ-ONLY reference).

### Interface declaration

`checkInstallation` lives on **`ExecutionEngineDiscovery`** (the static discovery
interface, not the launched instance) at `src/execute/types.ts:81`:

```ts
checkInstallation?: (conf: CheckConfiguration) => Promise<void>;
```

- Optional (`?`), returns `Promise<void>` — no success/failure return value;
  engines print their own output (or write `conf.jsonResult`) and **throw** on
  hard failure.
- Doc comment (types.ts:72-80) states the load-bearing contract: *"Engines
  implementing this method will automatically be available as targets for the
  check command (e.g., `quarto check jupyter`, `quarto check knitr`)."*
- `CheckConfiguration` is imported from the check command module
  (`src/command/check/check.ts:55-61`): `{ strict: boolean; target: Target;
  output: string | undefined; services: RenderServiceWithLifetime;
  jsonResult: CheckJsonResult | undefined }` where `CheckJsonResult =
  Record<string, unknown>`. When `jsonResult` is defined (i.e. `--output` was
  given), engines suppress console output and populate the object instead.
- The only other check-adjacent discovery members are `quartoRequired` (semver
  gate enforced at registration, `engine.ts:62-81`) and `populateCommand`
  (bd-m1jeqhhz's subject). `ExecutionEngineInstance.dependencies()` is
  render-time dependency resolution, unrelated.

### Call sites — exactly two, both in `quarto check`

**A. Target enumeration** — `src/command/check/check.ts:37-43`:

```ts
export function getTargets(): readonly string[] {
  const checkableEngineNames = executionEngines()
    .filter((engine) => engine.checkInstallation)
    .map((engine) => engine.name);
  return ["install", "info", ...checkableEngineNames, "versions", "all"];
}
```

Fires during argument validation (`enforceTargetType`, check.ts:46-49, from
cmd.ts:35). Engines without `checkInstallation` never become valid
`quarto check <name>` targets.

**B. Invocation** — `src/command/check/check.ts:112-119`, inside `check()`:

```ts
// Dynamic engine checks
for (const engine of executionEngines()) {
  if (engine.checkInstallation && (target === engine.name || target === "all")) {
    await engine.checkInstallation(conf);
  }
}
```

- Runs **after** the fixed non-engine checks (info/versions/install). Fires for
  `target === engine.name` or `target === "all"` (default).
- Optionality: guarded — non-implementing engines are **silently skipped**, no
  "not supported" message.
- Failure: a thrown error propagates out of `check()` (only a
  `finally { services.cleanup() }` wraps the loop) to the top-level CLI handler →
  non-zero exit. In JSON mode engines record errors into `jsonResult` instead of
  throwing, so `quarto check --output …` generally exits 0 with failures as
  `error` fields.
- There is **no other call site anywhere in Q1** — not render preflight, not
  engine selection, not install tooling. `checkInstallation` is a
  `quarto check`-only capability.

### Engine registry / which engines get checked

- `quarto check` enumerates the live registry `executionEngines()` =
  `[...kEngines.values()]` (`src/execute/engine.ts:83-85`), **not** a hard-coded
  list. Order = insertion order: standard engines `knitr, jupyter, markdown`
  first (`engine.ts:49-53,213-220`), then extension engines.
- Registry population: `initializeProjectContextAndEngines()`
  (`src/command/command-utils.ts:59-72`) runs at the top of the check action —
  builds a `projectContext` (or `zeroFileProjectContext` fallback, which exists
  specifically *"to load bundled engine extensions (like Julia)"*), then
  `resolveEngines(context)` dynamically imports `project.config.engines` entries
  (extension-contributed engines flow in via `mergeProjectEngines`,
  `src/project/project-context.ts:764-792`).
- **So extension engines' `checkInstallation` IS invoked by `quarto check`** —
  provided the extension is discovered/registered for the current context. In Q1
  as shipped: built-in jupyter + knitr always; bundled julia extension when
  loaded; marimo only if installed as a contributing extension. `markdown` does
  not implement it (never a target).

## Part 1c — Q1 `quarto check` report structure

Command: `src/command/check/cmd.ts:11-42`. Signature:
`quarto check [target] --output <path> --no-strict`.

- Description: "Verify correct functioning of Quarto installation.\n\nCheck
  specific functionality with argument install, jupyter, knitr, or all."
- Accepted targets (`getTargets()`): `install`, `info`, `<checkable engine
  names…>`, `versions`, `all`. Default `all`. Unknown target errors via
  `enforceTargetType` — validated *after* engine registration so extension
  engine names validate.
- `--output <path>`: quiet JSON mode — console helpers `checkCompleteMessage` /
  `checkInfoMsg` (check.ts:63-73) no-op when `conf.jsonResult` is set; result
  written as pretty JSON at check.ts:121-126.
- `--no-strict`: version checks use `>=` constraints instead of exact-match.

Report structure in order (`check()`, check.ts:75-130):

1. **Version banner** — `Quarto <version>` (check.ts:97).
2. **Fixed checks** (each gated on `target === name || target === "all"`), in
   this literal order:
   - `info` → "Checking environment information..." + `Quarto cache location:`.
   - `versions` → "Checking versions of quarto binary dependencies...": per-dep
     `<name> version <v>: OK` / NOTE lines. Hard-coded deps+pins at
     check.ts:246-251 (Pandoc 3.8.3, Dart Sass 1.87.0, Deno 2.4.5, Typst
     0.14.2). Then "Checking versions of quarto dependencies......OK".
   - `install` → subsections: "Checking Quarto installation......OK" (Version/
     Path/dev commit); Windows-only CodePage block; "Checking tools....................";
     "Checking LaTeX...................."; "Checking Chrome Headless....................";
     "Checking basic markdown render...." (renders a minimal doc; throws on
     failure in non-JSON mode).
3. **Dynamic engine checks** — the §1a call-site B loop; jupyter/knitr/julia
   sections appear here, after the fixed checks, in registry order.
4. **JSON flush + cleanup**.

Exit codes: no explicit exit-code aggregation anywhere in check.ts — errors
throw and bubble (non-zero via the CLI runner); JSON mode captures errors and
exits 0. Success output style is spinner-driven (`withSpinner`) with
dot-padded `Checking X....` + `OK` completions.

## Part 1b — Q1 per-engine implementations

Complete inventory of `ExecutionEngineDiscovery` objects in Q1:

| Engine | File | `checkInstallation`? |
|---|---|---|
| knitr | `src/execute/rmd.ts:52` | Yes — real checks (`rmd.ts:89-222`) |
| jupyter | `src/execute/jupyter/jupyter.ts:77` | Yes — real checks (`jupyter.ts:124-246`) |
| markdown | `src/execute/markdown.ts:30` | **No** (never a check target) |
| julia (bundled extension) | `src/resources/extension-subtrees/julia-engine/src/julia-engine.ts:75` | Yes — **stub** (`:119-127`, spinner + `delay(3000)`, checks nothing, ignores `conf`) |

(OJS is a cell handler, not an engine; the `create extension` engine template is
scaffolding, not live.)

### jupyter (`jupyter.ts:124-246`) — decision tree

1. Header `"Checking Python 3 installation...."`; probes
   `quarto.jupyter.capabilities()` under a spinner (JSON mode: no spinner).
2. **No Python 3** → `kMessage + "(None)\n"` + `pythonInstallationMessage`:
   "Unable to locate an installed version of Python 3. / Install Python 3 from
   https://www.python.org/downloads/". JSON: `installed: false`,
   `how-to-install-python`.
3. **Python found** → `kMessage + "OK"` + `jupyterCapabilitiesMessage(caps, kIndent)`
   (`src/core/jupyter/jupyter-shared.ts:45-62`): `Version: <maj>.<min>.<patch>
   [(Conda)]` / `Path: <executable>` / `Jupyter: <jupyter_core | (None)>` /
   `Kernels: <comma-separated names>` (when jupyter present).
   - **`jupyter_core` missing** → `installationMessage` ("Jupyter is not
     available in this Python installation." + pip/conda install line) + optional
     `unactivatedEnvMessage` ("Did you forget to activate it?" — scans cwd for
     pyvenv.cfg/conda-meta/requirements.txt).
   - **jupyter present + python kernelspec found** → real test render:
     `"Checking Jupyter engine render...."` → `OK\n`, rendering a minimal doc
     with a ```` ```{python} 1 + 1 ```` cell via `quarto.system.checkRender`.
     On render error: throws (non-JSON) / records `render.jupyter.error` (JSON).
   - **no python kernelspec** → `kIndent + "NOTE: No Jupyter kernel for Python found"`.

Probe mechanics: `jupyterCapabilities` (`src/core/jupyter/capabilities.ts:24-103`)
runs `<python> resources/capabilities/jupyter.py` (emits YAML: version fields,
conda flag, exec paths, then versions of `jupyter_core`, `nbformat`, `nbclient`,
`ipykernel`, `shiny`); rejects Python < 3; python resolution order
`QUARTO_PYTHON` → windows py-launcher → conda python → python3. **Cached**
per-language in a module-level map. Kernelspecs via `jupyter --paths --json`
(`kernels.ts:79-134`), also module-cached. **No process timeouts anywhere.**

### knitr (`rmd.ts:89-222`) — decision tree

1. Header `"Checking R installation..........."`; probes `checkRBinary()`
   (resolves `Rscript`, runs `Rscript --version`) then `knitrCapabilities(rBin)`
   (runs `Rscript resources/capabilities/knitr.R`, which prints YAML between
   `--- YAML_START/END ---` markers: R version, `R.home()`, `.libPaths()`, and
   knitr/rmarkdown package versions or null).
2. **`rBin` undefined** → `kMessage + "(None)\n"` + `rInstallationMessage`:
   "Unable to locate an installed version of R. / Install R from
   https://cloud.r-project.org/".
3. **R found, caps undefined** → `(None)` + three lines: `R succesfully found at
   <rBin>.` (typo verbatim in Q1) / "However, a problem was encountered when
   checking configurations of packages." / "Please check your installation of R."
   Special case: Windows-ARM x64-R exit codes throw `WindowsArmX64RError`.
4. **R + caps** → `OK` + `knitrCapabilitiesMessage` (`src/core/knitr.ts:169-191`):
   `Version:` / `Path: <R.home()>` / `LibPaths:` list / `knitr: <ver|(None)>` /
   `rmarkdown: <ver|(None)>` with `NOTE: … too old` lines. Version gates
   (`knitr.ts:32-41`): knitr >= 1.30, rmarkdown >= 2.3.
   - both version-OK → real test render `"Checking Knitr engine render......"` →
     `OK\n` with a ```` ```{r} 1 + 1 ```` doc.
   - else → `knitrInstallationMessage` per missing/outdated package: "The <pkg>
     package is not available in this R installation." + `Install with
     install.packages("<pkg>")` (or update variant).

No caching for the R probes. No timeouts.

### Shared presentation conventions

- Dot-padded headers align the `OK` column: `Checking Python 3 installation....`,
  `Checking R installation...........`, `Checking Jupyter engine render....`,
  `Checking Knitr engine render......`.
- Detail lines indented with `kIndent = "      "` (6 spaces).
- Spinner while probing (`withSpinner`, `src/core/console.ts:76-86`), completion
  via `completeMessage` (green check + message, `console.ts:147-156`).
- JSON mode (`conf.jsonResult` set): all console output suppressed; results land
  at `jsonResult.tools.<engine>` and `jsonResult.render.<engine>`.
- Shared render probe: `checkRender` (`src/command/check/check-render.ts:39-61`)
  — writes a temp `.qmd`, calls `render(tempFile, { services, flags: { quiet:
  true, executeDaemon: 0 } })`, returns `{ success, error }`. Exposed to engines
  as `quarto.system.checkRender` (`src/core/api/system.ts:25`).

## Part 1c — Q1 `quarto check` report structure

*(pending — agent report)*

## Part 1d — q2 `q2 check` today

**`q2 check` is a stub — it performs zero checks today.**

- Clap definition: `crates/quarto/src/main.rs:409-413` — doc-comment help "Verify
  correct functioning of Quarto installation", one optional positional
  `target: Option<String>` ("Target to check"). No flags (no `--output`/JSON, no
  `--quiet`).
- Dispatch: `crates/quarto/src/main.rs:744` — `Commands::Check { .. } =>
  commands::check::execute()`. The `target` argument is **destructured away and
  ignored**; `execute()` takes no parameters.
- Handler: `crates/quarto/src/commands/check.rs` (entire file, 9 lines) —
  unconditionally returns `Err(QuartoError::NotImplemented("check"))`
  (`crates/quarto-core/src/error.rs:49-52`), which propagates out of
  `main() -> Result<()>`: prints `Error: Command not yet implemented: check` to
  stderr, exit code 1.
- Tests: none. Docs: none (no docs/ page mentions the command).
- The command touches no quarto-core check infrastructure (none exists) and knows
  nothing about extensions or TS engines.

**Consequence for the parity constraint:** there is no existing `q2 check` output
to preserve or diverge from. Q1's `quarto check` report is the only spec for the
command's output. The open scope question (a decision point): does Plan 10
implement Q1's *full* report (version/deps/pandoc/etc. sections) or only the
engine-check portion plus enough scaffolding to host it? (See Decision points.)

## Part 1e — q2 TS-engine wire protocol + host + precedent (plan-1a pattern)

### Wire protocol (`crates/quarto-core/src/engine/ts_protocol.rs`)

- Conventions: internally tagged enums (`#[serde(tag = "type",
  rename_all_fields = "camelCase")]`), explicit per-variant
  `#[serde(rename = "camelCaseTag")]`; every payload fully typed (no
  `serde_json::Value` escape hatches, per plan-1a).
- `ToEngine` (line 31): 11 variants — `init`, `loadEngine`, `launchEngine`,
  `shutdown`, `claimsLanguage`, `claimsFile`, `markdownForFile`, `execute`,
  `intermediateFiles`, `dependencies`, `cancel`. Grouped by tier: lifecycle,
  **discovery (needs LoadEngine only)** = claimsLanguage/claimsFile, **instance
  (needs LaunchEngine)** = markdownForFile/execute/intermediateFiles/dependencies.
- `FromEngine` (line 105): 10 variants incl. `error { message, stack }` — the
  error envelope; correlated by id, converted to
  `ExecutionError::execution_failed` on the Rust side (ts_process.rs:720).
- Envelope (line 164): `Request { id, msg }` / `Response { id, msg }` — tag
  nested under `msg`, deliberately not flattened.
- `LoadEngineResult` (line 185): `name`, `valid_extensions`,
  `generates_figures`, `can_freeze`, `quarto_required`. **No
  `has_check_installation` capability flag** — q2 currently cannot reproduce
  Q1's `getTargets()` filter ("only engines implementing checkInstallation are
  targets") without a new flag or a probe call.

### Rust dispatch (`ts_engine.rs` / `ts_process.rs`)

- **One persistent shared Deno host per project registration**
  (`project/mod.rs:649`), created lazily only when an external engine exists;
  all TS engines share it. Deno child spawned on first request
  (`ts_process.rs:510`); `deno --version` availability probe
  (ts_process.rs:151). Two-step lazy lifecycle: `LoadEngine` (import +
  discovery, cheap, cached in a `OnceLock<LoadEngineResult>`) then
  `LaunchEngine`; expensive work deferred to first `execute`.
- Transport core `TsEngineHost::request` (ts_process.rs:637-741): monotonic id,
  pending-slot registration before send, timeout window → fire-and-forget
  `Cancel` + `ExecutionError::timeout`, cancellation support; caller matches the
  single expected `FromEngine::XResult` arm, `other =>
  ExecutionError::other("unexpected response …")`.
- Uniform verb pattern (e.g. `intermediate_files` ts_engine.rs:800-833, 10s
  window, soft-fail to warning + empty vec; `execute` ts_engine.rs:765-798,
  caller-set timeout, poisons instance on timeout/cancel).
- Registration: TS engines come from extension `contributes.engines`
  (`extension/read.rs:352`, `EngineContribution` types.rs:94-126; only
  pre-built `.js` bundles); `project/mod.rs:662-714` constructs `TsEngine` and
  `registry.register()`s it. `EngineRegistry` (registry.rs) is the enumeration
  source for a `q2 check` loop (`engines_in_order()`, registry.rs:159).

### TS host dispatch (`ts-packages/quarto-engine-host-deno/src/host.ts`)

- Frame loop → `switch (msg.type)` (host.ts:316); unknown tag → thrown error →
  `FromEngine::Error` under the request id (host.ts:906).
- **Optional-method precedent** — `intermediateFiles` (host.ts:541-561):
  optional-chain the method (`instance.intermediateFiles?.(msg.input) ?? null`)
  and let Rust treat `None` as "engine doesn't implement this".
- **Tier subtlety:** `checkInstallation` lives on the *discovery* object, so the
  host lookup is `engineByName.get(name)` → `discovery.checkInstallation?.(conf)`
  (like `claimsLanguage`, host.ts:470), **not** the `launchedByName` path.
- `quarto-api.ts:716-722` already exposes `checkRender(options)` ("Used by
  checkInstallation implementations to verify engines work") — the test-render
  side-channel engines need is already on the host API surface.

### plan-1a migration pattern (the epic's recipe)

From `claude-notes/plans/2026-04-16-plan1a-{protocol,host,engine}.md`:

1. Decide the tier (discovery vs instance) — `checkInstallation` is discovery.
2. Add the wire verb pair in `ts_protocol.rs` with typed payload structs
   (a Rust mirror of the config type; every field typed).
3. Round-trip tests in the `ts_protocol.rs` test module (tag test per variant +
   camelCase guards + full round trip; 52 existing tests to pattern-match).
4. Rust trait method using q2-native types (never `Ts*`); `TsEngine` impl =
   `ensure_loaded` → build `ToEngine::X` → `host.request(msg, window, &c)` →
   match `FromEngine::XResult`. Pick timeout + error policy (soft-warn vs
   hard-fail).
5. Host dispatch case with the `?.` optional-method pattern.
6. TS interface/config types already exist as vendored parity — consume them;
   update the coverage table.

### Fixture engines (`crates/quarto-core/tests/fixtures/extensions/`)

| Fixture | Built bundle? | `checkInstallation`? |
|---|---|---|
| julia-engine (`"julia"`) | yes | yes — stub (spinner + 3s delay) |
| marimo (`"marimo"`) | yes | yes — stub (spinner + 2s delay) |
| echo-engine (`"echo"`) | src-only | no (negative case) |
| echo-legacy (`"echolegacy"`) | src-only | no (negative case) |

Both stubs sit on the discovery object, ignore `conf`, and call
`quarto.console.withSpinner` — they exercise the invocation path + console API,
not real verification. echo engines are the "method absent → silently skipped /
not a target" cases.

## Part 2 — Rust ExecutionEngine trait + existing runtime-availability checks

### The trait (`crates/quarto-core/src/engine/traits.rs:61-217`)

**Sync**, object-safe, `Send + Sync` (used as `Arc<dyn ExecutionEngine>`; the
async-`?Send` convention applies to pipeline stages, not this trait). Required:
`name()`, `execute()`. Everything else has defaults. Closest existing analog to
a check capability:

```rust
/// Check if this engine is available in the current environment.
/// This checks whether the required runtime (R, Python, etc.)
/// is installed and accessible.
/// Default: `true` (assume available)
fn is_available(&self) -> bool { true }
```

Production implementors: `MarkdownEngine` (markdown.rs:44), `KnitrEngine`
(knitr/mod.rs:137, native-only), `JupyterEngine` (jupyter/mod.rs:145,
native-only), `TsEngine` (ts_engine.rs:605), plus test `FixtureEngine`/
`ReplayEngine`. `EngineRegistry::new()` always registers markdown; knitr +
jupyter under `#[cfg(not(target_arch = "wasm32"))]`.

### Existing runtime-availability probes (what a native check should reuse)

- **knitr**: `find_rscript()` (knitr/subprocess.rs:100-109) — `OnceLock`-cached
  PATH walk (`QUARTO_R` env → `which`), no subprocess. Runs at engine
  construction; `is_available()` = `rscript_path.is_some()`. Render-time gate:
  `execute()` returns `ExecutionError::runtime_not_found("knitr", "Rscript
  (install R from https://www.r-project.org/)")` when absent. Reusable helpers:
  `call_r()`/`CallROptions` (JSON-over-stdin protocol — could run a
  capabilities probe script à la Q1's `knitr.R`), `determine_working_dir()`
  (renv-aware), `parse_r_error`.
- **jupyter**: `find_jupyter()` (jupyter/mod.rs:115-131) — `OnceLock`-cached
  `which::which("jupyter")`. `is_available()` = `jupyter_path.is_some()`;
  render-time `runtime_not_found("jupyter", "jupyter")`. Kernel discovery:
  async `list_kernelspecs()` / `find_kernelspec_for_language()`
  (jupyter/kernelspec.rs) via runtimelib + `jupyter --paths --json` — exactly
  the data Q1's `Kernels:` line reports.
- **deno**: `ts_process.rs:151-158` `is_available()` spawns `deno --version`.

### Error family + reporting style

- `ExecutionError` (engine/error.rs): `RuntimeNotFound { engine, runtime }`
  ("Engine runtime not found: {engine} requires {runtime}"),
  `MissingPackage { engine, package, suggestion }`,
  `PackageVersionTooOld { engine, package, required_version, suggestion }` —
  the vocabulary a structured check result can map onto. **Not** wired to Q-*
  catalog codes today.
- **Engine resolution is availability-agnostic**: `resolve_engines()`
  (resolution.rs) never consults `is_available()`; the runtime-present gate is
  deferred to `execute()`. So a check capability duplicates nothing in the
  resolver — it gives the *pre-flight* surface that doesn't exist yet. The only
  soft surface is `EngineRegistry::get_or_default` falling back to markdown for
  *unregistered* names (registry.rs:204-218).
- CLI report styling: `DiagnosticMessageBuilder` (error/warning/info +
  `.with_code("Q-…")`, `.add_info/note/hint`) rendered via `.to_text(None)`
  (tidyverse-style ✖/ℹ/• markers) — the idiom used by `commands/render.rs`.
  The registry also carries an `Arc<Mutex<Vec<DiagnosticMessage>>>` diagnostics
  channel (registry.rs:58).

## Part 2 — Rust ExecutionEngine trait + existing runtime-availability checks

*(pending — agent report)*

## Additional load-bearing facts (direct verification, post-agent)

- **TS-engine console output does not reach the terminal today.** Engine
  `quarto.console.*` → `host.log.*` → Deno host writes `[INFO]`/`[WARN]`/`[ERROR]`
  prefixed lines to its **stderr** (deno-host.ts:58,251) → Rust `stderr_loop`
  (ts_process.rs:1096-1117) routes them into `tracing` (`target: "engine_host"`,
  hidden unless verbose) + a crash-diagnostics ring. For `q2 check`, the engine's
  console output IS the product, so a delivery mechanism is a core design choice.
- **q2's `withSpinner` is already ratified as neutral** (`ts-packages/quarto-api/
  src/console/index.ts:6-11`): no ANSI animation; emits the start message, runs
  the fn, emits a `[✓] <msg>` completion via `completeMessage` (Q1's `\r`-overwrite
  format adapted). So "spinner parity" is already defined as start+done lines.
- **`quarto.system.checkRender` is a stub in q2's host API**
  (`ts-packages/quarto-api/src/system/index.ts:272-280`, "STUB (Plan 2)", throws
  notYetImplemented). Real TS-engine test-render sub-checks would require a
  host→Rust render callback that does not exist. No current TS engine calls it
  (marimo/julia stubs don't).
- Registry iteration order for checkable engines matches Q1's effective order:
  q2 registers markdown (no check), knitr, jupyter, then TS engines
  (registry.rs:72-91 + contribution order); Q1 checks knitr, jupyter, then
  extensions (markdown filtered out). Same user-visible sequence.

## Design options

The binding constraint (Q1-identical user-visible behavior) fixes the *what*;
the options differ in *how* engine check output travels and how much of
`quarto check`'s surface Plan 10 implements.

### Axis 1 — how TS-engine check output reaches the terminal

**Option A — stderr passthrough channel.** Add a distinguished stderr prefix
(e.g. `[USER]`) that `stderr_loop` forwards straight to the user's terminal;
the engine's console output streams live while the check runs.
- \+ Live streaming (closest to Q1's animated-spinner feel); tiny wire payload.
- − Output ordering is asynchronous relative to the Rust-side report (a slow
  engine's lines can interleave with or trail the next section header); not
  deterministically snapshot-testable; overloads the diagnostics channel with
  user-facing output; JSON mode later would need a second mechanism anyway.

**Option B — wire-returned transcript (recommended).** New verb pair
`ToEngine::CheckInstallation { engine, conf }` /
`FromEngine::CheckInstallationResult { output: Vec<TsCheckOutputLine> }`.
During the check call the host routes the console sink into a capture buffer
(a swappable "current sink" indirection inside the host's `log` object) and
returns the transcript; the Rust command prints it in exact report order.
- \+ Deterministic ordering fully under `q2 check`'s control → Q1-identical
  report layout is guaranteed and snapshot-testable; plan-1a-conformant typed
  payload; JSON mode later reuses the same data path.
- \+ Sink-capture is safe in practice: during `q2 check` the command drives one
  request at a time and no render is in flight (invariant noted in the plan;
  cheap to assert host-side).
- − Buffered, not streamed: for a slow check the user sees nothing until the
  engine finishes, then all lines at once (Q1 shows an animated spinner
  meanwhile). Documented as decision point 2; q2's spinner is already neutral
  (start+done lines), so the residual divergence is only *when* the lines
  appear, not what they say.

**Option C — structured result schema (engines return data, CLI renders).**
Change the engine contract so `checkInstallation` returns structured results.
- Rejected outright: Q1 engines print and return `void`; changing the vendored
  contract breaks Q1-parity for third-party engines — violates the binding
  constraint.

### Axis 2 — capability discovery (Q1's `getTargets()` filter)

Add `#[serde(default)] has_check_installation: bool` to `LoadEngineResult`
(host: `"checkInstallation" in discovery` / `!== undefined`). Q1 parity
requires knowing which engines are checkable *before* invoking (target list +
silent skip). Alternative (call-and-coalesce `?? null`) can't reproduce
`quarto check <name>` target validation without invoking the check. Note:
enumerating targets forces `LoadEngine` on every registered TS engine (cheap:
module import in the shared host; no `LaunchEngine` needed — checkInstallation
is discovery-tier).

### Axis 3 — the Rust trait method (Part 2)

```rust
/// Run this engine's installation check, if it has one.
/// `None` = engine has no check (not a `q2 check` target; silently skipped
/// — Q1 semantics). `Some(Ok(report))` = check ran; report holds the
/// user-facing lines in order. `Some(Err(_))` = hard failure (aborts the
/// check run, non-zero exit — Q1 semantics).
fn check_installation(&self, ctx: &CheckContext)
    -> Option<Result<CheckReport, ExecutionError>> { None }
```

- `CheckReport` = ordered `Vec<CheckLine>` (kind: info/complete/warning/error +
  text) so native and TS engines render through one printer in the command.
- `CheckContext` carries `strict`, `target`, and (if/when test renders are in
  scope) a render callback constructed in the command crate — keeping
  quarto-core free of CLI dependencies.
- `TsEngine` impl: `ensure_loaded()` → consult `has_check_installation` → `None`
  if absent; else wire round trip (generous window, e.g. 60s — Q1 has no
  timeout; probes spawn subprocesses) mapping the transcript into `CheckReport`.
- Native impls (knitr, jupyter): reproduce Q1's decision trees and message
  strings exactly (Part 1b is the spec — including the `succesfully` typo,
  decision point 7), reusing `find_rscript`/`call_r` (embed a `capabilities.R`
  probe mirroring Q1's `knitr.R`, version gates knitr>=1.30 rmarkdown>=2.3) and
  `find_jupyter`/`list_kernelspecs` (embed/port Q1's `jupyter.py` probe).
  `MarkdownEngine`: keeps the default `None` (Q1: markdown is never a target).
- `is_available()` stays untouched — it's a cheap binary probe used elsewhere;
  `check_installation` is the rich, user-facing surface. No reconciliation
  conflict: resolution never consults `is_available()` (Part 2 findings), so
  the check capability duplicates nothing.

### Axis 4 — `q2 check` command scope

Implement in Plan 10:
- `Quarto <version>` banner; thread the currently-ignored `target` arg through
  dispatch; Q1-style target validation (`install`, `info`, `versions`, `all`,
  plus dynamic checkable-engine names; unknown target errors listing valid
  targets); engine check loop in registry order with Q1 semantics (silent skip,
  abort on first engine error, non-zero exit via propagation).
- Fixed sections (`info`, `versions`, `install`): **not** blindly cloned — q2
  has fundamentally different binary dependencies (no separate pandoc/dart-sass
  binaries; deno optional). Scope is decision point 1.

### Axis 1 revision (2026-07-03, ratified by Gordon) — streamed progress frames

The buffered transcript loses Q1's *within-engine* progress (spinner → OK →
details → second spinner, step by step); Gordon confirmed that progress
visibility matters. Revised, approved mechanism:

- Host: during `checkInstallation` the console sink forwards each line
  immediately as a `FromEngine::CheckProgress { line }` frame (same correlation
  id as the request) — simpler than a capture buffer. Final
  `CheckInstallationResult` (or `Error`) closes the request.
- Rust transport: pending slots are already per-id mpsc channels; add a
  `request` variant that invokes a progress callback per interim frame and
  returns on the final frame.
- `q2 check` prints each line as it arrives. One in-flight request at a time →
  deterministic order; final printed text identical to the buffered design,
  just live. Snapshot tests capture the same output.
- TTY spinner animates on the **engine-authored** start message (arrives as the
  first frame instantly), settling into the `[✓]` completion line. Non-TTY =
  plain lines (the degenerate buffered case).
- Native engines match by shape: the trait method takes a line sink (callback)
  rather than returning a completed report, so knitr/jupyter also emit
  progressively.

### Native check feasibility mapping (q2 mechanics verified, 2026-07-03)

Gordon approved full Q1-fidelity native checks for knitr/jupyter and asked
whether the q2 side was fully researched. Verified mapping — reuse vs new:

| Q1 step | q2 mechanism | Status |
|---|---|---|
| Resolve Rscript | `find_rscript()` (OnceLock-cached, `QUARTO_R` → which) | **reuse** |
| R capabilities probe (`knitr.R` YAML between markers) | run `Rscript <script>` directly (Q1-style), script copied into the embedded `KNITR_RESOURCES` (include_dir → temp extraction); the `call_r` JSON-action protocol is for execute, not needed here | **new script, existing plumbing** |
| knitr/rmarkdown version gates | port semver checks (knitr>=1.30, rmarkdown>=2.3) | **new (small)** |
| Resolve python (QUARTO_PYTHON → py launcher → conda → python3) | **q2 has no python resolution anywhere** (only jupyter-binary + kernel names) | **new (contained fn)** |
| Python capabilities probe (`jupyter.py`) | copy probe script into embedded jupyter resources; spawn resolved python | **new script + spawn** |
| `Kernels:` line | `list_kernelspecs()` (async, runtimelib + `jupyter --paths --json`) | **reuse** (bridge via current-thread tokio `block_on`, precedent `text_execute.rs:229-234`) |
| unactivated-env warning | port cwd scan (pyvenv.cfg/conda-meta/requirements.txt) | **new (small)** |
| Test render (`{python}`/`{r}` `1 + 1` doc) | `render_document_to_file` (`render_to_file.rs:201`) — sync, temp input, auto project discovery; passed into `CheckContext` as a callback from the command crate | **reuse** |
| Sync trait ↔ async helpers | current-thread tokio runtime + `block_on` | **reuse (precedent)** |

Q1 probe scripts (`capabilities/knitr.R`, `capabilities/jupyter.py`) are copied
locally per the External Sources Policy (one-time copy, never referenced from
`external-sources/` at build time).

## Recommended design (summary)

Option B transcript wire verb + capability flag on `LoadEngineResult` +
optional `check_installation` trait method with `CheckReport`, surfaced through
a real `q2 check` whose engine section mirrors Q1 exactly; native knitr/jupyter
checks port Q1's decision trees verbatim; markdown and non-implementing TS
engines silently skipped. JSON mode, strict mode, fixed Q1 sections, and
test-render sub-checks are explicitly scoped by decision points below.

## Decision points for Gordon (numbered; Q1 is the spec, deviations ratified here)

1. **Fixed report sections (`info`/`versions`/`install`) scope.**
   (a) *Recommended:* Plan 10 ships the version banner + engine section only;
   `install`/`info`/`versions` targets validate but print a minimal q2-true
   placeholder section (e.g. Quarto version/path), with full q2-appropriate
   content deferred to a follow-up strand. (b) Design q2-equivalent fixed
   sections now. Q1's versions section (Pandoc/Dart Sass/Deno/Typst pins) is
   largely inapplicable to q2's architecture, so (b) requires new UX decisions
   that exceed this strand.
2. **Buffered vs streamed engine check output.** ~~Buffered transcript~~
   **RATIFIED (Gordon, 2026-07-03): streamed `CheckProgress` frames + Rust-side
   TTY spinner animating the engine-authored messages** (see Axis 1 revision).
   Buffered output is the degenerate non-TTY case — same bytes, no animation.
3. **`--output <path>` JSON mode.** Recommended: defer (follow-up strand). The
   wire `conf` is designed to grow (`output`/`jsonResult` optional fields);
   engines receive `jsonResult: undefined` and take the console path — valid
   per the Q1 contract.
4. **`--no-strict` flag.** Recommended: defer alongside the versions section
   (strict only affects version pinning). Accept the flag? No — omit until the
   section exists (unknown-flag error today is honest).
5. **Test-render sub-checks** ("Checking Knitr/Jupyter engine render...."):
   (a) *Recommended, APPROVED (Gordon, 2026-07-03: full Q1-fidelity native
   checks):* include for **native** engines in Plan 10 (they're the heart of
   "verify the engine works"; q2 renders in-process via
   `render_document_to_file` passed as a callback in `CheckContext`);
   TS-engine `quarto.system.checkRender` remains a stub — defer with a linked
   strand (no existing TS engine calls it). See the feasibility mapping for
   reuse-vs-new inventory.
   (b) Defer all test renders — engine sections then end after
   capabilities/package reporting.
6. **Failure semantics.** Match Q1 exactly: an engine check that throws aborts
   the remaining checks and exits non-zero; JSON mode (when it lands) records
   errors and exits 0. Recommended: yes, exact match.
7. **Verbatim-typo parity.** Q1 prints `R succesfully found at <path>.` (sic).
   Recommended: reproduce Q1's wording *except* correcting the typo
   ("successfully") — flagged because "identical" was specified as binding.
8. **Native jupyter check identity.** Q1's jupyter check probes *Python*
   directly (`jupyter.py` via the resolved python binary). q2's jupyter engine
   resolves the `jupyter` binary + kernelspecs via runtimelib. Recommended:
   port Q1's python-probe approach (ship a `jupyter.py` equivalent) so the
   report content (Version/Path/Jupyter/Kernels lines) is Q1-identical, reusing
   q2's kernelspec enumeration for the `Kernels:` line.

Decision points 1-8: **all recommendations ratified by Gordon (2026-07-03)**
(2 in its revised streamed form; 5 in variant (a)).

## Final round — remaining questions + consequences (2026-07-03)

**Q9 — jupyter check truthfulness vs Q1-identical gating.** Q1's check reports
and gates on python *packages* (`jupyter_core`, `nbformat`, `nbclient`,
`ipykernel`) because Q1 executes through nbclient. q2's jupyter engine talks
ZeroMQ via runtimelib — its real prerequisites are the `jupyter` binary (kernel
*discovery* via `--paths`; static dirs still work without it) and a python
kernelspec (i.e. ipykernel). So a Q1-identical report prints package lines that
are informational rather than prerequisites for q2, and Q1's render-check gate
(`jupyter_core` present + python kernelspec) technically over-requires.
Mitigating fact: ipykernel depends on jupyter_core, so "kernelspec exists but
jupyter_core absent" is practically unreachable. *Recommendation:* keep the Q1
decision tree and lines verbatim (binding constraint; divergence cost ~nil),
and treat the test render as the ground truth of "q2 can execute".
(Knitr analog verified NON-issue: q2's R scripts require both knitr and
rmarkdown — `rmarkdown::` is called 30+ times in execute.R — so Q1's gates are
truthful for q2.)

**Q10 — "engine present but host runtime broken" UX (deno missing).** New
q2-only case with no Q1 analog (Q1 engines run in-process). If extension
engines are registered but `deno` is unavailable: *recommendation:* per-engine
`Checking <name> installation....(None)` + indented note "Unable to locate
deno (required to run extension engines)" — mirroring Q1's `(None)` +
install-hint pattern for missing runtimes — and **exit 0** (Q1 exits 0 on
missing runtimes; only render-check errors throw). A *broken engine bundle*
(LoadEngine returns `error`) instead aborts the command with the error — the
Q1 analog is an engine-import failure in `resolveEngines`, which also fails
the whole command.

**Consequences noted (no decision needed, will be encoded in the plan):**

- **C1 — cwd-dependence:** `q2 check`'s engine list depends on the directory
  it runs in (project extension discovery from cwd, zero-file fallback) —
  identical to Q1's behavior. The command reuses the project/registry setup
  path from `project/mod.rs`.
- **C2 — output stream:** report goes to **stderr** (Q1's `info()` writes to
  stderr), keeping pipe behavior compatible.
- **C3 — sibling-strand collision:** bd-m1jeqhhz (populateCommand) will touch
  the same seams (ToEngine/FromEngine variants, LoadEngineResult capability
  flags, host.ts dispatch). Additive changes, mechanical rebase; whichever
  plan lands second on the epic branch rebases.
- **C4 — timeout policy:** streaming enables an *idle* timeout (window resets
  on each `CheckProgress` frame, e.g. 60s idle) instead of a total budget —
  robust to slow test renders without hanging forever on a dead host.
- **C5 — Windows:** the python resolution port includes Q1's py-launcher
  branch (`PY_PYTHON`); cross-platform rule applies; macOS/Linux CI can't
  exercise it — flagged for the plan's test section.
- **C6 — target-name shadowing:** fixed targets (`install`, `info`,
  `versions`, `all`) shadow any engine that happens to use those names —
  inherited Q1 behavior, not worth diverging.
