---
title: 'html: escape the description meta tag and keep curly quotes'
date: 2026-11-25
description: 'A quote in description or description-meta broke the head description meta tag, and the Open Graph and Twitter description dropped the quotes. Escape the attribute and write smart quotes as curly quotes, as Quarto 1 does.'
status: in-progress
---

**Braid:** bd-jg7wgmx1

**Issue:** <https://github.com/quarto-dev/q2/issues/1369>

## Overview

Current behaviour:

- The head template inserts `$description-meta$` into
  `<meta name="description" content="…">` unescaped, so a `"` ends the
  attribute early and a `&` stays bare.
- The plain-text helpers write a smart-quoted `Quoted` node as straight
  quotes: `inlines_to_text` in `crates/quarto-core/src/template.rs` always
  writes `"`, and `inlines_to_plain_text` in
  `crates/quarto-core/src/transforms/metadata_normalize.rs` writes `'` or `"`.
- `text_of` in `crates/quarto-core/src/transforms/website_social_meta.rs`
  reads metadata through `ConfigValue::as_plain_text`, which drops the
  quotes, so `og:description` and `twitter:description` lose them.

New behaviour (Quarto 1 parity):

- `metadata_entry_to_template_value` attribute-escapes `description-meta`.
- Both plain-text helpers write curly quotes (`‘…’`, `“…”`), as Pandoc's
  `stringify` does.
- `text_of` uses the crate's own plain-text helpers for markdown inlines
  and blocks, so the card tags keep the curly quotes.

## Work items

- [x] Failing unit tests in all three files
- [x] Escape `description-meta`; curly quotes in both helpers; `text_of`
      keeps quotes
- [x] End-to-end check with the issue's reproduction
- [x] quarto-core tests, clippy (host and Windows), `cargo xtask lint`,
      `cargo xtask verify --skip-hub-build`
