---
title: "Navbar: mark the item for the page's sidebar section active"
date: 2026-11-05
description: "GH #1064: a navbar item is active on every page of the sidebar that links to it, with aria-current, as in Quarto 1."
---

Strand: `bd-inew8yhl` · Issue: <https://github.com/quarto-dev/q2/issues/1064>

## Overview

**Before.** `mark_active` (`crates/quarto-core/src/transforms/navigation_active.rs`),
called from `NavbarGenerateTransform`, marks a navbar item active only when its
`href` is the current page's source path. On `guide/install.html` no navbar link
is active even though the page sits in the Guide section's sidebar. Active links
also lack `aria-current="page"`.

**After.** When no navbar item links to the current page, the first top-level
navbar item (left, then right) whose `href` also appears in the page's resolved
sidebar is marked active. This matches Quarto 1, which marks the first
`a.nav-link` whose href is the current page or a link in `.sidebar-navigation`.
Exact matches still win and still mark every duplicate, as before. Active
`nav-link` anchors carry `aria-current="page"`.

To read the page's resolved sidebar, `SidebarGenerateTransform` now runs before
`NavbarGenerateTransform` (`crates/quarto-core/src/pipeline.rs`). Neither reads
the other's output otherwise.

Small deviation from Quarto 1: Quarto 1 takes the first link in document order
that matches either way, so a section link placed before the page's own link
would win. Here the page's own link always wins.

## Work items

- [x] Integration test reproducing the issue (`navbar_footer_pipeline.rs`), seen failing
- [x] `sidebar_hrefs` + `mark_active_for_section` in `navigation_active.rs`, with unit tests
- [x] Fallback call in `navbar_generate.rs`; reorder the generates in `pipeline.rs`
- [x] `aria-current="page"` on active `nav-link` in `render_html.rs`
- [x] Update the exact transform-order lists in `pipeline.rs` tests
- [x] nextest, clippy, Windows cross clippy, note render
