---
title: 'q2 preview diagnostics: include ariadne source-context snippet'
date: 2026-05-21
description: 'Shows the Ariadne source-context snippet that `q2 render` prints for each warning in the `q2 preview` diagnostics overlay, by sending a pre-rendered text field in the diagnostic JSON.'
---

**Issue:** bd-352bh
**Discovered-from:** bd-b9kzg (q2 preview diagnostics surface)
**Parent epic:** bd-kw93 (q2 preview)
**Date:** 2026-05-21
**Status:** DRAFT — awaiting user sign-off on wire-shape addition

## Problem

bd-b9kzg landed render warnings/diagnostics in the q2-preview
overlay, but each entry shows only the compact one-line summary:

```
Line 89: [Q-13-4] Body link references missing document - 'authoring/markdown/markdown-basics.qmd' is not in the project index.
```

The user wants the rich ariadne source-context snippet — same shape
the `q2 render` command prints to stdout:

```
Warning: [Q-13-4] Body link references missing document
   ╭─[ /Users/cscheid/rooms/room-1/q2/docs/authoring/markdown/index.qmd:89:23 ]
   │
89 │   * [Markdown Basics](./markdown-basics.qmd)
   │                       ──────────┬──────────
   │                                 ╰── 'authoring/markdown/markdown-basics.qmd' is not in the project index.
───╯
ℹ Check the spelling, or confirm the target file is included in the render set.
```

Hub-client already shows this for Pass-1 failures (the screenshot
the user referenced). q2-preview should too — and for warnings, not
only failures.

## How hub-client gets the ariadne text today

For Pass-1 failures:

1. `ParseError::render()` in `crates/quarto-core/src/error.rs:32-38`
   calls `diag.to_text(Some(&source_context))` for each
   `DiagnosticMessage`. `to_text(Some(ctx))` invokes ariadne to
   render the source-context box (see
   `quarto-error-reporting/src/diagnostic.rs:355-466` —
   `to_text_with_options` calls `render_ariadne_source_context`
   when both `location` and `source_context` are present).
2. `QuartoError::Parse(ParseError)` displays as its `render()`
   output via `impl Display for ParseError`.
3. The orchestrator wraps the failure in `FileFailure { error:
   format!("{}", e), .. }` (`crates/quarto-core/src/project/orchestrator.rs:337-`).
4. WASM's `pass_failure_response`
   (`crates/wasm-quarto-hub-client/src/lib.rs:1709-1735`) sets
   `result.error = "Pass 1 failed for {path}: {failure.error}"`.
5. The SPA pipes `result.error` into the overlay's
   `error.message`, which the overlay renders as
   `<pre>{stripAnsi(message)}</pre>`.

Net: the ariadne text is pre-rendered at the Rust layer and
ships as a plain string in `result.error`.

**But for warnings**, the same `DiagnosticMessage` ships through
the `JsonDiagnostic` shape (lifted into
`quarto-error-reporting/src/json.rs` under bd-b9kzg) and that
shape doesn't carry a pre-rendered text field today.

## Proposed change

Add a `rendered` field to `JsonDiagnostic`, populated by
`diag.to_text(Some(ctx))` when the diagnostic has both a location
and an available source context. The overlay renders the
`rendered` string as `<pre>{stripAnsi(...)}</pre>` when present,
keeping the existing compact list as a fallback for diagnostics
without locations.

### Rust side (`quarto-error-reporting/src/json.rs`)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct JsonDiagnostic {
    // ... existing fields unchanged ...

    /// Pre-rendered ariadne source-context snippet for this
    /// diagnostic. Populated when the diagnostic has a `location`
    /// and the converting site has access to a `SourceContext`.
    /// Same text the `q2 render` CLI prints to stdout (ANSI-coded;
    /// strip on the JS side for display). Consumers can render
    /// this verbatim in a `<pre>` block for the rich
    /// source-context view, or ignore it and fall back to the
    /// structured fields for a compact summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
}
```

Update `diagnostic_to_json` to populate the field:

```rust
let rendered = if diag.location.is_some() {
    Some(diag.to_text(Some(ctx)))
} else {
    None
};
```

### TypeScript side

Mirror on the `Diagnostic` interface
(`ts-packages/preview-renderer/src/types/diagnostic.ts`):

```typescript
export interface Diagnostic {
  // ... existing ...
  /** Pre-rendered ariadne source-context snippet. Render as <pre>
   *  with stripAnsi() when present; falls back to structured fields. */
  rendered?: string;
}
```

### Overlay (q2-preview-spa fork)

In `PreviewDiagnosticsOverlay.tsx`, the list-renderer for each
diagnostic gets a branch:

```tsx
{diagnostics.map((d, i) =>
  d.rendered ? (
    <pre key={i} className="preview-error-diagnostic-rendered">
      {stripAnsi(d.rendered)}
    </pre>
  ) : (
    <li key={i} className="preview-error-diagnostic-compact">
      {/* existing compact line */}
    </li>
  )
)}
```

Add a CSS rule for `.preview-error-diagnostic-rendered` (monospace,
same styling family as `.preview-error-message`).

## Impact on hub-client

The new field is `#[serde(skip_serializing_if = "Option::is_none")]`,
so JSON shape stays compatible for any consumer that doesn't
populate it. Hub-client's `PreviewErrorOverlay` ignores unknown
fields, so receiving `rendered` doesn't change its behavior.
Hub-client might *want* to adopt the field later (its Monaco
markers + overlay could show the rich snippet instead of just the
compact list), but that's a separate decision; this change is
additive.

## Concerns

1. **Cost.** Every diagnostic with a location now triggers an
   ariadne render. For a render with N warnings, that's N
   ariadne passes (each loads the source into ariadne's source
   store). For typical documents this is negligible; for very
   large documents or large warning counts it could be
   measurable. Mitigation: ariadne renders are O(local source
   region), not O(full source). Real-world projects will rarely
   have more than a handful of warnings per page. *Pragmatic
   choice: render eagerly; measure if it becomes a problem.*

2. **Wire size.** Each rendered text is typically 8-20 lines.
   For a render with 5 warnings, the JSON response gains \~100
   lines of text. Not blocking but worth noting. The overlay
   only renders the expanded view on click, so transport cost
   doesn't translate to render cost until the user expands.

3. **ANSI codes.** ariadne emits ANSI color codes by default.
   Strip on the JS side using the existing
   `@quarto/preview-renderer/utils/stripAnsi`. The upstream
   overlay already does this for `error.message`; reusing the
   same path keeps things consistent.

4. **Hyperlink mode.** `to_text_with_options` supports OSC-8
   hyperlinks (for terminals that render them). The browser
   can't render OSC-8; pass `enable_hyperlinks: false`. Default
   `to_text(Some(ctx))` already disables them; no change needed.

## Test plan

1. **Rust unit test** in `crates/quarto-error-reporting/src/json.rs`
   `tests` module: build a `DiagnosticMessage` with a
   `SourceInfo` location and a populated `SourceContext`, call
   `diagnostic_to_json`, assert `rendered` is `Some(_)` and
   contains the box-drawing chars (`╭─` or similar) that
   ariadne emits.

2. **Rust unit test**: build a `DiagnosticMessage` with NO
   location, call `diagnostic_to_json`, assert `rendered` is
   `None`.

3. **SPA component test** in
   `PreviewDiagnosticsOverlay.integration.test.tsx`: pass a
   `Diagnostic` with `rendered: '<ariadne text>'`, assert the
   text appears verbatim (after stripAnsi) inside a `<pre>` —
   not as compact list text.

4. **SPA component test**: pass a `Diagnostic` WITHOUT
   `rendered`, assert the existing compact list line renders.

5. **End-to-end** against `docs/authoring/markdown/index.qmd`
   per CLAUDE.md "End-to-end verification before declaring
   success". Open `q2 preview docs/`, navigate, expand the
   warning overlay, screenshot — the rendered output should
   match the user's image (ariadne snippet for each warning).

6. **Existing tests must keep passing**: 120/120 Rust, 41/41
   SPA integration, 8/8 SPA unit, 6/6 overlay component, 1/1
   Playwright, `cargo xtask verify` 12/12.

## Open questions

1. **Render eagerly or on first expand?**
   - (a) **Eagerly** at `diagnostic_to_json` time. Wire size
     grows; first render is slightly slower. **Recommended for
     MVP** — simplest and matches how `q2 render` works.
   - (b) **Lazy**: ship just structured fields; render the
     ariadne text on demand from a new
     `GET /api/preview/diagnostics/rendered?page=X&index=N`
     endpoint. More machinery; only worth it if (a) measures
     too costly.

2. **Show ariadne text in the compact list, OR replace the
   compact list entirely?**
   - (a) **Show ariadne when present; fall back to compact
     when absent.** Both modes coexist. **Recommended** —
     diagnostics without locations (rare but possible) still
     surface somehow.
   - (b) **Replace the compact list entirely** with ariadne
     blocks. Cleaner UI; loses the fallback for unlocated
     diagnostics.

3. **Should this also land for hub-client's overlay?**
   - The field becomes available to hub-client automatically
     (same wire format). Hub-client could adopt the rich
     rendering by updating its
     `PreviewErrorOverlay.tsx`. Out of scope here unless you
     want it bundled — hub-client's overlay already has its
     own UX considerations (Monaco markers + overlay), so a
     separate decision is probably right.

## Implementation phasing

1. Add field + populate in `diagnostic_to_json` (Rust unit
   tests).
2. Update TS `Diagnostic` interface + overlay component
   (component unit tests).
3. End-to-end verification + screenshot.
4. `cargo xtask verify` (full leg — `quarto-error-reporting`
   in scope so WASM rebuild required).
5. Commit + merge to main (no PR per user's prior preference;
   ask before push).
