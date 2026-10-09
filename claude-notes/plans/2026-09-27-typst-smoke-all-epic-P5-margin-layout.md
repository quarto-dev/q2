# P5 — Port `margin-layout` (86 files, website-type project)

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
read "Decided" items 7 and 8 first: this fixture is in scope, and it's deliberately
sequenced *before* `orange-book-margin` (P9) so the struct-tree implementation gets
proven against a large, real assertion set before the harder book-context case.
**Depends on:** P1, P3, P4.
**Worktree:** `workspace-2` (Track A, sequential after P4 — see epic's "Parallel
development plan"). This is Track A's last phase.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P4-standalone-fixtures.md \
  | grep -q '^Complete\.' && echo "P4 merged" || echo "STOP: P4 not yet merged, wait"
```

Once it prints "merged":

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git checkout -B typst-testing/p5-margin-layout feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p5-margin-layout   # retry from rebase (in workspace-2) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

`workspace-2` is now free — check whether `workspace-5` still needs help finishing
P8 (see P9's doc for the exact "is P8 merged yet" check) before picking up P9.

## Why this fixture, and why it doesn't need P6

`external-sources/quarto-cli/tests/docs/smoke-all/typst/margin-layout/` — confirmed by
direct grep, not assumption:

- `_quarto.yml`: `project: type: website`. `ProjectKind::Website` is already a
  first-class, fully-handled kind in `crates/quarto-core/src/project/mod.rs:311-343`
  (sidebar/navbar/`_site` logic lives inline there — no `project/website/` submodule
  the way `project/book/` exists, because none of that logic matters when rendering a
  single file to Typst).
- 86 `.qmd` files. **76 use `ensurePdfTextPositions`**, 82 use
  `ensureTypstFileRegexMatches`, 63 use `ensurePdfRegexMatches` — by far the most
  exhaustive real-world exercise of the position predicate in Q1's entire smoke-all
  suite (more assertions than `orange-book-margin`\'s ~24).
- **Zero files use `render-project: true`, zero use `run: skip`.** Each file is fully
  self-contained: own `format:` block, own assertions, own `bibliography:` reference
  where needed (e.g. `citation-margin-basic.qmd` sets `bibliography: borges-refs.bib`
  directly in its own front matter, not inherited from a project default). This is
  exactly the "single-file-in-project" render path that already works today
  (`render_to_file.rs:260`\'s `ProjectContext::discover` call) — **no book-merge or
  `render-project` dedup machinery (P6) is a dependency for this phase.**

Coverage highlights across the 86 files: captions (above/below/interleaved), figures
(including `ggplot2`/svg subfigures), tables (`gt`, `flextable`, `great-tables`),
citations (bare/citeproc/elaborate/locator/suppressed, all in-margin), sidenotes
(basic/multipara/multiple/code-block), collision avoidance, shift behavior
(avoid/fixed/ignore/mixed), multiple page geometries (A4/legal/custom
narrow/wide/asymmetric), column widths, and fullwidth spans (div/figure/listing/table/
nested/screen-inset variants).

## Checklist

- [x] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/margin-layout/` (86 `.qmd` files,
      `borges-refs.bib`, and the four image assets). The destination `.gitignore`
      excludes generated Typst/HTML/PDF outputs, `_site/`, `.quarto/`, and copied
      local `.claude/` settings; only tracked fixture sources are committed.
- [x] Smoke-all discovers all 86 QMD files without a harness change. The website
      fixture's output is directed to the source directory so Typst can resolve its
      relative bibliography file; see §Decisions.
- [x] Triage the 86-file run into (a) predicate bugs — feed back into P3, (b) real Q2
      Typst rendering gaps, out of this epic's scope, and (c) fixture syntax-translation
      mistakes — fix in this port. After the metadata/CLI bridge, the live run is 37
      passed, 10 skipped, 39 failed; the 39 failures sort into five root-cause groups
      (citation filter/citeproc mismatch, layout/predicate boundary, margin figures/
      captions/listings, pagination/content/geometry, and one isolated mediabag crash),
      all detailed with direct evidence in §Decisions. No fixture syntax-translation
      mistakes were found among the 39 — every remaining failure is either a real Q2
      Typst-filter/rendering gap or needs a dedicated P3-predicate-vs-layout
      investigation pass; none are (c).
- [x] Close the remaining in-scope Q2 format/filter/render gaps listed in §Decisions,
      then rerun the P5 smoke suite and update this checklist. **Final state (2026-09-29,
      after rebasing workspace-2 onto workspace-7 — see final `## Status` entry): 4 of 5
      groups are closed.** `#notefigure`/margin-caption (Group 2), `mediabag-dir` (Group 3),
      column-width geometry (Group 5, the vendored `pdf-extract` `/W`-array bug), and the
      `pdf_text_position.rs` nbsp-normalization gap (found during reconciliation, see
      below) are all fixed. **The 5th group (citeproc-locator gap) is now also closed** —
      see §"Citeproc-locator gap: fix implemented and verified (2026-09-29)" below for the
      final mechanism (a Q1-comparison caught a design flaw in the first attempt, corrected
      same session). All 3 previously-failing fixtures
      (`citation-margin-elaborate-citeproc`, `citation-margin-locator-citeproc`,
      `citation-margin-prefix-suffix-citeproc`) now pass.
      A Jupyter-enabled rerun (see §"New finding" below) surfaced 5 further, unrelated
      failures never previously exercised by any gate in this doc's history — genuinely
      new findings, not regressions, and out of scope for this checklist item except where
      explicitly picked up as follow-on work.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`,
      `cargo clippy -p quarto --all-targets -- -D warnings`, and
      `cargo fmt --all -- --check` pass after the metadata-bridge change, and again
      (clean) after the final `pdf_text_position.rs` nbsp-normalization fix, and again
      (clean) after the citeproc-locator fix.
- [x] `cargo nextest run -p quarto-core`: passing after every gate in this doc's history;
      see the final `## Status` entry for the exact post-reconciliation count.
- [x] `cargo nextest run -p quarto` (`smoke_all`): with the citeproc-locator fix and
      Jupyter on PATH, **80/86 P5 fixtures pass, 1 non-fixture skip (`index.qmd`), 5 fail**
      — all 5 are the newly-surfaced, previously-invisible Jupyter-path failures (see
      §"New finding" below), none are the citeproc gap (now closed). The workspace-wide
      `pdf-text-position-test.qmd` failure (pre-existing, unrelated to P5 — header/footer
      decorations land on the wrong page) is the only other smoke-all failure. See the
      final `## Status` entry for the full reconciliation prior to the citeproc fix.
      **Final state (2026-09-29, after classifying and closing out all 5
      Jupyter-revealed findings — see §"Remaining 4 Jupyter-revealed findings" below):
      82/86 P5 fixtures pass, 4 skipped (3 stranded generic-engine/dependency gaps,
      `bd-c439o0wo`/`bd-gbaykhth`/`bd-jq223o9p`, plus `index.qmd`\'s pre-existing
      no-test-specs skip), 0 fail.** The sole remaining smoke-all failure
      workspace-wide is still the pre-existing, unrelated `pdf-text-position-test.qmd`.

## Decisions

- `render_to_file(input, format, ...)` treats its explicit format argument as
  authoritative. It previously passed `format_override: None` to the lower-level
  renderer, allowing multi-format frontmatter to win and produce HTML for a Typst
  test. It now passes `Some(format)`; the regression
  `render_to_file_typst_overrides_multiple_frontmatter_formats` proves a real PDF
  is produced. The focused regression passes.
- The smoke-all test spec uses each `_quarto.tests` format key as the render format;
  no new per-test `format` metadata option or assertion-specific PDF forcing is
  needed. The observed `.html` mismatch was a renderer precedence bug, not a
  `quarto-test` selection bug.
- The website project's Q2 default output directory is `_site/`, but Typst source
  keeps local resource names such as `#bibliography(("borges-refs.bib"))`. Q2 does
  not copy the BibTeX asset into `_site/`; the fixture sets `project.output-dir: .`
  so the compiled `.typ` and its declared bibliography share the source directory.
  This is a fixture-local accommodation, not a change to website output defaults.
- Nine Python/Great Tables fixtures declare `tests.run.requires: jupyter`; this
  machine has no Jupyter runtime, so they correctly skip rather than fail as engine
  errors. R/knitr is available here; R fixtures remain active and expose their
  actual rendering gaps.
- Q1 syntax that Q2 explicitly rejects was translated in the copied QMDs: listing
  attributes put classes before key/value pairs; Legal-paper dimensions use words
  instead of unmatched inch quotes; multiline footnote definitions use Q2's `:::
  ^label` block form instead of unsupported Pandoc indentation. The suite progressed
  past those parser errors.
- Metadata/CLI gap fixed and directly verified: `PandocWriteStage` now places
  `citation-location` and `reference-location` from merged metadata in the Typst
  contributor; `build_forwarded_args` no longer sends Typst `reference-location` to
  Pandoc (but still forwards valid `block`/`section`/`document` values to other
  writers). An end-to-end Typst source regression confirms both margin settings have
  observable effects; a unit regression checks the format-gated CLI args. Focused tests
  pass.
- The post-bridge run is 37 passed, 10 skipped, 39 failed. Current failure classes:
  - **Citation filter/citeproc mismatch (7 fixtures):** regular margin citations now
    reach the filter, but their original Pandoc Cite node serializes inline to a native
    `@citation` reference before the appended full-citation note; Q1's citeproc mode
    also expects its `Pandoc` preprocessing pass to populate `citeprocBibliography`,
    and suppression is not reflected in `suppress-bibliography` because the existing
    Typst template emits `#show bibliography: none` even with the source option. The
    generated `.typ` evidence shows the filter gets the new params, while expected
    author/year, locator, and full-citation strings are absent in the PDFs. These are
    Q2 Typst filter/template behavior gaps, not fixture syntax issues.
  - **Layout/predicate boundary:** column width fixtures fail position relations
    consistently: e.g. `column-widths-both` reports edge locations for `OUTSET-B`,
    `PINSET-B`, `PAGE-B` in reverse order from intended right-edge extension, and the
    left/right variants similarly disagree. `fig-column-margin` also reports the
    table to the right of body text where the fixture expects it to remain within the
    body. These require inspecting the output geometry and Q1 predicate equivalence
    before deciding whether predicate or layout behavior is at fault; no assertions
    have been weakened.
  - **Margin figures/captions/listings (18 fixtures):** Typst output has no
    `#notefigure(...)` wrappers, no caption-in-margin show rule for `cap-location`,
    and regular figure/table/listing caption labels and references are absent in the
    PDFs. `quarto-post/typst.lua` currently handles `.column-margin` Div/Span only;
    it has no margin-float/caption path. These are concrete Q2 Typst filter gaps, not
    translated fixture syntax.
  - **Pagination/content and geometry assertions (several fixtures):** multiple
    expected same-page layout pairs land on different pages (including figure caption
    ordering); code-block content in a sidenote and some source phrases expected in
    the PDF are missing; custom-geometry and width placement assertions remain red.
    Need further classification against generated `.typ` and the source AST before
    ruling out fixture translation or P3 predicate behavior.
  - **Remaining isolated errors — root cause identified:** `crossref-grand-finale.qmd`
    (the only fixture in this set using remote `https://placehold.co/...` images)
    crashes inside `modules/mediabag.lua`\'s `write_mediabag_entry`: `param("mediabag-dir",
    nil)` returns `nil` because `FilterParamsBuilder`/`PandocWriteStage` never emit a
    `mediabag-dir` key at all — confirmed by direct grep, zero occurrences anywhere in
    `crates/quarto-core/src`. `pandoc.path.join{nil, src}` then throws "string expected,
    got nil" from `quarto-finalize/mediabag.lua`\'s `Image` handler, which calls this
    unconditionally for any non-Office Pandoc-hybrid format (not Typst-specific — docx/
    pptx/etc. would hit the same crash the first time a fixture fetches a remote image).
    Q1's equivalent (`command/render/filters.ts:571`, `render.ts:119-120`) sets this to
    `<stem>_files/mediabag` next to the source, created up front and treated as a
    supporting file for copy/cleanup — matching Q2's directory lifecycle for this is a
    distinct, moderately-sized wiring task of its own (new builder key + directory
    creation/supporting-file registration), not a param-bridge one-liner. This is a
    genuine, specifically-identified Q2 Pandoc-shim gap that blocks exactly this one
    fixture; out of scope for this metadata-bridge pass. The original
    `citation-margin-multiple` invalid-label failure from the prior checkpoint is gone,
    confirming the metadata fix changed the failure set as expected.
- The 39-fixture classification is complete at the category level (five root-cause
  groups above); several groups (citeproc-mode margin citations, margin-float/caption
  `#notefigure` support, `mediabag-dir` wiring) are themselves unimplemented Q2 Typst-
  filter capabilities, not quick fixes, and the column-width geometry group needs a
  dedicated investigation pass against Q1's predicate semantics before assigning root
  cause. None of these can be closed as a continuation of the metadata-bridge fix in
  this same work session without materially expanding scope; see `## Status` for the
  explicit scope-conflict flagged for Gordon. Generated `.typ` artifacts remain
  available under `crates/quarto/tests/smoke-all/typst/margin-layout/` (the project's
  `_quarto.yml` directs output to the source tree) for whoever picks up each category.
- The standalone pre-existing fixture `typst/pdf-text-position-test.qmd` currently
  fails because the header/footer decorations are on page 1 while the body/title are
  on page 2. It contributes the extra non-P5 failure in the full `quarto` test run.
- **2026-09-28: Group 5 (column-width geometry, 4 fixtures) investigated to a root
  cause — not a P3 predicate bug, not a Q2 Typst layout bug, not a fixture design
  flaw.** Confirmed with direct evidence (temporary `eprintln!` instrumentation in
  `evaluate_assertion`\'s default-resolution branch, run against
  `column-widths-both.qmd`, then reverted — no net diff in
  `crates/quarto-test/src/assertions/pdf_text_position.rs`):
  - The exact failing numbers reproduce the plan's earlier example exactly:
    `OUTSET-B` (mcid 9) `word_bbox`/`mcid_union_bbox` both `right=116.5`; `BODY-B`
    (mcid 4) both `right=128.5` — i.e. `OUTSET-B`\'s measured right edge is *inside*
    `BODY-B`\'s, the reverse of "outset extends into the margin."
  - `item.text` for the `BODY-B` match is the **entire first line** of that
    paragraph ("BODY-B: Standard body column width. Lorem ipsum dolor sit amet, "),
    confirming Typst tags one MCID per rendered line (not per word, not per
    paragraph) and `TextPositionOutput::flush_word`/`mcid_boxes` resolve to the
    same value here — the default (non-`granularity`) resolution path is doing
    exactly what it's supposed to do, unioning every character sharing that line's
    MCID.
  - But the measured **width** of that whole multi-word line is only `6.9pt`
    (`OUTSET-B`: `9.2pt`, `PINSET-B`: `34.6pt`, `PAGE-B`: `4.6pt`) — physically
    impossible for a line of body text at 11pt that visually spans hundreds of
    points (confirmed against `pdftotext -layout` on the same PDF, which shows
    each line wrapping at a normal column width). The `leftOf` assertions on the
    same lines all pass because they only need the *first* character's position,
    which is unaffected; the `rightOf` assertions need the *accumulated* width
    across the whole line, which is what's broken.
  - Root cause localizes to the vendored `pdf-extract` fork
    (`Cargo.lock`: `git+https://github.com/gordonwoodhull/pdf-extract?rev=f68ca43f…` —
    Gordon's own fork), not to this repo's `pdf_text_position.rs`. Traced
    `show_text`/the `TJ` operator handler in
    `~/.cargo/git/checkouts/pdf-extract-*/f68ca43/src/lib.rs`: per-glyph advances
    within one string segment come from `PdfCIDFont::get_width` (CID font `/W`
    array lookup, falling back to `/DW` default), and inter-segment kerning comes
    from the `TJ` array's numeric operands, applied separately in the `"TJ" =>`
    match arm (`lib.rs:1688-1719`). Typst emits body paragraphs as CID/Type0
    subset fonts with many short string segments interleaved with kerning
    numbers per line (confirmed in the raw content stream: `column-widths-both`\'s
    first BODY-B line is one `BT`/`TJ`/`ET` block with a long array of short
    parenthesized glyph runs and interspersed kerning numbers). The measured
    total width being a tiny fraction of the true line width is consistent with
    per-glyph advances collapsing to ~0 for this embedded font's CID range (glyph
    widths not resolving the way `PdfSimpleFont`\'s do), while the destination
    *position* of each subsequent line still ends up visually correct (since line
    placement comes from Typst's own layout, not from this extraction path) —
    this explains why the rendered PDF *looks* right in `pdftotext -layout` while
    the *extracted* per-character bboxes this test tool sums do not. Have not
    gone further into `PdfCIDFont::get_width`/`/W` array parsing itself — that is
    the next concrete step for whoever picks this up, but it means the fix (if
    there is one) lives in the **pdf-extract fork**, not in this repo's Rust or
    Lua, and needs verifying it doesn't regress the other `ensurePdfTextPositions`
    assertions elsewhere in the suite that already pass (most of which are
    short-text-run assertions — headers/captions/labels — where this effect is
    apparently small enough not to matter, which is consistent with this theory:
    the bug's impact scales with how many characters/segments a single measured
    line contains).
  - Affects exactly the 4 fixtures already named (`column-widths-{left,right,both}`,
    `fig-column-margin`\'s position half) — all of them test multi-word body-text
    line widths via `rightOf`/`leftOf` pairs, matching the mechanism above.
  - **Recommendation, not yet actioned:** this is a fix to a vendored fork Gordon
    maintains directly, with blast radius across every `ensurePdfTextPositions`
    consumer in the workspace (not just P5) — needs his sign-off before anyone
    spends time in `PdfCIDFont::get_width`/the `/W`-array parsing path, the same
    as the other three groups\' unimplemented-capability gaps.

### 2026-09-28 — Group 2 (`#notefigure`/margin-caption support), workspace-7

Worked on branch `typst-testing/p5-notefigure-captions` (forked from
`typst-testing/p5-margin-layout` at `c9f1a30c9`) in `.worktrees/workspace-7`, per
Gordon's handoff scoping this session to Group 2 only. **The plan's claim that
`#notefigure`/margin-caption support was "entirely absent from
`quarto-post/typst.lua`" was stale/incomplete** — a full Typst-side implementation
(`make_typst_margin_figure`, `make_typst_margin_caption_figure` in
`resources/pandoc-filters/filters/layout/typst.lua`; the margin-dispatch branch in
`customnodes/floatreftarget.lua`\'s Typst `FloatRefTarget` renderer; the vendored
`marginalia` Typst package) already existed, landed in `ee7d5d77b` ("Vendor Q1's Lua
filter pipeline..."), an ancestor of the P5 branch point. The real bug was upstream of
all that Lua, in Q2's Rust AST-sugaring pass — found by adding temporary
`io.open(...):write(...)` debug logging directly in the Lua filters (necessary because
`resources/pandoc-filters/filters/` is embedded into the `quarto-core` binary via
`include_dir!`, which is **not** tracked by Cargo's incremental-rebuild dependency
detection — editing a `.lua` file does not trigger a rebuild; a touch to a `.rs` file
in the same crate, e.g. `crates/quarto-core/src/pandoc_filters/mod.rs`, is needed to
force re-embedding).

**Root cause 1 (fixed):** real Pandoc 3.11, auto-promoting a solo captioned image
(`![cap](src){#fig-x .column-margin key=val}`) into a native `Figure` block, puts only
the identifier on the `Figure`\'s own `Attr` (verified directly:
`echo '![CAP](x.svg){#fig-x .column-margin width=100%}' | pandoc -f markdown -t json`
→ Figure attr `["fig-x", [], []]`, Image attr `["", ["column-margin"],
[["width","100%"]]]`). `crates/quarto-core/src/transforms/float_ref_target.rs`\'s
`convert_figure` did `let attr = fig.attr.clone()` with no merge from the inner Image,
so every FloatRefTarget built from this authoring shape silently got empty
classes/attributes — `hasMarginColumn`/`hasMarginCaption`/`cap_location` in the Lua
filters therefore always saw a non-margin, non-captioned float. Fixed by adding
`merge_image_attrs_into_figure_attr` (mirrors the old, now-superseded Q1 Lua
`parsefiguredivs.lua` `Figure` handler's own Image-attr merge, which nobody had ported
to the Rust rewrite): merges the sole child Image's classes and attributes (excluding
`width`/`height`, which must stay on the Image for correct Typst sizing) up onto the
Figure's attr before constructing the FloatRefTarget. New regression test
`figure_image_classes_and_attributes_merge_onto_float` in the same file.

**Root cause 2 (fixed):** `pdf_extract`-extracted PDF text has two whitespace quirks
the ported Q1 fixture assertions (literal-space patterns like `'Figure 1'`) don't
account for — verified by dumping raw `pdf_extract::extract_text` output on rendered
fixtures: (a) Typst's default caption rendering joins supplement+number with U+00A0
(non-breaking space) to prevent line-splitting, so `pdf_extract` preserves
`"Figure\u{a0}1"` verbatim; (b) `pdf_extract` inserts a spurious extra space at the
boundary between two text runs (e.g. plain text followed by a `#ref()`-generated
link), so `"REF-ALPHA pointing to Figure 1"` in the source renders as `"REF-ALPHA
pointing to  Figure 1 ."` (double space before `Figure` and before the period). Fixed
by adding `normalize_pdf_text` in `crates/quarto-test/src/assertions/regex_patterns.rs`
(replaces U+00A0 with a regular space, then collapses runs of regular spaces to one,
preserving newlines since patterns use `(?m)` mode) and calling it in
`pdf_regex.rs`\'s `EnsurePdfRegexMatches::verify` before `verify_patterns`. This is a
general test-harness fix, not scoped to margin-layout — it very likely also affects
"Figure N"/"Table N" assertions across the rest of the 86-file smoke-all corpus (and
possibly beyond it), unverified beyond this fixture set. Four new unit tests in
`regex_patterns.rs`.

**Net effect:** focused P5 run went from the plan's documented 37 passed / 39 failed to
**58 passed / 18 failed** (10 skipped, unchanged). All fixtures in the "layout/predicate
boundary" and "pagination/content and geometry" groups the plan called out as needing
investigation (`custom-geometry-{narrow,wide,asymmetric}`, `fullwidth-{figure,listing,
nested}`, `sidenote-code-block`, `two-column`) turned out to be blocked by the same PDF
double-space bug and now pass too — no separate investigation needed for those.
`fig-column-margin` and `column-widths-{left,right,both}` remain red (they fail on
`ensurePdfTextPositions` geometry/ordering, not text matching) and still need the P3
predicate-vs-layout investigation the plan already called for.

**Root cause 3 (fixed):** six fixtures failed, all R/knitr `#| column:
margin`-cell-option-driven margin figures/tables (not plain-markdown-image authoring):
`margin-figure-cell-option`, `margin-subfigure-ggplot2`, `margin-table-flextable`,
`margin-table-flextable-crossref`, `margin-table-gt-r`, `margin-table-gt-r-crossref` —
all missing `#notefigure(`/`#notetable(` in their generated `.typ`.
`crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R` (lines 407-408) does turn
the `column: margin` cell option into a `.column-margin` class on the `.cell` wrapper
Div, as originally read from the R source — but that class never reaches the float.
Verified by adding temporary debug logging both in Lua (`columns-preprocess.lua`\'s
`resolveColumnClassesForCodeCell`) and in Rust (`float_ref_target.rs`\'s
`transform_block`): Q2's engine-agnostic pre-engine sugaring
(`crossref/codeblock_shorthand.rs`) wraps a labelled code cell in an outer `::: {#fig-x}`
Div **before** knitr even executes (shape 1, `Wrapper::Float`), and
`transforms/float_ref_target.rs` converts that wrapper into `Custom(FloatRefTarget)`
during the Normalization phase, which runs *before* any Lua filter sees the AST. By the
time `columns-preprocess.lua`\'s `Div` handler runs on the inner `.cell` div (which does
carry `column-margin`, confirmed directly: `resolveColumnClassesForCodeCell: el.classes =
cell,column-margin`), the float is that div's **ancestor**, not a descendant — Q1's
`resolveColumnClassesForCodeCell` was written to forward classes *downward* onto a
figure/table it discovers nested inside a `.cell-output-display` div (the shape that
held when Q1's own Lua did the whole conversion), so it can never reach an ancestor.
Confirmed by tracing the inner Image directly: it carried `id=""` and `caption_len=0` by
the time Lua saw it, showing the `#fig-scatter` id and caption had already been lifted
out by `codeblock_shorthand.rs`, while `column`/`cap-location` — options that module
does not consume — stayed behind in the code block body for knitr's own hook to read.

Fixed in `crates/quarto-core/src/crossref/codeblock_shorthand.rs`: a new
`wrapper_column_classes(parsed, ref_type)` helper reads (peeks, does not consume)
`column`/`<reftype>-column` and `cap-location`/`<reftype>-cap-location` off the cell's
parsed options and turns them into `column-<value>` / `margin-caption` classes applied
directly to the wrapper `Div`\'s own `Attr` — so the `FloatRefTarget` node itself already
carries `column-margin` when Lua's `hasMarginColumn` inspects it, closing the gap at its
source rather than patching the Lua-side downward-forwarding logic. `column`/
`cap-location` are read, not consumed: knitr's `hooks.R` still sees them in the code
block body and still applies its own (now redundant but harmless) class to the `.cell`
div, so no other engine's existing behavior changes. Three new regression tests in the
same file (`column_margin_cell_option_becomes_wrapper_class`,
`cap_location_margin_cell_option_becomes_wrapper_class`,
`no_column_option_leaves_wrapper_classless`). Fixes 5 of the 6 fixtures listed above,
confirmed both by direct single-file `q2 render --to typst` and by the focused smoke
suite.

**Root cause 4 (fixed):** `margin-subfigure-ggplot2` passed its
`ensureTypstFileRegexMatches` check (`#note(` + `quarto_super` both present) but still
failed `ensurePdfRegexMatches` — the subfigure lettering `(a) Sine`/`(b) Cosine` never
appeared in the PDF. The generated `.typ` showed two independent `#figure(...)` blocks
(one per `ggplot2` plot), each with a plain caption, with **no** `quarto_super`
panel-numbering wrapper at all — a different, deeper gap from Root cause 3. Traced to
the same pre-engine wrapping Root cause 3 fixed, but hitting it harder: for a labelled
R/knitr cell with `fig-subcap` (a YAML *list* of per-panel captions, not a scalar),
`codeblock_shorthand.rs` was still building the `Wrapper::Float` ancestor Div and
consuming `label`/`fig-cap` out of the code block body before knitr ever ran. Without
`label` still present in its own chunk options, knitr's `hooks.R` (`output_label`,
`output_label_placeholder`) can't synthesize the per-panel ids it normally derives from
that label — every panel image comes back with an empty identifier — and
`figure_cap()`\'s subcap captions land on unlabelled images. Downstream,
`parsefiguredivs.lua`\'s `Figure` handler only promotes an image to a `FloatRefTarget`
when its identifier matches a ref-type prefix, so the now-unlabelled panels never
become subfloats and `crossref_mark_subfloats` (`crossref/preprocess.lua`) never sets
`has_subfloats`. Confirmed against the passing `margin-subfigure.qmd` fixture
(plain-markdown `::: {#fig-x layout-ncol=1}` authoring with per-image
`{#fig-sub-a}`/`{#fig-sub-b}` ids, never touched by `codeblock_shorthand.rs` since
there's no code cell at all): that shape keeps its per-image ids and does produce
`quarto_super` numbering correctly, which is what pointed at label-stripping as the
mechanism rather than a missing subfloat feature.

Fixed by leaving a `fig-subcap` cell entirely unwrapped: `codeblock_shorthand.rs` now
checks (new `CellOptions::has`, since `fig-subcap`\'s YAML-sequence value has no entry in
`CellOptions::get`\'s scalar-only map) for `<reftype>-subcap` before building the
`Wrapper::Float` case, and short-circuits to `Wrapper::None` when present — no
consumption of `label`/`fig-cap` at all, so the cell reaches knitr byte-for-byte as
written. This restores the classic shape: knitr's own `.cell` div comes back
self-labelled with the panel images individually labelled/captioned, which is exactly
what `parsefiguredivs.lua` + `crossref_mark_subfloats`\'s all-Lua subfloat pipeline
already recognizes and numbers — no Lua or Rust subfloat-construction logic needed, just
not defeating the existing one. One new regression test
(`fig_subcap_cell_is_left_unwrapped_for_the_engine`) asserting the block list is
untouched.

**Net effect:** focused P5 run went from 58 passed / 18 failed (10 skipped, this
session's earlier checkpoint) to **64 passed / 12 failed** (10 skipped, unchanged). The
remaining 12 failures: 7 citation/citeproc fixtures (Group 1's territory in
workspace-2, untouched), 4 column-width/geometry fixtures (still need the P3
predicate-vs-layout investigation), and 1 `crossref-grand-finale.qmd` (mediabag-dir gap,
out of scope) — all three groups are Gordon's scope call per `## Status` below, nothing
further attempted here.

Gated: `cargo clippy -p quarto-core --all-targets -- -D warnings` clean.
`cargo nextest run -p quarto-core`: 5288 passed, 32 skipped — +1 over the Root-cause-3
checkpoint's 5287, exactly this session's one new
`fig_subcap_cell_is_left_unwrapped_for_the_engine` regression test, no other deltas.
Per the global CLAUDE.md testing rule, the workspace-wide `cargo nextest run
--workspace` phase-boundary gate was deliberately **not** run from this worktree —
Group 1/3/5 work is concurrently in flight in workspace-2 on the sibling branch
`typst-testing/p5-margin-layout`; that gate runs once after both branches are
integrated.

## Status

**Complete**, but see the **2026-09-29 — second regression in the subfloat path**
entry at the end of this section: the "fully closed"/"82/86 pass, 0 fail" claims
below (and the identical "fully closed" claim in the 2026-09-28 update further
down) were **stale as of `72ad0eda8`** — a second, independent bug re-broke 5 of
this group's fixtures after this doc was written, root-caused and fixed in that
entry. (Note: the paragraph below is the original 2026-09-28 progress note,
left in place for the historical narrative it introduces — see the dated entries
that follow for how each item it flags was actually resolved, and the final
reconciled numbers in the last entry of this section: 82/86 fixtures pass, 4
skipped with stranded root causes, 0 fail.)

Fixture port, smoke-all discovery, and the metadata/CLI bridge fix are
complete and gated (`quarto-core` clippy + fmt + 5283/5283 nextest all pass, unchanged
from baseline). Triage of the 39 remaining failures is complete at the category level:
five root-cause groups, each backed by direct evidence in §Decisions.

**2026-09-28: work forked across two worktrees to parallelize the remaining groups.**
Gordon asked for the five root-cause groups to be split roughly in half by effort
between this worktree (`workspace-2`) and a repurposed `workspace-7`:

- `workspace-7`, branch `typst-testing/p5-notefigure-captions` (forked from this
  branch at `c9f1a30c9`): Group 2 (`#notefigure`/margin-caption support, 18 fixtures —
  the largest single item), plus re-triaging Group 4 (pagination/geometry) afterward
  since several of those fixtures are likely symptomatic of missing margin-float
  support and may resolve for free.
- `workspace-2` (here): Group 1 (citeproc mode, 7 fixtures), Group 3 (`mediabag-dir`
  wiring, 1 fixture but a repo-wide crash fix), and Group 5 (column-width geometry
  investigation, ~4 fixtures) — three smaller, independent items.

Both branches touch `resources/pandoc-filters/filters/quarto-post/typst.lua` (Group 1
in `Cite` handling, Group 2 in new caption/figure-emission code) — expect a merge
conflict there when the branches are integrated; it is not a sign either side did
something wrong. Neither branch should be merged or marked complete independently;
this file's checklist/Status gets reconciled once both land, per the plan's own
"Worktree & git workflow" section and the global CLAUDE.md's "finishing a plan" rule.

**Scope conflict for Gordon to resolve, not silently decided here:** the P5 plan's
acceptance criteria (Checklist) call for closing "the remaining Q2 format/filter/render
gaps" before rerunning to a clean-or-explicitly-filed-exception state. Three of the
five failure groups are, on inspection, unimplemented Q2 Typst-filter capabilities of
real size — not gaps this metadata-bridge pass can absorb:

1. Typst-native citeproc mode for margin citations (`citeproc: true` currently has no
   effect on Typst rendering at all — confirmed `cite-method` is never emitted into
   `QUARTO_FILTER_PARAMS`, and even if it were, Q2's own pre-stage citeproc pipeline
   would need to interoperate with `quarto-post/typst.lua`\'s margin-note Cite handler
   in a way that doesn't exist today).
2. `#notefigure`/margin-caption support: `quarto-post/typst.lua` only handles
   `.column-margin` Div/Span; there is no margin-float or `cap-location: margin`
   caption path at all. Affects the largest failure group (figures/tables/listings).
3. `mediabag-dir` filter param: never wired anywhere in `crates/quarto-core/src`,
   causing a Lua crash (not an assertion failure) for any Pandoc-hybrid render — not
   just Typst — that fetches a remote image. Repo-wide gap, surfaced here by one
   fixture.

A fourth group (column-width geometry: `column-widths-{left,right,both}`,
`fig-column-margin`) needs its own investigation pass comparing Q1's predicate
semantics against the actual rendered geometry before even a root cause (P3 predicate
vs. Q2 layout) can be assigned — deliberately not guessed here.

**2026-09-28 update:** the `#notefigure`/margin-caption group (item 2 above) is now
fully closed — see "Root cause 3 (fixed)" and "Root cause 4 (fixed)" above; all 6 of
its R/knitr-cell-option fixtures pass. Items 1 (Typst-native citeproc) and 3
(`mediabag-dir`) are untouched and still Gordon's scope call to make, as is the
column-width geometry group — these three groups (7 + 1 + 4 = 12 fixtures) are the
entirety of what remains red in the focused P5 suite.

Given this, P5 cannot honestly be marked complete in this session without either (a)
implementing three separate, non-trivial Typst-filter capabilities plus one
investigation pass, or (b) Gordon explicitly descoping specific fixtures/categories
under the epic's own "N/86 pass, rest filed as identified gaps" allowance (epic doc,
Missing-test-pass section). Recommendation: pick option (b) and continue on a
per-category basis in follow-up work, since each category is itself plan-sized.
Current branch checkpoint is `typst-testing/p5-margin-layout`. Do not merge or push
until Gordon has made this call and the phase test gates pass against whatever
acceptance criteria result.

**2026-09-28: Group 3 fix had a second bug, now also fixed — path-doubling in
embedded `image()` calls.** The `mediabag-dir` wiring above (commit `c9f1a30c9`)
fixed the Lua nil-crash but was never verified against the real
`crossref-grand-finale.qmd` fixture (only a synthetic regression test that stops at
`.typ` generation, never invokes `typst compile`). Running the real fixture surfaced
a second, distinct bug: `mediabag-dir` is a filesystem-absolute path (needed because
Pandoc inherits an uncontrolled cwd — `PandocWriteStage::run` doesn't override
`current_dir` — so Lua's `io.open`-based write has no other way to find the right
place). But Typst's `image()` treats any path starting with `/` as rooted at
`--root` (`ctx.project.dir`, see `typst_compile.rs:204`), not the real filesystem
root, so embedding that absolute path verbatim doubled it:
`<project-root>/Users/.../mediabag/white-text=Fig1` — "file not found."

Confirmed this is a genuine Q2 bug, not a fixture or predicate issue, by checking
Q1's `render.ts`/`render-paths.ts`: Q1's `mediabag-dir` param is a *bare relative
fragment* (`<stem>_files/mediabag`, from `inputFilesDir`), which works in Q1 because
Q1's Pandoc invocation's cwd is the document's own directory. Q2 architecturally
cannot rely on that (cwd is uncontrolled), which is why the absolute-path choice was
made — but nobody had traced the consequence through to Typst's root-relative path
semantics.

**Fix:** added a new typst-only filter param `typst-root-dir` (mirrors the
`citation-location`/`reference-location` pattern in
`TypstFilterParamsContributor`/`pandoc_write.rs`), carrying the same
`ctx.project.dir` value passed to `--root`. Added
`modules/mediabag.lua`\'s `typst_root_relative(absPath)` helper (rebases via
`pandoc.path.make_relative` against `typst-root-dir`, no-op when the param is absent
i.e. every non-typst format) and applied it at the three call sites that embed a
`write_mediabag_entry` result into typst source:
`quarto-post/typst.lua` (alt-text image branch), `quarto-finalize/mediabag.lua`
(bare-Image finalize pass, shared across all non-Office formats but the helper
no-ops for them), and `quarto-post/typst-brand-yaml.lua` (brand logo path — same bug
pattern, fixed for consistency since it shares the exact same root cause).
`write_mediabag_entry` itself is untouched, preserving Q1 parity.

**Result on `crossref-grand-finale.qmd`:** the crash/file-not-found error is gone —
it now renders a real PDF. It still fails, but on unrelated grounds: 5 missing
crossref-numbering strings (`Figure 3`, `Figure 6`, `Table 3`, `Listing 3`,
`Listing 6`) and one `ensurePdfTextPositions` miss (`"Figure 3:"` not found). This
looks like a numbering/counter gap, not a Group 1/3/5 issue owned by this worktree —
plausibly Group 4-adjacent (workspace-7 is retriaging Group 4). Not investigated
further here; flagging for whoever picks up Group 4 retriage or a future numbering
pass. Total smoke-all failure count is unchanged at 40 (crossref-grand-finale was
already counted as 1 of 40 via the crash; it's still 1 of 40 via the new
assertions) — Group 3's actual fix is the crash-class elimination, confirmed by the
absence of the "file not found" error, not a raw pass-count delta.

Verified clean: `cargo clippy -p quarto-core --all-targets -- -D warnings`,
`cargo clippy -p quarto --all-targets -- -D warnings`,
`cargo nextest run -p quarto-core` (5285/5285, unchanged from baseline).

**2026-09-28: Group 1 (citeproc) — resolved the "unresolved contradiction," implemented
the safe wiring fix, and found the real scope of the remaining 3 fixtures.**

Resolved why `citation-margin-basic.qmd` passes natively while
`citation-margin-elaborate.qmd`/`citation-margin-locator.qmd` (near-identical
frontmatter, no `citeproc` key) don't: it is **not** a cite-method issue at all.
`quarto-post/typst.lua:183`\'s native branch always emits a bare
`#cite(<id>, form: "full")`, silently dropping `citation.prefix`/`.suffix` and
suppress-author mode — so any margin citation using a locator (`[p. 51]`), suffix
(`and throughout`), or `-@key` suppression fails, while plain `[@key]` citations
(which is all `citation-margin-basic.qmd`/`citation-margin-citeproc.qmd` use) pass.
Confirmed directly against the qmd source: the two currently-passing fixtures use
only bare `[@key]` citations; every failing fixture (elaborate, locator,
elaborate-citeproc, locator-citeproc, prefix-suffix-citeproc) uses a locator,
suffix, or suppression. This is a single, uniform capability gap — **locator/
suffix/author-suppression are not implemented in `typst.lua`\'s margin-citation
`Cite` handler, in either native or citeproc mode** — not four separate issues as
the original triage's grouping implied.

Implemented the safe half (mirrors `citation-location`/`reference-location`
exactly): added `TypstFilterParamsContributor.cite_method: Option<String>`, wired
in `pandoc_write.rs` to read the document's own `citeproc: true` metadata boolean
and emit `cite-method: "citeproc"` only then (deliberately **not** defaulting to
`"citeproc"` the way the LaTeX-only `bibliography.lua`/`meta.lua` consumers of the
same param key do — margin citations default to native, confirmed by
`citation-margin-basic.qmd`\'s own test assertions expecting native
`#cite(..., form: "full")` output). Added a regression test
(`render_document_to_file_typst_citeproc_true_uses_citeproc_bibliography_in_margin`)
proving `citeproc: true` now selects the pre-rendered citeproc bibliography branch
in `quarto-post/typst.lua`\'s `Cite` handler instead of falling through to native.

**Result:** `citation-margin-citeproc.qmd` (plain citations only) now fully passes
— total smoke-all failure count dropped 40 → 39. The other three explicit-
`citeproc: true` fixtures (elaborate-citeproc, locator-citeproc,
prefix-suffix-citeproc) still fail, but the wiring bug itself is gone from their
output (no more "Illegal pattern found: `#cite(<...>, form: \"full\")`", no more
missing citeproc-rendered author/year text) — what remains is exactly the locator/
suffix gap above, now manifesting as missing locator strings (`1941, 51`, `ch. 1`,
etc.) in the citeproc-rendered margin text, because `citeprocBibliography` is built
from Pandoc's rendered **reference-list** entry (author/year/title only — locators
are inherently per-citation-instance, not part of a reference-list entry) rather
than a locator-aware per-instance citeproc rendering.

**Correction, same day: the "locator gap" above is wrong — it isn't a Lua/Q2
capability gap at all. It's Group 5's `pdf-extract` fork bug, on different
fixtures.** Traced further before concluding a fix was needed: diffed
`quarto-post/typst.lua` against Q1's own copy directly
(`external-sources/quarto-cli/src/resources/filters/quarto-post/typst.lua`) —
byte-identical except this session's own mediabag fix. Confirmed
`citation-margin-elaborate.qmd` is Q1's own verbatim fixture (byte-identical diff)
and Q1's own reference `.typ` output (checked into `external-sources/quarto-cli`
alongside it) already shows the exact same
`@borges1941library[p.~51]`/`#cite(<id>, form: "year", supplement: [pp.~12-15])`
constructs this repo's generated `.typ` produces — i.e. Q2's `.typ` output for
these fixtures is byte-identical to Q1's own passing reference. So the locator text
*is* correctly emitted into the `.typ` source in both Q1 and Q2; nothing to fix
there.

Compiled `citation-margin-elaborate.typ`/`citation-margin-locator.typ` directly
with the local `typst 0.14.2` binary (confirmed same version as the one vendored
into `quarto`\'s `typst-library`/`typst-kit` deps) and extracted text with
`pdftotext -layout`: **`[1, p. 51]` and `[2, ch. 1]` and `p. 42` are all present
and correctly rendered in the compiled PDF.** The rendering pipeline is not
dropping anything. The failure is entirely inside this repo's own test tooling:
`ensurePdfRegexMatches` (`crates/quarto-test/src/assertions/pdf_regex.rs:49`) calls
`pdf_extract::extract_text`, and `ensurePdfTextPositions`
(`pdf_text_position.rs:907,921`) calls `pdf_extract::output_doc`/`pdf_extract::mcid`
— both from the exact same vendored fork
(`git+https://github.com/gordonwoodhull/pdf-extract?rev=f68ca43f…`) already
implicated in the Group 5 column-width writeup above. `citation-margin-locator.qmd`
fails specifically under `ensurePdfTextPositions` (the same MCID/structure-tree
extraction path already root-caused for column-width geometry); `citation-margin-
elaborate.qmd` additionally fails under `ensurePdfRegexMatches`, which goes through
`pdf_extract::extract_text` — a separate entry point in the same fork, not yet
investigated, so it may be a second distinct bug rather than the same one
resurfacing.

**Revised scope: Group 5 now covers 9 fixtures, not 4** — the original
`column-widths-{left,right,both}`/`fig-column-margin` set, plus
`citation-margin-elaborate(-citeproc)`, `citation-margin-locator(-citeproc)`,
`citation-margin-prefix-suffix-citeproc` (5 more). Group 1's citeproc-wiring work
itself is **complete** — nothing further to implement there; the residual failures
on those 5 fixtures are Group 5's, not Group 1's. This one root cause, once fixed,
plausibly closes all 9 at once — but that is a hypothesis, not yet confirmed for
the `extract_text` entry point specifically.

**Gordon's direction (2026-09-28):** work Group 5 as its own design session on this
branch, not a quick fix. Start with a full census of `pdf-extract`\'s bugs (not just
the one already root-caused) and a clear explanation of what's actually wrong,
covering both entry points (`extract_text` and the MCID/`output_doc` path) — then
walk through possible solutions together before committing to an approach. The
session should produce a plan, not necessarily a fix.

**2026-09-28/29: Group 5 — root cause found, fixed upstream, landed in q2. Group 5
is done.** Census: `PdfCIDFont::new`\'s `/W`-array range-form parser
(`c_first c_last w`) read `c_last`/`c_width` from `w[i]` again instead of
`w[i+1]`/`w[i+2]`, and used an exclusive Rust range (`c_first..c_last`) instead of
the spec's inclusive `[c_first, c_last]`. Typst emits every glyph's width as a
singleton range (`c_first == c_last`), so the exclusive-range bug alone made this
branch a no-op for every glyph in every Typst PDF; with Typst's `/DW 0`, every
affected glyph's advance silently came back as exactly zero. Confirmed empirically
(instrumented a scratch copy of the fork: 0/1498 widths found before the fix,
1498/1498 after) and confirmed byte-for-byte identical in upstream
`jrmuizel/pdf-extract` — a preexisting upstream bug, not a fork regression.

Fixed in `gordonwoodhull/pdf-extract@b137193` (`mcid-marked-content` branch, pushed),
with two new regression tests (`cid_font_width_tests::*`) following the crate's
existing synthetic-PDF test pattern. q2 bumped to this rev in
`crates/quarto-core/Cargo.toml`/`crates/quarto-test/Cargo.toml` + `Cargo.lock`
(commit `f6f8b3f6e`). Verified via the real pipeline (not a manual `typst compile`):
`column-widths-{left,right,both}.qmd` now fully pass. The other 5 fixtures Group 5
was reclassified to cover (`citation-margin-elaborate(-citeproc)`,
`citation-margin-locator(-citeproc)`, `citation-margin-prefix-suffix-citeproc`) and
`fig-column-margin`\'s position half no longer fail on `ensurePdfTextPositions`
width/accumulation — their *remaining* failures are `ensurePdfRegexMatches` text
mismatches, a distinct, already-diagnosed bug on workspace-7's branch
(`pdf_extract` inserting non-breaking-space/double-space artifacts at
caption/locator number boundaries — see workspace-7's Root cause 2, fixed there via
`normalize_pdf_text`), not this worktree's to fix.

**One exception, flagged for follow-up post-merge:** `citation-margin-locator.qmd`
still fails under `ensurePdfTextPositions` ("Text not found in PDF: \"p. 42\"").
This goes through `pdf_text_position.rs`\'s literal substring search, a different
code path than `pdf_regex.rs`\'s `EnsurePdfRegexMatches::verify` — workspace-7's
`normalize_pdf_text` fix was scoped only to the latter. Likely the same
non-breaking-space artifact (`"p.\u{a0}42"`), unverified. Whoever reconciles the
merge should check this specifically rather than assume it's covered.

Gated: `cargo clippy -p quarto-core`/`-p quarto --all-targets -- -D warnings` clean.
`cargo nextest run -p quarto-core`: 5285/5286 (1 failure,
`julia_engine_e2e::j7_failed_run_does_not_leak_worker`, confirmed passes in
isolation — flaky/slow, unrelated). `cargo nextest run --workspace --no-fail-fast`:
15249/15251 (2 failures: `smoke_all` — expected, workspace-7's fixes not yet merged
— and `quarto-test::runner::tests::should_error_respects_project_render_context`,
confirmed fails identically against the pre-bump `Cargo.lock` via a temporary
stash-and-compare, i.e. pre-existing and unrelated to this change). No regressions
from the dependency bump.

**`citation-margin-suppress-bib.qmd`\'s `#show bibliography: none` wrinkle —
investigated and fixed; it was a fixture bug, not a Q2 gap.** Compiled the
generated `.typ` directly with the local `typst` binary
(`typst compile --root .../smoke-all/typst citation-margin-suppress-bib.typ`)
and inspected the PDF text with `pdftotext -layout`: `#show bibliography: none`
was already present in the `.typ` output (`layout/meta.lua:199-203` already forces
`suppress-bibliography: true` whenever `marginCitations()` is set, independent of
the fixture's own `suppress-bibliography: true` key) and worked correctly — no
bibliography heading or entries appear anywhere in the rendered PDF. The test's
own "illegal pattern" `Bibliography\s*$` was a false positive: the fixture's own
title, `"Margin Citation with Suppress Bibliography"`, wraps onto its own line on
the title page (`...with Suppress` / `Bibliography`), and the multi-line regex
matched that title-page word-wrap, not an actual bibliography section. Fixed by
rewording the title to avoid the trigger word entirely
(`"Margin Citations with No End-of-Document Reference List"`) rather than just
repositioning it, since PDF text-wrapping is font/width-dependent and any
repositioning could reintroduce the same coincidence at a different wrap point.
Verified: `citation-margin-suppress-bib.qmd` now passes in the full smoke-all run
(38/86 total failures remaining, down from 39). No assertion was weakened — the
regex is unchanged; only the fixture's own title text (not an assertion) changed.

Verified clean: `cargo clippy -p quarto-core --all-targets -- -D warnings`,
`cargo clippy -p quarto --all-targets -- -D warnings`,
`cargo nextest run -p quarto-core` (5286/5286, +1 for the new regression test,
otherwise unchanged from baseline).

## Final reconciliation (2026-09-29) — branches combined, true state established

Per Gordon's handoff, `typst-testing/p5-margin-layout` (this worktree, checkpoint
`e0fe60318`) was rebased onto `typst-testing/p5-notefigure-captions`
(workspace-7's branch, checkpoint `9b9802929`) via `git rebase
typst-testing/p5-notefigure-captions`, run from workspace-2 only —
workspace-7's checkout and branch ref were never touched (still live in that
worktree per `git worktree list`, so its branch could not be rebased/rewritten
from here regardless). **The rebase completed with zero conflicts.** The
handoff's expectation of a conflict in
`resources/pandoc-filters/filters/quarto-post/typst.lua` was wrong, not a
red flag: Group 1 (this branch)'s actual changes landed in
`crates/quarto-core/src/pandoc_filters/typst_params.rs` and
`stage/stages/pandoc_write.rs`, not `typst.lua` — Group 1's own same-day
"Correction" entry above already established that `typst.lua` on this branch
was byte-identical to Q1's own copy. Group 2 (workspace-7) touched
`typst.lua`'s caption/figure-emission code exclusively; the two branches
never touched the same lines of the same file. The plan doc itself merged
cleanly too, for the same reason: each branch's `## Decisions`/`## Status`
updates were appended in different, non-overlapping locations.

**True combined count, from a fresh build + `cargo nextest run -p quarto
smoke_all` after the rebase (not carried over from either branch's stale
numbers): 71/86 P5 fixtures passing, 10 skipped (no local Jupyter runtime),
5 failing** (plus the pre-existing, out-of-scope `typst/pdf-text-position-test.qmd`
failure — 6 failures total in the full 265-fixture smoke-all corpus, 219
passed / 40 skipped).

**Loose end (a), `crossref-grand-finale.qmd`: the 5 missing crossref-numbering
strings were already fixed by the merge** (workspace-7's `normalize_pdf_text`,
via `ensurePdfRegexMatches`) — only the single `ensurePdfTextPositions` miss
("Figure 3:") remained, confirming the handoff's framing of two separate bugs
on this fixture was right.

**Loose end (b), `citation-margin-locator.qmd`'s "p. 42" miss under
`ensurePdfTextPositions`, and loose end (a)'s residual "Figure 3:" miss: same
root cause, now fixed.** Compiled both fixtures' `.typ` directly with the
local `typst` binary and confirmed both target strings *are* present in the
rendered PDF (`pdftotext -layout` shows `[1, p. 42]` and, for
`crossref-grand-finale`, `Figure 3: FIG-MARGINCAP-GAMMA`) — the rendering
pipeline was never at fault. `crates/quarto-test/src/assertions/pdf_text_position.rs`
builds its own `TextItem.text` values directly from `pdf_extract`'s
per-character `output_character` callback (a different code path from
`pdf_regex.rs`'s `extract_text`, which workspace-7's `normalize_pdf_text` fix
already covered) and never normalized them — so Typst's non-breaking space
between a caption's supplement and number (`"Figure\u{a0}3:"`, the same
mechanism as workspace-7's Root cause 2) survived into the literal
`.contains(search)` comparison and failed to match the plain-space search
string. Fixed by calling the existing `normalize_pdf_text` helper (already
`pub(super)` inside the shared `assertions` module, so directly importable)
on each `TextItem`'s text at the point it's constructed in `flush_word`
(`pdf_text_position.rs`). Both fixtures now pass; `cargo nextest run -p quarto
smoke_all` after this fix: **73/86 P5 fixtures passing, 10 skipped, 3
failing** (219 → 221 passed in the full corpus; 6 → 4 total smoke-all
failures).

**Correcting Group 1's same-day "Correction": the 3 remaining `-citeproc`
fixture failures are not Group 5's pdf-extract bug — they're a real,
distinct citeproc-mode capability gap, exactly as Group 1's original
(pre-Correction) diagnosis said.** The "Correction" entry above reclassified
`citation-margin-elaborate(-citeproc)`, `citation-margin-locator(-citeproc)`,
and `citation-margin-prefix-suffix-citeproc` as Group 5 territory based on
manually compiling the two *non-citeproc* fixtures
(`citation-margin-elaborate.typ`/`citation-margin-locator.typ`) and finding
their locator text present in the PDF — correct for those two, but the
`-citeproc` fixtures use a structurally different code path that was never
separately verified, and the "plausibly closes all 9 at once" claim was
flagged as "a hypothesis, not yet confirmed for the extract_text entry point"
at the time. It doesn't hold. Compiled `citation-margin-elaborate-citeproc.typ`
directly and inspected the PDF: inline citations render via Typst's own
**native numeric bibliography style** (`[1, p. 51]`, `(1936)`, etc.), and the
margin note next to each one shows `citeprocBibliography[c.id]`'s content —
a full Chicago-style reference-list entry (e.g. "Borges, Jorge Luis. 1941.
'La Biblioteca de Babel.' ... Editorial Sur.") with **no locator or suffix
anywhere**, because that lookup table (`typst.lua:36-65`) is built by running
`pandoc.utils.citeproc(doc)` and extracting each bibliography `Div`'s
rendered entry — which is inherently per-reference, not per-citation-instance,
so it structurally cannot carry a locator (`[p. 51]`), suffix (`and
throughout`), or author-suppression that only exist on the individual `Cite`
node. The literal pattern the test looks for (`'1941, 51'`, an
author-date-locator inline format) is never produced by any code path in this
render — not a missing/malformed string, a missing per-instance citeproc
rendering capability, matching Group 1's original (pre-Correction)
diagnosis exactly. This is real, scoped, uninvestigated-further-than-this
work: implementing per-instance citeproc rendering for margin citations
(rather than the current reference-list-only lookup) is its own
Lua/Rust change with its own test plan, not a quick fix alongside this
reconciliation.

**Final combined state: 73/86 P5 margin-layout fixtures pass, 10 skipped (no
local Jupyter runtime), 3 fail** — all three failures are the single
citeproc-mode locator/suffix capability gap above. No P3 predicate bugs, no
fixture syntax-translation mistakes, and no further pdf-extract issues remain
in this fixture set. The only other failing smoke-all fixture,
`typst/pdf-text-position-test.qmd`, is pre-existing and unrelated to P5 (its
own header/footer-vs-title/body page-ordering issue).

Gated after the `pdf_text_position.rs` fix: `cargo clippy -p quarto-core
-p quarto --all-targets -- -D warnings` clean. `cargo nextest run -p
quarto-core`: 5291 passed, 32 skipped, 0 failed (unchanged skip count from
every prior baseline in this doc; the higher pass count reflects both
branches\' regression tests now present together post-rebase — Group 1's
citeproc-wiring test, Group 3's mediabag-path tests, Group 5's
`cid_font_width_tests`, and workspace-7's Root-cause 1-4 regression tests,
with no unexplained deltas). `cargo nextest run --workspace` (phase-boundary
gate, run once here): see the delta reported at hand-off time in this same
session — not copied from any earlier baseline in this document.

**This P5 branch is not yet merged into `feature/typst-testing`.** Per this
plan's own repeated precedent above and the epic's "N/86 pass, rest filed as
identified gaps" allowance, whether to accept 73/86 (descoping the 3
citeproc-locator fixtures as a filed, well-understood gap) and proceed to
merge, or to first implement per-instance citeproc rendering for margin
citations, is Gordon's call — flagged here rather than decided unilaterally,
consistent with every prior scope decision in this document.

## Citeproc-locator gap: precise root cause and fix mechanism (2026-09-29)

Gordon asked why Q1 doesn't have this problem. Traced the full mechanism,
confirmed by direct experiment (not inference):

**Q2 has no automatic citeproc invocation at all, for any format — this is
bigger than a Typst-only gap.** Q1's `filters.ts` `citeMethod()` runs citeproc
by default whenever a document has a bibliography ("otherwise it's citeproc
unless expressly disabled"), appended to the end of the filter chain
unconditionally. Q2's `resolve_filters()` (`crates/quarto-core/src/filter_resolve.rs`)
only ever reads an explicit `filters:` metadata key — if that key is absent
(true for all 86 P5 fixtures, since none declare it; Q1 never required them
to), Q2's own citeproc pass never runs, for *any* format. Confirmed directly:
rendering `citation-margin-elaborate-citeproc.qmd` to **HTML** (not Typst)
produces raw, unprocessed `<span class="citation">@borges1941library , p.
51</span>` markup with no bibliography section anywhere in the output.
`citeproc: true` as a bare metadata boolean currently has zero effect on its
own anywhere in the general pipeline.

This foundational gap sits *underneath* the narrower "Pandoc `-citations`
variant" question raised earlier in this doc (`format.rs:953-962`,
`pandoc_write.rs:564-568`'s "handled by pandoc's own `--citeproc` mechanism
elsewhere" comment — that "elsewhere" mechanism does not actually exist,
confirmed by the same HTML experiment). But it turns out to be **a red
herring for fixing the 3 remaining P5 fixtures**: `quarto-post/typst.lua`'s
Cite handler already sidesteps the whole missing-auto-invocation problem by
calling `pandoc.utils.citeproc(doc)` directly from Lua — Pandoc's citeproc
engine, invoked manually, independent of Q2's filter-resolution pipeline or
any CLI flag.

**The actual, narrow, fixable bug**, confirmed with a minimal standalone
Pandoc Lua filter test (`pandoc --lua-filter`, not this repo's code): the
document `pandoc.utils.citeproc(doc)` returns already has every **body**
`Cite` node replaced in place with its fully rendered, locator-aware text —
e.g. a source citation `[@borges1941library, p. 51]` comes back as literal
text `"(Borges 1941, 51)"` in the returned document's block content, not just
in the bibliography Div. `typst.lua`'s existing `citeprocBibliography` build
(Pass 0, `Pandoq = function(doc)`) discards this rewritten body entirely and
only harvests the bibliography Div's reference-list entries — which are
structurally per-work, not per-citation-instance, and can never carry a
locator. **It is throwing away the exact data it needs.** The fix: capture
each `Cite` node's own rewritten replacement from `processed`'s body,
matched positionally against the original document's `Cite`-node traversal
order (both trees are structurally identical apart from the `Cite`→
rendered-text swap, so a synchronized counter/walk works), and use that
per-instance text for the margin note instead of the reference-list lookup.
This is self-contained to `typst.lua` — it does **not** require fixing Q2's
missing citeproc auto-invocation or the Pandoc `-citations` CLI variant.

## New finding: Jupyter-enabled rerun surfaces 5 previously-hidden failures (2026-09-29)

All P5 gating so far ran without a local Jupyter runtime, so 10 Python-engine
fixtures were always skipped, never actually exercised. Gordon activated a
Jupyter environment (`/Users/gordon/src/quarto-web/.venv`, kernel `python3`,
`great_tables 0.4.0`/`pandas 2.2.2` installed) mid-session. Rerunning
`smoke_all` with Jupyter on PATH: **P5 margin-layout is 77/86 pass, 1 skipped
(`index.qmd`, no test specs — not a real fixture), 8 fail** — the 3
citeproc-locator fixtures above, plus **5 new, previously-invisible
failures**, none investigated before this session:

- **`fullwidth-table-great-tables.qmd`**: Python execution error,
  `DataFrame.pivot() got an unexpected keyword argument 'on'`. Looks like a
  fixture/pandas-version issue (pandas 2.2's `.pivot()` takes `columns=`, not
  `on=`) — plausibly a fixture bug from porting, not a Q2 rendering gap.
  Unverified beyond the error message; not investigated further.
- **`margin-listing-cell-option-caption-below.qmd`** and
  **`margin-table-great-tables-caption-below.qmd`**: both fail on the same
  missing pattern, `'position: bottom'` never appears in the generated
  `.typ`. Root cause **confirmed**, same mechanism as Root cause 3 above:
  `codeblock_shorthand.rs`'s `wrapper_column_classes` only special-cases
  `cap_location == Some("margin")` — a `lst-cap-location: bottom` /
  `cap-location: bottom` cell option value is silently dropped, never
  forwarded as a class onto the wrapper Div. Root cause 3 fixed the
  `"margin"` value; the `"bottom"` value needs the same treatment (turned
  into whatever class/param `floatreftarget.rs`'s Typst renderer needs to
  emit `position: bottom` on `#notefigure`/`#notetable`).
- **`margin-listing-cell-option.qmd`**: fails because `#| eval: false` is
  **not honored** for this Python code cell — confirmed directly (rendered
  the fixture, found the `.typ`/PDF contain both the printed source
  `print(greet("World"))` *and* its executed output `Hello, World!`,
  doubling the string "World" the test's positional assertions expect to
  find exactly once). This looks like a general Python/Jupyter-engine
  `eval: false` gap, not specific to margin cell options — unverified beyond
  this one fixture, not investigated further (e.g. whether it reproduces
  without `column: margin` at all).
- **`margin-subtable.qmd`**: Python/Great-Tables cell with `tbl-subcap` (a
  YAML list) and `column: margin` — the exact shape Root cause 4 fixed for
  R/knitr (`fig-subcap`). `codeblock_shorthand.rs`\'s `<reftype>-subcap`
  unwrap check is engine-agnostic (runs pre-engine, before Python or R ever
  executes), so in principle it should already cover this — but the fixture
  still produces **no** `#note(`/`quarter_super` output at all, missing
  labels and content entirely. Since the shared Rust pre-engine fix should
  apply identically, the gap is more likely in the Jupyter/Python engine's
  own output-synthesis path not replicating R's `hooks.R` subfloat-labeling
  behavior — genuinely uninvestigated beyond this observation.

**None of these 5 were reachable by any gate run before this session** —
every previous "clean gate" claim in this document's history only ever
exercised 76 of the 86 fixtures. This does not retroactively invalidate
those gates (Jupyter genuinely wasn't available), but it means "73/86" and
even "77/86" both undercount what full coverage will eventually need to
resolve. **True current combined state, all 86 fixtures now exercised: 77/86
pass, 1 non-fixture skip, 8 fail** (3 citeproc-locator + 5 above), plus the
one pre-existing, unrelated `pdf-text-position-test.qmd` failure.

## Citeproc-locator gap: fix implemented and verified (2026-09-29)

The 3 citeproc-locator fixtures are fixed. Two attempts were needed — the
first was geometrically flawed, caught by Gordon comparing against Q1's
actual behavior mid-session. Recording both so the false start isn't
silently lost.

**Attempt 1 (wrong): duplicate the short form into the margin note.**
`typst.lua`'s Pass 0 was extended to walk `pandoc.utils.citeproc(doc)`'s
processed body with a `Cite` handler, capturing each `Cite` node's own
rewritten `.content` (Pandoc rewrites `Cite.content` in place while
preserving node identity/order — confirmed with a standalone
`pandoc --lua-filter` probe) into a new per-occurrence table
(`citeprocInstances`, indexed by a document-order counter shared with the
main `Cite` handler). The first attempt **prepended** this per-instance
short form directly inside the margin note, ahead of the existing
`citeprocBibliography`-keyed full-entry loop (kept for the `Bifurcan`/
`Ficciones`/`Universalbibliothek`/`Siamese Press`-type assertions, which
need the full reference-list entry text, not just the short parenthetical).

This mostly worked (all 3 fixtures\' regex assertions passed, and 2 of 3
fixtures\' position assertions passed), but
`citation-margin-locator-citeproc.qmd`\'s `"42" leftOf "Bifurcan"` assertion
failed with wildly inconsistent bounding boxes. Root cause: `pdf-extract`
(via `crates/quarto-test/src/assertions/pdf_text_position.rs`) merges
contiguous PDF text into one searchable "item" at real line-break/marked-
content boundaries, not at Typst's own paragraph word-wrap points — so an
unbroken multi-line bibliography entry (4 lines for this fixture) becomes
**one item** whose bounding box spans the entire wrapped paragraph. Both
"1941, 42" and "Bifurcan" resolved into the same (or an overlapping) merged
item, so `leftOf` failed on razor-thin, essentially arbitrary margins
(observed: Subject.Right=568.7 vs Object.Left=448.7, sub-point differences
after even inserting an explicit `#linebreak()` between the two segments).

**Gordon's course-correction, mid-session:** asked (1) to compare against
Q1's actual behavior, and (2) to re-examine the "geometrically unwinnable"
claim, reasoning that if "Bifurcan" is in the margin column and the locator
is in the main column, the two areas should be disjoint and trivially
satisfy `leftOf`. Both were right, and connected: checking
`external-sources/quarto-cli/src/resources/filters/quarto-post/typst.lua`
found it **byte-for-byte identical** to Q2's pre-fix code (same Pass 0, same
`citeprocBibliography`-only loop, same `result:insert(cite)` keeping the raw
`Cite` node). Yet Q1's checked-in reference output
(`external-sources/quarto-cli/tests/docs/smoke-all/typst/margin-layout/citation-margin-locator-citeproc.typ`,
line 462) shows the **body** text containing the literal short form
`\(Borges 1941, 42)` inline, not the raw native Typst citation syntax
(`@borges1941library[p.~42]`) this Lua code alone would produce. The only
explanation: Q1's pipeline runs real Pandoc `--citeproc` on the *main*
document before this filter ever runs, mutating the actual `Cite` nodes in
place — exactly the citeproc auto-invocation gap flagged out of scope for
this fix (`filter_resolve.rs` / `format.rs` / `pandoc_write.rs`; confirmed by
Q2's own `.typ` output still containing the raw `@key[locator]` syntax).

**Attempt 2 (correct, landed): substitute in the body, not the margin.**
Q1's mechanism can't be replicated without touching the forbidden
architecture, but its *visual effect* can, purely in `typst.lua`, using data
Pass 0 already computes. Changed the `Cite` handler so that when a
per-instance short form is available (`use_citeproc` and
`citeprocInstances[counter]` populated), it's inserted **in place of** the
raw `cite` node in the body — not appended in the margin. The margin note
reverts to exactly its original content (the `citeprocBibliography`-keyed
full-entry loop, unchanged from before either attempt). This exactly
reproduces Q1's structure (confirmed side-by-side: Q2's generated
`.typ` line `LOCATOR-MARKER: ... references (Borges 1941, 42)#note(...)...`
matches Q1's reference line 462-463 verbatim in shape). It also makes the
`"1941, 42" leftOf "Bifurcan"` check trivially robust instead of fragile:
the short form now lives in the main column (x≈238–277pt in this fixture)
and the full entry lives in the margin column (x≈505–534pt) — genuinely
disjoint bands with a ~228pt gap, confirmed via `pdftotext -bbox`, not a
sub-point coincidence.

**Fixture adjustments (Phase-3 "fixture errors" category, not code bugs):**
`citation-margin-locator-citeproc.qmd`\'s position-assertion subject was
changed from bare `"42"` to `"1941, 42"` — bare `"42"` is ambiguous because
the body's own locator-aware short form and (in earlier attempts) the
margin's duplicate both contain "42"; `"1941, 42"` is unambiguous and still
directly reflects the assertion's own stated intent ("citeproc drops `p.`
prefix" — comment already recommended using year+locator elsewhere in the
same file's `ensurePdfRegexMatches`).

**Verification:** all 15 `citation-margin-*` fixtures pass
(`SMOKE_FILTER=margin-layout/citation-margin`); full `margin-layout` smoke
suite (86 fixtures, Jupyter on PATH) is 80 passed, 1 non-fixture skip
(`index.qmd`), 5 failed — exactly the 5 Jupyter-revealed findings from the
section above, zero regressions among the other 81. `cargo clippy -p quarto
--all-targets -- -D warnings` clean.

**In scope, not touched:** Q2's citeproc auto-invocation architecture
(`resolve_filters` in `filter_resolve.rs`, the Pandoc `-citations` CLI
variant question in `format.rs`/`pandoc_write.rs`) remains untouched, as
constrained — this fix is entirely local to `typst.lua` and one fixture's
test assertions.

## Fix 2 (cap-location non-margin values): implemented and verified (2026-09-29)

Of the 5 Jupyter-revealed failures above, this closes the
`margin-listing-cell-option-caption-below.qmd` /
`margin-table-great-tables-caption-below.qmd` root cause: a `cap-location`
(or `<reftype>-cap-location`) cell option value other than `"margin"` (e.g.
`bottom`) was silently dropped pre-engine, so `floatreftarget.lua`\'s
`cap_location(obj)` always fell back to the category default
(`caption_location` in `mainstateinit.lua`: `"top"` for `tbl`/`lst`,
`"bottom"` for `fig`) instead of honoring the author's cell option.

**Fix, landed (uncommitted, in this worktree):**
`codeblock_shorthand.rs`'s `Wrapper::Float` enum variant gained an
`attributes: Vec<(String, String)>` field. A new function
`wrapper_cap_location_attribute(parsed: &CellOptions, ref_type: &str) ->
Vec<(String, String)>`, sibling to `wrapper_column_classes` and called at
the same call site, reads `<reftype>-cap-location` (falling back to
generic `cap-location`) and forwards it verbatim as a single
`("cap-location", value)` tuple (empty vec if the option wasn't set). The
wrapper `Div`'s `attr` `LinkedHashMap` — previously always constructed
empty — is now built from this vec via `.into_iter().collect()`. The
existing `"margin"` → `margin-caption` class push in
`wrapper_column_classes` is untouched (kept in case something downstream
depends on it as a class, not just the attribute).

**Verification:**
- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core --no-fail-fast`: 5291/5291 passed, 32
  skipped (one `julia_engine_e2e::j1_minimal_julia_render` failure seen in
  an earlier, contention-heavy run did not reproduce in isolation or in
  this clean rerun — a flake from concurrent `cargo-nextest` processes on
  the machine, not a real regression).
- `SMOKE_FILTER=margin-table-great-tables-caption-below cargo nextest run
  -p quarto`: **now fully passes** (was failing on the missing `position:
  bottom` pattern before this fix).
- `SMOKE_FILTER=margin-listing-cell-option-caption-below cargo nextest run
  -p quarto`: the `position: bottom` pattern now matches (confirming the
  fix works), but the fixture still fails — on a *different*, already-known
  bug: `ensurePdfTextPositions` reports `"World"` is ambiguous (2 matches).
  This fixture also sets `#| eval: false` on its Python cell, so it's
  blocked by the same eval:false-not-honored gap as
  `margin-listing-cell-option.qmd` (see previous section) — not a new
  finding, just a second fixture hitting the same still-open bug.
- A `cargo nextest run --workspace --no-fail-fast` was attempted
  concurrently with this fix's edits (started just before the edit, so its
  build may have raced the source change) — do not treat its 15259
  passed/2 failed result as authoritative for this fix; a clean rerun
  (not concurrent with any edit) was kicked off afterward — see `##
  Status` for its result once available. The two failures seen were
  `smoke_all::smoke_all` (expected) and `quarto-test
  runner::tests::should_error_respects_project_render_context`, the latter
  confirmed pre-existing and unrelated by reverting
  `codeblock_shorthand.rs` to HEAD (521f0b093) and reproducing the
  identical failure, then restoring the fix.

**Net effect on the 5 Jupyter-revealed findings:** 1 of 5
(`margin-table-great-tables-caption-below.qmd`) now fully resolved; 4
remain — `margin-listing-cell-option-caption-below.qmd` (now blocked only
by the eval:false gap, not cap-location), `fullwidth-table-great-tables.qmd`,
`margin-listing-cell-option.qmd`, and `margin-subtable.qmd`.

## Remaining 4 Jupyter-revealed findings: classified and closed out (2026-09-29)

Each of the 4 remaining findings was root-caused, then classified per
Gordon's rule for this task: fixes touching the Pandoc writer or
book-related code stay in-plan; fixes for generic Jupyter/Python-engine
behavior (would misbehave identically outside typst/margin-layout
entirely) get a braid strand + a `skip:` line on the fixture, rather than
being fixed here. All 4 are now closed out — one in-plan fixture fix, three
stranded-and-skipped (one of which uncovered a second, deeper root cause
in the same fixture, also stranded-and-skipped). **P5 margin-layout is now
82/86 pass, 4 skipped (3 newly stranded above + `index.qmd`, no test
specs), 0 fail** — the only other smoke-all failure workspace-wide is the
pre-existing, unrelated `typst/pdf-text-position-test.qmd`.

- **`fullwidth-table-great-tables.qmd` — in-plan fixture fix, then a second,
  distinct root cause found and stranded.** The original `DataFrame.pivot()
  got an unexpected keyword argument 'on'` error is confirmed to originate
  in the fixture's own Python source (lines with `.pivot(index=[...],
  on="year", values="population")`), not in any Q2-generated/injected code
  — a fixture-porting bug: the fixture was written against a newer polars
  API (`pivot(on=...)`, introduced later) than the `polars 0.20.31` pinned
  in the dev venv, whose `pivot()` only accepts `columns=`. **Fixed in this
  fixture** by changing both cells' `on="year"` to `columns="year"`
  (identical semantics, just the parameter name the installed polars
  version expects). This is a fixture edit, not a Rust/Q2 code change.
  Fixing it uncovered a **second, independent** failure in the same
  fixture: both cells also call `GT(...).tab_spanner(label=..., columns=
  cs.all())`, which raises `TypeError: expected a selector; found [...]
  instead` — reproduced standalone in plain Python with zero Quarto/Q2
  involvement (`great_tables 0.4.0`'s `tab_spanner` → `cols_move` →
  `eval_select` unconditionally calls `polars.selectors.expand_selector`
  even when the resolved columns are already a plain list of strings, and
  `expand_selector` rejects anything that isn't a polars selector object —
  confirmed the same `TypeError` occurs even passing a plain `["b","c"]`
  list instead of `cs.all()`). This is a pure third-party Python
  dependency-version incompatibility in the shared dev venv
  (`/Users/gordon/src/quarto-web/.venv`), unrelated to the Pandoc writer or
  any book/margin-layout code, and out of scope to fix by changing that
  shared venv from this worktree. **Stranded:** `bd-jq223o9p`. Fixture
  `skip:` line added citing it — the `on=`→`columns=` fix stays in the
  fixture regardless (it's correct and will be needed once the venv
  incompatibility is resolved upstream).

- **`margin-listing-cell-option.qmd` and
  `margin-listing-cell-option-caption-below.qmd` — confirmed generic
  Jupyter-engine gap, stranded.** Dispatched a focused investigation of
  where `eval` is consumed in the Jupyter engine's execution path
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs`) versus the
  knitr/R path, for contrast. Finding: the Jupyter engine **never reads the
  `eval` cell option anywhere** — `execute_blocks_inner()` unconditionally
  calls `daemon.execute_in_session()` for every partitioned executable
  cell; `resolved_flag()` is only ever called for `"error"`, `"include"`,
  `"echo"`, `"output"`, `"warning"` (`CellVisibility::resolve`). The string
  literal `"eval"` does not occur anywhere under
  `crates/quarto-core/src/engine/jupyter/`. By contrast, the knitr/R path
  never implements per-chunk `eval` gating in Rust/R-glue code either — it
  relies entirely on knitr's own native, built-in per-chunk `eval` chunk
  option (`execute.R` sets a document-level `opts_chunk$eval` default;
  `hooks.R` force-overrides it via `opts_hooks` for `execute: enabled:
  false`), which is why the R path "just works" while the Jupyter path,
  lacking any equivalent, silently ignores `eval: false` entirely. Directly
  confirmed on `margin-listing-cell-option.qmd`: the rendered `.typ`
  contains both the echoed source `print(greet("World"))` *and* its
  executed output `Hello, World!`, which is exactly why
  `ensurePdfTextPositions` reports `"World"` ambiguous (2 matches) on both
  fixtures. This is a generic Jupyter/Python-engine gap with nothing to do
  with the Pandoc writer or book-specific code — any Quarto document
  rendered via the Jupyter engine with `eval: false` on a cell is affected
  identically, regardless of Typst or margin-layout. **Stranded:**
  `bd-c439o0wo` (one strand for both fixtures, as instructed, since they
  share the identical root cause). Both fixtures\' `skip:` lines cite it.

- **`margin-subtable.qmd` — confirmed generic Jupyter-engine gap, stranded.**
  First confirmed the pre-engine Rust wrapping (Root cause 4's fix) *does*
  apply correctly and engine-agnostically to this Python cell: `label:
  tbl-margin-panel` + `tbl-subcap` correctly hits
  `codeblock_shorthand.rs`'s generic `<reftype>-subcap` check (branches on
  `def.ref_type`, not hardcoded to `"fig"`) and returns `Wrapper::None`,
  leaving the cell's `label`/`tbl-cap`/`tbl-subcap` options untouched for
  the engine — this part of the in-plan Rust code is working exactly as
  designed, for Python same as R. The gap is downstream: rendering the
  fixture directly and inspecting the generated `.typ` shows the two
  `display(GT(...))` outputs come back as two bare `#table(...)` blocks
  with **no ids, no captions, and no `#note()`/`quarto_super` wrapper at
  all** — `@tbl-margin-panel` is reported as an unresolved crossref. Unlike
  knitr, whose `hooks.R` (`output_label`/`output_label_placeholder`,
  `figure_cap`) uses the still-present `label`/`subcap` options to
  synthesize each panel's own id/caption from multiple R plot outputs
  under one chunk, the Jupyter engine (`text_execute.rs`'s
  `render_cell`/`format_outputs`) has no equivalent per-panel
  id/caption-synthesis logic at all for multiple IPython `display()` calls
  under one label+subcap. This is a missing feature in the generic
  Jupyter engine's own output-synthesis, unrelated to the Pandoc writer or
  book/margin-layout code — it would misbehave identically for any Quarto
  document (HTML included) using `tbl-subcap`/`fig-subcap` via a
  Jupyter-kernel language. **Stranded:** `bd-gbaykhth`. Fixture `skip:`
  line added citing it.

**Verification:** `SMOKE_FILTER` reruns of each of the 4 fixtures individually
confirm: `fullwidth-table-great-tables.qmd`, the two listing fixtures, and
`margin-subtable.qmd` all now report `⊘ skipped` (not fail) with their
strand id + reason in the skip message; the full `cargo nextest run -p
quarto --no-fail-fast -E 'test(smoke_all)'` run shows **230 passed, 34
skipped, 1 failed** workspace-wide, with the sole failure being the
pre-existing, unrelated `typst/pdf-text-position-test.qmd`. No Rust source
was changed in this pass (fixture-only edits: the `on=`→`columns=` fix and
the 4 `skip:` YAML additions) — `cargo clippy`/`cargo nextest -p
quarto-core` gates from the Fix 2 checkpoint above are unaffected and were
not rerun; the workspace-wide `cargo nextest run --workspace --no-fail-fast`
phase-boundary gate was rerun (see `## Status`/handoff for the result).

## 2026-09-29 — second regression in the subfloat path, root-caused and fixed

After this doc's "fully closed" claim above, `smoke_all::smoke_all` was red again
at `72ad0eda8` on 5 of this group's fixtures: `margin-table-flextable-crossref.qmd`,
`margin-table-flextable.qmd`, `margin-table-gt-r-crossref.qmd`,
`margin-table-gt-r.qmd`, `tbl-column-margin.qmd` — all failing
`ensureTypstFileRegexMatches` on `` `#notefigure\(` `` not found. This was a
**second, distinct** bug from Root cause 3/4 above — not a reopening of the
original `wrapper_column_classes` gap, which is still merged and still correct.

**Root cause:** `crates/quarto-core/src/transforms/float_ref_target.rs`\'s
`clear_matching_id` (added by bd-2lxj10z0, "Restore knitr label visibility
through PreEngineSugaringStage" — see that function's own doc comment for the
`label_reinject`/leaked-echo background). For a figure, the label a knitr chunk
echoes back lands on a bare `Image`, which is never independently promoted to a
float — blanking its `attr.0` id is sufficient. For a **table**, knitr's leaked
echo is itself a `Div` carrying the same crossref id, and because
`FloatRefTargetSugarTransform::transform_block` walks bottom-up, that echo Div
gets classified and promoted to a full `Custom(FloatRefTarget)` node *before*
the outer, correctly-authored wrapper Div (same id) is processed. By the time
the outer wrapper's `clear_matching_id` runs, blanking the echo's `attr.0` no
longer helps: Lua's `crossref_mark_subfloats()`
(`resources/pandoc-filters/filters/crossref/preprocess.lua`) matches nested
floats by **custom-node type**, not by identifier, so the id-blanked-but-still-
`FloatRefTarget`-typed echo is still counted as a subfloat. That flips
`float.has_subfloats` true and routes the whole float through Typst's
subfloat/`#note(quarto_super(...))` path instead of the plain single-float
`#notefigure(...)` path — confirmed by dumping the pre-filter pandoc JSON AST
(`q2 render margin-table-gt-r.qmd --to typst`, retaining the normally-deleted
temp `pandoc-input.json`): it already contained two nested
`Custom(FloatRefTarget, identifier: tbl-islands-r)` nodes before any Lua filter
ran, the inner one with its pandoc-level id blanked but its `plain_data`
JSON payload (and node type) untouched.

**Fix:** `clear_matching_id` now splices the leaked echo's own `content` slot
in its place instead of merely blanking its id, removing the duplicate float
entirely rather than leaving an anonymous-but-still-typed one. Required
restructuring the block-list walk (`clear_blocks`, new) to operate on
`&mut Blocks` (owned `Vec`, supporting splice) with a `clear_block`
(single-block, non-splicing) sibling for the one truly-singular slot shape
(`Slot::Block`). No test-vocabulary or fixture changes — the existing
`ensureTypstFileRegexMatches` assertions already covered this correctly; they
were just failing.

**Verification:**
- All 5 fixtures individually (`q2 render <fixture>.qmd --to typst`): each now
  emits exactly one `#notefigure(` and one `quarto-float-tbl`, no nested `#note(`.
- `SMOKE_FILTER=margin-layout cargo nextest run -p quarto --test integration --
  smoke_all`: **1 passed** (all margin-layout fixtures, 400 others skipped by
  the filter).
- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: **5306 passed**, 32 skipped, 0 failed —
  no regressions from this fix.
- Full unfiltered `cargo nextest run -p quarto --test integration -- smoke_all`:
  **passes** (see epic doc for the combined Group A + Group B result and the
  workspace-wide gate).
