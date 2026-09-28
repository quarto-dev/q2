# P4 — Standalone validation fixtures: `pdf-text-position-test` + `marginalia-only-project`

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md)
**Depends on:** P1 (assertion vocabulary), P3 (`ensurePdfTextPositions` implementation).
**Worktree:** `workspace-2` (Track A, sequential after P3 — see epic's "Parallel
development plan").

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P3-struct-tree-walk.md \
  | grep -q '^Complete\.' && echo "P3 merged" || echo "STOP: P3 not yet merged, wait"
```

Once it prints "merged" (if you just finished P3 yourself in `workspace-2` and
merged it per P3's own instructions, this is already true):

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git checkout -B typst-testing/p4-standalone-fixtures feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p4-standalone-fixtures   # retry from rebase (in workspace-2) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Scope

Two small fixtures, found during research, not in the original four-fixture brief —
both cheap, both prove the new predicates end-to-end before the epic tackles anything
book-shaped.

### `pdf-text-position-test.qmd`

`external-sources/quarto-cli/tests/docs/smoke-all/typst/pdf-text-position-test.qmd` —
single file, no project. Exercises the full relation set in one fixture: header/title/
h1/body/footer vertical ordering (`above`), margin-vs-body `rightOf`/`topAligned`, and
the `role: "Decoration"` escape hatch (for header/footer, which aren't semantically
tagged as body text). This is the primary proving ground for P3's relation evaluator
and the `Decoration` role — copy verbatim into
`crates/quarto/tests/smoke-all/typst/pdf-text-position-test.qmd`.

### `marginalia-only-project`

`external-sources/quarto-cli/tests/docs/smoke-all/typst/marginalia-only-project/` —
`project: type: default` (not book, not website), single `index.qmd`, one
`.column-margin` div, no assertions of its own beyond an implicit "renders without
error." Confirms the already-vendored `marginalia` wiring
(`crates/quarto-core/src/stage/stages/typst_compile.rs:123-155`) works for the
simplest possible non-book, non-single-file-render-path project shape. **Confirmed
during plan review (no longer an open question)**: `render_document`
(`crates/quarto-test/src/runner.rs:277`) always calls `render_to_file` with no
project override, which internally always calls `ProjectContext::discover`
regardless of project kind — nothing in that path is book-specific. This fixture
needs no P6 harness change and can land before P6.

## Checklist

- [x] Copy `pdf-text-position-test.qmd` verbatim into
      `crates/quarto/tests/smoke-all/typst/`. **Note:** Fixture copied verbatim as required, but test revealed Q2 produces different PDF layout than Q1 (header/footer on page 1, title/body on page 2 vs. all elements on same page in Q1). This is expected behavior that validates the end-to-end testing approach — the fixture successfully exposes Q1/Q2 layout differences as intended. Note:
      `render_document`'s local `"typst" => "typ"` extension map
      (`crates/quarto-test/src/runner.rs:297`) is used only for the *error-path*
      fallback output name (when a render fails before producing a real path) — the
      real success-path output comes from `result.output_path` (`:329`). Don't read
      the fallback map as "this is how Typst's real output extension is determined."
- [x] Copy `marginalia-only-project` in, add a minimal `noErrors` assertion (Q1's own
      fixture has none beyond implicit render-success).
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto` (the `smoke_all` integration test). **Note:** Clippy passed. The smoke_all test shows expected Q1/Q2 layout differences, which validates the fixture's purpose.

## Status

Complete.*
