# Block editing in q2-preview — design / master spec

**Date:** 2026-06-06 (interaction model revised 2026-06-07; dual-node substrate +
series re-split 2026-06-08)
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Status:** Design approved in brainstorming; revised through pre-implementation
review. Series: **Plan 1, 2a, 2b, 3, 4** under `claude-notes/plans/`.
**Builds on:** `claude-notes/plans/2026-06-04-target-incremental-writes.md`
(the `apply_node_edit` / source-slice round-trip this feature extends).

---

## Overview

Edit a Quarto document's **markdown source, block by block, inside the live
preview**:

1. **Edit the markdown, not the rendered text.** The editor shows the *markdown
   source* of the node, source-sliced from the document `content`.
2. **Every source-backed block is its own edit affordance.** No pencil, no
   buttons: hovering (mouse), pressing (touch), or focusing (keyboard) a block
   **outlines** it; activating turns it into a same-sized markdown editor. Uniform
   across block types and input modalities.
3. **The untransformed AST is a first-class dispatch input.** Every rendered node
   carries both `transformedNode` (display) and `sourceNode` (the untransformed
   counterpart, for editing). Authors and the built-in editor never map between
   trees by hand.

All of it rides the existing write-back core: `parse_qmd_content → splice into the
untransformed AST → compute_reconciliation → incremental_write`. The
reconcile/writer core is unchanged.

## Interaction model (no pencil — revised 2026-06-07)

The earlier draft used a floating pencil overlay; dropped in favor of **the block
being its own affordance**, which removes overlay positioning/scroll-tracking and
is keyboard- and tablet-friendly.

- **Mouse:** hover outlines the deepest editable target; click activates.
- **Touch (Pointer Events):** one progressive press — `pointerdown` outlines
  (reveal); holding past `HOLD_MS` activates; early release / move-beyond-
  threshold cancels. OS gestures suppressed (`touch-action: none`, `contextmenu`
  preventDefault, `-webkit-touch-callout: none`).
- **Keyboard (roving tabindex):** the edit layer is a single Tab stop; arrows move
  the active region in **DOM pre-order** (section → heading → first para → …);
  Enter/Space activates; Esc exits; `:focus-visible` reuses the outline; ARIA
  role + name per region.

**Heading vs. section by geometry (Plan 4).** An `<h2>` is a full-width block box,
so the split uses **glyph-rect hit-testing**: the heading's inline text rectangle
(`Range.getClientRects()`) is the heading target; the section rectangle showing
through elsewhere (notably right of the heading) is the section target. Keyboard
expresses the same split as adjacent pre-order stops. This is the first instance
of a general **"text selects, background activates"** model the architecture is
built to extend.

**Editor sizing (Plan 2b prereq).** P1 no-reflow height match (textarea sized to
the measured box); P2 body-sized monospace (≈0.9× computed body); P3 auto-fit font
*considered, not implemented* (logical-vs-wrapped-line problem).

## Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Source-slice** for the edit buffer; **`sourceNode`** for AST edits. | The textarea shows exact source bytes (no WASM); render components edit the untransformed subtree. |
| D2 | **No request/response.** Ship `content` + `untransformedAst` on `UPDATE_AST`; the iframe slices and resolves `sourceNode` locally. | The iframe already has the pool; it only lacked `content` + the untransformed tree. |
| D3 | **Editable = source-backed** (`sourceNode != null`), active-file. | You edit source, not generated output. |
| D4 | **The block is the affordance — no pencil.** Hover/press/focus outlines; activate to edit. Heading-vs-section by glyph-rect (Plan 4). | Removes overlay positioning entirely; keyboard/touch-friendly; the block has the right geometry. The framework never wraps blocks. |
| D5 | **Editor = in-place `<textarea>`**, monospace, sized to the block (no reflow). | Plain-text markdown source; drops nbsp/`viewKey` hacks. |
| D6 | **Activate per modality** (click / progressive-press / Enter); commit = ⌘/Ctrl+Enter or blur; Esc/empty = cancel. | Uniform across types and inputs. |
| D7 | **Sections: frontend envelope + backend range** (Plan 4). | The section `Div` is `Generated` (not source-backed); a section spans multiple untransformed blocks. |
| D8 | **Edit only the active file** (`file_id == 0`). | `apply_node_edit` hardcodes `FileId(0)`. |
| D9 | **Editability is a structural property of `sourceNode`** (Plan 2a), not a context-propagation gate. | Shipping the untransformed AST lets us read reachability directly; obviates the opt-out `InsideContainerContext` + closed-container audit entirely. |
| D10 | **Empty commit = cancel.** | Avoids accidental destruction without an undo story. |

## Editability gate — the structural (dual-node) model

A node is editable iff the commit path can reach its untransformed counterpart.
Plan 2a ships the untransformed AST to the iframe and builds a SourceInfo-**value**
index in `PreviewContext` (the two trees have separate pools but shared values).
q2-preview's leaf components look up their pool entry in this index. Then:

```
editable(node) = ctx.resolveSource(node)?.reachabilityClass ∈ allowed(plan)
              && editableType(node)   // Plan 2b+
```

- **Present in `sourceIndex`** subsumes the old `t==0` (Original) **and** `d==0`
  (active-file) conjuncts — a Generated/Substring/Concat or included-file node's
  `s` value matches nothing in the active-file untransformed index.
- `reachabilityClass` is assigned at index-build time from the node's untransformed
  ancestry: **`TopLevel`** (top-level block), **`Descendable`** (nested only in
  `Div`(non-section)/`BlockQuote`/lists/`DefinitionList`/`Figure`-body),
  **`Opaque`** (reached by crossing a `Table` cell/caption or `Figure` caption;
  absorbing). A small **structural classifier** computed once, not a per-component
  context push.
- `allowed`: Plan 2a/2b `{TopLevel}`; Plan 3 `{TopLevel, Descendable}`; `Opaque`
  never editable in this series.
- `editableType` (Para/Header/CodeBlock/…) is Plan 2b — not part of Plan 2a's gate.
- Key format: `serializeSourceEntry(sourceEntry)` = `"${t}:${r[0]}-${r[1]}:${d}"` (compact, deterministic).
- **`NodeArgs` (`framework/types.ts`) is unchanged.** The index lives entirely in
  `PreviewContext`; q2-debug and q2-slides are unaffected.

Correspondence is **by value**, so it survives transforms that *move* blocks
(appendix-structure, footnotes) and *insert* structure (sectionize → section
`Div` is Generated → `sourceNode = null`). And **`sourceNode` is always a plain
primitive, never a CustomNode** (see "Plan 5 dissolved").

This **replaces** the earlier opt-out `InsideContainerContext` design: no context,
no default, no closed-container audit, no drift hazard.

## Format routing (by design)

**q2-preview** ships `untransformedAstJson`, builds `PreviewContext.sourceIndex`,
uses the structural gate, and commits **targeted subtrees** (SourceInfo →
`apply_node_edit`) — no copy-on-write bubble (the Rust reconcile rebuilds to root).

**q2-debug / q2-slides** drive the display directly from the AST they receive,
without a pipeline or transforms. They have their own registries and leaf components;
they never receive `untransformedAstJson` and never consult `sourceIndex`. The shared
framework (`Node`, `renderChildren`, `NodeArgs`) is unchanged by Plan 2a.

The two routings differ because only q2-preview has a transform gap between the
displayed AST and the source AST, and needs the targeted-subtree splice.

## Two edit modalities, two channels

- **Built-in editing → text channel.** The textarea shows source markdown (slice
  `content` over `sourceNode`'s range); commit sends `newText`; the parent
  `parse_qmd_content` → `apply_node_edit`. (The iframe can't run the writer, so
  text is the right representation for a textarea.)
- **Render-component editing → subtree channel.** A component calls
  `ctx.commitSubtreeEdit(destinationSourceInfoJson, modifiedBlock)` with a clone
  of `resolved.sourceNode`; `PreviewNodeEditPayload` is a `channel`-discriminated
  union (`'text'` | `'subtree'`); the parent passes the subtree variant straight
  to `apply_node_edit` (no parse step). Editing `sourceNode` (not the transformed
  node) is what keeps shortcodes/refs/includes **inside** the edited region from
  being baked in as their expansions.

## Plan 5 dissolved (CustomNode writing/editability)

We resurrected the old Plan-7e ("the empty `Block::Custom` writer arm deletes a
callout on edit") and then dissolved it. **Reason (verified 2026-06-08):** a
callout is a plain `Div.callout-note` in the *untransformed* AST — the
Callout/Theorem/Proof/FloatRefTarget CustomNodes are all *transform* products and
do not exist pre-pipeline. Since edits operate on `sourceNode` (untransformed),
the writer is **never** asked to serialize a CustomNode on the edit path. So:
- Writer arms (7e): **not needed** for this epic. (Optional hardening side-bead:
  make the empty `Block::Custom` arm *error* rather than emit nothing, as a net
  against a component that wrongly submits a transformed node.)
- Callout/theorem **editability**: subsumed by **Plan 2b** — their `sourceNode` is
  a writer-covered `Div`, editable-as-whole once the `custom/` components
  participate in the affordance. Their **bodies** ride **Plan 3** (nested descent
  into the untransformed `Div`).

## Render-component / built-in boundary (Plan 2b)

1. The framework never wraps a block (D4); an author *may* wrap but owns the
   theme-CSS/hit-test consequences; the affordance keys off `data-block-pool-id`
   on the block's own root (preserved by rendering through the dispatcher).
2. An overridden block that renders the underlying block through the framework
   still gets the built-in affordance (compose: comment *and* edit a paragraph).
3. A component may **opt its subtree out** of the built-in affordance when it
   deliberately hides source structure (e.g. `comment` strips spans for display).

## Key facts established by research (with refs)

- **`AttributionWrap` is a passthrough in preview** (`attribution.tsx:153`); blocks
  are not wrapped — the block's own root carries `data-block-pool-id`.
- **`useAttributionHover` is the affordance precedent** (`attribution.tsx:189`):
  delegated handler + `closest()`. The edit hover reuses that shape (no overlay).
- **Pool entry shape** (`types/sourceInfo.ts:69-77`): `Original {t:0, r:[s,e],
  d:file}`; `r` are UTF-8 byte offsets (slice via `TextEncoder`/`TextDecoder`).
- **Every block carries `s`** uniformly (`writers/json.rs`). Verified: a fresh
  parse of heading→para/list/table/quote/div/rawblock is all `t:0` Original,
  whole-block ranges (a fenced `:::` div is one Original range → editable as a
  whole, which is how you edit its attrs/classes). A callout parses to
  `Div.callout-note` (no `type_name`).
- **`preimage_in`** (`source_info.rs:435-471`): `Some` for Original; `None` for
  sectionize Divs (Generated).
- **The writer supports N→M block replacement** preserving outside bytes
  (`incremental.rs`); it re-serializes block **containers wholesale** (Tier-2).
- **Container shapes** (`block.rs`): `Div/BlockQuote/Figure` hold `Blocks`; lists
  hold `Vec<Blocks>`; `DefinitionList` `Vec<(Inlines, Vec<Blocks>)>` → Plan 3 path.
- **`lookup_block` is top-level only** today (`node_lookup.rs`), Pass-1 exact +
  Pass-2 `preimage_in` covering. Pass-2 is unused on live paths and is **removed**
  in 2b (it was target-incremental-writes' "property #2", retired with it).

## Data flow (edit)

```
render → parent sends UPDATE_AST { astJson, untransformedAstJson, content, … } → iframe
dispatch → each node gets { transformedNode, sourceNode, reachabilityClass }
input  → useBlockEditHover → outline deepest editable block
activate → built-in: textarea(sourceNode range slice)   |  component: edit sourceNode
commit → { channel:'text',    destinationSourceInfoJson, newText }
      OR { channel:'subtree', destinationSourceInfoJson, modifiedSubtreeJson }
parent → channel=text: parse_qmd_content(newText) → apply_node_edit
      OR channel=subtree: apply_node_edit directly
       → onContentRewrite(newQmd) → re-render → fresh UPDATE_AST
```

## Phase breakdown (the series)

- **Plan 1 — Markdown-faithful editing on Para/Header** *(frontend; done)*.
  `content` plumbing; `sliceBytes`; `PreviewContext` `content` + `editTarget`;
  Para/Header textarea showing sliced markdown (click activates — the seed).
- **Plan 2a — SourceInfo-value index + structural gate** *(frontend substrate;
  no Rust changes)*. Ship `untransformedAstJson` through the iframe chain;
  build `sourceIndex` in `PreviewContext` (`useMemo`); refactor Plan 1's
  Para/Header gate onto `sourceIndex` lookup; `NodeArgs` unchanged; q2-debug
  and q2-slides unaffected.
- **Plan 2b — Interaction + editing (built-in + render-component)** *(frontend +
  Rust cleanup)*. Hover/touch/keyboard → outline → activate; generalized textarea
  editor + sizing; affordance on all editable types incl. `custom/` components;
  subtree channel; fix the three render-component demos; the override boundary;
  Pass-1 guard + remove Pass-2.
- **Plan 3 — Nested-block descent** *(Rust + thin frontend)*. Recursive
  `lookup_block` (returns a path) + path splice; the gate admits `Descendable`
  (one predicate change). Enables editing inside fenced divs / list items /
  blockquotes / **callout bodies**. Container reformat is Tier-2/snapshotted.
- **Plan 4 — Section editing + heading/section geometry** *(Rust + frontend)*.
  Source-envelope range payload; `lookup_range` + range splice; glyph-rect
  heading-vs-section hit-test; keyboard section stop; whole-container-as-unit via
  the same geometry.

## Known limitations (v1 — start safe, more affordances later)

- **LineBlock** not editable (`| line` parses as `Para`).
- **Inline-span editing** out of scope (only `Cite`/`Note` carry `s`) → def-list
  terms, short captions, `CaptionBlock` not editable.
- **Table cells/captions, figure captions** not block-editable (`Opaque`).
- **Generated-container regions** (e.g. `appendix-structure`'s synthesized Div)
  gated off (no `sourceNode`) — safe, silently uneditable.
- **Cross-file edits** out of scope.
- **Selection/link vs click-to-edit** accepted for the demo (a future "background
  activates / text selects" model or an edit mode resolves it).

## Risks / watch-items

- **Submit is not a no-op.** Commit re-serializes the edited block; Tier-2
  (containers/tables) reformats. Only **cancel** is a guaranteed no-op; the kept
  guarantee is that blocks **outside** the edit stay byte-verbatim.
- **Value-equality invariant** (Plan 2a): `sourceNode` correspondence relies on a
  transformed Original node's SourceInfo value equaling its untransformed
  counterpart's; a mismatch degrades **safe** (`sourceNode = null` → not editable),
  never a wrong commit. The Pass-1 commit guard is the second line of defense.
- **Attribution + edit-hover coexistence:** when attribution lights up (inert in
  preview today), two pointer-driven hover systems share `#quarto-content`;
  undesigned — human-in-the-loop will observe.
- **Touch long-press** co-opts the OS selection/context-menu gesture.
- **Shared dispatcher / debug-slides degradation** must be preserved + tested.

## References

- Round-trip core: `claude-notes/plans/2026-06-04-target-incremental-writes.md`
- `apply_node_edit.rs`, `node_lookup.rs`, `writers/{incremental,qmd,json}.rs`
- `quarto-source-map/src/source_info.rs` (`preimage_in`)
- `framework/attribution.tsx`; `q2-preview/{dispatchers,entry,PreviewContext,
  PreviewDocument}.tsx`, `q2-preview/blocks/*`, `q2-preview/custom/*`,
  `iframe/Q2PreviewIframe.tsx`
- `hub-client/src/components/render/ReactPreview.tsx`, `…/applyNodeEdit*`
- Render-component demos: `~/docs/demo-playground/gordon/render-components2/`
- Plan 5 origin (dissolved): `2026-05-29-q2-preview-plan-7e-customnode-qmd.md`
  (`git show bf375258^:…`)
