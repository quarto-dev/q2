# Issue #964 — Incremental writer drops an editorial mark's explicit attribute when a sibling word is edited

- **GitHub**: https://github.com/quarto-dev/q2/issues/964
- **Reporter**: @davidrsch (David), 2026-10-29
- **Triage date**: 2026-10-30
- **Branch**: `issue-964`, topic branch in the main checkout (no worktree, per user request), based on `main` @ `1ea01e499`
- **Braid strand**: bd-87t8pnx6 (the fix); bd-01hx04kh (related: tiling auditor does not check id/class ranges)
- **Scope**: the single report in the issue. The reporter's cause analysis and suggested fix were both checked and are correct.

## Summary

Editing a sibling `Str` next to an untouched `[++ added]{author="Bob"}` makes the incremental writer emit `[++ added]` without the attribute. Reproduced at `main` @ `1ea01e499` for all four editorial mark types (`[++ ]`, `[-- ]`, `[!! ]`, `[>> ]`), for key/value attrs, for `{.cls}` and for `{#id}`. Real bug, small fix: the mark's own source span stops at the closing delimiter token and excludes the attribute that follows it, so the incremental writer's verbatim copy of the "unchanged" node is missing bytes. The attribution data (`author`/`date`) that tracked-changes workflows depend on is silently lost on every neighbouring edit.

## Reproduction

Fixture: `repro.qmd`. Recipe: `repro_test.rs` (drop into `crates/pampa/tests/integration/`, register in `main.rs`, run with the command below). Captured output: `repro-output.txt` and `repro-output-roundtrip-fixture.txt`.

The recipe parses the fixture with `pampa::readers::qmd::read` (the same reader `wasm_entry_points::qmd_to_pandoc` wraps), appends `Z` to the paragraph's first `Str`, runs `compute_reconciliation` and `incremental_write`, and also prints each top-level inline's source span and the tiling auditor's findings for the original.

| Input | Incremental output | Mark's recorded span |
|---|---|---|
| `Last [++ added]{author="Bob"} text.` | `LastZ [++ added] text.` | `5..15` = `[++ added]` |
| `Last [++ added]{author="Bob" date="2026-01-01"} text.` | `LastZ [++ added] text.` | `5..15` |
| `Last [-- gone]{author="Bob"} text.` | `LastZ [-- gone] text.` | `5..14` |
| `Last [!! hot]{author="Bob"} text.` | `LastZ [!! hot] text.` | `5..13` |
| `Last [>> note]{author="Bob"} text.` | `LastZ [>> note] text.` | `5..14` |
| `Last [++ added]{.cls} text.` | `LastZ [++ added] text.` | `5..15` |
| `Last [++ added]{#id} text.` | `LastZ [++ added] text.` | `5..15` |
| `Last [++ added] text.` (control) | `LastZ [++ added] text.` | `5..15` (correct) |
| `Last [added]{author="Bob"} text.` (Span control) | `LastZ [added]{author="Bob"} text.` | `5..26` (correct, includes attr) |

Expected in every attr row: only `Last` becomes `LastZ`; the mark and its attribute are byte-identical to the input.

The span column shows the mechanism directly: the following `Space` starts at `29` (after `}`), so the bytes `15..29` holding `{author="Bob"}` belong to no inline at all and the incremental writer never copies them.

On the real fixture `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/editorial_marks_author_date.qmd` the same edit drops the `author`/`date` attrs from all three top-level marks. The nested `[>> …]{author=…}` inside `[range …]{.note}` survives only because the enclosing Span's range is correct and the whole Span is copied verbatim.

## Localization

- `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:456` — `extract_delimiter_space_info`. The `adjusted_range` at `:539` is built from `first_delim.start + leading_ws` to `last_delim.end - trailing_ws`. `fallback_range` (the grammar node's full span, passed as `node_location(node)`) is only used when no delimiters are found, so an attribute after the closing delimiter is never part of the node's span.
- `crates/pampa/src/pandoc/treesitter_utils/editorial_marks.rs:36` — the `process_editorial_mark!` macro calls it for `Insert`/`Delete`/`Highlight`/`EditComment` and stamps `space_info.adjusted_range` on the node at `:84`. The `IntermediateAttr` child *is* collected into `attr`/`attr_source` at `:46`, so re-serialization writes it correctly; only the span is wrong.
- `crates/pampa/src/pandoc/treesitter_utils/span_link_helpers.rs:85` — `process_pandoc_span` uses `node_source_info_with_context(node, context)` (the full grammar node) and is therefore correct. This is the model for the fix.
- `crates/pampa/src/writers/incremental.rs:782` — `assemble_inline_content`, the inline-level `KeepBefore` copy that reproduces the drop. Not at fault: it copies exactly what the span says.
- Introduced by `a013fa7b4` (2026-01-08, "Fix missing Space nodes before editorial marks in Pandoc AST"), which added delimiter-space injection to the editorial marks.

## Open questions — resolved during triage

**Is the reporter's cause analysis right?** Yes. The span dump above shows the mark's span ending exactly at the closing `]` in every attr case, and the attr bytes belonging to no inline.

**Does the same defect affect emphasis/strong/strikeout/sub/superscript, which share `extract_delimiter_space_info` via `process_inline_with_delimiter_spaces`?** No in practice: those constructs cannot carry an attribute, so the grammar node ends at the closing delimiter and `fallback_range.end == last_delim.end`. The `*added*` control confirms the span is correct. The suggested fix (`max(delimiter-derived end, fallback_range.end)`) is therefore a no-op for them.

**Why did the existing tiling auditor not catch this?** Two independent gaps:
1. `audit_source_range_tiling` *does* check attr key/value containment for the four mark types (`incremental.rs:1552`–`1600`) and reports 2 `ContainmentViolation`s per kv attr on the repro (14 on the roundtrip fixture). But `tiling_corpus_tests.rs` `CORPUS_ROOTS` does not include `crates/pampa/tests/roundtrip_tests/`, which holds the only attr-bearing editorial-mark fixture in the tree; none of the audited roots contains one (`git ls-files '*.qmd' | xargs grep -lE '\[(\+\+|--|!!|>>)[^]]*\]\{'`). Folded into bd-87t8pnx6: add the root as the regression guard once the fix lands.
2. `audit_attr_source` (`incremental.rs:~1620`) only builds audit units for `attr_source.attributes`; id and class ranges are never checked. That is why the `{.cls}` and `{#id}` rows report 0 findings despite having the same span defect. Filed as bd-01hx04kh.

**Does the whitespace-injection behaviour that `a013fa7b4` added need to change?** No. Trailing whitespace trimming only applies to bytes inside the closing delimiter token, which always precedes any attribute; the two concerns are independent, exactly as the reporter says. `tests/integration/test_editorial_mark_spacing.rs` is the regression surface to keep green.

**Is this a duplicate?** No. `braid search` for "editorial mark", "incremental writer", "delimiter", "tiling" found no open strand on this span. bd-6392eba3 (open, `inline_source_span` accessor misuse) is a different defect in the same file and is not involved here (the spans above are plain `Original` ranges).

## Outcome / recommended next step

Filed bd-87t8pnx6 (P1 bug) with this fix scope:

1. In `extract_delimiter_space_info`, set `adjusted_range.end` to the later of `last_delim.end - trailing_ws_count` and `fallback_range.end` (row/column from whichever wins). Equivalent: when the node extends past the closing delimiter, use the node's true end.
2. Regression test in `tests/integration/incremental_writer_tests.rs` from the recipe in `repro_test.rs`, covering all four mark types plus `{.cls}`/`{#id}`.
3. Add `crates/pampa/tests/roundtrip_tests` to `CORPUS_ROOTS` in `tiling_corpus_tests.rs`; it must go from 14 findings to 0 with the fix.
4. Keep `test_editorial_mark_spacing.rs` and the qmd round-trip suites green.

Related, lower priority: bd-01hx04kh extends the auditor to id/class ranges.

## Verification commands used

```bash
cargo xtask verify --skip-hub-build                      # pre-flight on main @ 1ea01e499 (see note below)
gh issue view 964 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
git switch -c issue-964
# repro: copy repro_test.rs to crates/pampa/tests/integration/zz_issue_964_scratch.rs,
# add `pub mod zz_issue_964_scratch;` to main.rs, then
cargo nextest run -p pampa --test integration zz_issue_964_scratch --no-capture
git log --oneline -S"adjusted_range" -- crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs
git ls-files '*.qmd' | xargs grep -lE '\[(\+\+|--|!!|>>)[^]]*\]\{'
braid search "editorial mark"; braid search "incremental writer"; braid search "tiling"
```

Pre-flight result: all Rust gates and all 17541 nextest tests passed. The run exited 1 on one vitest in `ts-packages/quarto-sync-client` (`file-collision-recovery.test.ts`, "spaces recoveries by the cooldown", a 150 ms sleep-based timing assertion) while a second `cargo xtask verify` from another session was competing for the machine. Re-run in isolation three times with `fnm exec --using=24 npx vitest run src/file-collision-recovery.test.ts`: 3/3 pass. Load-sensitive flake, unrelated to pampa or this issue; not investigated further here.

## Cross-references

- bd-87t8pnx6 — the fix (this issue)
- bd-01hx04kh — tiling auditor id/class coverage gap, found here
- bd-rcdo (closed) — earlier "InlineSplice silently drops header attribute changes"; same symptom class, different cause
- bd-1d6io / `tiling_corpus_tests.rs` header — the previous "the auditor existed one function call away" story this repeats
- `claude-notes/designs/provenance-contract.md` — P4 containment is the invariant violated here
- `crates/pampa/AGENTS.md` — TDD round-trip workflow for the fix
