# Issue #1102 — Typst output prints editorial marks as plain body text

- **GitHub**: https://github.com/quarto-dev/q2/issues/1102
- **Reporter**: @EwoutH (Ewout ter Hoeven), 2026-11-07
- **Triage date**: 2026-11-07
- **Worktree**: `.worktrees/issue-1102` (branch `issue-1102`, based on `main` @ `09b740e81`)
- **Braid strand**: bd-7aj5b9ns
- **Scope**: `format: typst` rendering of all four editorial marks (`[++ ]`, `[-- ]`,
  `[!! ]`, `[>> ]`) in their inline and `:::` block forms. Out of scope: an
  "accept/reject changes" mode that applies or drops the marks (see Open questions).

## Summary

In Typst output, insertions, deletions, highlights and reviewer comments all come out as
ordinary body text, so a PDF silently shows deleted text and reviewer comments as if
they were content. Reproduced at HEAD. This is a real gap: docx and pptx already get a
dedicated transform (`EditorialMarksOoxmlTransform`), but Typst gets nothing. The agreed
direction (@cscheid on the issue, and in the triage request) is a Typst-gated transform
that turns each mark into a call to a Typst function defined in our template's
definitions, so the appearance is controlled in one place and users can restyle the marks
without writing Lua filters.

## Reproduction

Fixture: `repro.qmd` (the reporter's example plus a highlight and a `::: --` block).

```bash
# claude-notes/ is itself a Quarto project, so render a copy elsewhere
cp repro.qmd $SCRATCH/ && cd $SCRATCH
q2 render repro.qmd --to typst -M keep-typ:true
```

Observed (`repro-output-body.typ`, and `pdftotext repro.pdf`):

```
Deze zin is toegevoegd. Deze zin is verwijderd. Klopt dit getal?
Inline highlight: highlighted text.
#block[
A deleted paragraph (block form).
```

Expected: each mark visibly distinguished, e.g. insertions underlined/colored,
deletions struck through, highlights with a background fill, comments set apart
(colored inline box or margin note) rather than mixed into the prose.

## Localization

- The qmd reader desugars the marks into spans and divs whose **first class** is
  `quarto-insert` / `quarto-delete` / `quarto-highlight` / `quarto-edit-comment`
  (constants and `MarkKind` in `crates/quarto-core/src/editorial_marks.rs:28-67`).
- Nothing rewrites those for Typst. Pandoc's typst writer emits a Span's content
  unstyled, and a Div as `#block[...]`, which is exactly what the output shows.
- Model to copy: `crates/quarto-core/src/transforms/editorial_marks_ooxml.rs`
  (`EditorialMarksOoxmlTransform`). It runs in the Finalization phase, gates itself on
  `ctx.format.identifier` (docx/pptx), walks top-down over body and metadata, and handles
  block marks, I4 commented ranges (`editorial_marks::commented_range`) and attribution
  stamping (`stamp`, author/date from `AttributionData`).
- Registration: `crates/quarto-core/src/pipeline.rs:2770` (push), and the bucket table
  at `pipeline.rs:3246` (`("editorial-marks-ooxml", Bucket::B3)`) plus the per-profile
  expectation lists around `pipeline.rs:6907/7009/7157/7386`. A new transform needs the
  same entries.
- Typst template: `resources/pandoc-filters/typst-template/definitions.typ` (included by
  `template.typ` **before** `$header-includes$`, so a user's `include-in-header` can
  re-`#let` the functions to override them).

## Proposed fix shape

1. **Prelude functions** in `definitions.typ`, e.g.

   ```typst
   #let quarto-insert(author: none, date: none, body) = underline(text(fill: rgb("#1a7f37"), body))
   #let quarto-delete(author: none, date: none, body) = strike(text(fill: rgb("#cf222e"), body))
   #let quarto-highlight(body) = highlight(body)
   #let quarto-edit-comment(author: none, date: none, body) = box(fill: ..., inset: ..., text(size: 0.85em, body))
   ```

   plus block variants (or one function per kind that also accepts block content).
   Appearance lives **only** here. Prefer solid colors (same reasoning as
   `.claude/rules/hub-client-theme.md`).
2. **`EditorialMarksTypstTransform`** (new file in `crates/quarto-core/src/transforms/`),
   gated on `FormatIdentifier::Typst`: replace each mark Span with
   `RawInline("typst", "#quarto-insert(author: …)[")` + converted content +
   `RawInline("typst", "]")`, and each mark Div with the matching `RawBlock` pair. Reuse
   `MarkKind` and the attribution lookup. Typst-escape any attribute strings.
3. Tests: unit tests in the transform module for each kind plus block forms, nesting
   (a mark inside a comment, highlight in a deletion), marks in metadata (title), and a
   rendered-`.typ` integration check. `pipeline.rs` profile/bucket tests need updating.
4. Docs: the editorial-marks section of `docs/guides/authoring/markdown-basics.qmd`
   should explain how marks look in PDF/Typst and how to override the functions.

## Open questions — resolved during triage

- *Should the fix drop/apply marks instead of showing them?* The maintainer's direction
  on the issue is to show them visually for now ("add some simple visual annotations
  before we settle on a proper solution"). An accept/reject mode is a separate design
  question and isn't needed to fix the silent-misrender bug.
- *Do the Lua filters in the Pandoc stage already handle these classes?* No: the
  rendered `.typ` shows the content unstyled, so nothing downstream touches them.
- *Can users override functions defined in `definitions.typ`?* Yes, by load order:
  `template.typ` splices `definitions.typ` before `header-includes`, and a later
  `#let` shadows an earlier one for the rest of the document.

## Outcome / recommended next step

Filed **bd-7aj5b9ns** with the fix scope above.

## Verification commands used

```bash
gh issue view 1102 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build    # see note below
q2 render repro.qmd --to typst -M keep-typ:true
pdftotext repro.pdf -
```

**Pre-flight note:** `cargo xtask verify --skip-hub-build` on `main` @ `09b740e81` failed
two R-gated knitr tests (`engine::knitr::capture_tests::capture_classifies_pieces`,
`capture_include_graphics_is_an_image_paths_piece`). The cause is local: the R `png`
package isn't installed, so knitr's `raster_dpi_width` returns NULL (`widths: [None]`).
This is unrelated to this issue.

## Cross-references

- `crates/quarto-core/src/editorial_marks.rs` (shared vocabulary)
- `crates/quarto-core/src/transforms/editorial_marks_ooxml.rs` (docx/pptx counterpart)
- `docs/guides/authoring/markdown-basics.qmd` (editorial-marks syntax docs)
- `.claude/rules/hub-client-theme.md` (solid-color guidance, by analogy)
