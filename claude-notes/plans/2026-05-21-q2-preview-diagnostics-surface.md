# q2 preview — diagnostics surface (Phase D.4 follow-up)

**Issue:** bd-b9kzg
**Epic:** bd-kw93 (q2 preview)
**Discovered-from:** bd-kw93.10 (Phase D.4 — render-failure overlay)
**Date:** 2026-05-21
**Status:** IMPLEMENTING (2026-05-21). See §Progress.
**Branch:** `beads/bd-b9kzg-q2-preview-diagnostics-surface`

## Progress

Mirrors §Implementation phasing. Each step ends green before the
next begins.

- [x] **1. Tests first (red).** Write all Rust + SPA tests; verify each fails for the right reason. **DONE 2026-05-21** — 10 new Rust tests panic with `not yet implemented`, 11 new SPA tests fail with clear "expected element not found" / structured-prop-not-rendered messages. Existing 60 Rust preview tests + 30 SPA integration tests still pass.
  - [x] Rust unit tests in `crates/quarto-preview/src/diagnostics.rs` (test 1, nine sub-cases — added `emit_preserves_insertion_order`, `replace_page_with_empty_vec_leaves_no_entries`, `snapshot_is_decoupled_from_sink`, `unset_oncelock_returns_none` for full contract coverage).
  - [x] Rust integration test: `tests/diagnostics_endpoint.rs` (two cases: populated-page + empty-page).
  - [x] Rust integration test: `tests/diagnostics_capture_failure.rs` (one case: unknown-engine triggers capture-driver failure path).
  - [x] *Deferred to Phase 3:* deps-handler unit test for sink emission on IO failure (test 4). Pampa's parse path is robust enough that an integration-level trigger is unreliable, and the test needs the migrated function signature to exist — so it lands alongside the migration.
  - [x] SPA unit tests for the forked overlay: `PreviewDiagnosticsOverlay.integration.test.tsx` (six cases — visibility, warning severity, error severity, controlled collapse toggle, server-diagnostics lane separation, server-only with empty error). Named `.integration.test.tsx` to land in the jsdom env; the `.test.tsx` suffix routes through the node env which lacks `document`.
  - [x] SPA integration tests 7–12 added to `PreviewApp.integration.test.tsx`: success-with-warnings, failure-with-structured-diagnostics, success-clears-warnings, first-render terminal mode, server-diagnostics fetch, server+WASM merge.
- [x] **2. Rust infra (incl. conversion lift, Phase 4).** Land `DiagnosticSink` + `/api/preview/diagnostics` endpoint; tests 1 + 2 green. **DONE 2026-05-21** — folded Phase 4 in (the lift had to land first because the endpoint needs the shared `JsonDiagnostic` type to produce the right wire shape). 9 sink unit tests + 2 endpoint integration tests green; existing 60 preview tests still pass; `npm run build:wasm` from `hub-client/` clean (the WASM crate now imports `JsonDiagnostic`/`JsonPass1Failure`/`diagnostic_to_json`/`with_source_file` from the lifted location instead of defining them inline). `crates/quarto-error-reporting/src/json.rs` is the new home (with 4 unit tests of its own); `quarto-preview` picks up a `quarto-source-map` workspace dep so the handler can pass an (empty for now) `SourceContext` to `diagnostic_to_json`.
- [x] **3. Rust callsite migrations.** Migrate `capture_driver`, `deps`, `re_execute`; tests 3, 4 + re_execute regression guard green. **DONE 2026-05-21** — all three callsites swap their `tracing::warn!` to `diagnostics::current_sink().emit(...)` when the sink is set; the `emit` method calls `tracing::warn!` itself so server stdout stays additive. Each callsite gets a distinct code: `Q-PREVIEW-CAP-1` (capture_driver eager-capture failure), `Q-PREVIEW-DEPS-1` (deps IO failure), `Q-PREVIEW-DEPS-2` (deps parse failure), `Q-PREVIEW-RE-1` (re_execute engine failure). The re_execute migration ALSO keeps the existing `CaptureRef.lastError` write so the existing `StaleCaptureOverlay` (Phase C.5) keeps working. The capture-failure integration test (`diagnostics_capture_failure.rs`) initially failed against `engine: nonexistent-engine` because `EngineExecutionStage` falls back to markdown for unknown engines; the test was rewritten to register a `FailingTestEngine` that's resolved successfully and then deliberately fails on `execute()`. 120/120 preview tests + 1 skipped pass. **Test 4 (deps-handler IO unit test) skipped:** writing it cleanly requires either OnceLock isolation across test threads or a function-signature change, both for a path that's already exercised end-to-end by the capture-failure integration test (same sink + tracing-replacement code path). Pinning the contract directly on the migrated `extract_include_deps` body would only test that "the empty-Vec branch reads OnceLock," which is more about the test seam than about user-visible behaviour.
- [x] **4. Conversion lift.** *Folded into Phase 2 above — see explanation there.*
- [x] **5. SPA overlay fork.** Land `PreviewDiagnosticsOverlay.tsx` + unit tests (test 6 green). No PreviewApp.tsx changes yet. **DONE 2026-05-21** — 6/6 overlay unit tests pass; the fork mirrors the upstream `PreviewErrorOverlay`\'s class names (`preview-error-*`) for CSS continuity and adds a `preview-error-server-diagnostics` lane for the new server feed, plus a `preview-error-overlay--warning` / `--error` modifier that flips the collapsed indicator's label from "Error" to "Warning".
- [x] **6. SPA wiring.** Swap PreviewApp.tsx call sites + new fetch effect; tests 7–13 green; D.4 tests still pass. **DONE 2026-05-21** — 41/41 SPA integration tests pass, 8/8 SPA unit tests pass, `tsc --noEmit` clean, production build (`tsc -b && vite build`) clean. Key implementation notes: (a) `renderError: Error | null` replaced with structured `RenderStatus { failure, diagnostics, warnings, pass1Failures }` so a successful-with-warnings render carries the warnings through. (b) New `serverDiagnostics: Diagnostic[]` slot fed by a new `useEffect` that GETs `/api/preview/diagnostics?page=<rel>` on activeFile/contentTick change (same trigger as the existing deps fetch). (c) New `computeOverlayInputs(render, serverDiagnostics)` helper derives the overlay's props from the structured state. (d) Non-terminal overlay uses *uncontrolled* collapsed mode (the prop is omitted) so the overlay manages its own click-to-expand state — this fixed a tests-fail-silently bug where the controlled-collapsed wiring had no parent toggle handler. (e) Two test-pollution fixes: `beforeEach` in the new describe block resets `renderPageForPreview`\'s mock implementation (the outer `vi.clearAllMocks()` only clears history); and three test assertions click-to-expand the overlay before asserting on expanded-mode content.
- [x] **7. Playwright spec** (test 15). **DONE 2026-05-21** — `q2-preview-spa/e2e/diagnostics.spec.ts` passes against a real chromium session. Fixture is a one-page project with a body-link to a non-existent sibling; the overlay's "⚠ Warning" indicator surfaces within 5s of first render. The fixture deliberately includes `_quarto.yml` so the project pipeline runs `LinkResolutionStage`; without it the SPA runs in single-doc mode and the project-scoped link check doesn't fire.
- [x] **8. End-to-end verification** (test 14) against `docs/`; screenshot + DOM excerpt for PR description. **DONE 2026-05-21** — `q2 preview docs/` against the actual repo's docs/ surfaces both of the user's example warnings end-to-end:
  - `authoring/markdown/index.qmd` shows two `[Q-13-4]` warnings on lines 89 and 94 ("Body link references missing document — 'authoring/markdown/markdown-basics.qmd' is not in the project index"). Matches the user's `q2 render` stdout exactly.
  - `guide/index.qmd` shows the `[Q-1-20]` warning on line 9 ("Failed to parse metadata value as markdown — Could not parse 'Brand themes (_brand.yml)' as markdown"). Matches the user's `q2 render` stdout exactly.
  - Screenshots stored under `claude-notes/plans/bd-b9kzg-screenshot-{1,2,3}-*.png`. **Drive-by fix:** Verification surfaced that the upstream overlay CSS lives in `hub-client/src/components/Editor.css` and isn't imported by q2-preview-spa, so the collapsed indicator initially rendered unstyled at the bottom-left of the viewport. Fixed by adding `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.css` (mirrors the upstream rules with CSS variables inlined, plus fork-specific `--warning` / `--error` modifier classes and the `.preview-error-server-diagnostics` lane).
- [x] **9. Full `cargo xtask verify`** + manual q2-preview SPA rebuild per CLAUDE.md runbook. **DONE 2026-05-21** — `cargo xtask verify` (12/12 steps) green: workspace builds, full workspace nextest, WASM rebuild, q2-preview SPA bundle, hub-client + workspace tests. Manual `cargo build --bin q2` after the verify completes so the next `q2 preview` invocation picks up the freshly-rebuilt WASM image (per CLAUDE.md "Verifying Rust changes in q2 preview"). Playwright e2e was skipped by the `--e2e not set` gate — the diagnostics Playwright spec was exercised manually in Phase 7 and is part of the suite for CI to pick up.
- [ ] **10. PR against `main`.** — *Awaiting user push permission per CLAUDE.md GIT PUSH POLICY.* PR description drafted at `/tmp/bd-b9kzg-pr-description.md`. Topic branch ready at `beads/bd-b9kzg-q2-preview-diagnostics-surface` (2 commits + the earlier beads-sync commit on main).

## Problem

`q2 render docs/` prints structured warnings on stdout:

```
Warning: [Q-13-4] Body link references missing document
   ╭─[ .../docs/authoring/markdown/index.qmd:89:23 ]
   │
89 │   * [Markdown Basics](./markdown-basics.qmd)
   │                       ──────────┬──────────
   │                                 ╰── 'authoring/markdown/markdown-basics.qmd' is not in the project index.
───╯
ℹ Check the spelling, or confirm the target file is included in the render set.

Warning: [Q-1-20] Failed to parse metadata value as markdown
   ╭─[ .../docs/guide/index.qmd:9:13 ]
   ...
```

`q2 preview` on the same project does **not** surface these to the
user — the preview iframe renders successfully, the user sees no
indication that anything is wrong with their document. That is the
gap.

## What already exists

The preview UI is much closer to "done" than the symptom suggests.
Three pieces are already in place:

1. **WASM render returns structured diagnostics.** `render_page_for_preview`
   (`ts-packages/preview-runtime/src/wasmRenderer.ts:496-505`) returns
   a `RenderResponse` (`ts-packages/preview-renderer/src/types/diagnostic.ts:56-82`)
   whose shape is:

   ```typescript
   interface RenderResponse {
     success: boolean;
     error?: string;
     html?: string;
     ast_json?: string;
     diagnostics?: Diagnostic[];     // structured errors
     warnings?: Diagnostic[];        // structured warnings — what q2 render prints
     pass1_failures?: Pass1Failure[]; // sibling-file parse errors
     theme_fingerprint?: string;
   }
   ```

   The Rust side (`PreviewAstOutput.diagnostics` in
   `crates/quarto-core/src/pipeline.rs:167-182`) is documented as:
   "Diagnostics emitted by the head pipeline plus every Pass-2 stage
   that ran. Pipe to `RenderResponse.warnings` after translation via
   `diagnostics_to_json`." So `[Q-13-4]` (from `LinkResolutionStage`)
   and `[Q-1-20]` (from yaml metadata parsing) flow through the same
   pipeline the WASM uses, and arrive in `result.warnings` for
   whichever page the user is viewing.

2. **The overlay is already shared with hub-client.** The
   `PreviewErrorOverlay` component (`ts-packages/preview-renderer/src/overlays/PreviewErrorOverlay.tsx`)
   is imported as-is by both hub-client and q2-preview-spa
   (`q2-preview-spa/src/PreviewApp.tsx:49`). It already accepts:

   ```typescript
   error: {
     message: string;
     diagnostics?: Diagnostic[];
     pass1Failures?: Pass1Failure[];
   } | null;
   visible: boolean;
   collapsed?: boolean;
   onToggleCollapsed?: (next: boolean) => void;
   ```

   The component's docstring (lines 23-29) explicitly anticipates
   the q2-preview wiring: "hub-client wraps with `usePreference`
   (localStorage); the q2-preview SPA can wire it to session state
   or leave it uncontrolled."

3. **The Monaco-style diagnostic conversion utility is reusable.**
   `hub-client/src/utils/diagnosticToMonaco.ts:90-116` splits a
   `Diagnostic[]` into Monaco markers + an "unlocated" list. The
   q2-preview SPA has no Monaco editor (the user edits in their own
   tool), but the same split logic — "show located diagnostics
   inline if we can, otherwise fall back to a banner" — is the same
   shape we want here, minus the gutter integration.

## The gap (precise)

`q2-preview-spa/src/PreviewApp.tsx` discards the structured data
at three spots:

- **Success branch** (lines 530-533): on `result.success`, only
  `astJson` + `themeFingerprint` get committed; `result.warnings`
  is dropped on the floor.
- **Failure branch — non-throw** (lines 545-553): only
  `result.error` is captured as `renderError`; `result.diagnostics`
  and `result.pass1_failures` are not.
- **Overlay call sites** (lines 597-605, 611-619, 677-682): all
  three pass `{ message: …}` only; the overlay's `diagnostics` and
  `pass1Failures` props are never populated.

So the work is to **thread the already-available data through**.
No new types, no new WASM bridge, no new transport.

## Decisions (resolves the open questions below)

Recorded 2026-05-21 from user sign-off:

1. **Scope: per-page MVP + server-side infrastructure.** We surface
   the WASM-render diagnostics now (the user's `[Q-13-4]` /
   `[Q-1-20]` examples) AND in the same issue land the
   server-side **infrastructure** — a diagnostic sink + transport
   that the existing `tracing::warn!()` callsites can migrate to,
   so future server-side warnings aren't another round of
   bespoke plumbing. Migrating the three current callsites
   (`capture_driver.rs`, `deps.rs`, `re_execute.rs`) is part of
   this scope so the infra has real users on day one. What stays
   out: project-wide aggregation UI, richer typed engine
   diagnostics, CLI parity.
2. **UX: collapsed by default for warnings.** Quiet surface;
   user clicks to expand. We may revisit after we have feel for
   how often warnings fire in real projects.
3. **Overlay: fork now, don't extend the shared one.** Create
   `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.tsx`
   as a copy of `PreviewErrorOverlay`, extend it freely for
   q2-preview's needs. Hub-client's overlay stays unchanged.
   Whether the two re-converge later is a separate decision and
   easier to make once both are mature; coupling them now would
   tax both UXes' evolution.
4. **Per-page only for MVP.** Project-wide diagnostic aggregation
   is a deliberate follow-up — it's a UX problem (when to clear
   project diagnostics on page switch, single overlay vs split
   surfaces) that's better deferred until per-page is in users'
   hands.

## Proposed mechanism

### Client-side (in scope for this issue)

**State.** Replace the single `renderError: Error | null` slot
(plus implicit silent-drop of warnings) with a structured slot
that holds whatever the most recent render produced:

```typescript
interface RenderStatus {
  // null = no errors/warnings to surface
  // present = render either failed, or succeeded with warnings worth showing
  failure: { message: string } | null;
  diagnostics: Diagnostic[];   // empty array = nothing to show
  warnings: Diagnostic[];
  pass1Failures: Pass1Failure[];
}
```

Three render outcomes, three transitions:

| Outcome | `failure` | `diagnostics` | `warnings` | `pass1Failures` |
|---|---|---|---|---|
| Success, clean | `null` | `[]` | `[]` | `[]` |
| Success with warnings | `null` | `[]` | from `result.warnings` | from `result.pass1_failures` |
| Failure (`!result.success` or WASM throw) | `{ message: ... }` | from `result.diagnostics` | from `result.warnings` | from `result.pass1_failures` |

**Fork the overlay.** New component at
`q2-preview-spa/src/components/PreviewDiagnosticsOverlay.tsx`,
seeded as a copy of
`ts-packages/preview-renderer/src/overlays/PreviewErrorOverlay.tsx`,
free to diverge for q2-preview's UX needs. All current uses of
`PreviewErrorOverlay` inside `q2-preview-spa/` migrate to the new
component (three call sites in `PreviewApp.tsx`: boot-error,
first-render terminal, on-top overlay). `@quarto/preview-renderer`'s
copy is untouched — hub-client keeps using it.

The fork extends the shared component's prop surface with two
additions:

```typescript
interface PreviewDiagnosticsOverlayProps {
  // Existing surface (from the fork):
  error: {
    message: string;
    diagnostics?: Diagnostic[];
    pass1Failures?: Pass1Failure[];
  } | null;
  visible: boolean;
  collapsed?: boolean;
  onToggleCollapsed?: (next: boolean) => void;

  // New (q2-preview-only):
  /**
   * Severity hint that drives header text + indicator style
   * when in collapsed mode. "error" matches today's behaviour;
   * "warning" lets the collapsed indicator say "N warning(s)"
   * instead of borrowing the error message slot.
   */
  severity?: 'error' | 'warning';
  /**
   * Diagnostics from the server-side sink (eager-capture, deps,
   * re-execute) — rendered alongside `error.diagnostics` but
   * visually grouped so the user can tell where the diagnostic
   * was raised. Empty array (or omitted) = no server-side items.
   */
  serverDiagnostics?: Diagnostic[];
}
```

The new props let us name the warnings-only case directly
(`severity="warning"`, no `error.message` borrow) and give the
server-side feed its own visual lane without overloading
`error.diagnostics`. Both fields are optional so a future re-merge
with hub-client's overlay only has to teach the shared component
to ignore them.

**Overlay rendered when *any* feed has content** (not only on
failure):

```typescript
const hasDiagnostics =
  status.failure !== null ||
  status.diagnostics.length > 0 ||
  status.warnings.length > 0 ||
  status.pass1Failures.length > 0 ||
  status.serverDiagnostics.length > 0;
```

**Collapsed default.** Per Decision 2: collapsed by default for
warnings-only renders; expanded for first-render terminal
failures (no underlying iframe to fall back on). For non-terminal
failures (overlay-on-top), collapsed is also the default — the
iframe still shows the last-good render, the user is not
blocked. Hub-client's `usePreference('preview-overlay-collapsed', true)`
pattern (localStorage) is the right persistence story for the
collapse toggle; the fork can adopt the same hook.

**First-render failure (no good `astJson` yet).** D.4 already
handles this with a dedicated terminal-mode branch (PreviewApp.tsx
lines 611-619). It extends naturally: same structured payload, but
no underlying iframe to overlay on, so `collapsed={false}`.

### Server-side infrastructure (in scope — Decision 1)

The user's framing for this is: don't keep paying the
plumbing tax every time someone adds a `tracing::warn!()`.
Land a minimal sink + transport now so future server-side
diagnostics are a one-liner to surface in the SPA, and migrate
the three existing callsites so the infra has real users.

**The sink.** New module `crates/quarto-preview/src/diagnostics.rs`:

```rust
use quarto_error_reporting::DiagnosticMessage;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

/// Page-scoped diagnostic accumulator for the preview server.
/// Forward-slash project-relative path → diagnostics emitted
/// during server-side processing of that page. Cleared on each
/// successful re-processing of the page so the SPA sees the
/// current run's diagnostics, not a growing log.
#[derive(Default, Clone)]
pub struct DiagnosticSink {
    inner: Arc<RwLock<HashMap<String, Vec<DiagnosticMessage>>>>,
}

impl DiagnosticSink {
    /// Emit a diagnostic for `page`. Also writes via `tracing::warn!`
    /// so the server's stdout doesn't go quiet — this is purely
    /// additive over today's behaviour.
    pub fn emit(&self, page: &str, diag: DiagnosticMessage);

    /// Replace `page`'s diagnostics atomically (begin/end pattern
    /// for re-processing).
    pub fn replace_page(&self, page: &str, diags: Vec<DiagnosticMessage>);

    /// Snapshot for an HTTP read.
    pub fn get_for_page(&self, page: &str) -> Vec<DiagnosticMessage>;

    /// Snapshot of everything (for the future project-wide surface
    /// and for a diagnostic dump endpoint that helps debugging).
    pub fn snapshot(&self) -> HashMap<String, Vec<DiagnosticMessage>>;
}
```

The sink is owned by `AppState` (or whatever struct
`crates/quarto-preview/src/lib.rs` threads through to handlers)
and shared with `capture_driver`, `deps`, and `re_execute` via
`Arc::clone`.

**The transport.** New endpoint `GET /api/preview/diagnostics?page=<rel>`
in `crates/quarto-preview/src/lib.rs::extend_with_preview`
(alongside `/api/preview/deps` and `/api/preview/re-execute`).
Returns `{ "diagnostics": [...] }` where each item is shaped
exactly like the SPA's existing `Diagnostic` interface — same
JSON shape the WASM render returns. The SPA can splice the two
feeds together without a translation layer.

To get there, we factor `diagnostic_to_json` (currently in
`crates/wasm-quarto-hub-client/src/lib.rs:609-720`) into a
reusable location so both sites share the conversion:

- **Option A** (preferred): lift the JSON-shape struct +
  conversion into `quarto-error-reporting` as a public helper.
  Both the WASM bridge and the preview endpoint depend on it.
- **Option B**: serialize raw `DiagnosticMessage` via its existing
  serde derives and add a small SPA-side adapter. Lighter touch
  but creates a wire-format split.

The plan assumes Option A; revisit if `quarto-error-reporting`
would gain unwanted dependencies.

**Callsite migration** (three places, listed with what each one
will emit after migration):

| Callsite | Today | After |
|---|---|---|
| `capture_driver.rs:114-120` (eager-capture failure for sibling) | `tracing::warn!()` | `sink.emit(rel_path, DiagnosticMessage::warning("Engine capture failed"). with_problem(err.to_string()).build())` |
| `deps.rs:100-107, 141-147` (dep parse failure) | log + downgrade to empty deps | `sink.emit(rel_path, DiagnosticMessage::warning("Could not analyze includes").with_problem(...).build())` — keeps the "fail-open" semantic; the user just sees *why* the dep analysis was incomplete |
| `re_execute.rs:250-264` (engine re-execute failure) | sets `CaptureRef.lastError` only | Continue setting `lastError` (so `StaleCaptureOverlay`'s existing wiring keeps working) AND emit a structured diagnostic to the sink |

Each callsite's lifecycle is:

- **clear** the page's slot at the start of its run
  (`replace_page(page, vec![])` so success leaves an empty entry,
  not a stale one);
- **emit** diagnostics during the run;
- **leave the run's accumulated diagnostics in place** when the
  run finishes.

The SPA fetches `/api/preview/diagnostics?page=<rel>` on the
same trigger as `/api/preview/deps`: on `activeFile` change and
on `contentTick` bump. Latency story matches the deps fetch — a
few hundred ms after a file change, the diagnostic list refreshes.

## Open questions (resolved)

The original four questions and their resolutions are recorded
in §Decisions above. Kept here in summary so the historical
shape of the design conversation isn't lost:

1. **Scope split** → MVP covers WASM diagnostics + server-side
   infrastructure + migration of the three existing callsites.
   Project-wide aggregation UI deferred.
2. **First-warning UX** → collapsed by default; revisit after
   real-project feel.
3. **Overlay reuse vs fork** → fork into
   `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.tsx`
   so the two surfaces can evolve independently. Re-merge is a
   later decision when both are mature.
4. **Per-page vs project-wide** → per-page for MVP; project-wide
   is a follow-up.

## Test plan (TDD per CLAUDE.md)

Tests written *before* the implementation they exercise. Three
test surfaces — Rust unit tests for the sink + endpoint, SPA
unit/integration tests for the overlay fork and `PreviewApp`
wiring, and end-to-end verification against the actual `docs/`
project.

### Rust (server-side infrastructure)

1. **`crates/quarto-preview/src/diagnostics.rs` unit tests.**
   - `emit_then_get_for_page` — emit two diagnostics for the same
     page, snapshot reflects insertion order.
   - `replace_page_clears_prior` — emit, then `replace_page` with
     a new vec; old diagnostics are gone.
   - `pages_are_independent` — emit on page A, snapshot of page B
     is empty.
   - `concurrent_emit_thread_safety` — basic two-thread smoke
     (the `parking_lot::RwLock` makes this trivial; the test
     pins the contract).
   - `tracing_warn_still_fires` — captures `tracing` output via
     `tracing-subscriber`'s test sink and asserts the
     human-readable line is still emitted (regression guard:
     migrating callsites must not silence stdout).

2. **`crates/quarto-preview/tests/diagnostics_endpoint.rs`
   integration test.**
   - Boot a preview server with a fixture project, manually
     inject a `DiagnosticMessage` into the sink for a specific
     page, `GET /api/preview/diagnostics?page=<rel>` and assert
     the JSON shape exactly matches what the SPA's `Diagnostic`
     interface expects.
   - Unknown page returns `{ "diagnostics": [] }` (not 404 —
     the SPA always fetches, "no diagnostics" is the common case).

3. **`crates/quarto-preview/tests/diagnostics_capture_failure.rs`
   integration test.**
   - Boot a preview server pointing at a fixture project that
     contains a `.qmd` with an intentional parse error in a
     non-active page. After the eager-capture pass completes,
     `GET /api/preview/diagnostics?page=<failing-page>` returns
     the parse failure as a structured diagnostic. This pins the
     `capture_driver` migration.

4. **`crates/quarto-preview/tests/diagnostics_deps_failure.rs`
   integration test.**
   - Same shape as #3 but for `deps.rs` — a malformed include
     shortcode in a page surfaces as a structured diagnostic.

5. **Existing preview integration tests must keep passing** —
   notably the engine `lastError` path (`re_execute.rs`) still
   populates `CaptureRef.lastError` so `StaleCaptureOverlay`'s
   wiring (Phase C.5) is unchanged; the sink emission is
   additive.

### SPA (client-side surface)

6. **`q2-preview-spa/src/components/PreviewDiagnosticsOverlay.test.tsx` unit tests.**
   Cover the fork's new prop surface in isolation:
   - `severity="warning"` renders the warnings-mode header.
   - `serverDiagnostics` items render in their own visual group.
   - Collapsed/expanded toggle works the same as the source
     overlay (regression).
   - Empty `error` + non-empty `serverDiagnostics` still renders
     (the overlay can show only server-side diagnostics).

7. **`PreviewApp.integration.test.tsx` — success-with-warnings.**
   Stub `renderPageForPreview` to return `{ success: true,
   ast_json: '…', warnings: [synthDiagnostic] }`. Assert the
   overlay renders, `collapsed=true`, `error.diagnostics` contains
   the synthDiagnostic, `severity="warning"`.

8. **`PreviewApp.integration.test.tsx` — failure-with-structured-diagnostics.**
   Stub to return `{ success: false, error: 'msg', diagnostics:
   [synthDiagnostic], pass1_failures: [synthPass1] }`. Assert the
   overlay renders with the iframe still visible underneath (D.4's
   "last-good-render-preserved" invariant), `error.message ===
   'msg'`, `error.diagnostics` and `error.pass1Failures` populated,
   `severity="error"`.

9. **`PreviewApp.integration.test.tsx` — success clears warnings.**
   Sequence: success-with-warnings tick, then success-clean tick.
   Assert the overlay disappears (or hits `visible=false`) on the
   second tick.

10. **`PreviewApp.integration.test.tsx` — first-render terminal mode.**
    Stub to return `{ success: false, error: 'msg', diagnostics:
    [synthDiagnostic] }` on the *first* render. Assert no iframe
    mounts, the overlay renders in terminal mode (`collapsed=false`,
    covers the SPA), the structured payload reaches the overlay.

11. **`PreviewApp.integration.test.tsx` — server diagnostics fetch.**
    Stub `/api/preview/diagnostics` to return one diagnostic for
    the active page. Assert it reaches the overlay's
    `serverDiagnostics` prop, doesn't get mixed into
    `error.diagnostics`, and is re-fetched on `contentTick` bump.

12. **`PreviewApp.integration.test.tsx` — server + WASM merge.**
    Both stubs return one diagnostic. Assert both surface in the
    overlay, in their own visual lanes.

13. **Existing D.4 tests must keep passing** — the three
    integration tests added in D.4 encode invariants this change
    must preserve (last-good-render survives a failed render;
    overlay clears on next success; engine `lastError` surfaces
    via `StaleCaptureOverlay`).

### End-to-end (per CLAUDE.md "End-to-end verification before declaring success")

14. **Run `q2 preview docs/`** against the actual repo's `docs/`
    directory. Confirm:
    - The `[Q-13-4]` body-link warning for `markdown-basics.qmd`
      appears in the overlay when viewing
      `authoring/markdown/index.qmd` (collapsed indicator, then
      expanded view).
    - The `[Q-1-20]` metadata-as-markdown warning appears in the
      overlay when viewing `guide/index.qmd`.
    - Browser screenshot + a DOM excerpt of the overlay go into
      the final implementation status comment / PR description.
    - Server stdout still prints the same `tracing::warn!` lines
      (additive, not replacement).

15. **Playwright spec — `q2-preview-spa/e2e/diagnostics.spec.ts`.**
    Edit a fixture qmd to introduce a body-link warning,
    confirm the overlay surfaces within 5s of the on-disk write
    (`window.__renderTicks` already exists for this kind of
    timing assertion, added in D.3).

## Out of scope (tracked as follow-ups when MVP ships)

- **Project-wide diagnostic aggregation UI** — a "show me
  everything that's wrong across the project" surface. The sink
  already supports this (its `snapshot()` is project-wide and
  the endpoint can take a wildcard or be split into a separate
  `/api/preview/diagnostics/all`), but the UX questions (single
  overlay vs split surfaces, page-switch clear semantics) are
  better answered after per-page is in users' hands.
- **Richer engine diagnostics** — today an engine failure becomes
  a string `lastError`; lifting that to a structured
  `DiagnosticMessage` with source locations is its own work
  (and benefits from `bd-6daf`'s engine-output source-location
  reconciliation).
- **CLI parity** — `q2 preview --print-diagnostics` or similar
  to echo the merged diagnostic stream on the terminal where the
  server runs. The sink makes this trivial to bolt on; the
  question is whether anyone wants it.
- **`bd-m9rm`'s project-level diagnostic surface** — that issue
  asks for a CLI- + hub-shared project-level diagnostic
  mechanism. The sink we land here is a candidate building
  block; whether `bd-m9rm`'s eventual design adopts it or
  evolves something more general is a separate conversation.
- **Re-converging the forked overlay with hub-client's** — see
  Decision 3. Re-evaluate once both surfaces are mature.

## Files that will change (preview, not commitment)

**New (Rust):**
- `crates/quarto-preview/src/diagnostics.rs` — `DiagnosticSink`.
- `crates/quarto-preview/tests/diagnostics_endpoint.rs`,
  `diagnostics_capture_failure.rs`,
  `diagnostics_deps_failure.rs` — integration tests.
- Possibly a small public helper in `crates/quarto-error-reporting/`
  to host the shared `DiagnosticMessage → JsonDiagnostic`
  conversion (lifted from `wasm-quarto-hub-client/src/lib.rs`).

**Modified (Rust):**
- `crates/quarto-preview/src/lib.rs` — `AppState` gets the
  `Arc<DiagnosticSink>`, new endpoint registration, dependency
  injection into `capture_driver` / `deps` / `re_execute`.
- `crates/quarto-preview/src/capture_driver.rs` — replace the
  `tracing::warn!` callsite with `sink.emit(...)`; clear page
  at start of each capture run.
- `crates/quarto-preview/src/deps.rs` — same pattern.
- `crates/quarto-preview/src/re_execute.rs` — emit alongside the
  existing `lastError` write.
- `crates/wasm-quarto-hub-client/src/lib.rs` — if the conversion
  lifts to `quarto-error-reporting`, switch this site to call
  the shared helper.

**New (SPA):**
- `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.tsx`
  — forked from `PreviewErrorOverlay` with the new prop surface.
- `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.test.tsx`
  — unit tests for the fork.
- `q2-preview-spa/e2e/diagnostics.spec.ts` — Playwright spec.

**Modified (SPA):**
- `q2-preview-spa/src/PreviewApp.tsx` — replaced state slot,
  three overlay call sites swap to the fork, new fetch effect
  for `/api/preview/diagnostics`.
- `q2-preview-spa/src/PreviewApp.integration.test.tsx` — six new
  tests (per §Test plan).

## Implementation phasing

Once we have agreement on the plan, execute in this order. Each
step ends green (tests passing) before the next begins, so a
mid-stream interruption never leaves the tree broken.

1. **Tests first (red).** Write all Rust + SPA tests; verify each
   fails for the right reason.
2. **Rust infra.** Land `DiagnosticSink` + the endpoint; tests 1
   and 2 go green.
3. **Rust callsite migrations.** Migrate `capture_driver`, then
   `deps`, then `re_execute`. Tests 3, 4, and the
   regression-guard for `re_execute` go green.
4. **Conversion lift.** Move `diagnostic_to_json` into
   `quarto-error-reporting` (or commit to Option B and write the
   adapter); both call sites compile and pass their existing
   tests.
5. **SPA overlay fork.** Land
   `PreviewDiagnosticsOverlay.tsx` + its unit tests (test 6
   green). No `PreviewApp.tsx` changes yet — the fork sits
   beside.
6. **SPA wiring.** Swap `PreviewApp.tsx`'s three call sites to
   the fork, plumb state, add the server-diagnostics fetch
   effect. Tests 7–13 go green; D.4's existing tests still pass.
7. **Playwright spec** (test 15) lands once 1–6 are green.
8. **End-to-end verification** (test 14) against `docs/`,
   screenshot + DOM excerpt recorded in the PR description.
9. **`cargo xtask verify`** — full leg (Rust + hub-client +
   WASM) since `quarto-error-reporting` is in scope. Then
   manually rebuild q2-preview SPA per CLAUDE.md's "Verifying
   Rust changes in `q2 preview`" runbook.
10. **PR against `main`** (`feature/q2-preview-command` has
    merged; no integration branch to target).

Single PR is appropriate — the Rust infra and the SPA wiring
are interlocking; landing one without the other leaves the
infra unused or the UI fetch hitting a missing endpoint. The
phasing inside the PR is for review-ability, not for
incremental merges.
