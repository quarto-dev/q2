# GH #420 — rich-text editor loses focus on incoming CRDT changes

**Strand:** bd-84ljmbaf
**Issue:** https://github.com/quarto-dev/q2/issues/420
**Status:** diagnosed + fix validated by prototype (2026-08-26); implementation pending (TDD)

## Overview

In `q2 preview --allow-edit` (and hub-client, which uses the same
`preview-renderer` `PreviewRoot`), clicking into the rich-text editor and then
receiving an unrelated remote CRDT change disturbs the editor. Diagnosed live
against `q2 preview` with chrome-devtools (fixture: 3-paragraph project;
remote changes simulated by editing the file on disk, which the server syncs
into the CRDT).

### Observed behavior matrix (current main, browser-verified)

| Remote change | Editor fate | Focus | Caret | Uncommitted draft |
|---|---|---|---|---|
| After active block (no offset shift) | survives, same DOM node | kept | kept | kept |
| Before active block (offset shift, same block count) | **unmount + remount** | ~30ms blip (focusout→focusin) | **reset to end** | **rich: silently DISCARDED**; plain textarea: kept |
| Whole block inserted/deleted above | unmount + remount (index keys shift) | blip | reset | rich: discarded |
| Active block itself edited remotely | DROP: editor **closes** | moves to nearby block | — | discarded (by design, commit-on-drop guard) |

The reporter's "loses focus on any document edit" corresponds to rows 2–3:
*any* length-changing edit anywhere before the active block shifts its byte
offsets. The transient blip also drops keystrokes typed during the window, and
the rich surface loses all uncommitted edits — strictly worse than focus loss.

## Root cause

1. An incoming change bumps `contentTick` → WASM re-render → new
   `astJson`/`renderedContent` props reach `PreviewRoot`.
2. That render still carries the **stale** `editTarget` (old `anchorR0`). The
   dispatcher's edit-surface predicate
   (`ctx.editTarget.anchorR0 === resolved.sourceEntry.r[0]`,
   `dispatchers.tsx` ~line 134) fails for every block when offsets shifted →
   the block renders as normal content → `RichTextEditor` **unmounts** in that
   commit (tiptap instance destroyed).
3. The P2.3b self-heal `useLayoutEffect` (`PreviewRoot.tsx` ~line 388) then
   runs `findReanchorCandidate` (content-first, symmetric — robust), calls
   `setEditTargetRaw(reanchored)` → a second render **mounts a fresh editor**.
4. The fresh `RichTextEditor` seeds from the AST (`astToDoc`), not from the
   preserved `editDraftRef`, and its mount effect focuses `'end'` — so focus
   is programmatically restored but caret position, selection, undo history,
   and uncommitted doc content are gone. (`EditTextarea` reseeds from
   `editDraftRef`, which the self-heal deliberately preserves — that's why
   plain mode only blips. The rich surface never got the equivalent.)

The re-anchor *logic* is fine; the *timing* is the bug: it runs one render too
late, after React has already torn the editor down.

## Fix (validated by prototype)

Derive the re-anchored edit target **during render**, before children render,
so the first post-change render already matches the new offsets and React
reconciles the mounted editor in place (same position, same type, index key
unchanged for same-block-count edits):

```tsx
// in PreviewRoot render body, after the pool useMemo:
const effectiveEditTarget = useMemo(() => {
    if (editTarget === null) return null;
    const cand = findReanchorCandidate(
        pool, props.renderedContent ?? '',
        editTarget.anchorR0, editTarget.anchorSlice);
    if (cand && (cand.r0 !== editTarget.anchorR0 || cand.r1 !== editTarget.anchorR1)) {
        return { ...editTarget, anchorR0: cand.r0, anchorR1: cand.r1 };
    }
    return editTarget;
}, [editTarget, pool, props.renderedContent]);
editTargetRef.current = effectiveEditTarget;  // guards/commits agree with render
// ...and pass `editTarget: effectiveEditTarget` into PreviewContext.
```

The self-heal layout effect stays: it persists the re-anchor into state (KEEP,
now a same-values no-op render) and still owns the DROP path (content
mismatch → close + drop-focus). DROP behavior is unchanged.

Full prototype diff: `gh420-prototype.patch` (43 lines, 2 hunks, session
scratchpad — reproduce from this plan if lost; the memo above is the whole
change plus `editTarget: effectiveEditTarget` in the context value).

**Prototype verification (2026-08-26):**
- Invocation: `q2 preview --allow-edit --preview-dir q2-preview-spa/dist` on a
  3-paragraph fixture; editor opened on paragraph 1 with ` PROTO-DRAFT` typed;
  title in front matter edited on disk (offset shrink — the harder direction).
- Observed: same ProseMirror DOM node (instance marker survived), zero
  focusin/focusout events, caret offset unchanged (58), typed draft intact,
  remote change rendered. Output inspected via devtools script evaluation.
- `npm test` (578 passed) and `npm run test:integration` (628 passed) in
  `ts-packages/preview-renderer` — green with the prototype applied.

## Work items (TDD — test first)

- [x] Integration test (preview-renderer, jsdom):
      `editor-survives-remote-shift.integration.test.tsx` — real PreviewRoot,
      real pointer-event activation. Rich surface: element identity (`toBe`)
      + `document.activeElement` retention across forward AND backward
      offset shifts, plus a passes-before-the-fix baseline (edit after the
      block) proving the harness. Textarea surface: identity + dirty draft +
      focus. Verified FAILING first on the unfixed tree (3 failed / 1
      baseline passed, each on the remount assertions), then passing after
      the fix. (jsdom can't synthesize ProseMirror typing, so the rich
      dirty-doc guarantee is carried by element identity — a remount cannot
      preserve the element.)
- [x] Same-shape test for the textarea surface — included above.
- [x] Implement the derived `effectiveEditTarget` (PreviewRoot.tsx, after the
      pool useMemo; context gets the derived value; `editTargetRef` re-pointed
      at it).
- [x] Audit other `editTarget.anchorR0` consumers: dispatchers.tsx:135/402/
      464/474 and useBlockEditHover.tsx:232 read `ctx.editTarget` (context —
      derived); every PreviewRoot-internal callback reads `editTargetRef`
      (re-pointed at derived). The only state-value reader left is the reland
      fade effect, which tests null-ness only (identical between state and
      derived). No stale-vs-derived window remains.
- [x] Full gates: preview-renderer unit (578) + integration (632, incl. the 4
      new) suites green; `cargo xtask verify` full run + fresh browser e2e —
      see verification record below.

## Final verification record (2026-08-26, worktree bd-84ljmbaf)

- **TDD**: `editor-survives-remote-shift.integration.test.tsx` failed on the
  unfixed tree (3 failed: rich forward-shift, rich backward-shift, textarea —
  each on element identity/focus; the after-block baseline passed), then all 4
  passed after the fix.
- **Suites**: preview-renderer `npm test` 578 passed, `npm run
  test:integration` 632 passed. Full `cargo xtask verify` (all 14 steps,
  including hub-client `build:all` + `test:ci`): exit 0.
- **End-to-end** (real binary, output inspected): worktree
  `cargo run --bin q2 -- preview --allow-edit --no-browser --port 7481
  --preview-dir q2-preview-spa/dist <fixture>`; opened the rich editor on
  paragraph 1 in Chrome, typed ` E2E-DRAFT`; applied TWO offset-shifting
  remote edits on disk (front-matter title grow, then shrink). Observed via
  devtools scripting: same ProseMirror DOM instance (marker survived both),
  zero focusin/focusout events, caret unchanged (offset 49), draft intact,
  both remote changes rendered. Then committed with Cmd-Enter: the draft
  landed on the correct paragraph on disk with the remote title edit
  preserved — the commit destination is correct after two re-anchors.

## Follow-ups (separate strands, discovered-from bd-84ljmbaf)

1. **Block insert/delete above the active block still remounts** — the
   framework keys blocks by index (`key={i}` in `framework/dispatch.tsx`), so
   the active block's key shifts. Options: stable keys (hard — needs
   content-stable identity), or make an unavoidable rich-editor remount
   draft-preserving: keep the tiptap doc JSON + selection in a session ref
   (updated in `onUpdate`), reseed from it on remount instead of `astToDoc`,
   and restore selection — mirroring what `editDraftRef` already does for the
   textarea.
2. **Remote edit to the actively-edited block DROPs the editor** (focus loss
   the reporter flagged as "extremely cool" follow-up): merge remote content
   into the open editor instead of closing. Requires a 3-way story
   (base slice / local doc / remote slice); today's commit-on-drop guard is
   the safe default.

## Repro recipe (for regression checking)

1. Fixture: `_quarto.yml` + `index.qmd` with title front matter and 3 paragraphs.
2. `cargo run --bin q2 -- preview --allow-edit --no-browser --port 7480 <fixture>`
3. Open in Chrome; click into paragraph 1 (rich editor opens, focused). Type.
4. `sed -i '' 's/title: .*/title: something longer/' index.qmd` (offset shift).
5. Bug: editor remounts — focus blip, caret to end, typed text gone.
   Fixed: same node, caret and text intact.
