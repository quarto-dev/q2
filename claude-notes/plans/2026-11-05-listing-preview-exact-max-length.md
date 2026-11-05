---
title: 'Keep an exactly-max-length listing preview whole (bd-bpdh0rqh)'
date: 2026-11-05
description: 'The derived first-paragraph listing preview and the feed partial description keep text of exactly max-description-length characters whole, matching the truncateNode helper in Quarto 1.'
braid:
  strand: bd-bpdh0rqh
---

Strand: `bd-bpdh0rqh`. Issue: <https://github.com/quarto-dev/q2/issues/1068>.

## Overview

**Current behaviour.** When a listing item has no `description:`, q2 takes
the first paragraph of the item's page and truncates it to
`max-description-length` (default 175). The guard in `maybe_truncate`
(`crates/quarto-core/src/project/listing/post_render_upgrade/reader.rs`)
and in `maybe_truncate_visible`
(`crates/quarto-core/src/project/listing/feed/reader_ext.rs`) delegates to
`truncate_text_at_space`. That function ports Quarto 1's `truncateText`,
whose `trimLength` compares with a strict `<`. So a paragraph of exactly 175
characters comes out as 170 characters ending in `…`.

**Quarto 1.** Both the listing preview and the feed's `partial` description
(`rendered.firstPara`) come from `readRenderedContents`, which calls
`truncateNode` (`website-listing-shared.ts`). `truncateNode` calls
`truncateText` only when `currentLength + textNodeLen > maxLen`, so text of
exactly `maxLen` characters is kept whole. Explicit descriptions go through
`truncateText` directly (`website-listing-template.ts`), and there the strict
`<` does apply.

**New behaviour.** Both first-paragraph guards keep text of at most `max`
characters whole. The shared helper and the explicit-description path
(`binding.rs`, `metadata.rs`) are unchanged.

## Work items

- [x] Tests first: flip `maybe_truncate_exactly_max_is_truncated_q1_parity`,
      add a 175/176 repro test for `extract_first_para` and an exactly-max
      test for `extract_first_para_html`. Confirm they fail.
- [x] Change both guards to keep text of at most `max` characters whole.
- [x] Update the doc comments, including the one on `truncate_text_at_space`.
- [x] `cargo nextest run -p quarto-core`, clippy, `cargo xtask verify --skip-hub-build`.
