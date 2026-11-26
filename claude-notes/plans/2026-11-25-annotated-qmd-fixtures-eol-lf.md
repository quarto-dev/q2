---
title: 'annotated-qmd: pin test fixtures to LF'
date: 2026-11-25
description: 'The annotated-qmd test fixtures record byte offsets into their .qmd sources, but .gitattributes pinned only examples/ to LF, so a Windows CRLF checkout shifted the offsets and broke a content-provenance test.'
status: in-progress
---

**Braid:** bd-ej14v9lh

**Issue:** <https://github.com/quarto-dev/q2/issues/1343>

## Overview

`ts-packages/annotated-qmd/test/fixtures/*.json` record byte offsets into
the sibling `.qmd` files. `.gitattributes` pinned only
`ts-packages/annotated-qmd/examples/*` to `eol=lf`, so on Windows (where
`core.autocrlf=true` is the default) git checked the fixtures out with CRLF.
The two newlines before `line` in `yaml-block-scalar.qmd` each gained a byte,
and the "Substring over Concat" test in `test/content-provenance.test.ts`
read `'  li'` instead of `'line'`.

Fix: add `ts-packages/annotated-qmd/test/fixtures/* text eol=lf` next to the
existing examples pin and widen its comment to cover both.

## Work items

- [x] Add the `eol=lf` pin for `test/fixtures/*` in `.gitattributes`
- [x] Confirm with `git check-attr` and an `autocrlf=true` clone that the
      fixtures check out LF
- [x] Run the annotated-qmd tests
