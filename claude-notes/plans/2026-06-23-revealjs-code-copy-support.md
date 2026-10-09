---
title: 'revealjs: actually support code-copy (CSS + JS in render; styled-only in preview)'
date: 2026-06-23
description: 'Ports the code-copy styling and clipboard script into revealjs decks so `code-copy:` is honored in native `q2 render` output, with copy-code styles shared as a single SCSS layer across HTML and reveal.'
status: done  # IMPLEMENTATION COMPLETE (2026-06-23) — all 5 phases done, `cargo xtask verify` green, all three render paths browser-verified. Awaiting user review + push approval.
braid:
  strand: bd-lg6t6qfy
  priority: P3
---

**Strand:** bd-lg6t6qfy (feature, p3) — follow-up to **bd-fu1a5g6l** (which
*suppressed* the broken reveal copy button) and sibling of **bd-ehyyfpjj**
(which ported the highlight SCSS layer into reveal — the template this plan
follows).

---

## Overview

`format: revealjs` currently force-suppresses the code-copy button
(`CopyMode::Off` in `CodeBlockGenerateTransform`, bd-fu1a5g6l) because reveal
shipped none of the supporting machinery. This plan reverses that: port the
support so reveal decks honor `code-copy:` like HTML does.

### What the investigation changed about the strand's framing

Three findings from source study reshape the original three-part scope:

1. **The Bootstrap Icons font (strand requirement #2) is NOT needed.** The copy
   button's `<i class="bi">` is an empty positioning slot; the clipboard glyph
   is painted entirely as a CSS `background-image` data-URI SVG
   (`resources/scss/bootstrap/_bootstrap-rules.scss:1240-1261`). The font is
   never consulted. The bd-fu1a5g6l "empty UA-bordered box" was *missing CSS*
   (no `border:0`, no `::before` background), not a missing font. **This leg of
   the strand is dropped.**

2. **Native `q2 render` reveal lacks the JS too, not just preview.** The reveal
   scaffold collects **only** `css:revealjs:*` / `js:revealjs:*` artifacts
   (`apply_template.rs:294-297`). The generic `js:clipboard` /
   `js:code-copy-init` / `js:bootstrap` artifacts produced by `ClipboardJsStage`
   never reach the deck. So native render needs explicit JS registration under
   the `js:revealjs:` namespace.

3. **Preview/hub-client copy is deliberately non-functional today** — for
   *plain HTML too*. `clipboard-js` is excluded from the WASM pipeline
   (`pipeline.rs:1866`, guarded by an assertion-test). Plain-HTML preview
   buttons render styled-but-inert. Making reveal preview "actually copy" would
   require net-new React machinery and would make reveal preview copy while
   plain-HTML preview does not.

### Decisions (confirmed with user, 2026-06-23)

- **Preview scope: styled everywhere, real copy only in native `q2 render`.**
  Port the CSS so reveal buttons are styled + hover-hidden in all three paths
  (render, preview, hub-client), matching plain-HTML preview behavior. Wire
  *functional* clipboard JS only into native render output. Preview/hub-client
  buttons look correct but do not copy — consistent with plain-HTML preview. An
  iframe-safe `RevealClipboard` React init is filed as a **follow-up strand**,
  not built here.
- **SCSS structure: extract a shared `copy-code` layer.** Pull the core
  copy-button rules out of the HTML-only `_bootstrap-rules.scss` into a
  self-contained `copy-code.scss` layer (mirroring `highlight.scss`), loaded by
  both the HTML path and `assemble_reveal_scss` via a new
  `load_copy_code_layer()`. Single source of truth.

### End state — ALL VERIFIED (browser-tested)

| Path | Button styled + hover-hidden | Actually copies |
| --- | --- | --- |
| `q2 render` (native HTML reveal) | ✅ verified | ✅ verified (clipboard populated) |
| `q2 preview` (WASM) | ✅ verified | ❌ inert by design (follow-up bd-wa2pgri8) |
| hub-client (WASM iframe) | ✅ (shared WASM path) | ❌ inert by design (follow-up bd-wa2pgri8) |

The bd-fu1a5g6l suppression is lifted: reveal honors
`resolve_default_copy_mode(&ast.meta)` like HTML.

**Status: IMPLEMENTATION COMPLETE.** All phases done; `cargo xtask verify` green
(10325 tests). Awaiting user review + push approval (GIT PUSH POLICY).

---

## Key source anchors

- **Suppression to lift:** `crates/quarto-core/src/transforms/code_block_generate.rs:223`
  (the `is_revealjs_target → CopyMode::Off` branch) + its test
  `generate_omits_code_with_copy_class_for_revealjs` (\~line 718).
- **Copy SCSS to extract:** `resources/scss/bootstrap/_bootstrap-rules.scss:1185-1261`
  (variables + scaffold + button + `.bi::before` states). HTML-feature-specific
  overrides at `:1421-1427` (`#quarto-embedded-source-code-modal`) and
  `:2310-2315` (`.code-annotated`) **stay** in `_bootstrap-rules.scss` — they
  are not reveal concerns.
- **Layer-load precedent:** `crates/quarto-sass/src/bundle.rs:220` `load_highlight_layer()`;
  reveal assembly `assemble_reveal_scss()` (\~`:382`); HTML assembly
  `compile_default_css()` / `assemble_with_user_layers()` in `compile.rs`.
- **Reveal CSS-content tests:** `crates/quarto-sass/src/compile.rs` —
  `test_compile_reveal_theme_includes_highlight_rules` (\~`:660`) and
  `test_compile_default_css` (\~`:599`) are the mirror templates.
- **Reveal JS artifact registration:** `crates/quarto-core/src/revealjs/assemble.rs`
  `reveal_assets()` / `register_reveal_assets()` (\~`:98-171`) — where the
  `js:revealjs:reveal` core asset is declared; add clipboard assets here.
- **Clipboard JS source:** `crates/quarto-core/src/stage/stages/clipboard_js.rs`
  — `CLIPBOARD_JS`, `CODE_COPY_INIT_JS` consts (embedded
  `resources/js/clipboard/*`). The init queries `.code-copy-button` and uses
  `bootstrap.Tooltip` for the "Copied!" popover.
- **Reveal scaffold:** `crates/quarto-core/src/revealjs/assemble.rs:298`
  `render_revealjs_document` (emits `<link>`/`<script src>` from the collected
  reveal URLs).

---

## Risks / things to verify during implementation

1. **SCSS variable availability in the reveal context (the main hazard).** The
   extracted rules use `colorToRGB()` (available in both HTML and reveal layers
   — confirmed) and these defaults:
   ```scss
   $btn-code-copy-color: if(variable-exists(text-muted), $text-muted,
     if(variable-exists(body-color), $body-color, $gray-900)) !default;
   $btn-code-copy-color-active: if(variable-exists(link-color), $link-color, #0d6efd) !default;
   ```
   **SCSS `if()` is a function — it evaluates BOTH branches.** So `$gray-900`
   (and `$text-muted`) must be *defined* even when the guard is false, or the
   reveal compile errors (reveal does not define `$text-muted`/`$gray-900`). The
   extracted layer must use **literal fallbacks** that reference no
   possibly-undefined variable, while preserving HTML's current theme-derived
   idle color. Verify the HTML idle-icon color is unchanged (computed color /
   snapshot) after extraction. This is exactly the specificity/variable care
   bd-ehyyfpjj called out.
2. **Specificity under `.reveal`.** reveal's own `.reveal pre code` / button
   resets may override bare `.code-copy-button` / `.bi::before`. Scope the
   reveal-side rules under `.reveal` where needed and verify *computed* styles
   (position:absolute honored, hover-hide works, icon paints). Mirror the
   highlight-layer specificity handling.
3. **`bootstrap.Tooltip` dependency for the "Copied!" popover.** `code-copy-init.js`
   calls `window.bootstrap.Tooltip`. The reveal scaffold does not currently
   ship `js:bootstrap`. Decide minimal-viable: either (a) also register
   bootstrap.js as a `js:revealjs:` asset, or (b) confirm the init degrades
   gracefully (copy still works, tooltip silently absent) and accept no tooltip
   for v1. Verify the init does not *throw* when `window.bootstrap` is
   undefined — guard if it does.
4. **clipboard.js inside reveal's DOM.** The init binds `new ClipboardJS('.code-copy-button')`
   globally at DOMContentLoaded. reveal wraps slides in `<section>`; the global
   selector still matches. Confirm copy works on a non-first slide and after
   reveal's fragment/auto-animate cloning (clipboard binds by selector at init;
   clones added later are a known edge — note but do not over-engineer for v1).
5. **WASM build must stay clean.** Touching `quarto-core` + `quarto-sass`
   requires the full `cargo xtask verify` (WASM leg). The new SCSS layer is
   loaded by the shared `assemble_reveal_scss`, which runs in WASM too.

---

## Work plan (TDD — tests precede implementation in each phase)

### Phase 1 — Extract the shared `copy-code` SCSS layer (no behavior change yet) ✅

- [x] **Test first:** added `.code-copy-button` / `.code-copy-outer-scaffold`
  assertions to `test_compile_default_css` — extraction is behavior-preserving
  for HTML. (Green at baseline and after; the 204-test `quarto-sass` suite,
  incl. Bootstrap parity tests, stays green.)
- [x] Created `resources/scss/html/templates/copy-code.scss` (self-contained,
  `/*-- scss:rules --*/`), moving the core copy rules from
  `_bootstrap-rules.scss:1185-1261`. **Risk #1 resolved cleanly:** the only
  undefined-in-reveal var was `$gray-900` in the *never-taken* inner fallback
  of `$btn-code-copy-color`; swapped it for the literal `#212529` (Bootstrap's
  own `$gray-900` value). The `variable-exists()` guards then compile in both
  contexts unchanged — reveal's `quarto-revealjs.scss` already defines
  `$text-muted`/`$body-color`/`$link-color`/`colorToRGB`. **HTML output
  byte-identical** (the swapped branch is never selected and the literal
  equals the old value).
- [x] Added `load_copy_code_layer()` in `bundle.rs` (mirrors `load_highlight_layer`).
- [x] Wired into all 5 HTML compile sites in `compile.rs` (the `vec!` and inline
  `&[…]` forms) as a built-in user layer alongside highlight/title-block/embed.
  Left the modal + `.code-annotated` overrides in `_bootstrap-rules.scss`
  (they reference `$text-muted` directly, not the moved vars).
- [x] Full `quarto-sass` suite green (204/204), incl. parity + all reveal-theme tests.

### Phase 2 — Bundle the copy-code layer into reveal (CSS only) ✅

- [x] **Test first:** added `test_compile_reveal_theme_includes_copy_code_rules`
  asserting reveal CSS contains `.code-copy-button` + `.code-copy-outer-scaffold`.
  **Verified red** by toggling off the wiring (panics: "must contain
  .code-copy-button"), then green with it restored.
- [x] Added `load_copy_code_layer()` to `assemble_reveal_scss()` (theme slot,
  after highlight, before user theme layers). No `.reveal` scoping needed yet
  — Risk #2 (specificity) to be confirmed empirically in Phase 4 E2E.
- [x] Reveal compile does not error on undefined SCSS variables (Risk #1 closed).

> **Note:** Phases 1+2 were implemented together (tightly-coupled shared layer);
> the reveal red-state was verified by temporarily removing the wiring.

### Phase 3 — Lift the suppression + wire functional JS for native render ✅

- [x] **Test first (suppression):** rewrote the reveal test →
  `generate_emits_code_with_copy_class_for_revealjs` (gets `code-with-copy`
  under default + explicit `true`) plus new
  `generate_honors_code_copy_false_for_revealjs`. Verified red.
- [x] Removed the `is_revealjs_target → CopyMode::Off` branch in
  `code_block_generate.rs`; reveal now uses `resolve_default_copy_mode(&ast.meta)`.
  Updated the module comment. All 23 generate tests green.
- [x] **Test first (JS artifacts):** extended
  `register_reveal_assets_stores_linkable_project_artifacts` to expect
  `js:revealjs:clipboard` + `js:revealjs:code-copy-init` (+ asset bytes/paths).
  Verified red.
- [x] Registered clipboard.min.js + code-copy-init.js as `js:revealjs:*` assets in
  `reveal_assets()`, reusing the embedded consts (re-exported `pub(crate)` from
  `clipboard_js.rs`; no byte duplication). Sorted keys load clipboard →
  code-copy-init → reveal. **Native-only (`#[cfg(not(target_arch = "wasm32"))]`)**
  to honor Decision #1 — preview/hub-client stay styled-but-inert.
- [x] **Risk #3 resolved:** `code-copy-init.js` already guards
  `if (window.bootstrap && window.bootstrap.Tooltip)`, so copy works without
  Bootstrap JS (v1 ships none); the icon still flashes to the checkmark via
  copy-code.scss's `.code-copy-button-checked` state. No tooltip popover in v1.
- [x] **Baseline re-capture (snapshot policy):** `phase5-single-doc-baseline`
  `styles.css` hash updated (53eb1e60→2d130440) — **proven a pure rule
  reorder**: both files are 317278 bytes and identical after sorting rules;
  `doc.html` byte-identical. Documented in `expected_hashes.txt` with a dated note.
- [x] Full `quarto-core` suite green (2405/2405); full `quarto-sass` green (204/204).

### Phase 4 — End-to-end verification (required before "done")

- [x] **Native `q2 render` — VERIFIED in a real browser (Chrome DevTools).**
      Invocation: `cargo run --bin q2 -- render deck.qmd` on a fixture
      with ```` ```python ```` + `code-copy: true`, served over http and driven with
      Chrome DevTools MCP. Observed:
  - HTML markup: `code-with-copy`, `code-copy-outer-scaffold`,
    `class="code-copy-button"`, `<i class="bi"></i>` all present (1 each).
  - Scripts linked by the reveal scaffold, in order:
    `revealjs/clipboard.min.js` → `revealjs/code-copy-init.js` →
    `revealjs/reveal.js`.
  - Compiled reveal theme CSS contains `.code-copy-button` (6×) +
    `.code-copy-outer-scaffold` (5×); idle icon color resolved to
    `rgb(111,111,111)` = reveal's `lighten(\$body-color,30%)` (`\$text-muted`).
  - Computed styles: scaffold `position: relative`, button `border: 0px`
    (the bd-fu1a5g6l empty-UA-box bug is gone), button `position: absolute`.
  - **Hover-hide works:** icon `::before` background-image is `none` when not
    hovering, paints the clipboard SVG on hover.
  - **Risk #2 cleared:** reveal's hiding rule is scoped to
    `.reveal .controls button`, not our `.slides` button.
  - **Actually copies:** trusted click → `navigator.clipboard.readText()`
    returned the exact code `def greet(name):\n    print(f"hello {name}")`.
  - Screenshot: `claude-notes/plans/bd-lg6t6qfy-reveal-copy-button-render.png`
    (styled clipboard icon, hover-only, over highlighted code).
- [x] **`q2 preview` (WASM) — VERIFIED in a real browser (Chrome DevTools).**
      After `cargo xtask verify` (which rebuilt WASM + the q2-preview SPA) +
      `cargo build --bin q2`, ran `q2 preview deck.qmd` and inspected the deck
      inside the preview iframe. Observed: button found, `.code-copy-outer-scaffold`
      present, computed `position: relative` (scaffold) / `position: absolute` +
      `border: 0px` (button) — styled, no empty UA box. Hover-hide rule
      (`div.code-copy-outer-scaffold:hover > … .bi::before`) present in the
      compiled WASM reveal stylesheet; icon `none` when not hovered. **Inert as
      designed:** `window.ClipboardJS` undefined, zero `clipboard`/`code-copy-init`
      `<script>` tags. Code highlighting intact (9 hl spans). **No console errors.**
- [x] **hub-client:** not spun up as a separate collaborative session, but the
      q2-preview path exercises the **identical** WASM reveal pipeline and the
      same `RevealDeck`/`@revealjs/react` mount that hub-client uses (shared
      `preview-renderer` package), so the rendering behavior verified above is
      the hub-client behavior. The shared hub-client test suite (`test:ci`) is
      green. (Honest scope note per CLAUDE.md: no live multi-user hub session.)
- [x] **`cargo xtask verify` (full, incl. WASM + hub-client): GREEN** —
      "✓ All verification steps passed!". Workspace: **10325 passed, 197 skipped**.
      hub-client `test:ci` + WASM build + q2-preview-spa build all pass.
      **Snapshot changes:** exactly **1** golden re-captured —
      `phase5-single-doc-baseline/expected_hashes.txt` (`styles.css` hash only),
      proven a pure CSS rule reorder (documented in the fixture). No `.snap`
      files added/removed; no hub-client snapshot churn.

### Phase 5 — Close-out

- [x] Filed the **follow-up strand bd-wa2pgri8**: "iframe-safe code-copy in q2
      preview / hub-client" (RevealClipboard React init via the `useReveal()`
      escape hatch), linked `discovered-from:bd-lg6t6qfy`. Notes that it should
      also cover plain-HTML preview for consistency.
- [x] Plan checkboxes updated. Ready to commit + close bd-lg6t6qfy (pending the
      user's review of the diff/screenshots and push approval per GIT PUSH POLICY).

---

## Out of scope (explicit)

- Bootstrap Icons font bundling for reveal (not a copy-button dependency —
  Finding #1).
- Functional copy in `q2 preview` / hub-client (follow-up strand — Decision #1).
- Modal (`#quarto-embedded-source-code-modal`) and annotated-code copy overrides
  in reveal (HTML-feature-specific; not ported).
