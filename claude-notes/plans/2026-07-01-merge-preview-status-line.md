# Merge the preview executor + capture status bars into one status line

**Strand:** bd-yai4w8ly (task, P2). Discovered from bd-sfet3264
(remote code-execution provider).
**Date:** 2026-07-01.
**Status:** IN PROGRESS — open questions resolved 2026-07-01 (see
"Decisions locked"); implementing per the TDD checklist.

## Decisions locked (user, 2026-07-01)

1. **Palette** — don't bikeshed; a theme overhaul is coming anyway.
   Merge onto one reasonable strip color (using the blue capture tone
   `#eef6ff`, since the bar is mostly about output once you've run).
2. **Stale copy** — show **both** facts: `Showing executed output ·
   code changed` (don't swap the whole label to "Code changed…").
3. **No-executor + capture** — **yes**, still show "Showing executed
   output" + Clear when offline (Clear is executor-independent).
4. **Button order** — **Clear then Run**, right-aligned, so **Run stays
   pinned to the far right** across the natural transitions: no bar →
   executor online `… [Run]` → code runs `… [Clear] [Run]` → and Run
   disappears only when the executor goes away. Action group order is
   `[Clear results…] [Run/Re-run]`.
5. **Component boundary** — **one** `PreviewStatusBar` component.

## Overview

The hub-client preview pane currently shows **two independent status
bars** stacked above the preview iframe whenever code execution is in
play. They were built in separate phases of bd-sfet3264 and never
unified:

- **Bar A — executor / Run** (green, `#eefaf0`). One of:
  - `RunControl` (`src/components/render/RunControl.tsx`) — a green dot,
    a status label (`Executor online` / `Code changed since the last
    run.` / an error), and a **Run** / **Re-run** / **Executing…**
    button. Shown when an executor is online **and** the active doc has
    executable cells.
  - a plain read-only `.executor-online-bar` div (inline in
    `Editor.tsx`) — dot + `Executor online`, no button. Shown when an
    executor is online but the doc has **no** executable cells.
  - nothing, when no executor is online.
- **Bar B — capture / Clear** (blue, `#eef6ff`). `ClearCaptureControl`
  (`src/components/render/ClearCaptureControl.tsx`) — `Showing executed
  output` + a **Clear results…** button with a two-step inline
  confirm. Shown whenever the active doc has a capture entry.

The two are rendered back-to-back in `Editor.tsx:1102-1118`, so in the
common "an executor is online and I just ran the doc" state the user
sees **two stacked strips of different colors** saying closely related
things ("Executor online" + "Re-run" over "Showing executed output" +
"Clear results…").

**Goal:** collapse these into a **single status line** that selectively
shows the executor info when an executor exists, the "showing executed
output" message when a capture exists, and both the Run/Re-run and Clear
buttons as relevant — one strip, one color, one row.

## Current wiring (verified 2026-07-01)

All in `hub-client/`:

| Concern | Location |
|---|---|
| Both bars rendered | `src/components/Editor.tsx:1102-1118` |
| Run affordance | `src/components/render/RunControl.tsx` |
| Read-only executor fallback | inline `.executor-online-bar` div, `Editor.tsx:1108-1113` |
| Clear affordance | `src/components/render/ClearCaptureControl.tsx` |
| CSS | `src/components/Editor.css:279-375` (`.capture-results-bar`, `.executor-online-bar`, `.executor-online-dot`, `.run-control*`) |
| `executorsOnline` source | `App.tsx:115-118,699` — `useExecutionChannel(...).executors.length > 0` |
| `onRequestExecution` source | `App.tsx:700` — `requestExecution` from `useExecutionChannel` → ephemeral `exec/request` broadcast |
| `captures` source | `App.tsx:108,458-459,698` — `onCapturesChange` sync callback |
| `clearCapture` | imported from `@quarto/preview-runtime` (`Editor.tsx:16`); removes the shared `CaptureRef` sidecar entry |
| Executable-cell gate | `src/services/executableCells.ts` — `hasExecutableCells(content)` |

Inputs available at the render site (all already threaded to `Editor`):

- `executorsOnline: boolean`
- `hasExecutableCells(content): boolean`
- `capture = captures?.[currentFile.path]` — a `CaptureRef` or
  `undefined`, carrying `{ captureDocId, state ('idle'|'running'|
  'error'), staleness, lastError }`
- `onRequestExecution(path)` (ephemeral run request)
- `clearCapture(path)` (shared sidecar delete)

Both control components are **presentational** (mutations injected as
props) with their own tests:
`RunControl.integration.test.tsx`,
`ClearCaptureControl.integration.test.tsx`.

## The full state space

The merged bar is a function of two roughly-independent axes:

**Executor axis** (from `executorsOnline` + `hasExecutableCells`):
1. no executor online
2. executor online, doc has **no** executable cells
3. executor online, doc **has** executable cells

**Capture axis** (from `capture`):
a. no capture
b. capture present, `state: idle`
c. capture present, `state: running` (or a local run pending)
d. capture present, `state: error` (+ `lastError`)
e. capture present, `staleness: true` ("code changed")

Today Bar A owns the executor axis + the run-pending/error/stale text;
Bar B owns "showing executed output" + Clear. They overlap on the
running/stale/error signalling (those belong to the capture, but only
`RunControl` renders them, and only when there are executable cells).

## Proposed design — one `PreviewStatusBar`

Replace `RunControl`, the inline `.executor-online-bar`, and
`ClearCaptureControl` with a **single** `PreviewStatusBar` component
that renders **at most one strip**, laid out as:

```
[● dot?]  <status text>                       [Clear results…] [Run/Re-run]
   └ executor-online only        └ left, flex:1      └ right-aligned action group
```

Button order is `[Clear] [Run]` so **Run stays pinned to the far
right** across the natural transitions (decision 4): `… [Run]` when an
executor comes online, `… [Clear] [Run]` after a run, and Run leaves
only when the executor goes away.

### Visibility

Render the bar iff **any** of: an executor is online, or a capture
exists. (i.e. hide only in state 1a — nothing to say and nothing to
do.) This matches today: Bar A shows when executor online, Bar B shows
when capture exists; the union is "either."

### Left side — dot + status text (single precedence chain)

Show the green dot iff `executorsOnline`. Then pick **one** status
message by precedence (busy/error first, so transient states win):

1. **busy** (`state === 'running'` or local pending) → `Executing…`
2. **error** (`state === 'error'`) → `capture.lastError` (red text,
   `role="alert"`)
3. **capture present** → `Showing executed output`
   - if also `staleness` and executor online: append/replace with a
     "code changed since the last run" hint (see open Q2)
4. **executor online, no capture** → `Executor online`

(State 1a is already excluded by the visibility rule, so there is
always something to show.)

### Right side — action group (both buttons, independently gated)

Rendered in DOM order **Clear then Run** so Run is pinned far right
(decision 4):

- **Clear results…** button — shown iff a capture exists. Keeps the
  two-step inline confirm from `ClearCaptureControl`. When the user is
  mid-confirm, the confirm prompt + Clear/Cancel replace the normal
  status text (an `alertdialog`), as today.
- **Run / Re-run** button — shown iff `executorsOnline &&
  hasExecutableCells`. Label: `Executing…` (disabled) while busy, else
  `Re-run` if a capture exists, else `Run`. Same pending-snapshot /
  timeout logic as today's `RunControl` (lift it verbatim).

Both can appear together (executor online + executable cells + a
capture) — that is exactly the doubled-bar case we're collapsing, now
one row: `● Showing executed output    [Re-run] [Clear results…]`.

### Behaviour preserved from the two components

- Run pending-snapshot (`captureDocId` at request time), reset on
  `path` change, cleared on new capture / error / `PENDING_TIMEOUT_MS`
  (30s). — from `RunControl`.
- Clear two-step confirm, reset on `path` change; confirm names the
  collaborator-wide effect. — from `ClearCaptureControl`.
- Run request = `onRequestExecution(path)` (ephemeral); Clear =
  `clearCapture(path)` (shared sidecar delete). Same injected-prop
  shape → same testability.

### CSS

One class (`.preview-status-bar`) replacing `.run-control` /
`.executor-online-bar` / `.capture-results-bar`. Pick a single palette
(open Q1: green vs blue vs neutral). Reuse `.executor-online-dot`.
Right-align the button group (`margin-left:auto` on the action group,
or `flex:1` on the label as today). Keep the red error and
destructive-confirm button styles.

## State → rendering table (the contract to test)

| Executor | Capture | Dot | Status text | Clear btn | Run btn |
|---|---|---|---|---|---|
| offline | none | – | *(bar hidden)* | – | – |
| offline | idle | – | Showing executed output | Clear results… | – |
| offline | error | – | *lastError* (red) | Clear results… | – |
| online, no exec cells | none | ● | Executor online | – | – |
| online, no exec cells | idle | ● | Showing executed output | Clear results… | – |
| online, exec cells | none | ● | Executor online | – | Run |
| online, exec cells | idle | ● | Showing executed output | Clear results… | Re-run |
| online, exec cells | running/pending | ● | Executing… | Clear results… | Executing… (disabled) |
| online, exec cells | error | ● | *lastError* (red) | Clear results… | Re-run |
| online, exec cells | idle + stale | ● | Showing executed output · code changed | Clear results… | Re-run |

(Rows with a capture but `offline`/`no-exec-cells` are reachable: a
capture can outlive the executor that produced it, and non-`.qmd` or
prose-only docs can carry a capture from earlier. Clear must still work
with no executor — it is a pure CRDT mutation.)

## Open questions — RESOLVED (see "Decisions locked" at top)

All five settled with the user 2026-07-01: blue strip; show both facts
for stale; keep "Showing executed output" when offline; button order
`[Clear] [Run]` (Run pinned far right); one component.

## Plan (TDD) — DRAFT, do not start yet

Per CLAUDE.md: tests first, then implement, then verify green, then
end-to-end in a real browser session before declaring done.

- [x] **0 — Lock the open questions** (palette, stale copy,
      no-executor+capture, button order, component boundary) with the
      user. ✅ 2026-07-01 (see "Decisions locked").
- [x] **1 — Component test for `PreviewStatusBar`** (RED). ✅
      `PreviewStatusBar.integration.test.tsx` — 15 cases encoding the
      state→rendering table (dot/label/buttons per row) + ported
      behaviors: run pending clears on new capture / error / timeout /
      path change; two-step confirm calls `onClear`; cancel and
      path-change disarm it. Confirmed RED before the component existed.
- [x] **2 — Implement `PreviewStatusBar`** ✅ single component
      (`PreviewStatusBar.tsx`); a small inner `StatusLabel` owns the
      label precedence. `RunControl` + `ClearCaptureControl` deleted.
      GREEN (15/15).
- [x] **3 — Rewire `Editor.tsx`** ✅ replaced the three-way Bar A block
      + `ClearCaptureControl` with one `<PreviewStatusBar>` fed
      `executorsOnline`, `hasExecutableCells(content)`, the active
      `capture`, `onRun`, `onClear`. Inline `.executor-online-bar` gone.
- [x] **4 — CSS** ✅ added `.preview-status-bar` (+ `.preview-status-*`
      label/actions/buttons); removed `.run-control*`,
      `.executor-online-bar`, `.capture-results-bar`. Kept
      `.executor-online-dot`. Blue palette; Run pinned right via a
      `margin-left:auto` action group.
- [x] **5 — Delete/retarget old tests** ✅ old test files removed; the
      only remaining `RunControl`/`ClearCaptureControl` mentions are
      documentation comments.
- [x] **6 — Build + suite** ✅ `npm run build:all` green (strict
      `tsc -b` + vite); full suite green (unit 685 / integration 103 /
      wasm 126).
- [x] **7 — End-to-end** in a real browser ✅ 2026-07-01 (see
      "End-to-end evidence" below). Drove the full status-bar lifecycle
      against the local Option-B harness; the merged single-row bar
      renders and transitions correctly in every state.
- [x] **8 — Changelog** ✅ two-commit workflow: code `0b13dbcb`, then
      `hub-client/changelog.md` entry `1eb0183e` under 2026-07-01.

**Status: implementation complete on `feature/hub-execution-provider`
(commits `0b13dbcb` + `1eb0183e`).** All 8 checklist items done; TS
suite + strict build green; browser-verified. Not pushed (awaiting
approval). A full `cargo xtask verify` should run before the eventual
push, though this change is hub-client-only (TS/CSS); no Rust touched.

## End-to-end evidence (2026-07-01)

Per CLAUDE.md's end-to-end rule. Stood up the local Option-B harness
(`claude-notes/hub-execution-e2e/`): a no-auth `q2 hub --project`
(index id `HLDXSUAS7RBv9HPGYrvLTifK3sc`), the hub-client dev server
pointed at it (`VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:3031 npm run
dev`), and a connected executor (`q2 provide-hub --server
ws://127.0.0.1:3031 --allow-all --token dev
HLDXSUAS7RBv9HPGYrvLTifK3sc` → "Execution ENABLED"). Opened the Share
URL in a real browser and exercised the **single merged status bar**:

1. **online + executable cells + no capture** → one blue row:
   `● Executor online … [Run]` (dot present, Clear absent).
2. **click Run → capture arrives** → one row:
   `● Showing executed output … [Clear results…] [Re-run]`. This is
   exactly the state that previously showed **two stacked bars**; it is
   now a single strip, with **Clear before Run** and **Run pinned to
   the far right** (decision 4).
3. **click "Clear results…"** → confirm state replaces the status text:
   `● Clear executed output? This removes it for all collaborators
   until the document is run again. [Clear] [Cancel]` (red Clear).
4. **click Clear** → capture removed → back to state 1
   (`● Executor online … [Run]`), Clear gone, "Re-run" → "Run".

Every transition matched the state→rendering table. Observed directly
in the browser (screenshots captured in the session transcript).

**Orthogonal observation (not this change):** the executor's computed
value did not visibly splice into the preview cell in this harness (the
`{python}` `2 + 3` cell still rendered as source, no `5`), even though
the sidecar arrived and the bar correctly read "Showing executed
output". That is the capture-**consumption/splice** path (bd-sfet3264
Phase 1), untouched by this status-bar merge, and the parent plan notes
the in-browser splice was never verified ("the manual last mile", 1G /
4b notes). Flagged for the user; out of scope for bd-yai4w8ly.

## References

- Parent feature plan: `claude-notes/plans/2026-06-29-remote-execution-provider.md`
  (Phases 1F, 2D, 4b built the two bars).
- Local e2e harness: `claude-notes/hub-execution-e2e/`.
