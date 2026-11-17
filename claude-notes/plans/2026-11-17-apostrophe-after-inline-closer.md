# Apostrophe directly after an inline closer

**Strand:** bd-apostrophe-after-span-6l9c8c26 (related: bd-1qk5, GH #943)

**Status:** approved 2026-11-17; implemented, in verification.

## Overview

A `'` that directly follows the end of an inline construct is read today as an
opening single quote. The paragraph then fails with Q-2-7 or Q-2-10, or, worse,
silently pairs with a later `'`:

```
The _book_'s premise              Q-2-7 (unclosed single quote)
The [book](https://x.com)'s idea  Q-2-7
The `code`'s value                Q-2-7
The _books_' premises             Q-2-10
The **book**'s and "quote"'s      no error: Quoted [s and "quote"] then Str s
```

Pandoc reads every one of these as an apostrophe. The goal is to do the same,
so that authors stop needing the backslash escape in these positions.

GH #943 already solved the sibling case, a `'` after a *word*
(`Dogs' toys`), entirely inside the scanner by deciding at the start of the
word. That trick cannot work here: the scanner cannot look behind, and several
closers (the `)` of a link, the `}` of an attribute, the closing `$` of math)
are lexed by the internal lexer, so the scanner never sees them. A flag kept
in scanner state would cover only the closers that are external tokens.

## Design: let the LR state carry the context

The parse state already knows that an inline construct has just ended. We
expose that to the scanner through `valid_symbols` by giving the grammar one
new external token that is valid only in that position:

```js
_inline_element: $ => choice(
    $.pandoc_str,
    /* ...the other elements that do not take an apostrophe... */
    seq($._closable_inline,
        optional(alias($._span_apostrophe, $.pandoc_str))),
),

_closable_inline: $ => choice(
    $.pandoc_emph, $.pandoc_strong, $.pandoc_span, $.pandoc_image,
    $.pandoc_code_span, $.pandoc_math, /* ... */
),
```

In the scanner, `parse_single_quote` gains one branch ahead of the open/close
logic:

```c
if (valid_symbols[SPAN_APOSTROPHE] &&
    s->single_quote_depth == 0 &&   // inside '...' it is the closer
    s->ws_before_token == 0 &&      // `_a_ 'quoted'` is still a quote
    !in_error_recovery) {
    /* consume the apostrophe and the word characters after it */
    EMIT_TOKEN(SPAN_APOSTROPHE);
}
```

Properties of this design:

- **No scanner state is added.** Nothing new is serialized. The two fields it
  reads, `single_quote_depth` and `ws_before_token`, exist already.
- **No LR conflict.** The scanner decides deterministically, so the grammar
  keeps its no-`conflicts` invariant.
- **Every closer is covered uniformly**, whether its last token is external
  (emphasis, code span) or internal (link target, attribute, math).
- **The tree shape needs no pampa change.** The token is aliased to
  `pandoc_str` and sits as a sibling after the closed construct, which is
  the shape Pandoc produces (`Emph [Str "book"], Str "’s"`).

## Prototype results

Done on a scratch copy of `crates/tree-sitter-qmd` (not in the repository).

- `tree-sitter generate` succeeds with no conflicts.
- Parser states grow from 4773 to 4981 (about 4%); `parser.c` grows from
  7.58 MB to 7.90 MB.
- The existing corpus passes unchanged: 852 of 852.
- One bug was found and fixed along the way: the scanner skips leading
  whitespace before dispatching, so `*x* 'y'` reached the new branch. The
  `ws_before_token == 0` guard fixes it.

Parses with the prototype (all match Pandoc unless noted):

| Input | Result |
|---|---|
| `The _book_'s premise` | emph, then Str `'s` |
| `The [book](https://book.com)'s premise` | link, then Str `'s` |
| `` `code`'s ``, `**book**'s`, `"quote"'s` | apostrophe after each |
| `[span]{.x}'s`, `$x$'s`, `@cite's`, `x^2^'s` | apostrophe after each |
| a shortcode followed by `'s` | apostrophe after the shortcode |
| `The _books_' premises` | emph, then Str `'` |
| `"the _book_'s premise"` | apostrophe inside the double quote |
| `_a_ 'quoted'` | unchanged: a quoted span |
| `The '_book_'s are not good` | unchanged: a quoted emphasis, then Str `s` |
| `'the _book_'s premise'` | closes the quote at the emph (differs from Pandoc) |
| `_a_'b'` | parse error in the prototype (Pandoc: Str `’b’`); fixed in the final design |

## Decisions (Carlos, 2026-11-17)

1. **What may follow the apostrophe: anything.** This matches Pandoc and
   GH #943, and supersedes the 2026-09-23 note on the strand that kept plural
   possessives an error (GH #943 had already made `The books' premises`
   parse). `_books_' premises` is an apostrophe.

2. **A quote that abuts a closer, `_a_'b'`, reads as Pandoc reads it**
   (Str `’b’`), with the requirement that the quoted reading stays reachable
   through some syntax. An empty span cannot be that syntax, because a span is
   itself a closable inline. Two forms work and are pinned by tests:
   `_a_<!---->'b'` (an empty HTML comment, which renders as nothing in every
   format; the AST carries a `RawInline` for it) and `_a_&#8203;'b'` (a
   zero-width space as a character reference).

3. **Every inline with a closing delimiter takes the apostrophe:** span and
   link, image, inline and display math, code span, both quote kinds,
   highlight, insert, delete, edit comment, shortcode, citation, inline note,
   note reference, superscript, subscript, strikeout, emphasis, strong,
   autolink and HTML element. Left out: `pandoc_str`, HTML comments, the
   escaped shortcode, entities, line break and a bare attribute.

4. **Inside a single-quoted span** the `'` after a closer stays the quote
   closer, so `'the _book_'s premise'` still needs the escape. This is the
   limitation GH #943 has for words. Note it is not an error: it parses as
   a quoted span holding `the` and the emphasis, then `s premise’`. Pinned by a test, documented
   in the migration guide.

## Implementation notes

- The token is the `'` alone, not `'s`. What follows is lexed as usual and
  pampa already merges adjacent `pandoc_str` nodes, so `'s` is one Str and
  `_l_'été` needs nothing special. Decision 2 also falls out of this: the
  word after the apostrophe is at a word start, so the GH #943 rule picks up
  `b'` on its own.
- Error-recovery guard: `TRIGGER_ERROR` is used by no grammar rule, so it is
  a valid symbol only during recovery.

## Work items

### Phase 1: tests first

- [x] Corpus cases in `test/corpus/inline-quotes.txt` (13 new cases).
- [x] `crates/pampa/tests/integration/test_apostrophe_after_closer.rs`,
      registered in `main.rs`.
- [x] Confirmed the new tests fail on `main` (10 of 11 failed; the one that
      passed pins unchanged behavior).

### Phase 2: grammar and scanner

- [x] Split `_inline_element`; add `$._span_apostrophe` / `SPAN_APOSTROPHE`.
- [x] Branch in `parse_single_quote`.
- [x] `cargo xtask ts-test`: 865 of 865.

### Phase 3: pampa

- [x] Smart typography and Str merging work with no pampa source change.
- [x] Regenerated `_autogen-table.json` with `scripts/build_error_table.ts`.
- [x] One existing assertion in `test_apostrophe_after_word.rs` pinned the
      old error for `` `a.rs`'s file ``; removed, with a pointer to the new
      test file.

### Phase 4: docs

- [x] `docs/guides/authoring/migrating-markdown.qmd`, the Q-2-7 and Q-2-10
      error pages.
- [x] `claude-notes/instructions/writing-notes.md` (dropped the two
      apostrophe habits) and the matching sentence in `AGENTS.md`.
- [ ] Follow-up, not in this change: `qmd-syntax-helper` rules `q_2_7` and
      `apostrophe_quotes`, and the qmd writer (bd-8lcm), still add escapes
      that are no longer needed. They remain correct.

### Phase 5: verify

- [x] `cargo xtask verify` (all 14 steps pass).
- [x] Three existing notes stopped parsing under the strict claude-notes
      render and were fixed. Each one had parsed only by accident:
      - `2026-06-24-monaco-backtick-autoclose.md`: a code span holding
        backticks was written with single-backtick delimiters, so it was
        really two code spans with a quoted span between them. Now uses
        double-backtick delimiters.
      - `2026-09-21-book-projects-P0-number-sections.md`: a possessive after
        a code span had silently paired with a possessive after a `)` later
        in the paragraph. The first is now an apostrophe, which exposed the
        second (`)` is not a closable inline); it is escaped.
      - `2026-06-24-plan1b-review-findings.md`: the same accidental pairing,
        which the author had worked around by escaping the *closer* of a
        real quotation. The escape is removed.
- [ ] PR and CI.
- [ ] Re-tally Q-2-7 across `claude-notes` and record it on the strand.

## Behavior changes to call out in the changelog

- Inputs that were errors now parse (the purpose of the change).
- `**book**'s and "quote"'s`, which silently produced a quoted span, now
  produces two apostrophes.
- A single-quoted span written with no space after a closer, such as
  `_a_'b c'`, is no longer a quoted span. Pandoc agrees with the new reading.
- `The _book_'s are not good` no longer reports an error even if the author
  meant a plural inside a quote. This is the accepted trade-off.
