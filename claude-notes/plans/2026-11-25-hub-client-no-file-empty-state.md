---
title: 'hub-client: empty state when no file is selected'
date: 2026-11-25
description: 'A project with no files mounted an editable buffer that no file backs, and the preview showed a broken placeholder message. Show an empty state with a New file action instead.'
status: in-progress
---

**Braid:** bd-xrrugsfv
**Issue:** <https://github.com/quarto-dev/q2/issues/957>

## Overview

Current behaviour: when `currentFile` is null (a project with no files),
`hub-client/src/components/Editor.tsx` still mounts `DocumentEditor`
(Monaco), so the user can type into a buffer that is never saved. The
preview pane mounts `PreviewRouter`, which hands the literal fallback
`'no currentFile path'` to `NonQmdPlaceholderView`, rendering
"This .no currentFile path file can be edited in the editor".

New behaviour: with no current file, `Editor.tsx` renders a
`no-file-empty-state` block in place of the editor and preview panes:
"This project has no files yet. Create a file or pick one from the Files
panel." plus a **New file** button wired to the existing `handleNewFile`.
`PreviewRouter.tsx` returns `null` when it has no file instead of building
a message from a placeholder string.

## Work items

- [x] Characterization tests in `Editor.characterization.test.tsx`
      (`files: []` shows the empty state, no Monaco, no preview router;
      New file opens the dialog; a file arriving replaces the empty state)
- [x] Empty state in `Editor.tsx` + styles in `Editor.css`
- [x] `PreviewRouter.tsx`: no placeholder-string fallback
- [x] hub-client vitest, typecheck, ESLint, CSS lint
