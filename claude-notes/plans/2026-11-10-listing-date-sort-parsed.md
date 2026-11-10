---
title: 'Listing sort: order date fields by parsed date (bd-cysydu8x)'
date: 2026-11-10
description: 'A listing with sort by date compared the raw date text, so M/D/YYYY dates sorted wrong; compare parsed dates chronologically, as Quarto 1 does.'
status: active
braid:
  strand: bd-cysydu8x
---

Strand: bd-cysydu8x. Issue: <https://github.com/quarto-dev/q2/issues/1154>.

## Overview

**Current.** `apply_sort` in `crates/quarto-core/src/project/listing/sort.rs`
compares every field with `natural_compare` on the raw text. The `date` and
`date-modified` fields hold the author's text (`5/5/2023`), so a listing with
`sort: date` orders them as strings. Display and `data-listing-date-sort`
(in `helpers.rs`) already use `crate::dates::parse_date`.

**New.** For `date` and `date-modified`, `compare_items` parses both values
with `parse_date` and compares `unix_millis()`, as Quarto 1 does (it holds
these fields as `Date` values). ISO dates order exactly as before. A value
that doesn't parse sorts after every parsed date; two unparseable values keep
the old text comparison. Missing values still sort last in both directions.

## Work items

- [x] Failing unit tests in `sort.rs` (M/D/YYYY asc and desc, multi-key,
      mixed ISO and US forms, unparseable after parsed)
- [x] Parse date fields in `compare_items`
- [x] nextest, clippy (host and Windows), `xtask lint`, notes render
