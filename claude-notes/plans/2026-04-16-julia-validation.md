# Plan 4: Julia Engine Validation

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plans 1a/1b/1c, 2, 3, and 1c.2 P1.1+P1.1b — **all landed as of 2026-07-02** (see Prerequisites)
**Blocks:** Plan 4b (shadow-engine feature validation)
**Estimated sessions:** 2-3 (the net-new instrumentation in 4H/4I and the daemon test push this past the original 1-2 "pure debugging" estimate)
**Status: COMPLETE (2026-07-02).** All 13 success criteria met; frozen seams
J1–J6/J8/J9 green; V-1…V-7 evidence recorded (V-2 folded into J3); full
`cargo xtask verify` green at `1a44b4e2e`. Headline: `julia-engine.ts` ran
with ZERO source changes (rebundle byte-identical). Discovered-work strands:
bd-uf4epv4w (smart typography vs frontmatter strings), bd-l9jhy5u0 (QNR
worker leak on error path, P1 — reproduces under Q1 too), bd-cymkcyaf
(presentation-format per-writer execute defaults), bd-677297ca
(supporting-dir resource copy — FIXED+closed this session). Migration guide:
`claude-notes/research/2026-07-02-julia-engine-migration-guide.md`; evidence
trail: `…/2026-07-02-julia-engine-q2-compat.md` §1–§14. Julia-in-PATH stays a
per-session check by design (this session: julia 1.11.7). The unticked
observation sub-items in 4C/4H are honest divergence records (inline-MIME
figures; GR emits no htmlDependency), not gaps.

## Overview

End-to-end validation of the TypeScript engine extension system using the Julia engine from Quarto 1. Take the real `julia-engine.ts`, set it up as a q2 extension, and render documents with Julia code cells.

This plan is primarily integration debugging. If Plans 1a, 1b, 1c, 2, and 3 are solid and the echo engine test from Plan 1c Phase 3 passes, most of the infrastructure works. This plan surfaces the gaps specific to a real-world engine extension.

## Prerequisites

**All code prerequisites are satisfied on this branch as of 2026-07-02** — the
first action is to *confirm* them green, not to build anything.

- [x] Plans 1a, 1b, and 1c complete: Rust subprocess infrastructure + Deno harness + extension integration, echo engine passes (grand-plan table: all ✓)
- [x] Plan 2A complete: the `@quarto/api` package skeleton (`package.json`, `tsconfig.json`, exports map) and the `./config` key-list subpath are in place
- [x] Plan 2 complete: the remaining QuartoAPI surface built on that skeleton — `@quarto/api`'s text/markdown/format/path/system/console/crypto subpaths, all QuartoAPI namespaces except `jupyter` wired in
- [x] Plan 3 complete: `@quarto/api/jupyter` with `toMarkdown` working and wired into engine-host
- [ ] Julia installed on the test machine (`julia` in PATH) — machine-specific, verify per session
- [x] **Plan 1c.2 P1.1 — LANDED** (commit `2b2113e6c`; e2e tests
  `p1_1_project_context_threaded_{project,single_file}_leg`). `set_project` /
  per-render `EngineProjectContext` wired. It only ever gated Phase 4H and one
  success criterion — both are wiring assertions *(Q1's julia-engine.ts ignores
  the launch context entirely; its `launch(context)` never reads a field)*.
- [x] **Plan 1c.2 P1.1b — LANDED** (commit `5fdcd4b56`; e2e test
  `p1_1b_document_metadata_threaded_into_format`). Merged document metadata now
  flows into `TsFormatInfo.metadata` (`metadata: ctx.metadata.clone()`,
  `ts_engine.rs:396`) — so `execute:`/`julia:` frontmatter reaches the engine,
  Phase 4E is testable, and `execute: daemon: false` (which every Plan-4 test
  document sets — see the 4B warning) actually disables the detached Julia
  daemon. Confirm green: `cargo nextest run -p quarto-core -E 'test(p1_1)'`.

## Work Items

> **Execution order = document order.** The phase letters are historical: 4G (documentation)
> was moved last; 4H/4I were appended after the website-project epic landed on `main`; and 4F
> (regression audit) was moved after 4I on the 2026-07-02 review so the full-workspace verify
> covers all net-new code (4H/4I instrumentation) instead of running mid-plan.

### Phase 4A: Set up Julia engine extension

**We run the existing extension, not a re-specified one.** (Decision 2026-07-02.)
The source of truth is the upstream repo `~/src/quarto-julia-engine` (same
content as Q1's `src/resources/extension-subtrees/julia-engine/` subtree). Its
layout is already correct for q2: `_extensions/julia-engine/` holds
`_extension.yml`, the bundled `julia-engine.js`, `Project.toml`, and the three
`.jl` scripts — co-located because `julia-engine.ts` resolves resources via
`dirname(fromFileUrl(import.meta.url))`, i.e. the directory of the loaded
bundle (`src/julia-engine.ts:46`). `src/` holds the TS source. Do NOT invent a
new layout; the fixture copy is modified for q2 static claiming, and merging
those changes back upstream is deferred (Gordon's call).

- [x] Bring `resources/extension-build/deno.json` (and `deno.workspace.json`)
  to **Q1 import-map parity**: add the bare-specifier aliases from Q1's
  `src/resources/extension-build/import-map.json` — `path`, `path/posix`,
  `log`, `log/`, `fs/`, `encoding/` → pinned jsr `@std` packages
  (`@std/path@1.0.8`, `@std/log@0.224.0`, `@std/fs@1.0.16`,
  `@std/encoding@1.0.9`). This is what lets `julia-engine.ts`'s bare imports
  (`"path"`, `"fs/exists"`, `"encoding/base64"`) bundle **unchanged**. The q2
  port of the config dropped these aliases (apparent oversight — no recorded
  decision); note the parity restoration against plan1c's config spec
  (plan1c lines ~421-446).
- [x] Copy `~/src/quarto-julia-engine` into
  `crates/quarto-core/tests/fixtures/extensions/julia-engine/` (the
  established extension-fixture location, next to `echo-engine/`), preserving
  its layout. **Exclude `.git/`** and repo-only files (`tests/`, `example*`) as
  convenient; keep the root `_quarto.yml` — it makes the fixture a renderable
  project dir, which is where the 4B–4E test documents live. Provenance note:
  `~/src/quarto-julia-engine` is a machine-local checkout (same content as
  Q1's `extension-subtrees/julia-engine/`); this is a **one-time copy** — the
  committed fixture is what tests use thereafter, so no build or test may
  reference the home-dir path or `external-sources/`.
- [x] Modify the fixture copy's `_extensions/julia-engine/_extension.yml` for
  **q2 static claiming** (q2-native keys; Q1's file has none):
  ```yaml
  contributes:
    engines:
      - path: julia-engine.js
        name: julia
        claims:
          julia: { kind: primary, priority: 1 }   # static claim → zero-load resolution
        file-extensions: [".jl"]                  # can-handle pre-filter (decision 2026-07-02)
  ```
  With the static `claims:` declared, q2 resolves `{julia}` cells to this
  engine **without loading the Deno subprocess** — it spawns only to execute,
  once Julia has won ownership (see `claude-notes/designs/engine-resolution.md`
  §3.3). The engine's dynamic `claimsLanguage` returns a bare **boolean**
  (`language.toLowerCase() === "julia"`, not a `primary()` object); it is
  validated against the static claim on first load (`ensure_loaded`,
  `ts_engine.rs:240-329`, hard error on mismatch). *(Seam check 2026-07-02:
  the normalization `true` → `{kind: primary, priority: 1}` is already pinned
  — `mapLanguageClaim` at `host.ts:168` with named-revert tests at
  `host.test.ts:423`/`:489` — so the comparison will pass; if the first Julia
  load hard-errors, look elsewhere.)*
  - Resolved 2026-07-02: **declare `file-extensions: [".jl"]`** (as above),
    matching `engine-resolution.md` §3.3's own Julia example. It is only a
    can-handle **pre-filter**; `claims-files` stays undeclared (Julia's
    `claimsFile` is content-inspecting — `# %%` percent scripts), so this
    does **not** cause Pass-1 subprocess spawning and Phase 4I's zero-spawn
    assertion stands unchanged. (The earlier worry conflated the two axes.)
    Note: 1c.2 P4 renames the field `claims-files` → `claims-extensions`
    (not landed as of 2026-07-02; `read.rs` still parses `claims-files`) —
    since Julia never declares it, this plan is unaffected either way.
- [x] Rebundle with `cargo run --bin q2 -- build-ts-extension src/julia-engine.ts`
  (run from the fixture dir, after the import-map parity item; verify the
  output lands at `_extensions/julia-engine/julia-engine.js` — q2 mirrors
  Q1's output-path convention; see
  `crates/quarto/src/commands/build_ts_extension.rs` if it doesn't) and commit
  the regenerated bundle. The upstream bundle was built by Q1's
  `quarto call build-ts-extension`; rebuilding under q2's config is the real
  build-compatibility test.
  - **Note:** the literal invocation above doesn't run as written — `PATH`
    must be the extension directory (`_extensions/julia-engine`), not the
    `.ts` file, and `find_entry_ts` additionally requires the TS source to
    live at `<ext_dir>/src/<name>.ts`, which upstream's real (root-level
    `src/`) layout doesn't satisfy. Worked around with a local, uncommitted
    symlink (`_extensions/julia-engine/src -> ../../src`) for the duration
    of the build only; removed immediately after. Full detail + the exact
    two failure modes: `claude-notes/research/2026-07-02-julia-engine-q2-compat.md`
    §4/§8.1. Result: the rebuilt bundle is **byte-identical** to the
    upstream Q1-built one (MD5 `d9d5120eb94b187903a43fb500e65eea`).
- [x] Identify any remaining needed modifications to `julia-engine.ts`:
  - API calls: verify all `quarto.*` calls match our implementation signatures
    (25 distinct calls across 8 namespaces; all exist in `@quarto/api` per the
    2026-07-01 audit) — **audit re-run 2026-07-02 found 7 namespaces / 30
    call sites** (not 8/25); no missing-API issues found either way. See
    compat log §7 for the reconciliation flag.
  - Resource resolution: verify `import.meta.url`-based paths work after
    rebundling (bundle must stay co-located with the `.jl` files) — single
    use-site (`julia-engine.ts:46`), fixture keeps the bundle co-located
    with the `.jl` scripts under `_extensions/julia-engine/`; no issue found,
    flagged for 4B to confirm the TS-engine host doesn't relocate the bundle
    before load (compat log §7).
  - Deno APIs: verify `Deno.Command`, `Deno.connect`, `crypto.subtle`, file
    I/O all work (they should — it's running in real Deno) — all present in
    source (compat log §7); execution-time verification deferred to 4B.
- [x] Document every modification in a compatibility log (input for merging
  back to `~/src/quarto-julia-engine`, and for the 4G migration docs) — see
  `claude-notes/research/2026-07-02-julia-engine-q2-compat.md`.

### Phase 4B: Minimal Julia render

The simplest possible Julia document.

> **Daemon warning (read before the first render).** The julia engine's daemon
> default is `isInteractiveSession && !runningInCI`, and q2 wires those to real
> runtime values — so an interactive dev-machine render **starts a detached
> Julia server that outlives q2**, and q2 has no management surface for it
> (bd-m1jeqhhz). Policy (decision 2026-07-02, details in 4E): every Plan-4 test
> document sets `execute: daemon: false` — effective because 1c.2 **P1.1b** has
> landed (before it, the option was dropped with the rest of the metadata and
> could not disable the daemon). If a daemon escapes anyway: the transport file in the julia runtime
> dir (`quarto.path.runtime("julia")`, see `juliaTransportFile()`) has the
> server's port/PID — kill it from there.

- [x] Create test document **at the fixture root** (next to `_extensions/`; the
  fixture is a renderable project dir via its upstream `_quarto.yml`):
  ```markdown
  ---
  engine: julia
  execute:
    daemon: false
  ---

  ```{julia}
  1 + 1
  ```
  ```
  (committed as `crates/quarto-core/tests/fixtures/extensions/julia-engine/minimal.qmd`)
- [x] Run through q2's render pipeline. Use `cargo run --bin q2 -- render <file.qmd>` (the crate at `crates/quarto/` builds the `q2` binary). The automated-test harness template is `crates/quarto-core/tests/integration/echo_engine_e2e.rs` (see the Test Seam Spec).
- [x] Debug the first failure. Three real failures found + fixed (full trail in
  compat log §9). Against the checklist:
  - [x] Extension not discovered → `_extension.yml` parsing issue — YES: q2
    requires an `author` field (q2-vs-Q1 divergence); added to fixture.
  - [x] Deno subprocess won't start — no (started fine).
  - [x] Engine module fails to load — no.
  - [x] `engine.init()` fails → QuartoAPI construction — no (API pre-flight:
    all 25 called members exist, jupyter's 6 are all implemented).
  - [x] `engine.launch()` fails — no.
  - [x] Julia process won't start — no.
  - [x] Julia server connection fails — no (HMAC/TCP fine).
  - [x] Execution succeeds but output is wrong → **two distinct q2 bugs**:
    (a) empty wire source map made julia's `buildSourceRanges` send `[]` →
    QNR crashed on `maximum([])`; fixed by serializing a real source map in
    `ts_engine.rs`. (b) missing execute-visibility defaults
    (`include/output/eval`) made `jupyterToMarkdown` drop every cell → empty
    body; fixed with `applyExecuteDefaults` in the engine-host.
  - [x] Result deserialization fails — no.
- [x] Iterate until the simple document renders successfully
- [x] Verify output HTML contains the result `2` — `<div class="cell-output
  cell-output-display">…<code>2</code>…</div>`; frozen as seam **J1**
  (`crates/quarto-core/tests/integration/julia_engine_e2e.rs`), RED/GREEN +
  named-revert proven.

### Phase 4C: Julia with figures

- [x] Create test document with a plot (daemon policy: `daemon: false` like
  every Plan-4 doc):
  ```markdown
  ---
  engine: julia
  execute:
    daemon: false
  ---

  ```{julia}
  using Plots
  plot(1:10, rand(10))
  ```
  ```
  Committed as `crates/quarto-core/tests/fixtures/extensions/julia-engine/plot.qmd`.
  Required a one-time notebook-environment setup NOT anticipated by this plan
  item's literal text: the fixture root needed its own `Project.toml`/
  `Manifest.toml` (declaring `Plots`), separate from the extension's own
  `_extensions/julia-engine/Project.toml` — QuartoNotebookRunner activates
  `JULIA_PROJECT=@.` from the *notebook's* directory, not the engine's. Not a
  q2 bug; committed the environment files (40K `Manifest.toml`). Full trail:
  compat log §10.
- [x] Verify (concrete targets, per the e2e norm — record invocation + snippet):
  - [ ] Figure file generated at `<stem>_files/figure-html/<cell>-output-*.png`
        (or the fig-format in effect) — **did NOT materialize for this
        document**: Plots.jl's default GR-backend plot is `text/html`-showable,
        and Q1's own (faithfully ported) MIME-type priority always prefers
        `text/html` over `image/png` for HTML targets, so the file-writing
        branch (`mdImageOutput`) is never reached. Traced, not a q2 bug — see
        compat log §10 and the 4CD task report.
  - [ ] Output HTML contains `<img src="<stem>_files/figure-html/..."` pointing
        at that file — **did NOT materialize** (same root cause); actual:
        `<img src="data:image/png;base64,...">` (embedded).
  - [x] `supporting` files tracked in `ExecuteResult` — **CONFIRMED**, via a
        temporary `eprintln!` on `map_execute_result` (added, observed,
        removed): `supporting=["…/plot_files"]`, one entry, sent
        unconditionally by `julia-engine.ts` regardless of whether a figure
        file actually landed there. See compat log §10.
  - [x] The figure displays when the HTML is opened — confirmed (valid,
        complete base64-embedded PNG).

### Phase 4D: Multiple cells and error handling

- [x] Test multiple code cells (`execute: daemon: false` in the frontmatter,
  per the daemon policy — elided below for brevity):
  ```markdown
  ---
  engine: julia
  execute:
    daemon: false
  ---

  ```{julia}
  x = 42
  ```

  ```{julia}
  println("x is $x")
  ```
  ```
  Committed as `crates/quarto-core/tests/fixtures/extensions/julia-engine/multi-cell.qmd`.
- [x] Verify state persists between cells (x defined in first, used in second)
  — **V-5, manual, confirmed**: cell 2's stdout output is `x is 42`. Invocation
  + full snippet in the 4CD task report and compat log §10.

- [x] Test error handling:
  ```markdown
  ---
  engine: julia
  execute:
    daemon: false
  ---

  ```{julia}
  error("this should fail gracefully")
  ```
  ```
  Landed as the frozen seam **J4**
  (`crates/quarto-core/tests/integration/julia_engine_e2e.rs::j4_error_handling_does_not_wedge_host`),
  not just a manual doc — inline `ERROR_DOC` const, matching J1's precedent.
- [x] Verify error produces a useful message, not a crash: the render reports a
  diagnostic (or non-zero exit) that includes the Julia error text
  (`this should fail gracefully`) and ideally the cell's source location; q2
  itself must not panic and the Deno subprocess must not be left wedged
  (a subsequent render of the 4B document still works) — **confirmed, J4
  GREEN, RED-proven via the named revert in `TsEngineHost::request`'s
  `FromEngine::Error` arm (`ts_process.rs:~693`).** Note: the first draft of
  the test's error-message assertion (`contains("this should fail
  gracefully")` alone) turned out to be vacuous against this exact revert —
  `TsEngine::execute`'s generic fallback error message still contains that
  substring via its `{:?}` Debug dump. Strengthened before freezing (see the
  4CD task report and compat log §10 for the full RED/GREEN/named-revert
  trail, done twice — once exposing the vacuous assertion, once against the
  fixed one).

### Phase 4E: Julia-specific features

**Substrate: complete** (1c.2 P1.1b landed — see Prerequisites). Rust threads
merged metadata into `TsFormatInfo.metadata`; the Deno host's
`metadataAsFormat` partitions it into Q1's six-bin `Format`
(`daemon`/`fig-dpi`/etc. land in `format.execute`; a `julia:` block lands in
`format.metadata` and reaches QuartoNotebookRunner via the serialized
options).

**Daemon policy (decision 2026-07-02):** all Plan-4 fixtures set
`execute: daemon: false` (oneShot) **except** the one dedicated daemon test
below. q2 has no equivalent of Q1's `quarto call engine julia
status/kill/log/close/stop` (`populateCommand` is not wired) — future surface
tracked as **bd-m1jeqhhz**. The daemon test tears down out-of-band: transport
file in the julia runtime dir → port/PID → kill.

- [x] Test daemon mode (`execute.daemon: true`) — **V-1 (manual, deliberately
  not frozen)**: evidence recorded (compat log §11 + 4E task report), run in
  an isolated `HOME` because the transport file is global per user and a
  concurrent docs-agent daemon owned the real one (left untouched, verified).
  Key findings: `daemon: false` starts the detached control server anyway and
  closes only the per-file WORKER (transport file persists); `daemon: true`
  keeps the worker open — second render 0.33 s vs 5.73 s and the cell's
  `getpid()` printed the SAME worker pid (in-band reuse proof); teardown via
  transport-file PID → SIGTERM kills the worker and removes the transport
  file (QNR atexit). Zero orphan processes (final `ps` matched the
  pre-session baseline exactly). Stable → **promotable to a J-row**
  (additive seam entry, controller sign-off required; needs a HOME-isolated
  harness).
- [x] Test `exeflags` — **J3**: landed as
  `julia_engine_e2e::j3_exeflags_and_env_through_julia_block`, GREEN and
  RED-proven — but with two evidence-backed deviations from the frozen row
  (flagged for controller sign-off; full trail in the 4E task report and
  compat log §11):
  1. **Fixture placement**: the `julia:` block lives in the temp project's
     `_quarto.yml`, not document frontmatter — document-frontmatter
     `--threads=2` is smart-typography-mangled to `–threads=2` (en dash) by
     q2's DocumentMetadata markdown parse, and QNR treats it as a file arg
     (real substrate bug, filed **bd-uf4epv4w**; project-config strings stay
     literal, so the spec's exact flag survives there).
  2. **Named revert re-anchored**: the spec'd T14 revert CANNOT redden any
     QNR-observable assertion — QNR merges the notebook file's own
     frontmatter under the wire options AND (deeper) julia-engine.ts sends
     `target.markdown` (q2's post-merge serialized AST), which QNR's socket
     layer uses as a file-content override (`socket.jl:497`) — so merged
     metadata reaches QNR even with the wire path reverted (verified
     empirically: the T14 revert left both document-level and project-level
     fixtures GREEN). Re-anchored revert: the project metadata layer in
     `MetadataMergeStage::run` (`metadata_merge.rs:~214`) → RED proven.
     T14 itself stays revert-bound by J2 (host-side `format.execute`
     consumption has no QNR fallback).
  Schema stop-point resolved against installed QNR 0.17.4 source (matches
  the fixture pin): `options["format"]["metadata"]["julia"]["exeflags"]` /
  `["env"]` (`server.jl:151-168`), i.e. a top-level `julia:` mapping with
  `exeflags`/`env` string arrays.
- [x] Test `env` — **folded into J3** (same doc/test, one extra cell line +
  frozen assertion `FOO=BAR`; no separate V-2 needed).
- [x] Test cell options — **J2** landed as
  `julia_engine_e2e::j2_document_level_echo_false_hides_source_keeps_output`,
  GREEN, RED-proven verbatim against the spec'd T14 revert
  (`metadata: HashMap::new()` at `ts_engine.rs:394` → source listing present
  → RED at the source-absent assertion). Manual greps recorded in the 4E
  task report: `#| output: false` → cell output absent;
  `#| warning: false` → warning text absent, normal output present.

### Phase 4H: Website-project integration

Validates that the TS engine subsystem cooperates with the two-pass
project orchestrator (`ProjectPipeline`) that landed on `main` after
these plans were drafted. The Julia engine itself has no project-
specific logic, so this phase is a smoke test of the integration.

- [x] Create a minimal website fixture with a Julia page and a
  markdown page — **DONE** (`crates/quarto-core/tests/fixtures/extensions/
  julia-website/{_quarto.yml, index.qmd, plot.qmd}`; `_extensions/` +
  notebook `Project.toml`/`Manifest.toml` copied in at runtime from the
  sibling `julia-engine` fixture, nothing julia-specific committed under
  `julia-website/`). `plot.qmd` uses the file-based-figure mechanism
  (`GKSwstype=100` + `savefig` + an `image/png`-only `PngFigure` wrapper —
  see the 4H task report §1; `fig-format: png` was investigated and rejected
  as insufficient for Plots' `text/html`-showable default).
  ```
  crates/quarto-core/tests/fixtures/extensions/julia-website/
    _quarto.yml             # project.type: website
    _extensions/julia-engine/   # populated from ../julia-engine/_extensions/julia-engine
    index.qmd               # markdown only
    plot.qmd                # ```{julia} plot(...) ``` with figures
  ```
  Do **not** commit a symlink for `_extensions/julia-engine` — committed
  symlinks are unreliable on Windows checkouts (`.claude/rules/cross-platform.md`).
  Per the seam spec (J5): commit only `_quarto.yml` + the two `.qmd`s under
  `julia-website/`; the automated test's setup copies the extension in from
  the sibling `julia-engine` fixture at runtime (the echo `setup_project`
  pattern). For manual renders, copy it in the same way.
- [x] Run `cargo run --bin q2 -- render <project-dir>` and verify (manual,
  recorded in the 4H task report §1/§7):
  - [x] `_site/index.html` and `_site/plot.html` both produced — **DONE.**
        Both files are written AND the render now returns Ok: the
        bd-677297ca blocker (file_copy on the `plot_files` DIRECTORY
        supporting entry) was fixed by expanding supporting directories
        into contained files in `DocumentResourceReport::add_engine_files`
        (option (c), controller-adjudicated). Bound by the now-un-ignored
        J5 (`j5_website_figure_lands_as_file_and_is_referenced`, GREEN).
  - [x] `_site/site_libs/` contains shared assets (`bootstrap/`, `quarto/`)
        *(observation only — accepted-untested)*
  - [x] Julia-emitted figures land at the expected per-page location
        (`_site/plot_files/figure-html/cell-2-output-1.png`), **not** in
        `site_libs/` (`find _site/site_libs -name '*.png'` → empty). Bound by
        J5 (bd-677297ca fixed; J5 un-ignored and GREEN).
  - [ ] `htmlDependency` — the default Plots/GR backend emits none
        *(accepted-untested — if-observed only; not observed)*
  - [x] Sidebar/navbar transforms run normally (the `_quarto.yml` navbar
        rendered) *(observation only — accepted-untested)*
- [x] Verify the `Arc<TsEngineHost>` is shared across both files' renders:
  **J8 (observable) landed + unit-tested TDD-first; J6 (assertion) GREEN.**
  Net-new production `tracing::info!(target: "engine_host", pid, …)` in
  `ensure_started_inner` — GREEN, RED→GREEN + named-revert proven
  (`test_j8_spawn_event_*` in `ts_process.rs`). The one-spawn property was
  first observed end-to-end (manual `q2 render`: exactly one `engine-host
  spawned` event, ordered after two `engine resolution complete` events —
  report §4), and is now bound by the **automated** J6 project-render row
  (`j6_one_engine_host_per_project_render`), un-`#[ignore]`d after
  bd-677297ca was fixed (see the 4H item above) and GREEN; it runs under
  `QUARTO_JOBS=1` so the whole render stays on the thread the tracing
  capture is scoped to (rayon Pass-2 workers don't see thread-local
  subscribers). Capture discrimination proven GREEN by
  `j6_capture_discriminates_two_hosts` (deno-only). The J8 event also serves
  Phase 4I (J9) — J9's resolution-complete event was ALSO added now
  (`resolve_engines`, target `engine_resolution`) so 4I is test-only.
- [x] Verify each file's Julia `launch()` receives a populated project
  context (V-3) — **DONE** (report §6): temporary launch-site instrumentation
  (reverted before commit) observed
  `project_dir=Some(<root>) output_dir=Some(<root>/_site) is_single_file=false`
  — non-empty, correct, not `default()`.

### Phase 4I: Pass-1 cost audit

Pass 1 advances every project file to the `DocumentProfile`
checkpoint without running engines. For Julia documents this means
parse + metadata-merge only. Verify the Julia subprocess is **not**
started during Pass 1 *or during Pass-2 resolution* — with static
claims it spawns only at the first execute (Julia claims by language
only; no `claims_file` wiring for `.jl` percent scripts in v1).

> **Seam check 2026-07-02 — the original assertion here was vacuous.**
> "Spawn happens after Pass 1" is true for a legacy dynamic-claims engine
> too (it spawns during Pass-2 *resolution*), so reverting the entire
> static-claims machinery would leave it green. The discriminating surface
> is **"no spawn during resolution"** — the spawn event must order after
> resolution-complete (i.e. at first execute). Seam spec row **J9** binds
> this echo-based (deno-gated only, no Julia needed); the Julia run below
> is recorded evidence (V-4) on top.

- [x] Implement J9 (echo-based ordering test: both events present AND spawn
  AFTER resolution-complete; named revert = the static early-answer branch
  in `claims_language`, `ts_engine.rs:~550`; the resolution-complete event
  fires at the end of `resolve_engines`, `resolution.rs:~334`). —
  `j9_resolution_before_spawn_zero_load` in `echo_engine_e2e.rs`, RED→GREEN
  + named-revert proven verbatim (see compat log §12). Commit `62d7dadf6`.
- [x] In a website fixture with multiple Julia pages, reuse the same
  events (J8) to observe spawn timing. Render with
  `cargo run --bin q2 -- render <fixture>`. — done via a temp copy of the
  committed `julia-website` fixture, `RUST_LOG=engine_host=info,
  engine_resolution=info ./target/debug/q2 render /tmp/v4-julia-website`.
- [x] Verify and record (V-4): exactly one spawn, ordered after
  resolution-complete and at the first Julia execute. — confirmed: one
  `engine-host spawned` line, after both `engine resolution complete`
  lines, immediately followed by the child's own execute-time stderr
  (`Running [1/1] at line 27...`, the first line of `plot.qmd`'s cell).
  Full log snippet in compat log §12.
- [x] If `claims_file` is wired for `.jl` percent scripts later, the
  subprocess will spawn during Pass 1 — note that as expected
  behavior, not a regression. — not wired in v1; noted here as the
  documented future-behavior caveat, no action needed now.

### Phase 4J: Julia-in-preview validation (V-7 — added 2026-07-02, user-requested)

Plan 1c's **R5** wired TS engines into `q2 preview`'s **native** capture →
splice pipeline (all three call sites: eager `capture_driver.rs`,
`preview_record`/`cache.rs`, `re_execute.rs`) and proved it with the echo
engine (P2-14). Nothing has validated a *real* engine through preview. This
phase is **manual evidence only (V-7)** — no frozen test; the binding for the
registry-read hunks stays P2-14's echo seam.

- [x] Run `cargo run --bin q2 -- preview <julia-fixture-doc>` against a
  `daemon: false` Julia doc (temp copy of the committed fixture). Record:
  the initial preview shows executed output (the `2`, not an inert code
  block); the capture path logged a real engine execute. **Caveat:** curl
  against the served page only reaches the SPA shell (content syncs over
  the automerge/samod websocket, not a plain HTTP GET); confirmed instead
  via the Phase C.7 filesystem cache (`<data_dir>/captures/*.bin`), which
  the code documents as byte-identical wire format to what the WASM side
  ungzips — see compat log §13.
- [x] Edit the cell (e.g. `1 + 1` → `2 + 3`) and record the live
  re-execution result (`5`) through the `/api/preview/re-execute` path.
  Confirmed via the same cache-file mechanism + tracing (compat log §13).
- [x] Observe daemon behavior under preview: preview is an interactive
  session, so WITHOUT `daemon: false` julia would default to a detached
  server (bd-m1jeqhhz — no management surface). Record transport-file
  state after the `daemon: false` session (expect none) and note the
  `daemon: true`-by-default hazard for real users in the compat log.
  Confirmed: shared daemon transport files unchanged across the whole
  session (compat log §13).
- [x] Cleanup: verify no orphan julia/QNR processes from the session.
  Confirmed: 25 julia processes before and after (identical to the
  pre-existing bd-l9jhy5u0 leaked pool; no new entries); the two
  engine-host PIDs had already exited before shutdown (compat log §13).
- [x] Record all invocations + snippets in the compat log (§13); note any
  divergence between preview-spliced output and the `q2 render` output of
  the same doc (they need not be pixel-identical — note, don't fix). No
  divergence found — both show `5` for the edited doc.

### Phase 4F: Regression audit

(Moved after 4I on the 2026-07-02 review so the full verify covers the 4H/4I
net-new instrumentation code, not just the fixture work.)

- [x] Run same test documents through Quarto 1 for comparison — done via
  `~/bin/quarto` (dev checkout, `99.9.9`) against a temp copy of
  `~/src/quarto-julia-engine`; minimal/multi-cell/error/echo-false docs all
  rendered. See compat log §14.
- [x] Document output differences — compat log §14 comparison table +
  corrected-finding write-up (the §9 "HTML hides source by default" note was
  overstated for plain HTML; the real gap is presentation-format-scoped).
- [x] Verify all existing q2 tests pass (`cargo nextest run --workspace`) —
  verify #2 green 2026-07-02, exit 0 (HEAD `1a44b4e2e`); not re-run this
  session per the session's already-green status.
- [x] Run `cargo xtask verify` for full validation — verify #2 green
  2026-07-02, exit 0 (HEAD `1a44b4e2e`); not re-run this session per the
  session's already-green status.
- [x] File issues (via `braid create`) for any gaps discovered — bd-cymkcyaf
  filed (format-agnostic execute defaults vs. Q1's presentation-format
  overrides); bd-uf4epv4w, bd-l9jhy5u0, bd-m1jeqhhz, bd-677297ca reviewed,
  no new strands needed for those.

### Phase 4G: Adaptation documentation

- [x] Write a summary of all changes needed to `julia-engine.ts` — headline
  result: **zero source changes** (byte-identical rebundle, compat log §4).
  Written up as `claude-notes/research/2026-07-02-julia-engine-migration-guide.md`.
- [x] Categorize changes:
  - Import path adjustments — none in the extension; one repo-side shipped
    fix (import-map parity, `e56da9c29`).
  - API signature differences — none found (30/30 call sites match, §7/§9).
  - Missing QuartoAPI methods (if any were stubbed) — none (all 6
    `jupyter.*` members implemented).
  - Behavioral differences — `_extension.yml` q2-native keys + required
    `author`; `build-ts-extension` directory-resolution mismatch (symlink
    workaround); notebook-environment setup (CI Manifest.toml cost
    callout); three q2-side completeness fixes (execute-visibility
    defaults, execute source map, supporting-dir expansion) that are q2
    catching up to Q1 behavior, not engine adaptation; four still-open
    tracked gaps (bd-cymkcyaf, bd-uf4epv4w, bd-l9jhy5u0, bd-m1jeqhhz).
- [x] This becomes the basis for documentation for extension authors
  migrating from Quarto 1 — the migration guide is framed explicitly for
  that audience (see its "Bottom line for an extension author" section).

## Test Seam Spec (frozen — prevalidated 2026-07-02)

One row per durable automated test this plan produces. **Tier · real unit
mounted · seam (harness + assertion surface) · mock boundary · named revert
hunk.** Once green, assertions and harness are frozen — never edited to go
green. Manual validations (V-rows) name the production hunk whose absence
would change the recorded output; they are evidence, not regression guards.

**Harness template (all J-rows):** the echo pattern in
`crates/quarto-core/tests/integration/echo_engine_e2e.rs` — **in-process**
`render_to_file` (same entry as `quarto render`) / `ProjectPipeline`, fixture
copied into a TempDir under `_extensions/`, gated by early-return skip with an
`eprintln!("SKIP: …")`. Julia rows gate on `deno_available() &&
julia_available()`; a skip on a machine with both is a signal, not a pass.
Manual 4B–4E renders use the committed fixture root directly; automated tests
always go through the TempDir copy.

> **Round-3 corrections (2026-07-02, pre-implementation — no row was green
> yet, so the freeze is unviolated):** J2 pinned to *document-level*
> `execute: echo: false` (a cell-level `#| echo: false` travels inside the
> cell source and would NOT redden the P1.1b revert); J4's revert hunk
> relocated to the `FromEngine::Error` arm in `TsEngineHost::request`
> (`ts_process.rs:~693`) — `TsEngine::execute` never sees an error frame;
> J9's refs corrected (`claims_language` static branch `ts_engine.rs:~550`;
> event at end of `resolve_engines`, `resolution.rs:~334`) and both-events-
> present made explicit; J5 refs drifted to `:458`/`:465`; J2/J3's shared
> hunk is 1c.2's now-landed T14 revert — they add Julia-behavior coverage on
> that hunk, not new-hunk coverage. Also: P1.1/P1.1b LANDED, so revert
> phrasing below means "remove the landed population," not "don't build it."

**Seam-check findings (2026-07-02):**
- **Already bound, no new test:** the `true ≡ Primary(1)` normalization the
  4A item worries about is pinned by `mapLanguageClaim` (`host.ts:168`) with
  existing named-revert tests (`host.test.ts:423`, `:489`), and the
  static-vs-dynamic mismatch hard-error path has ts_engine unit coverage. If
  the first Julia load errors, the cause is elsewhere.
- **Vacuity fix (4I):** "subprocess spawns *after Pass 1*" does NOT
  discriminate — a legacy dynamic-claims engine also spawns in Pass 2
  (resolution). Reverting the whole static-claims machinery leaves that
  assertion green. The discriminator is **"no spawn during resolution"**:
  the spawn event must order after resolution-complete (i.e. at first
  execute). J8/J9 below re-anchor 4I to that surface.
- **Zero-load binds without Julia:** J9 uses the echo fixture, so the 4I
  property is guarded deno-gated-only; the Julia 4I run is a V-row on top.

### J-rows (durable automated tests)

- **J1 — minimal Julia render (4B).** Tier: integration, julia+deno-gated.
  Unit: the full chain (discovery → static resolution → load/launch/execute →
  jupyter `toMarkdown` → HTML writer) with real Deno + real Julia; no mocks.
  Seam: TempDir project with the committed julia fixture; render the 4B doc
  (`execute: daemon: false`); assert output HTML contains the cell result `2`.
  Revert hunk: delete the `contributes.engines` entry from the fixture
  `_extension.yml` → no engine claims `julia` → render error → RED. (Smoke row:
  it binds the fixture's registration; deeper properties bind in J2–J7.)
- **J2 — cell options via metadata threading (4E).** Tier: integration,
  julia+deno-gated. Unit: 1c.2-P1.1b Rust threading (LANDED,
  `metadata: ctx.metadata.clone()` at `ts_engine.rs:396`) + host
  `metadataAsFormat` + jupyter `toMarkdown` include logic. Seam: doc with
  **document-level frontmatter `execute: echo: false`** (corrected round 3:
  NOT cell-level `#| echo: false`, which travels inside the cell source and
  survives the named revert); assert the HTML contains the cell's output but
  NOT its source listing. Named revert: restore `metadata: HashMap::new()` at
  `ts_engine.rs:396` (= 1c.2's T14 revert — J2 adds Julia-behavior coverage
  on the shared hunk, and reverting it reddens T14 too) → option dropped →
  source listing present → RED. (Discriminator check: assert both halves —
  output-present + source-absent — so "render failed entirely" can't fake a
  pass.) Cell-level `#|` variants are the 4E manual greps, binding
  `toMarkdown`'s cell-option path instead.
- **J3 — exeflags through the julia block (4E).** Tier: integration,
  julia+deno-gated. Unit: P1.1b threading of the `julia:` frontmatter subtree
  → `format.metadata` → serialized options → QuartoNotebookRunner. Seam: doc
  with `julia: exeflags: ["--threads=2"]` — **stop-point: confirm the exact
  frontmatter schema against QNR docs before writing this row** — and a cell
  printing `Threads.nthreads()`; assert `2` in HTML. Named revert: same
  shared T14 hunk as J2 (see note there) → QNR defaults → `1` ≠ `2` → RED.
  (`env` gets the same shape only if cheap; otherwise it's V-2.)
  **(corrected at implementation, 2026-07-02 — controller-ratified, two changes.)**
  As landed (`julia_engine_e2e.rs` J3): (a) the `julia:` block lives in the
  fixture's **`_quarto.yml`**, not document frontmatter — frontmatter string
  values are smart-typography-mangled (`--threads=2` → en dash; substrate bug
  **bd-uf4epv4w**); (b) the named revert is re-anchored to the **project-layer
  merge binding, `metadata_merge.rs:214`** — the stop-point investigation
  found QNR consumes julia-engine's `target.markdown` (q2's post-merge
  serialized AST) as a file-content override, so the T14 wire revert is
  structurally undiscriminating for QNR-observable options (verified
  empirically twice; T14 stays bound by J2). `env` was folded into J3 with
  its own `FOO=BAR` assertion; V-2 not needed. Evidence: 4E task report +
  compat log §11.
- **J4 — error handling (4D).** Tier: integration, julia+deno-gated. Unit:
  host execute error path (`host.ts` error response) → `FromEngine::Error` →
  `ExecutionError` mapping. Seam: render the
  `error("this should fail gracefully")` doc; assert (a) render returns an
  error whose message contains `this should fail gracefully`, (b) q2 does not
  panic, and (c) a subsequent render of the J1 doc through the SAME process
  still succeeds (host not wedged — binds pending-request cleanup). Named
  revert (corrected round 3): the `FromEngine::Error` arm in
  `TsEngineHost::request` (`ts_process.rs:~693`) — `TsEngine::execute` never
  sees an error frame; `request()` converts it to `Err` first → error
  swallowed/typed differently → (a) RED.
- **J5 — figures + supporting in a website render (4C+4H).** Tier:
  integration, julia+deno-gated. Unit: jupyter assets path + `supporting`
  forwarding (`map_execute_result`, `ts_engine.rs:~447`) + project artifact
  copying. Seam: temp website project (extension copied in by setup — nothing
  symlinked, nothing julia-specific committed under `julia-website/`);
  `index.qmd` markdown-only + `plot.qmd` with a Plots cell; assert
  `_site/index.html` and `_site/plot.html` exist, `_site/plot_files/
  figure-html/*.png` exists on disk, and `plot.html` references it via
  `<img src=`. Named revert: the `supporting` forward in
  `map_execute_result` (fn at `ts_engine.rs:~458`, forward at `:~465`)
  → figure file absent from `_site` → RED. (The single-doc 4C render is a
  V-row: `<img>`+file in place binds the assets path but NOT `supporting` —
  single-doc output can pass with the forward reverted; the website copy is
  the discriminating surface.)
- **J6 — one host per project render (4H).** Tier: integration,
  julia+deno-gated (or echo-based if flake-prone — the property is
  engine-agnostic). Unit: registry/host construction in
  `ProjectContext::discover` shared across Pass-2 files. Seam: J5's project
  render with a tracing capture subscriber installed; assert exactly ONE
  `engine_host` spawn event (J8's event) across both files. Named revert:
  move registry+host construction from the single `discover` call into the
  orchestrator's per-file loop → two spawn events → RED.
- **J7 — Julia `launch()` context populated (4H).** NOT a new frozen test —
  the binding lives in 1c.2's T1/T2 echo seams (CONTEXT_JSON). The Julia leg
  is V-3 (observe the LaunchEngine payload via `RUST_LOG`/tracing during the
  J5 render and record `projectDir`/`outputDir` non-empty). Do not duplicate
  the T1 binding with a Julia-gated copy.
- **J8 — spawn observability event (shared seam for J6/J9).** Tier: Rust
  unit (mock-init transport, existing `ensure_started` double-checked-spawn
  tests). Unit: net-new production line `tracing::info!(target:
  "engine_host", pid, "engine-host spawned")` in `ensure_started_inner`
  (beside the `#[cfg(test)]` counter — the counters are NOT visible to
  integration tests, which compile without `cfg(test)`; `is_alive()` is
  production but can't count). Seam: capture subscriber; assert exactly one
  event per real spawn including under the concurrent-spawn Barrier test.
  Named revert: remove the tracing line → J6/J9 captures see zero events →
  RED (and this unit row RED).
- **J9 — zero-load resolution ordering (4I, echo-based, deno-gated only).**
  Tier: integration. Unit: the static-claims early-answer branch in
  `claims_language` (`ts_engine.rs:~550` — corrected round 3) + lazy spawn.
  Seam: temp project with the echo (static-claims) fixture and one
  `{echo}`-cell doc; tracing capture; assert **both events are present**
  (a missing resolution-complete event must FAIL the test, not vacuously
  pass the ordering check) and the `engine_host` spawn event orders AFTER
  the resolution-complete event (net-new INFO event at the end of
  `resolve_engines`, `resolution.rs:~334` — same TDD note as J8) and that
  exactly one spawn occurs. Named revert: remove the static early-answer
  branch (fall through to the dynamic wire call) → spawn precedes
  resolution-complete → RED. This replaces 4I's vacuous "after Pass 1"
  surface; the Julia multi-page run is recorded as V-4 evidence.

### V-rows (manual validations — record invocation + output snippet)

- **V-1 — daemon mode (4E).** Uncertain Julia-side semantics (does oneShot
  avoid the detached server entirely, or only close the file worker?) make a
  frozen assertion premature. Record: transport-file presence after a
  `daemon: false` render vs a `daemon: true` render; second-`daemon: true`-render
  reuse evidence; out-of-band teardown. If the observations are stable,
  promote to a J-row in a follow-up (new seam entry required — this spec is
  frozen, additions only).
- **V-2 — `env` option (4E)** if not folded into J3.
- **V-3 — launch-context payload for Julia (4H)** — see J7.
- **V-4 — Julia multi-page Pass-1/ordering run (4I)** — evidence on top of J9.
- **V-5 — state persistence across cells (4D).** QuartoNotebookRunner-internal
  behavior (cells execute in one `run` request); no q2 hunk to bind. Validate
  and record only.
- **V-6 — Q1 output comparison (4F).** Inherently manual.
- **V-7 — Julia through `q2 preview` (4J; added 2026-07-02, user-requested —
  additive per the freeze rule).** First real-engine validation of 1c-R5's
  native capture → splice preview path (echo/P2-14 is the frozen binding for
  the registry-read hunks; V-7 is evidence, not a regression guard). Record:
  initial capture executes (output `2`), live re-execute on edit (`5`),
  daemon behavior under an interactive session with `daemon: false`
  (transport-file state; note the daemon-true-by-default hazard,
  bd-m1jeqhhz), cleanup verified.

### Accepted-untested (logged, not silently omitted)

- **Import-map parity build (4A):** the discriminating act is the rebundle
  itself (`q2 build-ts-extension` fails if the aliases are missing) — but it
  is network-dependent (jsr fetch), so no committed automated test. Rationale:
  one-time dev-machine step; the committed bundle is the artifact under test
  thereafter.
- **`htmlDependency` dedup into `site_libs` (4H):** conditional on Julia
  emitting an HTML dependency, which the default Plots/gr backend does not do
  deterministically. Left as an if-observed manual check; the dedup mechanism
  itself is upstream `store_html_dependencies` behavior with its own coverage.
- **Daemon-mode behavior (4E):** see V-1 — deliberately not frozen until the
  Julia-side semantics are observed.
- **`_site/site_libs/` shared assets and sidebar/navbar transforms (4H):**
  generic website-epic behaviors with their own coverage upstream; the Julia
  render observes them (recorded), but no Julia-gated row duplicates that
  binding.

## Design Notes

### Debugging approach

The subprocess architecture helps debugging — you can run the Deno engine-host independently. (Corrected 2026-07-02; the original snippet had the wrong entry point and message shape.) The entry point is `src/main.ts` (guarded by `import.meta.main` — `src/host.ts` is the platform-neutral dispatch loop and does nothing when run directly), or the production esbuild bundle `dist/engine-host-deno.js`. Every frame is a newline-delimited `{id, msg}` envelope; `init` carries `global` (a `HostGlobalConfig`), and the engine path goes in a separate `loadEngine` frame:

```bash
# Run engine-host manually for debugging
printf '%s\n%s\n' \
  '{"id":1,"msg":{"type":"init","global":{"resourceDir":"...","runtimeDir":"...","dataDir":"...","isInteractiveSession":false,"runningInCi":false,"quartoVersion":"0.0.0"}}}' \
  '{"id":2,"msg":{"type":"loadEngine","enginePath":"./_extensions/julia-engine/julia-engine.js"}}' | \
  deno run --allow-all ts-packages/quarto-engine-host-deno/src/main.ts
```

You can also add `console.error()` statements in the engine or harness and see them on stderr (the Rust host forwards child stderr to `tracing` target `engine_host`).

### Standard library imports

The Julia engine imports `"path"`, `"fs/exists"`, `"encoding/base64"` from Deno's standard library. Following Quarto 1's approach, these are resolved at **build time** via the import map and inlined into the bundled `.js` file. At runtime, no import resolution is needed (the engine-host runs bundles with `deno run` and no `--config`).

Q1 ships these aliases in `src/resources/extension-build/import-map.json` (`path` → `jsr:@std/path@1.0.8`, `fs/` → `jsr:/@std/fs@1.0.16/`, `encoding/` → `jsr:/@std/encoding@1.0.9/`, plus `path/posix` and `log`). q2's `resources/extension-build/deno.json` currently maps only `@quarto/*` and the `@std/` prefix — restoring the Q1 aliases is a Phase 4A work item, so the engine source bundles unchanged.

The build step for the Julia engine fixture is `q2 build-ts-extension` (which shells out to `deno bundle` with the 4-tier config precedence: `--config` > extension-local `deno.json` > `deno.workspace.json` > shipped `deno.json`):
```bash
cargo run --bin q2 -- build-ts-extension src/julia-engine.ts
```

### CI gating

Julia engine tests use the same mechanism as the echo E2E suite (decided with
the seam spec — no feature flag or nextest tag): a **runtime probe with an
early-return skip**, `deno_available() && julia_available()`, printing
`eprintln!("SKIP: …")` so the skip is visible in test output. On a machine
with both installed they run; a skip there is a signal, not a pass. They run
manually during development and automatically anywhere Julia+Deno are
present.

Note the real environmental prerequisite is more than `julia` in PATH: on
first run `ensure_environment.jl` instantiates the QuartoNotebookRunner
project (network + package downloads), and Phase 4C's `using Plots` is a
heavyweight install. Budget for a slow, network-dependent first render, and
don't let the 10s server-ready timeout / 15-try transport-file poll in
`julia-engine.ts` masquerade as a q2 bug when it's a cold Julia environment.

## Success Criteria

- [x] Julia engine extension discovered and loaded by q2
- [x] Simple Julia code cell executes and produces correct output
- [x] Figure generation works
- [x] Multiple cells with shared state work
- [x] Error handling produces useful messages
- [x] All modifications to julia-engine.ts documented
- [x] Website-project integration: a multi-page project with both
  markdown and Julia pages renders to `_site/`, with Julia figures in
  per-page directories and any Julia HTML dependencies deduped under
  `site_libs/`
- [x] Zero-load resolution: Deno subprocess is not spawned during Pass 1 **or
  during Pass-2 resolution** — spawn orders after resolution-complete, at the
  first execute (J9; reworded round 3 — the old "during Pass 1" phrasing was
  the vacuous surface the seam check retired)
- [x] Julia's `launch()` receives a populated per-render `EngineProjectContext`
  (`project_dir`/`output_dir` non-empty), not `default()` — the outcome of Plan 1c.2 P1.1,
  validated here
- [x] Frontmatter `execute:`/`julia:` options demonstrably reach the engine — the
  outcome of Plan 1c.2 P1.1b, validated by 4E (J2 `echo: false`, J3 `exeflags`
  observable in cell output; daemon-mode *behavior* is V-1 evidence, recorded
  not asserted — its Julia-side semantics are the open observation)
- [x] `Arc<TsEngineHost>` is shared across all files in a project
  render (one Deno PID across N pages)
- [x] No regressions in existing tests
- [x] `cargo xtask verify` passes
