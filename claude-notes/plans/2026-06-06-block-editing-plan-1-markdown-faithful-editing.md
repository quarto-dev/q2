# Block editing — Plan 1: markdown-faithful editing on today's surfaces

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Builds on:** `claude-notes/plans/2026-06-04-target-incremental-writes.md`
**Phase:** 1 (series: 1, 2a, 2b, 3, 4). Frontend only. No Rust changes.

## Overview

Make the existing click-to-edit surfaces (paragraphs, headings) show the
**markdown source** of the node instead of `innerText`, by source-slicing the
node's original bytes out of the document `content`. This establishes the core
edit mechanism (content plumbing + byte-slice + textarea editor + edit-target
context) that Plan 2 generalizes, and makes the Header `#`-reprepend hack
(`Header.tsx:37-38`) obsolete. Uses the **existing** single-block
`apply_node_edit` — no Rust changes.

**Earliest visible win:** editing a paragraph shows `a *emph* word`; a heading
shows `## My Heading`; an edit replaces only the target block, and the rest of
the document is preserved verbatim. (Note: committing **unchanged** text is
*not* guaranteed to be a no-op — see Risks/"Submit is not a no-op".)

## Scope

**In:** `content` on `UPDATE_AST`; byte-accurate slice util; `PreviewContext`
gains `content` + `editTarget`; Para/Header switch to a `<textarea>` showing
sliced markdown; remove the `#` hack; commit/cancel keys (D6); empty=cancel (D10).

**Out:** the dual-node substrate + structural gate (Plan 2a); the generalized
interaction model — hover/press/keyboard affordance, render-component editing
(Plan 2b); nested blocks incl. callout bodies (Plan 3); ~~sections (Plan 4 — cancelled 2026-06-13)~~.
*(Plan 1's click-to-edit is retained as the seed of Plan 2b's model, not removed;
Plan 2a refactors Plan 1's interim `t==0 && d==0` gate onto `sourceNode`.)*

## TDD work items (tests first)

### Tests
- [ ] `ts-packages/preview-renderer/src/utils/sliceSource.test.ts` —
  `sliceBytes(content, start, end)` over **byte** offsets: ASCII; multibyte
  (accented + emoji) where UTF-16 `String.slice` would misalign; boundary at 0
  and `len`.
- [ ] Extend `hub-client/src/services/applyNodeEdit.wasm.test.ts`:
  - **Slice fidelity:** `sliceBytes(content, start, end)` for a paragraph and a
    heading returns exactly the block's source markdown (e.g. `a *emph* word`,
    `## My Heading`) — assert the slice content directly. This is what fills the
    textarea; do **not** append `\n`. Also assert that passing the raw slice
    (no trailing `\n`) directly to `parseQmdContentSync` succeeds — this verifies
    the WASM parser accepts unterminated lines, an assumption the commit path
    relies on.
  - **Surrounding-verbatim round-trip:** slice a block → `parse_qmd_content` →
    `apply_node_edit` → assert the **blocks outside the edited one are
    byte-verbatim** and no spurious blank lines are introduced at the boundary.
    Cover the **last** block (EOF `\n`) and a **middle** block — EOF handling
    differs from mid-document. **Do NOT assert global byte-identity**: committing
    re-serializes the edited block through the writer (see Risks); para/heading
    happen to be stable today but we don't pin that, and Tier-2 types reformat.
  - Paragraph inline markdown round-trip (`a *emph* word` survives the edit).
  - Heading round-trip via markdown (`## X`) with the `#` hack removed.
- [ ] Confirm existing `applyNodeEdit.wasm.test.ts` cases stay green.

### Implementation
- [ ] Byte-slice util — **generalize the existing UTF-8 byte-slice idiom** in
  `blocks/CodeBlock.tsx` (`utf8Encoder`/`utf8Decoder` + `Uint8Array.subarray`,
  `:62–130`) into two new files: `utils/utf8Slice.ts` exports
  `sliceUtf8(s, start, end)` = `decoder.decode(encoder.encode(s).subarray(start,
  end))` (the raw primitive); `utils/sliceSource.ts` exports
  `sliceBytes(content, start, end)` wrapping `sliceUtf8` — this is what block
  components import. The test file `utils/sliceSource.test.ts` tests `sliceBytes`.
  Refactor `CodeBlock` onto the shared primitive — but keep its **single**
  `encode(text)` outside the span loop (it slices many subarrays); do **not**
  re-encode per span (perf).
- [ ] Thread the **rendered-generation** content to the iframe — NOT the live
  editor content (see Risks/Generation skew):
  - `ReactPreview.tsx` / `PreviewApp.tsx` — capture the qmd text that produced
    the current render. In `ReactPreview`, **merge all three render-result
    values** — `astJson`, `untransformedAstJson`, and `renderedContent` — **into
    a compound state** `{ astJson: string | null, untransformedAstJson: string |
    null, renderedContent: string }` updated with a **single** setter call at
    `~:376` (using the `qmdContent` parameter already in scope). The initial
    value is `{ astJson: null, untransformedAstJson: null, renderedContent: '' }`
    — empty string is safe because editing requires a rendered preview (`astJson`
    non-null), so `renderedContent` is always populated before any slice runs. Do **not** use
    separate `useState` setters — sequential calls allow one render cycle to see
    a fresh `astJson` with a stale `renderedContent`, defeating the skew
    guarantee. This replaces both `setAst(result.astJson)` (line 376) and
    `setUntransformedAst(result.untransformedAstJson ?? null)` (line 378); the
    `handleSetAst` dep array changes `untransformedAst` →
    `compound.untransformedAstJson`. In `PreviewApp`, add `renderedContent: string`
    to `PreviewAppState` and include it alongside `astJson`/`untransformedAstJson`
    in the same `setState` call (`~:728`) — `renderedContent` is the VFS content
    read into a local variable just before calling `renderPageForPreview`.
  - `iframe/Q2PreviewIframe.tsx` — add `content?: string` to
    `Q2PreviewIframeProps`, include it in the `UPDATE_AST` payload (`~:166`),
    and add it to the effect's dep array (alongside `astJson`). No extra
    `postMessage` calls result — `content` and `astJson` always change together
    via compound state, so the dep satisfies exhaustive-deps without causing
    additional firings.
  - `hub-client/src/components/render/ReactRenderer.tsx` — add
    `renderedContent?: string` to `ReactRendererProps`, destructure it, and
    forward to `Q2PreviewIframe`; `ReactPreview.tsx` passes
    `compound.renderedContent` down. `PreviewApp.tsx` renders `Q2PreviewIframe`
    directly at line ~886 (no intermediate component) and passes
    `renderedContent={state.renderedContent}` directly.
- [ ] `q2-preview/entry.tsx` — `UpdateAstPayload`/`updateAst`/`PreviewRoot` gain
  a `content` field; `PreviewRoot` **also owns the interactive edit state**
  `const [editTarget, setEditTarget] = useState<string | number | null>(null)`
  (the payload carries `content`; `editTarget` is local UI state, not from
  `UPDATE_AST`). Both are supplied via `PreviewContext`.
- [ ] `q2-preview/PreviewContext.tsx` — add `content?: string`,
  `editTarget?: string | number | null`, and `setEditTarget?`. Target = pool id
  for v1; `setEditTarget(poolId)` on click, `setEditTarget(null)` on
  cancel/commit.
- [ ] `q2-preview/blocks/Para.tsx`, `Header.tsx` — remove the per-component
  `const [editing, setEditing] = useState(false)` entirely; a block is the edit
  target when `ctx.editTarget === poolId`. Click calls `ctx.setEditTarget(poolId)`;
  commit and cancel both call `ctx.setEditTarget(null)` (after `commitEdit` on
  commit, unconditionally on cancel). When this block is the `editTarget`, render
  an in-place `<textarea>` (monospace, sized to the measured box, auto-grow)
  pre-filled with `sliceBytes(content, start, end)`, where `[start, end]` come
  from `pool[poolId]`; commit on ⌘/Ctrl+Enter or blur via `commitEdit(poolId,
  newText)` (no `\n` append); Esc and empty value cancel. Remove the `#`
  reprepend. Remove the `+ '\n'` from Para's `commitEdit` call (current
  `Para.tsx:32` appends `\n`; the textarea path sends raw sliced markdown and
  the Rust writer owns boundary newlines). Drop the `innerText`/nbsp/`viewKey`
  machinery (textarea supersedes it).
- [ ] **Minimal slice guard (interim — full gate is Plan 2).** The slice already
  fetches `pool[poolId]`; gate the slice-edit on `content != null && entry?.t === 0
  && entry?.d === 0` (content present, Original, active file) — field-reads on the
  entry already in hand, plus a defensive `content` presence check.
  Non-`Original` (`t≠0`) or included-file (`d≠0`) blocks would slice the wrong
  bytes from the active content, so keep them off the textarea path. The richer
  container/section editability gate is Plan 2.

## End-to-end verification (per CLAUDE.md)
- [ ] `cd hub-client && npm run build:wasm` (Plan 1 is JS-only, but rebuild to be
  safe), then dev server. **Use a single-file fixture with no `{{< include >}}`
  and no filter/shortcode-generated paragraphs** — Plan 1's scope is active-file
  `Original` blocks only, so a fixture with included/generated blocks could demo
  a (known, Plan-2-gated) garbage slice and mislead the verification. Edit a
  paragraph → textarea shows `*emph*`; edit a heading → shows `## X`; after
  commit the edited block changes and **surrounding blocks are unchanged** in the
  VFS/Automerge content. **Cancel** (Esc or clear-to-empty) → content fully
  unchanged. Do **not** expect commit-unchanged to be a zero diff — that is *not*
  guaranteed (see Risks/"Submit is not a no-op"). Record the exact steps +
  observed output in this file.
- [ ] Click directly from one editable block to another (no Esc between them) —
  verify the first block commits and the second opens without a visible flash.
  Browser event order (blur before click) should produce the correct functional
  result; confirm React 18 batches the two `setEditTarget` calls so there is no
  intermediate render with no edit target.
- [ ] `npm run build:all` (production build is stricter than tsc/vitest).

## Risks / watch-items
- **Generation skew (resolved by `renderedContent`):** the pool ranges belong to
  the *rendered* AST generation, which lags the *live* `content` — `astJson` is
  async-render state, `content` is immediate Automerge state, so they are **not**
  updated together. Ship the `renderedContent` snapshot captured at the `setAst`
  site, never the live `content` prop. With the snapshot the two move in lockstep
  and the iframe's `content` can never be offset from its pool. (The existing
  `apply_node_edit` commit path already pairs live `content` with the rendered
  `untransformedAst` at `ReactPreview.tsx:464` — that tolerates skew because
  commits happen on a settled preview; the *slice* is read at edit-start and is
  more exposed, hence the snapshot.)
- **Active-file `file_id` / non-`Original`:** the minimal slice guard
  (`entry.t === 0 && entry.d === 0`) handles this in Plan 1; the full editability
  gate (container/section) is Plan 2, which **must** test the `t` and `d`
  conjuncts (added to Plan 2's test list).
- **Submit is not a no-op (known, accepted risk).** Committing the *same* sliced
  text is **not** guaranteed to leave the document byte-identical. The commit
  round-trips the edited block through `parse_qmd_content` → `apply_node_edit` →
  the writer, which **re-serializes** that block — and the writer is free to
  reformat in the area of the change: bullet chars (`*`/`+`→`-`), ordered-list
  markers and renumbering, blockquote `>` reflow, **table** column padding, and
  **definition lists**. Para/heading happen to be stable today, but we do not
  rely on it. The **only** guaranteed no-op is to **cancel** — Esc, or clear the
  textarea (empty = cancel, D10). We deliberately do **not** add client-side
  "text unchanged → skip commit" detection in Plan 1 (and likely never): a user
  who opens an editor and commits identical text may see the block reformatted,
  and that is acceptable. What we *do* guarantee is that blocks **outside** the
  edited one stay byte-verbatim (the writer's `KeepBefore`→`Verbatim` copy).
- **Trailing newline (slice convention, not a round-trip oracle).** Slice exactly
  `[start,end]` from the pool and never append `\n` — appending would inject a
  spurious blank line into the textarea and, on commit, into the boundary. The
  *round-trip* formatting of the edited block is owned by the Rust writer, not by
  the slice util; if a boundary newline looks wrong after an edit, fix it in the
  writer / pool range, not by massaging the slice.

## Post-success follow-up

- [ ] **File a bead** for the `handleSetAst` byte-offset correctness risk in
  `ReactPreview.tsx`: the write-back path uses the live Automerge `content` prop
  for `apply_node_edit`, but the pool byte offsets in `destinationSourceInfoJson`
  belong to the *rendered* generation (`compound.renderedContent`). If the user
  commits an edit while a re-render is in-flight, the offsets may not match the
  live content. `compound.renderedContent` is now available and could replace the
  live prop; deferred because commits are assumed to happen on a settled preview
  and `PreviewApp.tsx`'s already-correct use of `state.renderedContent` can serve
  as the Playwright repro target. Include a failing test that edits, then types
  quickly before the re-render settles, then commits — assert the surrounding
  blocks are unchanged.

## References
- Spec D1, D2, D5, D6, D8, D10.
- `apply_node_edit.rs`, `writers/json.rs` (byte offsets), `types/sourceInfo.ts`.
