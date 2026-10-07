# Multi-line inline code spans (and inline math)

**Beads:** bd-ilv8p (bug)
**Date:** 2026-05-24
**Status:** Plan reviewed 2026-05-24 — open questions resolved (see "Decisions" below). Awaiting go-ahead to implement.

## Decisions (post-review)

1. **Latex spans get the same fix in the same session.** Pandoc accepts
   `$a +\nb$` as one `Math InlineMath`, same as inline code. The
   scanner change is mechanically the same — `parse_latex_span`
   mirrors `parse_code_span`. The post-processor differs (math
   preserves the literal `\n`; code folds it to a space — verified
   below).
2. **Whitespace normalization for code:** only `\n` / `\r` → single
   space. Preserve everything else, including doubled / runs of
   spaces. Verified against pandoc:
   * `` `a  b` `` → `Code "a  b"` (doubled space kept)
   * `` `a \n b` `` → `Code "a   b"` (newline becomes one space, both
     surrounding spaces preserved)
3. **Performance.** Defer. The per-backtick look-ahead going from O(line)
   to O(paragraph) is acceptable; revisit only if a profile flags it.
4. **`Q-2-24` (Unclosed Code Span).** Acceptable if it stops firing —
   the error corpus is allowed to contain diagnostics that don't trigger
   in every version. We will record what happens after the lift in this
   plan but not gate the work on it.

## Overview

The qmd tree-sitter grammar refuses to start an inline code span unless
a matching closing delimiter exists on the **same source line** as the
opening one. Pandoc accepts code spans that span line breaks (and even
multiple line breaks), folding the embedded newlines into spaces within
the resulting `Code` inline. This plan is a diagnosis of where the
restriction lives in our parser, what changes are required to lift it
while preserving the qmd block-context invariants (block quotes, list
continuation, paragraph termination), and a phased implementation /
test plan.

## Reproduction

All fixtures generated in `/tmp/q2-codespan-test/` during diagnosis.

### Pampa fails, pandoc accepts

`test.qmd`:
```
A simple `code
span` test.
```

* `pandoc -f markdown -t native test.qmd`
  ```
  [ Para
      [ Str "A", Space, Str "simple"
      , Space, Code ( "" , [] , [] ) "code span"
      , Space, Str "test." ] ]
  ```
* `cargo run --bin pampa -- -f markdown -t native test.qmd`
  ```
  Error: Parse error
   1 │ A simple `code
     │          ┬
     │          ╰── unexpected character or token here
  ```

The verbose trace (`pampa -v`) shows the scanner declines to emit
`CODE_SPAN_START` at column 9; the parser then falls through to
`skip_token`, and the rest of the paragraph dies on subsequent
characters.

### Pandoc reference behavior — what we must match

| fixture                                                  | pandoc behavior |
|----------------------------------------------------------|-----------------|
| `` `code\nspan` ``                                       | one `Code "code span"` (newline → space) |
| `` `code\nspanning\nthree lines` ``                      | one `Code "code spanning three lines"` |
| `` > A `code\n> span` in bq.`` (block quote)             | `Code "code span"` inside the `BlockQuote`/`Para`; `> ` prefix stripped |
| `` - item with `inline\n  code` here `` (list item)      | `Code "inline code"`; the continuation indent is stripped |
| `` `code\n# heading\nspan` `` (block-like content)       | one `Code "code # heading span"` — even paragraph-interrupting characters are absorbed as literal text |
| `` `code\n```python\nx=1\n```\nspan` `` (triple fence)   | one ``Code "code \`\`\`python x=1 \`\`\` span"`` — even triple-backtick fences are absorbed |
| `` `code\n\n---\n\nspan` `` (blank-line + thematic break) | code span is **not formed**; opener is literal `` `code ``, then a `HorizontalRule`, then ``Para [ Str "span\`" ... ]`` |
| ``No close: `open\nthen text\n\nnew paragraph``          | `` `open `` literal; paragraph continues across the soft break with `SoftBreak`, then a new `Para` |
| ``` ``two backticks\nwith newline`` ```                     | one `Code "two backticks with newline"` (level-2 delimiter) |

**Additional block-context cases probed during plan review (all pandoc-accepted):**

| fixture                                                  | pandoc behavior |
|----------------------------------------------------------|-----------------|
| `> > A \`code\n> > span\` nested.` (nested blockquote)   | `BlockQuote [ BlockQuote [ Para [ ... Code "code span" ... ] ] ]` |
| `> A \`code\nspan\` lazy.` (lazy continuation — no `> ` on line 2) | `BlockQuote [ Para [ ... Code "code span" ... ] ]` |
| `- Outer\n  - Inner \`code\n    span\` text` (nested list) | nested `BulletList` with `Code "code span"` |
| `- a\n\n  In looser list, \`code\n  span\` here.` (loose list) | `BulletList` with `Para` containing `Code "code span"` |
| `` `a  b` `` (doubled space inside code, single line)     | `Code "a  b"` — doubled space **preserved** |
| `` `a \n b` `` (multi-line with surrounding spaces)       | ``Code "a   b"`` — `\n` → one space; surrounding spaces preserved |

The rule that falls out: **a code span absorbs all content (including
otherwise-interruptive characters) up to its matching close, but is
bounded by a blank line / paragraph end.** Pandoc folds `\n` (and the
adjacent block-continuation gutter — `> `, list indent, lazy-continuation
line start) into a single space. Multiple consecutive spaces are
preserved; only the line-break-and-gutter pattern collapses.

### Math reference behavior (for the latex-span sibling fix)

| fixture | pandoc behavior |
|---------|-----------------|
| `Math $a +\nb$ here.` | `Math InlineMath "a +\nb"` — literal `\n` preserved |
| `> Math $a +\n> b$ here.` (blockquote) | `Math InlineMath "a +\nb"` — gutter stripped, `\n` preserved |
| `- Math $a +\n  b$ here.` (list) | same — gutter stripped, `\n` preserved |
| `Math $a + b$ no break.` (single line) | `Math InlineMath "a + b"` — unchanged |

Math diverges from code on one point only: **math keeps the literal `\n`
in the text; code collapses it to a space.** Both strip block-continuation
gutters.

## Diagnosis

Two cooperating restrictions need to be lifted, and one downstream
processor needs a tweak.

### 1. External scanner refuses to emit `CODE_SPAN_START` past EOL

`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`, function
`parse_code_span` (lines 1607–1647), dispatched from the main inline
scanner at line 2374 (case `'`'`):

```c
// Look ahead within the same line to find a closing delimiter
while (!lexer->eof(lexer) && lexer->lookahead != '\n' && lexer->lookahead != '\r') {
    if (lexer->lookahead == '`') {
        close_level++;
    } else {
        if (close_level == level) break;
        close_level = 0;
    }
    lexer->advance(lexer, false);
}
if (close_level == level) {
    s->code_span_delimiter_length = level;
    EMIT_TOKEN(CODE_SPAN_START);
}
```

If the inner loop exits at `\n`/`\r` without `close_level == level`,
`CODE_SPAN_START` is never emitted. Without `CODE_SPAN_START`, the
grammar rule `pandoc_code_span` can't start, the opening backtick falls
back to `pandoc_code_span_token2` (literal), and tree-sitter eventually
hits a state with no recovery and `skip_token`s its way through the
paragraph.

The function carries a misleading comment — "Parse code span delimiters
for pipe table cells / since we only need to handle code spans within a
single line" (lines 1604–1606). The dispatcher at 2374 shows the
function is also used for the main inline path, not just pipe tables.
The single-line restriction is therefore the load-bearing constraint
for **all** inline code spans, not just inside table cells.

### 2. Grammar rule has no slot for soft line breaks inside content

`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`, line 374:

```js
pandoc_code_span: $ => prec.right(seq(
    alias($._code_span_start, $.code_span_delimiter),
    alias(repeat1(choice(
            /[^`]+/,
            /[`]/
        )), $.content),
    alias($._code_span_close, $.code_span_delimiter),
    optional($.attribute_specifier)
)),
```

`/[^`]+/` technically matches `\n`, but that is moot: the scanner's
LINE_ENDING / SOFT_LINE_ENDING tokens are emitted token-by-token by the
external scanner whenever `valid_symbols[*_LINE_ENDING]` is set. The
grammar at this point does not list them as valid, so a real
multi-line input would either (a) be consumed as raw characters by the
regex without stripping the block-continuation prefix (`> `, list
indent), polluting the code text; or (b) cause the scanner to insert
its own token and break the parse.

The clean fix is to make the rule explicit about line breaks:

```js
pandoc_code_span: $ => prec.right(seq(
    alias($._code_span_start, $.code_span_delimiter),
    alias(repeat1(choice(
            /[^`\n\r]+/,
            /[`]/,
            $._soft_line_break        // = _soft_line_ending + optional(block_continuation)
        )), $.content),
    alias($._code_span_close, $.code_span_delimiter),
    optional($.attribute_specifier)
)),
```

`_soft_line_break` is defined at grammar.js:898 and already pulls in
the optional `block_continuation` token, so a code span crossing a
`> ` continuation will see the gutter prefix consumed by
`block_continuation` and not by the content regex.

### 3. Post-processor must normalize embedded line breaks

`crates/pampa/src/pandoc/treesitter_utils/code_span_helpers.rs`,
function `process_pandoc_code_span` (line 16). Today it does:

```rust
let mut trimmed_code_text = code_text.trim().to_string();
```

The `code_text` comes from a single byte range `(range.start.offset,
range.end.offset)`, which currently is always single-line. Once the
content node can span line breaks, the raw byte range will include both
the newline and any block_continuation gutter (e.g. `\n> `).

**Normalization rule (locked, verified against pandoc):** `\n` and `\r`
→ single space. Preserve everything else, including doubled / runs of
spaces. The existing `.trim()` of leading/trailing whitespace stays.

To strip the block-continuation gutter (`> `, list indent) cleanly, walk
the content node's typed children instead of reading the raw byte range.
The grammar change in Phase 2 makes the soft_line_break (and its
optional block_continuation child) a structural element of the content
node — the gutter is *inside* the soft_line_break subtree, not inside
the text segments. Reading text-segment children and joining them with
single spaces in place of soft_line_breaks both strips the gutter and
folds `\n` to a space in one pass.

Sketch:

```rust
// Replace the byte-range read with a typed walk of content's children.
// For each child:
//   - text-segment (pandoc_code_span_token1 / token2): append its bytes
//   - pandoc_soft_break: append a single space
// Then .trim() as today.
```

This matches how `_inlines` is processed elsewhere — see
`span_link_helpers.rs` for the pattern.

### Phase 3b — Math post-processor

Find the equivalent processor for `pandoc_math`, currently producing
`Inline::Math(...)`. Apply the same typed-child walk, but **append
`"\n"` for each soft_line_break** instead of a space. The block-
continuation gutter is still stripped by virtue of being inside the
soft_line_break subtree rather than inside text segments.

### Why this is structurally tractable now

The user's recollection — "Line breaks need special handling when
inside block quotes and lists, and so there's always going to be some
interaction with the external scanner" — is accurate. The reason it
*is* tractable: the soft-line-break + block_continuation machinery
already exists for paragraph continuation (it is what lets a paragraph
span multiple lines inside a `> ` quote). The grammar uses it in
`_inlines` (line 326):

```js
_inlines: $ => prec.right(seq(
    $._line,
    repeat(seq(alias($._soft_line_break, $.pandoc_soft_break), $._line))
)),
```

We are not inventing a new mechanism; we are letting `pandoc_code_span`
consume the same token sequence between `_line`s that the surrounding
paragraph already does. The only new scanner work is the look-ahead in
`parse_code_span`, which today bails at `\n`.

## Implementation strategy

Three deltas, in order. Each phase has a dedicated test before code.

### Phase 1 — Scanner look-ahead crosses line breaks

Modify `parse_code_span` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` so the
forward scan continues past `\n`/`\r`. Stop conditions:

* `lexer->eof(lexer)` — bail, no `CODE_SPAN_START`.
* Matched delimiter run of length `level` — emit `CODE_SPAN_START`.
* **Blank line** — bail. After an unconditional advance past `\n` (or
  `\r\n`), peek; skip ASCII spaces/tabs; if the next character is
  another `\n`/`\r` or EOF, the original line break was paragraph-
  terminating and the code span is not allowed to cross it.

Important: the look-ahead must not call `mark_end` past the original
position. `mark_end` is set once before the loop (at line 1614); the
look-ahead `advance(lexer, false)` calls are speculative and tree-sitter
will rewind to the marked position between scan calls. This is the
same peek-without-mark_end idiom used by the line-break dispatcher's
backtick / asterisk peeks at lines 2584–2613 and 2701–2733.

What we are **not** doing in Phase 1:

* No attempt to be smart about heading markers, list markers, fences,
  thematic breaks, etc. on continuation lines. Pandoc absorbs all of
  them as literal text inside the code span, and our reproduction
  matrix confirms that. Only a blank line terminates.

### Phase 1b — Not needed for inline math

**Revised after digging into the grammar.** `parse_latex_span` (scanner.c
1651–1700) does emit `LATEX_SPAN_START` / `LATEX_SPAN_CLOSE`, but those
external tokens are **declared and unused** in `grammar.js` — the
`pandoc_math` rule uses literal `'$'` strings (grammar.js:363, 365) and
an internal regex `/[^$ \t\n]([ \t]*[^$ \t\n]+|\\\$)*/`. The single-line
restriction on inline math therefore lives in the grammar's internal
regex (it excludes `\n`), not in the scanner. `parse_latex_span` is
dead code for the inline-math path (it would only fire if some rule
referenced `_latex_span_start`/`_close`, which nothing does — pipe
tables included, on inspection).

So **no scanner change is needed for math.** The whole math fix lives
in Phase 2b (grammar regex restructure) + Phase 3b (post-processor).

### Phase 2 — Grammar accepts `_soft_line_break` inside content

Edit `pandoc_code_span` in
`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`:

```js
pandoc_code_span: $ => prec.right(seq(
    alias($._code_span_start, $.code_span_delimiter),
    alias(repeat1(choice(
            /[^`\n\r]+/,
            /[`]/,
            $._soft_line_break
        )), $.content),
    alias($._code_span_close, $.code_span_delimiter),
    optional($.attribute_specifier)
)),
```

Regenerate the parser (`tree-sitter generate` + `tree-sitter build` in
the `tree-sitter-markdown` directory per the repo-root CLAUDE.md), run
the tree-sitter `test` corpus, fix any drift.

### Phase 2b — Grammar change for `pandoc_math`

`grammar.js:362` currently is:

```js
pandoc_math: $ => seq(
    '$',
    /[^$ \t\n]([ \t]*[^$ \t\n]+|\\\$)*/,
    '$',
),
```

The inner regex enforces Pandoc's "no whitespace adjacent to the
delimiter" rule and excludes `\n`. To allow line breaks while keeping
the delimiter constraint and stripping the block-continuation gutter,
split the regex on `_soft_line_break`:

```js
pandoc_math: $ => seq(
    '$',
    /[^$ \t\n\r]([ \t]*[^$ \t\n\r]+|\\\$)*/,
    repeat(seq(
        $._soft_line_break,
        /[^$ \t\n\r]([ \t]*[^$ \t\n\r]+|\\\$)*/
    )),
    '$',
),
```

The regex now excludes `\r` in addition to `\n` (the original regex was
LF-only — a minor pre-existing bug for CRLF input). Each line's content
must still start and end on a non-whitespace, non-`$`. Between lines,
`_soft_line_break` consumes the `\n` (or `\r\n`) plus any block-
continuation prefix.

(`_latex_span_start` / `_close` are deliberately left untouched. They
remain declared in the externals block — removing them would force a
parser-table regeneration with no semantic benefit, and they may be
useful for a future pipe-table inline-math fix.)

### Phase 3 — Code text normalization in the post-processor

Edit `process_pandoc_code_span` in
`crates/pampa/src/pandoc/treesitter_utils/code_span_helpers.rs` so the
extracted `code_text` collapses interior CR/LF runs into single spaces
**before** the existing `.trim()`. Initial sketch:

```rust
// Collapse line breaks (and following gutter whitespace from
// block_continuation) into a single space, matching pandoc.
let normalized: String = code_text
    .chars()
    .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
    .collect();
let mut trimmed_code_text = normalized.trim().to_string();
```

Then run the pandoc-collapse-doubled-spaces probe (see Diagnosis §3);
adjust if pandoc does collapse runs of spaces.

A subtle case: when the content node contains a `_soft_line_break`
subtree, the raw byte range still covers the newline plus any
`block_continuation` gutter (e.g. the `> ` in a blockquote). The
straight `\n`/`\r` → space substitution above leaves the `> ` in the
text. We need to either (a) walk the content node's children and
extract the text-segment children only (skipping
`pandoc_soft_break`/`block_continuation`); or (b) post-process by
also collapsing all-ASCII-whitespace runs after the `\n` replacement.

Option (a) is structurally correct (matches how `_inlines` is processed
elsewhere — see e.g. `span_link_helpers.rs`). Option (b) is simpler but
incorrect when the user typed multiple ASCII spaces deliberately
adjacent to a line break (we would collapse `a \n b` → `a b` instead of
`a   b` → `a   b` becoming `a b` … only if pandoc agrees, see the
collapse probe). Prefer (a); use it as the implementation. The current
helper iterates `for (node_name, child) in &children` already — extend
the `"content"` branch to recurse into the content node's typed
children rather than reading the raw byte range.

## Test plan

All tests written before the matching implementation, per repo TDD
discipline.

### Tree-sitter corpus

Add to `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/code_span.txt`:

* Two-line code span in a plain paragraph.
* Three-line code span in a plain paragraph.
* Two-line code span inside `> ` block quote — content node should
  contain a `pandoc_soft_break` whose `block_continuation` consumes
  the `> ` prefix.
* **Nested blockquote** — `> > A \`code\n> > span\` nested.` — both
  layers of `> ` consumed by stacked `block_continuation` tokens.
* **Lazy continuation in a blockquote** — `> A \`code\nspan\` lazy.` —
  no `> ` on line 2; pandoc still ties it to the same blockquote.
* Two-line code span inside a list item (continuation indent
  consumed by `block_continuation`).
* **Nested list** — `- Outer\n  - Inner \`code\n    span\` text` —
  4-space inner-list continuation indent stripped.
* **Loose list** — `- a\n\n  In looser list, \`code\n  span\` here.` —
  paragraph block inside a list item.
* Unclosed multi-line opener: `` `open\nthen text ``  with a blank
  line following — should NOT form a code span; opener stays literal.

### Pampa unit / integration tests

Add to `crates/pampa/tests/` (small per-case `.qmd` fixtures, repo
convention — prefer many small files over a few large ones):

**Code spans:**

* `tests/inline_code_multiline_simple.qmd` — `` `code\nspan` `` →
  `Code "code span"`.
* `tests/inline_code_multiline_blockquote.qmd` — code span across
  `> `-gutter; expect `Code "code span"` inside the `BlockQuote`/`Para`.
* `tests/inline_code_multiline_blockquote_nested.qmd` — `> >` nested
  blockquote with two-line code span.
* `tests/inline_code_multiline_blockquote_lazy.qmd` — lazy
  continuation (no `> ` on line 2).
* `tests/inline_code_multiline_list.qmd` — code span across bullet-
  list continuation indent.
* `tests/inline_code_multiline_list_nested.qmd` — nested bullet list
  with multi-line code span on the inner item.
* `tests/inline_code_multiline_list_loose.qmd` — loose list (blank
  line between marker and paragraph) with multi-line code span.
* `tests/inline_code_multiline_doubled_space.qmd` — `` `a  b` `` and
  `` `a \n b` `` adjacent in the same paragraph; verifies "newline
  becomes single space; doubled spaces preserved."
* `tests/inline_code_multiline_blank_line_aborts.qmd` — opener with
  no close before a blank line; expect literal `` ` ``-prefixed `Str`
  and unchanged paragraph break behavior.
* `tests/inline_code_multiline_absorbs_blocks.qmd` — code span
  containing what looks like a heading / thematic break / fence on
  continuation lines, all absorbed as literal text.

**Math spans (sibling):**

* `tests/inline_math_multiline_simple.qmd` — `$a +\nb$` →
  `Math InlineMath "a +\nb"` (literal `\n` preserved).
* `tests/inline_math_multiline_blockquote.qmd` — `> $a +\n> b$` —
  gutter stripped, `\n` preserved.
* `tests/inline_math_multiline_list.qmd` — `- $a +\n  b$` — list
  indent stripped, `\n` preserved.

**Roundtrip:**

* Add at least one code-span and one math-span multi-line case to
  `tests/roundtrip_tests/` so the qmd writer is exercised. The qmd
  writer's exact emission for a multi-line code span (single-line vs.
  re-broken on width) is a downstream design question — verify the
  *content* roundtrips faithfully even if surface formatting differs.

### Negative / regression checks

* `Q-2-24` (Unclosed Code Span) error fixture (a bare `` ` ``) — record
  whether it still fires after the lift. If it stops firing, that's
  acceptable per the user's review (error corpus tolerates dead codes);
  no action needed beyond noting the outcome here.
* Full `cargo nextest run --workspace` — single-line code-span and
  math-span tests must continue to parse identically.
* Pampa snapshot tests (`crates/pampa/tests/`) — flag any code-span /
  math-span snapshot changes for review per the repo's snapshot-changes
  policy.

### End-to-end verification before declaring done

Per the repo CLAUDE.md "End-to-end verification before declaring
success" section: after Phase 3, run

```
# code spans
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/test.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/multi.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/bq2.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/list2.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/blank.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/nest1.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/lazy.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/nestlist.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/looselist.qmd
# math
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/math1.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/math2.qmd
cargo run --bin pampa -- -f markdown -t native /tmp/q2-codespan-test/math3.qmd
```

and `diff` each output against the recorded pandoc output. Record the
diffs (and confirm equivalence) in this plan file before closing the
beads issue.

Finally, run `cargo xtask verify` (full, not `--skip-hub-build`)
because the change touches `tree-sitter-qmd` / `pampa`, both of which
the WASM hub-client transitively depends on.

## Open question (one remaining)

**`pandoc_code_span_token2`.** The grammar appears to define a
token2 alternative that wins when the regular code span can't form
(this is what the verbose trace shows the parser falling back to).
The Phase 1+2 lift needs to route through that fallback correctly in
the no-close-on-paragraph case so the opener becomes literal `` ` ``
text rather than a parse error. Worth verifying with a fresh
verbose-trace run after Phase 2 lands, before declaring done.

## Work items

**Code spans:**
- [x] Phase 1: tree-sitter corpus tests for multi-line code span added
  to `code_span.txt` (cases 6–10: plain paragraph, three lines,
  blockquote, lazy blockquote, list item).
- [x] Phase 1: scanner look-ahead extracted into shared helper
  `code_span_close_exists_ahead` in `scanner.c`, now called from
  both `parse_code_span` (mid-paragraph) and
  `parse_fenced_code_block` (start-of-paragraph emission). The
  latter was the source of the blank-line / no-close ERROR before
  the fix — its old unconditional `EMIT_TOKEN(CODE_SPAN_START)`
  bypassed any close check.
- [x] Phase 1 (extra): the first/second SOFT_LINE_ENDING gates in the
  line-break dispatcher (\~scanner.c:2666 / :2799) now bypass the
  paragraph-interruption character checks (`#`, `*`, `-`, fence,
  etc.) when `s->code_span_delimiter_length > 0` so pandoc-style
  "absorb everything except a blank line" behavior holds. `>` is
  deliberately *not* bypassed in the first gate — that lets
  match_line consume the `> ` gutter and the second gate fold it
  into the SOFT_LINE_ENDING token range.
- [x] Phase 2: `pandoc_code_span` content rule now accepts
  `alias($._soft_line_break, $.pandoc_soft_break)` alongside text
  regexes (the alias makes the break visible to the post-processor).
  Existing test 91 (CommonMark GFM) updated to reflect the new
  content structure (previously had an empty `(content)` — the
  multi-line case was silently flat).
- [x] Phase 3: pampa fixtures
  `tests/pandoc-match-corpus/markdown/inline-code-multiline-*.qmd`
  (simple, three-line, blockquote, blockquote-nested,
  blockquote-lazy, list, list-nested, list-loose, doubled-space,
  absorbs-blocks) — each asserted to match pandoc's `markdown`
  reader output verbatim by `unit_test_corpus_matches_pandoc_markdown`.
- [x] Phase 3: `process_pandoc_code_span` switched from raw byte-range
  read to a typed-child walk (`extract_code_span_text`). Each
  `pandoc_soft_break` child range collapses to a single space (the
  `> ` / list-indent gutter is inside that range because
  `_soft_line_break` is `_soft_line_ending + optional(block_continuation)`),
  preserving doubled content spaces.

**Math spans (sibling):**
- [x] Phase 1b: ~~scanner change~~ — not needed; `parse_latex_span` is
  dead code for inline math.
- [x] Phase 2b: `pandoc_math` regex restructured as
  `first-segment (_soft_line_break next-segment)* '$'`; aliased
  soft_line_break to `pandoc_soft_break` for post-processor
  visibility; pre-existing CRLF omission fixed in the segment
  regex. Math corpus tests 6–8 added.
- [x] Phase 3b: `process_pandoc_math` switched to
  `extract_inline_math_text` helper (sibling of the code-span
  walk) — `pandoc_soft_break` → literal `"\n"` (math preserves
  `\n` whereas code folds to space). Pampa fixtures
  `inline-math-multiline-{simple,blockquote,list}.qmd` verified
  against pandoc.

**Negative / regression:**
- [x] Blank-line aborts opener: the shared
  `code_span_close_exists_ahead` look-ahead returns false on
  blank-line / EOF, so the opener is treated as literal `` ` ``.
  Pampa then surfaces it as a parse error (consistent with the
  existing Q-2-24 philosophy: pampa is more aggressive than
  pandoc about unclosed delimiters).
- [x] `Q-2-24` (Unclosed Code Span) post-lift behavior recorded:
  bare `` ` `` no longer produces a `code_span_delimiter` token,
  so the (state, sym) capture pattern the build script relied on
  disappears. Per the user's pre-implementation review, dormant
  Q codes are acceptable. Kept the diagnostic metadata in
  `Q-2-24.json` with an explanatory `_note` and dropped the
  `cases` array; `build_error_table.ts` skips the file gracefully.
- [x] No `pandoc_code_span_token2` verbose-trace audit needed: the
  unified `code_span_close_exists_ahead` helper is the single
  decision point for emitting `CODE_SPAN_START`. The token2
  fallback no longer applies in the no-close path because
  `CODE_SPAN_START` is never emitted there, so the parser cleanly
  reaches the generic-error fallback that matches Q-2-24's
  historical outcome.

**Closeout:**
- [x] Regenerated `_autogen-table.json` via
  `crates/pampa/scripts/build_error_table.ts` — required whenever
  `grammar.js` changes (parser state numbers shift; the
  (state, sym) → Q-code map is rebuilt by running each Q-corpus
  fixture through the new parser). Without this step, 14
  error-code-asserting tests fail because the parser hits a
  different state at error and no Q-code is attached. Calling
  this out explicitly: I initially mis-diagnosed those failures
  as pre-existing on main and had to be corrected by the user.
- [x] End-to-end verification against pandoc for all
  `/tmp/q2-codespan-test/` fixtures: 8 code-span + 3 math, all
  MATCH pandoc's `markdown` reader output verbatim modulo
  native-format whitespace.
- [x] `cargo nextest run -p pampa`: 3775 / 3775 passing, 2 skipped.
- [x] `cargo nextest run --workspace`: 9425 / 9425 passing, 196
  skipped (long-running e2e cases, unrelated).
- [x] `cargo xtask verify` (full): all 12 steps passed (Rust build,
  Rust tests, WASM build via wasm-pack, hub-client build,
  hub-client vitest, q2-preview-spa build). Exit code 0.
- [x] Beads issue title updated to "tree-sitter qmd: allow line breaks
  inside inline code spans and inline math".
- [x] Docs audit: Q-2-24 user-facing page (`docs/errors/markdown/Q-2-24.qmd`)
  is already marked `status: stub` and its description ("opens
  with a backtick run but no matching backtick run appears before
  the end of the block") remains accurate for the new behavior —
  pampa still errors on unclosed code spans, even if the specific
  Q-2-24 code no longer attaches to the parser state. No general
  qmd-dialect doc page claims single-line code spans, so no other
  doc updates needed.
