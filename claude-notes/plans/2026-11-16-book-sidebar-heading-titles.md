---
title: 'Book sidebar labels chapters titled by their first heading (bd-icgwln66)'
date: 2026-11-16
description: 'A book chapter with no front-matter title showed its file name in the sidebar; Quarto 1 falls back to the first heading, and now q2 does too.'
braid:
  strand: bd-icgwln66
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1234> ·
**Branch:** `braid/bd-icgwln66-book-sidebar-heading-titles`

## Overview

**Before.** `chapter_to_sidebar_item` (`crates/quarto-core/src/project/book/config.rs`)
labels a chapter from its explicit `text:` or its document-profile title, and the profile
title comes only from metadata. A chapter titled by `# Introduction {#sec-intro}` has
neither, so it stayed a bare href and the sidebar showed `intro.html`. Pagination
links, which reuse the sidebar entries, showed the same.

**Quarto 1.** `inputTargetIndex` (`src/project/project-index.ts`) takes the metadata
`title`, else `markdown.headingText`: the first heading found by
`markdownWithExtractedHeading` (ATX or setext, fence-aware) with the `#` prefix and a
trailing `{attrs}` removed by `parsePandocTitle`.

**After.** `book_render_items` already reads each chapter's source to decide its
numbering. It now also records the first-heading text on `BookRenderItem::heading_title`
(new `chapter_heading_title` in `render_item.rs`, sharing a `first_heading` port of
Q1's extractor with `chapter_is_numbered`). The sidebar falls back to it after the
explicit `text:` and the profile title, decorated with the chapter number when numbered.

Side effect: `chapter_is_numbered` now stops at a setext heading the way Q1 does, rather
than skipping it and reading a later ATX heading's classes.

## Work items

- [x] `first_heading` / `parse_pandoc_title` / `chapter_heading_title` in `render_item.rs`, with unit tests
- [x] `BookRenderItem::heading_title`, filled when the chapter is read for numbering
- [x] Sidebar fallback in `config.rs::chapter_to_sidebar_item`, with a unit test that fails without it
- [x] Heading markdown projected to plain text via the qmd reader
- [x] End-to-end check with the issue's three-chapter book: sidebar reads Preface, 1 Introduction, 2 Has YAML title

## Not covered

- The heading's markdown is projected to plain text (`# The *R* language` → "The R
  language"), the same projection a front-matter title gets in the sidebar. Inline
  formatting is dropped rather than rendered, as for front-matter titles.
- The page `<title>` of a heading-titled chapter is still just the book title; filed separately.
