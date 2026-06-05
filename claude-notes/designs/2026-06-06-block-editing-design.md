# Block editing in q2-preview — design / master spec

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Status:** Design approved in brainstorming; spec under review. Expands into a
four-plan series under `claude-notes/plans/`.
**Builds on:** `claude-notes/plans/2026-06-04-target-incremental-writes.md`
(the `apply_node_edit` / source-slice round-trip this feature extends).

---

## Overview

Two refinements to contentEditable in q2-preview, delivered as a progress-early
series:

1. **Edit the markdown, not the rendered text.** When editing starts, the box
   shows the *markdown source* of the node (e.g. `## Heading`, `a *emph* word`),
   obtained by **source-slicing** the node's original bytes out of the document
   `content`. This makes the Header `#`-reprepend special case
   (`Header.tsx:37-38`) obsolete. (Note: this makes the *edit buffer* faithful to
   the source; it does **not** make re-submitting unchanged text a no-op — the
   commit re-serializes the edited block. See "Submit is not a no-op" in Edge
   cases.)
2. **A pencil on every (source-backed) block.** In addition to the existing
   click-to-edit on paragraphs/headings, a hover pencil appears in the top-right
   of every source-backed block — including sections. Clicking it turns the whole
   block into a same-sized markdown editor.

Both ride the existing write-back core: `parse_qmd_content → splice into the
untransformed AST → compute_reconciliation → incremental_write` (see the
target-incremental-writes plan). Nothing about the reconcile/writer core changes.

## Decisions (locked in brainstorming)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Source-slice only** for the edit buffer (option A). | The editor shows the exact original bytes; no WASM needed to fill the buffer. (Commit still re-serializes — submit is not a no-op; see Edge cases.) Non-sliceable nodes get no pencil. |
| D2 | **No request/response postMessage.** Ship `content` on `UPDATE_AST`; the iframe slices locally. | The iframe has the pool ranges; it only lacked `content`. Slicing is pure JS. |
| D3 | **Editable = `Original` (`t:0`) source-backed blocks + sections** (via source range). | `Substring`/`Concat`/`Generated` ranges aren't absolute in the pool (need `preimage_in`, which is Rust). User-authored blocks parse as `Original`. You edit source, not generated output. |
| D4 | **Deepest block under the cursor.** Floating overlay pencil, no wrapper divs. | Theme CSS relies on `>`/`+`/`:nth-child` among block elements; a wrapper breaks it. Mirror the existing `useAttributionHover` mechanism. |
| D5 | **Editor = `<textarea>`**, monospace, sized to the block. | Plain-text markdown source: preserves newlines, no `&nbsp;`/`<br>`/paste mangling; drops the nbsp-normalization + `viewKey` remount hacks. |
| D6 | **Commit = ⌘/Ctrl+Enter; Enter = newline; Esc = cancel; blur = commit.** | Multi-line-safe (sections, code blocks); consistent across block types. |
| D7 | **Sections: frontend envelope + backend range.** No sectionize change. | The generated section `Div` is `Generated{by:sectionize, from:[]}` → not sliceable; and a section spans multiple untransformed blocks → a range lookup is needed regardless. |
| D8 | **Edit only the active file** (`file_id == 0`). | `apply_node_edit` hardcodes `FileId(0)`; the `INCLUDE_DOC` test already rejects cross-file edits. |
| D9 | **Pencil is sticky to the active block.** Appears on hover, stays anchored until a different editable block is hovered (or editing starts / click elsewhere); anchored in the scrolling content so it tracks scroll. | The attribution badge clears on mouse-out (`attribution.txt:224-229`), so a corner pencil would vanish before it can be clicked. Sticky sidesteps the cursor-travel gap and stale `position:fixed` coords. |
| D10 | **Empty commit = cancel** (block untouched); deletion is not a clear-to-delete gesture in v1. | Matches today's `if (newText)` guard; avoids accidental destruction without a clear preview undo story. |

## Key facts established by research (with refs)

- **`AttributionWrap` is a passthrough in preview** — `attribution.tsx:153`
  (`if (!attribution) return <>{children}</>`). Blocks are *not* wrapped in
  q2-preview, which is why the theme CSS works. We must not wrap either.
- **`useAttributionHover` is the affordance precedent** — `attribution.tsx:189`:
  delegated `onMouseOver` on the document root,
  `target.closest('.q2-attr-wrap[data-sid]')` (nearest ancestor = deepest),
  `getBoundingClientRect()` for a floating overlay. `PreviewDocument` already
  uses it.
- **Wrappers break theme CSS** — e.g. `main.content > p:has(+ section)`,
  `main.content > section:first-of-type > h2:nth-child(1)`,
  callout `> .callout-header`, list `ul > li` selectors
  (`resources/scss/bootstrap/_bootstrap-rules.scss:834,838,1794-1858`).
- **Pool entry shape** (`types/sourceInfo.ts:69-77`):
  `Original` = `{t:0, r:[start,end], d:file_id}`. `r` are **UTF-8 byte
  offsets** (`source_info.rs:10`, `json.rs` `start_offset`/`end_offset`).
  → slice via `TextEncoder`/`Uint8Array`/`TextDecoder`, not `String.slice`.
- **All block components return a single root element** (Para/Header/Div/
  CodeBlock — `q2-preview/blocks/*`), so each can spread an injected
  `data-block-pool-id` onto its own root (no wrapper).
- **`preimage_in`** (`source_info.rs:435-471`): `Some(range)` for `Original`
  (if `file_id` matches), composed for `Substring`, contiguous-only for
  `Concat`, and only via an `Invocation` anchor for `Generated` (→ `None` for
  sectionize Divs, which have `from: []`).
- **The writer already supports N→M block replacement** preserving bytes
  outside the span — `compute_reconciliation` + `incremental_write` coarsen/
  assembly (`incremental.rs`; `KeepBefore`→`Verbatim` copies original bytes,
  changed blocks `Rewrite`/`InlineSplice`). Plan 4 needs only range
  lookup+splice, not writer changes.
- **Current splice** `a_u_prime.blocks.splice(idx..=idx, subtree.blocks)`
  (`apply_node_edit.rs:154`) already does 1→N; range needs `i..=j`.
- **Container shapes** (`quarto-pandoc-types/src/block.rs`): `Div/BlockQuote/
  Figure` hold `Blocks`; `BulletList/OrderedList` hold `Vec<Blocks>`;
  `DefinitionList` holds `Vec<(Inlines, Vec<Blocks>)>` → a path locator is a
  sequence of indices.

## Architecture

### Frontend (TypeScript)

- **`content` on `UPDATE_AST`** (`Q2PreviewIframe.tsx:166`): one new payload
  field, threaded from the parent (`ReactPreview`/`PreviewApp` both hold the
  rendered-generation `content`).
- **`PreviewContext`** gains `content: string` and an `editTarget` /
  `setEditTarget` pair. (Keeps `pool`, `commitEdit`.)
- **Byte-slice util** (`utils/sliceSource.ts`, new):
  `sliceBytes(content, start, end): string` via `TextEncoder`/`TextDecoder`.
- **Editable-block mechanism**: a block component, when it is the `editTarget`,
  renders a `<textarea>` **in place** (monospace, sized to the block's measured
  box, auto-growing from that height) pre-filled with its sliced markdown;
  commit (⌘/Ctrl+Enter or blur) calls the existing `commitEdit(poolId, newText)`
  (or the range variant); Esc cancels; an empty value cancels (D10). The
  in-place swap accepts a transient CSS reflow during the edit (D5/the element
  is briefly a `<textarea>` rather than `<p>`/`<section>`).
- **`useBlockEditHover`** (new, modeled on `useAttributionHover`): delegated
  `onMouseOver` on the `PreviewDocument` root; `closest('[data-block-pool-id]')`
  → deepest block → set as the **active** block. A single pencil button is
  rendered as a root-level sibling, anchored to the active block within the
  scrolling content (tracks scroll), and **stays until a different editable
  block becomes active** (D9). Click sets `editTarget`.
- **`data-block-pool-id`** spread onto each block component's root element
  (CSS-safe, no wrapper). Sections instead carry `data-section-range` (the
  envelope) since the section `Div` is not `Original` (Plan 4). The hover
  hit-test matches either marker.
- **Editability gate via context** (Plan 2): non-section containers
  (`Div` without `.section`, lists, `BlockQuote`) push an "inside-container"
  flag through a context; a block is editable iff `Original && file_id==0 &&
  !insideContainer`. Sections are transparent (push nothing). Plan 3 removes the
  `!insideContainer` restriction.

### Backend (Rust / WASM)

- **`apply_node_edit`** (single target) — unchanged for Plans 1–2.
- **`lookup_block`** — Plan 3 extends to recurse into containers, returning a
  **path** (`Vec<usize>` with list-item sub-indices); `apply_node_edit` splices
  at the path.
- **`lookup_range` + range splice** — Plan 4: given `[start,end]` (active-file
  byte range), find the contiguous span of untransformed top-level blocks within
  it and `splice(i..=j, new_blocks)`. New WASM entry / payload `range` variant.

### Data flow (edit)

```
render → parent sends UPDATE_AST { astJson, content, … } → iframe
hover  → useBlockEditHover → pencil over deepest [data-block-pool-id] block
click  → setEditTarget(poolId | sectionRange)
edit   → target block renders <textarea> filled by sliceBytes(content, start,end)
commit → commitEdit → SET_AST { PreviewNodeEditPayload }  (one-way, existing)
parent → parseQmdContentSync(newText) → applyNodeEdit | applyRangeNodeEdit (WASM)
       → onContentRewrite(newQmd) → re-render → fresh UPDATE_AST
```

## Phase breakdown (the four-plan series)

Each phase ships an end-to-end-verifiable increment (TDD-first per repo rules).

### Plan 1 — Markdown-faithful editing on today's surfaces *(frontend only)*
Establishes the core mechanism. `content` on `UPDATE_AST`; `sliceBytes` util;
`PreviewContext.content` + `editTarget`; convert Para/Header click-to-edit to a
`<textarea>` showing sliced markdown; delete the `#` hack. Uses the **existing**
single-block `apply_node_edit`.
**Win:** editing a paragraph shows `*emph*`, a heading shows `## X`; an edit
replaces only the target block and the rest of the document stays verbatim.
**Tests:** `sliceBytes` (multibyte); slice-fidelity + surrounding-verbatim WASM
round-trip (**not** global byte-identity — see Edge cases); existing
`applyNodeEdit.wasm.test` still green; browser check.

### Plan 2 — Hover pencil + generalized block editor *(frontend only)*
`useBlockEditHover` + floating pencil; `data-block-pool-id` spread on block
roots; generalize the textarea editor to all editable blocks. **Editability
gate:** `Original` + `file_id==0` + **no non-section container ancestor** (so it
commits via the existing top-level `lookup_block`). Section pencils suppressed
(pending Plan 4). **Win:** pencils everywhere applicable; click-edit any
top-level block type (code, quote, list, para, heading). **Tests:** deepest-
block hit-testing; gate logic (container-ancestor); pencil render/position;
browser check editing a code block + blockquote.

### Plan 3 — Nested-block descent *(Rust + thin frontend)*
`lookup_block` recurses into `Div/BlockQuote/list/DefinitionList`, returning a
path; `apply_node_edit` splices at the path. Frontend: drop the
"no container ancestor" gate so nested pencils go live (payload unchanged).
**Win:** edit a paragraph inside a callout / fenced div / list item.
**Investigate first (done):** the writer always re-serializes the whole
container wholesale — `RecurseIntoContainer` alignment falls back to `Rewrite`
for every block container in `coarsen_plan_phase5` (`incremental.rs:218-220`).
Sibling list items renumber / re-bullet, `>` reflows, tables re-pad. Accepted
for v1: the guarantee is that bytes **outside** the container stay verbatim.
Extending the writer to recurse block-by-block is a possible follow-up.
**Tests (Rust `node_edit_tests`):** nested lookup for each container; path
splice; sibling fidelity — **byte-verbatim if the writer recurses, else snapshot
the reformatted container** (do not assert sibling byte-identity
unconditionally); 1→N nested. Browser e2e.

### Plan 4 — Section editing *(Rust + frontend)*
Frontend computes a section's source envelope `[min start, max end]` over its
`Original` descendants; payload gains a `range` variant; `lookup_range` +
range-splice (`i..=j`) in Rust; section pencils go live. Writer unchanged.
**Win:** edit a whole section (heading + body) at once; nested sections handled
by the envelope. **Tests (Rust):** `lookup_range`; N→M range replace (e.g.
heading+3 paras → 2 blocks) with byte preservation; nested-section envelope;
boundary-alignment rejection. Frontend envelope util test. Browser e2e.

## Edge cases & notes

- **Submit is not a no-op.** Committing the same sliced text does **not**
  round-trip byte-identical: the commit re-serializes the edited block through
  the writer, which reformats in the area of the change — bullet chars,
  ordered-list markers/renumbering, blockquote `>` reflow, **table** padding,
  **definition lists**. Para/heading are stable today but not pinned. The only
  guaranteed no-op is **cancel** (Esc / empty-clear, D10); we do **not** enforce
  client-side "unchanged → skip" (Plan 1, likely never). The guarantee we keep is
  that blocks **outside** the edited one stay byte-verbatim.
- **Slice/trailing-newline convention:** slice exactly `[start,end]`; do **not**
  append `\n` (avoids a spurious blank line in the buffer and at the commit
  boundary). Round-trip formatting of the edited block is the Rust writer's
  concern, not the slice util's.
- **Active-file gate:** editable iff `Original` and `file_id==0`; assumes the
  active page is `file_id 0` (consistent with `apply_node_edit`'s `FileId(0)`).
  Verify in Plan 1.
- **Section with no `Original` descendant** (only generated content) ⇒ no
  envelope ⇒ no pencil. Acceptable.
- **Generated block inside an edited range/section:** its invocation source is
  within the sliced span and is an untransformed block in range, so it
  round-trips; the envelope is bounded by surrounding `Original` blocks.
- **Concurrency:** edits are pinned to the rendered `untransformedAst`
  generation (existing contract). v1 is single-user; drift ⇒ re-render first.

## Out of scope (v1)

- Inline-span editing (only `Cite`/`Note` inlines carry `s`; blocks only here).
  This also means definition-list **terms**, short captions (`Caption.short`),
  and `CaptionBlock` (all `Inlines`) are not editable.
- Block-by-block editing **inside table cells, table captions, and figure
  captions** (`Cell.content`, `Table.caption.long`, `Figure.caption.long`).
  Plan 3 descends `Div`/`BlockQuote`/figure-**body**/lists/definition-**bodies**
  only; these regions stay whole-block Tier-2 edits (Plan 2). See Plan 3's
  Limitations section.
- Editing transform-generated blocks as their *rendered* form.
- Cross-file edits (includes).
- Resolving non-`Original` pool ranges in the iframe (would need a round-trip).

## References

- Round-trip core: `claude-notes/plans/2026-06-04-target-incremental-writes.md`
- `apply_node_edit.rs`, `node_lookup.rs`, `writers/incremental.rs`,
  `writers/qmd.rs` (`write_single_block`), `writers/json.rs`
- `quarto-source-map/src/source_info.rs` (`preimage_in`)
- `transforms/sectionize.rs`
- `ts-packages/preview-renderer/src/framework/attribution.tsx`
  (`AttributionWrap`, `useAttributionHover`)
- `q2-preview/{dispatchers,entry}.tsx`, `q2-preview/blocks/*`,
  `q2-preview/PreviewContext.tsx`, `iframe/Q2PreviewIframe.tsx`
- `hub-client/src/components/render/ReactPreview.tsx`,
  `hub-client/src/services/applyNodeEdit*`
- `types/{sourceInfo,diagnostic}.ts`
