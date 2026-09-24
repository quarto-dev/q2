# Block-level attributes on list items (`<li class>`)

**Strand:** bd-aeyss6p5 (discovered-from bd-itqcfxc3; related bd-38ioql41)
**Date:** 2026-06-18
**Status:** planning
**Parent plan:** [`2026-06-17-block-level-attrs-inline-attr.md`](./2026-06-17-block-level-attrs-inline-attr.md)
(the `Para` capability, merged in PR #310; see its "List-item JSON
representation — study" section, which this plan implements)

## Goal

Let a block attribute land on a list item's `<li>` — e.g. `<li class="…">`
for per-item styling. `- item {.foo}` should render `<li class="foo">item</li>`
in **all three renderers**: `q2 render` (html.rs), `q2 preview` (json.rs →
React, including the `q2-slides`/revealjs incremental path), and AST→JSON→AST
round-trips.

## Confirmed facts (verified 2026-06-18)

- **The parser already emits the carrier.** `- item one {.foo}` parses to a
  list item whose **last block** ends with a trailing `Inline::Attr({.foo})`
  (a `Plain` for tight items, a `Para` for loose items). No filter/transform is
  needed — the capability is reachable by plain authoring, just like `Para`.
- **Current behavior is broken/dropping:**
  - `-t native` and `-t json` both **error `Q-3-32`** (standalone attr) on the
    item's trailing `Inline::Attr`.
  - `-t html` **drops** the attr (the inline writer skips `Inline::Attr`), and
    leaves a stray trailing space: `<li>item one </li>`.
- **Why list items differ from `Para`** (recap of the study section): a Pandoc
  list item is `Vec<Block>` — a bare `[Block,…]` **array** in JSON with no
  object to hang an extra `attr` key on. So the attr must be **hoisted** out of
  the item's last block up to the `<li>`, and carried in JSON via a **parallel
  sibling key on the enclosing list node** (the `rowsS` table precedent), not an
  object key on the item.

## JSON wire form (resolved)

Mirror the table `rowsS` precedent: a sibling key on the list node, an array
**parallel-indexed** to the items, each entry an `Attr` triple or `null`.

- **Key name:** `itemAttr` (decided; `rowsS`-style suffix doesn't fit since this
  is an attr, not source info).
- **Emitted only when at least one item carries a non-empty attr** — so ordinary
  lists are byte-for-byte unchanged (keeps the \~3900 existing tests inert).
- **Key order:** alphabetical, matching the house convention
  (`stream_write_simple_node` emits `c`, then `l?`, `s`, `t`). `itemAttr` sorts
  between `c` and `l`.

BulletList:
```json
{"t":"BulletList","c":[[…item0…],[…item1…]],"itemAttr":[["",["foo"],[]],null],"s":…}
```
OrderedList (items live in `c[1]`; `itemAttr` is parallel to `c[1]`):
```json
{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[…i0…],[…i1…]]],"itemAttr":[null,["",["foo"],[]]],"s":…}
```

Pandoc ignores the unknown `itemAttr` key, so `-t json | pandoc -f json` still
degrades to a plain list (verified-by-analogy with `Para.attr`; will re-verify
empirically in the end-to-end step).

## AST representation (no type change)

The item attr **stays** a trailing `Inline::Attr` in the item's last block —
the canonical shape the parser already produces and `qmd`/`incremental` already
round-trip. No new field on `BulletList`/`OrderedList`. The hoist happens only
at **write** time (and the inverse fold at **read** time).

## Shared helper (the keystone)

Add to `crates/pampa/src/writers/block_attr.rs`, reusing
`split_trailing_block_attr` for the merge semantics:

```rust
/// For a list item (`&[Block]`), collect the trailing block attr from its LAST
/// block — but only when that block is a `Paragraph`/`Plain` (the blocks that
/// carry inline content). Returns the merged `Attr` and, when the attr is
/// non-empty, a *stripped clone* of the last block (trailing attr run removed)
/// for the caller to render in place of the original. Returns `(empty_attr,
/// None)` when there is no hoistable attr, so the caller renders the item as-is.
pub(crate) fn split_list_item_attr(item: &[Block]) -> (Attr, Option<Block>);
```

- Clones **only** the last block, and **only** when an attr is present (rare).
  This is what kills the **precedence trap**: the stripped clone means the inner
  `Para`/`Plain` writer never sees the trailing attr, so the class lands on
  `<li>`, never on an inner `<p>`.
- **Limitation (decided 2026-06-18: acceptable for v1):** if the item's last
  block is not a `Para`/`Plain` (e.g. it ends with a nested list or code block),
  no item attr is hoisted — matches where the parser puts authored attrs.
  Revisit only if a consumer needs broader scanning.

All three writers and the reader go through this one helper, so the merge rule
stays single-sourced (no Rust/TS duplication — the cross-language divergence the
project fears).

## Implementation plan (TDD — failing tests first)

### Phase 0 — helper + unit tests ✅
- [x] `split_list_item_attr` in `block_attr.rs` with unit tests: tight item
      (last block `Plain`) hoists; loose item (last block `Para`) hoists;
      multi-block item hoists from the **last** block; item with no trailing
      attr → `(empty, None)`; last block not `Para`/`Plain` → `(empty, None)`;
      empty trailing attr → `(empty, None)`; empty item → `(empty, None)`;
      stripped clone has the trailing attr-run removed. **7 new tests, all green.**

### Phase 1 — html.rs (`q2 render`) ✅
- [x] **Failing tests** (mirror `mod tests`): `- item {.foo}` → `<li class="foo">`;
      inner `<p>` does **not** also get the class (loose item); composes with the
      incremental `fragment` class (`<li class="fragment foo">`); id + kvs
      applied; no stray trailing space; ordinary list unchanged. **6 tests, were
      red, now green.**
- [x] Replaced `list_item_open` with `composed_list_item_attr` + `write_list_item`
      (per-item open merging `classes = ["fragment"?] ++ item_classes` plus id/kvs
      via `write_attr`). Bullet + Ordered loops both call `write_list_item`.
- [x] Renders the item via the helper's stripped clone (`item[..last] + stripped`)
      so the trailing attr is gone before `write_blocks_inline` runs.
- [x] **End-to-end:** `pampa -i list.qmd -t html` → `<li class="foo">item one</li>`.

### Phase 2 — json.rs writer (`q2 preview` transport) ✅
- [x] **Failing tests** (3): `BulletList`/`OrderedList` wire form carries
      `itemAttr` parallel to items, `null` for plain items; inner block emitted
      **without** the trailing attr; ordinary list emits **no** `itemAttr` key.
- [x] Custom inlined emitter for Bullet/Ordered (fast path = byte-identical
      `stream_write_simple_node` when no item attr): helpers `list_item_attrs`,
      `stream_write_list_items` (emits stripped items), `stream_write_item_attr_key`
      (alphabetical position c→itemAttr→l/s/t).
- [x] **End-to-end:** `pampa -t json` emits `itemAttr`; `pandoc -f json` ignores
      it and degrades to a plain `<li>` (no error).

### Phase 3 — readers/json.rs (round-trip) ✅
- [x] **Failing test**: AST→JSON→AST round-trip preserves the item attr (item
      with attr regains trailing `Inline::Attr`; item without one stays clean).
- [x] `fold_item_attrs` helper threaded into both `BulletList`/`OrderedList`
      arms (mirrors the `Para` `attr` fold-back + table `rowsS`).
- [x] **End-to-end:** `pampa -t json | pampa -f json -t html` →
      `<li class="foo">item one</li>`.

### Phase 4 — React preview-renderer (both code paths) ✅
- [x] **Failing vitest** (4): attributed item → `<li class="foo">` in registry
      path + deck path; incremental composes `class="fragment foo"`; id/`data-*`
      applied; null item stays plain. Were red, now green.
- [x] `framework/types.ts`: added optional `itemAttr?: (Attr | null)[]` to
      `BulletListBlock`/`OrderedListBlock`.
- [x] Shared helper `framework/listItemAttr.ts` `liItemAttrProps(attr, fragment)`
      (single-sources the compose rule, mirrors Rust `composed_list_item_attr`),
      exported from `framework/index.ts`.
- [x] Incremental path — `blocks/BulletList.tsx` & `blocks/OrderedList.tsx` use
      `liItemAttrProps(node.itemAttr?.[i], incremental)`.
- [x] Non-incremental path — `framework/dispatch.tsx`
      `renderChildrenRegistry.BulletList`/`OrderedList` use
      `liItemAttrProps(node.itemAttr?.[i], false)` (edit-drop documented inline).
- [x] preview-renderer suites green (219 unit + 233 integration); `tsc` clean.
  - **Edit edge case (decided 2026-06-18: accept the drop for v1).** These
    registry renderers\' `setLocalAst` rebuilds `{t:'BulletList', c:newItems}`
    **without** `itemAttr`, so an in-preview text edit of an attributed item
    drops its class. We apply `itemAttr` on render but do **not** thread it
    through edits in v1 (matches how other sidecar data behaves on edit).
    Document the limitation; file a follow-up strand if preservation is wanted.

### Phase 5 — end-to-end + full verify
- [x] `pampa -t html` on real fixtures: bullet `<li class="foo">`, ordered
      `<li id="one" class="alpha" data-key="val">` — inspected.
- [x] `-t json | pandoc -f json` degrades gracefully (plain `<li>`, no error).
- [x] `-t json | pampa -f json -t html` roundtrips to `<li class="foo">`.
- [x] `-t json | pampa -f json -t qmd` roundtrips to `{#one .alpha key="val"}` /
      `{.beta}` on the items (reader fold-back + qmd writer).
- [x] React render path covered by vitest **integration** tests driving the real
      `Ast`/registry component tree, both the registry (non-incremental) and the
      `mountInDeck` incremental paths.
- [\~] **Live browser** `q2 preview` (WASM-served) revealjs session: NOT yet run.
      The JSON contract (Rust tests) + React reads (vitest) cover the chain
      piecewise; a live check is the final seal — see status note.
- [x] Full `cargo xtask verify` — **all 14 steps passed (exit 0):** lints+clippy
      clean, fmt clean, workspace build (`-D warnings`) clean, Rust tests, WASM
      rebuilt via hub-build, hub-client tests (600+72+111), preview-renderer
      (219+233+74), q2-preview-spa built. Step 14 Playwright E2E skipped (no
      `--e2e`).

## Out of scope / deferred

- **`native.rs`** keeps erroring `Q-3-32` on the item's trailing attr (consistent
  with the `Para` decision; native's faithfulness contract is a separate call).
- **Strict-pandoc-JSON vs Quarto-2-JSON split.** The current "JSON/native must be
  Pandoc-compatible, else `Q-3-32`" decision makes AST inspection during this kind
  of work awkward (you can't dump an AST that contains a standalone attr without
  first teaching a writer to encode it). Not needed for this feature, but a good
  candidate **interstitial strand** if AST inspectability keeps biting. Flagged
  per the user's 2026-06-18 note. → file as discovered-from bd-aeyss6p5 if we want it.
- `BlockQuote`/other block-attr carriers (parent plan, separate increments).
- Wiring the bd-38ioql41 reveal-figure-caption consumer (parent plan item).

## Bookkeeping

- `braid update bd-aeyss6p5 --status in_progress` when implementation starts.
- Leave a trail with `braid comment` per phase.
- Snapshot changes (if any list `.snap` files move) get reported per CLAUDE.md.
