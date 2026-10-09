---
title: 'bd-c05x6 — Plumb SourceInfo into Q-13-4 body-link "missing document" warnings'
date: 2026-05-20
description: 'Passes the parser''s URL source range into the body-link missing-document warning so it reports a file, row and column, instead of only the broken target name.'
---

**Status**: Implementation complete. Awaiting user review before commit.

**Issue**: bd-c05x6 (P3, task)
**Parent (discovered-from)**: bd-hjv5o (item #1 in its checklist)
**Related precedent**: bd-qor9a (nav-surface SourceInfo plumbing) — done.
**Related**: bd-8d6rk (structured Q-13-\* diagnostic shape) — done.

## Reproducer

Running `cargo run --bin q2 -- render docs/` on this repo's docs site
today prints (excerpt):

```
Warning [Q-13-4]: Body link references missing document
'authoring/markdown/markdown-basics.qmd' is not in the project index.
ℹ Check the spelling, or confirm the target file is included in the render set.
```

There is no `at file:row:col` line — the user has to grep the source
tree to find which `.qmd` file holds the broken link. Compare with the
sidebar / navbar / footer Q-13-\* diagnostics, which (after bd-qor9a)
already render an `at …` line pointing at the offending YAML scalar.

The desired output looks like:

```
Warning [Q-13-4]: Body link references missing document
  at docs/authoring/markdown/index.qmd:42:8
'authoring/markdown/markdown-basics.qmd' is not in the project index.
ℹ Check the spelling, or confirm the target file is included in the render set.
```

(or, when source context is wired and the file is on disk, an
Ariadne-rendered source excerpt — same machinery as the nav surfaces).

## Why this is a one-line fix in spirit

Three pieces of infrastructure are already in place:

1. **The parser populates `Link.target_source.url`**. In
   `crates/pampa/src/pandoc/treesitter_utils/span_link_helpers.rs:109`,
   the inline-link constructor stores the URL range as
   `SourceInfo::Original` whenever the URL string is non-empty.
2. **The helper accepts an optional location.**
   `resolve_doc_relative_href` (in
   `crates/quarto-core/src/transforms/navigation_href.rs:287`) takes
   `location: Option<SourceInfo>` and, when `Some`, threads it through
   `DiagnosticMessageBuilder::with_location` inside
   `missing_document_warning`.
3. **The render driver already prints location.**
   `print_render_diagnostics`
   (`crates/quarto/src/commands/render.rs:704`) calls
   `diagnostic.to_text(Some(&result.render_output.source_context))`,
   which renders either an `at file:row:col` line or an Ariadne source
   excerpt when the diagnostic has location info.

The only missing wire is at the body-link callsite:

```rust
// crates/quarto-core/src/transforms/link_rewrite.rs:219-227
let new_url = resolve_doc_relative_href(
    &link.target.0,
    self.source,
    self.resolver,
    Some(self.index),
    None,                 // <-- this is the bug; should be link.target_source.url.clone()
    self.diagnostics,
);
```

bd-hjv5o item #1 already calls this out as a "lightweight follow-up"
to bd-qor9a — no type changes, no plumbing through new fields, no
RenderContext changes. The `Link` node carries the source info into
the visitor; we just have to use it.

## Decisions

**D1: Use `target_source.url`, not `link.source_info`.**
`target_source.url` is the precise byte range of the URL text inside
the parens of `[text](url.qmd)`. `link.source_info` covers the entire
`[text](url)` span, which is correct in a fallback sense but less
useful — the diagnostic should point at the URL, not the link's
display text. Both are populated by the same parser code path
(`span_link_helpers.rs`), so neither is strictly less reliable than
the other.

When `target_source.url` is `None` (URL was empty string — rare for
this codepath since the helper short-circuits on non-`.qmd` paths
before any diagnostic fires), we just pass `None` through; the
diagnostic loses its location but stays correct, exactly matching the
status quo.

**D2: No fallback to `link.source_info`.** If `target_source.url` is
`None`, the diagnostic stays location-less rather than falling back
to a less-precise span. Reasons:

- The only paths that reach the `.qmd`-shaped miss branch have a
  non-empty URL, which means the parser would have populated
  `target_source.url`. The only way it ends up `None` here is if a
  Lua filter (or some other AST-mutating transform) constructed a
  fresh `Link` programmatically without setting `target_source.url`.
  In that case, `link.source_info` is also likely to be
  `SourceInfo::default()` or some filter-provenance value that wouldn't
  resolve cleanly either.
- Keeps the code change to one cloned `Option` field; no conditional
  logic.

**D3: No changes to the resolution helper's signature.** The
`location: Option<SourceInfo>` parameter on `resolve_doc_relative_href`
has been forward-looking since it was added; this issue is the one
that finally populates it. The helper, the diagnostic builder, and
the renderer don't need any changes — just the one callsite.

**D4: Test shape mirrors the bd-qor9a nav-surface tests.** The
existing `link_rewrite_diagnostic_uses_body_link_label`
(`crates/quarto-core/src/transforms/link_rewrite.rs:637`) is the
right place to extend the unit-test coverage. We assert
`d.location.is_some()` after the rewrite, and we extend
`link_inline()` in the test helpers to accept (or construct from a
default) a synthetic `TargetSourceInfo { url: Some(SourceInfo::…), … }`
so the test exercises the populated path.

The integration test in `crates/quarto-core/tests/link_rewriting_pipeline.rs`
(lines 340 and 385) already builds Pandoc ASTs from real qmd source,
so its `Link::target_source.url` will be populated by the parser. We
add a third assertion (alongside the existing code + path checks)
that each Q-13-4 diagnostic carries a non-`None` location pointing at
the right file.

## TDD checklist

- [x] **Phase 0.1**: New unit test
  `link_rewrite_diagnostic_carries_source_location` (alongside
  the existing `link_rewrite_diagnostic_uses_body_link_label`)
  stamps a `SourceInfo` into `Link.target_source.url` and asserts
  that the emitted Q-13-4 carries it. Initial run **failed** with
  `left: None, right: Some(Original { … })`, confirming the
  `None`-passed status quo.
- [x] **Phase 0.2**: Extended the two Q-13-4 assertions in
  `crates/quarto-core/tests/link_rewriting_pipeline.rs`
  (`pipeline_body_link_broken_qmd_emits_diagnostic` and
  `pipeline_body_link_unresolvable_in_website_warns`) with
  `q_13_4.unwrap().location.is_some()`. Both **failed** with
  `location: None` before the fix.

## Implementation checklist

- [x] **Phase 1.1**: Changed the `None` at
  `crates/quarto-core/src/transforms/link_rewrite.rs:224` to
  `link.target_source.url.clone()`.
- [x] **Phase 1.2**: Re-ran the unit + integration tests; all three
  previously-failing tests now pass. Full
  `cargo nextest run -p quarto-core` clean (2060 passed, 33
  skipped).
- [x] **Phase 1.3**: Updated the doc-comments on
  `missing_document_warning`, `resolve_href_for_html`, and
  `resolve_doc_relative_href` in `navigation_href.rs` so they
  reflect the post-bd-c05x6 reality — body-link callsite now
  passes the URL's `SourceInfo`; the "forward-looking, callers
  pass None" caveats are gone.

## End-to-end verification

- [x] **E1**: `q2 render` on `docs/` (the user's reproducer) now
  emits the Q-13-4 warnings *with* source location:

  ```
  Warning: [Q-13-4] Body link references missing document
      ╭─[ docs/authoring/markdown/index.qmd:89:23 ]
      │
   89 │   * [Markdown Basics](./markdown-basics.qmd)
      │                       ─────────┬──────────
      │                                ╰── 'authoring/markdown/markdown-basics.qmd'
      │                                    is not in the project index.
  ```

  Both Q-13-4 warnings in the docs/ render carry an Ariadne
  excerpt pointing at the offending `.qmd:line:col`. Verified
  with `target/debug/q2 render /…/docs` after `cargo build --bin q2`.
- [x] **E2**: Skipped as a separate fixture — the two integration
  tests in `link_rewriting_pipeline.rs` already exercise the
  Q-13-4 path through `ProjectPipeline::run` against fixture qmds,
  and both now assert `location.is_some()`. A fresh fixture would
  duplicate that coverage without adding signal.
- [x] **E3**: `cargo xtask verify --skip-hub-build` — all 12 steps
  green (build + lint + workspace tests + WASM + q2-preview-spa
  bundle).

## What's deliberately out of scope

- **Other Q-13-* surfaces**: bd-qor9a already covered
  Q-13-1 / Q-13-2 / Q-13-3 / Q-13-7.
- **Image link diagnostics**: images don't lookup-check against the
  index (Q1 parity decision, `link_rewrite.rs:229-234`).
- **`Image::target.0`**: same as above — images point at static
  resources, not project documents.
- **Diagnostic *content* changes**: the warning text stays unchanged;
  we only add a location.
- **Source info on links constructed by Lua filters**: those won't
  have `target_source.url` populated, so the diagnostic remains
  location-less for filter-introduced links. That's consistent with
  how filter-introduced provenance is handled everywhere else.

## Future work (not this issue)

- The unit-test `link_inline()` helper currently produces
  `TargetSourceInfo::empty()`. We may want a sibling
  `link_inline_with_source(url, text, url_range)` so future tests can
  assert location-aware diagnostics without each test constructing
  the source info inline.
- Once this lands, bd-hjv5o item #1 can be checked off; items #2–#5
  (AutoSpec paths, listing contents paths, format.html.css /
  bibliography in frontmatter, crossref absolute-file refs) still
  need separate scoping.
