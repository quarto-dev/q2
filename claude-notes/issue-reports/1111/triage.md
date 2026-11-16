# Issue #1111 — unknown citation key: no Q-code, no source location

- **GitHub**: https://github.com/quarto-dev/q2/issues/1111
- **Reporter**: @EwoutH (Ewout ter Hoeven), 2026-11-07
- **Triage date**: 2026-11-07
- **Worktree**: `.worktrees/issue-1111` (branch `issue-1111`, based on `main` @ `250e2ede9`)
- **Braid strand**: bd-s1kn2w1r (related: bd-be2f7)
- **Scope**: the whole issue: the HTML (citeproc) path, the Typst (native `#bibliography`) path, and the Q-10-2 collision it mentions (which is already tracked as bd-be2f7).

## Summary

The report is correct. If a document cites a key that is not in its bibliography, q2 fails the render. For HTML the error has no code and no location, and only the first missing key is reported. For Typst the error carries the generic `typst compile` code Q-21-3 and points into the intermediate `.typ` file. I reproduced both on `main`.

Triage found one more problem. **Pandoc and Quarto 1 do not treat this as an error.** They emit `[WARNING] Citeproc: citation X not found` and render the key as bold `X?`. The user decided to match that behaviour: a missing key becomes a coded **warning** at the key's `.qmd` location, for both HTML and Typst. This is a real bug with moderate scope, filed as bd-s1kn2w1r.

## Reproduction

The fixtures are in this directory: `repro.qmd` (one missing key), `repro-mixed.qmd` (one good key and two missing keys), and `refs.bib`. Render copies of them outside `claude-notes/`, because `claude-notes/` is itself a Quarto project and q2 refuses to render files that project excludes.

```bash
q2 render repro.qmd --to html
q2 render repro-mixed.qmd --to typst
```

The full output is in `observed-output.txt`. Key lines:

```
# --to html (both fixtures)
Error: Citation processing error: Reference 'missing2020' not found
```

No Q-code and no location. In `repro-mixed.qmd`, `@alsomissing` is never reported because processing stops at the first missing key.

```
# --to typst
Error [Q-21-3]: typst compile exited with exit status: 1:
error: label `<missing2020>` does not exist in the document
    ┌─ repro-mixed.typ:436:23
error: label `<alsomissing>` does not exist in the document
    ┌─ repro-mixed.typ:436:40
```

Pandoc 3.12.1 on the same input (`pandoc repro-mixed.qmd --citeproc -t html`):

```
[WARNING] Citeproc: citation alsomissing not found
[WARNING] Citeproc: citation missing2020 not found
... (<strong>missing2020?</strong>) ... (<strong>alsomissing?</strong>)
```

**Expected (decided):** one coded warning per missing key, at the key's span in the `.qmd`; the render succeeds and shows **key?** in place of each missing cite. This applies to HTML and to Typst.

## Localization

- `crates/quarto-citeproc/src/eval.rs:449`, `src/types.rs:524`, `src/types.rs:544`: every `Error::ReferenceNotFound` is built with `location: None`.
- `crates/quarto-citeproc/src/error.rs:107`: `to_diagnostic` maps `ReferenceNotFound` to Q-10-2 and would attach a location if it had one. That code collides with the template subsystem's Q-10-2 (bd-be2f7).
- `crates/pampa/src/citeproc_filter.rs:181`: `.map_err(|e| CiteprocFilterError::ProcessingError(e.to_string()))` turns the error into a string, which drops the code and any location. The `Display` impl for `ProcessingError` lives in `crates/pampa/src/unified_filter.rs:215`.
- `crates/pampa/src/citeproc_filter.rs:773ff` (`collect_citations_from_inlines`): converts `Inline::Cite` to citeproc `CitationItem`s and discards `Citation.id_source`. `CitationItem` (`quarto-citeproc/src/types.rs:147`) has no field to hold it.
- `id_source` **is** filled in by the parser (`crates/pampa/src/pandoc/treesitter_utils/citation.rs:28-79`), so the `.qmd` span exists. It is lost before citeproc runs.
- `apply_citeproc_filter` already returns a diagnostics vector (`Ok((pandoc, context, vec![], ...))` at `citeproc_filter.rs:208`), which can carry the new warnings.
- Typst: Q2 sets `citeproc: false` for Typst (`crates/quarto-core/src/filter_resolve.rs:53-62`), so Typst's own `#bibliography()` resolves `@key`. The citeproc filter never runs, and the failure comes from `typst compile`, which only knows the `.typ` file.

## Open questions — resolved during triage

1. **Should a missing key be an error or a warning?** Pandoc 3.12.1 warns and renders `**key?**` (see Reproduction). Quarto 1 inherits that behaviour. **Resolved by the user: warning, matching Pandoc.**
2. **Is the `.qmd` location available?** Yes. `Citation.id_source` is populated by the tree-sitter citation handler and preserved by `filters.rs`. Only the conversion to citeproc items drops it.
3. **Why does Typst point at the `.typ` file?** Typst does its own bibliography (`citeproc: false`), so the error comes from `typst compile`. To report a `.qmd` location, Q2 must check the keys itself before writing the `.typ`. Mapping Typst diagnostics back through the source map would be a larger, general feature, and no strand exists for it. The pre-check also lets Q2 substitute **key?** so Typst does not hard-fail, consistent with HTML.
4. **Is there already a strand?** No. `braid search` for citation, citeproc, typst compile, and location found nothing covering this. bd-be2f7 covers the Q-10-* collision and is linked as related.

## Outcome / recommended next step

Filed **bd-s1kn2w1r** (bug, P2, related to bd-be2f7). Fix scope:

- **HTML/citeproc:** check each cited id against the loaded references before processing. For each miss, push a located, coded warning into the filter's diagnostics and render `Strong[Str "key?"]` for that item. Stop stringifying citeproc errors (`citeproc_filter.rs:181`), so any remaining errors keep their `DiagnosticMessage`.
- **Typst:** run the same pre-check against the parsed bibliography before `typst compile`, with the same warning and the same **key?** substitution.
- **Q-code:** allocate a non-colliding catalog code instead of Q-10-2, and coordinate with bd-be2f7.
- **Tests:** the citeproc filter unit tests (`citeproc_filter.rs` already has typst/html target tests around line 4320), plus an end-to-end render test that asserts the code and the `.qmd` line and column.

## Verification commands used

```bash
gh issue view 1111 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build      # on main @ 250e2ede9 (see note)
cargo nextest run -p quarto-core -E 'binary(integration) & test(jupyter_record_shadow::shadow_twenty_cells)'
target/debug/q2 render repro.qmd --to html|typst          # built from 250e2ede9
target/debug/q2 render repro-mixed.qmd --to html|typst
pandoc repro-mixed.qmd -f markdown --citeproc -t html     # pandoc 3.12.1
grep -rn ReferenceNotFound crates
grep -rn id_source crates/pampa/src
braid search citation|citeproc|"typst compile"
```

Note on pre-flight: the full verify run had 18,700 of 18,701 tests passing. The one failure was `jupyter_record_shadow::shadow_twenty_cells`, which timed out after 300s waiting for a Jupyter kernel while the machine was under cold-build load. Rerun alone, it passed in 3.4s, so HEAD is green. The worktree is at the same commit, so verify was not rerun there.

## Cross-references

- bd-s1kn2w1r: the fix
- bd-be2f7: Q-10-* code collision between citeproc and doctemplate
- bd-o5zfde39: citeproc bibliography lists uncited references (issue #1109); in progress and touching the same filter
- `crates/pampa/AGENTS.md`: TDD workflow for the fix
