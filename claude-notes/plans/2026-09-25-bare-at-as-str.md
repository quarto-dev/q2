# Bare `@` (not a citation) is literal text, where it cannot be a typo

**Status:** implemented and verified 2026-09-25 on
`braid/bd-bare-at-literal-w3ytmu8e-bare-not-citation-uncoded`. Committed
locally, not pushed.
**Tracking issue:** bd-bare-at-literal-w3ytmu8e (filed 2026-09-23 as a child of
bd-uk8zgkha; this plan attaches to it rather than filing a duplicate)
**Follow-ups filed:** bd-2o8rq2xj (Q-code + `\@` hint for the kept `@`
errors), bd-0idqzj33 (other uncoded parse-error sources, split from this
strand's original scope), bd-5qmh5acq (Unicode citation keys)
**Owner:** cscheid
**Last updated:** 2026-09-25
**Precedents:** bd-j9cf (bare `<` as Str, `2026-05-18-bare-lt-as-str.md`),
bd-star-as-str-qigl02pz (`2026-09-25-star-as-str.md`, PR #731), which
added the flanking rules and the renamed `LITERAL_STR` token this plan reuses.

## Overview

Any `@` that does not start a well-formed citation fails the whole
document with an **uncoded** `Error: Parse error` (no Q-code). The
dominant real-world source is the plan-header idiom
``**Branch:** `main` @ `6bee9ebe` ``. Pandoc treats every such `@` as
literal text.

**Policy (Carlos, 2026-09-25): don't simply follow pandoc here.** People
get complex Markdown syntax wrong. A parser that silently guesses what
they meant lets a typo reach production. So an `@` becomes literal only
where it is **unlikely to be a mistyped citation**:

- a **standalone** `@`: whitespace or line start before it, and
  whitespace, end of line, or end of file after it (`main @ sha`,
  `Q&A @ 3pm`);
- a standalone-preceded `@` directly followed by a **quote**, straight
  or curly (`a @"quoted" b`, `a @“quoted” b`; curly added 2026-09-25,
  see Decisions);
- a **word-final** `@` (`word@`, as in `pkg@ latest` or `a@`), which
  is consistent with `word@word` being a word.

Every other non-citation `@` **keeps its error**, because it looks like
citation syntax with a mistake in it: `@-foo` (meant `-@foo`), `@,` /
`(see @)` / `[see @]` (missing key), `-@` alone, `@@`, `@}`, `@{weird
key}`. The error should improve, though: those errors are still
uncoded, and giving them a Q-code is follow-up bd-2o8rq2xj.

## Why `@` errors today

`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`:

```c
case '@':
    return parse_cite_author_in_text(s, lexer, valid_symbols);
```

`parse_cite_author_in_text` emits `CITE_AUTHOR_IN_TEXT` whenever the parser
state allows it, without looking at what follows the `@`. The grammar
then requires a key (`[0-9A-Za-z_]+([:.#$%&+?<>~/-][0-9A-Za-z_]+)*`) and
fails when it doesn't find one. For `a @ b`, tree-sitter gives
`(ERROR (pandoc_str) (citation_delimiter) (shortcode_name))`.

## Behaviour, measured and decided

The probes ran on `main` at `ce01489c4` with a freshly built
`target/debug/pampa -t native` and `pandoc 3.11 -f markdown -t native`.
"pampa today" is ERR for every row in A1 and A2.

### A1. Error today → literal after this change

| input | pandoc (target AST) | tier |
|---|---|---|
| `a @ b` | `Str "a", Space, Str "@", Space, Str "b"` | 1 |
| `@` (whole doc) | `Str "@"` | 1 |
| `@ b`, `@ at start` | `Str "@", Space, …` | 1 |
| `a @` (EOL / EOF) | `…, Space, Str "@"` | 1 |
| `first @\nsecond` | `… Str "@", SoftBreak, Str "second"` | 1 |
| `text\n@ more` | `Str "text", SoftBreak, Str "@", Space, …` | 1 |
| `a\t@\tb`, `a  @  b` | as `a @ b` | 1 |
| `x @ y @ z` | both literal | 1 |
| `Q&A @ 3pm`, `10 @ $5 each` | literal | 1 |
| `- a @ b` / `> a @ b` / `# a @ b` | list / blockquote / header, literal `@` | 1 |
| `\| x @ y \|` pipe table | table, literal | 1 |
| `[a @ b]{.x}` / `[a @ b](u)` / `*a @ b*` | span / link / emph, literal | 1 |
| `a @"quoted" b` | `Str "@", Quoted DoubleQuote […]` | 1 |
| `a @'x' b` | `Str "@", Quoted SingleQuote […]` | 1 |
| `a @“quoted” b`, `a @‘x’ b`, `a @” b` | `Str "@“quoted”"` (curly quotes are plain text) | 1 |
| `a@ b`, `a@` (EOL / EOF) | `Str "a@"` | 2 |
| `a@.`, `a@,` | `Str "a@."` | 2, by construction (see tier 2) |

### A2. Error today → stays an error (likely a citation typo)

| input | pandoc | reason to keep the error |
|---|---|---|
| `@-foo`, `a @- b` | literal | probably `-@foo` |
| `@:foo`, `@~ x`, `@< x` | literal | a key with a mangled first character |
| `a @, b`, `a @. b` | literal | key missing before `,` / `.`, as in `[see @, p. 3]` |
| `(see @)`, `[see @]` | literal | citation with the key missing |
| `a (@) b` | `Str "(@)"` | a stray example marker, or `(@label)` with no label |
| `a -@ b`, `-@`, `a -@` | `Str "-@"` | suppress-author citation with no key |
| `a @@ b`, `@@`, `@@foo` | `Str "@@"` / `Str "@", Cite` | doubled sigil |
| `a @} b`, `@{`, `@{foo`, `@{weird key}` | literal | braced key, malformed |
| `x ~@ y`, `(@ b` | literal | `@` glued to punctuation; ambiguous (and see the lexing note under tier 1) |
| `**@**` | `Str "**", Cite["*"], Str "*"` | pandoc oddity |
| `@é`, `café @élan` | `Cite["é"]` | a *real* key our ASCII-only regex rejects; follow-up |
| `@{}` | `Cite[""]` | empty key |

### B. Success today → different success (comes with tier 2)

Pandoc's textual-citation parser refuses an `@` directly after an
alphanumeric Str (`notAfterString`). We split the word and emit a
`Cite` instead, which renders as an unresolved `???` in the middle of an
email address when citeproc runs:

| input | pampa today | pandoc = after tier 2 |
|---|---|---|
| `user@example.com` | `Str "user", Cite[example.com]` | `Str "user@example.com"` |
| `foo@bar` | `Str "foo", Cite[bar]` | `Str "foo@bar"` |
| `mermaid@11`, `std@0.224.0` | `Str "mermaid", Cite[11]` | `Str "mermaid@11"` |

A `word@word` isn't a working citation today, so it has no current
meaning to preserve: it silently produces a broken `Cite`. Making it a
word is the pandoc reading and removes that silent failure.

### C. Correct today, must not change

| input | both |
|---|---|
| `@ref`, `see @ref for`, `@123`, `@_foo`, `@types/node`, `@dataclass` | `Cite` (AuthorInText) |
| `-@ref` | `Cite` (SuppressAuthor) |
| `[@ref]`, `[see @a, p. 3; @b]` | `Cite` (NormalCitation) |
| `@foo.`, `@foo-`, `@foo: x` | `Cite[foo]` then `Str "."` / `"-"` / `":"` |
| `(@foo)`, `see (@foo) here`, `"@foo"` | `Cite` inside the punctuation |
| `@{https://example.com}`, `-@{…}` | braced-key `Cite` |
| `(@) item` at block start | example list |
| `\@ b` | `Str "@"` |
| `` `a @ b` `` | `Code` |
| `<a@b>`, `<sales@example.com>` | email autolink (bd-email-autolink-dropped-2jj38iiv) |

## Measurement over `claude-notes/` (bd-uk8zgkha corpus)

The scan uses the same method as the star-as-str plan: run
`pampa --json-errors --no-prune-errors` on each of the 1443 `.md` files
and record the first error.

- **Clean today:** 718 / 1443, the same as the post-#731 figure.
- **First error sits on an `@`:** 141 files. Almost all are the
  `` `main` @ `sha` `` header idiom. A few are ``main @⏎`sha` `` (`@` at
  EOL) or `` `[Idea]` @ 15 ``.
- **Simulated fix:** the failing files were rewritten outside code,
  with the *revised* rule (standalone single `@` followed by
  whitespace, EOL, or a quote), then rescanned. **+50 → 768 clean.**
  Adding the word-final and intraword rule changes nothing (+50). The
  first, broader simulation, which also accepted `@@`, `@,` and the
  like, gave the same +50. **The conservative rule gives up none of the
  corpus win.** The other 91 of the 141 then stop at their next error,
  mostly Q-2-7, which is the next class to attack for bd-uk8zgkha.
- **Intraword `@` in prose (table B population):** 26 occurrences in 15
  files, such as `rustsec@googlegroups.com`, `mermaid@11`, and
  `std@0.224.0` outside backticks.

## Design

### Tier 1: standalone `@` → literal (scanner only)

In `parse_cite_author_in_text`, record `preceded_by_ws =
delimiter_preceded_by_ws(s, lexer)` **before** advancing, as
`parse_star` does. Then advance past `@` and:

```c
if (lookahead == '{' && valid[CITE_AUTHOR_IN_TEXT_WITH_OPEN_BRACKET])  -> as today
else if (lookahead is [A-Za-z0-9_] && valid[CITE_AUTHOR_IN_TEXT])      -> as today
else if (preceded_by_ws && (at_ws_or_eol(lexer) || lookahead is '"' or '\'')
         && valid[LITERAL_STR]) { mark_end; EMIT_TOKEN(LITERAL_STR); }  // just the '@'
else                                                                  -> as today (error)
```

- The key-start checks come first and are unchanged, so no citation can
  become literal.
- `parse_cite_suppress_author` (`-@`) is **not** changed; `-@` without a
  key keeps its error.
- Non-ASCII after `@` is neither a key start nor whitespace or a quote,
  so `@é` and `@“` keep today's error.
- Leading whitespace follows the same path as `*` and `<`. The scanner
  folds it into the token range, and pampa's `pandoc_str` arm splits it
  back into a `Space`. Nothing new is needed there, but the tests check
  the source ranges.
- **Lexing note:** the scanner cannot look behind. "Preceded by
  whitespace" comes from `ws_before_token` or column 0. For an `@`
  glued to a previous character, it can't tell `a@ b` from `(@ b` or
  `~@ b`. That is why tier 1 accepts only the whitespace-preceded
  shape, and `word@` is handled by the lexer's word regex instead
  (tier 2). `(@ b` and `~@ b` therefore keep their error.
- **Line-content start (resolved, not a limitation).** The risk was
  real and wider than containers. Measured: `> @ b`, `> a\n> @ b`,
  `- @ b`, `1. @ b`, `(@) @ b`, `- a\n\n  @ b`, and plain
  `text\n  @ more` all errored after the first cut of tier 1. The
  reason: a block-quote marker, list marker, soft line ending, or block
  continuation token swallows the line's prefix *and the whitespace in
  it*, so the next token sees `ws_before_token == 0` and a nonzero
  column. (`# @ b` and table cells were fine.)

  First attempt: a boolean flag set after those tokens and cleared on
  every `scan()` call. **Unsound**, and the `:error` probes (`> (@ b`,
  `- (@ b`, `text\n  (@ b`, `>(@ b`) caught it. Tree-sitter restores
  the state serialized with the *last external token* before every
  `scan()` call. A clear during a call that returns false is discarded,
  so the flag survived the internal `(` token and leaked to the `@`.

  Shipped: a **column anchor**. `Scanner.line_content_column` (column +
  1, serialized) records where line content starts. It's set through
  `note_line_content_start()` only at emit sites where the token end is
  provably the lexer's current position. Those sites are the block-quote
  marker; block continuation (`match_line` never calls `mark_end`); the
  no-peek branches of both `SOFT_LINE_ENDING` gates; `-`/`*` list
  markers when the run length is 1; and the `+`, ordered, and example
  markers. The peeking branches deliberately leave the token end behind
  the lexer, so they don't anchor. The entry point copies the anchor on
  every successful scan, so any external token replaces it. The `@`
  rule's `at_ws_or_line_content_start()` compares the current column to
  the anchor. That's exact within a line, since the column only grows.
  Across lines it relies on line breaks being external tokens.
  Residual, documented in the code: an internal token spanning a newline
  (multi-line display math, a quoted shortcode string) could leave a
  stale anchor equal to the column of a later `@`. The only possible effect
  is reading that `@` as literal text, which pandoc does too.

  The emphasis flanking rules still use `delimiter_preceded_by_ws` and
  keep their documented approximation. Moving `*`/`_` onto the anchor
  would change existing closer behaviour, so it is a candidate
  follow-up, not part of this change.

### Tier 2: `@` inside or at the end of a word is part of the word (grammar regex)

In the continuation part of `PANDOC_REGEX_STR`, allow `@` **only
immediately after an alphanumeric**, by adding a `[alnum]@` alternative
next to the existing `['’][\p{L}\p{N}]` one. The longest match then
lexes `user@example.com`, `mermaid@11`, `a@` and `a@.` as word text.
`merge_strs` joins any split pieces, so the result is pandoc's single
`Str`.

- This is how `word@` (A1) gets accepted. The same regex also accepts
  `word@word` (table B), because a `[alnum]@` pair *followed by* more
  word characters is exactly `word@word`. There is no regex form that
  takes `word@` but leaves `word@word` a `Cite`: longest match would
  lex `foo@` and then `bar` anyway. So `word@` and table B come
  together. **Approved**: it is an improvement, since `word@word` is a
  silent bogus `Cite` today.
- `a@.` and `a@,` are accepted as a consequence. The regex can't require
  whitespace after the `@`. **Approved** as plain text.
- It doesn't affect `-@foo` or `(@foo)`: `-`, `(` and whitespace are not
  alphanumeric, and the scanner claims `-` first anyway.
- Needs `tree-sitter generate` (a `parser.c` churn). Audit
  `treesitter_utils/citation.rs` for code that stitches a preceding
  `Str` onto a `Cite`.

### Dropped from the first draft

- **Tier 1b** (`@{` opens only if `}` appears before whitespace):
  dropped. Malformed braced keys keep their error.
- **"Any `@` not followed by a key character is literal":** replaced by
  the tier-1 rule above, per the typo policy.

### Follow-ups (filed 2026-09-25)

- **bd-2o8rq2xj**: give the kept `@` errors (table A2) a Q-code with a
  hint to write `\@` for a literal at sign. Includes error-corpus
  entries and a `docs/errors` page. This makes keeping the errors cheap
  for authors.
- **bd-0idqzj33**: the other uncoded parse-error sources (code spans
  containing backticks, `$ ` at line start, braces in prose, indented
  lines). This work was in bd-bare-at-literal-w3ytmu8e's original scope
  and was split out to keep this change about `@`.
- **bd-5qmh5acq**: Unicode citation keys (`@é`, `@Müller2020`).

## Behaviour changes to track

- **Tier 1: success → error: none.** Tier 1 fires only where the parse
  fails today.
- **Error → success:** table A1.
- **Tier 2: success → different success:** table B.
- **Tier 2: success → error (one shape, found in implementation):**
  `word@{key}` was `Str "word"` + a braced `Cite`, a citation glued to
  a word, which pandoc doesn't produce either (`Str "word@{key}"`).
  Now `word@` is a word and the `{` gets the existing coded **Q-2-41**
  ("Curly braces are reserved for attribute syntax", with an escape
  hint). That fits the typo policy.
- **pampa tests updated (2):** `test_citation_without_leading_space`
  and `test_citation_multiple_spacing_patterns` used `Hi@cite` /
  `A@cite1` as a convenient no-leading-space citation. See work
  items.

## Other touch points

- `crates/pampa/src/writers/qmd.rs:1789`: the writer escapes every `@`
  as `\@`, which stays correct and safe. The comment ("or an outright
  parse error (any other position)") needs updating. Keep escaping.
- There are no error-corpus entries, tiling `EXPECTED_UNPARSEABLE`
  entries, or syntax-helper rules for `@`, so no fixture churn is
  expected, unlike #731.
- `scanner.c` and the generated parser go into the WASM build too, so
  the hub half of verify must run.

## Work items

- [x] Branch: `braid/bd-bare-at-literal-w3ytmu8e-bare-not-citation-uncoded`
      off `main` at `ce01489c4`, in place in the main checkout
      (`cargo xtask switch-task`; strand marked in_progress).
- [x] Tests first. New corpus file `test/corpus/at-as-str.txt` (84
      cases). Expected trees are *derived*, not hand-written: tier-1
      targets come from the same input with the `@` under test replaced
      by `<` (a bare `<` already takes the `LITERAL_STR` path), tier-2
      targets from `@`→`X`, and guards from the current parser. All
      were generated against the baseline scanner. Kept errors use
      `:error`. At baseline, the 41 tier-1/2 cases failed and the guards
      and errors passed.
- [x] Tier 1 in the scanner (`parse_cite_author_in_text`), with a
      comment block stating the typo policy. `-@` path unchanged.
- [x] Line-content-start anchor (see "Line-content start" under tier 1).
      Corpus: every tier-1 case passes, all leak probes still error,
      and the rest of the corpus is unchanged (tier 2's 6 cases
      pending).
- [x] Guard found a pre-existing bug: `a - @baz` → SuppressAuthor
      (pandoc: `Str "-"`, AuthorInText). Filed **bd-yl0a8kdi**, and the
      guard was split so it doesn't pin the buggy tree. Side effect:
      `a - @ b` stays an error until that strand is fixed.
- [x] Tier 2: `wordAtRegex` (`[alnum]@`) added to `PANDOC_REGEX_STR`
      as both a start alternative and a continuation item. Regenerated:
      `parser.c` grows 1.7% (lexer DFA). Throughput on a 1.3 MB concat
      of claude-notes is about 5.25k vs 5.30k bytes/ms at baseline,
      within run-to-run noise. Full corpus green, 0 failures.
- [x] `citation.rs` audit: it only converts `citation` nodes. No pass
      splits `@` out of a `Str` or joins a word onto a `Cite`. Writer:
      `[Str "foo", Cite AuthorInText bar]` with no space between (only
      reachable from JSON or a filter) is written as `foo@bar` and reads
      back as one Str. **Pandoc's markdown writer does exactly the
      same** (checked with pandoc 3.11), so this is documented, not
      worked around.
- [x] Probe tables rerun end to end with `pampa -t native` against
      pandoc: every A1 and table-B row now equals pandoc, and every A2
      row still errors. The remaining differences are pre-existing and
      unrelated (`[@ref]` / `-@ref` Cite content, pandoc's `(@foo)` /
      `@foo.` example-list quirk, a column-mismatched pipe table, and
      `10 @ $5 each`, which errors on the `$` with or without the `@`).
- [x] Rust suites (`pampa`, `qmd-syntax-helper`, `quarto-lsp-core`):
      5106/5108 on first run. The 2 failures pinned the old table-B
      behaviour: `test_treesitter_refactoring::test_citation_without_leading_space`
      (`Hi@cite`) and `test_citation_multiple_spacing_patterns`
      (`A@cite1 … @cite2C@cite3`). They test Space injection around a
      citation with no leading space, and `word@cite` was only their
      convenient input. They now use `Hi(@cite)` and
      `A(@cite1) B @cite2(@cite3) D` (both match pandoc), keeping
      their intent.
- [x] pampa integration test `tests/integration/test_bare_at_str.rs`
      (named after its sibling `test_bare_lt_str.rs`, registered in
      `main.rs`). 14 tests covering the A1 shapes (including line-content
      start after `>`, `-`, `1.`, `#` and indented continuation lines),
      table B, citation guards, the kept A2 errors (asserted as failing;
      bd-2o8rq2xj should tighten them to its Q-code), leak probes,
      `word@{key}` → Q-2-41, source ranges (the `@` Str covers only the
      `@`; the folded whitespace becomes the Space), and writer round
      trips (`a \@ b`, `user\@example.com`).
- [x] **Found and fixed in passing: ATX headings starting with a
      literal-Str token got a leading Space.** `# @ b`, and
      *pre-existing* `# < b` (bd-j9cf) and `# * b` (#731), gave
      `Header [Space, Str "@", …]`: the literal token folds the space
      after `#` into its range, and the `pandoc_str` handler splits it
      back out. `process_atx_heading` already stripped *trailing*
      Spaces for pandoc parity; it now strips leading ones too, and all
      cases match pandoc. Pinned in
      `heading_starting_with_a_literal_str_has_no_leading_space`.
- [x] Round-trip fixtures `roundtrip_tests/qmd-json-qmd/bare_at_spaced.qmd`,
      `bare_at_eol.qmd`, `bare_at_word.qmd`, `bare_at_line_start.qmd`
      (named after the `bare_lt_*` ones). `test_qmd_roundtrip_consistency`
      passes.
- [x] Writer comment updated (`qmd.rs`, the `'@'` escape arm). It still
      always escapes.
- [x] Rescanned `claude-notes/` (1444 files, including this plan):
      **718 → 770 clean.** 51 of the old files became clean, 0 became
      failing, and no file's first error sits on an `@` anymore. This
      plan itself parses clean, which exercises most shapes in the
      tables above (one `` `@`'s `` was rephrased; that was a Q-2-7
      apostrophe issue, not `@`). Top remaining first errors: Q-2-7
      (409), uncoded (77), Q-2-10 (56), Q-2-12 (37).
- [x] Full `cargo xtask verify` (run under the pinned Node 24 via
      `fnm exec --using=24`, bare, log read afterwards): **all 14 steps
      passed** on 2026-09-25. That covers clippy, fmt, the warnings-denied
      build, the tree-sitter corpus plus the CRLF parity rerun, 15010 Rust
      tests (201 skipped), the hub-client build incl. WASM with 1243 + 147
      + 153 tests, trace-viewer, shared preview packages, hub MCP
      packages, and the q2-preview-spa build. After verify, a comment
      reflow in `scanner.c` and one extra assertion (`>@ b`); the corpus
      and `test_bare_at_str` were rerun green.
- [x] Follow-up strands filed (bd-2o8rq2xj, bd-0idqzj33, bd-5qmh5acq).
- [x] Curly quotes after a standalone `@` (`“ ” ‘ ’`, U+201C/D and
      U+2018/9), same as `"` and `'`: `is_quote_mark()` in the scanner.
      Three corpus cases were confirmed red without the change and are
      green with it (808/808 with an isolated `TREE_SITTER_LIBDIR`),
      plus three pampa assertions matching pandoc (`Str "@“quoted”"`).
      The other quote marks in `PANDOC_SMART_QUOTES` (`„ ‚ « » ‹ ›`) were
      deliberately not included.
- [x] PR #732 overlap checked (non-ASCII punctuation as Str). A trial
      merge conflicts only in the generated `parser.c` / `grammar.json`;
      `grammar.js` auto-merges into a regex carrying both changes.
      Regenerated, the merge passes 816/816 corpus and 5127 Rust tests.
      Resolution for whichever PR lands second: take the merged
      `grammar.js`, then `tree-sitter generate` and `tree-sitter test`
      with an isolated `TREE_SITTER_LIBDIR` (bd-agsgrbfn: the default
      grammar cache is shared across checkouts).
- [x] #732 landed on main first. Merged main into this branch
      (`6ef88b664`): only the generated files conflicted and were
      regenerated from the auto-merged `grammar.js`. tree-sitter test
      819/819 (isolated lib dir). Full `cargo xtask verify` green: 15015
      Rust tests plus CRLF parity and all hub/TS suites. claude-notes:
      771/1445 clean with both changes.
- [x] Comment on the strand with the results.

## Decisions (2026-09-25)

1. **Tier 2 is in.** `word@`, `word@word`/emails → `Str` (table B), and
   `a@.` / `a@,` → plain text are all accepted. (The belief that
   `a@b` already worked came from a `-f qmd -t qmd` round trip; the
   JSON/native view shows the bogus `Cite`.)
2. **Kept `@` errors get a Q-code in a follow-up** (bd-2o8rq2xj), not
   here.
3. **Container-prefix limitation is acceptable** for this fix if the
   `> @ b` / list-continuation tests can't be made to pass cheaply.
4. **Curly quotes (2026-09-25, after the #732 review):** `a @“x”`
   is literal like `a @"x"`. Rationale: easy to explain, and it avoids
   surprises now that curly quotes are ordinary text.
5. **Strand scope narrowed to `@`.** The other uncoded sources are
   bd-0idqzj33, and the strand description was updated.
