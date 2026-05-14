# Plan: Q-2-35 — Reject 4-space indented code blocks with a high-quality error

- **GH issue:** [#184](https://github.com/quarto-dev/q2/issues/184)
- **Triage:** `claude-notes/issue-reports/184/triage.md`
- **Beads:** bd-7l1u
- **Branch:** `issue-184`
- **Approach:** mirror the existing Q-2-32 (`TRIPLE_STAR`) pattern — scanner emits an external token that no grammar rule consumes, the resulting `(state, sym)` pair is mapped to a user-facing message through the Merr-style error table.

## Overview

CommonMark indented code blocks (4 leading spaces) are deliberately **not** supported in qmd. Today the parser silently consumes the leading whitespace and emits a sequence of paragraphs at column 0, which corrupts the document on a qmd → qmd round-trip (issue #184). This plan adds a parse error that fires whenever the scanner sees 4+ "leftover" leading spaces (i.e. indentation beyond what the enclosing block container required) at the start of a content line.

The fix is small and additive — no grammar rules change shape — because we route through the same machinery already in place for `***` triple-star (Q-2-32). Lifting Q-2-32 has shown us that this pattern is the right one for "construct lexically detectable, semantically forbidden."

## Test plan (TDD — write first, watch fail, then fix)

Because this is fundamentally a Merr-style error addition, the **error-corpus snapshot tests** are the failing-test artifact: a new `Q-2-35.json` plus the `crates/pampa/tests/test_error_corpus.rs` harness automatically produce text and JSON snapshot files. Before any scanner change, those snapshots will be either absent (test discovers the case but cannot capture an error) or capture the current buggy "no error, parsed as paragraph" state. After the scanner change they capture the expected diagnostic.

In addition to the error-corpus harness, add focused regression tests so the test surface is durable:

- [x] **Phase 0 — Failing-test scaffolding (before any scanner / Rust changes)**
  - [x] Add `crates/pampa/resources/error-corpus/Q-2-35.json` with cases listed below. Wrote four positive cases (`basic`, `tab-indent`, `more-than-four`, `inside-list-item`); the planned negative `well-indented-list` case can't live in the error corpus (the harness panics if a case file parses successfully) so it became a separate Rust regression test in Phase B.
  - [x] Run `./scripts/build_error_table.ts` — surface failure mode. **Result**: `Case file: resources/error-corpus/case-files/Q-2-35-basic.qmd didn't produce errors` followed by `SyntaxError: Unexpected end of JSON input` from `JSON.parse(outputStdout)`. The `--_internal-report-error-state` invocation produced empty stdout because the parser succeeded. (Aside: deno was missing on this machine; installed via `brew install deno`.)
  - [x] Run `cargo nextest run -p pampa --test test_error_corpus`. **Result**: `test_error_corpus_ariadne_output` and `test_error_corpus_json_locations` both FAILED with `Expected resources/error-corpus/case-files/Q-2-35-basic.qmd to produce errors, but it parsed successfully` at `crates/pampa/tests/test_error_corpus.rs:71`. (The two snapshot tests glob `*.qmd` directly under `error-corpus/`, not `case-files/`, so they're unaffected by Q-2-35 case files. They'll be exercised in Phase B once the table regenerates cleanly.)
- [ ] **Phase 1 — Round-trip regression test**
  - [ ] Add a roundtripping test (or refresh an existing snapshot fixture) showing that the reporter's input now yields a parse error rather than silent rewriting. Use `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/` or the closest existing harness; if "expected error on parse" cases are not yet supported by that harness, add the test under `crates/pampa/tests/` as a direct unit test against the reader.
- [ ] **Phase 2 — Tree-sitter corpus test**
  - [ ] In `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/` add a test case asserting that 4-space-indented content produces an `ERROR` node containing the new token (the existing TRIPLE_STAR corpus entry is the template, find it via `grep -rn TRIPLE_STAR test/corpus`).
  - [ ] Run `tree-sitter test` from `crates/tree-sitter-qmd/tree-sitter-markdown/` to confirm the test currently fails (before scanner change) and passes after.

### Error-corpus cases for `Q-2-35.json`

Each case is a small `.qmd` chosen to exercise the detection logic at a distinct location:

| Case `name`           | Description                                                                  | Why it matters                                                                       |
| ---                   | ---                                                                          | ---                                                                                  |
| `basic`               | 4 spaces + plain text at top level after a blank line                        | The straightforward reporter case.                                                   |
| `tab-indent`          | A single leading tab (which expands to column 4)                             | Confirms tab handling agrees with `advance()`'s 4-column expansion in scanner.c.    |
| `more-than-four`      | 5–8 leading spaces at top level                                              | "4 or more" boundary.                                                                |
| `inside-list-item`    | Inside a list item: list marker, then continuation line with **extra** 4-space indent beyond the list-item indent | Confirms the check fires on **leftover** indentation, not raw column count.        |
| `well-indented-list`  | Continuation line whose indentation **exactly** matches the list-item indent | **Negative** case (no error). Should be a "captures": [] test that expects success. The harness needs to accept negative cases — if it doesn't, file a follow-up beads. |
| `after-paragraph`     | 4-space-indented line directly following a paragraph (CommonMark would call this a "lazy continuation"; we still want to reject it) | Lazy-continuation interaction. |

Use `crates/pampa/resources/error-corpus/Q-2-32.json` as the structural template. The `captures` field points at the disallowed leading whitespace span.

## Work items

### Phase A: tree-sitter scanner & grammar

- [ ] Add a token to the `TokenType` enum near the existing `TRIPLE_STAR` (e.g. `INDENTED_CODE_BLOCK_DISALLOWED`) in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`.
- [ ] Add the matching entry to the debug name array (the array at line 234).
- [ ] Implement the detection. Likely site: after the existing block matchers have run and consumed their share but **before** the fall-through that emits the paragraph token, check `s->indentation >= 4`. Use the existing emitters at lines 742, 868, 888, 933, 1007, 1081 (all gated `s->indentation <= 3`) as the structural template — the new emitter is the "else" branch they all share. Care points:
  - Must run only at line-start (not mid-line).
  - Must run after `match()` has reduced `s->indentation` by the container's required indent (lines 506-552), so that a list continuation that is *correctly* indented is **not** flagged.
  - Must NOT fire inside a fenced code block (`s->fenced_code_block_delimiter_length > 0`) or inside other "raw content" contexts that already shadow normal line scanning.
  - Must NOT fire on blank lines (lines that are all whitespace).
  - Tab handling: `s->indentation` is already in column units thanks to `advance()`, so the check is `s->indentation >= 4`, not "byte count ≥ 4".
- [ ] Add the external token to `grammar.js`'s `externals` list with a `_disallowed`-style name following the `$._triple_star_error` precedent (lines 1045-1052). Add a comment block mirroring the Q-2-32 one (file the comment under "KNOWN LIMITATION:").
- [ ] Run `tree-sitter generate; tree-sitter build` from `crates/tree-sitter-qmd/tree-sitter-markdown/`.
- [ ] Run `tree-sitter test` and confirm the new corpus case (Phase 2 above) passes.

### Phase B: error message wiring

- [ ] Write `crates/pampa/resources/error-corpus/Q-2-35.json` with the cases listed in the Test Plan.
- [ ] Run `crates/pampa/scripts/build_error_table.ts` (deno hashbang) — regenerates `case-files/` and `_autogen-table.json`.
- [ ] Run `cargo nextest run -p pampa --test test_error_corpus`. The snapshot files under `crates/pampa/snapshots/error-corpus/text/` and `.../json/` should be created or updated. Review with `cargo insta review` if `insta` is configured; otherwise inspect the diff manually.
- [ ] If `lookup_error_entry` returns multiple matches for the same `(state, sym)` (because the same parse state is hit by other unrelated error corpora), follow the disambiguation pattern from Q-2-32 — see `crates/quarto-parse-errors/src/error_table.rs:65-94`.

### Phase C: documentation

- [ ] Add a Known Limitations entry in `crates/tree-sitter-qmd/tree-sitter-markdown/CONTRIBUTING.md` (find the Q-2-32 entry; add a sibling entry).
- [ ] In `grammar.js`, write a comment block next to the new external matching the style of the Q-2-32 comment at lines 1045-1051.
- [ ] If `docs/syntax-notes.md` (or any user-facing doc under `docs/`) discusses code-block syntax, add a sentence noting that indented code blocks are unsupported and pointing at fenced code blocks. Skip if no obvious user-facing location exists; do not invent a new page.

### Phase D: full-stack verification

- [ ] `cargo xtask verify --skip-hub-build` from the worktree.
- [ ] Run `cargo run --bin pampa -- claude-notes/issue-reports/184/repro.qmd` and confirm the new diagnostic appears, with the leading whitespace span highlighted. Capture the exact CLI output and paste it back into this plan document under a "Verification output" section so future readers can see the realized state.
- [ ] Inspect that `cargo run --bin pampa -- claude-notes/issue-reports/184/repro.qmd -t qmd` now also errors (rather than silently rewriting). The qmd writer path should not run when parse errors are present, but verify.
- [ ] Confirm no unrelated snapshot tests changed. If they did, surface the count and a one-sentence summary in the eventual commit message (per CLAUDE.md "Snapshot Test Changes").

### Phase E: commit & sync

- [ ] Commit the implementation on the `issue-184` branch.
- [ ] From the **main** repo (not the worktree), run `br sync --flush-only && git add .beads && git commit -m "sync beads"` to commit the beads JSONL changes.
- [ ] Wait for explicit user approval before pushing.

## Out of scope (do not creep)

- Changing the qmd writer to *emit* indentation back. The writer is fine; the problem is that the reader silently accepts a construct that's not in our grammar. Fix the reader, not the writer.
- Adding a "warning-only" mode or a `--loose` flag that downgrades this to a warning. Project policy per the GH issue comment is *error*, not warning.
- Generic "any disallowed lexical construct" framework. We already have Q-2-32 as the one-of-a-kind precedent; a generalisation is premature with only two examples.
- Touching `s->column` semantics or the `advance()` tab-expansion rules — they are correct as-is; we are only adding a *reader* of the existing `s->indentation`.

## Verification output

### Reporter's exact repro through `cargo run --bin pampa`

Invocation:

```
$ cargo run --bin pampa -- claude-notes/issue-reports/184/repro.qmd
```

Output (ANSI escapes stripped, layout preserved):

```
Error: [Q-2-35] Indented code blocks are not supported
   ╭─[ claude-notes/issue-reports/184/repro.qmd:3:1 ]
   │
 3 │     categories:
   │ ───────┬───────
   │        ╰─────────── Quarto Markdown does not support 4-space indented
   │                     code blocks. Use a fenced code block (```) instead,
   │                     or remove the leading indentation.
───╯
```

The diagnostic spans the entire offending line — both the four leading
spaces and the `categories:` content — pointed back to the start of line
3 (per the user's request to "nudge" the highlight onto the indentation
rather than the first non-whitespace character). The widening lives in
the QMD-specific layer (`crates/pampa/src/readers/qmd_error_messages.rs`,
`widen_diagnostic_to_line`) so the scanner stays minimal and the generic
parse-error system is unaffected. Output was inspected directly.

### Test totals

- `tree-sitter test` (in `crates/tree-sitter-qmd/tree-sitter-markdown/`):
  **480/480 pass**, including the four new Q-2-35 cases and the updated
  GFM Example 209 (now expects an `ERROR` because Q2 deliberately rejects
  indented blockquotes).
- `cargo nextest run -p pampa --test test_error_corpus`: **4/4 pass**
  (ariadne text, JSON locations, text snapshots, JSON snapshots — all
  with the new `Q-2-35-*.qmd` case files).
- `cargo nextest run -p pampa`: **3687/3687 pass**, no regressions.
- `cargo xtask verify` (full, including hub-client WASM build and
  hub-client tests after `npm install`): **All verification steps passed**.
- `cargo xtask lint`: **705 files checked, all clean.**

### Snapshot-test impact

`test_error_corpus_text_snapshots` and `test_error_corpus_json_snapshots`
created new snapshot files under `crates/pampa/snapshots/error-corpus/`
for the Q-2-35 entries (none of the existing snapshots were modified).
Per CLAUDE.md, these will be enumerated in the commit message.
