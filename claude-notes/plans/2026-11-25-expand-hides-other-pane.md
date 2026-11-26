---
title: 'hub-client: Expand markup and Expand preview hide the other pane'
date: 2026-11-25
description: 'The expand presets in the document header used to move the divider to its 15%/85% drag limit. They now send 1 and 0, and Editor hides the other pane and the divider for those values.'
---

**Braid:** bd-goed1vda · **Issue:** <https://github.com/quarto-dev/q2/issues/1037>

## Overview

Current behaviour: `SPLIT_PRESETS` in
`hub-client/src/components/ViewToggleControl.tsx` maps "Expand markup" to
0.85 and "Expand preview" to 0.15, the drag clamp, so the other pane stays
on screen as a 15% strip.

New behaviour: the expand presets send 1 (markup only) and 0 (preview
only). A divider drag can't produce those values. For them, `Editor.tsx`
adds `split-markup-only` / `split-preview-only` to `.editor-main`, and
`Editor.css` hides the collapsed pane and `.pane-divider` with
`display: none`. The preview stays mounted, so `PreviewRouter` keeps
feeding diagnostics to Monaco. Files with no preview pane (plain text,
images) and the fullscreen preview don't get the classes. "Split equally"
is unchanged.

## Work items

- [x] `SPLIT_PRESETS`: markup 1, preview 0; update the doc comment.
- [x] `Editor.tsx`: derive `splitCollapse`, add the class to `.editor-main`.
- [x] `Editor.css`: hide the collapsed pane and the divider.
- [x] Tests: `ViewToggleControl.test.tsx` (preset values, active state);
      `Editor.characterization.test.tsx` (classes, preview stays mounted,
      diagnostics still reach Monaco, plain-text file keeps buttons disabled).
- [ ] Check in a real browser (not done by the autofix run).
