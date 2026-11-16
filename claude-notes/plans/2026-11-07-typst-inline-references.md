---
title: 'Typst: citations to inline front-matter references fail to compile (bd-yjf99rzx)'
date: 2026-11-07
description: 'Typst renders that cite a key defined only in front-matter `references:` fail with a raw Typst label error, as in Quarto 1; keep Quarto 1 behavior, but stop before Typst runs with a Q-21-5 error and warn (Q-21-6) about ignored inline entries, for single documents and books.'
status: implemented  # 2026-11-10, branch braid/bd-yjf99rzx-typst-citations-inline-front
braid:
  strand: bd-yjf99rzx
---

**Worktree:** `.worktrees/bd-yjf99rzx-typst-citations-inline-front` (branch `braid/bd-yjf99rzx-typst-citations-inline-front`, based on `main` @ `d4b4bb26c`)

## Triage verdict

**Ready to implement.** The user chose to keep Quarto 1\'s behavior (option A), fail before Typst runs with a Q2 error, warn about ignored inline entries, and cover book renders. Unknown keys are split off to bd-74zz2qko.

## Decisions (2026-11-09)

1. **Behavior: keep Quarto 1's (option A).** Typst keeps its native citation engine and its `citeproc: false` default. q2 does not turn citeproc on automatically or convert `references:` for Typst.
2. **Severity: a Q2 error before Typst runs.** When q2 knows Typst will fail, it stops first and says why, instead of passing on Typst's raw label error.
3. **Mixed documents: render everything Quarto 1 renders.** The user prefers strict validation but doesn't want to refuse documents that have a reasonable reading. The error fires only for a cited key that is defined in `references:` and in no `bibliography:` file, which is exactly the case where Quarto 1 fails (see the table below).
4. **Books: in scope.**
5. **Shadowed inline entries: warn.** When a cited key is defined in both `references:` and a `bibliography:` file, Typst uses the file\'s entry. q2 renders as before but warns that the inline entry is ignored, with a new diagnostic code.
6. **Unknown keys: a follow-up.** A cited key defined nowhere (a typo) is bd-74zz2qko, which can reuse this plan\'s key reading.
7. **Codes in the Typst family.** Q-21-5 (error): a cited key is defined only in `references:`. Q-21-6 (warning): a cited key\'s inline entry is shadowed by a `bibliography:` file. Both were free on `main` @ `d4b4bb26c`; recheck before landing.

## Issue context

Filed 2026-11-07 (P2 bug) while verifying issue #1110. A document whose references come only from front-matter `references:` fails `q2 render --to typst`. The `Cite` becomes a native Typst `@item`, nothing defines `<item>`, and Typst stops with Q-21-3 `label <item> does not exist in the document`.

## Dependency graph

- **related**: bd-kn01ctxg (closed): issue #1110, document `lang` for the citeproc locale. This strand surfaced while that fix was checked against Typst output; there is no causal link.
- No `discovered-from`, `blocks` or parent edges, so there is no downstream pressure.

## What the code looks like today

**Why Typst behaves differently.** Typst keeps Quarto 1's `citeproc: false` format default (bd-ysqekrm2). `citeproc_defaults()` in `crates/quarto-core/src/stage/stages/user_filters.rs` sets `from_bibliography: !is_typst && !in_book`. So on Typst, neither `bibliography:` nor `references:` turns citeproc on; `apply_citeproc_default()` (`crates/quarto-core/src/filter_resolve.rs`) treats both keys alike for the other formats. Citations then go to Typst's own engine: Pandoc's Typst writer emits `@key`, and the template emits `#bibliography(...)` only from `$bibliography$` file paths. Inline `references:` never reach Typst.

**Quarto 1 does the same.** Its Typst template (`external-sources/quarto-cli/src/resources/formats/typst/pandoc/quarto/biblio.typ`) also emits `#bibliography` only for `bibliography:` files. A Quarto 1 dev build (`quarto-cli` @ `d3263862a`, 2026-11-03) fails with the identical Typst error. No matching quarto-cli issue turned up in a GitHub search; that is weak evidence that none exists.

**Typst books differ.** `crates/quarto-core/src/project/book/single_file_render.rs` runs citeproc on the merged book itself, except in `citation-location: margin` mode, where it leaves `Cite` nodes for Typst. So a default-mode q2 book already resolves inline references (Quarto 1's doesn't), and margin-mode books fail like single documents.

### Behavior matrix

Typst target, `citeproc` unset. Reproduce with `typst-inline-references-investigation/probe.sh` (`probe.txt`) and `probe-cases.sh` (`probe-cases.txt`).

| Case | q2 today | Quarto 1 | After this plan |
|---|---|---|---|
| `references:` only, cited (`repro.qmd`) | fails: label error | same | **Q-21-5 error** |
| `references:` + `citeproc: true` | renders | renders | renders |
| `references:` present, nothing cited (`mixed-cases/nocite.qmd`) | renders | renders | renders |
| `bibliography:` + `references:`, cite a `.bib`-only key (`uncited.qmd`) | renders | renders | renders |
| key defined in both; the `.bib` entry wins, the inline one is silently ignored (`inboth.qmd`) | renders | renders | renders, **Q-21-6 warning** |
| `bibliography:` + `references:`, cite an inline-only key (`inlineonly.qmd`) | fails | fails | **Q-21-5 error** |
| single document, `citation-location: margin` (`single-margin.qmd`) | fails | fails | **Q-21-5 error** |
| book, default citation mode (`book/`) | renders | fails | renders |
| book, `citation-location: margin` | fails | fails | **Q-21-5 error** |

**What Typst's native bibliography reads.** Typst 0.15.1 (CI's `TYPST_VERSION`) accepts only Hayagriva `.yml`/`.yaml` and BibLaTeX `.bib`; CSL-JSON fails with `unknown bibliography format`. (This ruled out option C cheaply; recorded for whoever revisits it.)

**Where the check fits.** `PandocPrepareStage` (`crates/quarto-core/src/stage/stages/pandoc_prepare.rs`) runs on every target, native and WASM, and receives the final document AST just before the Pandoc request is built. At that point:
- citeproc has already run if it was going to, so a remaining `Cite` will reach Typst as `@key`;
- in margin mode the `Cite` nodes are still present, because `quarto-post/typst.lua`\'s margin handler runs inside Pandoc afterwards;
- crossref `Cite`s (`@fig-x`) are already resolved, and a key not defined in `references:` can't trigger the rule anyway.

To confirm in Phase 1: the book single-file path reaches `PandocPrepareStage` with the merged AST.

## Proposed phases (draft)

- **Phase 0 — Tests first.**
  - Unit tests for the rules: inline-only cited key → Q-21-5; key also in a `.bib` or a Hayagriva `.yml` → no error, one Q-21-6 warning; uncited inline entry → nothing; `citeproc: true` → nothing; HTML target → nothing; unparseable bibliography file → no Q-21-5 for keys it might define.
  - smoke-all Typst fixtures for `repro.qmd`, `inlineonly.qmd`, `inboth.qmd` and the single-document margin case.
  - A Typst book fixture in margin mode.
- **Phase 1 — The check.** In `PandocPrepareStage`, for a Typst target:
  - collect the keys of `Cite` nodes in the final AST;
  - collect `references:` ids from metadata;
  - collect keys from each `bibliography:` file read through the `SystemRuntime` (so it works in WASM): BibLaTeX via the `biblatex` crate pampa already uses; Hayagriva `.yml` via its top-level mapping keys. If a file can't be read or parsed, don't report its keys as missing (Typst will report that file itself).
  - For each cited key in `references:` and in no file, emit Q-21-5 (error) naming the key, pointing at the citation\'s source location, and suggesting `citeproc: true`.
  - For each cited key in `references:` *and* in a file, emit Q-21-6 (warning) naming the key and the file whose entry Typst uses.
- **Phase 2 — Codes.** Add Q-21-5 and Q-21-6 to `crates/quarto-error-catalog/error_catalog.json` with docs pages `docs/errors/typst/Q-21-5.qmd` and `Q-21-6.qmd`, following Q-21-1 to Q-21-4.
- **Phase 3 — Books.** Confirm the book margin-mode path hits the same check; add the book fixture.
- **Phase 4 — Docs.** A "Citations" section in the Typst guide (`docs/guides/formats/typst/index.qmd`): Typst's native engine by default, what `bibliography:` and `references:` do there, and `citeproc: true`.

## Open questions for the user

None; all settled on 2026-11-09 (see Decisions).

## Implementation notes (2026-11-10)

- **Where.** `crates/quarto-core/src/typst_citations.rs` holds the check. `PandocWriteStage::prepare()` calls it for Typst targets, so native renders (`PandocWriteStage`) and browser renders (`PandocPrepareStage`) share it. `prepare()` resolves `bibliography:` paths with the same `meta_path_files` it uses for the PDF request.
- **When it applies.** A `Cite` still in the final AST is one Typst will see as `@key`; citeproc removes the ones it resolves, and book renders in default mode run citeproc on the merged book, so they are unaffected. `citeproc: true` turns the check off: with margin citations, the vendored Lua margin handler then formats the citations inside Pandoc, after `prepare()`, so the `Cite` nodes are still present but never reach Typst as `@key` (verified with a `keep-typ` render).
- **Typst source only.** With an `output-ext` other than `pdf`, Typst never runs, so Q-21-5 is a warning there and the `.typ` is still written. Decided during implementation; flagged in the PR.
- **Reading keys.** BibLaTeX via the `biblatex` 0.12 crate (already in the build through pampa; hayagriva itself uses `biblatex` 0.11), Hayagriva via top-level YAML mapping keys. Any other format, an unreadable file, or a parse failure gives "unknown", which suppresses Q-21-5.

## Risks / tradeoffs (draft)

- **False positives from bibliography parsing.** If q2 fails to read a key that Typst would find (a BibLaTeX or Hayagriva construct q2's reader mishandles), q2 would refuse a document Typst would compile. Mitigation: only flag keys that are defined in `references:`, and treat a file that fails to parse as "unknown," never as "missing keys."
- **Two parsers for one file.** q2 reads the `.bib`/`.yml` only to collect keys, while Typst does the real parse. Keeping the q2 side to keys only limits how far the two can disagree.
- **Margin mode.** The check relies on margin-mode `Cite` nodes still being in the AST at prepare time. That holds today, and the test fixture will pin it.
