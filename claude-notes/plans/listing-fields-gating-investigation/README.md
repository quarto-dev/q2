---
title: 'Repro: listing fields gating (bd-p80b9jy9)'
date: 2026-10-09
description: 'Two-post website showing that grid and default listings render every field regardless of fields:.'
---

`repro/` is a website project with two posts under `posts/`. Both set `title`,
`subtitle`, `author`, `date` and `description`; `a` also has `categories`, and
`b` has an `image`. There are four listing pages: `default-narrow.qmd` and
`grid-narrow.qmd` use `fields: [date, title, description]`, while
`default-all.qmd` and `grid-all.qmd` use the type defaults.

```
cd repro
../../../../target/debug/q2 render
for f in default-narrow default-all grid-narrow grid-all; do
  echo "== $f"
  for p in img-placeholder pic.png SUBTITLE- AUTHOR- CAT- POST-; do
    printf '%s=%s ' "$p" $(grep -o "$p" _site/$f.html | wc -l)
  done; echo
done
```

## Results before the fix (2026-10-09, main @ ea72d68aa)

| page | placeholder | image | subtitle | author | categories | title |
|------|-------------|-------|----------|--------|------------|-------|
| default-narrow | 1 ✗ | 1 ✗ | 2 ✗ | 2 ✗ | 0 ✓ | 2 ✓ |
| default-all    | 1 | 1 | 2 | 2 | 1 | 2 |
| grid-narrow    | 1 ✗ | 1 ✗ | 2 ✗ | 2 ✗ | 0 ✓ | 2 ✓ |
| grid-all       | 1 | 1 | 2 | 2 | 1 | 2 |

In the narrow listings only categories respond to `fields:`. Every
other field is still rendered. The target for the narrow rows is 0 for
everything except title (and date and description).

Descriptions aren't counted. Both cards show "Body of …" instead of `DESC-*`,
because of bd-listing-description-precedence-x4bh6w3m, which is being fixed
separately.

## Results after the fix (2026-10-09, branch `braid/bd-p80b9jy9-listing-fields-gating`)

| page | placeholder | image | subtitle | author | categories | title |
|------|-------------|-------|----------|--------|------------|-------|
| default-narrow | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 2 ✓ |
| default-all    | 1 | 1 | 2 | 2 | 1 | 2 |
| grid-narrow    | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 2 ✓ |
| grid-all       | 1 | 1 | 2 | 2 | 1 | 2 |

Quarto 1 (`quarto render --output-dir _site-q1`, 99.9.9 dev build) gives
the same narrow rows. Its `*-all` rows differ: it shows no categories
(Q1 adds `categories` to the field set only when the listing sets
`categories:`) and, in the grid, no subtitle. That is a difference in the
default field sets, not in gating. It is tracked as bd-n7g28c3o.
