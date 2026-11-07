---
title: "HTML: wrap code blocks in div.sourceCode for every language Quarto 1 recognizes"
date: 2026-11-06
description: "Match Quarto 1's div.sourceCode wrapping for code blocks whose language has no q2 grammar (GH #1079)."
---

Strand: `bd-jxzjn4bs`. Issue: <https://github.com/quarto-dev/q2/issues/1079>.
Triage: [`claude-notes/issue-reports/1079/triage.md`](../issue-reports/1079/triage.md).

## Overview

**Current.** The HTML writer (`crates/pampa/src/writers/html.rs`, `Block::CodeBlock` arm) and the preview renderer
(`ts-packages/preview-renderer/src/q2-preview/blocks/CodeBlock.tsx`) emit
`div.sourceCode > pre.sourceCode > code.sourceCode` only when the block carries a non-empty `data-hl-spans` list.
That list exists only for languages with a q2 grammar, so `ini`, `default`, `toml`, `conf` and so on render as a
bare `<pre>`, without the theme's code-block box.

**New (decision: match Quarto 1 first).**

1. `quarto-highlight` gets a generated table of the syntaxes Pandoc 3.10 knows (skylighting-core 0.14.7) plus
   Quarto 1's extra syntax definitions, with skylighting's lookup rule: full name or short name (case-insensitive),
   then the class as a file extension matched against each syntax's globs.
2. The annotate step writes an empty `data-hl-spans` list (`[]`) on a code block that got no spans but has a class
   Pandoc knows. Inline `Code` is unchanged.
3. The HTML writer and the preview renderer wrap a code block whenever `data-hl-spans` is present, even if empty.

## Work items

- [x] Generator script + generated table (`crates/quarto-highlight/scripts/gen_pandoc_languages.py`, `src/pandoc_languages.rs`)
- [x] Unit tests for the lookup (name, short name, case, extension globs, unknown classes)
- [x] Annotate: empty marker for known-but-ungrammared blocks; tests
- [x] HTML writer: wrap on attribute presence; tests (`unhighlighted_codeblock_has_no_div_wrapper` still holds: no attribute, no wrapper)
- [x] Preview renderer: same rule; tests
- [x] End-to-end test over the issue's repro
- [x] Verify (nextest, clippy, Windows cross clippy, vitest, tsc, eslint, notes render)

Out of scope (follow-up strand `bd-hodntkwd`): the writers take the *first* class as the language, so
`{.cell-code .toml}` gets the wrapper but puts `cell-code` on `pre`/`code` where Quarto 1 puts `toml`.

The lookup was cross-checked against `quarto pandoc` (3.10, with Quarto 1's five syntax definitions): every name,
short name and simple `*.ext` glob in the table, plus a few negatives, 598 probes in all, wraps in Pandoc exactly
when `is_pandoc_known_language` says so. (18 further probes, such as `c++` or `1`, are not valid Pandoc class
names, so Pandoc never sees them as code-block classes.)
