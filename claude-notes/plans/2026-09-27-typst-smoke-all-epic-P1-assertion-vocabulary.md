---
title: 'P1 — Assertion vocabulary: `ensureTypstFileRegexMatches` + `ensurePdfRegexMatches`'
date: 2026-09-27
description: 'Adds `ensureTypstFileRegexMatches` and `ensurePdfRegexMatches` assertions to the smoke-test spec, reusing the existing two-array match shape and reading PDF text in-process through `pdf_extract` instead of a binary.'
---

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md)
**Depends on:** nothing. Can start immediately, in parallel with P2.
**Worktree:** `workspace-3` (bootstrap phase — see epic's "Parallel development
plan"). P1 and P2 have no dependency on each other; do them in either order, but
sequentially in this one worktree (only one branch checked out at a time).

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
# One-time, only if it doesn't exist yet:
git rev-parse --verify feature/typst-testing 2>/dev/null || \
  git checkout -b feature/typst-testing explore/typst-smoke-all-epic
git checkout -B typst-testing/p1-assertion-vocabulary feature/typst-testing
```

Implement the checklist below, gating each task on `cargo clippy -p quarto-test
--all-targets -- -D warnings` + `cargo nextest run -p quarto-test`. When done, flip
this doc's checklist to `[x]` and its `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git rebase feature/typst-testing               # pick up anything P2 merged meanwhile
git checkout feature/typst-testing
git merge --ff-only typst-testing/p1-assertion-vocabulary   # retry from rebase if this isn't a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Scope

Q2's `crates/quarto-test/src/spec.rs` already supports `ensureFileRegexMatches`
(`spec.rs:201`), which implements Q1's two-array match/no-match convention
(`spec.rs:343-354`):

```yaml
ensureFileRegexMatches:
  - ["match1", "match2"]      # must match
  - ["noMatch1", "noMatch2"]  # must NOT match (optional)
```

This is the same shape Q1's `ensureTypstFileRegexMatches` and `ensurePdfRegexMatches`
use (`external-sources/quarto-cli/tests/verify.ts:814-821,835-853`) — both delegate to
a shared two-array-match helper. This phase is a reuse of that existing shape against
two new text sources, not new design.

`ensureTypstFileRegexMatches(file, matches, noMatches, inputFile?)` — Q1's version
reads the **kept `.typ` intermediate** (requires `keep-typ: true`; Q1's own code has a
`// FIXME: do this properly without resorting on file having keep-typ` comment — even
Q1 considers this a hack, but it's what every fixture uses). The optional 4th
`inputFile?` param exists in Q1 to disambiguate a per-chapter intermediate `.typ` (a
book project renders each chapter's own `index.typ` at the project root, not a single
merged file) from the file the assertion's own front matter lives in; no fixture
passes it explicitly today. **Q2 drops it**: `render_book_single_file` merges an
entire book into one synthetic document and writes one `.typ`+`.pdf` pair together in
`_book/` (see P6/P2), so there's no per-chapter intermediate to disambiguate —
`output_path.with_extension("typ")` (already used by `book_numbering_torture.rs` et
al., including for book fixtures) is sufficient on its own. Q2's Rust integration
tests already do the equivalent directly — same mechanism, just needs exposing
through the spec DSL.

`ensurePdfRegexMatches(file, matches, noMatches)` — Q1 shells out to the `pdftotext`
binary on PATH (`verify.ts:845-853`). **Q2 must use `pdf_extract::extract_text()`
instead** — no new subprocess/binary dependency, and matches what the six existing
Rust book tests already do (`book_appendix_letter_parity.rs:143,209`,
`book_numbering_torture.rs:225`). `pdf-extract` is already used elsewhere in the
workspace (`quarto-core`, dev-dependency-scoped — see P2), but not yet by
`quarto-test` itself; this phase adds it there too (see checklist).

## WASM / cross-platform compliance

This phase adds `pdf-extract` to `quarto-test` as a **non-dev** dependency (see
checklist) — worth an explicit note since P2/P3 both need one for their own
`pdf-extract` usage. Verified safe: `wasm-quarto-hub-client/Cargo.toml` has no
`quarto-test` dependency, and structurally can't — `quarto-test` depends on
`quarto-core`, not the reverse. No `.claude/rules/wasm.md` action needed.

## Checklist

- [x] Add `ensureTypstFileRegexMatches` assertion type to `spec.rs`\'s parser, reusing
      the shared two-array parsing logic for `ensureFileRegexMatches`, `ensureCssRegexMatches`,
      and the new Typst/PDF assertions, against the render output's `.typ` sibling path
      (`output_path.with_extension("typ")`). Fail clearly if the intermediate is missing,
      with guidance to set `keep-typ: true`.
- [x] Add `pdf-extract = "0.7"` as a non-dev `quarto-test` dependency and update `Cargo.lock`.
      During this bootstrap phase, Cargo resolves the registry package once for both crates;
      P3 will re-point `quarto-test` at P2's git-fork revision for MCID support. No
      `pdf_extract::*` types cross the `quarto-core`/`quarto-test` boundary.
- [x] Add `ensurePdfRegexMatches` assertion type using `pdf_extract::extract_text(output_path)`.
- [x] Add inline module tests covering successful required-pattern checks, forbidden-pattern
      rejection, and the Typst missing-intermediate error. PDF tests create minimal synthetic
      PDFs with `lopdf` re-exported by `pdf-extract`.
- [x] Test that `_quarto.tests.run.skip` parses and yields the configured skip reason alongside
      both new assertion types for a chapter path.
- [x] `cargo clippy -p quarto-test --all-targets -- -D warnings` and `cargo nextest run -p
      quarto-test` pass (75/75 tests).

## Status

Complete.
