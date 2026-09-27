# P1 — Assertion vocabulary: `ensureTypstFileRegexMatches` + `ensurePdfRegexMatches`

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

- [ ] Add `ensureTypstFileRegexMatches` assertion type to `spec.rs`'s parser, reusing
      the `ensureFileRegexMatches` two-array parsing logic (`spec.rs:343-354`) against
      the render output's `.typ` sibling path (`output_path.with_extension("typ")`).
      Fail clearly if `keep-typ: true` was not set (mirrors Q1's own hack, documented
      as a hack, not silently wrong).
- [ ] **Expected, benign transient state**: once this item lands, `Cargo.lock` will
      carry two separate `pdf-extract` `[[package]]` entries (one registry-sourced
      for `quarto-core`'s existing dev-dependency, one for `quarto-test`'s new one)
      until P3 re-points `quarto-test` at P2's git rev too. Verified this compiles
      fine — Cargo's `SourceId`-based resolution treats same-name-different-source
      as genuinely distinct crates by design, no unification is attempted, and
      nothing here passes a `pdf_extract::*` type across the `quarto-core`/
      `quarto-test` boundary (both only call `extract_text()` → `String`). If
      `cargo tree` shows a duplicate `pdf-extract` during this window, that's
      expected, not a bug to chase.
- [ ] `quarto-test`'s own `Cargo.toml` has no `pdf-extract` dependency at all today
      (confirmed by grep — only `quarto-core`'s does, as a `[dev-dependencies]`
      entry for its own test files). This assertion type lives in `quarto-test`'s
      `src/` (production code of that crate, not a test file), so it needs
      `pdf-extract` added as a genuine, non-dev dependency of `quarto-test` itself.
      Pin it to crates.io `"0.7"` (matching `quarto-core`'s pre-P2 version) for
      now — this assertion type only needs plain `extract_text`, not P2's MCID fork,
      so it doesn't need to wait for P2. **P3 will need to re-point this same
      dependency at P2's git-fork rev** once it starts (to get MCID surfacing in
      `quarto-test` too) — note that hand-off explicitly in P3 rather than
      discovering the stale crates.io pin mid-P3.
- [ ] Add `ensurePdfRegexMatches` assertion type, same two-array shape, against
      `pdf_extract::extract_text(output_path)`.
- [ ] Both assertion types need an explicit test (not just exercised transitively
      via P4/P5's fixtures) — a minimal synthetic `.qmd` + `.typ`/`.pdf` fixture,
      matches + non-matches. `quarto-test` has no `tests/` directory at all; every
      existing assertion type is tested via an inline `#[cfg(test)] mod tests` in
      the module that implements it (see `assertions/file_regex.rs` and siblings).
      Add the new tests the same way, in the modules implementing
      `ensureTypstFileRegexMatches`/`ensurePdfRegexMatches`, not a new top-level test
      file.
- [ ] Confirm `_quarto.tests.run.skip` (already supported per `spec.rs:24,55-57,89`)
      composes correctly with these new assertion types for a chapter file — no new
      code expected here, just a confirming test.
- [ ] `cargo clippy -p quarto-test --all-targets -- -D warnings` + `cargo nextest run
      -p quarto-test`.

## Status

Not started.
