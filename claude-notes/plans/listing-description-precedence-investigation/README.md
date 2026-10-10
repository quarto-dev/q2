# Repro: listing description precedence (bd-listing-description-precedence-x4bh6w3m)

`repro/` is a website project with four listed posts. Each post has a body
paragraph that L7 derivation could use:

| post | front matter                        | body          |
|------|-------------------------------------|---------------|
| a    | `description: EXPLICIT-A`           | `BODY-A ...`  |
| b    | (none)                              | `BODY-B ...`  |
| c    | `listing-item: {description: LISTING-ITEM-C}` | `BODY-C ...` |
| d    | `abstract: ABSTRACT-D`              | `BODY-D ...`  |

It also has three listing pages over `posts/`: `default.qmd`, `grid.qmd`, and
`table.qmd` (`fields: [title, description]`).

```
cd repro
../../../../target/debug/q2 render
quarto render --output-dir _site-q1          # Quarto 1 (99.9.9 dev build)
for t in default grid table; do
  echo "== $t"; grep -oE "(EXPLICIT|BODY|LISTING-ITEM|ABSTRACT)-[A-D]" _site/$t.html | sort | uniq -c
done
```

## Results (2026-10-09, main @ ea72d68aa)

| listing | a (explicit) | b (none) | c (listing-item) | d (abstract) |
|---------|--------------|----------|------------------|--------------|
| q2 default | **BODY-A** ✗ | — ✗ | **BODY-C** ✗ | — ✗ |
| q2 grid    | **BODY-A** ✗ | — ✗ | **BODY-C** ✗ | — ✗ |
| q2 table   | EXPLICIT-A ✓ | — ✗ | LISTING-ITEM-C ✓ | — ✗ |
| Q1 (all three) | EXPLICIT-A | BODY-B | BODY-C¹ | ABSTRACT-D |

¹ Q1 has no `listing-item:` key, so it ignores it and derives a description.
In q2, `listing-item.description` is the documented author override.
`crates/quarto-core/tests/integration/document_profile_pipeline.rs`
asserts that it wins, so the q2 target is LISTING-ITEM-C.

Reading the table:

- **a and c (this strand).** The item data is correct, as the table listing
  shows. Default and grid wrap `$description$` in the L7 envelope for every
  document-origin item (`binding.rs` ~441). The L7 pass in
  `post_render_upgrade/substitute.rs` then replaces whatever is inside the
  envelope with the sibling page's first paragraph, unconditionally.
- **b (bd-listing-default-no-derived-desc-m0wrr8ty).** Without an L1
  description, `$if(description)$` suppresses the envelope, so L7 has nothing
  to replace. Table emits no envelope at all.
- **d.** q2 has no `abstract` fallback. `DocumentProfile` doesn't carry
  `abstract`, and `hydrate_item` only chains `listing-item.description` →
  `description`. Q1 (`website-listing-read.ts:1127`) chains
  `description || abstract || placeholder`.

Net: q2 inverts Q1. It replaces authored descriptions and leaves missing ones
empty.

## After the fix (2026-10-09, branch `braid/bd-listing-description-precedence-x4bh6w3m-…`)

| listing | a (explicit) | b (none) | c (listing-item) | d (abstract) |
|---------|--------------|----------|------------------|--------------|
| q2 default/grid/table | EXPLICIT-A ✓ | BODY-B ✓ | LISTING-ITEM-C ✓ | ABSTRACT-D ✓ |
| Q1 (all three) | EXPLICIT-A | BODY-B | BODY-C¹ | ABSTRACT-D |

Identical to Q1 except c, where q2's documented `listing-item:` override
wins. No envelope markers remain in the output, and in the grid the
derived description keeps its link (`<a href="posts/b.html">BODY-B
paragraph.</a>`).

## Follow-up after #815 (2026-10-09)

#815 (bd-p80b9jy9) gave `type: custom` without `fields:` the fields some
item carries (`fields_items_carry`). That check didn't count a
description or image that only L7 can derive, so a custom listing whose
items had no L1 description dropped the envelope. `custom.qmd` lists
`derived-only/e.qmd`, whose only paragraph sits inside a div. L1 skips
that paragraph, but L7 finds it in the rendered page. Before the fix,
`INSIDE-DIV-E paragraph.` was missing from `custom.html`; after the fix,
it shows.
