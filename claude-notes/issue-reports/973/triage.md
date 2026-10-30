# Issue #973 — pressing Enter on a comment does nothing when Edit is off

- **GitHub**: https://github.com/quarto-dev/q2/issues/973
- **Reporter**: @midleman (Marie Idleman), 2026-10-30
- **Triage date**: 2026-10-30
- **Branch**: topic branch `issue-973` in the main checkout, based on `main` @ `ad262fde8` (no worktree, by request)
- **Braid strand**: bd-r2withkk
- **Scope**: the silent comment loss with the Edit pill off, plus the same loss in `q2 preview` without `--allow-edit`. Commenting on a text selection in read mode is excluded (see Open questions).

## Summary

Reproduced: with Edit off, the comment box accepts text, Enter clears nothing visible, and the comment is never saved. This is a real bug. The renderer offers the comment UI while the host discards the commit. Decision (Carlos): allow comments with Edit off. The Edit pill governs editing *content*; comments are in-band in Quarto 2, but keep a distinct UI affordance.

## Reproduction

`hub-client/e2e/q2-preview-edit-toggle.spec.ts`, test "adds a comment with editing off": turn Edit off, hover a paragraph, click `+`, type, press Enter.

```bash
cd hub-client && VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws npm run build
npx playwright test e2e/q2-preview-edit-toggle.spec.ts --project=chromium --workers=1
```

With the original guard restored, the test fails: the bubble holds only `Enter to add · Esc to close`, and the comment never appears. With the fix, it passes.

## Localization

- `hub-client/src/components/render/ReactPreview.tsx`, `handleSetAst`: `if (!previewEditing) return;` dropped every payload, comments included (bd-ew0vak6b, plan decision D7).
- `ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`: the chrome ignores `editingDisabled`, so it stayed fully usable. After Enter, the input only closes once the comment appears in the list, which never happened.
- `q2-preview-spa/src/PreviewApp.tsx`, `handleSetAst`: `if (!state.allowEdit) return;` is the same pairing, so a read-only `q2 preview` loses comments the same way. Found by reading the code; no browser run.

## Open questions — resolved during triage

- **Allow comments, or hide them?** Allow (Carlos).
- **How does the host tell a comment from a content edit?** Comment commits now carry `scope: 'comment'` on the subtree payload, and the hub guard exempts that scope. Task-list checkboxes use the same channel without the tag, and remain off in read mode.
- **What about hosts that cannot write at all?** The SPA without `--allow-edit` has nowhere to save a comment, so exempting it would not help. A new `commentingDisabled` flag makes bubbles display-only there: existing comments can be read and expanded, with no `+`, input, or ✓. The SPA's guard is unchanged.
- **Does the reporter's "select some text" flow work with Edit off?** Not as a selection. Span comments on a selection start from the block editor's toolbar, which does not exist in read mode. Comments attach through the block's `+` or an existing span's bubble. Selection commenting in read mode would be a new feature; not filed, pending demand.
- **Could the host guard also drop the draft that closes when Edit is turned off (plan D6)?** Possibly. From reading only: a mouse click on the pill blurs the editor, which commits while editing is still on. Other routes were not checked, and nothing was run. No strand filed.

## Outcome / recommended next step

Filed bd-r2withkk; the fix is the next commit on this branch.

## Verification commands used

```bash
gh issue view 973 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build
cd hub-client && npm run build:wasm && npm run test:wasm
```

Pre-flight note: the first `verify` failed `smokeAll.wasm.test.ts` on six TOC assertions from a stale local WASM bundle. After `npm install` and `npm run build:wasm`, `test:wasm` passed 465/465, so HEAD was not broken.

## Cross-references

- bd-ew0vak6b, `claude-notes/plans/2026-09-09-q2-preview-edit-toggle.md` (the Edit pill)
- bd-ov4gqk3m (`editingDisabled`, `--allow-edit`)
