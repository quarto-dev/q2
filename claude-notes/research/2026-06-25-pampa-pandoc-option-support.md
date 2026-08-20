# pampa / quarto-core support for Pandoc `FormatPandoc` options

**Status:** reference / supporting evidence for PROTO-1 in
`claude-notes/plans/2026-06-25-plan1a-return-to-q1.md`.
**Method:** code search over `crates/pampa/src/writers/` + `crates/quarto-core/src/`
on `review/1b-engine-host-deno`, classifying each `FormatPandoc` field
(`external-sources/quarto-cli/src/config/types.ts:566+`) by whether q2's *own*
pipeline honors it. **Date:** 2026-06-25.

## Why this exists

q2 has **no pandoc binary** — `pampa` is its own parser + writer, emitting
HTML/etc. directly from a Pandoc AST. So `FormatPandoc` options (`to`, `template`,
`variables`, `toc`, `number-sections`, `highlight-style`, `filters`,
`include-in-header`, …) are only meaningful to the extent **q2's pipeline honors
them**. This inventory answers the PROTO-1 "q2-recovery check" for the dropped
`ExecuteResult.pandoc?` (`FormatPandoc`) field: an engine returning `pandoc?`
overrides has a real recovery path for whatever q2 actually honors.

## Per-option classification

| Option | Status | Evidence |
|---|---|---|
| from / to / writer | Supported | `FormatIdentifier` (`quarto-core/src/format.rs:23+`); writers in `pampa/src/writers/mod.rs` |
| template | Supported | `quarto-core/src/template.rs:55+`; ext metadata `extension/read.rs:239` |
| output-file | Unsupported | CLI output-path only; no metadata key |
| standalone | Partial | q2 always emits a complete templated document |
| self-contained / embed-resources | Unsupported | no resource-embedding feature |
| variables | Partial | metadata flows to template context; no `--variable` semantics |
| markdown-headings | Unsupported | parser normalizes headings |
| include-in-header / before-body / after-body | Supported | `stage/stages/include_resolve.rs:73+` |
| resource-path | Unsupported | resources resolved via project context, not this key |
| reference-location | Supported | `transforms/appendix.rs:94+`, `footnotes.rs:23+`, `revealjs/footnotes.rs:105+` |
| citeproc | Supported | native `quarto-citeproc/` |
| cite-method | Unsupported | citeproc always on; no natbib/biblatex |
| filters | Supported | `quarto-core/src/filter_resolve.rs:73+` |
| quarto-filters | Unsupported | not distinct in q2 |
| pdf-engine / -opts / -opt | Unsupported | q2 does not shell to external engines |
| epub-cover-image | Unsupported | no EPUB image handling |
| css | Supported | `template.rs:391+` (paths injected) |
| toc / table-of-contents | Supported | `transforms/toc_generate.rs:87+`, `toc_render.rs:86+` |
| toc-depth | Supported | `transforms/toc_generate.rs:117+` |
| listings | Supported | `project/listing/config.rs:463+` |
| number-sections | Partial | metadata key recognized (`metadata_merge.rs:1741+`); not auto-applied to HTML |
| number-offset | Unsupported | no heading offset |
| highlight-style / syntax-highlighting / syntax-definitions | Unsupported | highlighting via tree-sitter (`stage/stages/code_highlight.rs`), not Pandoc's |
| section-divs | Partial | native section handling |
| html-math-method | Supported | `stage/stages/math_js.rs:119+` (MathJax/KaTeX/plain) |
| top-level-division | Unsupported | no Pandoc top-level wrapping |
| shift-heading-level-by | Unsupported | no heading shift |
| title-prefix | Unsupported | template-driven |
| slide-level | Supported | `revealjs/transform.rs:61+` |
| columns | Supported | `template.rs:395+` |

## Bottom line

q2 honors **~18 of ~37** `FormatPandoc` options — the **semantic layer** (toc/toc-depth,
citeproc, filters, include-in-header/before/after, reference-location, html-math-method,
css, template, slide-level, columns, number-sections) — and drops the options that need an
external binary (pdf-engine\*), post-processing (self-contained/embed-resources), or
Pandoc-specific text transforms (markdown-headings, syntax-highlighting, top-level-division,
shift-heading-level-by, title-prefix). HTML is the only real writer; PDF/DOCX/EPUB are
declared `FormatIdentifier`s but don't render today.

**Implication for PROTO-1:** dropping `ExecuteResult.pandoc?` from the wire is **unforced**
for the supported subset — an engine returning `pandoc?` overrides *could* be merged into
q2's format and honored, and (crucially) the transform pipeline runs **after** execute, so a
merge point exists in principle. The blocker is mechanical, not architectural: q2 has no
single `FormatPandoc`-shaped struct (options are scattered across metadata-merge + per-transform
reads), and format is currently an execute-*input* (`TsFormatInfo`) rather than an
execute-*output*. So PROTO-1 records `pandoc?` as a **documented, recoverable deferral**, not a
Q1-incompatibility.
