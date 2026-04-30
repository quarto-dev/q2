# bd-af1e — Tree-sitter splits paragraph at line starting with backtick

## Summary

A continuation line that begins with a single backtick (inline code span) is incorrectly treated as a paragraph-interrupting block start by the tree-sitter external scanner. The scanner refuses to emit `SOFT_LINE_ENDING` and instead closes the current paragraph, producing two `pandoc_paragraph` nodes where there should be one paragraph with a `pandoc_soft_break`.

The bug is fully reproducible at the tree-sitter layer; no Rust code is involved. The fix is entirely in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`.

## Reproduction

### Original report

```
- **Title prefix.** Each page's `<title>` becomes
  `<page title> – <website.title>`. This page's `<title>` should
  read `Home – My Site`.
```

`pampa -t native` (and `pandoc -f markdown -t native`) should produce a single block with `SoftBreak`s. Pandoc does. Pampa splits at line 2 (which starts with `` ` ``):

```
[ BulletList [[Para [..., Str "becomes"], Para [Code ... "<page title> ...", ...]]] ]
```

### Minimal pure-tree-sitter reproductions

Both fail (split into two `pandoc_paragraph`):

```
foo bar
`code` more text
```

```
- foo bar
  `code` more text
```

Verified by running, from `crates/tree-sitter-qmd/tree-sitter-markdown/`:

```
tree-sitter parse <file>
```

The expected behaviour is a single `pandoc_paragraph` with a `pandoc_soft_break` between the two lines, exactly like other inline starts (e.g. `_emph_` at line start works correctly).

### Negative control (must keep working)

Three or more backticks legitimately open a fenced code block and *should* end the paragraph:

```
foo bar
``` info
body
```
```

## Root cause

`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`, in the line-break handler around lines 2228–2316. When the scanner encounters a newline it has to decide whether the next line is:

1. A continuation of the current paragraph (emit `SOFT_LINE_ENDING`), or
2. The start of a new block that interrupts the paragraph (let the parser close it via `LINE_ENDING` and re-open).

The decision is made by a character class on the first non-whitespace character of the next line. Two parallel checks gate the `SOFT_LINE_ENDING` emission (lines 2263–2272 and 2291–2315):

```c
if ((!(s->state & STATE_INSIDE_ATX)) &&
    lexer->lookahead != '*' && lexer->lookahead != '-' &&
    lexer->lookahead != '+' && lexer->lookahead != '>' &&
    lexer->lookahead != ':' && lexer->lookahead != '#' && lexer->lookahead != '`' &&
    lexer->lookahead > ' ' && !(lexer->lookahead >= '0' && lexer->lookahead <= '9')) {
    s->state |= STATE_WAS_SOFT_LINE_BREAK;
    lexer->mark_end(lexer);
    EMIT_TOKEN(SOFT_LINE_ENDING);
}
```

The intent is "any character that *might* start a new block disqualifies a soft break." That is a sound conservative heuristic for `*`, `-`, `+`, `>`, `:`, `#` (each of those can open a block when followed by suitable context), but it is **wrong for backtick**: a single or double backtick at column zero is always an inline code span — only **three or more** consecutive backticks (`` ``` ``) open a fenced code block (see CommonMark §4.5 / GFM, and confirmed in the same scanner at `parse_fenced_code_block`, line 648: `level >= 3`).

So the scanner is over-rejecting `SOFT_LINE_ENDING` whenever a continuation line happens to begin with an inline code span.

### Why 3+ matters

`parse_fenced_code_block` (scanner.c:615–675) only emits `FENCED_CODE_BLOCK_START_BACKTICK` when `level >= 3`. With only one or two leading backticks the scanner falls through to inline parsing (`CODE_SPAN_START` at line 626–628). So at the line-break decision point, "lookahead == '`'" is overly broad — we need to count backticks before deciding.

## Proposed fix

In both branches of scanner.c that exclude backtick from soft-line-break candidates (lines ~2263–2272 and ~2291–2315), replace the bare `lexer->lookahead != '\``'` test with a count: only treat `` ` `` as a paragraph interrupter when there are **3 or more** consecutive backticks.

Approach:

1. After the lexer has already advanced past the newline and any leading indentation (lines 2241–2254), if `lexer->lookahead == '`'`, peek-advance through consecutive backticks counting them.
2. If the count is `>= 3`, leave the existing "do not emit soft break" behaviour (a fenced code block is starting).
3. If the count is `< 3`, treat the same as any other inline character: emit `SOFT_LINE_ENDING`.

The `lexer->mark_end()` call performed earlier in the block (line 2247) anchors the soft-line-ending token at the right offset (just past the newline), so advancing further to count backticks does not corrupt the emitted token's range. The same pattern is already used elsewhere in this file when the scanner needs to look ahead past the mark.

A self-contained helper such as:

```c
// Returns true if the next consecutive run of backticks is long enough to
// open a fenced code block (>= 3). Caller must have already advanced past
// the newline and indentation, and must have called mark_end() so further
// advance() calls don't extend the emitted token's range.
static bool peek_paragraph_interrupting_backticks(Scanner *s, TSLexer *lexer) {
    if (lexer->lookahead != '`') return false;
    int level = 0;
    while (lexer->lookahead == '`' && level < 3) {
        advance(s, lexer);
        level++;
    }
    return level >= 3;
}
```

… and then in both gates, the test changes from

```c
&& lexer->lookahead != '`'
```

to

```c
&& !peek_paragraph_interrupting_backticks(s, lexer)
```

Care is needed in the second gate (lines 2291–2315) because it re-uses `lexer->lookahead`. After the helper has consumed up to 3 backticks the lookahead is no longer `` ` ``, but the surrounding condition then evaluates the *new* lookahead — that's actually fine for the soft-break path, because if we got here with fewer than 3 backticks we want to treat the line like any other inline continuation, and the original-backtick check is the only character-class test that still cares. We should compute `peek_paragraph_interrupting_backticks` once and reuse the boolean.

## Test plan

### tree-sitter corpus tests (primary)

Add cases to `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/paragraph.txt` (or a new `soft_break_backtick.txt`) covering:

- [ ] **Single backtick at start of continuation line, no list** (currently fails):
  ```
  foo bar
  `code` more text
  ```
  Expect: one `pandoc_paragraph` with `pandoc_soft_break` and `pandoc_code_span`.

- [ ] **Single backtick at start of continuation line, inside list item** (the original report):
  ```
  - foo bar
    `code` more text
  ```
  Expect: one `pandoc_paragraph` inside the `list_item` with `pandoc_soft_break`.

- [ ] **Two backticks at start of continuation line** (`` ``code`` ``-style code span):
  Expect: still a soft break; a level-2 backtick run is also a code span, not a fence.

- [ ] **Three backticks at start of continuation line** (negative control):
  ```
  foo bar
  ``` lang
  body
  ```
  Expect: paragraph closes; fenced code block opens.

- [ ] **Four+ backticks** (negative control):
  Expect: same as 3-backtick case — paragraph closes.

Run with `cd crates/tree-sitter-qmd/tree-sitter-markdown && tree-sitter test`.

### Rust integration verification (secondary)

After regenerating the parser:

- [ ] `cargo run --bin pampa -- /Users/cscheid/Desktop/daily-log/2026/04/30/list-test.qmd -t native` matches `pandoc -f markdown -t native …` (single Para/Plain with SoftBreaks; no extra Para).
- [ ] `cargo nextest run --workspace` passes — in particular none of the existing snapshot/corpus tests regress.
- [ ] `cargo xtask verify --skip-hub-build` passes.

## Work items

- [x] Add failing tree-sitter corpus tests for the five cases above (paragraph.txt 4–8).
- [x] Verify they fail against the current scanner (`tree-sitter test`): 4, 5, 6 fail with the expected `pandoc_paragraph` split; 7 and 8 (negative controls, 3- and 4-backtick fences) pass.
- [x] Implement backtick peek-and-count in both gates in `scanner.c`.
- [x] `tree-sitter generate` from `crates/tree-sitter-qmd/tree-sitter-markdown/` (regenerates parser.c).
- [x] `tree-sitter test` — all 456 cases pass (added 5, regressed 0).
- [x] Run `cargo nextest run --workspace` — 8125 passed, 0 failed.
- [x] Run end-to-end pampa vs pandoc check on the original fixture: matches exactly.
- [x] Verified additional edge cases against pandoc:
   - top-level paragraph with `` `code` `` continuation → soft break ✓
   - blockquote with `` `code` `` continuation → soft break ✓
   - blockquote with lazy continuation starting with backtick → soft break ✓
   - paragraph followed by 3-backtick fence → paragraph closes, fence opens ✓
   - double-backtick code span at line start (`` ``code`` ``) → soft break ✓
- [x] Run `cargo xtask verify --skip-hub-build` — all steps pass.
- [ ] Stage and commit changes.
- [ ] Sync beads (`br close bd-af1e --reason "Fixed in <commit>"`, `br sync --flush-only`, commit `.beads/`).
- [ ] Ask user for permission to push.

## Implementation notes

The scanner's line-break handler (scanner.c:2228–2399) decides between
`SOFT_LINE_ENDING` (paragraph continues) and `LINE_ENDING` (paragraph closes)
based on a character class on the first non-whitespace character of the
next line. The original code unconditionally excluded `` ` `` from being a
soft-break candidate, but this is too aggressive: only **3+** consecutive
backticks open a fenced code block (per CommonMark, confirmed in the same
scanner at `parse_fenced_code_block`, `level >= 3` at line 648).

The fix peek-counts up to 3 consecutive backticks at two points: before
the first gate (line-start) and after `match_line` for the second gate
(post-block-prefix). The peek advances the lexer; we deliberately do
**not** call `mark_end` during the peek, so tree-sitter rewinds the lexer
to the previously-marked position (`line 2247`'s pre-indent mark) between
scan calls. For the soft-break-firing path with a leading-backtick line,
the SOFT_LINE_ENDING token's range is therefore the bare newline (not
including the indent); we set `STATE_MATCHING` and reset `s->matched` and
`s->indentation` so that on the next scan the indent is consumed as a
`block_continuation` token via `match_line`. The grammar rule
`_soft_line_break: seq(_soft_line_ending, optional(block_continuation))`
permits this shape.

This means a list-item soft-break created by this code path renders as
`(pandoc_soft_break (block_continuation))` in the parse tree, where
existing soft-break-in-list-item parses (no leading backtick) render as
`(pandoc_soft_break)` with the indent absorbed into the soft-break token.
The grammar accepts both shapes; downstream consumers (pampa) produce
identical Pandoc-AST output for both. `paragraph.txt 5` is asserted with
the with-block_continuation shape to lock in the new behaviour.

## Related code references

- Scanner line-break handler: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:2228–2342`
- The two over-eager backtick exclusions: `scanner.c:2266` and `scanner.c:2294`
- Fenced code block confirmation that 3+ is required: `scanner.c:646–648`
- Code-span (single/double backtick) path that proves `< 3` is not a fence: `scanner.c:626–628`
- Grammar-level paragraph definition: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:206–210`
- Grammar-level soft break definition: `grammar.js:878–884`

## Notes / out of scope

- The same exclusion list also rejects `*`, `-`, `+`, `>`, `:`, `#`, and digits. Those have similar pathologies in principle (e.g. a continuation line beginning `*emph*` should soft-break, not start a list), but they are out of scope for this issue. If we observe related bug reports, file follow-ups; the same `peek` pattern would generalise (e.g. require `*<space>` for a list interpretation, etc.). The user's report and CommonMark conformance only require the backtick fix here.
- Tilde fences (`~~~`) appear unsupported by this grammar; not affected.
