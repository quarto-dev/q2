---
title: "HTML: write the document's `dir` on the root element"
date: 2026-11-16
description: "GH #1219: `dir: rtl` was accepted but never written to the html element, so RTL pages rendered left-to-right."
---

Strand: bd-9x0rhis4 · Issue: <https://github.com/quarto-dev/q2/issues/1219>

## Overview

**Current behaviour.** The document option `dir` (text direction) reaches
metadata, but no HTML output writes it. The built-in templates in
`crates/quarto-core/src/template.rs` open with
`<html$if(lang)$ lang="$lang$"$endif$>` (the minimal template and the
`html_head!` macro shared by the full and dashboard templates), and the
reveal.js scaffold in `crates/quarto-core/src/revealjs/assemble.rs` writes
`<html lang="{lang}">`. Arabic, Hebrew, Persian and Urdu pages render
left-to-right.

**New behaviour.** Match Quarto 1 and Pandoc's HTML5 / reveal.js templates:
`$if(dir)$ dir="$dir$"$endif$` on `<html>`, and the same in the reveal.js
scaffold (attribute omitted when `dir` is unset).

Only the `dir` attribute is in scope. Quarto 1's `xmlns` / `xml:lang`
attributes are not added.

## Work items

- [x] Unit tests: all three built-in HTML templates write `dir` (and omit it when unset)
- [x] Integration test: the reveal.js scaffold writes `dir`
- [x] Templates: add `dir` to the minimal template and `html_head!`
- [x] Reveal.js scaffold: add `dir` in `assemble.rs`
- [x] Verify (nextest, clippy, Windows cross-clippy, xtask lint, notes render)
