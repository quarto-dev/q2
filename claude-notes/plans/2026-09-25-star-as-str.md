# Flanking rules for `*` `_` `~` `^`: whitespace-adjacent delimiters are literal text

**Status:** tiers 1 and 2 implemented on `braid/bd-star-as-str-qigl02pz-tree-sitter-qmd-parse`, not pushed
**Tracking issues:** bd-star-as-str-qigl02pz (this plan), bd-whitespace-flanked-delimiters-0ncy8bgq (closed by tier 2)
**Owner:** cscheid
**Last updated:** 2026-09-25
**Precedents:** bd-j9cf (bare `<` as Str, `claude-notes/plans/2026-05-18-bare-lt-as-str.md`),
bd-6kewx (Unicode Po/Pc punctuation as Str), bd-ly83qewg (`< text` not html)

## Overview

A lone `*` in prose was a hard parse error (`O(N * D) cost` failed with
Q-2-12), and worse, an *even* number of whitespace-flanked delimiters
paired up silently: `a * b * c` rendered as `a <em>b</em> c`,
`~5 and ~10` as a subscript, `_quarto.yml and _metadata.yml` as an
emphasis. Pandoc (its `markdown` and `commonmark` readers) treats every
one of these as literal text.

The cause was in the four delimiter handlers of
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
(`parse_star`, `parse_thematic_break_underscore`, `parse_tilde`,
`parse_caret`): once a block-level reading (thematic break, list
marker) was ruled out they emitted an emphasis/sub/sup opener or closer
whenever the parser state allowed one, never looking at the characters
around the run.

The fix, in three tiers (1 and 2 shipped together, 3 is a follow-up):

1. **Opener side.** A `*` / `_` run followed by whitespace, EOL or EOF
   is not left-flanking (CommonMark §6.2) and cannot open.
2. **Closer side, and `~` `^`.** A `*` / `_` run preceded by whitespace
   or at line start is not right-flanking and cannot close. A `~` / `^`
   only opens a sub/superscript when its closer appears before the next
   whitespace (Pandoc's markdown rule, what Quarto 1 does). A delimiter
   that can do neither is emitted as the literal-Str token.
3. **Unmatched opener → literal** (not done): `*.md`, `*args`,
   `a *b c`, `_quarto.yml`, `(* text *)` are left-flanking openers with
   no closer. Today they error (Q-2-12 / Q-2-5); making them literal
   needs a block-bounded lookahead for a plausible closer. Only ever
   converts errors into successes. File as its own strand.

The context is bd-uk8zgkha (render `claude-notes/` as a q2 website).
Effect on that corpus, measured with `pampa` over the 1441 `.md` files
(scan script: run `pampa --json-errors --no-prune-errors` per file,
count first-error codes):

| | before | after |
|---|---:|---:|
| files that parse clean | 491 | 718 |
| files whose first error is Q-2-17 (`~`) | 348 | 0 |
| … Q-2-16 (`^`) | 6 | 0 |
| … Q-2-12 (`*`) | 49 | 34 |
| … Q-2-13 (`**`) | 26 | 7 |
| … Q-2-5 (`_`) | 17 | 28 |

Q-2-5 rises because parsing now reaches the underscore filenames that
used to be hidden behind earlier errors, and because `_quarto.yml and
_metadata.yml` is now an error instead of a silent `<em>`. Tier 3 is
what turns those into literal text.

## Current behaviour, measured

Probes run on main at `da468214c` with `tree-sitter parse` (grammar
dir) and `pandoc 3.11 -t native`. "TS" is our parser; "pandoc" is
`-f markdown` unless noted. Full probe files are reproducible from the
inputs shown.

### Cases the change fixes (error today, literal in pandoc)

| input | TS today | pandoc |
|---|---|---|
| `a * b` | ERROR | `Str "a", Space, Str "*", Space, Str "b"` |
| `foo *` (EOL / EOF) | ERROR | `Str "foo", Space, Str "*"` |
| `first *\nsecond` | ERROR | `... Str "*", SoftBreak, Str "second"` |
| `a\t*\tb` | ERROR | as `a * b` |
| `a ** b` | ERROR (Q-2-13) | `Str "**"` |
| `foo **` | ERROR | `Str "**"` |
| `a *** b` | ERROR | `Str "***"` |
| `O(N * D) cost` | ERROR (Q-2-12) | `Str "O(N", Space, Str "*", Space, Str "D)"` |
| `Q-2-* codes` (star after punct, before space) | ERROR | literal |
| `Ts* types` (star after word, before space) | ERROR | `Str "Ts*"` |
| `value*` (star after word, at EOF) | ERROR | `Str "value*"` |
| `*x* * y` | ERROR | `Emph[x], Space, Str "*", Space, Str "y"` |
| `**a * b**` | ERROR | `Strong[a * b]` |
| `*a ** b*` | ERROR | commonmark: `Emph[a ** b]`; pandoc-md: literal |
| `- a *\n- b` | ERROR | list, `Str "*"` in first item |
| `> a * b` | ERROR | blockquote, literal |
| `:::{.note}\na * b\n:::` | ERROR | div, literal |
| `[a * b]{.x}`, `[a * b](u)` | ERROR | span / link, literal |
| `# a * b`, setext `a * b\n===` | ERROR | header, literal |
| `\| x * y \|` (pipe-table cell) | ERROR | table, literal |
| `para\n*` (star alone on last line, EOF) | ERROR | `Str "para", SoftBreak, Str "*"` |

### Cases unchanged (block-level uses keep precedence)

| input | TS today | pandoc | note |
|---|---|---|---|
| `* a` | list | list | `LIST_MARKER_STAR` checked before the new rule |
| `text\n* more` | list (interrupts paragraph) | pandoc-md: paragraph; commonmark: list | existing q2 dialect choice (bd-1xph), untouched |
| `a\n\n* * *\n\nb` | hrule | hrule | `THEMATIC_BREAK` checked before the new rule |
| `a\n***\nb` | hrule | pandoc-md: paragraph; commonmark: hrule | existing q2 choice, untouched |
| `* a\n*\n` | list with empty item | same | `LIST_MARKER_STAR_DONT_INTERRUPT` wins |
| `a \* b` | literal | literal | backslash escape already works |
| `` `a * b` `` | code span | code span | scanner never reaches `parse_star` |
| `**bold **` | Strong[bold ] | pandoc-md: Strong[bold, Space]; commonmark: literal | closer side, out of scope |
| `*a *b* c*` | Emph[a] b Emph[c] | pandoc-md: Emph[a ] b* c*; commonmark: nested | closer side, out of scope |

### Cases still erroring after the change (need lookahead; follow-up)

These are stars that *are* left-flanking (followed by a non-space) and
simply never get closed. Making them literal requires knowing there is
no closer ahead, which is a different mechanism (see Tier 3).

| input | TS today and after | pandoc |
|---|---|---|
| `a *b c` | ERROR | `Str "*b"` |
| `files *.md here`, `**/*.md` | ERROR | literal |
| `char *p = q`, `f(*args, **kwargs)` | ERROR | literal |
| `a *, b` | ERROR | `Str "*,"` |
| `O(N*D)` | ERROR | `Str "O(N*D)"` |

### One currently-successful parse that changes

`(* comment *)` parses today as `Str "(", Emph[comment], Str ")"`. Both
pandoc readers say it is literal (`(*` is followed by a space, so it
cannot open). After the change the first star becomes `Str "*"`, and
the second (`*)`, followed by punctuation) opens an emphasis with no
closer, so the document becomes a Q-2-12 error instead of a silently
wrong parse. The corpus has no test of this shape; it is the only
success-to-error transition found in the probes. Tier 3 would rescue it.
I recommend accepting this and documenting it in the PR.

### Real-world measurement (claude-notes on main)

Script: run `pampa --json-errors --no-prune-errors` over the 1441 `.md`
files under `claude-notes/`, collect every Q-2-12 diagnostic, and
classify the opener star by its neighbours.

- 491 files clean, 896 fail without reaching a star (mostly Q-2-17 `~`
  and Q-2-7 `'`, which the other agent is escaping), 54 files show a
  Q-2-12.
- Of the 41 Q-2-12 openers with a resolvable position: 21 are
  whitespace-followed (fixed by this plan: `jupyter.* na`,
  `Q-1-* entries`, `O(N * D)`, `Ts* types`, `Document* type`), 11 are
  punctuation-followed globs (`*.rs`, `*.snap`, `et al.*,`; Tier 3),
  8 are word-followed (`*real*` split across lines, `*️⃣` keycap;
  separate issues), 1 other.
- 13 diagnostics had no opener position (pipe-table cells, per the
  bd-uk8zgkha notes).

This is a lower bound: the 896 files that fail earlier hide their
stars. The braid notes for bd-uk8zgkha count Q-2-12 at 53 → 80 files
as the other classes were escaped.

## Design

### Where the information comes from

- **Followed by whitespace** is a one-character lookahead after the
  contiguous run (space, tab, `\n`, `\r`, EOF).
- **Preceded by whitespace** is knowable because tree-sitter tries the
  external scanner before the internal `_whitespace` regex at every
  token boundary: `scan()` consumes the whitespace in front of the run
  before dispatching (this is why the opener token in `a *b* c` spans
  columns 1–3, space included). `scan()` now records that count in a
  per-call field `ws_before_token` (deliberately not serialized; reset
  at the top of every call; `s->indentation` is stale mid-line and must
  not be used). Line start is `lexer->get_column() == 0`, called at most
  once per delimiter run. The information is never lost: every parser
  state that admits a closer also admits an opener and `pandoc_str`, so
  the first scan call at a position always emits something.
- **`~` / `^`** need no lookbehind at all: `closer_before_ws()` advances
  (without `mark_end`) to the next whitespace looking for the matching
  delimiter, skipping backslash escapes.

### Token

The literal token is the one bd-j9cf added for bare `<`, renamed from
`_pandoc_lt_str` / `LT_STR_LITERAL` to `_pandoc_literal_str` /
`LITERAL_STR` ("this is text after all"). It is a `pandoc_str`
alternative, so the AST shape is uniform and `treesitter.rs` already
splits the chomped leading whitespace back out into a `Space`. The qmd
writer already escapes `*` `_` `~` `^` in a `Str`, so round-trip works
with no writer change.

### `mark_end` discipline

The star and underscore loops consume whitespace *between* runs (a
thematic break is `* * *`), so the lexer cannot rewind to the end of
the contiguous run afterwards. Each handler now calls `mark_end` after
every character of the contiguous run and never again on the inline
paths; the thematic-break and list-marker paths set their own end as
before. Side effect: a `__` closer no longer swallows the space after it
into its token (that is the one existing corpus expectation that
changed, see below). The old "very ugly hack" that emitted
`EMPHASIS_CLOSE_STAR` before looking at anything is gone; closers are
still tried before openers, but only when flanking allows each.

### Precedence, unchanged

`TRIPLE_STAR` (Q-2-32), `THEMATIC_BREAK`, `LIST_MARKER_STAR[_DONT_INTERRUPT]`
are all decided before the inline rules, exactly as before, so `* a`,
`* * *`, `***foo***`, `* a\n*\n` (empty item) are untouched. One
adjustment to the paragraph-interruption peek in the `SOFT_LINE_ENDING`
gate: a `*` alone on a line is an *empty* list item, which CommonMark
lets start a sibling item inside an open list but not interrupt a
top-level paragraph, so `para\n*` is now one paragraph
(`Para [para, SoftBreak, "*"]`, as pandoc) instead of a paragraph plus
an error. Gated on `any_list_item_open()` so the empty-item corpus
tests keep passing.

### Known approximations

- A container prefix consumed by `match_line` (`> `, list continuation
  indent) is not counted as "whitespace before", so a closer that is the
  very first character of a quoted/indented continuation line may still
  close. That is today's behaviour, never a new error.
- `*` alone at EOF at the very start of a document is `Str "*"`; pandoc
  says empty bullet list. Both are successes.
- `*a**` (emph closer followed by a stray star) is still an error, as
  before; the run is two long and only a one-star closer is valid.

### qmd writer: spaces inside sub/superscripts

Round-trip check (2026-09-25, on request): `~a\ b~` reads as
`Subscript [Str "a\u{a0}b"]` (the escape becomes U+00A0 as in pandoc),
the writer emits the U+00A0 raw, and it re-reads as the same subscript
because a non-breaking space is not whitespace to `closer_before_ws`.
But a Subscript/Superscript holding a real `Space` or `SoftBreak`
inline (from JSON, a filter, or pandoc's commonmark_x reader) was
written as `~a b~`, which the new reader takes as literal text. The qmd
writer now tracks `sub_sup_depth` and writes those as `\ ` inside a
sub/superscript, which is what pandoc's markdown writer does; pinned by
`sub_and_superscript_containing_a_space_round_trip_through_the_qmd_writer`.
Strikeout (`~~a b~~`) is unaffected by the rule and was checked too.

## Behaviour changes to track (bd-star-as-str-qigl02pz)

Per the request to keep track of valid parses that become invalid:

**tree-sitter corpus — tests removed: none.** 718/718 pass. One
existing expectation was *updated*, not removed:

- `emph.txt: nested underscore emph underscore star emph`
  (`_hello __strong__ world_`): same parse, but the space after the
  `__` closer is now its own `pandoc_space` node instead of being inside
  the closer token. The pandoc AST is identical.

**Parses that were successes and are now errors** (none had a corpus
test; pinned as tier-3 boundary cases in `flanking-delimiters.txt`):

- `**bold **` → Q-2-13. Pandoc-markdown said `Strong[bold ]`,
  CommonMark says literal; with the closer rule the `**` after the space
  cannot close and the opener is unmatched.
- `*a and then *b` → Q-2-12 (was `<em>a and then</em>b`).
- `_quarto.yml and _metadata.yml`, `a_ and _b` → Q-2-5 (were silent
  `<em>`). Correct per all three pandoc readers once tier 3 lands.
- `(* comment *)` → Q-2-12 (was `(` `Emph[comment]` `)`; pandoc:
  literal).
- `~a and b~`, `^a and b^` → literal text (were sub/superscript). This
  is the Pandoc-markdown / Quarto 1 rule; commonmark_x would keep them.

**pampa fixtures whose meaning changed:**

- `tests/smoke/001.qmd` (`^ he llo^`) moved out of `EXPECTED_UNPARSEABLE`
  in `tiling_corpus_tests.rs`: it now parses (as literal text).
- Error corpus: the cases `a *`, `foo* a ` (Q-2-12), `**` (Q-2-13),
  `__` (Q-2-15), `_` (Q-2-5) are valid documents now and were replaced
  by word-followed openers (`a *b`, `foo *a `, `**a`, `__a`, `_a`). The
  `~` (Q-2-17) and `^` (Q-2-16) cases became `~a\`b~\`` / `^a\`b^\``:
  since a sub/superscript only opens when its closer is in sight, the
  remaining way to leave one unclosed is a closer swallowed by a code
  span. One tiling (suffix `'a'`) was dropped from the Q-2-5/12/13/15
  cases because the new content makes it report Q-2-7 first.
  `case-files/` and `_autogen-table.json` were regenerated with
  `deno run crates/pampa/scripts/build_error_table.ts`.
- `qmd-syntax-helper` Q-2-16/Q-2-17 tests: `x^2` and `H~2O` are no
  longer violations (they are literal text); the violation inputs use
  the code-span shape and a new test pins the bare opener as clean.

## Verification

- `tree-sitter test`: 718/718 (663 pre-existing + 55 new in
  `flanking-delimiters.txt`).
- `cargo nextest run -p pampa -p qmd-syntax-helper -p quarto-lsp-core`:
  green after the fixture updates above.
- `cargo xtask verify --skip-hub-build` (fmt, clippy, build, tree-sitter
  corpus incl. CRLF, 14866 Rust tests) and
  `cargo xtask verify --skip-rust-build --skip-rust-tests` (WASM build,
  hub-client 1174 tests, preview SPA): all steps passed, 2026-09-25.
  The hub half needs `npm install` in the worktree first and the WASM
  package built (`--skip-hub-build` alone leaves hub-client tests unable
  to resolve `wasm-quarto-hub-client`).
- claude-notes scan before/after: table in the overview.

## Follow-ups

- [x] Tier 3 strand filed: bd-unmatched-opener-literal-dni5aiwd
  (unmatched left-flanking opener → literal: `*.md`, `*args`,
  `a *b c`, `_quarto.yml`, `(* text *)`).
- [ ] `~~` strikeout was left alone (`a ~~ b` still opens a strikeout
  and errors; Q-2-18 unchanged). Decide whether it should follow the
  `*` flanking rule.
- [ ] `* ` (star, space, EOL) after a paragraph still interrupts as an
  empty item; only the bare `*` line was fixed.
- [ ] Container-prefix approximation above, if a real document hits it.
