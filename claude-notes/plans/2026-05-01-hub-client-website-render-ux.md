# Hub-client website rendering UX

## Overview

Hub-client uses the same Pass-1 (profile sweep) / Pass-2 (active-page
render) orchestrator as `quarto render`, but its UX for two failure
modes is much worse than the CLI's, and one third bug surfaces as
"the sidebar is gone." Concretely, on
`examples/websites/08-hub-preview/`:

1. **Parse error in `about.qmd` is misattributed.** When `about.qmd`
   has a syntax error, opening `index.qmd` shows
   `Sidebar references unknown document 'about.qmd'` and
   `Body link references unknown document 'about.qmd'`. The file
   exists. The real cause is that Pass-1 dropped it, so it is
   missing from the `ProjectIndex` that Pass-2's nav transforms
   consult. The CLI shows the same warnings — but the CLI *also*
   prints `warning: profile-pass skipped about.qmd: Error: …` with
   the actual diagnostic, so a user can connect the two. Hub-client
   surfaces only the misleading downstream warning.

2. **Parse error on the active page renders as a generic "no
   output" string.** Navigating to `about.qmd` itself shows the
   preview banner *Project render produced no output for the active
   page*. The actual parse diagnostic — present in
   `summary.pass1_failures[i].diagnostics` — is dropped on the
   floor. The CLI version of this same failure prints the full
   ariadne-style snippet with line/column markers.

3. **Sidebar is missing from the hub-client preview** even when
   the render succeeds (e.g., on `index.qmd` after the parse error
   is fixed). Cause not yet confirmed; needs in-browser repro. See
   "Bug 3" below for hypotheses.

We also need a small enabler so future debugging (mine and the
user's) is less click-heavy: a console-callable JS API on
`window` to read/write the active hub-client project's files.

Tracking: `bd-0tr6` (websites epic) is the parent. This plan adds
four children.

## Code-path inventory (gathered before writing this plan)

### Pass-1 / Pass-2 orchestration
- `crates/quarto-core/src/project/orchestrator.rs:498-584` — `pass_one()` runs `profile_with_cache()` per file; failures collected into `summary.pass1_failures: Vec<FileFailure>`; Pass-2 only runs for files that succeeded Pass-1.
- `crates/quarto-core/src/project/orchestrator.rs:284-287` — `FileFailure { input, error: String, diagnostics: Vec<DiagnosticMessage> }`. Note: **the diagnostics are already preserved**; nobody is consuming them.

### Where the misleading "unknown document" warning comes from
- `crates/quarto-core/src/transforms/navigation_href.rs:88-96` — emits `"{tag} references unknown document '{path}'"` whenever `ProjectIndex::get_profile(target)` returns `None`. Source-labeled by `sidebar_render.rs:161` etc. The warning has no field saying *why* the profile is missing.

### CLI emits the parse error; hub-client doesn't
- `crates/quarto/src/commands/render.rs:580-586` — CLI iterates `summary.pass1_failures` and prints `warning: profile-pass skipped {input}: {error}` plus diagnostics.
- `crates/wasm-quarto-hub-client/src/lib.rs:1374-1428` — WASM analogue. **Never reads `summary.pass1_failures`.** Path on active-page-failure goes:
  - L1381: `summary.outputs.next()` is `None` (active page was dropped in Pass-1).
  - L1386: `summary.pass2_failures.next()` is also `None` (Pass-2 was skipped, not failed).
  - L1393: returns the literal string `"Project render produced no output for the active page"`.

### Hub-client surface
- `hub-client/src/components/render/Preview.tsx:126-265` — calls WASM `renderToHtml`, sets error overlay with `{ message, diagnostics }`, forwards warnings to parent via `onDiagnosticsChange`.
- `hub-client/src/components/render/PreviewErrorOverlay.tsx:1-64` — renders `error.diagnostics?: Diagnostic[]`; each diagnostic shows `title`, `start_line`, `problem`. **No `source_file` field** — the overlay can't tell the user which file's parse failure caused a project-scoped warning.
- `hub-client/src/components/render/DoubleBufferedIframe.tsx:338,346` — preview is an iframe with `srcDoc={html}`. The WASM-returned HTML is the **full** HTML document including sidebar markup. So bug 3 is *not* an extract-body bug.

### `RenderResponse` wire shape
- `crates/wasm-quarto-hub-client/src/lib.rs:692-704`:
  ```
  { success: bool, error?: string, html?: string,
    diagnostics?: JsonDiagnostic[], warnings?: JsonDiagnostic[] }
  ```
  `JsonDiagnostic` has no `source_file` field today.

### Existing window-globals in hub-client
- None (greenfield for the debug API). Verified — no `(window as any).foo = …` or `globalThis.quarto*` assignments anywhere under `hub-client/src/`.

## Bug 1 — Parse error misattributed as "unknown document"

### Root cause
A file that fails Pass-1 is excluded from `ProjectIndex.profiles()`.
Pass-2's sidebar/body-link href transforms see "no profile for
`about.qmd`" and emit
`Sidebar references unknown document 'about.qmd'`. The *real*
warning (`profile-pass skipped about.qmd: <diagnostic>`) lives on
`summary.pass1_failures[i].diagnostics` and never crosses the WASM
boundary.

### Fix shape (proposed)
1. Surface Pass-1 failures across the WASM boundary. Add Pass-1
   failure entries to `RenderResponse.warnings` (or a sibling
   `pass1_failures` array — see Q1 below) so the JS layer can show
   them with the same severity treatment as project-scoped
   warnings. Each entry needs: `source_file`, `error_summary`, and
   `diagnostics: JsonDiagnostic[]` (the rich form).
2. Tag `JsonDiagnostic` with an optional `source_file: string`.
   Right now diagnostics are positional within a single page's
   source map; project-scoped warnings have no provenance. Adding
   `source_file` lets the overlay say "from `about.qmd`" instead
   of just listing a free-floating warning.
3. (Stretch) When `navigation_href.rs` emits the
   "unknown document" warning, check whether the missing target
   is one of the Pass-1 failures, and if so, emit a
   *different* message — something like
   `Sidebar entry 'about.qmd' was skipped because it failed to
   parse; see the parse error below.` This avoids the "the file
   exists, why does it say unknown?" confusion entirely. Requires
   threading a `pass1_failed_paths: HashSet<...>` through the
   transform context.
4. PreviewErrorOverlay grows a section that lists Pass-1 failures
   with the same diagnostic formatting (line/column, ariadne
   snippet) it already uses for the active page's own errors.

### Tests
- WASM unit test: render a project where `about.qmd` is malformed,
  active page is `index.qmd`. Assert the response contains a
  Pass-1 failure entry for `about.qmd` with non-empty
  `diagnostics`.
- TypeScript test: render the same fixture through `Preview.tsx`
  and snapshot the overlay DOM; assert it shows the parse error
  with the `about.qmd` source attribution.
- (If we do Stretch 3) Rust unit test in `navigation_href.rs`:
  given a `pass1_failed_paths` set, the warning text changes.

## Bug 2 — Parse error on the active page shows generic message

### Root cause
`render_project_active_page_to_response` (lib.rs:1325-1429) checks
`pass2_failures` but never checks `pass1_failures`. When the
active page is itself a Pass-1 failure, both `summary.outputs` and
`summary.pass2_failures` are empty for it, so the L1393 fallback
fires.

### Fix shape (proposed)
In the L1381 "no output" branch, **before** falling through to the
generic message:
- look up `summary.pass1_failures` for an entry whose `input`
  matches the active path,
- if found, build a `RenderResponse` with `success: false`, the
  error string set to the failure's summary, and
  `diagnostics: failure.diagnostics` mapped to `JsonDiagnostic`s
  (using the same source-context lookup the success path uses, so
  line/column anchoring works in the overlay).

This is the same routing logic the CLI uses, just on the WASM
side.

### Tests
- WASM unit: malformed active page; assert the response contains
  the diagnostic with the right line/column and `success: false`.
- TS overlay test: the parse error renders with ariadne-style
  formatting in `PreviewErrorOverlay`.

### Note: Bug 1 ⊃ Bug 2 (mostly)
Bug 1's "surface Pass-1 failures across the WASM boundary" already
gives Bug 2's UI most of what it needs. The Bug 2 work is the
narrow active-page bookkeeping — picking the right entry to put
in `error` vs `warnings`. Worth keeping it as a separate ticket
because the test surfaces are different.

## Bug 3 — Sidebar missing from hub-client preview

### Status: needs in-browser repro

The transforms run unconditionally (`sidebar_render.rs:71-143`,
no `RenderMode` gate), the iframe receives the full HTML document
via `srcDoc`, and the template puts the sidebar inside a CSS-grid
`<div id="quarto-content">` next to `<main>`. So the bug is not
"the sidebar markup is gone." Likely candidates, ordered by
plausibility:

1. **Bootstrap CSS / theme CSS not loaded in the iframe.** The
   sidebar grid layout and visual treatment depend on Bootstrap +
   Quarto's theme bundle. If `link[href="/.quarto/..."]` entries
   resolve to empty content (or don't get post-processed by
   `useIframePostProcessor`), the sidebar element exists but
   collapses to invisible / unstyled. Diagnostic:
   `document.querySelector('.sidebar.sidebar-navigation')` from
   the iframe console; check `getComputedStyle` for the grid
   container.
2. **Project not detected → render falls through to single-doc
   path.** `render_page_in_project` falls through to the
   single-doc branch when no `_quarto.yml` ancestor is found. If
   project discovery is racing with VFS hydration on first load,
   we'd silently render without the website project type. Check
   the WASM logs for "Discovered project at …" and confirm
   `ProjectContext::discover` succeeds.
3. **`auto:` sidebar contents starve.** The example uses an
   explicit `contents:` list, so `SidebarGenerateTransform` is
   not the failure path here. Including this as a known
   non-suspect.

### Plan
- Reproduce in Chrome via the dev tools plugin against
  `http://localhost:5173/#/p/.../file/index.qmd`.
- Inspect the iframe DOM. Decide which hypothesis above is true.
  *Then* write the fix and a regression test (likely a hub-client
  smoke test that asserts the rendered HTML for a website project
  contains a `.sidebar` element with at least one nav-item link).

## Enabler — Hub-client console debug API

### Goal
Let an agent (or a developer in DevTools) read and write files in
the active project without going through the UI. Cuts the loop
for reproducing bugs like the three above.

### Proposed surface (sketch — see Q4)

```ts
// Available as window.quartoDebug when a project is loaded.
window.quartoDebug = {
  project: () => ProjectInfo,           // id, name, file paths
  listFiles: () => string[],             // qmd + assets
  readFile: (path: string) => string,
  writeFile: (path: string, contents: string) => Promise<void>,
  rerender: () => Promise<void>,         // force re-render of active page
  getActiveFile: () => string,
  setActiveFile: (path: string) => void,
  // Diagnostics:
  lastRenderResponse: () => unknown,     // the raw RenderResponse JSON
  vfsList: (prefix?: string) => string[],
  vfsRead: (path: string) => Uint8Array | null,
};
```

Wiring: a single `useEffect` in the top-level `App` (or a small
`debugApi.ts` service) that mutates `window.quartoDebug` when a
project is mounted, and clears it on unmount. `writeFile` should
go through the same Automerge mutation path the editor uses, so
sync is exercised. `rerender` should invalidate the same caches a
keystroke would.

### Constraints
- Dev-only by default (`if (import.meta.env.DEV)`), to avoid
  shipping a write surface in production builds. Or gate behind a
  query string flag (`?debug=1`) — see Q5.
- Don't bypass auth checks / project ownership.
- Mutations should be observable through the existing presence /
  Automerge sync, so changes are visible to other connected
  clients (and to me, when I'm scripting changes between
  re-renders).

### Tests
- Unit test for the API service (mocking the storage layer).
- One smoke test that calls `writeFile`, awaits the next render,
  and asserts the new content is reflected.

## Phasing

We can land these out of order. Suggested order (pending answers
to questions below):

1. **Enabler** first — a few hundred lines of TS, no Rust changes,
   and it makes me much faster on the next two.
2. **Bug 2** (active-page parse error) — small, contained change
   in `lib.rs:1381`, plus a little overlay work. High UX impact.
3. **Bug 1** (cross-page misattribution) — needs the
   `JsonDiagnostic.source_file` field plumbed through and
   overlay treatment. Medium-sized.
4. **Bug 3** (sidebar) — start with repro and DOM inspection;
   real fix scope unknown until then.

## Resolved decisions

(Originally drafted as open questions; resolved in conversation
on 2026-05-01.)

**D1 — Pass-1 failures get a dedicated `pass1_failures` field.**
The orchestrator already produces `summary.pass1_failures` with
rich diagnostics; only the external surfaces (CLI render, WASM
`RenderResponse`) flatten it. We will:

- Add a dedicated `pass1_failures` field to `RenderResponse` with
  per-entry `{ source_file, error, diagnostics }`. Do not fold
  them into `warnings`.
- Keep the engine **policy-free**. Strict-vs-lenient is a
  consumer choice:
  - **`quarto render` (CI / headless): strict.** Any
    `pass1_failures` entry causes a non-zero exit. Today the CLI
    string-matches warnings to surface these — that goes away.
  - **`quarto preview` / hub-client (interactive): lenient.**
    Render the active page if Pass-2 produced output for it;
    render a failure overlay if Pass-1 dropped *that* page;
    surface `pass1_failures` for *other* pages as
    attributed warnings alongside whatever did render. Partial
    progress is preserved.
- Document the strict-vs-lenient contract in the document-profile
  contract doc (`claude-notes/designs/document-profile-contract.md`)
  so future consumers (e.g., the hub-client-based `quarto preview`
  binary) inherit it.
- Side benefit: the CLI's existing
  `warning: profile-pass skipped …` line gets the same
  diagnostic-rich treatment in CI logs that hub-client will get
  in the overlay.

The work fans out beyond the original Bug-1 scope — `quarto render`
strictness is in scope here, since it shares engine code with the
preview path. Adding a sibling beads issue to track the CLI
behavior change.

**D2 — `navigation_href.rs` wording.** Proximal change: replace
`"references unknown document '{path}'"` with
`"missing document information for '{path}'"` (slightly more
indicative of an error, less misleading when the file actually
exists). The richer pass1-failure-aware rewrite (e.g., "skipped
because it failed to parse") is a separate, future task; not
under this plan.

**D3 — Bug 3 splitting.** If reproduction shows the cause lives
in a different subsystem (CSS pipeline, VFS hydration, project
discovery race), the fix moves to its own beads issue and
session. `bd-f5yi` stays as the investigation/repro ticket.

**D4 — Debug API binary support.** `writeFile` accepts
`string | Uint8Array`. The JS shim handles conversion to
whatever the Automerge layer expects; callers that already hold
a `Uint8Array` for a binary asset don't need to round-trip
through base64. Switch-projects is out of scope (manipulate the
currently-loaded project only); presence/collaborator hooks are
out of scope.

**D5 — Debug API gating.** Dev-only by default
(`import.meta.env.DEV`), with a manual prod escape hatch
(`localStorage.setItem('quartoDebug', '1')`). Documentation TODO:
add a short section to the hub-client README (or wherever
hub-client developer docs live — confirm during implementation)
describing the API surface, the gating, and how to enable it in
prod for one-off debugging.

**D6 — Naming.** `window.quartoDebug`.

## Work items

- [x] Resolve open questions (see Resolved decisions section)
- [x] **Enabler** (`bd-2rv8`): Hub-client console debug API
  - [x] Add `debugApi.ts` service with the agreed surface (incl.
        `string | Uint8Array` for `writeFile`)
  - [x] Wire into `App` so it mounts/unmounts with the active project
  - [x] Gate behind `import.meta.env.DEV ||
        localStorage.getItem('quartoDebug') === '1'` (D5)
  - [x] Document the API + gating in hub-client developer docs
  - [x] Tests for read/write/rerender paths
- [x] **Bug 2** (`bd-mwtf`): Surface active-page Pass-1 errors in WASM response
  - [x] Extend `FileFailure` with structured `diagnostics` +
        `source_context` (extracted from `QuartoError::Parse`).
        Both `pass_one` and `pass_two` use the same helper.
  - [x] WASM: in `render_project_active_page_to_response`, look up
        `summary.pass1_failures` for the active path before falling
        through to the generic "no output" message; surface
        diagnostics via a new `pass_failure_response` helper.
  - [x] Native integration test (`render_page_in_project.rs`):
        malformed active page produces a structured Pass-1 failure
        with non-empty `diagnostics` and `source_context`. The
        rendered ariadne snippet is preserved in `failure.error`
        for the CLI's text path.
  - [x] TS overlay test (`PreviewErrorOverlay.integration.test.tsx`):
        diagnostics array drives line-number + title + problem
        rendering when expanded.
- [x] **Bug 1** (`bd-rqba`): Surface cross-page Pass-1 errors with attribution
  - [x] Add dedicated `pass1_failures` field to `RenderResponse` (D1)
  - [x] Add `source_file?: string` to `JsonDiagnostic` (for project-scoped warnings)
  - [x] WASM: emit non-active Pass-1 failures into `RenderResponse.pass1_failures`
        via the new `pass1_failures_to_json` helper; tag inner diagnostics
        with their source file
  - [x] PreviewErrorOverlay renders `pass1Failures` as a separate
        section with source-file attribution; new CSS rules in `Editor.css`.
        Banner message ("Sibling page 'X' failed to parse") populated by
        Preview on a successful active-page render.
  - [x] `navigation_href.rs` D2: `"references unknown document"` →
        `"references missing document information for"`. No snapshot
        breakage in workspace tests (8166/8166 pass).
  - [x] Native test: malformed sibling produces a structured
        `pass1_failures` entry alongside successful active render,
        and confirms the new D2 wording is in place.
  - [x] TS overlay tests: pass1Failures section renders with
        attribution, and falls back to the raw error string when
        structured diagnostics are absent.
- [ ] **CLI strictness** (new sibling — file beads ticket):
        `quarto render` exits non-zero on any `pass1_failures`
        entry; remove any string-matching of warning text; document
        the strict-vs-lenient contract in
        `claude-notes/designs/document-profile-contract.md`. (D1)
- [ ] **Bug 3** (`bd-f5yi`): Sidebar missing in hub-client website preview
  - [ ] Reproduce in Chrome via dev tools plugin; capture DOM + computed styles
  - [ ] If cause is in a different subsystem, split fix into its own ticket and session (D3)
  - [ ] Fix + regression test
