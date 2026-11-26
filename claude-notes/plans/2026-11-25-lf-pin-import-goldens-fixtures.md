---
title: 'quarto-core: pin import-recordings and pandoc-goldens fixtures to LF'
date: 2026-11-25
description: 'Two hub-client harness specs failed on Windows because their fixture inputs checked out with CRLF. Pin the text fixtures to eol=lf in .gitattributes and guard the pin with a test.'
status: in-progress
---

**Braid:** bd-00i1n8ny

**Issue:** <https://github.com/quarto-dev/q2/issues/1347>

## Overview

On `windows-latest`, `pandoc-import-emf.harness.spec.ts` (basic-docx
`expected.qmd`) and `pandoc-parity.harness.spec.ts`
(`smoke-all/mermaid/backticks.qmd`) fail because the inputs check out with
CRLF line endings. No `.gitattributes` rule covered
`crates/quarto-core/tests/fixtures/import-recordings/**` or the
`pandoc-goldens` sources outside `parity/`.

The fix pins the text types in both directories (`qmd`, `md`, `txt`, `json`,
`rtf`) to `text eol=lf` by extension and marks the binaries (`docx`, `pptx`,
`epub`, `odt`, `png`, `jpg`, `emf`, `wmf`) as `binary`. Every text file is
already LF in the index, so no renormalization is needed.

## Work items

- [x] Add the `.gitattributes` rules.
- [x] Add `crates/quarto-core/tests/integration/fixture_line_endings.rs`:
  a `git check-attr eol` check (fails before the fix on every platform)
  and a no-CRLF content check (fails on a CRLF checkout).
- [ ] Rerun the two harness specs on `windows-latest` through the opt-in
  `runners` workflow input (after the PR is open).
