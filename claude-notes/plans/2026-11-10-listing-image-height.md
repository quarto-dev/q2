---
title: 'Listing thumbnails honor image-height (GH #1155)'
date: 2026-11-10
description: 'Grid listings ignored image-height, so large thumbnails rendered at full size; emit the inline height and card-img class the way Quarto 1 does.'
status: done
braid:
  strand: bd-xgwaawhf
---

Strand: `bd-xgwaawhf` · Issue: <https://github.com/quarto-dev/q2/issues/1155>

## Overview

**Before.** A grid listing's thumbnail was
`<img class="thumbnail-image">` with no size, so a 1600px image rendered
at full size, overlapped the next cards and widened the page.
`listing.image_height` was parsed but never emitted. The default type
carried an unused `120px` default, and grid/table had none.

**After (matching Quarto 1).** Q1's `utilities.img`
(`website-listing-template.ts`) adds `style="height: <image-height>;"`
whenever `image-height` is set. `website-listing-read.ts` defaults it to
`150px` for grid and `40px` for table and leaves it unset for the default
type. `item-grid.ejs.md` adds `card-img`. The empty placeholder div
(`emptyDiv`) and the post-render preview `<img>` (`imageSrc`, always
`thumbnail-image card-img`) carry the same height.

Code locations, all in `crates/quarto-core/src/project/listing/`:

- `config.rs` `apply_type_defaults`: grid `150px`, table `40px`, no
  default for the default type.
- `helpers.rs`: `image_html` adds `card-img` for grid listings and the
  height style. `image_height_style` and `image_placeholder_html` are new.
  `image_placeholder_begin` writes `height=<h>` into the marker's attrs.
- `binding.rs`: new per-item `image-placeholder-html` key, which the
  `item-grid` and `item-default` templates use instead of the literal
  empty div.
- `post_render_upgrade/substitute.rs`: `build_thumbnail_img` reads the
  marker's `height=` (`marker_height`) and adds `card-img`.

## Work items

- [x] Failing unit tests: config defaults, `image_html`,
  `image_placeholder_html`, marker attrs, L7 substitution
- [x] Implement the defaults, helpers, binding key, templates and
  substitution
- [x] Update assertions that pinned the exact `class="thumbnail-image"`
  on substituted images
- [x] End-to-end integration test
  (`listing_inline_records::grid_thumbnails_carry_image_height_and_card_img`)
- [x] Render the issue's repro with `q2 render`: output matches the Q1
  expected HTML
