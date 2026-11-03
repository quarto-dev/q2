# Issue #1020 — A callout with an id loses its number in the default title

- **GitHub**: https://github.com/quarto-dev/q2/issues/1020
- **Reporter**: @mcanouil (Mickaël Canouil), 2026-11-02
- **Triage date**: 2026-11-03
- **Worktree**: `.worktrees/issue-1020` (branch `issue-1020`, based on `main` @ `2e34b3c46`)
- **Braid strand**: bd-h14a4m83
- **Scope**: HTML rendering of crossref-eligible callout titles (`#nte-`, `#wrn-`,
  `#tip-`, `#imp-`, `#cau-` ids). This covers the untitled case the issue
  reports, and also the titled and `appearance="simple"` cases, which share
  the same root cause. Non-HTML formats are not covered.

## Summary

A callout with a crossref id (`::: {#nte-x .callout-note}`) renders its
default title as "Note" in q2, while `@nte-x` resolves to "Note 1". Quarto 1
writes "Note 1" in both places. I reproduced the bug, and it is wider than
the report. q2 **never** puts the crossref prefix into a callout title:

| Input                                       | Quarto 1               | q2             |
| ------------------------------------------- | ---------------------- | -------------- |
| `#nte-x`, no title                          | `Note 1`               | `Note`         |
| `#nte-x`, `## My title` heading             | `Note 1: My title`     | `My title`     |
| `#wrn-y`, `title="Attr title"`              | `Warning 1: Attr title`| `Attr title`   |
| `#tip-z`, `appearance="simple"`, no title   | `Tip 1`                | *(no title)*   |

This is a real bug, and the fix is a pipeline-ordering problem rather than a
one-line change in the title code. The callout's crossref number does not
exist yet when `CalloutResolveTransform` builds the title.

## Reproduction

Fixtures:
- `repro.qmd`: the reporter's case.
- `exp-titled.qmd`: heading title, `title=` attribute, and `appearance="simple"`.
- `exp-two.qmd`: two callouts plus a missing ref. It confirms the reference
  numbers are real (`Note 1`, `Note 2`) and not a fallback.

`claude-notes/` is itself a Quarto project that excludes these files, so I
rendered copies outside the repo:

```sh
cp claude-notes/issue-reports/1020/*.qmd "$SCRATCH"/ && cd "$SCRATCH"
q2 render repro.qmd --to html        # target/debug/q2 @ 2e34b3c46
quarto render repro.qmd --to html    # Quarto 1 (local dev build, 99.9.9)
grep -n -A2 'callout-title-container\|quarto-xref' repro.html
```

q2 output (`repro.qmd`):

```html
<div id="nte-x" class="callout callout-style-default callout-note callout-titled">
...
<div class="callout-title-container flex-fill">
Note
</div>
...
<p>See <a href="#nte-x" class="quarto-xref">Note&nbsp;1</a>.</p>
```

Quarto 1 writes `Note&nbsp;1` in the title container. The reference text in
q2 already uses U+00A0 between kind and number, so it matches Quarto 1. Only
the title is wrong.

## Localization

The cause is the order of the transforms in `build_transform_pipeline`
(`crates/quarto-core/src/pipeline.rs`):

1. `CalloutTransform` (`pipeline.rs:2306`) turns the Div into a `Callout`
   CustomNode. When the id classifies, it records the crossref triple
   (`ref_type`, `kind`, `identifier`) in `plain_data`
   (`crates/quarto-core/src/transforms/callout.rs:285-296`).
2. `CalloutResolveTransform` (`pipeline.rs:2307`, still in the
   **Normalization** phase) immediately turns the CustomNode back into a plain
   `Div` and builds the title. There is no crossref number yet, so the default
   title is just `callout_display_name(type)`
   (`crates/quarto-core/src/transforms/callout_resolve.rs:248-262`). A
   user-supplied title passes through without a prefix. `is_crossref`
   (`:246`) is used **only** to suppress the screen-reader span. The comment
   at `:240-245` assumes that a crossref prefix is "already" in the title, but
   nothing puts it there.
3. `FloatRefTargetSugarTransform` (`pipeline.rs:2441`) classifies **any** Div
   whose id has a registered prefix (`classify_div`,
   `crates/quarto-core/src/transforms/float_ref_target.rs:265-274`). The
   resolved callout Div `#nte-x` gets wrapped as a `FloatRefTarget`.
4. `CrossrefIndexTransform` (`pipeline.rs:2454`, **Crossref** phase) numbers
   that FloatRefTarget. As a result, `@nte-x` resolves to "Note 1".
5. `CrossrefRenderTransform` sees a non-float kind (`nte`), takes the legacy
   pass-through branch (`crossref_render.rs:686-693`), and emits the callout
   Div unchanged. The number never reaches the title.

The callout's own crossref annotation (step 1) is dead weight in the render
pipeline. The callout gets its number by accident, through the generic float
path, and only after its title is already built. The callout annotation is
live only in the analysis/LSP pipeline (`build_analysis_transform_pipeline`,
`pipeline.rs:1075`), which omits `CalloutResolveTransform`.

**A model to copy:** `prefix_caption` in
`crates/quarto-core/src/transforms/crossref_render.rs:1435` already produces
Quarto 1's `"{kind}\u{a0}{n}: "` prefix shape for float captions. It uses
`terms.crossref_prefix(ref_type)` for the localized kind
(`crossref_render.rs:1252`).

## Open questions — resolved during triage

- **How does `@nte-x` resolve at all if the callout is already a Div by
  indexing time?** I read the walker. `CrossrefIndexTransform` only indexes
  `Custom` nodes with the crossref triple, plus `sec` headers
  (`crossref_index.rs:207-246, 368-388`). The only `index.insert` caller is
  that walker. The answer is the float-sugar catch-all (step 3 above).
  Nothing seeds callouts earlier: `PreEngineSugaringStage` only seeds
  `crossref.ids`, and the analysis pipeline is LSP/book-only.
- **Is the reference number real or a fallback?** It is real: `exp-two.qmd`
  gives `Note 1` and `Note 2`, and `@nte-missing` gives the unresolved
  warning.
- **Does Quarto 1 also prefix titled and `simple` callouts?** Yes, see the
  table above (rendered locally). The fix must cover all three cases, not
  only the default title.
- **Is the nbsp in the reference text a separate gap?** No. q2 already emits
  U+00A0 in the xref link (checked with `od -c`).

## Outcome / recommended next step

Filed bd-h14a4m83. Fix scope:

1. Make the callout's crossref number available when its title is built.
   There are two candidate shapes; the fix session should pick one:
   - **(a) Defer title construction past `CrossrefIndexTransform`.** Keep the
     callout a `Callout` CustomNode until after indexing, so the indexer
     numbers it through its own annotation and `plain_data.order` is
     populated. The float sugar then never sees a callout Div. Then resolve
     it (or just its title) in a phase ≥ Crossref. Risks: the transforms
     between the two positions (Sectionize, Footnotes, TOC generation,
     Theorem/Float sugar) currently see a Div and would see a CustomNode
     instead. Panel-tabset and the phase-ordering test
     (`test_build_transform_pipeline_phase_ordering`) constrain where the
     resolve can go.
   - **(b) Keep the order and patch the title in the Crossref/Finalization
     phase.** Leave a marker on the resolved Div and prefix its
     `callout-title-container` once the number is known. This is a smaller
     diff, but it keeps the accidental float-path numbering and spreads
     callout knowledge into crossref code.

   (a) is the principled fix. It also retires the accidental
   `FloatRefTarget` wrapping of callouts.
2. Title rules (Quarto 1 parity):
   - Untitled: `"{Kind}\u{a0}{n}"`. This applies to `appearance="simple"`
     too, which today has no title at all, so `callout-titled` must be set.
   - Titled: `"{Kind}\u{a0}{n}: {title}"`.
   - The kind comes from the localized crossref prefix
     (`terms.crossref_prefix`), not `callout_display_name`.
3. Tests: unit tests in `callout_resolve.rs` (or wherever the title moves)
   for the three cases, plus an end-to-end HTML render check for
   `repro.qmd`.
4. Out of scope (check separately): pandoc-hybrid and Typst/LaTeX legs,
   which render callouts through other paths.

## Verification commands used

```sh
gh issue view 1020 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build                  # pre-flight on main
target/debug/q2 render {repro,exp-titled,exp-two}.qmd --to html
quarto render {repro,exp-titled}.qmd --to html       # Quarto 1 comparison
grep -n -A2 'callout-title-container\|quarto-xref' *.html
grep -o 'quarto-xref">Note.1' repro.html | od -c     # nbsp check
```

## Cross-references

- `claude-notes/designs/transform-pipeline-phases.md`: the phase model that
  constrains where callout resolution can move.
- Quarto 1 reference: `src/resources/filters/modules/callouts.lua` (title
  injection, `needs_screen_reader_callout_type`).
