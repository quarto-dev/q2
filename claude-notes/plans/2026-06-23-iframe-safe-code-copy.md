---
title: 'iframe-safe code-copy in q2 preview / hub-client'
date: 2026-06-23
---

**Strand:** bd-wa2pgri8 (feature, p3) — follow-up to **bd-lg6t6qfy**, which made
revealjs code-copy buttons styled + hover-hidden in all paths but functional
(actually copies) only in native `q2 render`. This strand makes them copy in the
WASM/iframe paths (q2 preview + hub-client) too.

**Status:** IMPLEMENTATION COMPLETE (2026-06-23) — both reveal and plain-HTML
preview copy verified working in a real q2-preview iframe; edit-isolation
confirmed. Awaiting push approval.

---

## Background / why preview copy is inert today

- `ClipboardJsStage` is excluded from the WASM pipeline (`pipeline.rs ~1866`,
  "hub-client iframe reinit"), and bd-lg6t6qfy registered the reveal clipboard
  assets `#[cfg(not(target_arch = "wasm32"))]`. So neither plain-HTML nor reveal
  preview ships any copy JS — buttons render styled (the copy-code.scss layer is
  bundled unconditionally in WASM) but do nothing.
- The original suppression rationale: the preview iframe re-renders on every
  edit, which blows away stateful JS (event listeners bound to specific button
  nodes die when those nodes are replaced). A naive `new ClipboardJS('.btn')`
  bind would silently stop working after the first edit.

## Design — one capture-phase delegated listener on `previewHostRef`

**Investigation result that shapes this:** `PreviewRoot` (shared by *both*
q2-preview and hub-client — `hub-client/.../render/ReactRenderer.tsx` mounts it)
renders a single `<div ref={previewHostRef} style={{display:'contents'}}>`
(`PreviewRoot.tsx:1504`) that wraps **both** the `<RevealDeck>` branch (slides)
and the `<Ast>` branch (plain HTML). The reveal `.reveal` DOM is a descendant of
this host. So **one delegated listener on `previewHostRef.current` covers reveal
AND plain-HTML preview, in both hosts, from a single mount point.**

This is simpler and more consistent than the strand's suggested per-deck
`RevealClipboard` component (which would cover reveal only and leave plain-HTML
preview still inert — the inconsistency the strand flags). It needs no
`useReveal()` / `getRevealElement()` escape hatch because the host already
contains the reveal DOM.

**Iframe-safe by construction:**
- **Event delegation** — bind ONE listener to the stable host, match
  `event.target.closest('.code-copy-button')`. Buttons created/destroyed by edit
  re-renders need no re-binding; the host div persists across re-renders (stable
  ref), so the listener survives every edit. This is exactly the property the
  original stateful-bind approach lacked.
- **Capture phase + `stopPropagation`** — so a copy click cannot also trip
  PreviewRoot's block-edit activation. (To verify: whether edit activation is
  click- or pointer-driven for code blocks; if pointer-driven, may also need to
  swallow `pointerdown`. The copy `<button>` is a *sibling* of the `<code>`
  inside the scaffold, so a button click does not pass through the code block's
  own handlers — but an ancestor outer-block handler could still see it.)

**Copy mechanism:** `navigator.clipboard.writeText` — the codebase's established
pattern (`ShareDialog.tsx`, `Editor.tsx`, `ProjectTab.tsx`); no clipboard.js
dependency added. Secure-context only (localhost/https — both preview hosts
qualify).

**Text extraction:** mirror native `code-copy-init.js`\'s `getTextToCopy`: from
the button, `closest('.code-copy-outer-scaffold')` → `querySelector('code')` →
clone → strip `.code-annotation-*` children → `innerText`.

**Feedback:** on a successful copy, add `code-copy-button-checked` for 1000ms
(the checkmark SVG state already ships in copy-code.scss), then revert. No
Bootstrap tooltip (none in preview) — parity with native render v1.

### Files

- **New** `ts-packages/preview-renderer/src/utils/codeCopy.ts` —
  `installCodeCopy(root: HTMLElement): () => void` (pure, framework-free: installs
  the capture-phase delegated listener, returns a cleanup fn) + `getTextToCopy`
  helper. Unit-testable without React.
- **Edit** `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx` — one
  `useEffect(() => installCodeCopy(previewHostRef.current!), [])` (guarded for a
  null ref). \~5 lines.

### Out of scope

- Native `q2 render` (already functional via ClipboardJsStage / `js:revealjs:*`).
- Re-enabling `ClipboardJsStage` in the WASM pipeline (we deliberately do NOT —
  this React-level handler is the iframe-safe replacement).
- The "Copied!" Bootstrap tooltip.

---

## Risks / verify

1. **Edit-activation interference.** A copy click must not open the block
   editor. Capture-phase `stopPropagation` should prevent click-driven
   activation; verify pointer-driven activation (`useBlockEditHover` uses
   `onPointerUp`) isn't separately triggered. If it is, swallow `pointerdown` on
   a `.code-copy-button` target too.
2. **`display:contents` host.** Confirm `addEventListener` + `contains` behave on
   the host (events propagate through `display:contents` elements — they do, but
   assert in a test).
3. **`navigator.clipboard` absence in jsdom.** Mock `navigator.clipboard.writeText`
   with a `vi.fn()` in tests; guard for absence in the util (no throw if missing).
4. **Double-binding.** The `[]`-dep effect installs once; the cleanup removes on
   unmount. Confirm no duplicate listeners across edit re-renders (host is stable,
   effect doesn't re-run).

---

## Work plan (TDD)

### Phase 1 — `installCodeCopy` util (pure, framework-free) ✅

- [x] **Test first** (`utils/codeCopy.integration.test.ts`, jsdom): 7 cases —
      copy on click, checked-class flash + 1s revert (fake timers),
      `.code-annotation-*` stripping, non-button clicks ignored, pointer-event
      isolation (capture stopPropagation vs a stand-in block handler), cleanup
      removes listeners, no-throw when `navigator.clipboard` absent. Verified red
      (module missing) → green.
- [x] Implemented `installCodeCopy` + `getTextToCopy` in `utils/codeCopy.ts`.
      Capture-phase delegated listeners (pointerdown/pointerup stopPropagation +
      click → copy). Uses `navigator.clipboard.writeText` (no new dep).

### Phase 2 — Wire into PreviewRoot (covers reveal + HTML, both hosts) ✅

- [x] **Test first** (`q2-preview/code-copy.integration.test.tsx`): mounts the
      real `PreviewRoot`, injects a copy scaffold into its `previewHostRef` host,
      clicks → asserts `writeText` got the code; asserts cleanup on unmount.
      Verified the wiring.
- [x] Added the `installCodeCopy(previewHostRef.current)` effect (one `useEffect`,
      next to the `installLinkHandlers` effect). Covers reveal + HTML, both hosts.
- [x] Full `preview-renderer` suite green: 456 unit + 493 integration (+9 new).

### Phase 3 — Build + E2E ✅

- [x] `hub-client && npm run build:all` (strict `tsc -b` project-refs + WASM +
      vite) GREEN — no type errors. `npm run test:ci` GREEN (121 tests).
- [x] **Browser E2E (q2 preview), real iframe, both paths:**
      - **Reveal deck**: trusted click on the copy button →
        `navigator.clipboard.readText()` returned the exact code
        `def greet(name):\n    print(f"hello {name}")`.
      - **Plain-HTML doc**: instrumented `writeText` captured the exact code
        `x = sum([1, 2, 3])`, AND **the block editor did NOT open** on the copy
        click (edit-isolation via capture-phase stopPropagation works).
      - No console errors in either.
      - Iframe-safe-across-edits property: assured by construction (one delegated
        listener on the stable host, matched by selector — buttons re-created by
        an edit re-render need no re-binding). Not separately exercised in-browser.
- [x] **hub-client:** shares the same `PreviewRoot` path (its `ReactRenderer`
      mounts `PreviewRoot`, which auto-detects slides → `RevealDeck`); the same
      `previewHostRef` listener applies. test:ci green. No live multi-user hub
      session exercised (honest scope note).
- [ ] **Changelog:** changes live in `ts-packages/preview-renderer/`, not
      `hub-client/`, so the two-commit rule doesn't strictly trigger — but the
      behavior is user-visible in hub-client. **Decision deferred to the user**
      (offer to add an About-section entry).

### Phase 4 — Close-out

- [x] Plan updated. Committed on `braid/bd-wa2pgri8-iframe-copy`. Pending the
      user's review + push approval; then close bd-wa2pgri8.
