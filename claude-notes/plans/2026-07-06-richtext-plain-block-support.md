# Rich-text editor support for `Plain` blocks (tight list items)

**Strand:** bd-7pxub583 (related to bd-sjb4pzx8 — the tiptap rich-text block editor)
**Status:** IN PROGRESS — user approved 2026-07-06
**Date:** 2026-07-06

## Decisions locked with user (2026-07-06)

- **Scope: BROAD.** Add `'Plain'` to `RICHTEXT_SUPPORTED_TYPES` so the rich-text
  editor activates for *every* reachable `Plain` edit target, not just
  list-item `Plain`s. Rationale (user): "the more settings where the rich text
  editor works well, the better." Risk #2's narrow option is dropped.
- **Characterize tight/loose empirically with the `pampa` binary.** Q2 uses a
  **custom** markdown parser whose tight-vs-loose rules are *not identical to
  Pandoc's*. Do not rely on memory of Pandoc list semantics — generate each
  list variant as qmd and run it through `cargo run -p pampa -- -t json` (or
  `-t native`) to see the actual `Plain`/`Para` shape Q2 produces, and derive
  fixtures from that ground truth.

## Overview

In `format: q2-preview`, the tiptap rich-text block editor (bd-sjb4pzx8)
activates only for a subset of Pandoc block types. That subset omits `Plain`.
Because **tight** bullet/ordered lists store each item's content as a `Plain`
block (loose lists use `Para`), clicking to edit a tight-list item's content
resolves a `sourceNode` of type `Plain`, the availability gate returns `false`,
and the editor falls back to the monospaced textarea instead of the rich-text
surface.

Goal: make the content of tight-list items editable as rich text, matching the
experience already available for paragraphs and headers.

## Root cause (confirmed by reading the code)

The single gate is `RICHTEXT_SUPPORTED_TYPES` in
`ts-packages/preview-renderer/src/q2-preview/richTextSupport.ts:17`:

```ts
export const RICHTEXT_SUPPORTED_TYPES = new Set<string>(['Para', 'Header']);
```

`richTextAvailable(ctx, sourceNodeType)` (same file) is `ctx.richText &&
RICHTEXT_SUPPORTED_TYPES.has(sourceNodeType)`. The dispatcher
(`dispatchers.tsx:517-526`, `renderBlockEditSurface`) uses exactly this
predicate to choose `<RichTextEditor>` vs. the textarea. A tight-list item's
leading block is a `Plain` (see `nestingNav.ts:649-653`, `outerBlocks.ts:333`),
so it never matches.

### Why the rest of the pipeline is (probably) already ready

- **AST → ProseMirror** already maps `Plain` to a paragraph node:
  `astToProseMirror.ts:131-135` handles `case 'Para': case 'Plain':`
  identically. Tightness is inferred separately by `isTight()`
  (`astToProseMirror.ts:122-126`), which checks whether any item block is a
  `Para`. So seeding the rich editor from a `Plain` source node already works.
- **Commit path is shared with the textarea.** Both `<RichTextEditor>` and
  `<EditTextarea>` commit through the identical `commitTextEdit(destination,
  newText)` byte-range-replacement channel (`usePreviewEdit.ts`,
  `PreviewContext.tsx`). The textarea **already** lets a user edit a tight-list
  `Plain` item today (it just isn't rich). So the byte-range resolution and
  the Rust-side re-parse/`apply_node_edit` machinery are already exercised for
  `Plain`. The rich editor changes only *what text* is written into that same
  range, not *how* it is written.

**Net:** the visible bug is a one-line omission, but "add `'Plain'` to the set"
is necessary, not obviously sufficient. The real work is verifying round-trip
fidelity and deciding the scope of "any `Plain`" vs. "only list-item `Plain`".

## Key risks / open questions to resolve before/while implementing

1. **Tight-stays-tight round-trip — largely already solved by the backend.**
   When the rich editor serializes a single-`Plain`-seeded doc back to markdown
   (`docToMarkdown`, `serializer.ts`), the PM node is a `paragraph` —
   indistinguishable from a `Para` seed. The edit target is only the item's
   *inner* byte range (no `- ` marker), and `commitTextEdit` splices new text
   into that range, where it re-parses as a bare `Paragraph`. **But the Rust
   commit path already anticipates exactly this:** `preserve_leaf_variant` in
   `crates/pampa/src/apply_node_edit.rs:253-265` coerces a *single-block*
   `Paragraph` replacement back to `Plain` when the original block was `Plain`,
   precisely so an inline-text edit of a tight list item does not loosen the
   list. The rich editor commits through the **same text channel** the textarea
   uses, so it inherits this guard for free. The `len() == 1` condition is
   load-bearing: a legitimately multi-block edit still loosens. Since a single
   tight-item content edit stays one paragraph (the serializer's `_`-italic and
   mark transforms don't add blocks), the guard should fire. `nestingNav.ts:184`
   flags the same hazard for divs, and the textarea path already relies on this
   guard today. → **Test:** interactive edit of a tight-list item, assert the
   on-disk qmd stays tight and the AST item block stays `Plain` — this is a
   *regression guard* on `preserve_leaf_variant` being exercised by the rich
   path, not new machinery we must build.

2. **Scope of "`Plain`".** `Plain` appears in more than tight list items:
   table cells, definition-list terms/definitions, and (historically) some
   figure/caption contexts. Adding `'Plain'` to `RICHTEXT_SUPPORTED_TYPES`
   makes *every* reachable `Plain` edit target rich. We must decide:
   - **(a) Broad:** allow rich text for any `Plain`. Simplest; but only correct
     if every `Plain` edit target round-trips safely and the measured-edit box
     renders acceptably in those contexts (table cells especially).
   - **(b) Narrow:** allow rich text for `Plain` only when the resolved edit
     target sits inside a list item. Requires threading list-context into the
     availability predicate (the resolver / breadcrumb already knows the
     ancestor chain — see `nestingNav.ts`, `outerBlocks.ts`).
   → Determine which `Plain` contexts are even *reachable* as edit targets in
   q2-preview today. If only list items are reachable, (a) and (b) converge and
   we take (a).

3. **Multi-block tight items.** A tight list item can be `Plain` + nested
   `BulletList` (a tight item with a sublist). The edit target for the leading
   `Plain` should cover only that `Plain`'s range, leaving the sublist intact.
   Confirm the leading-`Plain` edit target does not swallow the sublist on
   commit. → **Test:** nested tight list, edit the parent item text, assert the
   sublist survives unchanged.

4. **Breadcrumb / measured-edit parity.** The breadcrumb and the
   measure-and-set edit box already special-case `Plain` (`outerBlocks.ts:333`,
   `403`, the `Pl` glyph in `nestingNav.ts`). Adding rich-text for `Plain`
   should reuse the existing `leaf-text` category (`categoryForSourceNode`
   already groups `Plain` with `Para`/`Header`), so the breadcrumb should need
   no change — verify, don't assume.

## Plan (TDD — tests first, per project policy)

### Phase 0 — Reproduce & characterize (no code change yet)
- [x] **Characterize Q2's tight/loose list AST with `pampa`.** Ran qmd variants
      through `cargo run -p pampa --bin pampa -- -t json`. Findings:
      - Tight bullet/ordered → items are `Plain`; loose (blank line between
        items) → items are `Para`. Matches the standard model.
      - Nested tight: first item = `[Plain, BulletList]`; the leading `Plain`
        has its own source range distinct from the sublist, so an edit target
        scopes to just that `Plain` (won't swallow the sublist).
      - **Table cells → `Plain`.** Under broad scope, table-cell content becomes
        rich-editable too — include in corpus + verify.
      - Def lists: `Term\n:   def` did **not** parse as a `DefinitionList` in Q2
        (came out as two `Para`s), so def-list `Plain` is not a live edit-target
        context here. Not a concern.
- [x] Reachable `Plain` contexts: **tight list items (bullet/ordered, nested)**
      and **table cells**. Broad scope covers both.
- [x] **Backend tightness guarantee already exists AND is tested.**
      `preserve_leaf_variant` (`apply_node_edit.rs:253`) coerces a single
      re-parsed `Paragraph` back to `Plain` on the text channel. Covered by
      `text_edit_preserves_bullet_list_tightness` (node_edit_tests.rs:1308) and
      `text_edit_preserves_ordered_list_tightness` (1347), which drive
      `edit_nested_block` — the *exact* text-channel path `commitTextEdit` uses.
      **Conclusion:** no Rust changes needed; risk #1 is already closed. The
      fix is TS-gate-only.
- [ ] Live browser "before" is superseded by the failing gate test (Phase 1) as
      the bug evidence; live confirmation folded into Phase 3 end-to-end.

### Phase 1 — Tests first (TDD red → green)
- [x] **Gate unit test** — `richTextSupport.test.ts` (new; the module had no
      direct test). Locks the membership set + flag/mode interactions. The
      `Plain` assertions **failed before the fix** (3 red) and pass after. This
      is the direct encoding of the bug fix.
- [x] **Dispatcher integration test** — `plain-list-item-richtext.integration.test.tsx`
      (new). Drives the real `PreviewRoot`; clicking a tight bullet-list item
      opens the **rich** editor (`.q2-rt-toolbar` present, no textarea) with the
      flag on, and the **textarea** with the flag off. **Confirmed it fails with
      the fix reverted** (toolbar null) → true regression guard.
- [x] **Converter seed test** — `richtext/plainSeed.test.ts` (new, pure/no
      pampa). Guards the exact `astToDoc([Plain], …)` seed the RichTextEditor
      uses: a lone `Plain` → one paragraph → serializes to the bare inline text
      (marks incl. `**bold**`/`_italic_`), no dropped nodes, no list marker.
- [~] Tightness-on-commit is **already** covered by the Rust suite
      (`text_edit_preserves_{bullet,ordered}_list_tightness`, Phase 0). The rich
      editor commits through that same text channel, so no new Rust test is
      needed; Phase 3 confirms it live.

### Phase 2 — Implement the gate change (BROAD)
- [x] Added `'Plain'` to `RICHTEXT_SUPPORTED_TYPES` in `richTextSupport.ts` and
      documented why (tight-list items + table cells; backend preserves
      tightness). No dispatcher change needed — the existing predicate + the
      already-Plain-aware `astToProseMirror` do the rest.
- [x] Phase 1 tests green with the fix in place.

### Phase 3 — End-to-end verification (mandatory, per CLAUDE.md)
Built the real chain (`cargo xtask build-q2-preview-spa` + `cargo build --bin q2`;
no WASM/Rust change so no WASM rebuild needed) and drove `q2 preview --allow-edit`
in a browser on a tight-list fixture.

- [x] **Rich editor opens for a tight-list `Plain` item.** Clicking "banana"
      opened the tiptap editor — ProseMirror `<p>banana</p>`, the formatting
      toolbar (B/I/S/sub/sup/link), the "Editing… rich text / plain text"
      toggle, and the `Pl` breadcrumb glyph. Before the fix this opened the
      monospaced textarea. **This is the core deliverable and it works.**
- [x] **Simple text edit round-trips and preserves tightness.** Edited
      "banana" → "banana XX" and committed; on-disk file kept a **tight** list
      (`pampa -t json` → all three items `Plain`). Instrumented commit confirmed
      `sourceNodeType: "Plain"`, committed markdown `"banana XX"`.
- [x] Suites green after the change (unit 484, integration 517, tsc clean; the
      pampa-oracle round-trip 15/15). No regressions in the existing Para/Header
      rich-text tests.
- [x] **`cargo xtask verify` (Rust+hub) still owed** before push (TS-only change,
      but run it per policy).

#### ⚠️ Bug found during E2E — filed as **bd-hafs0qho** (not caused by this change)
Committing a **bold** edit (select-all + bold + commit) could **drop the item's
content** — e.g. `- banana` → `-` (empty). Instrumented capture showed the
committed ProseMirror doc was `paragraph[hardBreak]` (text gone), so the
serializer correctly produced an empty string.

Root cause is **upstream** of the serializer and the Rust commit path, both of
which I verified correct:
- serializer: `paragraph[strong "x"]` → `**x**`, `paragraph[strong "x", hardBreak]`
  → `**x**` (deterministic tests);
- pampa `apply_node_edit`: replacing a tight-list `Plain` with `**BOLD**` →
  `* **BOLD**`, tight preserved (node_edit_tests.rs repro).

It is **not `Plain`-specific**: the RichTextEditor, `astToProseMirror`, and the
serializer are type-agnostic (Para and Plain both map to a `paragraph` node and
share seed/edit/serialize/commit). A synthetic repro on a **Para** also injected
a spurious `hardBreak`, so the behavior is shared with the pre-existing Para path
(bd-sjb4pzx8) and predates this change. My browser repros were partly confounded
(couldn't drive tiptap's internal selection from outside), so bd-hafs0qho needs a
clean real-user reproduction + root-cause of the spurious hardBreak/text-drop.

**Decision point for the user:** the Plain-gate change is safe and delivers the
feature for text editing, and the bold anomaly is pre-existing (affects Para
equally). Options: (a) merge this change now and fix bd-hafs0qho separately, or
(b) hold this change until bd-hafs0qho is root-caused. Awaiting direction.

### Phase 4 — Wrap up
- [ ] Record the exact invocation + inspected output in this plan and the
      strand, per the end-to-end-verification policy.
- [ ] Update `hub-client/changelog.md` if any `hub-client/` file changed
      (the two-commit workflow).
- [ ] Close bd-7pxub583 with a reason once all tests pass and the feature is
      verified end-to-end.

## Files in scope

- `ts-packages/preview-renderer/src/q2-preview/richTextSupport.ts` — the gate
  (primary change).
- `ts-packages/preview-renderer/src/q2-preview/richtext/roundtrip.test.ts` —
  new fixtures/assertions.
- `ts-packages/preview-renderer/src/q2-preview/richtext/astToProseMirror.ts` /
  `serializer.ts` — likely **no change** (already handle `Plain`), but the
  serializer is where a tightness-preservation fix would land if risk #1 bites.
- Possibly `dispatchers.tsx` — only if the narrow (context-aware) option is
  chosen and the predicate needs more than the node type.

## Non-goals

- Structural list editing (Enter to split items, Tab to nest) — that is
  bd-sjb4pzx8's Phase 1c and is out of scope here. This strand is specifically
  about making the *content* of an existing tight-list item rich-text editable.
- Table-cell / definition-list rich editing beyond whatever falls out of the
  broad-vs-narrow decision.
