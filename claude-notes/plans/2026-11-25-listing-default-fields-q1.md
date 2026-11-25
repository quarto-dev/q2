---
title: 'listings: default and grid field defaults match Quarto 1'
date: 2026-11-25
description: 'Default and grid listings without fields defaulted to an 11-field set, so items showed categories and reading time that Quarto 1 leaves out. Use the Quarto 1 sets and add categories only when the listing sets categories.'
status: in-progress
---

**Braid:** bd-p6m552at
**Issue:** <https://github.com/quarto-dev/q2/issues/1031>

## Overview

`apply_type_defaults` in `crates/quarto-core/src/project/listing/config.rs`
filled an 11-field set for `default` and `grid` listings, including
`categories`, `filename`, `file-modified` and `reading-time`. The binding's
presence filter (`effective_fields` in `binding.rs`) keeps any defaulted
field some item carries, and every document has a reading time, so every
item showed a "1 min read" line and, when it had categories, a chip row.

Quarto 1 (`website-listing-read.ts`) uses six fields for `default` (date,
title, author, subtitle, image, description) and five for `grid` (title,
author, date, image, description). It appends `categories` to a default or
grid listing only when the listing sets `categories:`, and an explicit
`fields:` replaces the whole set.

The new behaviour copies that: the Quarto 1 sets, plus `categories` when the
listing's categories mode isn't `Disabled` and `fields:` wasn't given. One
consequence is that a grid listing no longer shows the subtitle by default,
as in Quarto 1.

## Work items

- [x] Unit tests in `config.rs` for the default/grid sets and the
      `categories:` append (fail before the fix).
- [x] Replace the default/grid sets in `apply_type_defaults` and append
      `categories`.
- [x] Update the `listing_render.rs` tests and two integration tests that
      relied on the old set (categories, reading time, grid subtitle).
- [x] Run the quarto-core tests, clippy (host and Windows), and `xtask lint`.
