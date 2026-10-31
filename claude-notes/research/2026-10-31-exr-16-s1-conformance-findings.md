---
title: 'EXR-16 S1: Positron parser and hash conformance, findings'
date: 2026-10-31
description: 'What Positron''s Quarto parser, content hash and cell ids do on the S1 fixture documents, from a vitest on the fork. The q2 side is not compared yet.'
---

**Question.** Where do Positron's parser, content hash and cell ids disagree with q2's?

**Branch.** `spike/q2-s1-conformance` in `/Users/gordon/src/positron-spike-s1` (a worktree of
Gordon's fork, cut from `upstream/main` at `f854bb8e66`, committed locally, not pushed).
The test is `src/vs/workbench/contrib/positronQuarto/test/browser/quartoQ2Conformance.vitest.ts`.

**Method.** `npm install --ignore-scripts` (91 seconds, node v24.5.0 although `.nvmrc` says
24.22.1, no complaint), then `npx vitest --run` on the one file (17 seconds). Hashes use
`StringSHA1` with the same recipe as the private `computeContentHash`. Cell ids and labels
come from `QuartoDocumentModel.cells`.

**What was not tested.** The q2 side. There is no fixture generator yet and `hash_definition`
(EXR-4 Phase 5) does not exist, so nothing below is a measured disagreement with q2. It is
Positron's behavior, to compare against q2's expected values later.

## Hash

- All four provisional vectors from the plan agree with node `crypto`, including `café`, the emoji
  and the `#| label: fig-é` text.
- A lone surrogate hashes the same as U+FFFD (`c3693aea616c886c` for both). The plan expected
  this. Whether q2 gives the same value is open.
- A CRLF cell has the same hash as its LF twin (`34bce5f775de97f5`), so Positron normalizes line
  endings before hashing. Whether q2 does is open.
- A `#| label:` line is part of the hashed text: the labeled cell hashes differently from the
  plain one.

## Cells and ids (id format `index-hash8-label`)

| Document | Positron result | Note |
|---|---|---|
| plain `{python}` | one cell, `0-34bce5f7-unlabeled` | |
| `#| label: fig-a` line | one cell, label empty | the label comment is not read as a label |
| `{r, label=foo}` | one cell, label empty | the brace label is not read |
| `{python #bar}` | label `#bar` | the `#` stays in the label |
| `{python .marimo}` and `{python.marimo}` | label `.marimo` for both | the language token is `python`; marimo is a label |
| `{.python}` | no cell | dotted form is not recognized |
| fence inside a blockquote | no cell | |
| fence inside a list item | no cell | |
| `~~~{python}` | no cell | tilde fences are not recognized |
| `{=html}` raw block | no cell | agrees with excluding raw blocks |
| four-backtick `markdown` fence holding a `{python}` fence | one cell | phantom cell |
| three-backtick `text` fence holding a `{python}` fence | one cell, lines 2 to 4 | phantom cell |
| two cells with prose between | indexes 0 and 1 | |

## Go or no-go

Hashes: go for sharing the algorithm; the line-ending and lone-surrogate cases need q2's values.
Ids: the label segment, nesting and tilde fences are findings, not fixes. If q2 ids must equal
Positron's, q2 would need Positron's label rules, which differ from the scanner's. Recorded for
the Phase A conformance tables.
