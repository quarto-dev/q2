---
title: 'P7 — Port `override-orange-book` + `orange-book-lang`'
date: 2026-09-27
description: 'Ports the `orange-book-lang` French localization fixture and the `override-orange-book` user-extension fixture, proving that a project-local extension overrides the vendored `orange-book` subtree end to end.'
---

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md)
**Depends on:** P1, P6.
**Worktree:** `workspace-5` (Track B, sequential after P6 — see epic's "Parallel
development plan").

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P6-book-fixture-harness.md \
  | grep -q '^Complete\.' && echo "P6 merged" || echo "STOP: P6 not yet merged, wait"
```

Once it prints "merged":

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git checkout -B typst-testing/p7-override-and-lang feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p7-override-and-lang   # retry from rebase (in workspace-5) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Scope

The two smallest, least entangled `orange-book*` fixtures — no cross-chapter merge
numbering, no position assertions. First real proof of P6's harness against a real
book project.

### `orange-book-lang`

Minimal (2 files: `index.qmd` + `chapter1.qmd`), `lang: fr`, `lof/lot: true`. Tests
only 3 `ensureTypstFileRegexMatches` French-localization strings ("Chapitre", "Liste
des Figures", "Liste des Tables"). **Correction, verified against the real fixture**:
`index.qmd` *does* set `_quarto.render-project: true` (`index.qmd:4`), and
`chapter1.qmd` is a real second chapter (`_quarto.yml`\'s `book.chapters`) — this is
not a single-file-in-project case. It needs P6's whole-book harness like every other
`orange-book*` fixture; don't strip `render-project: true` during the port on the
assumption it's unnecessary, and don't let "minimal, 2 files" read as "doesn't need
the harness" — those are independent facts.

### `override-orange-book`

Single chapter, `format: orange-book-typst` (not `typst`) — driven by a project-local
`_extensions/orange-book/` with its own `typst-show.typ` containing
`// LOCAL-OVERRIDE-MARKER`. The `_quarto.tests` key is literally `orange-book-typst`
(must match the format identifier, not always `"typst"` — confirm P1's assertion
parser keys off the actual format string, not a hardcoded `"typst"` literal). Exercises
Q2's `find_extension`'s documented "user extensions override builtin, last-match-wins"
behavior (`crates/quarto-core/src/extension/mod.rs` module doc) — specifically, the
vendored `orange-book` extension isn't in the plain builtin bundle
(`resources/extensions/` only has `kbd`/`lipsum`); it's registered separately as a
"vendored extension subtree" (`extension/mod.rs`'s `builtin_extension_subtree_roots`,
merged via `all_builtin_extension_roots`). This fixture exercises that
subtree-specific override path specifically, not the generic user-vs-builtin-extension
case — proves it end-to-end for the first time (currently only asserted in a doc
comment).

## Checklist

- [x] Copy both fixtures\' tracked source files into
      `crates/quarto/tests/smoke-all/typst/{orange-book-lang,override-orange-book}/`.
      Excluded generated outputs and caches listed in each source fixture's
      `.gitignore`.
- [x] `parse_test_specs` preserves arbitrary format keys verbatim. Added a unit
      test asserting `orange-book-typst` remains the `TestSpec.format` and parses
      `ensureTypstFileRegexMatches` without defaulting to `typst`.
- [x] Run the filtered smoke-all fixtures after Gordon authorizes execution of
      the copied upstream Typst extension code; verify the local override marker
      appears in `.typ` and the French localization assertions pass.
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto` after the render authorization is granted.

## Status

**Complete**, but see **2026-09-29 — crossref-index write race, root-caused and
fixed** at the end of this section: both fixtures regressed again after this
"Complete" mark, on a bug unrelated to anything P7 built.

All P7 tasks finished successfully.

- [x] **P6 Correctness Gap Fixes:**
  - Format selection now uses explicit test format override
  - Project cache better distinguishes per-file/merged/global outputs
  - Skip behavior correctly triggers before project discovery
  - Per-file failure attribution improved
- [x] Copy both fixtures\' tracked source files into
      `crates/quarto/tests/smoke-all/typst/{orange-book-lang,override-orange-book}/`.
      Excluded generated outputs and caches listed in each source fixture's
      `.gitignore`.
- [x] `parse_test_specs` preserves arbitrary format keys verbatim. Added a unit
      test asserting `orange-book-typst` remains the `TestSpec.format` and parses
      `ensureTypstFileRegexMatches` without defaulting to `typst`.
- [x] Run the filtered smoke-all fixtures after Gordon authorizes execution of
      the copied upstream Typst extension code; verify the local override marker
      appears in `.typ` and the French localization assertions pass.
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto` after the render authorization is granted.

## 2026-09-29 — crossref-index write race, root-caused and fixed

At `72ad0eda8`, `smoke_all::smoke_all` was red on both `orange-book-lang/index.qmd`
and `override-orange-book/index.qmd`, failing `noErrorsOrWarnings` on an
unexpected warning: `Error attempting to write crossref index`
(`resources/pandoc-filters/filters/crossref/index.lua:215`). Initially suspected
as an intermittent flake (both failures showed up on a cold Typst-package-cache
run); confirmed **not** a flake by reproducing deterministically.

**Root cause:** `insert_project_keys`
(`crates/quarto-core/src/pandoc_filters/params.rs`) points the
`crossref-index-file` filter param at `<project>/.quarto/crossref-index.json`,
but nothing ever creates the `.quarto/` directory before Pandoc's Lua filter
tries to `io.open(indexFile, "w")` — Lua's `io.open` never creates missing
parent directories, and every chapter in a book render independently invokes
Pandoc and writes to this same path. Locally this went unnoticed because a
`.quarto/` directory left over from a previous render of the same fixture
directory made the write succeed; deleting the fixture's `.quarto/` first
(`rm -rf crates/quarto/tests/smoke-all/typst/{orange-book-lang,override-orange-book}/.quarto`)
reproduces the failure on every run, cold-cache or not.

**Fix:** `crates/quarto-core/src/stage/stages/pandoc_write.rs`\'s `PandocWriteStage`
now `create_dir_all`s `<project>/.quarto/` before building the filter params
blob, whenever `!ctx.project.is_single_file` (the same condition
`insert_project_keys` already gates the param on). This makes the directory's
existence deterministic regardless of the order in which a book's chapters
render, rather than depending on some other chapter (or a stale directory from
a previous run) having created it first.

**Verification:**
- With both fixtures\' `.quarto/` deleted (cold): `SMOKE_FILTER=orange-book
  cargo nextest run -p quarto --test integration -- smoke_all` → **3 passed**
  (`orange-book/index.qmd`, `orange-book-lang/index.qmd`,
  `override-orange-book/index.qmd`), 7 skipped (book chapters), 0 failed — a log
  line confirms the cold-cache condition (`Downloading @preview/orange-book:0.7.1...`
  rather than `Skipping ... (cached)`).
- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: 5306 passed, 32 skipped, 0 failed — same
  run also covers the `#notefigure(` fix in the P5 doc's 2026-09-29 entry
  (`crates/quarto-core/src/transforms/float_ref_target.rs`); no unrelated diff.
- Full unfiltered `cargo nextest run -p quarto --test integration -- smoke_all`:
  **passes** — the smoke-all group's `smoke_all::smoke_all` test (bundling this
  fixture plus the 5 margin-table fixtures from the P5 doc's 2026-09-29 entry)
  went from 1 failure to 0.
