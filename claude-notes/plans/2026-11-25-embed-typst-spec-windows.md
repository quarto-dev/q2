---
title: 'q2-preview-spa: missing-typst embed spec works on Windows'
date: 2026-11-25
description: 'The missing-typst embed E2E built its pandoc-only PATH with which, an extensionless symlink, and /usr/bin:/bin, so on Windows q2 reported pandoc missing (Q-20-1) instead of typst (Q-21-1). Build that PATH the Windows way on win32.'
status: in-progress
---

**Braid:** bd-ultpa32o
**Issue:** <https://github.com/quarto-dev/q2/issues/1350>

## Overview

`q2-preview-spa/e2e/embed-typst-pdf.spec.ts`, test "the pane banner names the
missing compiler (Q-21-1) instead of staying blank", gives the spawned server
a PATH that holds pandoc but not typst. It found pandoc with `which`,
symlinked it as `pandoc` (no `.exe`), and appended `/usr/bin` and `/bin`. On
Windows the extensionless name doesn't resolve, so q2 reports Q-20-1 (pandoc
missing) before it gets to typst, and the assertion on Q-21-1 fails.

On win32 the spec now finds pandoc with `where` (first line), copies it as
`pandoc.exe` (symlinks need extra privileges on Windows), and uses
`%SystemRoot%\System32` as the system directory, both for the server's PATH
and for the `onPathIn` typst skip check. Other platforms are unchanged.

## Work items

- [x] Build the pandoc-only PATH per platform in the spec
- [x] Typecheck the spec
- [ ] CI: the q2-preview-spa-e2e workflow runs the spec on Linux; a
      `workflow_dispatch` with `runners=["windows-latest"]` covers Windows
