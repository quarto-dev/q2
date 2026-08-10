# Grammar html_entity_regex() mangles legacy no-semicolon entity names (bd-v8qc9zyc)

**Date:** 2026-08-10
**Braid:** bd-v8qc9zyc (bug, P3), discovered-from bd-named-entities-w6xbfftj
**Checkout:** main (stacked on the combining-marks work, PR #489)
**Status:** Implemented 2026-08-10; full verify green.

## Problem

`html_entity_regex()` in `crates/tree-sitter-qmd/common/common.js` builds the
`entity_reference` regex alternatives with `name.substring(1, name.length - 1)`.
That strips `&`…`;` correctly for the 2,125 semicolon-terminated keys of
`html_entities.json`, but the WHATWG table also carries 106 legacy keys
*without* a trailing semicolon (`&AMP`, `&AElig`, …), whose last **letter**
gets stripped instead — producing bogus alternatives (`AM`, `AEli`, …). Net
effect (verified against the current parser):

- `&AM;` lexes as `entity_reference` (then hits the converter's verbatim
  fallback — output text is right, but the CST is wrong and the regex carries
  106 garbage alternatives);
- bare `&AMP` does **not** match — which is correct: CommonMark recognizes
  only semicolon-terminated entities in markdown, so the legacy keys should
  not be in the regex at all.

No user-visible AST change results from the fix (`&AM;` merges to the same
`Str "&AM;"` either way); this is grammar hygiene, pinned at the CST level.

## Fix

Filter the keys to semicolon-terminated ones before mapping:
`Object.keys(html_entities).filter(name => name.endsWith(';'))`. Regenerate
(`tree-sitter generate; tree-sitter build`), corpus tests. The converter's
verbatim fallback in `crates/pampa/src/pandoc/treesitter_utils/entity_reference.rs`
stays as defense-in-depth; its comment (which cites this strand as the only
reachable-miss source) gets updated to reflect that misses are now
unreachable from grammar-produced nodes.

## Work items

### Phase 0 — Tests first (TDD)

- [x] Corpus tests `test/corpus/entity_reference_legacy.txt` (new file):
      `&AMP;` → `entity_reference` (guard), `&AM;` → plain `pandoc_str`s
      (fails pre-fix), bare `&AMP` → plain `pandoc_str`s (guard).
      Verified pre-fix: 2 pass, `&AM;` case fails producing
      `(entity_reference)`.
- [x] Existing Rust guard: `test_entity_reference_unknown_emits_verbatim`
      ("A &AM; B" → text "A &AM; B") was written to hold across this fix —
      keeps passing before and after.

### Phase 1 — Grammar fix

- [x] Filter in `html_entity_regex()`; `tree-sitter generate`;
      `tree-sitter build`; full corpus suite green (554/554); `parser.c`
      shrank net −121 lines.
- [x] Update the stale comments in pampa (`entity_reference.rs`,
      `test_treesitter_coverage.rs`) that describe the pre-fix behavior.

### Phase 2 — Verification + bookkeeping

- [x] pampa + tree-sitter-qmd suites 4326/4326; workspace suite
      11285/11285; full single-shot `cargo xtask verify` green (environment
      recovered after the Jupyter-kernel port leak was cleaned up). One
      unrelated macOS-only flake surfaced and was filed as bd-zazptk5s
      (automerge storage splay-prefix case collision).
- [x] Commit; close bd-v8qc9zyc.
