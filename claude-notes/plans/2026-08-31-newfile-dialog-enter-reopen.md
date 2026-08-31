# NewFileDialog: Enter-submit reopens the dialog (GH #635)

**GitHub issue:** https://github.com/quarto-dev/q2/issues/635
**Braid strand:** bd-zcv0iea4
**Status:** planned, not yet implemented. Root cause confirmed empirically in a real
browser; fix hypothesis verified end-to-end and then reverted pending TDD.

## Overview

In hub-client's "New file" dialog:

- **Click "Create"** → file created, dialog closes. Correct.
- **Press Enter in the filename input** → file created, dialog **reappears**
  (empty, focused) instead of staying closed.

During investigation a second, worse defect was confirmed in the same handler:

- **Tab to "Cancel", press Enter** → the file **is created anyway** (and the
  dialog also reappears). The user asked to cancel and got a file.

Both defects live in `hub-client/src/components/NewFileDialog.tsx`'s
dialog-level Enter handler; neither requires any change to `ModalDialog`'s
focus-restore machinery, which is working as designed.

## Root cause (confirmed, not hypothesized)

`NewFileDialog` handles Enter via `ModalDialog`'s `onKeyDown` delegation — a
handler on the dialog **container div**, reached by bubbling from whatever is
focused (lines 113–120). On Enter it calls `handleCreateTextFile()`, which
creates the file and calls `onClose()`. It never calls `e.preventDefault()`.

Per the UI Events spec, an unprevented Enter keydown has a **default action**:
the browser synthesizes a `click` on the focused element, dispatched *after*
the keydown listeners and the microtask checkpoint. The sequence, captured
with capture-phase event logging in headless Chromium (dev server + local hub):

```
keydown target=INPUT.qh-input  key=Enter        ← handler creates file, closes dialog
focusin target=BUTTON.new-file-btn              ← ModalDialog's queueMicrotask focus restore
click   target=BUTTON.new-file-btn detail=0 isTrusted=true   ← Enter's DEFAULT ACTION
keyup   target=BUTTON.new-file-btn key=Enter
focusin target=INPUT.qh-input                   ← dialog reopened, autofocus
```

The dialog unmounts inside the keydown handler; `ModalDialog` restores focus
to the sidebar's "New file" button (its correct WCAG contract); the browser
then delivers the keydown's synthesized click to the **newly focused** button
— which is `onNewFile`, reopening the dialog. `detail=0, isTrusted=true` is
the signature of a keyboard-activation click.

The Cancel defect is independent of focus restore: the keydown from the
focused Cancel button **bubbles up** to the container handler, which submits;
the button's own native Enter activation then also fires Cancel's `onClick`.
Create-then-close both run.

Verified fix hypothesis: adding `e.preventDefault()` to the Enter branch
suppresses the synthesized click; the same instrumented run then shows no
`click` event, `dialog-count=0`, and focus resting on the "New file" button.
(The edit was reverted; it lands via TDD below.)

### Why the Create-button path is unaffected

A pointer click has no pending default keydown action, so close + focus
restore complete with nothing left to deliver. Confirmed: after click-create,
`dialog-count=0`, focus on `.new-file-btn`.

### Sibling audit (same session)

| Site | Enter handling | Affected? |
| --- | --- | --- |
| `ShareDialog.tsx:78` | dialog-level Enter → copy, then `onClose` **deferred 500 ms** | Not today — by the time it closes, the default click already fired inside the still-open dialog. Fragile: removing the delay would recreate #635 here. Hygiene `preventDefault` recommended while in the area. |
| `NewAssetDialog.tsx:297` | per-row rename inputs; Enter just blurs | No |
| `FileSidebar.tsx:540` (rename) | non-modal inline input | No — no modal close/focus-restore pair |
| `ProjectsHome.tsx:1644`, `ProjectSelector.tsx:880` | inline rename saves | No — same shape as above |

## Design decision

Two viable shapes for the fix:

- **(A) Keep the dialog-level handler, guard + prevent** *(recommended)*:
  ```tsx
  if (e.key === 'Enter') {
    if (e.target instanceof HTMLButtonElement) return; // let buttons be buttons
    e.preventDefault();
    handleCreateTextFile();
  }
  ```
  Preserves today's "Enter submits from anywhere in the form" behavior
  (including the template `<select>`), fixes both defects, one site.

- **(B) Move Enter handling onto the filename input** (drop the `onKeyDown`
  delegation): semantically cleanest, but changes behavior — Enter on the
  template select would no longer submit. Not recommended for a minimal fix.

Either way, document the contract on `ModalDialog`'s `onKeyDown` prop doc:
*a delegated handler that synchronously closes the dialog must call
`e.preventDefault()` on Enter, or the keydown's default-action click lands on
the focus-restore target.* This is what makes the bug a class rather than a
one-off.

## Work items (TDD order)

### Phase 1 — tests first (must fail before the fix)

- [ ] **Unit (jsdom)**, in `NewFileDialog.integration.test.tsx`:
      jsdom does not synthesize keyboard-activation clicks, so the *reopen*
      cannot reproduce there; instead assert the mechanism directly —
      `fireEvent.keyDown(input, { key: 'Enter' })` returns `false`
      (i.e. `defaultPrevented`) **and** `onCreateTextFile` + `onClose` were
      called. Fails today (fireEvent returns `true`).
- [ ] **Unit (jsdom), Cancel path**: focus the Cancel button,
      `fireEvent.keyDown(cancelButton, { key: 'Enter' })` → expect
      `onCreateTextFile` **not** called. Fails today.
- [ ] **Browser-level regression (harness e2e)** — the only tier where the
      real mechanism reproduces: add a *stateful* DevHarness route (e.g.
      `dialog-new-file-stateful`) mirroring the Editor wiring — a trigger
      `<button>` that sets `isOpen`, `onClose` clearing it, a visible list of
      created files. New `*.harness.spec.ts` (pattern:
      `sidebar-keyboard.harness.spec.ts` / `bootHarness`): open via the
      button, type a name, press Enter → expect file recorded once, dialog
      **not** visible, focus on the trigger. Fails today. Note the existing
      `dialog-new-file` route is static-props and cannot express this.
      (Harness specs run via `playwright.harness.config.ts` — no hub tier
      needed. Mind the `ci-test-suite-unwired` lint note: harness specs ride
      the existing `test:harness` wiring, no new package/test script.)

### Phase 2 — fix

- [ ] Apply design (A) in `NewFileDialog.handleKeyDown`.
- [ ] Extend `ModalDialog`'s `onKeyDown` prop docstring with the
      preventDefault-on-close contract (comment-only change).
- [ ] All Phase-1 tests green.

### Phase 3 — hardening + verification

- [ ] `ShareDialog`: add `e.preventDefault()` to its Enter branch (behavior
      unchanged today, removes the latent trap). Covered by an assertion in
      its existing test file if one exists; otherwise a one-line unit test.
- [ ] `cd hub-client && npm run test:ci` and `npm run build:all` (production
      tsc is stricter — required by CLAUDE.md before claiming done).
- [ ] End-to-end check per CLAUDE.md: real browser against the dev server —
      New file → type name → Enter → dialog stays closed, file appears;
      Tab-to-Cancel → Enter → no file created. (Repro scripts from the
      investigation session can be re-run; see below.)
- [ ] Changelog: two-commit workflow — code commit first, then
      `hub-client/changelog.md` entry with the hash.

## Reproduction recipe (for the implementing session)

Local-only, nothing touches the public sync server:

1. `cargo build --bin hub && target/debug/hub --data-dir <tmp> --port 3799 --allow-insecure-auth`
2. `cd hub-client && VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:9/ws npm run dev -- --port 5199`
   (the unreachable default is fine; the setup form takes `ws://127.0.0.1:3799/ws`)
3. Playwright (workspace dep) headless: create project set → "+ New project" →
   "Default" → name it → editor. Click "New file" (sidebar, aria-label),
   fill `#filename`, `keyboard.press('Enter')` → observe
   `.qh-dialog.new-file-dialog` count is 1 again and the file exists.

Investigation scripts (session-scratchpad, not committed): `repro.mjs`,
`mechanism.mjs`, `cancel-enter.mjs` under the 2026-08-31 session scratchpad.
