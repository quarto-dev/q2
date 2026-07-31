# Pre- and post-render project scripts (bd-w348iu63)

**Status: implemented 2026-07-31 (all phases complete, full verify
green) — awaiting commit approval.**

Port Quarto 1's `project.pre-render` / `project.post-render` script support to
Quarto 2. Strand: `bd-w348iu63`.

## Overview

Quarto 1 lets a project declare scripts that run before and after a project
render:

```yaml
project:
  type: website
  pre-render: prepare.py          # string or list
  post-render:
    - cleanup.R
    - tools/notify.sh
```

Scripts receive a `QUARTO_PROJECT_*` environment-variable contract, run with
the project root as cwd, and can (pre-render only) mutate the project — add
input files, edit `_quarto.yml` — with the project re-read afterward.

### Correction to the session premise

Q1's feature is **not** restricted to website projects. `pre-render` /
`post-render` live in the generic `project` schema
(`src/resources/schema/project.yml:44-51` in quarto-cli) and
`renderProject` runs them for every project type with zero type branching.
So "lift the website restriction" is already the Q1 baseline; the port keeps
that. The genuinely new design work in Q2 is: where the hooks fit in a
pipeline whose file set is fixed at discovery time, what the interpreter
dispatch looks like without a bundled Deno, and what preview/WASM do.

## Q1 behavior assessment (what we're porting)

Full investigation notes: see the session that produced this plan. Summary of
the contract, with file references into `external-sources/quarto-cli`:

### Configuration

- `project.pre-render`, `project.post-render`: `maybeArrayOf: string`
  (string is normalized to a one-element list at read time,
  `src/project/project-context.ts:280-290`).
- Scripts can also arrive via included metadata files and via extensions of
  type `metadata` (paths resolved relative to the extension dir,
  `src/extension/extension.ts:920-940`). Arrays concatenate on merge.

### Execution point and frequency

All in `src/command/render/project.ts` (`renderProject`):

- Pre-render scripts run near the top of `renderProject` (`:309-368`), before
  any file renders. Afterward Q1 **re-reads the whole project context**
  (`:341-346`) — `_quarto.yml`, metadata includes, re-globbed input list —
  and **recomputes the render list** (`:359-367`), so files created by a
  pre-render script get rendered in the same pass and show up in navigation.
- A mutation guard then forbids three changes relative to the pre-script
  config (`:84-107`, `:348-357`): `project.type`, `project.output-dir`, and
  the project dir itself. Violation aborts the render.
- Post-render scripts run at the very end (`:831-861`), after the
  project-type's internal `postRender` hook, with the list of produced
  output files.
- **Scripts run once per `renderProject` invocation — including incremental
  ones.** `quarto render single-file.qmd` inside a project still runs both
  script sets (comments in Q1 claiming incremental gating are stale). The
  scripts distinguish full vs partial renders via `QUARTO_PROJECT_RENDER_ALL`,
  which is set to `"1"` only when the whole project is being rendered.
- A single-file render *outside* any project never runs scripts.

### Environment contract

Env is merged into (not replacing) the parent environment; cwd is the
project root. Both pre and post get:

| Var | Value |
|---|---|
| `QUARTO_PROJECT_DIR` | absolute project dir (set process-wide in Q1) |
| `QUARTO_PROJECT_OUTPUT_DIR` | absolute output dir (= project dir if no `output-dir`) |
| `QUARTO_PROJECT_RENDER_ALL` | `"1"` iff rendering all inputs, else **absent** |
| `QUARTO_PROJECT_SCRIPT_PROGRESS` | `"1"`/`"0"` (multi-file render and not quiet) |
| `QUARTO_PROJECT_SCRIPT_QUIET` | `"1"`/`"0"` (from `--quiet`) |

Pre-render only: `QUARTO_PROJECT_INPUT_FILES` — newline-separated paths of
the files about to render, relative to the project dir.
Post-render only: `QUARTO_PROJECT_OUTPUT_FILES` — newline-separated output
paths relative to the project dir (e.g. `_site/index.html`).

Escape hatch for env-size limits (Q1 issue #10828): if the user sets
`QUARTO_USE_FILE_FOR_PROJECT_INPUT_FILES=<path>` (resp. `..._OUTPUT_FILES`),
the list is written to that file instead of the env var.

Known Q1 wart: the shared env is computed once before pre-render and reused
for post-render, so `RENDER_ALL` can be stale if a pre-render script changed
the input set. We should compute the post-render env fresh.

### Interpreter dispatch (Q1)

Each entry is parsed shell-ish (split on spaces, double quotes honored), so
`pre-render: python3 tools/gen.py --flag` works. Dispatch is **by extension
of the first token**:

- `.ts`/`.js` → bundled **Deno** (`--allow-all`, import map for deno_std)
- `.py` → discovered Python (`py`/jupyter python/`python3`)
- `.r`/`.R` → `Rscript` (honors `QUARTO_R`)
- `.lua` → run as a **pandoc Lua filter** (`pandoc --from markdown --to plain --lua-filter script`)
- anything else → direct `exec` of the token, no shell wrapper (a `.sh`
  needs an executable bit + shebang)

### Failure handling (Q1 — bad, don't copy)

Non-zero exit throws an *empty-message* `Error`; the user sees only the
script's own stderr. Remaining scripts don't run; pre-render failure aborts
before rendering; preview catches the error and keeps serving.

### Preview (Q1)

Project preview runs both script sets on the initial render **and again on
every file-change re-render** (no incremental guard) — a known perf/behavior
surprise, combined with the mandatory project re-read.

## Q2 landscape (where this lands)

- `q2 render` CLI: `crates/quarto/src/commands/render.rs` —
  `execute_single_doc` (`:688`) / `execute_project` (`:748`); both do
  `ProjectContext::discover()` → `ProjectPipeline::new()` → `run()`.
- Orchestrator: `crates/quarto-core/src/project/orchestrator.rs` —
  `ProjectPipeline::run()` (`:861`) runs pass 1, then the *internal*
  `ProjectType::pre_render` hook (`:884`), pass 2, internal `post_render`
  (`:916-927`). **Note**: these internal Rust hooks are unrelated to user
  scripts and must stay unrelated (website uses `post_render` for
  sitemap/favicon).
- Config: `crates/quarto-core/src/project/mod.rs` — `ProjectConfig`
  (`:313`), `parse_config` (`:607-681`). No schema/validation layer exists;
  unknown keys are ignored silently.
- Subprocess infra: `SystemRuntime::exec_command`
  (`crates/quarto-system-runtime/src/traits.rs:353-371`) exists but takes
  **no cwd and no env** — insufficient as-is. Engines (knitr, jupyter)
  instead use raw `std::process::Command` in native-gated modules; the
  closest analogue to "run a user script portably" is
  `crates/pampa/src/json_filter.rs` (incl. a Windows shebang workaround).
- **Q2 currently exports zero `QUARTO_*` env vars to subprocesses** — the
  whole `QUARTO_PROJECT_*` contract is net-new.
- Preview: `q2 preview` does not run `ProjectPipeline` natively; the browser
  WASM renders per-active-page, the native side records engine captures
  (`crates/quarto-preview/src/lib.rs`, hooks at `:208` on-ready and `:246`
  on-file-changed). Subprocesses are only possible on the native side.
- Prior art in plans: `claude-notes/plans/2026-03-16-extensions-grand-plan.md`
  Phase 7 lists pre/post-render scripts as future extension-contributed
  work. No existing braid strand covers user scripts.
- Naming collision to keep out of docs/errors: `"pre-render"`/`"post-render"`
  are also **filter entry-point sentinels** in
  `crates/quarto-core/src/filter_resolve.rs:31-44`.

## Design

### D1. Hook placement: around the pipeline, in a shared helper

The key structural mismatch: Q2 fixes the project file set at
`ProjectContext::discover()` time, and `ProjectPipeline::run()` runs pass 1
before any hook fires. Pre-render scripts must be able to create input
files. So the scripts cannot run inside the pipeline as it stands.

**Decision**: run the scripts in the *drivers*, bracketing discovery and the
pipeline, via a shared native-only module `quarto-core::project::render_scripts`:

```
execute_project / execute_single_doc (render.rs), publish.rs:
  1. locate _quarto.yml, parse config          (existing find_project_config/parse_config)
  2. run pre-render scripts                    (new)
  3. ProjectContext::discover()                (existing — sees script-created files)
  4. validate: type/output-dir unchanged       (new, Q1-compatible guard)
  5. ProjectPipeline::run()                    (existing, untouched)
  6. run post-render scripts                   (new, env computed fresh from step 5 outputs)
```

Because scripts run **before** the full discovery, we get Q1's
"re-read the project after pre-render" semantics for free — there is only
one authoritative read. Step 4 re-checks the two Q1-forbidden mutations
(`project.type`, `project.output-dir`) by comparing the step-1 parse with a
re-parse at step 3, with a proper diagnostic (Q1's error here has a typo and
its general script-failure error is empty — we do better).

The internal `ProjectType::pre_render`/`post_render` Rust hooks are not
touched, and `ProjectPipeline::run()` itself is not modified. This keeps the
WASM pipeline path completely unaffected.

Cost of this placement: each native driver (render, publish, later preview)
wires the calls explicitly. That's two call sites today and is the honest
shape — script execution is a native, filesystem-level concern, same tier as
`NativeRuntime::with_cache_dir` wiring that already lives in the drivers.

Wrinkle: `QUARTO_PROJECT_INPUT_FILES` must describe the render set *after*
pre-render mutation in Q1 — actually no: Q1 passes the *pre-mutation* list
(computed before scripts run) and recomputes the render list afterward only
for rendering. We match Q1: compute the input list from a cheap pre-script
discovery (step 1 can reuse `ProjectContext::discover()`; it's not
expensive), pass it to the scripts, then re-discover at step 3.

### D2. Config surface

- `project.pre-render`, `project.post-render`: string or list of strings,
  exactly Q1's shape. Parsed in `parse_config` into two new
  `ProjectConfig` fields `pre_render_scripts` / `post_render_scripts`
  (each `Vec<RenderScript>` carrying the YAML `source_info`, following the
  `project_resources::RawResourcePattern` pattern, so errors point at the
  YAML entry).
- Available to **all** project kinds, single-file synthetic projects
  excluded (matching Q1: no project ⇒ no scripts; `is_single_file`
  contexts created from a bare `q2 render file.qmd` with no `_quarto.yml`
  never run scripts).
- Typo guard: since Q2 has no schema layer, add a targeted diagnostic for
  `project.pre_render` / `project.post_render` (underscore variants) —
  cheap and catches the likely mistake.
- Extension-contributed scripts: **out of scope** (extensions Phase 7,
  extensions grand plan); the config plumbing should not preclude it.

### D3. Interpreter dispatch — simplified from Q1

Keep Q1's "parse shell-ish command line, dispatch on first token's
extension" model, minus the parts Q2 cannot honor:

| Extension | Q1 | Q2 proposal |
|---|---|---|
| `.py` | discovered python | `python3`/`python` on PATH (honor `QUARTO_PYTHON` if set) |
| `.r`, `.R` | Rscript | Rscript via existing knitr discovery conventions (`QUARTO_R`) |
| `.ts`, `.js` | bundled Deno + import maps | **`node` from PATH only** (`QUARTO_NODE` override, same convention as the `q2 mcp` launcher's node lookup). No deno lookup, no import maps — Q1's Deno import-map scheme was a misguided stdlib-stability attempt we deliberately do not carry forward. `.ts` therefore only works where node can run it; the documented recommendation for anything else is an explicit interpreter in the command line. |
| `.lua` | pandoc filter (!) | **not special-cased** (decided; a future mlua-based runner is a possible follow-up strand if demand appears) |
| other | direct exec | direct exec, no shell; Windows shebang caveat documented (json_filter.rs precedent) |

The command line is parsed with double-quote support (port of Q1's
`parseShellRunCommand`), so `pre-render: python3 tools/gen.py --flag`
works and sidesteps extension dispatch entirely — that stays the documented
recommendation for anything unusual.

### D4. Environment contract — Q1-compatible, computed fresh per phase

Export exactly Q1's variables (table above), with these fixes:

- Post-render env computed **after** the render from actual results
  (fresh `RENDER_ALL`, real output-file list from the pipeline's
  `output_paths`), fixing Q1's staleness wart.
- `QUARTO_PROJECT_DIR` set per-subprocess (not process-wide like Q1).
- Keep the `QUARTO_USE_FILE_FOR_PROJECT_{INPUT,OUTPUT}_FILES` escape hatch —
  small, and real projects hit env-size limits (Q1 #10828).
- Paths relative to project dir, newline-separated, matching Q1 so existing
  user scripts port unchanged.

Mechanism: extend nothing on `SystemRuntime` (avoids touching ~15 test
stubs for a native-only feature); the runner module uses
`std::process::Command` directly with `.current_dir(project_dir)` and
`.envs(...)`, native-gated at module level like
`engine/knitr/subprocess.rs`. If a future consumer needs script exec through
the runtime trait, that's a separate refactor.

### D5. Failure handling — better than Q1

- Non-zero exit → abort with a real diagnostic: script entry (with YAML
  source location), exit code, pointer that the script's own stderr appears
  above. Registered as a `Q-*` code in `quarto-error-catalog`.
- Remaining scripts in the list do not run (Q1-compatible).
- Script stdout/stderr inherit by default; under `--quiet`, capture stdout
  (Q1 behavior) but **still pass stderr through** on failure.

### D6. Frequency semantics — Q1-compatible

Scripts run on every project-scoped render (`FullProject` and `Subset`,
including `q2 render some-file.qmd` resolving into a project), once per
invocation. `QUARTO_PROJECT_RENDER_ALL=1` only for full renders. No
incremental gating — scripts that care use the env var, exactly as in Q1.

### D7. Preview and WASM

- **`q2 preview` (native side)**: run pre-render scripts **once at server
  boot only** (alongside `record_eager_captures` in the on-ready hook).
  No re-runs — not on file edits, not on `_quarto.yml` changes (decided;
  restart the preview to re-run scripts). This is a deliberate improvement
  over Q1's every-keystroke re-run; the browser-side per-page render makes
  Q1's behavior impossible to match anyway. Post-render scripts do not run
  in preview (there is no materialized output dir in the preview loop).
  Deviations surfaced in docs.
- **Hub/WASM (browser preview, hub-client)**: scripts cannot run. If a
  project declares them, surface a one-time diagnostic warning through the
  existing `DiagnosticMessage` channel rather than failing.
- Phase-gated: preview integration is its own phase and can ship after the
  render/publish support.

## Resolved design questions (Carlos, 2026-07-29)

1. **Mutation guard strictness** — keep Q1's ban: pre-render scripts may
   not change `project.type` or `project.output-dir` (they already received
   `QUARTO_PROJECT_OUTPUT_DIR`, so a change would hand them a stale value).
2. **`.ts`/`.js` support** — look up **`node` only** on PATH (with
   `QUARTO_NODE` override, mirroring the `q2 mcp` launcher). No deno
   lookup; explicitly do not reproduce Q1's Deno import-map scheme.
3. **`.lua` scripts** — drop Q1's pandoc-filter dispatch; no special case.
   An mlua-based runner is a possible follow-up strand if demand appears.
4. **Preview cadence** — pre-render scripts run **on preview boot only**;
   no re-runs of any kind (not even on `_quarto.yml` change). No
   post-render in preview. Documented deviation from Q1.
5. **CLI escape hatch** — yes, add `--no-render-scripts` to `q2 render`.
6. **Naming** — `pre-render`/`post-render` spellings verbatim; no aliases.

## Work items

### Phase 0 — design sign-off
- [x] Resolve open questions 1–6 with Carlos; update this plan
      (resolved 2026-07-29; see "Resolved design questions" above)
- [x] Explicit go-ahead from Carlos to begin execution (2026-07-31)

### Phase 1 — tests first (TDD)
- [x] CLI e2e integration tests in
      `crates/quarto/tests/integration/render_scripts_cli.rs` (14 tests;
      verified failing before implementation, 13/14 red on 2026-07-31):
      pre-render creates input; post-render OUTPUT_FILES; env contract
      full vs subset; failing script aborts (exit code, stderr
      pass-through, later scripts skipped); output-dir + type mutation
      guards; string/list forms + ordering; explicit-interpreter command
      line with quoted args; `--no-render-scripts`; escape hatch;
      underscore typo warning; no-project render
- [x] Cross-platform fixture strategy: Python with graceful skip
      (`require_python!`), `#[cfg(unix)]` shell + `#[cfg(windows)]`
      batch variants for direct-exec
- [x] Unit tests for command-line parsing (quote handling), extraction,
      typo guard, mutation guard, catalog registration (17 tests in
      `render_scripts.rs`)

### Phase 2 — config parsing
- [x] `RenderScript` type with `source_info`; `ProjectConfig::pre_render_scripts`
      / `post_render_scripts`; extraction in `parse_config`
      (`crates/quarto-core/src/project/mod.rs`)
- [x] String-or-list normalization; underscore-typo diagnostic (Q-5-11,
      emitted by `underscore_typo_diagnostics`, printed by the render
      driver)

### Phase 3 — script runner
- [x] `crates/quarto-core/src/project/render_scripts.rs`: command-line
      parser + config extraction (target-agnostic), exec half native-gated
      (`#[cfg(not(target_arch = "wasm32"))] mod exec`); extension dispatch
      (.py → QUARTO_PYTHON/python3, .r → knitr `find_rscript`, .ts/.js →
      QUARTO_NODE/node, else direct exec resolved against project dir);
      env assembly; catalog entries Q-5-8 (script failed), Q-5-9
      (forbidden mutation), Q-5-10 (launch failure), Q-5-11 (typo)
- [x] `QUARTO_USE_FILE_FOR_PROJECT_{INPUT,OUTPUT}_FILES` escape hatch

### Phase 4 — driver wiring
- [x] `execute_project` in `crates/quarto/src/commands/render.rs`:
      discover → run pre-render → re-discover → mutation guard →
      pipeline → post-render (fresh env from `summary.outputs`,
      post-render only after a successful render, Q1-compatible).
      `execute_single_doc` needs no wiring — `RenderTarget::SingleDoc`
      only fires with no surrounding `_quarto.yml`, so no scripts exist.
- [x] Same bracket in `crates/quarto/src/commands/publish.rs`
      (`ProjectPublishRenderer::render`; post-render runs before the
      sidecar walk so script-added output files get published)
- [x] `--no-render-scripts` flag on `q2 render`

### Phase 5 — preview + WASM
- [x] Native preview: pre-render at boot only, inside the on-ready
      spawn_blocking *before* `record_eager_captures` (scripts may
      generate data the engines read); failure is reported but the
      preview keeps serving. TDD test
      `quarto-preview::integration render_scripts_boot` (verified red
      first) also pins "no re-run on file change".
- [x] WASM/hub-client: one-time (AtomicBool) Q-5-12 warning pushed
      into the `warnings` channel of
      `render_project_active_page_to_response` when scripts are
      configured
- [x] Full `cargo xtask verify` (WASM leg touched via quarto-core) —
      all 14 steps green 2026-07-31 (first run flagged clippy
      `map_unwrap_or` / unnested-or-patterns, fixed; second run
      failed on a stale `node_modules` unrelated to this feature,
      fixed with `npm install` from the repo root)

### Phase 6 — verification + docs
- [x] End-to-end verification per CLAUDE.md (2026-07-31, output
      inspected): fixture with `pre-render: gen_news.py` (creates
      `news.qmd` from `QUARTO_PROJECT_INPUT_FILES`) and
      `post-render: python3 report.py --label "site build"`.
      `q2 render` printed:
      ```
      Running pre-render script: gen_news.py
      Rendering project: …/e2e-scripts (type: website)
      Rendered 2 of 2 files to …/e2e-scripts/_site
      Running post-render script: python3 report.py --label "site build"
      ```
      `_site/news.html` contains "Generated from 1 inputs.";
      `report.txt` contains `label=site build`, `render_all=1`, and
      both `_site/*.html` paths. A failing script produces the Q-5-8
      ariadne diagnostic pointing at `_quarto.yml:4:15`
      (`pre-render: gen_news.py`) with the exit status.
- [x] User-facing docs page `docs/guides/projects/scripts.qmd`
      (sidebar-linked; rendered cleanly with
      `cargo run --bin q2 -- render docs/guides/projects/scripts.qmd`
      after `cargo xtask stage-doc-examples`)
- [x] Close out: `cargo build --workspace` clean;
      `cargo nextest run --workspace` 10806 passed / 0 failed;
      `cargo xtask verify` all steps passed; `cargo xtask lint` clean
      (all 2026-07-31)

## Q1 → Q2 behavior differences (running list for docs)

| Area | Q1 | Q2 (decided) |
|---|---|---|
| `.ts`/`.js` | bundled Deno + import maps | `node` from PATH only (`QUARTO_NODE` override) |
| `.lua` | pandoc Lua filter | no special case |
| Post-render env | stale (computed pre-render) | fresh |
| Script failure msg | empty `Error` | sourced diagnostic + exit code |
| Preview cadence | scripts on every re-render | pre-render at boot only; no post-render in preview |
| Extension-contributed scripts | supported (1.5+) | deferred to extensions Phase 7 |
| Skip flag | none | `--no-render-scripts` |
