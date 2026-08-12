# Scanner.c changes: always sweep the indentation × context dimensions

Any change to line-start behavior in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` — the two
SOFT_LINE_ENDING gates, the peek helpers (`peek_ordered_marker`,
`peek_dash_plus_opens_block`, the backtick/star/colon peeks),
`match_line`, or the marker parsers — must be tested across, at
minimum:

- **indent**: 0, 1, 3, 4+ leading spaces on the affected line;
- **context**: top level, inside a list item (both at and past the
  item's content column), inside a block quote.

A corpus case at indent 0 tells you nothing about indent 2. The
regression tracked by bd-indented-continuation-parse-error-j7be7kuc
(introduced by 92737cdd, shipped in v0.18.0) passed 572/572
tree-sitter tests because every new corpus case sat at indent 0; at
indent 1–4 the same inputs were hard parse errors that dropped entire
files from renders.

## Why this space is treacherous

1. **`s->indentation` changes meaning between the two gates.** At
   gate 1 it is the *raw* column count — `match_line` has not yet
   consumed open-block prefixes (`> `, list-item continuation
   columns). At gate 2 (post-`match_line`) it is *post-prefix*. A
   guard like `if (s->indentation > 3)` is only correct at one of the
   two call sites; at gate 1 it misjudges a legitimate nested marker
   at raw indent 4 inside a list as prose.

2. **Skipping `mark_end` after a peek leaves the line's leading
   whitespace for the next scan, and who consumes it depends on what
   follows.** An external-scanner token (code-span start, emphasis
   open) absorbs it into its own range; `block_continuation` claims
   exactly the open blocks' prefix columns; anything else falls to
   the internal lexer as `_whitespace`. Whether the grammar state
   after `_soft_line_ending` can shift that `_whitespace` decides
   between a clean parse and a hard error — and a hard parse error
   drops the *whole file* from the render, not just the block.

3. **Gate 2 reuses gate 1's peek verdict** (`first_peeked` short-
   circuits the second peek), so a gate-1 misjudgment propagates even
   though gate 2 runs with better information.

## Checklist for scanner line-start changes

- Add tree-sitter corpus cases for the full indent × context sweep of
  the construct you are touching, not just the motivating input.
- Diff behavior against Pandoc (and Quarto 1 where relevant) for each
  cell of the sweep; qmd deviations must be deliberate, not
  accidental.
- Exercise at least one failing-direction case end-to-end
  (`cargo run --bin q2 -- render`) — the corpus alone does not show
  whether an ERROR node drops the file.
- If you touch `mark_end` placement or a peek helper, re-read this
  file's "treacherous" list against your change.

Related: `claude-notes/instructions/parser-whitespace-policy.md`
(non-ASCII whitespace policy, a different scope).
