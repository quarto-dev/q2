# Parser rejects combining characters / join controls in prose (bd-96fswwce)

**Date:** 2026-08-10
**Braid:** bd-96fswwce (bug, P2 — arguably P1, see scope), discovered-from bd-named-entities-w6xbfftj
**Checkout:** main (on top of the entity-decode work, PR #488)
**Status:** Implemented 2026-08-10; all verification legs green (piecewise — see Phase 2 caveat).

## Problem

`pandoc_str` in `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` is an
explicit union of content character classes (alphanumerics, curated symbol
categories, non-ASCII Po/Pc punctuation, smart quotes, non-ASCII whitespace).
Combining marks (`Mn`/`Mc`/`Me`) and the join controls ZWNJ/ZWJ
(U+200C/U+200D) are absent, so the lexer finds no token and the parse errors.
Same bug family and fix shape as bd-6kewx (which added non-ASCII `Po`/`Pc`
after bare `§` in prose produced parse errors).

All of these are hard parse errors today (verified at `9509f8d2`):

| Input | Class | Note |
|---|---|---|
| `x ≂̸ y` (U+2242 U+0338) | Mn after symbol | what `&NotEqualTilde;` decodes to |
| `cafe` + U+0301 | Mn mid-word | any NFD/decomposed text |
| `का` (क + U+093E) | Mc | **any Hindi text with vowel signs** |
| `a` + U+20DD | Me | enclosing marks |
| `ab` + U+200C + `cd` | Cf (ZWNJ) | Persian/Indic joining control |
| `ab` + U+200D + `cd` | Cf (ZWJ) | outside emoji sequences |

Pandoc parity (verified with system pandoc): every one of these folds into
`Str` verbatim (`cafe\769`, `ab\8204cd`, `a\8413`, …).

## Fix

In `grammar.js`, define a content class for combining marks + join controls
(`\p{M}\u{200C}\u{200D}`) and add it to `PANDOC_REGEX_STR`:

1. as a new single-char alternative (mark after a symbol token / after space /
   at line start — the AST converter already merges adjacent `Str`s), and
2. inside the word-continuation class (so `cafe` + U+0301, `का`, `ab‌cd` stay
   single tokens).

ZWJ interplay with `EMOJI_REGEX` (which uses U+200D inside emoji sequences) is
safe: the lexer takes the longest match, so emoji ZWJ sequences still win.

Then `tree-sitter generate; tree-sitter build; tree-sitter test` in
`crates/tree-sitter-qmd/tree-sitter-markdown/` (build.rs recompiles the
generated `parser.c` into the Rust crate).

## Work items

### Phase 0 — Tests first (TDD)

- [x] Corpus tests `test/corpus/combining_marks.txt` (new file only): the six
      cases above; verified all 6 fail pre-fix (ERROR nodes).
- [x] Rust coverage tests in
      `crates/pampa/tests/integration/test_treesitter_coverage.rs` asserting
      verbatim `Str` content for each case; verified all 6 fail pre-fix.
- [x] Roundtrip: re-added `&NotEqualTilde;` to
      `tests/roundtrip_tests/qmd-json-qmd/named_entities.qmd` and added a
      literal `combining_marks.qmd` fixture.

### Phase 1 — Grammar fix

- [x] Added `PANDOC_COMBINING_MARKS = "\\p{M}\\u{200C}\\u{200D}"` to
      `grammar.js` (single-char alternative + word-continuation class);
      `tree-sitter generate`; `tree-sitter build`; full corpus suite green
      (551/551, incl. the 6 new — `\p{M}` compound category accepted).
- [x] Rust tests pass (coverage + roundtrip + entity tests: 14/14).

### Phase 2 — Verification + bookkeeping

- [x] `cargo nextest run --workspace`: 11237 passed, 197 skipped.
- [x] End-to-end `q2 render` of a fixture with literal `x ≂̸ y`, Hindi
      `का matra`, NFD `cafe`+U+0301, and ZWNJ/ZWJ words: all render verbatim;
      NFD sequence byte-verified in the HTML (`6361 6665 cc81` — not
      normalized).
- [x] Verification of all `cargo xtask verify` legs — **piecewise** (see
      caveat): lints/clippy + workspace build green in every attempt; Rust
      workspace tests 11237/11237 post-change; hub-client `build:all` (incl.
      WASM from the regenerated parser) green; hub-client `test:ci` 131/131
      green; ts-packages build green.
      **Caveat:** three consecutive single-shot `cargo xtask verify` runs
      failed on *different*, unrelated network-dependent tests (quarto-hub
      auth, q2-preview listener, sync-client websocket), each passing in
      isolation. Root cause is environmental: ~2,300 orphaned Jupyter
      ipykernel processes (started 2026-08-06, PPID 1) hold 13,973 of the
      16,384 ephemeral ports, so parallel test bursts hit EADDRNOTAVAIL on
      loopback. Cleanup (`pkill -f ipykernel_launcher`) left to the user —
      it would also kill any live notebook kernels. CI on the PR provides the
      independent single-shot check.
- [x] Commit; close bd-96fswwce.

## Risks

- Adding `\p{M}` to the *continuation* class only (not `startStrRegex`) keeps
  emphasis/underscore boundary logic untouched; mark-initial runs lex via the
  single-char alternative instead.
- `tree-sitter generate` must support `\p{M}` (compound category). The grammar
  already uses `\p{L}`/`\p{N}`/`\p{So}`; if compound `M` is rejected, fall
  back to `\p{Mn}\p{Mc}\p{Me}`.
- State-count growth in the generated parser is possible; watch `parser.c`
  size and generation time.
