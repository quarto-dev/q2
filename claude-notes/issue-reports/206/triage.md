# Issue #206 — Fenced div close `:::` immediately after a pipe table fails to parse

- **GitHub**: https://github.com/quarto-dev/q2/issues/206
- **Reporter**: @rundel (Colin Rundel), 2026-05-15
- **Triage date**: 2026-05-15
- **Worktree**: `.worktrees/issue-206` (branch `issue-206`, based on `main` @ `09b2de7e`)
- **Beads issue**: bd-expy
- **Scope**: the specific parse failure when a `:::` line directly follows a pipe table row. Adjacent oddities found during triage (pipe table absorbing trailing block content as cells) are flagged but **not** in scope for this issue.

## Summary

Reproduced cleanly. The reporter's hypothesis — that the pipe-table parser mis-handles `:::` after a row — is confirmed and refines to a specific cause: **the first `:` of `:::` is consumed as the start of a `caption`** (which is the only block construct, besides another `pipe_table_row`, that the grammar accepts in that position after a row terminator). Once that `:` is shifted, the parser is committed to the `caption` rule and the next `:` is a parse error because `caption` requires inline whitespace immediately after the colon. A blank line between the table and `:::` breaks the table out of "looking for more rows / a caption" state, which is why the reporter's workaround succeeds.

This matches the user's suspicion exactly: table-caption detection (which also starts with `:`) is what's getting confused.

## Reproduction

Failing input (`claude-notes/issue-reports/206/repro.qmd`):

```
::: foo
|   |   |
|:-:|:-:|
| a | b |
:::
```

```
$ cargo run --bin pampa -- < claude-notes/issue-reports/206/repro.qmd
Error: Parse error
   ╭─[ <stdin>:5:2 ]
   │
 5 │ :::
   │  ┬
   │  ╰── unexpected character or token here
───╯
```

Working: with a blank line before `:::` (`exp-blank-line.qmd`) it parses into a `Div` containing the `Table`.

Caption sanity check (`exp-caption-no-div.qmd`): `: This is a caption` after the table parses correctly into a `Caption` — confirms the caption path is functional and the bug is specifically the `:::` × caption-start collision.

The fenced div wrapper is **not required to trigger the bug.** Even without `::: foo` on top, a bare pipe table followed by `:::` fails identically (`exp-no-div-just-colons.qmd`, error at the second `:`).

## Localization

`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`:

- `caption` rule, line 235:
  ```
  caption: $ => seq(":", $._inline_whitespace, $._inlines, choice($._newline, $._eof)),
  ```
- `pipe_table` rule, lines 245–253. After the row repeat, the rule has
  `optional(seq($._pipe_table_newline, $.caption))`. After consuming the
  `_pipe_table_newline` that ends `| a | b |`, the parser sees `:` and the only
  productions reachable with `:` are this `caption` and the top-level
  `caption` at line 125 (which has the same prefix). Either way the first `:`
  is shifted; only `_inline_whitespace` is acceptable next, so the second `:`
  errors.

Concrete syntax-tree trace evidence (`cargo run --bin pampa -- -v`):

```
state:949, row:4, col:0           # just finished pipe_table_row
lexed_lookahead sym::, size:1     # the first colon of ":::"
reduce sym:_pipe_table_newline    # consume the newline
shift state:2957                  # SHIFT the ':' — committed to caption
state:2957, row:4, col:1
lexed_lookahead sym::, size:1     # second colon
detect_error lookahead::          # bang
```

So the commit point is the shift of the first `:`. After that, tree-sitter cannot back-track because the `optional()` wrapper has already been resolved by lookahead.

The reasonable fix shapes (to be decided in the fix issue, not here):

1. **Tighten `caption`'s first token via the external scanner.** Have the scanner emit a `_caption_start` token only when it sees a `:` that is *not* immediately followed by another `:` (and is followed by whitespace). The internal `caption` rule then keys off `_caption_start` rather than the literal `:`. This is the most surgical fix and matches the spirit of how the scanner already disambiguates `_pipe_table_delimiter` from prose `|`.
2. **Have the scanner refuse `_pipe_table_newline` when the next non-blank line starts with `:::`.** Lets the pipe_table close before the parser commits to a caption start. Slightly broader — it also helps the table-rows-absorbing-following-content issue below.
3. **Refuse to shift `:` as the start of `caption` when followed by another `:`.** Could be done with a GLR/multi-version branch but tree-sitter handles ambiguity via the external scanner, so this collapses to (1).

The `scanner.c` already has the necessary line-start lookahead machinery for `_pipe_table_line_ending` (see lines 970–1041 of `grammar.js` for the externals list), so adding a `_caption_start` external token alongside is the natural shape.

## Adjacent finding (NOT in scope for this issue)

While testing variations, I found the pipe-table parser absorbs trailing non-pipe block content as additional cells:

```
$ printf '|   |   |\n|:-:|:-:|\n| a | b |\n# heading\n' | cargo run --bin pampa --
[ Table ... [Row ... [Cell ... [Plain [Str "a"]] ... [Plain [Str "b"]] ] ,
                     Row ... [Cell ... [Plain [Str "#", Space, Str "heading"]] ] ] ]
```

The `# heading` is silently swallowed as a one-cell row. Same with bare paragraph text. The reporter's claim "Other block successors (headings, paragraphs) immediately following a pipe table parse fine" is therefore **not quite right** — they don't error, but they don't parse as separate blocks either. This is a distinct bug worth filing separately, but it's out of scope for issue #206, which is specifically about the `:::` parse failure. I'll note this in the beads issue's description so it can be picked up as a related follow-up if desired.

## Open questions — resolved during triage

- **Is the fenced-div wrapper required for the bug?** No. Bare pipe table + `:::` reproduces the same parse failure. The fenced div is incidental; the real interaction is "pipe-table row context" × "`:` caption start". (Confirmed via `exp-no-div-just-colons.qmd`.)
- **Does the working caption path still work?** Yes (`exp-caption-no-div.qmd`). The fix must not regress single-`:`-as-caption.
- **Does TS Quarto accept this in the wild?** The reporter cites `quarto-dev/quarto-web` `docs/websites/website-navigation.qmd` lines 155–159, which is shipped quarto-web documentation. Strong evidence that this is expected-to-work syntax for users coming from TS Quarto, so the fix should make it parse rather than improve the error message.

## Outcome / recommended next step

**Filed bd-expy** with fix scope:

- Disambiguate the `:` caption-start from `:::` (and likely `::` in fenced-div opener too) at the scanner level so the parser doesn't commit to `caption` when the next character is also `:`. Approach (1) above is the recommended starting point.
- Add a tree-sitter test in `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/` covering: `:::`-after-table (the bug), `: caption`-after-table (must still work), and bare `:::` after table (no fenced div).
- Add a pampa round-trip test against `repro.qmd`.
- Re-test on `quarto-dev/quarto-web` `docs/websites/website-navigation.qmd` after the fix.
- **Not in scope**: the table-absorbs-following-blocks behaviour; file as a separate follow-up if desired.

## Verification commands used

```bash
# Pre-flight
cargo xtask verify --skip-hub-build --skip-hub-tests

# Reproduce
cargo run --bin pampa -- < claude-notes/issue-reports/206/repro.qmd

# Verbose CST trace
cargo run --bin pampa -- -v < claude-notes/issue-reports/206/repro.qmd

# Comparison fixtures (under claude-notes/issue-reports/206/)
cargo run --bin pampa -- < exp-caption-no-div.qmd     # caption alone — works
cargo run --bin pampa -- < exp-no-div-just-colons.qmd # table + ::: no div — same error
cargo run --bin pampa -- < exp-heading-after.qmd      # table absorbs heading as row
cargo run --bin pampa -- < exp-para-after.qmd         # table absorbs paragraph as row
cargo run --bin pampa -- < exp-blank-line.qmd         # workaround — parses
```

## Cross-references

- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` lines 117–253
- Scanner: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` (externals list at `grammar.js` lines 970–1041)
- In-the-wild example cited by reporter: `quarto-dev/quarto-web/blob/.../docs/websites/website-navigation.qmd` L155–L159
