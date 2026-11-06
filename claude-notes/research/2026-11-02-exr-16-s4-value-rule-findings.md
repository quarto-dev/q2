---
title: 'EXR-16 S4: the span value rule against EXR-3'
date: 2026-11-02
description: 'The Positron spike now runs the same value rule as q2 (last text output, then a per-target mime choice ported from Quarto 1). All 22 shared precedence cases, all 24 escape cases and the EXR-3 value cases pass. The only difference is deliberate: the editor view is text only.'
---

**Question.** Does "the last output carrying a text mime type, never keyed on `output_type`"
give the same values as q2\'s rule for inline expressions (EXR-3)?

**Answer.** Yes, once the mime choice is made per target. The spike\'s first rule read only
`text/markdown` and `text/plain`, which differs from EXR-3 for `HTML()` and for a DataFrame.
EXR-3 already ports Quarto 1\'s `displayDataMimeType`, which depends on the output format
(`claude-notes/plans/2026-10-28-exr-03-jupyter-iee.md`, decision 3). The spike now ports the same
function, so no rule change is needed on the q2 side.

**Branch.** `spike/q2-s3-inline-exec`, commit `acaa4450cd`. The test is
`src/vs/workbench/contrib/positronQuarto/test/browser/quartoInlineExprValueRule.vitest.ts`; the
rule is in `quartoInlineExpr.ts` (`displayDataMimeType`, `asEscapedMarkdown`,
`renderInlineValue`, `spanValueFromMessages`).

## Method

Two fixture files are shared with q2 and were copied unchanged into
`test/browser/fixtures/` on the spike branch (the source is
`ts-packages/quarto-api/src/jupyter/__fixtures__/`, also read by
`crates/quarto-core/src/presenter/inline.rs`): `display-data-precedence.json` (22 cases of
mime choice by flags) and `inline-escape.json` (24 cases of the quoted-value escape). The
value cases follow the EXR-3 list: an integer, a string, markdown, `HTML()`, `None`, a
DataFrame, a statement, an error, a value that arrives only as `display_data`, and an image-only
bundle.

## Result

All 58 tests pass (22 precedence, 24 escape, 12 value cases). The suite is run with
`npx vitest run src/vs/workbench/contrib/positronQuarto/test/browser/quartoInlineExpr`; with the
older scanner and executor tests that is 77 tests in three files.

| Value | HTML target | Editor view (text only) |
|---|---|---|
| `2` | `2` | `2` |
| `'hello_world'` | `hello\_world` | `hello\_world` |
| `Markdown('**b**')` | `**b**` | `**b**` |
| `HTML('<b>hi</b>')` | `` `<b>hi</b>`{=html} `` | `<IPython.core.display.HTML object>` |
| DataFrame | its html table as a raw span | its `text/plain` repr |
| `None` | `None` | `None` |

## The one deliberate difference

An editor decoration is text, so it cannot show raw HTML. `spanValueFromMessages` takes a
`textOnly` option that drops `text/html` and `text/latex` before the mime choice. The Positron
executor passes it. Without it, a DataFrame would show its html table in the editor, because Quarto
1\'s rule adds `text/html` whenever the bundle holds a table. For `HTML()` the editor therefore
shows the repr, which is a known limit and not a rule difference.

## Differences from q2 that are not rule differences

- Raw spans pick a backtick fence longer than any run in the value, as q2 does (an improvement
  on Quarto 1\'s fixed single backtick). The fixtures do not cover it; one test shows the plain
  case only.
- q2 treats a quoted empty string as "leave the span source in place"; the spike returns the
  empty string and does not special-case it.

## Not tested

Live message sequences from a real kernel (the cases are hand-built); R values; the latex target;
widget mime types beyond the precedence fixture.
