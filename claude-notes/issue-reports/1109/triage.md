# Issue #1109: citeproc bibliography includes uncited references

- **GitHub**: https://github.com/quarto-dev/q2/issues/1109
- **Reporter**: EwoutH (Ewout ter Hoeven), 2026-11-07
- **Triage date**: 2026-11-07
- **Worktree**: `.worktrees/issue-1109` (branch `issue-1109`, based on `main` @ `bbb4884de`)
- **Braid strand**: bd-o5zfde39 (related: bd-ae2i9ei1, bd-mumobar8, bd-69rqex6n)
- **Scope**: the citeproc bibliography contents (cited references plus `nocite`). The triage also covers two closely related differences from Quarto 1 found along the way: unsorted-style order and Typst-native `nocite`. Each is filed as its own strand.

## Summary

With `bibliography: refs.bib`, q2's citeproc puts every reference from the file into the bibliography. Pandoc and Quarto 1 include only the references cited in the document, plus the ones named in `nocite` metadata; `nocite: "@*"` includes all. Reproduced on q2 0.42.0 (`bbb4884de`). This is a real bug. The fix is small and belongs in pampa's citeproc filter (bd-o5zfde39). The user asked us to match Quarto 1, so the target behavior is Pandoc's `getReferences`.

## Reproduction

Fixtures are in this directory. `refs.bib` has four entries: `uncited`, `cited`, `second`, `extra`.

`claude-notes/` is a Quarto project that excludes `issue-reports/` from rendering, so render a copy of the fixtures elsewhere:

```bash
cp claude-notes/issue-reports/1109/* "$SCRATCH"/ && cd "$SCRATCH"
q2 render repro.qmd --to html
grep -o 'id="ref-[a-z]*"' repro.html
```

| Fixture | Cites | Expected (Quarto 1) | Observed (q2 0.42.0) |
|---|---|---|---|
| `repro.qmd` | `cited` | `cited` | `second`, `extra`, `uncited`, `cited` |
| `exp-nocite.qmd` (`nocite: @extra`) | `cited` | `extra`, `cited` | all four |
| `exp-nocite-all.qmd` (`nocite: @*`) | `cited` | all four | all four, plus warning Q-1-20 |
| `exp-numeric-order-reversed.qmd` (IEEE) | `second`, then `cited` | `[1]` Adams, `[2]` Smith | Doe (no number), `[2]` Smith, `[1]` Adams, Brown (no number) |

In the IEEE case, the uncited entries get no citation number because they were never cited. Once uncited entries are filtered out, that symptom goes away. The order problem stays (see bd-mumobar8).

## Localization

- `crates/pampa/src/citeproc_filter.rs`, `apply_citeproc_filter`: loads every inline `references:` entry and every `bibliography:` file into the `Processor` with `add_references`, with no filter.
- `crates/quarto-citeproc/src/types.rs`, `generate_bibliography_to_outputs` (and `generate_bibliography`): iterates `self.references`, which holds everything loaded.
- `crates/pampa/src/citeproc_filter.rs`, `extract_config`: fills `CiteprocConfig.nocite` through `get_meta_string_list`, which flattens inlines to text. Nothing reads the field afterwards.
- Model to copy: Pandoc `getReferences` in `external-sources/pandoc/src/Text/Pandoc/Citeproc.hs` (about line 180). It collects `Cite` ids from the whole document, metadata included, and from `nocite`. If `nocite` contains `*`, it keeps every reference; otherwise it keeps a reference only when its id is in that set. The filter applies to inline `references` and to the bibliography files alike.
- Book projects are already correct for cited-only. `build_merged_bibliography` in `crates/quarto-core/src/project/book/bibliography.rs` builds from each chapter's citation manifest (cited references only). Book `nocite` is not handled anywhere.

## Open questions resolved during triage

1. **Fix in `quarto-citeproc` or in pampa?** In pampa. `quarto-citeproc`'s CSL conformance harness (`tests/integration/csl_conformance.rs`, `bin/csl_conformance_report.rs`) calls `generate_bibliography_to_outputs` and expects every input item in the output. Pandoc also filters before calling the citeproc library. Filtering where the references are loaded keeps the citeproc crate spec-shaped.
2. **How does q2 parse `nocite`?** `nocite: "@extra"` reaches the config as inlines that render as `@extra`, so the ids can be read from `Cite` inlines the way Pandoc does. `nocite: "@*"` does not parse. It produces warning Q-1-20 and a `yaml-markdown-syntax-error` span, seen with `q2 get-config exp-nocite-all.qmd`. Filed as bd-ae2i9ei1. In the body, `@*` is a hard parse error, but nobody needs it there. Workaround until bd-ae2i9ei1 lands: `nocite: !str "@*"` (`exp-nocite-all-str.qmd`) parses with no warning and reaches the config as the plain string `"@*"`. So the fix for bd-o5zfde39 must also read ids from a plain-string `nocite`: split on commas and whitespace, then strip the leading `@`. Quarto 1's docs write `nocite: |` followed by `@*` with no tag, so copied documents still need bd-ae2i9ei1.
3. **Is Typst affected?** With `citeproc: true`, yes, through the same filter (the reporter confirmed this). With the default Typst-native bibliography, Typst already lists only cited works. But q2's `resources/pandoc-filters/typst-template/biblio.typ` lacks Quarto 1's `nocite-ids` loop (`#cite(label(...), form: none)`) and `full-bibliography` (`full: true`), so `nocite` has no effect there. Pandoc's Typst writer sets both variables (`Writers/Typst.hs:103-104`). Filed as bd-69rqex6n.
4. **Does bibliography order match Quarto 1?** Not for styles without a bibliography `<sort>`. q2 uses bibliography-file order; Pandoc uses citation order. Filed as bd-mumobar8 because it is a separate defect with its own conformance-suite question.

## Outcome / recommended next step

Filed **bd-o5zfde39** (P1). Fix scope:

1. Write tests first, in pampa's citeproc filter tests: cited only; cited plus an explicit `nocite` id; `nocite` with `*` (build the meta `Cite` in the test until bd-ae2i9ei1 lands); inline `references:` filtered the same way; a numeric style where the uncited entry disappears.
2. In `apply_citeproc_filter`, collect cited ids from the body and metadata `Cite` inlines plus the `nocite` `Cite` ids. Unless `*` is present, keep only matching references before calling `add_references`.
3. Replace the flattened-string `CiteprocConfig.nocite` with ids taken from `Cite` inlines. Also accept a plain-string value such as `!str "@*"` or `!str "@a, @b"`: split on commas and whitespace, then strip the leading `@`. The `!str` form is the documented workaround for `@*` until bd-ae2i9ei1 lands.
4. Run an end-to-end check with `q2 render` on `repro.qmd` and `exp-nocite.qmd`, for HTML and for Typst with `citeproc: true`.

Related, filed separately: bd-ae2i9ei1 (`@*` parse), bd-mumobar8 (unsorted order), bd-69rqex6n (Typst-native `nocite`).

## Verification commands used

```bash
gh issue view 1109 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
q2 render <fixture>.qmd --to html      # run on a copy outside claude-notes/
grep -o 'id="ref-[a-z]*"' <fixture>.html
q2 get-config exp-nocite-all.qmd | jq '.. | objects | select(has("nocite")) | .nocite'
grep -n "nocite" external-sources/pandoc/src/Text/Pandoc/Citeproc.hs
grep -n "nocite-ids\|full-bibliography" external-sources/pandoc/src/Text/Pandoc/Writers/Typst.hs
```

## Cross-references

- bd-o5zfde39 (main), bd-ae2i9ei1, bd-mumobar8, bd-69rqex6n
- Quarto 1 documentation: `external-sources/quarto-web/docs/authoring/citations.qmd`, section "Including Uncited Items"
- Quarto 1 Typst template: `external-sources/quarto-cli/src/resources/formats/typst/pandoc/typst.template`, lines 136-148
- `claude-notes/plans/2026-09-21-book-projects-P6-bibliography.md` (book bibliography merge)
