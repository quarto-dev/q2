# Angle brackets with inner whitespace should not lex as `html_element` (bd-ly83qewg)

**Status: approved 2026-08-07 — in progress.**

## Overview

### Symptom

```text
*a < b this text is interpreted as an HTML element.* a > b
```

fails to parse with `[Q-2-12] Unclosed Star Emphasis`. Unintuitively,
deleting the trailing `>` makes the document parse fine (both `<` and
`>` become literal `Str`s).

Original repro: `~/Desktop/daily-log/2026/08/07/test-qmd-parse-issue.qmd`
(the on-disk copy is the `>`-deleted, passing variant).

### Diagnosis

`parse_open_angle_brace` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` (~line 1821)
handles every inline `<`. After consuming `<` it scans forward looking
for a closing delimiter:

- `}` → `RAW_SPECIFIER` (qmd raw-reader extension, `` `x`{<pandoc} ``)
- `>` with URL-ish content and no whitespace → `AUTOLINK`
- **any other `>` → `HTML_ELEMENT`** — a best-effort token that exists
  "simply for good error reporting" (we recognize HTML elements but warn
  against them)
- EOF with no delimiter → `LT_STR_LITERAL` (the bd-j9cf fallback: only
  the `<` is consumed, and it becomes a plain `Str`)

In the repro, the scanner starts at the `<`, walks 54 bytes to the `>`
after the emphasis has already closed, and emits one `HTML_ELEMENT`
token spanning `< b this … element.* a >` — **swallowing the closing
`*`**. The paragraph then has an opening emphasis delimiter with no
closer → Q-2-12. Verified with `pampa -v`:

```text
lexed_lookahead sym:html_element, size:54
```

Deleting the `>` removes the only closing delimiter, so the scan
reaches EOF and the bd-j9cf `LT_STR_LITERAL` fallback kicks in — which
is why the error "unintuitively" vanishes.

`AUTOLINK` is already immune: the scan sets `could_be_autolink = false`
on the first whitespace character. Only the `HTML_ELEMENT` arm lacks a
whitespace guard.

### Pandoc behavior (target)

`pandoc -f markdown -t native` on the repro yields
`Emph [Str "a", Space, Str "<", …]` with `Str ">"` outside — both
brackets are literal text and the emphasis closes. That matches the
CommonMark/HTML rule this plan adopts: an open tag's name must
*immediately* follow `<` (letter, or `/` `!` `?` for the other
constructs); `<` followed by whitespace can never begin any HTML
construct or autolink.

### Proposed rule

**Whitespace (space, tab, CR, LF) or EOF immediately after `<`
disqualifies `HTML_ELEMENT`** (autolink is already disqualified by the
existing `could_be_autolink` logic; HTML comments are dispatched earlier
on `<!`).

Notes on scope:

- The user-suggested rule was "inner whitespace on *both* sides"
  (`< text >`). Whitespace-after-`<` alone is strictly stronger (the
  both-sides rule is a subset), is the spec-aligned condition, and also
  fixes cases like `*a < b* c>d` where the eventual `>` is *not*
  preceded by whitespace. We deliberately do **not** add a
  whitespace-before-`>` condition: `<div >` is a valid HTML open tag
  (trailing whitespace before `>` is allowed by the spec), so that
  side must not disqualify. **Open question for review**: confirm we
  prefer the after-`<` rule over the literal both-sides rule.
- `< text >` sequences where the whitespace is only *interior*
  (e.g. `<not a tag>`) still lex as `html_element` and warn. As noted
  in the report, this class of ambiguity can't be fully eliminated;
  this plan only removes the spec-unambiguous case.
- The `RAW_SPECIFIER` path is left byte-for-byte unchanged: when
  `RAW_SPECIFIER` is a valid symbol we still scan ahead for `}` exactly
  as today (so `` `x`{<pandoc} `` — and even a hypothetical
  `{< pandoc}` — keep their current behavior); we only suppress the
  `HTML_ELEMENT` emission on `>`.

### Sketch of the scanner change

In `parse_open_angle_brace`, after consuming `<` and the bd-j9cf
`mark_end`:

1. Compute `bool html_possible = !(lookahead is ' ' / '\t' / '\r' /
   '\n' or EOF)`.
2. **Fast path**: if `!html_possible && !valid_symbols[RAW_SPECIFIER]`,
   nothing downstream can match (autolink needs no-whitespace, comment
   already dispatched), so immediately `EMIT_TOKEN(LT_STR_LITERAL)` when
   `lt_str_valid`, else `return false`. This also avoids an O(n) scan to
   EOF for every `< ` in a document.
3. In the scan loop, guard the `HTML_ELEMENT` arm with
   `html_possible`. A skipped `>` is just an ordinary character: the
   scan continues (a later `}` can still make a `RAW_SPECIFIER`) and
   otherwise falls through to the existing EOF → `LT_STR_LITERAL`
   fallback.

`scanner.c` is handwritten (not generated), so the change is
scanner-only; no `grammar.js` edit is expected. Rebuild with
`tree-sitter generate; tree-sitter build` in
`crates/tree-sitter-qmd/tree-sitter-markdown` per CLAUDE.md, then
`cargo` picks up the new `scanner.c` for the Rust crate.

## Work Items

### Phase 1 — tests first (TDD) — DONE 2026-08-07

- [x] Add failing corpus tests to
      `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/lt-as-str.txt`
      (this is the bd-j9cf file; the new cases are its natural
      extension):
  - [x] repro: `*a < b text.* a > b` → emphasis closes; `<` and `>` are
        `pandoc_str`s
  - [x] plain `a < b > c` → all `pandoc_str`s (today: `html_element`)
  - [x] `<` at end of line with a `>` on a later line (newline counts
        as inner whitespace) — confirmed today's behavior is an
        `html_element` *spanning the soft break*
- [x] Add regression corpus cases pinning current behavior (some may
      already exist in `lt-as-str.txt` — verify rather than duplicate):
  - [x] `<b>` → `html_element` (already present, bd-j9cf)
  - [x] `<div >` → `html_element` (whitespace before `>` only — must
        NOT be disqualified) — added
  - [x] `<not a tag>` → `html_element` (interior-only whitespace —
        out of scope, stays) — added
  - [x] `<https://example.com>` → `autolink` (already present)
  - [x] `<!-- c -->` → `comment` (already present)
  - [x] `` `a span`{<pandoc} `` → `raw_specifier` (present in
        `inline-markdown.txt` case 3; runs with the suite)
  - [x] bd-j9cf cases still pass (`1 < 2`, `foo <`, `a <foo`)
- [x] Run `tree-sitter test` and verify the new cases fail as expected —
      `tree-sitter test -i lt-as-str`: 3 failed (the three bd-ly83qewg
      cases, each showing `html_element` where `pandoc_str`s are
      asserted), 9 passed
- [x] Add pampa-level regression tests in
      `crates/pampa/tests/integration/test_bare_lt_str.rs` (3 new
      behavior tests + 2 new RawInline regression guards); verified
      failing: emphasis case dies with Q-2-12, the other two mismatch
      (12 passed, 3 failed)

### Phase 2 — scanner fix — DONE 2026-08-07

- [x] Implement the `html_possible` guard + fast path in
      `parse_open_angle_brace` (sketch above)
- [x] `tree-sitter generate && tree-sitter build && tree-sitter test`
      — all 545 corpus tests green
- [x] `cargo nextest run -p pampa` bare-lt suite — all 15 green

### Phase 3 — verification — DONE 2026-08-07

- [x] Full `cargo xtask verify` (all 14 steps green, exit 0) — this
      includes `cargo build --workspace`, `cargo nextest run
      --workspace`, the WASM build, and hub-client build + tests
- [x] End-to-end per CLAUDE.md: ran the `pampa` binary on the failing
      variant (`*a < b this text is interpreted as an HTML element.* a > b`):

      ```
      $ target/debug/pampa repro.qmd
      [ Para [Emph [Str "a", Space, Str "<", Space, Str "b", …,
        Str "element."], Space, Str "a", Space, Str ">", Space, Str "b"] ]
      ```

      Output inspected; byte-identical in structure to
      `pandoc -f markdown -t native` on the same input.
- [x] Snapshot churn: none — no `.snap` files changed

### Phase 4 — bookkeeping

- [x] `braid comment bd-ly83qewg` with the outcome + e2e evidence
- [x] `braid close bd-ly83qewg` once all tests pass

## Open questions — resolved 2026-08-07 (user review)

1. **Rule shape**: after-`<`-only rule **approved** (subsumes the
   both-sides rule; `<div >` must stay a valid open tag).
2. `HTML_ELEMENT` scan stopping at newlines: **no** — by the same
   spec argument, `<div\n  class="foo">` is a valid open tag, so the
   scan must keep crossing newlines. (Note the after-`<` rule still
   treats a newline *immediately* after `<` as disqualifying, which is
   spec-consistent: a tag name must immediately follow `<`.)
3. `qmd-syntax-helper` angle: **no** — nothing to rewrite once the
   construct parses correctly.
