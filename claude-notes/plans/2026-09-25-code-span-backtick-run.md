# Code span containing a backtick run longer than its delimiter is a parse error (bd-code-span-longer-backtick-run-nycn85a8)

**Date:** 2026-09-25
**Braid:** bd-code-span-longer-backtick-run-nycn85a8
**Branch:** `braid/nycn85a8-code-span-backtick-run` (topic branch in the main checkout, based on `main` \@ `5b811915`; no worktree, per user request)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The failure mechanism is fully understood and reproduced
at HEAD with the tree-sitter CLI; the scanner already carries the state the
fix needs (`code_span_delimiter_length`, serialized); the change is a small
grammar + scanner edit plus tests. The open questions are about scope
(which neighbouring CommonMark code-span cases to fold in) rather than
about the mechanism.

## Issue context

Filed 2026-09-24 by Carlos (bug, P2, open). An inline code span whose
content contains a backtick run *longer* than its own delimiter fails with
an uncoded `Parse error`:

    a `x``y` b             -> Parse error   (2-run inside 1-delim)
    a ` ```mermaid ` b     -> Parse error   (3 inside 1)
    a `` ```{r} `` b       -> Parse error   (3 inside 2)
    a ``` x````y ``` b     -> Parse error   (4 inside 3)
    a `` `x` `` b          -> OK            (shorter inner run)

CommonMark's rule: a closer is a backtick string of *exactly* the opener's
length; any run of a different length (shorter **or longer**) is content.
q2 today effectively implements "any run shorter than the delimiter is
content", so authors must pick a delimiter longer than the longest inner
run. About 127 of the uncoded parse errors in the claude-notes render
(bd-uk8zgkha) are this class; agents write "` ```mermaid ` fenced block"
routinely. The error also cascades into later inline content in the same
block (e.g. a later `~` reported as Q-2-17).

Carlos's 2026-09-25 comment: this is a known tree-sitter-qmd limitation
with no earlier design; approach to try is tracking the opening run length
as explicit external-scanner state so a run of a different length inside
the span is emitted as content, with the state serialized for incremental
reparse.

## Dependency graph

- **discovered-from / parent-child:** bd-uk8zgkha (epic: render claude-notes
  as a q2 website). The epic's triage step says: for each error class,
  decide parser bug vs dialect difference. This one is a parser bug: pandoc
  and CommonMark both accept the inputs. The epic renders with the q2
  *nightly*, so the fix only pays off in claude-notes once a nightly ships
  it; until then documents use the longer-delimiter workaround.
- **blocks:** none. **related:** none beyond the parent. No incoming
  pressure other than the epic's error count.

## What the code looks like today

All paths current at HEAD (`5b811915`).

**Grammar** — `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:490`:

```js
pandoc_code_span: $ => prec.right(seq(
    alias($._code_span_start, $.code_span_delimiter),
    alias(repeat1(choice(
            /[^`\n\r]+/,
            /[`]/,                      // <-- one backtick at a time
            alias($._soft_line_break, $.pandoc_soft_break)
        )), $.content),
    alias($._code_span_close, $.code_span_delimiter),
    optional($.attribute_specifier)
)),
```

**Scanner** — `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`:

- `parse_code_span` (line 1974): counts the backtick run, `mark_end`, then
  emits `CODE_SPAN_CLOSE` iff `level == s->code_span_delimiter_length`, else
  tries `CODE_SPAN_START` (gated by `code_span_close_exists_ahead`, which
  already looks for a run of *exactly* `level`). Otherwise returns false.
- `parse_fenced_code_block` (line 740) is the paragraph-start twin; it also
  emits `CODE_SPAN_START`, but only for `level < 3`.
- Dispatch at line 2763: `` ` `` goes to `parse_code_span` when
  `CODE_SPAN_START || CODE_SPAN_CLOSE` is valid and
  `FENCED_CODE_BLOCK_START_BACKTICK` is not.
- `s->code_span_delimiter_length` (line 394) is **already** scanner state,
  **already serialized/deserialized** (lines 442, 476), and already used as
  an "inside a code span" flag by the soft-line-ending gates (lines 3149,
  3353, from bd-ilv8p). So the state half of Carlos's proposal exists.

**Mechanism of the bug** (confirmed with `tree-sitter parse`, see
`code-span-backtick-run-investigation/baseline-before.txt`, case 01):

1. `a `x``y` b`: opener commits (close-ahead finds the final single
   backtick). `code_span_delimiter_length = 1`.
2. Inside content at `` ``y `` the scanner counts a run of 2, which is not
   1, so it emits nothing and returns false.
3. The **internal** lexer then matches `/[`]/` — one backtick — as content.
4. The scanner is called again at the *second* backtick of the run: run
   length 1 == delimiter, so it emits `CODE_SPAN_CLOSE`. The span is
   `x` + one backtick, closed by the middle of the run.
5. The trailing `` ` `` after `y` has no closer ahead, so `CODE_SPAN_START`
   is refused and no internal rule accepts a lone backtick: parse error.

So the scanner *does* implement the exact-length rule; the internal
`/[`]/` rule defeats it by splitting runs. Every failing case in the
strand's table follows this pattern. A run *shorter* than the delimiter
survives only because the split-off single backticks never equal the
delimiter length.

**Consumer side** — `crates/pampa/src/pandoc/treesitter_utils/code_span_helpers.rs:75`
walks the `content` node and concatenates raw bytes between structural
`pandoc_soft_break` children. Hidden (underscore-prefixed) external tokens
do not appear in the tree, so replacing `/[`]/` with a hidden external
token changes nothing on the pampa side.

**Baseline at HEAD** (`bash claude-notes/plans/code-span-backtick-run-investigation/run-cases.sh`):

| case | input | HEAD | cause |
| --- | --- | --- | --- |
| 01–04 | strand's four failing rows | ERROR | this bug |
| 05–07 | strand's three OK rows | ok | |
| 12 | `` ` `` ` `` (spec) | ERROR | this bug |
| 13 | `` `  ``  ` `` (spec) | ERROR | this bug |
| 17 | `` ` foo `` bar ` `` (spec) | ERROR | this bug |
| 15 | `` `foo\`bar` `` (spec) | ERROR | trailing lone backtick after the span (q2 strictness, see Q3) |
| 18, 19 | `*foo`*``, `[not a `link](/foo`)` | ERROR | Q-2-12 / unclosed bracket: q2 strictness, unrelated |
| 20–22 | unclosed backtick strings (spec says literal) | ERROR | uncoded parse error, q2 strictness (Q3) |
| 30 | ```` ``` x ``` b ```` at paragraph start | ERROR | `level < 3` gate in `parse_fenced_code_block` (Q2) |
| 31 | fence line inside an open span | ERROR | same split-run mechanism; CommonMark would interrupt the paragraph (Q4) |
| 10, 11, 14, 16, 23 | remaining spec examples | ok | |

## Proposed phases (draft)

- **Phase 0 — Tests first.** Add corpus tests to
  `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/code_span.txt`
  for every inner/delimiter length combination in the strand table (1..4
  inner, 1..3 delimiter, both directions), the in-scope CommonMark examples
  (12, 13, 17), and pampa integration tests asserting the `Code` text
  (`crates/pampa/tests/integration/`, existing `test_pandoc_code_span_*`
  family in `test_treesitter_refactoring.rs`). Confirm they fail at HEAD.
- **Phase 1 — Scanner: emit whole runs as content.** Add an external token
  (working name `_code_span_backtick_run`) to `externals` and the
  `TokenType` enum. In `parse_code_span`, after the close check: if the
  token is valid and `s->code_span_delimiter_length > 0` and
  `level != s->code_span_delimiter_length`, `EMIT_TOKEN` it (the run has
  already been consumed and `mark_end`ed). No new serialized state is
  needed: `code_span_delimiter_length` already round-trips.
- **Phase 2 — Grammar: stop splitting runs.** In `pandoc_code_span`'s
  content choice, replace `/[`]/` with `$._code_span_backtick_run`. Rebuild
  (`tree-sitter generate; tree-sitter build`), run `tree-sitter test`, and
  refresh `baseline-after.txt` with the runner script.
- **Phase 3 — Verify downstream.** `cargo xtask verify`; check the
  merr-style error table (`resources/error-corpus/_autogen-table.json`)
  still regenerates cleanly, since adding an external token shifts parse
  states; re-render claude-notes with the local build and count remaining
  code-span parse errors against the strand's ~127.
- **Phase 4 — Docs.** Note the CommonMark-conformant rule in the parser
  docs / agent guidance (bd-uk8zgkha's guidance doc) and drop the
  "delimiter must be longer than any inner run" workaround from it.

## Open design questions for the user

1. **Is the token-emission design acceptable?** Concretely: a new hidden
   external token that `parse_code_span` emits for any backtick run inside
   an open span whose length differs from the delimiter, and the grammar's
   `/[`]/` alternative goes away. This is exactly the "track the opener
   length in the lexer" idea, and the state already exists. The alternative
   of keeping `/[`]/` and instead having the scanner *refuse* to close in
   mid-run is impossible: by the time the scanner sees the second backtick
   the first is already gone, so the run length is unrecoverable without a
   new token. I see no cheaper option.
2. **Paragraph-start triple-backtick spans (case 30).** ```` ``` x ``` b ````
   at the start of a paragraph is a code span in CommonMark (an info string
   may not contain backticks), but `parse_fenced_code_block` refuses
   `CODE_SPAN_START` for `level >= 3`. Fold this into the same change
   (lift the gate when the fence path rejects the line because the info
   string has a backtick), or file it separately?
3. **Unclosed backtick strings (cases 15, 20, 21, 22).** CommonMark renders
   an unmatched run as literal backticks; q2 raises an uncoded parse error.
   The strand says "at minimum, a Q-code if some case stays unsupported."
   Proposal: keep the strictness (consistent with q2's treatment of
   unclosed `*`, `~`, `'`) but give it a Q-code with the "add a matching
   closer or escape with `` \` ``" hint, as a separate strand. Agree?
4. **Fence line inside an open span (case 31).** After the fix, `a `x` +
   newline + ```` ``` ```` + newline + `b`` would parse as a three-line code
   span whose content contains the fence line. CommonMark and pandoc
   instead let the fence interrupt the paragraph. bd-ilv8p deliberately
   made the soft-line-ending gate ignore fences inside spans, so this is
   consistent with existing intent; I propose to leave it and just add a
   corpus test documenting the behaviour. OK?
5. **Acceptance for the claude-notes corpus.** The strand's acceptance
   includes "claude-notes shows no code-span parse errors". That needs a
   render with the *local* build, not the nightly. Is the check
   "`q2 render claude-notes --json-errors` from this branch, tally by code,
   zero uncoded errors pointing at a backtick" sufficient, or do you want
   the tally script from bd-uk8zgkha's workflow step 2 built as part of
   this strand?

## Risks / tradeoffs (draft)

- **Parse-state shift.** Adding an external token regenerates `parser.c`
  and renumbers states; anything keyed on state numbers (the merr-style
  error-corpus table) needs regenerating. Same cost as any grammar change
  here, but worth budgeting.
- **Pipe table cells.** The externals comment says the code-span tokens
  exist "for parsing pipe table cells"; `pandoc_code_span` is the only
  grammar user of them, so cells go through the same rule, but the
  pipe-table corpus tests should be re-run with a backtick-run case added.
- **Error recovery.** In tree-sitter error recovery every symbol is valid,
  so the new token could be emitted with a stale
  `code_span_delimiter_length`. Gate on `code_span_delimiter_length > 0`
  and `valid_symbols[CODE_SPAN_CLOSE]` together to keep it to genuine
  in-span states.
- **Incremental reparse.** No new state, so no new serialization surface;
  the existing round-trip of `code_span_delimiter_length` covers it. Worth
  one LSP-style edit test if one exists for code spans.
