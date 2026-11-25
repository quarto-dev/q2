---
title: 'diagnostics: no Q-2-49 for a span with an empty attribute block'
date: 2026-11-25
description: 'Q-2-49 treated an explicit empty attribute block like a bare bracket group, because the parsed attribute is empty in both. Read the span source text to tell them apart.'
status: in-progress
---

**Braid:** bd-unxw412o

**Issue:** <https://github.com/quarto-dev/q2/issues/1188>

## Overview

Current behaviour: `[10]{}` warns with `Q-2-49` ("has no attribute block"),
exactly like a bare `[10]`. `is_bare_span` in
`crates/quarto-core/src/transforms/reference_link_diagnostics.rs` only looks
at the parsed attribute, which is empty in both cases.

New behaviour: a span whose source text ends in `}` was written with an
attribute block, so it is not bare. It gets no `Q-2-49`, and it can't be half
of a `Q-2-45` reference use or a `Q-2-46` definition either. Bare spans keep
every diagnostic they had.

Why source text and not offsets: the parser keeps `{}` in the span's source
range, but it also drops trailing spaces inside the brackets from the
content. So `[x  ]` and `[x]{}` both end three bytes past their content, and
only the text tells them apart. `RenderContext::source_context` provides it.
Without a source context (or for a span with no single source range), the
check keeps the warning.

## Work items

- [x] Tests: `[10]{}`, `[10]{ }`, `[]{}` and `[*x* y]{}` don't warn; a bare
      `[10]` or `[x  ]` next to one still warns; no source context still warns.
- [x] `collect_diagnostics` takes `Option<&SourceContext>`; `is_bare_span`
      skips spans whose source text ends in `}`.
- [x] End-to-end `q2 render` of the issue's repro: no warning for `[10]{}`,
      the HTML is unchanged.
- [ ] Not verified: `[10]{}` produced by engine output (`Markdown(...)` from
      Python), since Jupyter isn't installed on the machine that ran this.
