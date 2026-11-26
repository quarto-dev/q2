---
title: 'quarto-nav: fire quarto-hrChanged on header offset updates'
date: 2026-11-25
description: 'Port the Quarto 1 quarto-hrChanged window event to the q2 quarto-nav.js, so page scripts and extensions can follow the navbar offset.'
status: in-progress
---

**Braid:** bd-cq5057e3 · **Issue:** <https://github.com/quarto-dev/q2/issues/1377>

## Overview

Quarto 1's `quarto-nav.js` dispatches a `quarto-hrChanged` `CustomEvent` on
`window` at the end of every `updateDocumentOffset` call except the first.
Site scripts (for example Great Docs) listen for it to re-place elements
after the script resets each `.sidebar`'s `top` and `max-height`.

The q2 port in `resources/js/quarto-nav/quarto-nav.js` makes the same updates
but never fires the event; its header comment listed this as a deliberate
deviation ("no consumer"). The consumers are outside q2, so port the event:
an `init` flag in `quartoNavMain`'s scope, and a dispatch at the end of
`updateDocumentOffset` once it is set.

## Work items

- [x] JSDOM test in
  `ts-packages/preview-renderer/src/q2-preview/quarto-nav-hrchanged.integration.test.ts`:
  no event on the first update, one on the 250ms update, one more on resize.
- [x] Dispatch the event in `updateDocumentOffset`; drop the deviation line
  from the header comment.
