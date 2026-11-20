---
title: 'Website: page-level `sidebar: id` and `navbar: true` keep site navigation (GH #1217)'
date: 2026-11-20
description: 'A page scalar sidebar or navbar replaced the whole site definition; match Quarto 1, where only false disables and a string sidebar selects one by id.'
status: in-review
braid: bd-rr8gbukc
---

Issue: <https://github.com/quarto-dev/q2/issues/1217>

## Overview

**Before.** `resolve_website_value` (`crates/quarto-config/src/website.rs`)
merges a page's top-level `navbar`/`sidebar` over `website.navbar`/`website.sidebar`,
top-level winning. A scalar (`sidebar: main`, `navbar: true`) therefore replaced
the site's whole definition, and the page lost its sidebar or navbar.
`sidebar_for_page` (`crates/quarto-navigation/src/sidebar.rs`) read the page's
sidebar choice only from `site-sidebar`.

**After (Quarto 1 semantics).** Quarto 1 (`website-navigation.ts`,
`sidebarForHref` in `website-shared.ts`) never merges page metadata into
the site navigation: a page's `navbar`/`sidebar` disables it only when
`false`, and a string `sidebar` picks the sidebar with that `id`.

- `resolve_website_value`: for `navbar` and `sidebar`, a top-level scalar
  other than `false` does not replace an existing `website.<key>`; the
  nested value is returned. (`false` still disables; maps still merge.)
- `sidebar_for_page`: also read a string top-level `sidebar` as the id.

## Work items

- [x] Tests first: `resolve_website_value` unit tests; `sidebar_for_page` test; render test for both repros
- [x] Fix `resolve_website_value`
- [x] Fix `sidebar_for_page`
- [x] Verify (nextest, clippy, Windows cross clippy, xtask lint, notes render)
