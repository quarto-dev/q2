# Fix issue #196: list-item continuation regression from PR #194 (bd-3mgb)

- **GitHub:** https://github.com/quarto-dev/q2/issues/196
- **Beads:** bd-3mgb (discovered-from bd-7l1u, the Q-2-35 implementation)
- **Worktree branch:** `issue-196`
- **Triage:** `claude-notes/issue-reports/196/triage.md`

## Goal

Restore correct parsing of list-item continuations when the intervening blank line carries ≥4 columns of trailing whitespace, **without weakening** the original Q-2-35 detection of true 4-space-indented code blocks added by PR #194.

A successful fix:

1. Makes the reporter's `repro.qmd` parse to `[ OrderedList ... [Para "Outer:", Para Image] ]` again.
2. Leaves every existing Q-2-35 positive case (`Q-2-35-basic.qmd`, `Q-2-35-more-than-four.qmd`, `Q-2-35-tab-indent.qmd`, `Q-2-35-indented-blockquote.qmd`) still emitting the friendly diagnostic.
3. Adds a tree-sitter corpus regression so the bug cannot return silently.
4. Passes full `cargo nextest run --workspace` and `cargo xtask verify --skip-hub-build`.

## Constraints

- The fix lives in the scanner (`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`). The Rust readers / error-corpus mapping are downstream of this signal and should not change.
- The PR description's "conservative gate" intent — the gate fires *only* at true block-start positions outside container-continuation contexts — is the right design. The current implementation undershoots that intent; we tighten it.
- No hacky workarounds (per CLAUDE.md). The fix must be a sound shift to the gate's domain, not a special case on `repro.qmd`.

## Phase 0 — Setup

- [x] Worktree exists at `.worktrees/issue-196/`, on branch `issue-196`. Triage commit landed.
- [x] Beads `bd-3mgb` claimed (`in_progress`).
- [x] Plan doc written (this file).

## Phase 1 — TDD: failing test first

Per CLAUDE.md, the test goes in first and must be observed failing before any code change.

- [x] Added three tree-sitter corpus cases to `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/qmd.txt` — ordered-list (image continuation), bullet-list (text continuation), and tab-indent variants. All three failed initially with the same shape: root `ERROR` node, list marker captured, first paragraph captured, second paragraph dropped.
- [x] Pure-Rust regression skipped — the tree-sitter corpus cases plus the end-to-end CLI verification in Phase 5 cover the same ground without duplicating fixtures across crates.

## Phase 2 — Diagnosis with evidence

The triage doc identified candidate fix angles. Phase 2's job was to pick the right one with evidence, not opinion.

- [x] Surveyed the 30 `s->indentation` reset/accumulate sites in `scanner.c`. Resets happen at line-ending (line 2293, 2495), at SOFT_LINE_ENDING (line 2375, 2470), inside container matchers, inside parser-specific paths, and at deserialization. The whitespace-consumption loop at line 2103 uses `+=`, so any stale value from a previous call persists across mid-line `scan()` invocations.
- [x] Confirmed the gate sits inside `static bool scan(...)` (the workhorse called from `tree_sitter_markdown_external_scanner_scan`). The function does NOT reset `s->indentation` on entry — the architecture relies on the line-ending reset to fire between lines.
- [x] Instrumented the gate with an `fprintf` dumping `(s->indentation, valid_symbols[ATX_H1_MARKER], valid_symbols[BLANK_LINE_START], valid_symbols[BLOCK_CONTINUATION], s->open_blocks.size, s->matched, s->column, s->state, lexer->lookahead)`. Ran the instrumented parser on (a) the reporter's `repro.qmd` (misfire), (b) `Q-2-35-basic.qmd` (legit top-level), (c) a synthesized in-list 8-space indent case (legit nested Q-2-35).
- [x] **Decision** — pick **(B)**, the BLOCK_CONTINUATION discriminator, refined to `valid_symbols[BLOCK_CONTINUATION] && s->open_blocks.size > 0`.

### Why (B) wins

The trace data showed a clean separator:

| Case                                       | indentation | ATX | BLANK | CONT | open_blocks | should fire? |
| ------------------------------------------ | ----------- | --- | ----- | ---- | ----------- | ------------ |
| Q-2-35 top-level (`Before.\n\n    foo`)    | 4           | 1   | 0     | **1**| **0**       | yes ✓        |
| Q-2-35 in-list (`4)  ...\n\n        bar`)  | 4           | 1   | 0     | **0**| 1           | yes ✓        |
| Misfire (`4)  ...\n    \n    !`)           | 4           | 1   | 0     | **1**| **1**       | no ✗         |

The misfire is uniquely identified by `BLOCK_CONTINUATION valid` *plus* `open_blocks.size > 0`. In that state the parser still owes the line a container-continuation absorption — the indent has not yet been confirmed as "extra". Either flag alone overcounts (BLOCK_CONTINUATION is also valid for legit top-level Q-2-35, where `open_blocks.size == 0`; `open_blocks.size > 0` alone would miss the in-list Q-2-35).

### Why (A) and (C) lose

- **(A)** Resetting `s->indentation = 0` at the top of `scan()` would break `match()` for LIST_ITEM (line 533–542), which expects `s->indentation` to accumulate across its own internal whitespace loop and reach `list_item_indentation` before subtracting. Across-the-board resets touch every container-matching path; the blast radius is the whole grammar.
- **(C)** Tracking "did the whitespace loop consume bytes this call" suppresses the recovery-state mid-line misfire but does NOT suppress the start-of-line misfire (where the loop *does* consume the 4 spaces because match_line was not entered — STATE_MATCHING was unset by the preceding blank-line path). Confirmed by the trace: the bad case at start of row 2 has `s->indentation = 4` *as the result of the loop running on this call*. So (C) alone is necessary but not sufficient.

(B) addresses the start-of-line misfire (BLOCK_CONTINUATION valid + open blocks → defer) AND the recovery-state mid-line misfire (BLOCK_CONTINUATION valid + open blocks → defer) without disturbing the legitimate paths.

## Phase 3 — Implementation

- [x] Edited `scanner.c` gate at line 2128 to add `!(valid_symbols[BLOCK_CONTINUATION] && s->open_blocks.size > 0)`. Replaced the explanatory comment block with one that records the discriminator from the trace and why the alternatives lose.
- [x] Ran `tree-sitter generate` to regenerate `parser.c`.
- [x] `tree-sitter test`: 483/483 (was 480 + 3 new tests). All four existing Q-2-35 positive cases (`Q-2-35: 4-space indent rejected`, `Q-2-35: tab indent rejected`, and corresponding negatives) still fire / pass unchanged. GFM Example 209 unchanged.

## Phase 4 — Workspace verification

- [x] `cargo nextest run -p pampa` — 3686/3686 passed, 2 skipped.
- [x] `cargo nextest run --workspace` — 8863/8863 passed, 195 skipped.
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests --skip-trace-viewer-build --skip-trace-viewer-tests` — all 9 steps green. (Hub-client / trace-viewer skipped because no `npm install` has been run in this clone; their builds need `vitest` / `tsc` which is a bootstrap issue independent of this fix.)

## Phase 5 — End-to-end CLI check

- [x] `cargo run --bin pampa -- < claude-notes/issue-reports/196/repro.qmd` →
  `[ OrderedList (4, Decimal, OneParen) [[Para [Str "Outer:"], Para [Image ( "" , ["border"] , [] ) [] ("img.png" , "")]]] ]` — exactly the pre-#194 shape the reporter cited.
- [x] Same correct shape for `exp-tab-on-blank.qmd`, `exp-bullet-list.qmd` (yields `BulletList` instead), and `exp-trailing-ws-text.qmd`.
- [x] All three real-world quarto-web files cited in issue #196 (diagrams.qmd, engine.qmd, netlify.qmd) parse cleanly end-to-end.
- [x] All four `Q-2-35-*.qmd` case files still emit the friendly `[Q-2-35] Indented code blocks are not supported` diagnostic.

## Phase 6 — Commit, sync, close

- [ ] Commit the fix on `issue-196` (separate from the triage commit).
- [ ] `br close bd-3mgb --reason "<one-line summary + commit hash>"` from main, sync, commit `.beads/issues.jsonl`.
- [ ] Stop. Ask the user before pushing.

## Out of scope

- Improving the Q-2-35 friendly-error mapping table to cover additional `(state, sym)` pairs. The reporter noted this as a secondary observation, but once we stop emitting `INDENTED_CODE_BLOCK_DISALLOWED` in the regression case, the mapping question becomes moot for this input. If a separate audit of recovery-state mappings is warranted, it gets its own beads issue.
- Refactoring or simplifying the broader scanner state machine. The 30 sites that touch `s->indentation` are evidence that this code is fragile, but a full audit is out of scope for a regression fix.
