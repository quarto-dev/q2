# Block-level attributes via collected trailing `Inline::Attr`

**Strand:** bd-itqcfxc3 (discovered-from bd-38ioql41)
**Date:** 2026-06-17
**Status:** in progress — JSON-writer keystone landed (see Progress below)

## Progress

- **2026-06-17 — JSON transport keystone (done).** The conceptual blocker was
  that the `q2 preview` path is `json.rs` → React, and `json.rs` *errored*
  (`Q-3-32`) on a stray `Inline::Attr`, so a block attr could never reach the
  preview. **Resolved via the "safe extra key" channel** (the same trick the
  `s`/`l` source-info keys use): a `Para` with a trailing `Inline::Attr` now
  serializes to a Pandoc-valid `Para` node carrying an extra `attr` object key
  (`{"attr":[id,classes,kvs], "c":[…stripped inlines…], …}`). **Verified
  empirically** that Pandoc 3.9 ignores the extra key (`pandoc -f json` on the
  writer's literal bytes yields `Para [Str …]` → `<p>…</p>`) — so the wire form
  degrades gracefully for external Pandoc while our React renderer can read it.
  - New shared helper `crates/pampa/src/writers/block_attr.rs`
    `split_trailing_block_attr(&[Inline]) -> (&[Inline], Attr)` — collects the
    trailing `Inline::Attr` run (+ interleaved/preceding whitespace), merges
    (id last-wins, classes accumulate+dedup, kv last-wins), returns the retained
    prefix. Block-agnostic; html.rs and the list-item case will reuse it.
  - Wired into the **streaming** `Para` writer (`json.rs` `stream_write_block`;
    note `write_with_config` uses the streaming path, not the tree `write_block`).
  - Tests: `block_attr` unit tests (semantics) + `test_paragraph_trailing_attr_emits_attr_key`
    (wire format). Full `pampa` suite green (3965), inert for existing docs (no
    paragraph carries a trailing `Inline::Attr` today).
  - **Merge semantics chosen:** trailing-run-only, id last-wins, classes
    accumulate (dedup), kv last-wins. Easy to revisit — it's localized to the
    helper.

- **2026-06-17 — `Paragraph` across all three renderers (done).**
  - **html.rs** `Paragraph` writer collects via `split_trailing_block_attr`,
    emits `write_attr` on the `<p>`, renders the retained content. The inline
    writers (`write_inlines` / `find_attribution_run_end` / `write_inlines_as_text`)
    were widened from `&Inlines` to `&[Inline]` so a content *prefix* can be
    rendered. Test `paragraph_trailing_attr_becomes_p_class`.
  - **React** `Para.tsx` reads the optional `attr` key (added to `ParaBlock` in
    `framework/types.ts`) and applies id/classes/`data-*`/`role` to the `<p>`,
    mirroring `Header.tsx`. Test in `q2-preview.integration.test.tsx`. Full
    preview-renderer suite green (219 unit + 229 integration); `tsc` build clean.
  - **End-to-end through the binary** (✅ inspected). The parser *already* emits a
    trailing `Inline::Attr` for plain qmd `paragraph text {.caption}`, so the
    capability is reachable by plain authoring — no consumer filter needed:
    - `pampa -t html -i cap.qmd` → `<p class="caption">This is a caption.</p>`
    - `pampa -t json -i cap.qmd | pandoc -f json -t html` → `<p>This is a caption.</p>`
      (Pandoc ignores the `attr` key — graceful, no error)
    - This was previously a `Q-3-32` *error* in JSON; it now works.
  - **Behavior change worth noting:** plain qmd `paragraph {.class}` now yields
    `<p class="class">` in html/json/preview (it errored/was dropped before).
    Consistent with `## Heading {.class}`; arguably the intended qmd semantics.
    Full `pampa` suite stayed green (3967), so no existing test relied on the old
    drop/error.

### `Plain` is out of scope (decided)

A `Plain` emits **no wrapping element** — bare inlines in HTML, a bare fragment
in React — so a block attr on a `Plain` has nowhere to land. The figure-caption
that wants `<p class="caption">` must be a **`Paragraph`**. `Plain` is therefore
intentionally *not* a block-attr carrier; the `json.rs`/`native.rs` `Plain`
arms are left as-is.

### Remaining (next increments)

- [ ] **`native.rs`** still errors `Q-3-32` on a `Para` trailing attr (pre-existing;
      now reachable via plain qmd `paragraph {.attr}`). Decide its contract:
      faithfully print the resolved attr, drop silently, or keep erroring as a
      debug-only format. Deferred — needs a call on native's faithfulness intent.
- **2026-06-17 — JSON reader fold-back (done).** `readers/json.rs` `Para` arm
      now restores a trailing `Inline::Attr` from the `attr` key, so AST→JSON→AST
      round-trips preserve the block attr. Verified end-to-end through the binary:
      `pampa -t json cap.qmd | pampa -f json -t html` → `<p class="caption">…</p>`.
      Test `test_paragraph_trailing_attr_roundtrips`.
  - **Q-3-32 still live (no docs change).** The Para writer no longer errors, but
      `native.rs` and `json.rs`\' non-Para `Inline::Attr` paths (stray attr in a
      `Plain`, mid-content, etc.) still emit `Q-3-32`, so the error code stays
      emitted — the docs/ page needs no "no longer emitted" note.
- [ ] List items (`<li class>`) — distinct hoist mechanism (attr rides on the
      item's child block; the `<li>` writer must hoist it, composing with the
      incremental `fragment` class). Later sub-task (**bd-aeyss6p5**). JSON
      representation resolved below.

## List-item JSON representation — study (for bd-aeyss6p5)

The `Para` `attr`-key trick relies on a `Para` being a JSON **object**
(`{t,c,…}`) onto which an extra key can hang. A list item is **not** an object:
`BulletList`/`OrderedList` are `content: Vec<Blocks>` (`crates/quarto-pandoc-types/src/block.rs:138-148`),
and in JSON each item is a bare `[Block,…]` **array** inside the list node's `c`.
There is no per-item object to carry an `attr` key.

**This exact problem is already solved in the source-location / attr-source
work**, via a single reusable pattern:

> When a Pandoc sub-structure is array-shaped (no object to hold an extra key),
> carry the Quarto-only data in a **parallel sibling key on the nearest
> enclosing object node**, shaped to mirror the array (parallel-indexed), and
> read it back from that key. Pandoc ignores unknown object keys (proven for
> `Para.attr`; `a`/`rowsS`/`captionS` already ship and round-trip).

Concrete precedents (all in `crates/pampa/src/writers/json.rs` + reader):
- **Attr source** — the attr triple `[id,[class],[[k,v]]]` is pure arrays/strings,
  so its source rides in a sibling `a` key as `AttrSourceJson { classes:[…
  parallel to classes…], id, kvs:[… parallel to kvs…] }` (`json.rs:137,676`,
  stream `:2020`; reader `read_attr_source` at `readers/json.rs:635`).
- **Tables** — rows/cells/heads/bodies/feet are all array-shaped; their
  out-of-band data rides in sibling keys `a`, `rowsS`, `cellsS`, `headS`,
  `bodyS`, `bodiesS`, `captionS`, each an array **parallel-indexed** to its data
  array (writer `stream_write_table_head_source` `:2435`, `stream_write_row_source`
  `:2399`; reader threads `obj.get("rowsS")` / `get("bodyS")` / `get("captionS")`
  at `readers/json.rs:1668,1726,1787,2037`). This is the closest analog to a
  list: a `TableHead` is `[attr,[rows]]` and per-row data lives in `rowsS`
  parallel to the rows — *exactly* the "per-item data parallel to an item array"
  shape we need.

**Resulting design for list items (recommended):**
- **AST (no type change):** the item attr stays a **trailing `Inline::Attr` in
  the item's last block** — same canonical shape as `Para`, and what `- item
  {.foo}` would naturally parse to.
- **JSON writer:** per item, run `split_trailing_block_attr` on the item's last
  block; hoist the merged `Attr` into a **parallel sibling key** on the list node
  (mirroring `rowsS`; name e.g. `itemAttr`/`cAttr` — TBD), an array
  parallel-indexed to `c`, each entry an `Attr` triple or `null`; emit the item's
  blocks **without** the trailing attr. Stripping here also fixes the
  precedence trap — the inner `Para`/`Plain` writer never sees the attr, so the
  class lands on `<li>`, not an inner `<p>`.
  ```json
  {"t":"BulletList","c":[[…item0…],[…item1…]],"itemAttr":[["",["foo"],[]],null],"s":…}
  ```
- **JSON reader:** read the parallel key; fold each non-null entry back into a
  trailing `Inline::Attr` on the matching item's last block — exactly like the
  `Para` `attr` fold-back (`readers/json.rs` Para arm) and the table `rowsS`
  threading.
- **html.rs / React:** hoist to `<li class>`, composing with the incremental
  `fragment` class (`list_item_open` at `html.rs:1272`; React BulletList/
  OrderedList both paths).

Net: item arrays stay Pandoc-valid (Pandoc ignores `itemAttr`), the preview
reads the sibling key, and it round-trips — all consistent with the established
table/attr-source machinery rather than a new ad-hoc channel.
- [ ] Wire the actual consumer: bd-38ioql41 reveal figure caption emits a
      `Paragraph[..caption.., Attr{.caption}]`; add the q2-slides preview parity
      check there.



## Motivation

Pandoc's AST attaches `Attr` (id, classes, key-values) to some blocks
(`Header`, `CodeBlock`, `Div`, `Table`, `Figure`) but **not** to others
(`Paragraph`, `Plain`, list items). This blocks several things Quarto wants:

- `<p class="caption">` for a hoisted figure caption (the
  reveal auto-stretch figure work, bd-38ioql41 Case 1).
- `<li class="…">` / per-list-item styling (came up independently elsewhere).
- Block-level attributes generally, produced by pure-AST filters.

We already have the building block: `Inline::Attr(InlineAttr)` — a **standalone
attribute node** carrying a full `Attr` + source info. It was introduced "to
represent commonmark attributes … in places where they are not directly attached
to a block, like in headings and tables" (`inline.rs:35-41`). Today:

- The **parser** emits a trailing `Inline::Attr` for `## Heading {#id .c}` etc.
- **Postprocess folds** it into the block when the block *has* an `Attr` field —
  e.g. `Header` at `postprocess.rs:924-969` pops the trailing `Inline::Attr`
  into `header.attr`.
- For blocks **without** an `Attr` field there is nowhere to fold, and the
  **HTML inline writer drops** `Inline::Attr` (`html.rs:1058`: "These should not
  appear in final output").

## Proposal

Let a filter/transform inject a standalone `Inline::Attr` (typically at the end
of a block's inline content), and teach the **writers** to *collect* those
nodes, strip them from the rendered inline content, and apply the merged `Attr`
to the emitted block element. This gives Attr-less Pandoc blocks the attributes
the AST can't natively carry, without changing the Pandoc block types.

Example AST → HTML:

```text
Paragraph[ Str("This is a caption."), Attr({"", ["caption"], {}}) ]
  ⇒  <p class="caption">This is a caption.</p>
```

## Architecture (confirmed 2026-06-17 — this reframes the risk)

The render and preview paths are **two different renderers**, and the primary
consumer (reveal figure caption) flows through the *non-HTML* one:

- **`q2 render` (to disk)** → `crates/pampa/src/writers/html.rs`. Today it
  **drops** a stray `Inline::Attr` (`html.rs:1058`). It consumes the in-memory
  AST directly and can emit raw `<p class="…">` trivially.
- **`q2 preview` (live iframe)** → `crates/pampa/src/writers/json.rs` produces a
  **Pandoc-shaped JSON tree + an `astContext` sidecar**, which is rendered by a
  **separate React renderer**, `ts-packages/preview-renderer/` — *not* by
  `html.rs`. For `format: revealjs` the preview pseudo-format is `q2-slides`
  (`wasm-quarto-hub-client/src/lib.rs` `map_format_for_preview`): same JSON
  pipeline, React builds the reveal shell. **The figure-caption consumer
  (bd-38ioql41) is a reveal feature, so its preview is rendered by React, not
  by `html.rs`.**
  - Today `json.rs` **errors** (`Q-3-32`) on a stray `Inline::Attr` and emits an
    empty `Str` placeholder, so the node never reaches the JSON.
  - The React side has **no `Attr` inline** at all — an unregistered inline
    renders a "(not yet implemented)" placeholder
    (`ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx`).
  - React block leaves read the Pandoc attr triple directly: `Header.tsx` reads
    `node.c[1] = [id, classes, kvs]`; `Div.tsx` reads `node.c[0]`. `Para.tsx`
    emits `<p>{children}</p>` with **no** attr; `Plain.tsx` a bare fragment;
    `BulletList.tsx`/`OrderedList.tsx` wrap each item's blocks in a bare `<li>`.

**Q2's JSON extension model:** node shapes stay Pandoc-valid; Quarto-only data
is either desugared to a valid Pandoc node (e.g. `Custom`→marker `Span`,
`Shortcode`→`Span`) **or** carried out-of-band in the `astContext` sidecar
(source info, attribution are already threaded this way and read by the React
renderer via `resolveSource`). A block-level attr on a paragraph is **not
expressible as a standard Pandoc node** (`Para` is `["Para",[inlines]]`), so
*any* approach that makes the preview render `<p class>` must use one of these
two extension channels and the React renderer must gain explicit handling.

## Design questions to resolve (the session's real work)

### 1. Where does collection happen? (the decisive decision)

Plan option (A) below ("writer-side scan") was the original lean, but the
two-renderer finding above reframes its cost: a writer-side scan means the
**merge rule is implemented twice — once in Rust (`html.rs`) and once in
TypeScript (React leaves)** — with `json.rs` emitting the raw node. That cross-
language duplication *is* the preview-parity divergence the project fears.

- **(A) Per-renderer scan.** `html.rs` scans its inlines, merges, applies,
  skips them; `json.rs` emits the `Inline::Attr` faithfully (desugared to a
  marker `Span` to stay Pandoc-valid); React `Para`/`Plain`/`li` scan children,
  merge, and apply. AST node types unchanged. **Merge logic duplicated
  Rust+TS.** Lowest type churn, highest divergence risk.
- **(B) Resolve once in Rust (recommended).** A shared late AST transform folds
  the trailing `Inline::Attr` run into a single resolved `Attr` and stores it on
  the block, so **both** writers read a resolved attr and neither re-merges.
  Merge semantics live in exactly one place (Rust). Two carrier sub-options:
  - **(B1) `astContext` sidecar keyed by the block's pool-id** (mirrors how
    attribution/source-info already reach React). Node tree stays valid Pandoc;
    `html.rs` reads the resolved attr; React looks up pool-id → applies, no TS
    merge logic. Most consistent with the existing extension model; most
    plumbing.
  - **(B2) Add an `attr: Attr` field to `Paragraph`/`Plain`** (our types — we
    already extend Pandoc heavily). Cleanest semantics: one transform fills it;
    `html.rs` and React read it like `Header`/`Div`. Cost: touches every
    `Paragraph`/`Plain` construction site (mechanical, via `Default`), and the
    `Para` *JSON node shape* would diverge from Pandoc unless we still sidecar
    it for `-t json` — i.e. a strict-vs-preview JSON emit split.
- **(C) Normalization into a different block (rejected).** Folding into a
  `Div`/`Span` changes the DOM (`<div class><p>…`), which is not the `<p class>`
  we want.

**Recommendation: (B).** Resolve in Rust so the merge rule is single-sourced;
lean **(B1 sidecar)** to keep JSON Pandoc-valid and reuse the React renderer's
existing `astContext` lookup, falling back to a `para.attr` field (B2) only if a
first-class attributed paragraph is judged worth the wider change. A hybrid is
also possible: a `para.attr` field internally (clean for `html.rs`) that
`json.rs` emits into the `astContext` sidecar (Pandoc-valid wire form).

### 2. Collection semantics

- **Which nodes:** all `Inline::Attr` in the block, or only a trailing run?
  (Heading precedent is trailing-only.) Leaning: collect *all*, but document.
- **Merge rule:** classes accumulate (in order), ids — last non-empty wins (or
  error on conflict?), key-values — last wins.
- **Whitespace:** strip the `Space`/`SoftBreak` immediately preceding a
  collected `Inline::Attr` so no trailing space leaks into the element text.
- **Empty-attr no-op:** an empty `Inline::Attr` collects to nothing (today
  postprocess already treats empty attrs specially — see `is_empty_attr`).

### 3. Which blocks, in what order

- `Paragraph` first (unblocks the figure caption) — the attr rides *in* the
  paragraph's own inlines, so collection/application is local to the `<p>`.
- `Plain` next (same shape; the reveal caption may be a `Plain`, not `Para`).
- **List items** are a *different mechanism* and probably a later sub-task. A
  Pandoc list item is `[[Block]]` with no per-item attr; the injected
  `Inline::Attr` would live inside the item's first/last child block (a
  `Plain`/`Para`). Producing `<li class>` therefore means the list writer must
  **hoist** the resolved attr from the item's child block up to the `<li>`
  element — extra plumbing beyond the `<p>` case. (Note the existing
  `list_item_open` at `html.rs:1272` already special-cases `class="fragment"`
  for incremental reveal lists; an item attr must compose with that.) Keep the
  collection helper block-agnostic (returns `(content, Attr)`); the *hoist* is
  list-writer-specific.
- Consider `BlockQuote` and others later.

### 4. Source mapping & round-trip

- `InlineAttr` already carries `attr_source` + `source_info`. For filter-injected
  attrs the source is `Generated{by:…}`. The HTML change is output-only; the
  `qmd` writer already round-trips `Inline::Attr` (`qmd.rs:2393`). Verify the
  collected-and-applied path keeps source info coherent for the source map.

## Affected code (survey, verified 2026-06-17)

Rust:
- `crates/quarto-pandoc-types/src/inline.rs:332` — `InlineAttr { attr,
  attr_source, source_info }`; `inline.rs:41` the `Inline::Attr` variant.
  (B2 only) `Paragraph`/`Plain` in `block.rs` would gain an `attr` field.
- `crates/pampa/src/writers/html.rs` — `Inline::Attr` dropped at `:1058`; the
  attr emitter is `write_attr` (`:504`, used by `Header`/`Div`); `Paragraph`
  writer `:1286`, `Plain` `:1282`, `list_item_open` `:1272`.
- `crates/pampa/src/writers/json.rs:922` (and `:2846`) — currently **errors**
  `Q-3-32` on `Inline::Attr`. This is the preview transport; must change.
- `crates/pampa/src/writers/native.rs:384` — also errors `Q-3-32`; decide
  whether native faithfully prints the resolved attr or stays an error.
- `crates/pampa/src/writers/{qmd,incremental}.rs` — already **round-trip**
  `Inline::Attr` back to `{…}`; must stay correct (don't double-emit if the
  transform also strips/relocates the node).
- `crates/pampa/src/writers/{plaintext,ansi}.rs` — no-op on `Inline::Attr`; fine.
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:924-969` — the Header
  fold precedent to mirror (trailing-only pop + `trim_inlines`).
- A new shared collection helper / transform — location TBD by Decision 1
  (writer-local helper for (A); an `AstTransform` in `quarto-core` for (B)).

TypeScript (preview-renderer, `q2 preview` / `q2-slides`):
- `ts-packages/preview-renderer/src/q2-preview/blocks/{Para,Plain,BulletList,OrderedList}.tsx`
  — apply the resolved/collected attr to the `<p>`/`<li>` wrapper.
- `ts-packages/preview-renderer/src/framework/types.ts` — (A only) add an
  `Attr` inline type; (B1) the sidecar lookup reuses existing `astContext`.
- `ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx` — (A only)
  register an `Attr` inline so it isn't a "(not yet implemented)" placeholder.

## Test plan (TDD)

Rust:
- `html.rs` unit tests (mirror `mod tests` there): `Paragraph` with a trailing
  `Inline::Attr{.caption}` → `<p class="caption">…</p>`; id + kvs applied;
  multiple attrs merge per rule; empty attr is a no-op; the `Space` before the
  attr is stripped (no trailing space in `<p>` text); `Plain` and a list item
  variant per scope.
- `json.rs` test: the same `Paragraph` serializes so the preview carries the
  resolved attr (node tree stays Pandoc-valid; attr in `astContext` for B1, or
  marker form for A) — guards the cross-renderer contract at the boundary.
- Round-trip: a parser-emitted `Inline::Attr` (heading/table path) still
  round-trips through `qmd`/`incremental` unaffected.
- **End-to-end** (per CLAUDE.md): drive a fixture through `render_document_to_file`
  for `q2 render` and assert `<p class="caption">` in the HTML; do not rely on
  `render_qmd_to_html` defaults.

TypeScript:
- preview-renderer test: AST with an attributed `Para`/`li` renders the wrapper
  with the class (vitest), so `q2 preview` matches `q2 render`.
- Real-render parity: render the reveal figure-caption fixture and confirm both
  `q2 render` HTML and the `q2 preview` (q2-slides) DOM show `<p class="caption">`.

## Relationship to other work

- **bd-38ioql41** (reveal figure auto-stretch) — Case 1's caption can then be a
  proper `<p class="caption">` (plan option (c)). That strand can ship first
  with a plain `<p>` and adopt this capability when ready; not a hard blocker,
  hence `discovered-from`/related rather than `blocks`.
- The `<li>` styling need (raised separately) is a second consumer; design the
  helper to serve both.
