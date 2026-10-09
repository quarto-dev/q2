---
title: '`html_element` runaway: `<6.1` in prose swallows the document'
date: 2026-09-28
date-modified: 2026-09-28
description: 'Tightens the inline HTML scanner so that a `<` followed by a non-letter, as in `<6.1` in prose, no longer opens an HTML element that can run across blank lines and swallow later tables.'
status: done  # implemented on `braid/bd-html-element-runaway-k1eo50h8-htmlelement-lexing-runs-away`; full `cargo xtask verify` green locally; PR open, awaiting CI + review
braid:
  strand: bd-html-element-runaway-k1eo50h8
  priority: P2
  labels: [diagnostics, markdown, parser]
---

**Owner:** cscheid
**Precedents:** bd-j9cf (bare `<` as Str, `2026-05-18-bare-lt-as-str.md`),
bd-ly83qewg (`< text` not html, `2026-08-07-angle-bracket-inner-whitespace.md`),
bd-star-as-str-qigl02pz (flanking rules, `2026-09-25-star-as-str.md`)

## Symptom

A paragraph containing `typescript-eslint (supports <6.1)`, followed by a
blank line and a pipe table whose cell contains `` `--packagePath <vsix>` ``,
fails to render:

```
Error: [Q-2-2] Mismatched Delimiter in Attribute Specifier   snippet.qmd:11:438
Error: Parse error                                           snippet.qmd:11:581
```

Both locations are inside the table row, hundreds of columns from the
real trigger on line 6. Nothing points at `<6.1`. The Q-2-2 is a
red herring: it is the `{"dependencies": false}` inside a code span,
which only became visible as an attribute specifier because the code
span's opening backtick had already been eaten.

Reduced repro (`tree-sitter parse` from the grammar directory):

```
so eslint (supports <6.1)
and tooling.

| a | b |
|---|---|
| run `x <vsix>` and `{"k": false}` here | c |
```

```
(pandoc_paragraph [0, 0] - [6, 0]
  ...
  (pandoc_str [0, 11] - [0, 19])          ; "(supports"
  (html_element [0, 19] - [5, 15])        ; "<6.1)\nand tooling.\n\n| a | b |\n|---|---|\n| run `x <vsix>"
  (pandoc_code_span [5, 15] - [5, 34]     ; "` and `" — the wrong pair of backticks
    (attribute_specifier [5, 22] - [5, 34]
      (ERROR ...)))
  (ERROR [5, 34] - [5, 41]) ...
```

One `html_element` token spans five lines, crosses a blank line, and
consumes the pipe-table header and delimiter row. Everything after it
is lexed in the wrong state.

## Root cause

`parse_open_angle_brace` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` (called on
every inline `<`) does two things too liberally:

1. **It accepts any non-whitespace character after `<`.** bd-ly83qewg
   added `html_possible`, which only rules out whitespace/EOF
   immediately after `<`. A digit, `-`, `=`, `(`, `$` … all still start
   a candidate tag. HTML and CommonMark require a tag name to begin
   with an ASCII letter (`[A-Za-z][A-Za-z0-9-]*`); the other
   angle-bracket constructs begin with `/`, `!`, `?`. Pandoc's markdown
   reader applies the same rule: `<6.1` is `Str "<6.1"`.

2. **The scan has no upper bound.** The loop advances until it finds
   `>`, `}` (raw specifier) or EOF, crossing newlines *and blank lines*.
   Because `html_element` is an inline token, a blank line (paragraph
   boundary) is never a legal place for it to still be open. When there
   is no `>` at all, the bd-j9cf EOF fallback emits a literal `<`, but
   any `>` anywhere later in the file — in a table, a code span, a
   blockquote marker, a later paragraph — closes the "tag".

Together: `<6.1)` opens a candidate tag, and the first `>` in the file
is the one in `<vsix>`, six lines away.

bd-j9cf's plan explicitly deferred (1) as the "aggressive" option
("Default to conservative; revisit if user feedback wants the broader
behavior"). bd-ly83qewg's review decided the scan must keep crossing
*newlines* (`<div\n  class="foo">` is a valid open tag) but did not
consider blank lines. This strand is the revisit.

## Current behaviour, measured

Probes on main at `82c33ff8e`, `tree-sitter parse` in the grammar dir
vs `pandoc 3.11 -f markdown -t native`.

### Cases the change fixes (wrong today, literal in pandoc)

| input | TS today | pandoc |
|---|---|---|
| `supports <6.1 and x > y` | `html_element` `<6.1 and x >` | `Str "<6.1" … Str ">"` |
| `x <- 5 and y -> 6` | `html_element` `<- 5 and y ->` | `Str "<-" … Str "->"` |
| `a <=b and c >= d` | `html_element` `<=b and c >=` | `Str "<=b" … Str ">="` |
| user repro (above) | 5-line `html_element`, 2 errors | Para + Table |

The `<-` / `->` case is R prose ("assign with `<-`") and the `<=` case
is any comparison; both are common in this project's own notes.

### Cases that change shape but were never valid HTML

| input | TS today | pandoc `markdown` | pandoc `commonmark` | proposed |
|---|---|---|---|---|
| `x <foo\n\nbar> y` | one `html_element` across the blank line | `RawInline "<foo\n\nbar>"` | `Str "<foo"` / new Para `Str "bar>"` | as commonmark |

This is the one place the plan diverges from pandoc's `markdown`
reader. Pandoc's inline-HTML parser reads a tag off the raw character
stream and will happily span a blank line; CommonMark's will not, because
a paragraph ends at the blank line before inline parsing starts. Naked
HTML is already an unsupported authoring form in qmd (Q-2-9 warns and
points at `{=html}` / `::: {.class}`), so siding with CommonMark costs
nothing we support, and it is what makes the failure mode bounded.

### Regression pins (must not change)

| input | today | why it stays |
|---|---|---|
| `<b>`, `</b>`, `<div >`, `<not a tag>` | `html_element` | letter / `/` after `<` |
| `<span\n class="x">` | `html_element` | newline inside a tag is fine (bd-ly83qewg decision) |
| `<#sec-intro>` | `html_element` → anchor Link (bd-p2tx) | `#` must stay in the gate |
| `<1user@example.com>` | `autolink` | email autolinks may start with a digit; the gate is on `HTML_ELEMENT` only |
| `<https://example.com>` | `autolink` | unchanged |
| `<!-- c -->` | `comment` | dispatched before the gate |
| `1 < 2`, `a <5 b`, `foo <`, `a <foo` (no `>`) | Strs (bd-j9cf) | unchanged |
| `*a < b text.* a > b` | emphasis closes (bd-ly83qewg) | unchanged |

## Proposed rules

Both live in `parse_open_angle_brace`. They are independent; (A) fixes
the reported document, (B) bounds the damage when (A) still lets a
candidate through.

### (A) Tag-start gate

After consuming `<`, compute

```
tag_possible = isalpha(c) || c == '/' || c == '?' || c == '#'
```

where `c` is the lookahead. `!` is already dispatched to the comment
parser before this point. `#` is not HTML, but qmd's `<#id>` anchor
shorthand rides on the `html_element` token (pampa's
`parse_anchor_shorthand`), so it must stay.

- The `HTML_ELEMENT` arm of the scan loop requires `tag_possible`
  (today it requires `html_possible`, i.e. not-whitespace).
- The existing fast path becomes: if `!tag_possible` and the character
  cannot start an autolink either (whitespace/EOF, or `/`) and
  `RAW_SPECIFIER` is not valid, emit `LITERAL_STR` immediately.
- Inside the loop, once `could_be_autolink` turns false (first
  whitespace) and `tag_possible` is false and `RAW_SPECIFIER` is not
  valid, stop scanning and fall through to `LITERAL_STR`. This is what
  keeps `<6.1 and … >` from walking to the `>`: the walk ends at the
  space after `<6.1`.

Autolinks are deliberately left on their own predicate: CommonMark
email autolinks may begin with a digit (`<1user@example.com>` is a
pinned regression), and URI autolinks are already validated in pampa.

Effect: `<6.1`, `<-`, `<=`, `<(`, `<$`, `<5>`, `<,>` all become a
literal `<` followed by ordinary Strs, matching pandoc.

### (B) Paragraph-bounded scan

In the scan loop, on `\n` (or `\r\n`): peek past spaces/tabs; if the
next character is another line ending or EOF, the line is blank. Stop
scanning. `mark_end` is still at `<`+1 (bd-j9cf), so the fallback emits
`LITERAL_STR` for the `<` alone and the parser proceeds; the paragraph
ends at the blank line as it should.

*Dropped during implementation:* clearing `could_be_autolink` on a
line ending. It would flip `<https://x\ny>` from `autolink` to
`html_element`, which is a pre-existing oddity either way and not this
strand's subject; the blank-line bound already covers the runaway
case for autolinks. Left as-is so the diff stays about the two rules.

Also added in the loop: once a `>` closes nothing (no autolink match,
tag ruled out at the first character), stop scanning; a later `>`
cannot help either. Same for the moment the autolink reading dies at
the first whitespace when no tag was possible. Both keep `<6.1 …` from
walking the rest of the paragraph.

The `RAW_SPECIFIER` arm (`}` terminator) gets the same bound. A raw
specifier is a single-line attribute; the prior plan kept that path
"byte-for-byte unchanged" only to limit blast radius, not because
spanning a blank line was wanted.

Effect: an unclosed `<foo` never swallows more than the rest of its
paragraph. `x <foo\n\nbar> y` becomes two paragraphs of Strs
(CommonMark's reading). Whatever mis-lex remains is now local, so any
error the parser does raise lands in the paragraph that caused it,
which is the "more helpful error" the report asks for.

### What is *not* proposed: a new diagnostic

Two shapes were considered and are not recommended for this strand.

- **An error token when the scan hits a blank line** ("HTML tag not
  closed before end of paragraph"). Rejected because after (A) the only
  inputs reaching (B) are letter-led (`<foo`, `<div`), and bd-j9cf
  already made the same input at EOF a silent literal `<`. An error
  here would be inconsistent with that, and would fire on prose like
  `if x <y` at the end of a paragraph. Literal text is the safe
  reading: it only ever turns errors into renders.
- **A warning from pampa** when a paragraph begins with `Str "<"` +
  tag-like Str. Possible later as a lint (bd-j9cf's open question 2
  declined a diagnostic for the same reason). File separately if
  wanted.

Q-2-9 continues to fire for every real `html_element`, and with (B)
its span is at most one paragraph, so the warning's location becomes
more useful too.

### Alternatives considered

- **Stop the scan at any newline.** Rejected by bd-ly83qewg's review:
  `<div\n  class="foo">` is valid. The blank line is the right bound.
- **Only (A), no (B).** Fixes the report but leaves `<foo` + later `>`
  able to eat the file. Cheap to do both; (B) is ~10 lines.
- **Full CommonMark open-tag validation** (attribute grammar, quoted
  values, `>` inside quotes). Out of scope; the token is documented as
  best-effort and pampa does not parse it further.

## Implementation steps

1. **Tests first** (written to fail on main):
   - `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/lt-as-str.txt`
     (or a sibling `html-element-bounds.txt`): the three "fixes" rows,
     `x <foo\n\nbar> y`, the reduced user repro (paragraph + blank +
     pipe table with `<vsix>` in a code span → `pandoc_paragraph` then
     `pipe_table`), and the regression-pin rows not already present
     (`</b>`, `<span\n class="x">`, `<#sec-intro>`, `<1user@example.com>`).
   - `crates/pampa/tests/integration/test_bare_lt_str.rs`: pampa-level
     assertions for `<6.1`, `<-`, `<=` (Str sequences, no warnings), the
     blank-line case (two Paragraphs), and the full user snippet
     (a Paragraph, a Table, zero diagnostics). Guard that `<b>` still
     yields `RawInline` + Q-2-9 and `<#x>` still yields the anchor Link.
2. **Scanner change** in `parse_open_angle_brace`: (A) then (B) as above.
   Scanner-only; no `grammar.js` change, so no `tree-sitter generate`.
   Add an `is_ascii_alpha` helper if none exists (grep found none).
   Update the bd-ly83qewg comment block and the `LITERAL_STR` comment in
   `grammar.js` (line ~1250) to name this strand.
3. **Corpus run**: `tree-sitter test` in the grammar dir; expect the
   existing `lt-as-str` and `comment` cases green.
4. **Workspace verify**: `cargo xtask verify --skip-hub-build`. Audit
   any `.snap` changes: each must be an `html_element` → Strs flip on
   input that was never a tag (`<` + non-letter), or a blank-line span.
   Anything else is a regression.
5. **Corpus measurement** (optional, same script as
   `2026-09-25-star-as-str.md`): run `pampa --json-errors
   --no-prune-errors` over `claude-notes/**/*.md` before and after;
   report the count of files whose first error moved or vanished.
6. **Docs**: one paragraph in `dev-docs/syntax-notes.md` "No naked HTML
   support" stating the two rules (tag must start with a letter, `/`,
   `?`, `#`; an inline tag cannot span a blank line) and the
   CommonMark-vs-pandoc divergence. Add the strand to CONTRIBUTING's
   known-limitations list only if the divergence is judged worth
   listing.
7. Commit on `braid/bd-html-element-runaway-k1eo50h8-…`, push as
   `feature/…`, PR, close the strand with the commit id.

## Corpus measurement (step 5, done 2026-09-28)

`pampa --json-errors --no-prune-errors -t native` over the 1459 `.md`
files under `claude-notes/`, first diagnostic code per file, main
(`82c33ff8e`) vs this branch. Script: `measure.py` (scratchpad; trivial
to recreate from the description).

| | main | branch |
|---|---:|---:|
| files that parse clean | 730 | 741 |
| exit 1 with no coded diagnostic (bare "Parse error") | 51 | 42 |
| Q-2-35 (indented code block) | 11 | 8 |
| Q-2-13 (`**`) | 10 | 9 |
| Q-2-9 (naked HTML, warning) | 37 | 37 |

18 files changed first code; **0 regressed** (no file went from clean to
anything). 11 became clean; the other 7 now fail *later*, on an
unrelated construct the runaway had hidden (`'` Q-2-7, `*` Q-2-12). The
bare-"Parse error" bucket is where the reported document lived: an
`html_element` that ate a table or list leaves the parser with an
`ERROR` node and no coded diagnostic to attach — which is exactly the
unhelpful message the strand is about.

## Risks

- **Snapshot churn.** Any fixture containing `<` + non-letter + later
  `>` in one paragraph flips from `RawInline` (with Q-2-9) to Strs.
  bd-j9cf's audit in May found this small; step 4 re-audits.
- **`<!` other than a comment** (`<!DOCTYPE`, `<![CDATA[`): the `!`
  branch returns false without emitting when the comment parser
  declines, which is pre-existing behaviour and untouched here. Note
  it as a follow-up if it shows up in the audit.
- **`>` as blockquote marker on a continuation line** (`<span\n> x`)
  already terminates the candidate tag today; (B) does not change that.
- **Quarto 1 parity**: a document relying on an inline tag spanning a
  blank line renders in Quarto 1 and will not in q2. No such document
  is known; it would already carry a Q-2-9 warning.

## Out of scope

- Parsing the contents of `html_element` (attributes, quoted `>`).
- `native_divs` / `native_spans` (see `dev-docs/syntax-notes.md`).
- Reporting a diagnostic for `<foo` left open at a paragraph end
  (see "What is not proposed").
- The qmd writer's handling of `RawInline html` (unchanged).

## Open questions for review

1. **Rule (B) fallback**: literal `<` (recommended, consistent with
   bd-j9cf's EOF fallback) vs a new error code. See above.
2. **`?` in the gate**: processing instructions are meaningless in qmd
   output. Keeping `?` is the spec-faithful choice and costs nothing;
   dropping it would make `<?php` a Str. Recommend keep.
3. **Corpus measurement (step 5)**: worth the run, or skip for a
   scanner change this small? Recommend run; it is cheap and the
   `<-` / `<=` cases suggest the note corpus will move.

## Work items

- [x] Phase 1: failing corpus + pampa tests — 2026-09-28. Corpus:
  `test/corpus/html-element-bounds.txt` (7 behaviour cases, 4 regression
  pins; 7 failed on main as intended). Pampa: 11 tests appended to
  `tests/integration/test_bare_lt_str.rs`, including the report verbatim
  (OrderedList + Table, zero diagnostics).
- [x] Phase 2: scanner change (A) + (B), comments updated — 2026-09-28.
  `is_ascii_alpha` helper added next to `is_punctuation`; `grammar.js`
  `_pandoc_literal_str` comment names the strand. The only corpus
  expectation I had to correct after the fact was token granularity
  (`tooling.` is one Str; the literal `<` carries its leading space, so
  there is no `pandoc_space` node before it) — both pre-existing
  conventions, not behaviour changes.
- [ ] Phase 3: `tree-sitter test` (830/830 locally), `cargo xtask verify`,
  snapshot audit
  - [x] `tree-sitter test`: 830/830.
  - [x] Rust stages of `cargo xtask verify --skip-hub-build`: green after
    one `cargo fmt` on the new tests. **No `.snap` changed**, so the
    snapshot audit is empty: no fixture in the workspace had `<` +
    non-letter + `>` in one paragraph.
  - [x] hub-client WASM tests: the `--skip-hub-build` run failed two
    tests (`projectCreate.wasm` expects a `book` template;
    `smokeAll.wasm` crossref captions). The on-disk WASM was built
    2026-09-26 14:56, *before* main's e8379cfe1 (18:40) that added the
    book expectation. Full `cargo xtask verify` (with hub build) passed
    every step, 153/153 WASM tests included — stale artifact confirmed.
- [x] Phase 4: corpus measurement (table above; spot checks of the moved
  files found `<100ms`, `<10ms`, `<2GB`, Rust lifetimes `<'proj>` — all
  `<` + non-letter), syntax-notes paragraph added under "No naked HTML
  support".
- [ ] Phase 5: PR opened, CI green, close bd-html-element-runaway-k1eo50h8
