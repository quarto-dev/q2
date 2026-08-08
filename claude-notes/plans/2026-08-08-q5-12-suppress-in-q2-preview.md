# Suppress the Q-5-12 render-scripts warning in `q2 preview`

**Strand:** bd-pq72bplh (caused-by bd-w348iu63) — closed 2026-08-08
**Status:** done — PR #472 merged to main (`958d331f`); strand closed.

## Overview

`q2 preview` of a project that configures `project.pre-render` /
`project.post-render` scripts shows the "Render Warning" overlay:

> [Q-5-12] Project render scripts do not run in the hub preview — This
> project configures `project.pre-render` / `project.post-render` scripts,
> which cannot run in the browser. The preview renders without them; use
> `q2 render` on a machine with the interpreters installed to run the
> scripts.

That message is correct for the **hub-client** preview (browser-only, no
subprocesses) but false for **`q2 preview`**, whose native host *does* run
pre-render scripts once at server boot (design decision D7 of
`claude-notes/plans/2026-07-29-pre-post-render-scripts.md`). Goal: stop
showing Q-5-12 in `q2 preview` while keeping it in the hub preview.

## Reproduction (2026-08-08, verified end-to-end)

Scratch project: `_quarto.yml` with `project.type: website` +
`project.pre-render: pre.sh`; `pre.sh` writes `pre-ran.txt`; one
`index.qmd`.

```
cargo run --bin q2 -- preview <scratch-dir> --port 7799 --no-browser
```

Observed (output inspected directly):

- Terminal prints `Running pre-render script: pre.sh` and `pre-ran.txt`
  appears on disk — **the script ran**.
- Chrome at `http://127.0.0.1:7799/?page=index.qmd` shows the ⚠ Warning
  pill; expanding it shows exactly the Q-5-12 text quoted above
  (inspected via devtools a11y snapshot of the `PreviewDiagnosticsOverlay`).

## Root cause

One shared WASM function serves two hosts with different script semantics:

- **Warning emitter:** `render_project_active_page_to_response`
  (`crates/wasm-quarto-hub-client/src/lib.rs:1598`) unconditionally appends
  `render_scripts_unsupported_diagnostic(&project.config)`
  (`src/lib.rs:1776` / builder at `:1858`) whenever the project config has
  any pre/post-render scripts. Once-per-session gating via a static
  `AtomicBool`.
- **Hub-client** reaches it through `render_page_in_project` /
  `render_page_in_project_with_attribution` (`src/lib.rs:1121`, `:1184`).
  Here the warning is *correct*: the browser cannot run the scripts.
- **`q2 preview`** reaches it through `render_page_for_preview`
  (`src/lib.rs:1290`) — the entry point the q2-preview SPA calls
  exclusively (`q2-preview-spa/src/PreviewApp.tsx:1062`). Here the warning
  is *wrong*: the native server already ran pre-render scripts at boot
  (`run_boot_pre_render_scripts`, `crates/quarto-preview/src/lib.rs:301`,
  invoked from the on-ready hook at `:223`), and their outputs are on disk
  and synced into the preview VFS before any page render.

Script-semantics truth table (per D7, all deliberate):

| Host | pre-render | post-render |
|---|---|---|
| `q2 render` | every invocation | every invocation |
| `q2 preview` | once at boot (restart to re-run) | never (no materialized output dir in the preview loop) |
| hub preview | never | never |

So in `q2 preview` the only true statement is "post-render scripts don't
run" — and post-render has no observable effect in the preview loop
anyway, since nothing consumes an output dir.

The existing `prefer_preview_format: bool` parameter happens to correlate
with the caller today (`true` only from `render_page_for_preview`), but it
means "apply the q2-preview format-default substitution", not "the host
runs render scripts" — and a hub document could in principle reach the
`pipeline_kind == "preview"` branch via an explicit format. Overloading it
would conflate two meanings; we should thread explicit host information
instead.

## Design options

### Option A (recommended): thread host context, suppress for `q2 preview`

Add an explicit host parameter to the shared path and gate the warning on
it. Sketch:

1. In `crates/wasm-quarto-hub-client/src/lib.rs`, add a small enum:

   ```rust
   /// Which application is driving this WASM render. Decides
   /// host-dependent diagnostics (Q-5-12: the q2-preview native host
   /// runs pre-render scripts at boot; the hub browser cannot).
   #[derive(Clone, Copy, PartialEq, Eq)]
   enum RenderHost {
       HubClient,
       NativePreview,
   }
   ```

2. `render_project_active_page_to_response` takes `host: RenderHost`;
   `render_page_in_project_with_attribution` passes `HubClient`,
   `render_page_for_preview` passes `NativePreview`. (The single-doc path
   needs no parameter — single-file projects have no `_quarto.yml`, hence
   no scripts, and the warning is only emitted on the project path.)

3. The warning push becomes:

   ```rust
   if host == RenderHost::HubClient {
       if let Some(diag) = render_scripts_unsupported_diagnostic(&project.config) { ... }
   }
   ```

4. For testability, split the existing function: a **pure** builder
   `fn render_scripts_unsupported_diagnostic_pure(config) -> Option<DiagnosticMessage>`
   (no static), wrapped by the existing once-per-session AtomicBool gate.
   Unit tests target the pure part plus a host-gating helper
   (`fn should_warn_render_scripts(host, config) -> bool`), avoiding
   cross-test interference from the static.

No message text, catalog entry, or hub behavior changes. The `q2 preview`
user story is covered natively: the terminal prints
`Running pre-render script: …` at boot, failures print with a
"restart to re-run" note, and the deviations are documented per D7.

### Option B (alternative): preview-specific milder note

Instead of full suppression, `NativePreview` gets a different, accurate
info-level note — e.g. only when `post_render_scripts` is non-empty:
"post-render scripts do not run in `q2 preview`; they run in `q2 render`"
(new catalog code Q-5-13). Rejected as the default because post-render has
no observable effect in the preview loop (nothing consumes the output
dir), so the note is noise; and pre-render staleness ("edited `pre.sh`,
preview didn't re-run it") is better handled someday by the native side,
which actually knows when the scripts last ran. Noted here so we can
revisit if users get confused by boot-only cadence.

### Option C (rejected): filter Q-5-12 in the q2-preview SPA

Have `PreviewApp.tsx` / `PreviewDiagnosticsOverlay` drop warnings with
code Q-5-12. Wrong layer: the engine knows which host it serves; the SPA
would be string-matching a code to undo an upstream mistake, and the
incorrect warning would still sit in the `RenderResponse` for any other
consumer.

## Work items (Option A, TDD)

### Phase 1 — tests first

- [x] Unit tests for the host-gating decision. **Placement change from
      the original sketch:** `crates/wasm-quarto-hub-client` is
      `exclude`d from the workspace (root `Cargo.toml:12`), so tests
      there would never run under `cargo nextest run --workspace`.
      The pure decision fn + `RenderHost` enum therefore live in
      `quarto-core::project::render_scripts` (alongside the other
      Q-5-x diagnostics), tests in its `unsupported_diagnostic` test
      module: hub+pre → warn; hub+post-only → warn; native-preview +
      any combination → silent; no scripts → silent for both hosts.
      Verified red first (E0432 on the not-yet-existing API), then
      green after implementation (4/4 pass).
- [x] WASM-level regression test
      `hub-client/src/services/renderScriptsWarning.wasm.test.ts`
      (vitest, real WASM): `render_page_for_preview` → no Q-5-12;
      then `render_page_in_project` → exactly one Q-5-12 (proving the
      suppressed path doesn't consume the once-gate); then again → none
      (once-per-session). Added after the Rust red/green cycle, so its
      red state was not observed against the pre-fix WASM; the Rust
      unit test carried the TDD burden.

### Phase 2 — implementation

- [x] `RenderHost { HubClient, NativePreview }` + pure
      `render_scripts_unsupported_diagnostic(host, config)` in
      `quarto-core::project::render_scripts` (message text unchanged).
      WASM crate keeps only the once-per-session AtomicBool wrapper
      (`render_scripts_unsupported_once`) and threads
      `host: RenderHost` through
      `render_project_active_page_to_response`
      (`render_page_in_project_with_attribution` → `HubClient`,
      `render_page_for_preview` → `NativePreview`).
- [x] `cargo build --workspace` clean; targeted nextest green;
      `cd hub-client && npm run build:wasm` clean (direct
      `cargo check --target wasm32-unknown-unknown` fails in cc on
      tree-sitter C parsers — the npm script is the sanctioned build).
- [ ] `cargo nextest run --workspace` (full run; scheduled with the
      Phase 3 verification).

### Phase 3 — end-to-end verification (both hosts)

- [x] Rebuilt the preview chain: `cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`
      (dist WASM timestamp confirmed fresh before the binary rebuild).
- [x] Re-ran the reproduction end-to-end (2026-08-08):
      `cargo run --bin q2 -- preview <scratch-dir> --port 7799
      --no-browser` on the same scratch project. Observed and
      inspected directly: terminal printed
      `Running pre-render script: pre.sh`; `pre-ran.txt` recreated on
      disk; page rendered in Chrome; devtools a11y snapshot shows
      **no** "⚠ Warning" button (previously present at the same spot),
      a `wait_for("Warning")` probe timed out, and a screenshot shows
      a clean page.
- [x] Hub path: Q-5-12 still fires — verified by executing the real
      WASM through the hub entry point (`render_page_in_project`) in
      `renderScriptsWarning.wasm.test.ts`: exactly one Q-5-12 warning
      on first render, none on second (once-gate). **Not** verified in
      a live hub browser session (a local-prod + Automerge project
      setup was judged not worth the cost given the WASM-level test
      hits the identical code path).
- [x] `cargo nextest run --workspace`: 11069 passed, 0 failed.
- [x] Full `cargo xtask verify` (no skips): all steps passed
      (2026-08-08). `cargo clippy -p quarto-core` clean.

### Phase 4 — wrap-up

- [x] Record the end-to-end invocation + observed output in this plan
      (see Phase 3 items and the Reproduction section).
- [x] Committed as `87393d27` on
      `bugfix/bd-pq72bplh-q5-12-preview-warning`; PR
      https://github.com/quarto-dev/q2/pull/472 (changelog entry
      skipped per Carlos — test-only hub-client change).
- [x] CI green on the PR: all 8 checks pass (test suites on
      ubuntu/macos across both workflows, WASM Tests, Hub-Client E2E,
      Snyk license/security), 2026-08-08.
- [x] `braid close bd-pq72bplh` (PR #472 merged as `958d331f`).

## Open questions for review

1. **Suppress entirely vs Option B's post-render note** — recommendation:
   suppress entirely (Option A); revisit B only on user confusion.
2. **Enum vs bool** — recommendation: the two-variant `RenderHost` enum
   (self-documenting at call sites, room for future host-dependent
   diagnostics); a `host_runs_render_scripts: bool` would also do.
3. Should the Q-5-12 catalog `message_template` stay as-is? It already
   says "the browser-based hub preview", which remains accurate for the
   only host that will still emit it. Recommendation: leave untouched.
