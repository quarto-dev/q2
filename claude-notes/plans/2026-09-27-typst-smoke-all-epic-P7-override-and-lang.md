# P7 — Port `override-orange-book` + `orange-book-lang`

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
`chapter1.qmd` is a real second chapter (`_quarto.yml`'s `book.chapters`) — this is
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

- [ ] Copy both fixture directories' **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/{orange-book-lang,override-orange-book}/`
      — not a literal directory copy. Q1's checkout carries generated/local cruft
      alongside the source (`.quarto/` caches, `_book/` pre-rendered output,
      `index.typ`, and in at least one of these fixture directories a stray
      `.claude/settings.local.json` that was accidentally committed into the
      `quarto-cli` checkout) — each fixture's own `.gitignore` lists what's
      generated; copy everything *except* those, don't copy the directory wholesale.
- [ ] Confirm P1's spec parser handles a non-`"typst"` format key
      (`orange-book-typst`) correctly — `parse_test_specs` already iterates every
      non-`run` key in `_quarto.tests` generically, so this should need no new code;
      write a test that would fail if it silently defaulted to `"typst"`.
- [ ] Confirm the local extension actually shadows the vendored builtin (the
      `LOCAL-OVERRIDE-MARKER` string must appear in the compiled `.typ`, proving
      `override-orange-book`'s local `_extensions/` won, not the vendored
      `resources/extension-subtrees/orange-book/`).
- [ ] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto`.

## Status

Not started.
