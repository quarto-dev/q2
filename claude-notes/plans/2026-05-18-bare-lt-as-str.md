# Recognize bare `<` as a `Str` token

**Status:** implemented (awaiting review / push)
**Tracking issue:** bd-j9cf
**Owner:** cscheid
**Last updated:** 2026-05-18

## Overview

Today a literal `<` outside of math, code, or a recognized HTML
construct produces a hard parse error in qmd. The minimal trigger
is a one-line document:

```
1 < 2
```

```
Error: Parse error
 1 │ 1 < 2
   │   ┬
   │   ╰── unexpected character or token here
```

The reason is that the external scanner in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
(`parse_open_angle_brace`) only emits a token from `<` when it can
resolve the construct to one of:

- `HTML_COMMENT` (`<!-- ... -->`)
- `AUTOLINK` (`<https://...>`, `<foo@bar>` — requires `:` or `%`)
- `RAW_SPECIFIER` (`<=html}` and friends)
- `HTML_ELEMENT` (anything up to the next `>` — best effort, used to
  emit the `Q-2-9` "HTML element converted to raw HTML" warning)

If the scanner walks to EOF / end-of-block without finding `>` or
`}`, it returns `false` and emits no token at all. The grammar's
internal `pandoc_str` regex
(`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`, ~line 80)
also does **not** include `<` in any of its character classes (it
explicitly excludes `<` from `PANDOC_VALID_MATH_SYMBOLS` and never
adds it back). So `<` ends up lexed by nothing, the parser hits the
generic "unexpected character or token here" fallback in
`crates/quarto-parse-errors/src/error_generation.rs:316`, and the
document fails to render.

The escaped form `\<` already works (`pandoc_str` regex starts with
`\\.` so a backslash-escape always lexes as `Str`). The qmd writer
already escapes raw `<` defensively
(`crates/pampa/src/writers/qmd.rs:1414` — `'<' => "\\<"`), so a
production round-trip for the new `Str "<"` node is already wired
up: emit Str `<`, write `\<`, re-read as `Str "<"`. No writer
changes are needed.

## Goal

A bare `<` that is not the start of a recognized HTML / autolink /
raw-specifier construct should parse as a plain `Str` containing the
single character `<`, with the same source-info handling as any
other `Str` node.

Concrete acceptance criterion: the document `1 < 2` parses without
error and produces `Para [Str "1", Space, Str "<", Space, Str "2"]`.

## Approach

We disambiguate inside the external scanner — the parser is
LR-style and cannot do this without help, because the same first
character (`<`) starts at least four different productions.

### CommonMark-style lookahead

Add a new external token, tentatively `_pandoc_lt_str`. The scanner
emits it when the lookahead after `<` cannot start any of the
existing constructs. Specifically, when valid for `_pandoc_lt_str`:

1. Position is at `<`. Save the byte position of `<`+1.
2. Peek the next character:
   - `!`  → fall through to existing `parse_html_comment` path.
   - `[a-zA-Z]` → existing tag-scanning path (may emit
     `HTML_ELEMENT` or `AUTOLINK`).
   - `/`  → existing closing-tag-scanning path (`HTML_ELEMENT`).
   - `?`  → existing processing-instruction-ish path
     (`HTML_ELEMENT`, with the same `Q-2-9` warning).
   - anything else (space, tab, digit, punctuation, EOF, newline)
     → **mark_end at `<`+1 and emit `_pandoc_lt_str`** so only the
     single `<` character is consumed.
3. If the existing tag-scanning path walks to EOF / newline without
   finding a `>` (the current "give up and return false" branch in
   `parse_open_angle_brace`), retract to `<`+1 and emit
   `_pandoc_lt_str` instead. This covers `<foo\n` and `<foo<EOF>`,
   which today are parse errors.

The exact set of "starter" characters mirrors
[CommonMark §6.6](https://spec.commonmark.org/0.31.2/#raw-html);
intentionally narrower than the current "anything until `>`"
behavior so that `<5>` and `<,>` *also* become `Str` content. This
is a small behavior change for HTML-ish input that was already
producing a `Q-2-9` warning — see "Risks" below.

### Grammar wiring

In `grammar.js`:

- Declare `$._pandoc_lt_str` in the `externals` list (near
  `$.html_element`, ~line 1046).
- Add it as an alternative for `_inline_element`, aliased to
  `$.pandoc_str` (or as a distinct node-type that
  `pampa/src/pandoc/treesitter.rs` maps to `Str { text: "<" }`).
  Aliasing to `pandoc_str` is the lower-blast-radius option and
  reuses the existing AST mapping in `treesitter.rs:612` without
  touching downstream code.

The lexer reservation cost is one token. No new conflicts are
expected because `_pandoc_lt_str` is only valid where
`_inline_element` is, and the scanner never emits it when
`HTML_ELEMENT` / `AUTOLINK` / `HTML_COMMENT` / `RAW_SPECIFIER` are
on the table.

### Downstream

- `crates/pampa/src/pandoc/treesitter.rs:612` already maps
  `pandoc_str` → `Inline::Str`. If we alias `_pandoc_lt_str` to
  `pandoc_str` in the grammar, no code change is needed here.
- `crates/pampa/src/writers/qmd.rs:1414` already escapes `<` as
  `\<` on write. Round-trip works for free.
- Error corpus `Q-2-9` ("HTML element converted to raw HTML") still
  fires for `<a>`, `<div class=…>`, etc. — only the
  scanner-failure case stops being an error.
- The generic "unexpected character or token here" fallback in
  `crates/quarto-parse-errors/src/error_generation.rs:316` is
  unchanged; documents with truly malformed input (e.g. unmatched
  `{`) still hit it.

## Risks and prior art

1. **Behavior change for `<5>`, `<,>`, etc.** The current scanner
   eagerly emits `HTML_ELEMENT` whenever it finds a closing `>`,
   even when the contents are not a valid HTML tag. With the
   proposed CommonMark-style first-char gate, `<5>` would become
   three `Str` inlines (`<`, `5`, `>`) instead of a `RawInline html
   "<5>"` with a `Q-2-9` warning. This is more correct but
   user-visible. Mitigations:
   - Audit existing snapshots / corpus tests for `<` followed by
     a non-letter and a closing `>` to see how many sites change.
   - If the change is too broad for one PR, keep the lookahead
     conservative (only treat `<` as `Str` when the next char is
     whitespace, EOF, or newline) and widen later in a follow-up.

2. **Math mode and code spans.** The scanner only fires
   `parse_open_angle_brace` when inline parsing is active — math
   and code-span tokens win at the lexer level — so `$1 < 2$` and
   `` `1 < 2` `` are unaffected. Tests should still pin this.

3. **Pipe-table cells.** Pipe-table cell parsing has its own
   tokenization for `|` and code spans; adding a Str-`<` token in
   the inline grammar should compose, but the cell-boundary code
   in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
   needs a quick check.

4. **Performance.** One extra branch in `parse_open_angle_brace`
   and one extra external token slot. Negligible.

## Test plan (TDD — write tests first)

### Phase 0 — failing tests

- [x] Add a tree-sitter corpus test in
  `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/lt-as-str.txt`
  exercising the same cases at the grammar level (4 new-behavior
  cases + 3 regression cases for `<b>`, `<https://…>`, `<!-- … -->`).
- [x] Add `crates/pampa/tests/test_bare_lt_str.rs` with 10 unit tests
  driving `readers::qmd::read` directly: 4 new-behavior cases (bare
  `<` between digits, at EOL, before digit, unclosed tag), 6
  regression cases (html_element, autolink, comment, `\<`,
  math `$1 < 2$`, code span `` `1 < 2` ``).
- [x] Add roundtrip fixtures
  `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/bare_lt_simple.qmd`,
  `…/bare_lt_eol.qmd`, `…/bare_lt_unclosed_tag.qmd`.
- [x] Run each new test once and capture the failure mode (parse
  error vs. unexpected AST) as a baseline before any scanner
  change.

**Baseline captured (2026-05-18, pre-fix):**

```
tree-sitter test -i lt-as-str
  ✗ bare `<` followed by space is a Str  → ERROR + pandoc_str
  ✗ bare `<` at end of line is a Str     → ERROR + pandoc_str
  ✗ bare `<` followed by digit is a Str  → ERROR + pandoc_str + shortcode_name
  ✓ `<b>` still parses as html_element   (regression baseline)
  ✓ autolink still parses as autolink    (regression baseline)
  ✓ HTML comment still parses as comment (regression baseline)
  ✗ `<foo` no closing `>` → Str `<` + Str `foo` → ERROR + …
```

```
cargo nextest run -p pampa --test test_bare_lt_str
  ✗ bare_lt_between_digits_parses_as_str        (parse error at <)
  ✗ bare_lt_at_end_of_line_parses_as_str        (parse error at <)
  ✗ bare_lt_followed_by_digit_parses_as_str     (parse error at <)
  ✗ unclosed_tag_parses_as_str_lt_plus_text     (parse error at <)
  ✓ html_element_still_parses_as_raw_html
  ✓ autolink_still_parses_as_link
  ✓ html_comment_still_parses_as_raw_html_comment
  ✓ backslash_escaped_lt_is_unchanged
  ✓ lt_in_math_is_unchanged
  ✓ lt_in_code_span_is_unchanged
```

Roundtrip fixture `bare_lt_eol.qmd` confirms the same: the
roundtrip runner fails with `Parse error … unexpected character
or token here` at the `<`.

### Phase 1 — scanner change

- [x] Add `LT_STR_LITERAL` to the externals list in `grammar.js`
  (as `$._pandoc_lt_str`) and to the scanner's `TokenType` enum +
  `token_names[]` debug table in `scanner.c`.
- [x] Implement the fallback emission in
  `scanner.c::parse_open_angle_brace`. The scanner advances past
  `<`, calls `mark_end` to fix a candidate end at `<+1`, then runs
  the existing scan loop. Each emitting branch
  (`HTML_ELEMENT`, `AUTOLINK`) gained an explicit `mark_end` after
  its advance past `>` so the early mark_end at `<+1` is updated
  to the actual token end. If the scan loop reaches EOF without
  finding a closing delimiter, the scanner now emits
  `LT_STR_LITERAL` (consuming only the `<`).
- [x] Add `$._pandoc_lt_str` as a third alternative inside
  `pandoc_str` (`choice(regex, '|', $._pandoc_lt_str)`) so the
  AST shape stays uniform — every Str-like inline is a
  `pandoc_str` node.
- [x] Update the main `case '<':` switch in `scanner.c` to enter
  `parse_open_angle_brace` when `LT_STR_LITERAL` is valid even if
  no HTML token is.
- [x] Regenerate parser with `tree-sitter generate; tree-sitter
  build` from `crates/tree-sitter-qmd/tree-sitter-markdown/`.
- [x] Run `tree-sitter test`: **508/508 passing** (501 existing + 7
  new lt-as-str cases).
- [x] Run the pampa tests; all 10 `test_bare_lt_str` cases pass.

**Discovered downstream change** (not in the original plan):

Tree-sitter chomps preceding whitespace into the external token's
reported range (the block-level scan loop at `scanner.c` ~line 2160
consumes indentation before dispatching to
`parse_open_angle_brace`). This is the same behavior that the
`html_element` and `autolink` handlers already split out into a
leading `Space` inline. Extended `treesitter.rs`'s `pandoc_str`
branch to do the same:

- If the `pandoc_str` text starts with ASCII whitespace, emit a
  `Space` inline for the leading run and a `Str` inline for the
  remainder (with source ranges adjusted accordingly).
- **Leading-only** — we deliberately do not strip trailing
  whitespace, because `\<space>` backslash escapes match the
  `\\.` branch of `PANDOC_REGEX_STR` and produce a 2-char
  `pandoc_str` ending in a real space; `process_backslash_escapes`
  then turns that into U+00A0. Stripping trailing space would lose
  the escape's payload (regression in
  `test_backslash_space_becomes_nbsp` and friends, bd-1aip).
- Regular pandoc_str text never has leading ASCII whitespace
  (PANDOC_REGEX_STR's start anchors are non-whitespace), so the
  split is a no-op for all non-`_pandoc_lt_str` matches.

**Parser-state knock-on**:

Adding a new external token shifts LR state numbers, which broke
`toplevel_unclosed_attr_stays_q_2_2` (relies on a frozen
state→error-code table in `_autogen-table.json`). Regenerated by
running `crates/pampa/scripts/build_error_table.ts`.

### Phase 2 — regression / round-trip

- [x] Roundtrip fixtures in
  `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/` all pass:
  `bare_lt_simple.qmd`, `bare_lt_eol.qmd`,
  `bare_lt_unclosed_tag.qmd`. The qmd writer already escapes `<`
  as `\<` (`crates/pampa/src/writers/qmd.rs:1414`), and re-reading
  `\<` produces `Str "<"` via the `\\.` branch of `pandoc_str`.
- [x] `<b>` regression test passes — Q-2-9 ("HTML element
  converted to raw HTML") still fires.
- [x] Math (`$1 < 2$`) and code-span (`` `1 < 2` ``) tests pass —
  the inline scanner for those constructs runs before
  `parse_open_angle_brace` is even considered.
- [x] Snapshot audit: full pampa test suite ran clean
  (`cargo nextest run -p pampa --no-fail-fast`: **3742/3742
  passing, 2 skipped**). No snapshot files needed updating. The
  only behavior change beyond the new `<` support is the
  `_autogen-table.json` regeneration (state-number remap, same
  error codes).
- **Decision on aggressive-vs-conservative**: We took the
  **conservative** path. The scanner still attempts HTML element
  / autolink / comment / raw-specifier first and only falls back
  to `LT_STR_LITERAL` when no `>` / `}` closer is found. So
  `<5>`, `<,>`, etc. continue to lex as `HTML_ELEMENT` and emit
  the Q-2-9 warning — no user-visible change there. The fallback
  exclusively rescues `<` followed by content that walks to EOF
  (the `1 < 2` case the user filed).

### Phase 3 — full verification

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace`: **8998/8998 passing,
  195 skipped**.
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests`:
  *All verification steps passed!*
- [x] `cargo xtask verify --skip-rust-build --skip-rust-tests`
  (hub-client build + WASM tests + trace-viewer): *All
  verification steps passed!* Required `npm install` first at
  both repo root and worktree.
- [x] End-to-end binary verification:

  ```text
  $ cargo run --quiet -p pampa --bin pampa -- /tmp/q2-lt-test-bd-j9cf/simple.qmd
  [ Para [Str "1", Space, Str "<", Space, Str "2"] ]

  $ cargo run --quiet -p pampa --bin pampa -- /tmp/q2-lt-test-bd-j9cf/eol.qmd
  [ Para [Str "foo", Space, Str "<"] ]

  $ cargo run --quiet -p pampa --bin pampa -- /tmp/q2-lt-test-bd-j9cf/unclosed.qmd
  [ Para [Str "a", Space, Str "<foo"] ]

  $ cargo run --quiet -p pampa --bin pampa -- -t qmd /tmp/q2-lt-test-bd-j9cf/simple.qmd
  1 \< 2

  $ cargo run --quiet -p pampa --bin pampa -- -t html /tmp/q2-lt-test-bd-j9cf/simple.qmd
  <p>1 &lt; 2</p>
  ```

  All three native ASTs match the test expectations; the qmd
  writer emits the safe-roundtrip `\<` form; the HTML writer
  entity-escapes `<` as `&lt;`. Output inspected manually.

## Out of scope

- Pretty-printing `<` as `&lt;` in HTML output. The HTML writer
  already handles entity escaping for `Str` nodes; verify in
  Phase 2 but no code change expected.
- Symmetric handling of a bare `>`. Today `>` is allowed inside
  `pandoc_str` (see `PANDOC_REGEX_STR` line 87, `[>.,;!?]`), so
  no work needed — but call it out in the PR description.
- Reference-style links (intentionally unsupported in qmd; see
  `CLAUDE.md`).

## Open questions

1. Aggressive (`<5>` becomes `Str`) vs. conservative (`<5>`
   still `HTML_ELEMENT`)? Default to conservative; revisit if
   user feedback wants the broader behavior.
2. Should the new behavior emit any diagnostic at all
   (e.g. an `info`-level hint that `\<` is the explicit form)?
   Default to no — bare `<` is now valid, and there's no
   ambiguity to warn about.

## Work items (checklist)

- [x] Phase 0: write failing tests (fixtures + corpus + snapshot
  assertions), capture baseline.
- [x] Phase 1: scanner + grammar change, regenerate parser, get
  Phase 0 tests green.
- [x] Phase 2: round-trip, regression, snapshot audit.
- [x] Phase 3: full workspace verify; record E2E invocation in
  PR description.
- [x] Decide aggressive-vs-conservative based on snapshot audit
  results; document the choice in the PR description. (Chose
  conservative — see Phase 2 above.)
