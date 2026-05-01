# Unicode whitespace handling in the qmd parser

**Beads:** bd-rmx3 (bug), bd-8oe4 (audit task, discovered-from bd-rmx3)
**Date:** 2026-04-30
**Status:** Plan — awaiting user review before implementation

## Overview

`pampa` fails to parse qmd input that contains non-ASCII whitespace
characters. The motivating repro is a one-line file copied from a
Claude.ai conversation transcript:

```
You (Apr 16, 2026, 10:18 AM)
```

The space immediately before `AM` is **U+202F NARROW NO-BREAK SPACE**
(bytes `e2 80 af`), not U+0020. Pampa reports:

```
Error: Parse error
 1 │ You (Apr 16, 2026, 10:18 AM)
   │                         ┬
   │                         ╰── unexpected character or token here
                              col 25
```

This is reproducible from `~/Desktop/daily-log/2026/04/30/whitespace-bug.qmd`
in the user's home directory, and any equivalent fixture we add to the
repo. The provenance — the Claude web UI emits U+202F before AM/PM in
its timestamp chips — means real users will hit this when pasting chat
transcripts into qmd, which is a documented use case.

The narrow scope is "accept U+202F where we accept space." The wider
scope, which is the point of doing an audit before patching, is that
hand-written tree-sitter scanners typically test for whitespace with
ASCII-only predicates (e.g. `c == ' ' || c == '\t'`), so we likely have
the same bug for every other Unicode whitespace code point. Fixing only
U+202F leaves a class of bugs latent.

## Goals

1. Match Pandoc's classification: Unicode whitespace bytes (other than
   ASCII whitespace and the existing line terminators) are *content*,
   not whitespace, and live inside `Str` nodes.
2. Implement uniformly across the tree-sitter grammars and any
   non-grammar code paths that currently reject non-ASCII bytes.
3. Land regression tests that cover the experimentally-confirmed
   Pandoc behavior for each codepoint we tested.
4. Document the policy so future scanner work follows it.

Non-goals (for this plan):

- Bidi / RTL handling.
- Normalization (NFC/NFD) of input.
- Changing semantics for ASCII whitespace.
- Introducing a new AST node for Unicode whitespace.
- Source-map round-trip plumbing (not needed under Pandoc-compat).

## Policy decision

**Decided 2026-04-30 (user):** match Pandoc 3.9.0.2's behavior. The
plan was originally drafted with a richer "Unicode whitespace-aware"
policy; an experiment against Pandoc collapsed it to something simpler.
Recorded experimental results below.

### Pandoc 3.9.0.2 experiment (2026-04-30)

Test fixtures were one byte sequence per codepoint, of the form
`a<CP>b\n` (and a paired `a <CP> b\n` form to probe ASCII-spaced
context). Tested with both `markdown` and `commonmark_x` readers and
their matching writers.

| Codepoint | Name                       | `markdown` reader native AST                | Round-trip |
|-----------|----------------------------|---------------------------------------------|------------|
| U+00A0    | NO-BREAK SPACE             | `[Str "a\160b"]`                            | byte-identical |
| U+1680    | OGHAM SPACE MARK           | `[Str "a\5760b"]`                           | byte-identical |
| U+2000    | EN QUAD                    | `[Str "a\8192b"]`                           | byte-identical |
| U+2003    | EM SPACE                   | `[Str "a\8195b"]`                           | byte-identical |
| U+2009    | THIN SPACE                 | `[Str "a\8201b"]`                           | byte-identical |
| U+200A    | HAIR SPACE                 | `[Str "a\8202b"]`                           | byte-identical |
| U+2028    | LINE SEPARATOR             | `[Str "a\8232b"]`                           | byte-identical |
| U+2029    | PARAGRAPH SEPARATOR        | `[Str "a\8233b"]`                           | byte-identical |
| U+202F    | NARROW NO-BREAK SPACE      | `[Str "a\8239b"]`                           | byte-identical |
| U+205F    | MEDIUM MATHEMATICAL SPACE  | `[Str "a\8287b"]`                           | byte-identical |
| U+3000    | IDEOGRAPHIC SPACE          | `[Str "a\12288b"]`                          | byte-identical |

`commonmark_x` reader produces the same ASTs for every codepoint
tested.

ASCII-spaced context (`a <CP> b`) tokenizes the way you'd expect:
ASCII spaces produce `Space` nodes, the Unicode codepoint stays in its
own `Str`. Example for U+202F:

```
[Str "a", Space, Str "\8239", Space, Str "b"]
```

Pandoc's `\ ` nbsp escape is recognized on input as U+00A0 and
normalized to literal U+00A0 on output (input `a\ b` → output
`a<U+00A0>b`).

### Implications

- **No new AST nodes.** Unicode whitespace is just bytes inside `Str`.
- **No special break semantics for U+2028 / U+2029.** Pandoc keeps
  them as literal Str content; the earlier "hard break / paragraph
  break" line in this plan was wrong and has been removed.
- **No source-map round-trip needed.** The bytes survive because they
  are content.
- **The bug becomes narrow:** the tree-sitter scanner currently
  rejects non-ASCII bytes mid-text. The fix is to accept them as
  ordinary text bytes in inline contexts. Block-structural predicates
  (indent counting, blank-line detection, fenced-block info-string
  delimiters, attribute-syntax delimiters) stay ASCII-only — that's
  what Pandoc does and that's what `commonmark_x` does.
- **The `\ ` nbsp escape on the writer side is a separate question**
  the audit can flag if/when we decide to mirror Pandoc's input
  recognition. It is not part of this plan's required work.

## Audit scope

**Decided 2026-04-30 (user):** comprehensive. The audit covers every
hand-written scanner in the workspace — both tree-sitter grammars
(`tree-sitter-qmd` block + inline, `tree-sitter-doctemplate`) and any
post-grammar scanner code in `pampa`, `quarto-yaml`,
`quarto-yaml-validation`, `quarto-xml`, `quarto-csl`,
`quarto-doctemplate`, `quarto-parse-errors`, and any other crate that
hand-rolls character classification. The fix is scoped to whatever
the audit classifies as (a) inline-text accumulator. Block-structural
(b) sites stay ASCII-only with a one-line comment.

## Phases

### Phase 0 — Confirm and lock the repro

Pampa's existing tests do not use a `fixtures/` directory; small repros
are byte-literals inline in a `tests/test_*.rs` file (see
`tests/test_unicode_error_offsets.rs`). We follow the same convention.

- [x] Add `crates/pampa/tests/test_unicode_whitespace.rs` with the
      U+202F Claude-timestamp repro as a byte literal
      (`b"You (Apr 16, 2026, 10:18\xe2\x80\xafAM)\n"`).
- [x] Add a failing test that asserts the post-fix AST: native
      output containing `Str "10:18\u{202F}AM)"` (literal codepoint,
      not the `\8239` escape — pampa's native writer emits the byte
      directly), and asserts no spurious `Space` node was produced.
- [x] Confirm the failure matches the user-observed diagnostic:
      `test.qmd:1:25 unexpected character or token here`. (The test's
      panic message renders the same diagnostic verbatim, so the
      failure-mode UX is locked in by the test itself; no separate
      sibling test needed.)

### Phase 1 — Audit (bd-8oe4)

- [x] Grep `crates/tree-sitter-qmd/` and
      `crates/tree-sitter-doctemplate/` scanners for the predicates
      that currently reject non-ASCII bytes mid-text.
- [x] For each hit, classify as (a) inline-text accumulator vs
      (b) block-structural predicate.
- [x] Sweep `pampa`, `quarto-yaml`, `quarto-yaml-validation`,
      `quarto-xml`, `quarto-csl`, `quarto-doctemplate`,
      `quarto-parse-errors` for hand-written character classification.
      (Remaining workspace crates do not contain hand-rolled
      tokenizers; they call into one of the above.)
- [x] Audit table written below.
- [ ] Construct fixtures covering each codepoint from the experiment
      table above, at every grammar position the audit classifies as
      (a). (Deferred to Phase 2 — done as part of writing failing
      tests.)

#### Architecture note

`crates/tree-sitter-qmd` is a **unified** grammar — block + inline
in one parser, contrary to the older `CLAUDE.md` description of two
grammars. There is one external scanner (`scanner.c`, ~2381 lines,
purely block-structural) and one grammar (`grammar.js`) whose
`pandoc_str` rule is a JS regex with Unicode flag. There is no
separate `tree-sitter-markdown-inline` directory in the repo, despite
what `tree-sitter.json` claims.

#### Audit findings

| ID  | Site                                                                                  | What it does                                                       | Class | Action                                          |
|-----|---------------------------------------------------------------------------------------|--------------------------------------------------------------------|-------|--------------------------------------------------|
| G1  | `tree-sitter-qmd/.../grammar.js:35-81` (`PANDOC_REGEX_STR`)                           | Inline-text token regex; `startStrRegex` + continuation class hard-code only U+00A0 from the Unicode whitespace set | **(a)** | **Required fix:** add the rest of the non-ASCII `White_Space=Yes` codepoints to both character classes (start + continuation). |
| C1  | `tree-sitter-qmd/.../scanner.c` (≈50 hits)                                            | Block structure: ATX/list markers, fenced blocks, blank lines, pipe-table cells, indent counting, inline-attr delimiters | **(b)** | No change — Pandoc's block grammar is ASCII-defined. |
| C2  | `tree-sitter-doctemplate/.../scanner.c:79,93-95` (`lex_whitespace`, `lookahead_is_space`) | Template syntax delimiter whitespace                              | **(b)** | No change. |
| C3  | `tree-sitter-doctemplate/.../grammar.js:35` (`text: /[^$]+/`)                         | Template literal-text token (already Unicode-permissive by negation) | n/a   | No change. |
| P1  | `pampa/src/pandoc/treesitter_utils/quote_helpers.rs:39-63`                            | Strip leading/trailing whitespace from quote delimiter token       | **(b)** | **Defensive:** change `char::is_whitespace` → `char::is_ascii_whitespace`. The scanner only captures ASCII into delimiters today; this prevents future scanner changes from accidentally peeling a U+00A0 off into a `Space` node. |
| P2  | `pampa/src/pandoc/treesitter_utils/text_helpers.rs:276-301`                           | Strip leading/trailing whitespace from emphasis delimiter token    | **(b)** | Defensive change as P1. |
| P3  | `pampa/src/pandoc/treesitter_utils/citation.rs:59`                                    | Strip leading whitespace from citation token                       | **(b)** | Defensive change as P1. |
| P4  | `pampa/src/pandoc/treesitter_utils/code_span_helpers.rs:51`                           | Strip leading whitespace from code-span delimiter token            | **(b)** | Defensive change as P1. |
| P5  | `pampa/src/pandoc/treesitter_utils/shortcode.rs:58-61`                                | Strip leading whitespace from shortcode delimiter token            | **(b)** | Defensive change as P1. |
| P6  | `pampa/src/pandoc/treesitter_utils/uri_autolink.rs:33-39`                             | Strip leading/trailing whitespace from URI autolink                | **(b)** | Defensive change as P1. |
| P7  | `pampa/src/pandoc/treesitter.rs:749`                                                  | Strip leading whitespace from note-reference token (`[^id]`)       | **(b)** | Defensive change as P1. |
| P8  | `pampa/src/pandoc/treesitter.rs:540`                                                  | Hard-line-break detection (count trailing ASCII spaces ≥ 2)        | **(b)** | Already ASCII-only via `b' '`. No change. |
| P9  | `pampa/src/pandoc/treesitter.rs:69` (`parse_anchor_shorthand`)                        | Reject `<#id>` containing whitespace                               | **(b)** | Defensive change to `char::is_ascii_whitespace` for consistency; behavior unchanged because `<#…non-ASCII-whitespace…>` is also not a valid anchor. |
| P10 | `pampa/src/utils/trim_source_location.rs:46,60`                                       | Generic source-range trimming helper                               | n/a   | Caller-dependent; not on the bug path. Leave Unicode-aware. |
| P11 | `pampa/src/utils/autoid.rs:47`                                                        | Slug-id generation: whitespace → `-`                               | n/a   | Operates on parsed inline text, not on tokenizer output. Unicode-aware behavior is reasonable. Leave as-is. |
| P12 | `pampa/src/lua/types.rs:1543-1577` (`split_string_to_inlines`)                        | Pandoc `B.text`: ASCII space → `Space`, ASCII newline → `SoftBreak`, else → word | **(a)** | Already ASCII-only. Matches Pandoc. No change. |
| P13 | `pampa/src/writers/qmd.rs` and `writers/ansi.rs` (newline-tracking sites)             | Track line starts on `b'\n'`                                       | **(b)** | Already ASCII-only. No change. |
| X1  | `quarto-xml/src/parser.rs:454,470,501`                                                | Skip whitespace around `=` in attribute parsing; end of unquoted value | **(b)** | **Spec-correctness fix:** XML 1.0 §2.3 defines S = `(#x20|#x9|#xD|#xA)+` (ASCII only). Change `c.is_whitespace()` → `c.is_ascii_whitespace()`. |
| X2  | `quarto-xml/src/parser.rs:340`                                                        | Skip text nodes whose `.trim()` is empty                           | **(b)** | Spec-correctness change as X1 (use ASCII-only trim equivalent). |
| D1  | `quarto-doctemplate/src/doc.rs:134-135` (`trim_end_matches('\n')`)                    | Strip trailing LF from Doc text                                    | **(b)** | Already ASCII-only. No change. |
| E1  | `quarto-parse-errors/src/error_generation.rs:336,356`                                 | Track newlines for error position mapping                          | **(b)** | Already ASCII-only via `ch == '\n'`. No change. |
| Y1  | `quarto-yaml`, `quarto-yaml-validation`, `quarto-csl`                                 | No hand-written whitespace classification (delegate to upstream parsers) | n/a   | No work. |

#### Cuts

The required, root-cause fix is exactly **one site (G1)**: the JS
regex in `grammar.js`. Once that is in, the U+202F repro and every
codepoint in the experiment table parses as Pandoc-compatible Str
content.

The defensive changes (P1-P7, P9, X1, X2) are independently
justified — they make latent class-of-bug exposure go away — and
they're cheap, mechanical edits. They are **in scope** for this
plan because the user explicitly asked for a comprehensive sweep
("we might as well be comprehensive"). They will land in their own
commit(s) so the root-cause fix is reviewable in isolation.

P10, P11, P12, P13, D1, E1 require **no change** — already correct.

### Phase 2 — Tests first (TDD)

- [x] Parameterized Rust tests in
      `crates/pampa/tests/test_unicode_whitespace.rs` exercise every
      codepoint from the experiment table at four positions:
      mid-word (`aXb`), ASCII-spaced (`a X b`), alone on a line
      (`X\n`), and CRLF (`aXb\r\n`). Plus a multi-codepoint word and
      a line-of-only-U+00A0 negative test. **Status: 6 of 7 fail
      today; 1 passes (the NBSP variant of the standalone-line test,
      because U+00A0 was previously hand-fixed in the regex).**
- [x] Tree-sitter corpus tests in
      `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/unicode-whitespace.txt`
      assert grammar-level structure for representative codepoints
      (U+00A0, U+202F, U+3000, U+2003, U+2028, U+2029) at mid-word,
      ASCII-spaced, and alone-on-line positions. **Status: 5 of 8
      fail today; 3 pass (the U+00A0 variants).**
- [x] The "line of only U+00A0 does not separate paragraphs" test in
      `test_unicode_whitespace.rs` is the negative test for the (b)
      classification of blank-line detection in scanner.c. (Currently
      passes because U+00A0 is already supported. Will continue to
      pass post-fix because the change is purely additive.)
- [x] CRLF coverage included via the
      `non_ascii_whitespace_with_crlf_line_endings` test, which
      runs every codepoint with `\r\n` line endings.

### Phase 3 — Implement

- [x] **G1 (root-cause fix):** added `PANDOC_NON_ASCII_WHITESPACE`
      constant to `grammar.js` covering U+00A0, U+1680, U+2000–U+200A,
      U+2028, U+2029, U+202F, U+205F, U+3000. Both `startStrRegex`
      and the continuation class now reference this constant
      (subsuming the previous lone U+00A0 entry). `tree-sitter
      generate` regenerated `parser.c`. Bundled `tree_sitter/array.h`
      was reverted to the checked-in version (collateral churn from
      a tree-sitter CLI version difference; the new parser.c
      compiles fine against the older array.h).
- [x] **Defensive (b) changes:** switched `char::is_whitespace` →
      `char::is_ascii_whitespace` and `.trim()/.trim_start()` →
      `.trim_ascii()/.trim_ascii_start()` at:
      - `pampa/src/pandoc/treesitter_utils/quote_helpers.rs`
      - `pampa/src/pandoc/treesitter_utils/text_helpers.rs`
      - `pampa/src/pandoc/treesitter_utils/citation.rs`
      - `pampa/src/pandoc/treesitter_utils/code_span_helpers.rs`
      - `pampa/src/pandoc/treesitter_utils/shortcode.rs`
      - `pampa/src/pandoc/treesitter_utils/uri_autolink.rs`
      - `pampa/src/pandoc/treesitter.rs:749` (note reference)
      - `quarto-xml/src/parser.rs` (X1, X2: attribute parsing,
        whitespace-only text skipping)
      Each site got a one-line comment referencing the policy doc.
- [x] **Skipped intentionally:**
      - `parse_anchor_shorthand` (treesitter.rs:69) — the existing
        Unicode-aware check is the *stricter* validation (rejects
        more invalid IDs) and is more user-friendly for HTML-fragment
        compatibility. No change.
      - `code_span_helpers.rs:80` (`code_text.trim()` on code-span
        content) — Pandoc's actual code-span trim is a single-space
        strip, not `.trim()`; that's a separate latent bug not
        covered by this plan. No change.
- [x] All Phase 2 Rust tests pass (7/7); all Phase 2 corpus tests
      pass (8/8); full corpus suite passes (459/459).
- [x] `cargo nextest run --workspace` — 7609/7609 passing.
- [x] `cargo xtask verify --skip-hub-build` — all 9 steps pass.
- [x] **End-to-end verification:**
      `cargo run -p pampa --bin pampa -- ~/Desktop/daily-log/2026/04/30/whitespace-bug.qmd`
      now succeeds and emits
      `[Para [Str "You", Space, Str "(Apr", ..., Str "10:18\u{202F}AM)"]]`.
      `-t json | jq -r '.blocks[0].c[-1].c' | xxd` confirms the
      bytes `10:18 e2 80 af AM)` are preserved verbatim through the
      whole pipeline.

### Phase 4 — Round-trip

- [x] Added six fixtures to
      `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`:
      `unicode_whitespace_u00a0_nbsp.qmd`,
      `unicode_whitespace_u202f_claude_timestamp.qmd`,
      `unicode_whitespace_u3000_ideographic.qmd`,
      `unicode_whitespace_u2028_line_separator.qmd`,
      `unicode_whitespace_u2029_paragraph_separator.qmd`,
      `unicode_whitespace_mixed.qmd`. The existing
      `test_qmd_roundtrip_consistency` driver picks them up via glob;
      all pass (`QMD → JSON → QMD → JSON` produces equal JSON).
- [x] Added a stronger byte-for-byte round-trip test
      (`non_ascii_whitespace_round_trips_byte_for_byte` in
      `test_unicode_whitespace.rs`) that asserts `qmd::write` of the
      AST produced by `qmd::read` is *byte-identical* to the input
      for every codepoint in the experiment table plus the headline
      Claude-timestamp repro. (The JSON-equivalence driver above is
      necessary but weaker — it would still pass under a writer that
      normalised U+00A0 to `\ `.)
- [x] Verified the original Desktop repro round-trips byte-for-byte
      via the actual binary:
      `cargo run -p pampa --bin pampa -- ~/Desktop/.../whitespace-bug.qmd -t qmd`
      produces output whose `xxd` is identical to the input's `xxd`
      (`diff <(xxd input) <(xxd output)` is empty).

### Phase 5 — Documentation

- [x] Wrote `claude-notes/instructions/parser-whitespace-policy.md`
      stating the rule, listing the codepoints, naming the
      enforcement sites (grammar, block scanner, downstream
      extractors, XML parser), and recording the rationale +
      history.
- [x] In-code one-line comments at each (b)-classified site already
      reference the plan doc; the new policy doc points at the same
      plan for the experiment table. A future developer landing on
      either entry point gets the full picture.
- [x] No `docs/` update — the behavior is user-invisible: previously
      failing input now parses correctly; previously-working input
      is unchanged.

## Status

**Complete (2026-04-30).** Closing bd-rmx3 and bd-8oe4. The fix
shipped with:
- 8 new Rust tests (`crates/pampa/tests/test_unicode_whitespace.rs`)
- 8 new tree-sitter corpus tests
  (`unicode-whitespace.txt` in tree-sitter-markdown/test/corpus/)
- 6 new round-trip fixtures
  (`unicode_whitespace_*.qmd` in roundtrip_tests/qmd-json-qmd/)
- 1 grammar regex change (G1)
- 9 defensive predicate updates across pampa + quarto-xml
- 1 new policy doc (`claude-notes/instructions/parser-whitespace-policy.md`)
- Full workspace: 7610/7610 passing
- Full corpus: 459/459 passing
- `cargo xtask verify --skip-hub-build`: all 9 steps pass
- Original Desktop repro round-trips byte-for-byte through `pampa`

## Risk and exposure

- **Snapshot churn.** Any existing snapshot fixture that currently
  records the *parse failure* on a non-ASCII byte will change once
  parsing succeeds. The audit should grep snapshots too. Per
  CLAUDE.md, every changed snapshot must be enumerated in the commit
  message.
- **Pandoc divergence.** Minimal — the policy here matches Pandoc as
  recorded in the experiment table above. The only known divergence
  candidate is the writer's handling of the `\ ` nbsp escape on
  *output*, which Pandoc normalizes to literal U+00A0; we are not
  required to mirror that as part of this plan.
- **CRLF interactions.** bd-ylig (the CRLF pipe-table fix at commit
  9921b295) is a recent reminder that scanner whitespace handling
  has cross-platform sharp edges. Phase 2 explicitly includes
  CRLF × Unicode-codepoint coverage.
- **Performance.** Negligible. We are *removing* a rejection branch
  in the scanner, not adding a Unicode property lookup. No new
  per-codepoint decoding is introduced.

## How to resume

If this plan is picked up after compaction:

1. Read this file in full.
2. Check `br show bd-rmx3` and `br show bd-8oe4` for status.
3. Look for an updated table in Phase 1 — that signals the audit is
   done.
4. The next unchecked `[ ]` is the next action.
