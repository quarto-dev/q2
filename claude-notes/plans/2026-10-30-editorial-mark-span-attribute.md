---
title: 'Editorial mark span excludes its trailing attribute (bd-87t8pnx6)'
date: 2026-10-30
description: 'The incremental writer dropped author/date attributes from untouched editorial marks because the mark''s source span ended at the closing delimiter; the span now extends to the end of the grammar node.'
status: done  # PR #990 merged 2026-10-30; follow-ups bd-01hx04kh + bd-nq3ed2im on branch braid/bd-01hx04kh-tiling-auditor-id-class
braid:
  strand: bd-87t8pnx6
---

**Branch:** `issue-964` (topic branch in the main checkout, based on `main` @ `1ea01e499`)
**Triage:** `claude-notes/issue-reports/964/triage.md` (GH #964)

## Verdict

Ready and implemented in the same session: the user confirmed the triage diagnosis and asked to proceed. The fix is a clamp of one range end; the bulk of the change is tests.

## Issue context

GH #964 (davidrsch, 2026-10-29). Editing a `Str` next to an untouched `[++ added]{author="Bob"}` makes the incremental writer emit `[++ added]` with no attribute. Affects all four editorial marks (`[++ ]`, `[-- ]`, `[!! ]`, `[>> ]`) and kv, class and id attributes. Tracked-changes attribution is silently lost on every neighbouring edit. P1 bug, filed from the triage on 2026-10-30.

## Dependency graph

- **related:** bd-01hx04kh — the tiling auditor only checks kv ranges, never id/class, which is why the `{.cls}`/`{#id}` variants were invisible to it. Not fixed here.
- **discovered-from (outgoing):** bd-nq3ed2im — 16 `AttrAlignmentSkipped` census findings over 11 `roundtrip_tests/qmd-json-qmd` fixtures, surfaced when this plan widened the tiling corpus. Pinned in `KNOWN`, not fixed here.
- No incoming `blocks` edges. No `discovered-from` parent: the strand was filed directly from the GH triage.

## What the code looked like

`extract_delimiter_space_info` in `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs` built `adjusted_range.end` from the closing delimiter token minus trailing whitespace, and used `fallback_range` (the full grammar node) only when no delimiter was found. For an editorial mark with an attribute the grammar node runs to the closing `}` but the span stopped at `]`, so the attribute bytes belonged to no inline and the inline-level `KeepBefore` copy in `assemble_inline_content` never wrote them. `process_pandoc_span` was already correct because it takes the node range directly. Introduced by `a013fa7b4` (2026-01-08). Repro and span dumps: `claude-notes/issue-reports/964/`.

## Phases

- **Phase 0 — tests first.** `editorial_mark_attribute_survives_sibling_edit` (nine input/expected pairs covering the four marks, kv/class/id attrs, and two controls), `editorial_mark_span_covers_trailing_attribute` (the mark's span runs to `}` and the following `Space` starts there), and `editorial_mark_attribute_survives_sibling_edit_roundtrip_fixture` (the real `editorial_marks_author_date.qmd`), all in `tests/integration/incremental_writer_tests.rs`. Failed before the fix, pass after.
- **Phase 1 — the fix.** In `extract_delimiter_space_info`, the end of `adjusted_range` is the later of the delimiter-derived end and `fallback_range.end`. Trailing-whitespace trimming applies only inside the closing delimiter token, which always precedes an attribute, so the two concerns stay independent. Emphasis, strong, strikeout, sub/superscript go through the same function but cannot carry attributes, so for them the node end equals the delimiter end and nothing changes; `test_editorial_mark_spacing.rs` and the emphasis suites stay green.
- **Phase 2 — regression guard.** `crates/pampa/tests/roundtrip_tests/qmd-json-qmd` added to `CORPUS_ROOTS` in `tiling_corpus_tests.rs`. Before the fix the auditor reported 14 `ContainmentViolation`s on the editorial-marks fixture there; after, zero. Only the `qmd-json-qmd/` subdirectory is a live fixture set (the roundtrip glob in `test.rs`); the loose files above it use unsupported syntax and are consumed by nothing, so they are not audited. The 16 pre-existing census findings in that directory are pinned in `KNOWN` under bd-nq3ed2im.
- **Phase 3 — verification.** `cargo clippy -p pampa --all-targets -- -D warnings` clean. Affected pampa suites (309 tests: editorial marks, tiling, incremental writer, inline splice, roundtrip) green. Full `cargo xtask verify --skip-hub-build` run before commit. No platform-specific code, so no Windows cross-check needed.

## Open questions

None blocking. Two follow-ups were filed rather than folded in, and both were then implemented on `braid/bd-01hx04kh-tiling-auditor-id-class` (2026-10-30, after PR #990 merged):

1. bd-nq3ed2im — the 16 census findings reduced to four producers (anchor shorthand, URI/email autolink, math-with-attribute Span, note-reference Span, and the list-table Div to Table lowering). Each now keeps `AttrSourceInfo` aligned with `Attr`, using `None` placeholders for synthesized components and filtering the sidecar in step with the attr for list-table. `KNOWN` in `tiling_corpus_tests.rs` is empty. Six JSON snapshots changed only by `classes: []` becoming `[null]`.
2. bd-01hx04kh — `audit_attr_source` now checks id and class ranges with the same tightness, containment and disjointness checks as kvs. Identical class ranges from `class="a b"` (bd-0vfgz2cl) count as one unit. Zero new findings across the 307-document corpus; against the pre-#964 reader it reports the `{.cls}` and `{#id}` variants. Five tests in `tiling_phase3_tests.rs`.

## Risks

- The clamp uses `fallback_range.end` only when it is strictly past the delimiter end, so documents without attributes are byte-for-byte unchanged in span terms. The one behavioural change is that an editorial mark's span now includes its attribute, which is what every other attr-bearing inline already did.
- `KNOWN` grew from 1 to 17 entries in PR #990. The follow-up branch above empties it.
