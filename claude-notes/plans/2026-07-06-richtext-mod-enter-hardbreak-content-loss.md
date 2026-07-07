# Rich-text editor: `Mod-Enter` commit drops selected content (HardBreak collision)

**Strand:** bd-hafs0qho (discovered-from bd-7pxub583; related to bd-sjb4pzx8)
**Status:** IN PROGRESS — user approved (A)+(B) on 2026-07-06

## Decision locked with user (2026-07-06)

Do **both (A) and (B)**:
- **(A)** Move the `Mod-Enter` commit into tiptap's keymap so it wins the race
  and suppresses `setHardBreak`; retire the DOM `addEventListener` commit path.
- **(B)** Disable HardBreak's `Mod-Enter` binding (keep `Shift-Enter`).

Rationale (user): a hard break on `Mod-Enter` is never wanted here — the rich
editor is **not** intended for extensive multi-block editing; the preview's
plain-text editor and hub-client's source editor cover those cases.
**Date:** 2026-07-06

## Summary

In the `format: q2-preview` rich-text (tiptap) block editor, committing with
`Mod-Enter` while text is **selected** deletes the selected text. The most
visible path: click a block → select-all (`Mod-a`) → bold (`Mod-b`) → commit
(`Mod-Enter`) → the block is written to disk **empty** (a tight list item
`- banana` becomes `-`; a paragraph is deleted entirely).

It is **not** specific to `Plain` / list items and **not** introduced by
bd-7pxub583 — it reproduces identically on a `Para`, in the shared,
type-agnostic RichTextEditor path, and predates the Plain-gate change.

## Reliable reproduction (done)

`hub-client/e2e/q2-preview-richtext-bold-content-loss.spec.ts` (Playwright,
real CDP keyboard events — the only faithful vehicle; jsdom and MCP-browser
synthetic events were confounded). Two cases: a tight bullet-list item and a
paragraph. Each opens the rich editor, does `Mod-a` / `Mod-b` / `Mod-Enter`
(via Playwright `ControlOrMeta`, which maps to ProseMirror's `Mod`), and asserts
the committed qmd contains the **bolded** text. **Currently RED** — the file is
written with the item emptied:

```
## List

* apple
*            ← "banana" gone
* cherry
```

> Cross-platform note: use `ControlOrMeta` in the spec. Plain `Control` on macOS
> does NOT trigger ProseMirror's `Mod-b` (which is Meta on mac), so bold never
> fires and the bug is masked — the reason two earlier runs falsely "passed".

## Root cause (confirmed)

tiptap's **HardBreak extension binds `Mod-Enter` → `setHardBreak()`** (verified
in `@tiptap/extension-hard-break/dist/index.js`; it binds both `Mod-Enter` and
`Shift-Enter`). RichTextEditor *also* uses `Mod-Enter` for commit — but via a
**DOM `addEventListener('keydown', …)`** on `editor.view.dom` that is registered
*after* ProseMirror's keymap plugin. So on `Mod-Enter`:

1. ProseMirror/tiptap's keymap runs first → `setHardBreak()` **replaces the
   current selection** with a hardBreak node. After select-all + bold the whole
   text is selected → the text is **deleted**, leaving `paragraph[hardBreak]`.
2. RichTextEditor's DOM handler runs next → `e.preventDefault()` (too late to
   undo step 1) → `commit()` → serializes `paragraph[hardBreak]` → `docToMarkdown`
   correctly yields `""` → an empty block is committed.

Corroborating evidence:
- Instrumented commit (captured earlier) showed the committed doc was exactly
  `{paragraph:[{hardBreak}]}`, md `""`.
- The **serializer** and **Rust `apply_node_edit`** are both verified correct in
  isolation, and **pure tiptap `selectAll()+toggleBold()+serialize`** (no
  `Mod-Enter`) is clean — the damage only appears when `Mod-Enter` is pressed.
- Simple text edits "worked" but the committed doc always carried a **trailing**
  hardBreak (`[text, hardBreak]`) — the same `Mod-Enter`→hardBreak insertion, but
  harmless at a collapsed cursor because it appends rather than replaces, and a
  trailing hardBreak serializes to nothing.

So the defect is general: **`Mod-Enter` inserts a hardBreak on every commit**; it
is silent at a collapsed caret and destructive over a selection.

## Fix options (to discuss)

The goal: `Mod-Enter` must commit **without** inserting a hardBreak, deterministically.

- **(A) Move commit into tiptap's keymap (recommended).** Add a small tiptap
  extension (or `editor`-level `addKeyboardShortcuts`) binding `Mod-Enter` to a
  handler that fires the commit and returns `true`. Because it lives in
  ProseMirror's keymap with higher priority than HardBreak, it wins the race and
  suppresses `setHardBreak`. This also retires the fragile DOM `addEventListener`
  commit path (the root of the race). Escape/plain-Enter handling can move the
  same way for consistency.
- **(B) Disable HardBreak's `Mod-Enter` binding.** Configure the HardBreak
  extension (via StarterKit) so only `Shift-Enter` inserts a hard break; then
  `Mod-Enter` falls through to the existing DOM commit handler with nothing to
  race. Smaller change, but leaves the DOM-listener commit path in place.
- **(C) Change the commit key.** Least desirable — `Mod-Enter` is the expected
  "confirm" gesture; keep it.

Leaning **(A)**, optionally plus **(B)** as belt-and-suspenders (a hard break on
`Mod-Enter` is never wanted in this single-block editor). Keep `Shift-Enter` as
the intentional hard-break key (the editor already treats `Shift+Enter` as a
hard break; plain `Enter` is swallowed).

Either way, also confirm the **trailing-hardBreak-on-plain-commit** disappears
(no more `[text, hardBreak]` docs), so commits stop carrying a stray break.

## Plan (TDD)

### Phase 1 — Reproduction (DONE)
- [x] Playwright e2e repro (`q2-preview-richtext-bold-content-loss.spec.ts`),
      red today; covers list-item **and** paragraph (shared-path proof).

### Phase 2 — Root cause (DONE)
- [x] Identified the `Mod-Enter` ↔ HardBreak `Mod-Enter` collision + DOM-listener
      race; corroborated by the captured `paragraph[hardBreak]` commit doc.

### Phase 3 — Tests first
- [x] Keep the e2e repro as the end-to-end regression gate (RED before the fix).
- [x] **Fast editor-config unit test** (`richtext/editorConfig.test.ts`, jsdom):
      builds the editor via the shared `buildRichTextExtensions()`, select-all,
      dispatches a real `Mod-Enter` keydown, asserts the doc is NOT mutated to a
      hardBreak (text intact). Also: collapsed-caret `Mod-Enter` leaves no
      trailing hardBreak; `Shift-Enter` still inserts one. **Confirmed RED** with
      the fix reverted (default HardBreak), green with it.

### Phase 4 — Implement the fix (A)+(B)
- [x] **(B)** `richtext/editorConfig.ts`: shared static extension config;
      disables StarterKit's built-in HardBreak and re-adds one bound to
      `Shift-Enter` only. Added `@tiptap/extension-hard-break` as a direct dep.
- [x] **(A)** `RichTextEditor.tsx`: Esc/`Mod-Enter`/`Enter` moved into a
      high-priority tiptap keymap extension (`q2CommitKeymap`) via a handlers
      ref; the racy DOM `keydown` listener is removed (focusout-commit kept).
- [x] Preserved behavior: Escape cancels; plain Enter swallowed; `Shift-Enter`
      is a hard break; dirty guard / stale-target guards unchanged.
- [x] tsc clean; full preview-renderer suites green (487 unit incl. the 3 new,
      517 integration); the editor-mount integration tests (p3-4-inline-breadcrumb,
      RichTextEditor.caret) still pass.

### Phase 5 — Verify
- [x] **e2e repro GREEN** (was red). Both cases now assert the *bolded* text is
      present on disk: `**banana**` (list item) and `**Hello world paragraph.**`
      (paragraph). This is the faithful end-to-end check — real hub + real
      browser + real `Mod`-keyboard, asserting the Automerge/disk content. It
      supersedes the earlier MCP-browser manual check (which was confounded by
      synthetic-event hardBreak injection).
- [x] hub-client build (VITE_E2E) green; hub-client unit tests green (229).
- [ ] `cargo xtask verify` (hub leg) — run before push.
- [ ] hub-client changelog entry (two-commit workflow) since `hub-client/` is
      touched (the e2e spec) and the fix is user-facing (data-loss fix in the
      bundled editor). To add as part of the commit.

## Status: implementation complete + verified; ready to commit

Working tree = the fix + tests + plan only (build-artifact churn reverted).
Awaiting go-ahead to commit + open a PR (will run `cargo xtask verify` and add
the changelog entry as part of that).

## Files likely in scope

- `ts-packages/preview-renderer/src/q2-preview/richtext/RichTextEditor.tsx` —
  the `useEditor` extension config + the keydown/commit handling (the fix).
- Possibly a new tiny extension module for the `Mod-Enter` keymap.
- `hub-client/e2e/q2-preview-richtext-bold-content-loss.spec.ts` — the repro
  (already written; may add assertions).
- A new fast unit/integration test under `ts-packages/preview-renderer/…/richtext/`.

## Non-goals

- Structural list editing (Enter-to-split, Tab-to-nest) — separate (bd-sjb4pzx8
  Phase 1c).
- Any change to the serializer or the Rust commit path — both verified correct.
