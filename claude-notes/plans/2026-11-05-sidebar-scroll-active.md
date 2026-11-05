---
title: "Sidebar: scroll the active item into view on page load"
date: 2026-11-05
description: "Port Quarto 1's active-sidebar-item scroll into q2's quarto-nav.js (GH #1065)."
---

Strand: `bd-b4q1xni0`. Issue: <https://github.com/quarto-dev/q2/issues/1065>.

## Overview

On a website with a long sidebar, Quarto 1 scrolls `nav#quarto-sidebar`
on load so the active item is visible. q2 always leaves the sidebar
scrolled to the top, so a page far down the list cannot see itself in
the navigation.

Quarto 1 does this in `quarto-nav.js` ("Scroll the active navigation
item into view, if necessary"): a `ResizeObserver` on the sidebar waits
until its height has resolved, then, if `li.sidebar-item a.active` is
below the visible area, sets `scrollTop` to the item's `offsetTop`.
q2's port of that file (`resources/js/quarto-nav/quarto-nav.js`) only
took the header machinery and left this block out.

The fix ports the block into `quartoNavMain`. One deliberate
deviation: Q1 runs it only off `file:` because it sits inside the
index.html link fixup block. The scroll has nothing to do with links,
so q2 runs it on every page.

## Work items

- [x] Behavior tests in
  `ts-packages/preview-renderer/src/q2-preview/quarto-nav-sidebar-scroll.integration.test.ts`
  (JSDOM, stubbed geometry, fake `ResizeObserver`); confirmed failing.
- [x] Port the block into `resources/js/quarto-nav/quarto-nav.js`.
- [x] Confirm `quarto-nav.js` ships on a sidebar-only site, and check
  the repro from the issue in Chrome.

## End-to-end check

The issue's 41-page sidebar-only site, rendered with `q2 render`,
served over HTTP and opened in Chrome at 1280×800. The page ships
`site_libs/quarto-nav/quarto-nav.js` (the secondary nav renders a
`#quarto-header`). Measured on `page-35.html`:

```
scrollTop: 332.5, scrollHeight: 1132, clientHeight: 800
active item "Page 35" at top: 647 px, visible
```

That matches Quarto 1's 647 px (Q1's `scrollTop` is 326 because its
sidebar is 6 px shorter). `page-03.html` stays at `scrollTop: 0`, and
`page-35.html` opened from `file://` scrolls the same way.
