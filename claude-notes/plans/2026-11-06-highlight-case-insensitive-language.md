---
title: "Highlight: case-insensitive code block language matching"
date: 2026-11-06
description: "Match a code block's language class to a grammar case-insensitively, so R and Dockerfile fences highlight (GH #1080)."
---

Strand: `bd-e49rwirw`. Issue: <https://github.com/quarto-dev/q2/issues/1080>.

## Overview

**Current.** `quarto-highlight` looks a code block's class up in the built-in
alias map (`Registry::resolve`, `crates/quarto-highlight/src/registry.rs`) and
in the user-grammar provider (`pick_first_resolvable_class` / `annotate_attr`,
`crates/quarto-highlight/src/annotate.rs`) by exact string. A `R` or
`Dockerfile` fence is left unhighlighted, though `r` and `dockerfile` work.
Quarto 1 (Pandoc) matches language names case-insensitively.

**New.** The lookup is case-insensitive; the author's class is unchanged on
the element.

- `Registry::resolve` tries the class as written, then its ASCII-lowercased
  form. Every built-in key and alias is lowercase. This also covers the
  public `highlight`, `highlight_captures` and `is_language_supported`, so the
  editor's semantic tokens agree with the render.
- The annotation walker asks the user-grammar provider for the class as
  written, then lowercased, and highlights with whichever key the provider
  recognized. This works for every `UserGrammarProvider` (native wasmtime and
  the hub-client JS one) without changing them. A user grammar whose `.wasm`
  stem is lowercase (the convention) now matches any casing of the class.

## Work items

- [x] Failing tests: built-in `R` / `Python` annotate; mock user grammar
      `dockerfile` matches `Dockerfile`; `is_language_supported("R")`.
- [x] `Registry::resolve` lowercase fallback.
- [x] Walker: exact-then-lowercase user-grammar lookup.
- [x] `cargo nextest run -p quarto-highlight -p quarto-lsp-core`, clippy (host + Windows cross).
