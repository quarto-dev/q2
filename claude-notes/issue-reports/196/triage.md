# Issue #196 — Reader rejects valid 4-space-indented list-item continuations as a parse error

- **GitHub:** https://github.com/quarto-dev/q2/issues/196
- **Reporter:** @rundel (Colin Rundel)
- **Status:** Confirmed regression introduced by PR #194 (closes #184, Q-2-35).
- **Verdict:** Real bug. Filed as bd-3mgb (discovered-from bd-7l1u, the Q-2-35 implementation).
- **Worktree branch:** `issue-196` at `.worktrees/issue-196/`.

## Scope

A single regression. No sub-bugs bundled in the report.

## Repro

```text
4)  Outer:
    
    ![](img.png){.border}
```

The blank line on row 2 contains four trailing spaces (hex dump: `20 20 20 20 0a`). With `pampa` reading from stdin:

```
$ cargo run --bin pampa -- < repro.qmd
Error: Parse error
   ╭─[ <stdin>:3:5 ]
   │
 3 │     ![](img.png){.border}
   │     ┬
   │     ╰── unexpected character or token here
───╯
```

Expected (pre-#194 behaviour, reported by @rundel): an `OrderedList` whose single item contains two `Para` children — verified locally against `exp-no-trailing-ws.qmd`, which strips the trailing whitespace and parses cleanly to:

```
[ OrderedList (4, Decimal, OneParen) [[Para [Str "Outer:"], Para [Image (...) [] ("img.png" , "")]]] ]
```

## Fixtures (all under `claude-notes/issue-reports/196/`)

| File                              | Trailing whitespace on blank line       | Result        |
| --------------------------------- | --------------------------------------- | ------------- |
| `repro.qmd`                       | 4 spaces                                | **Parse error** |
| `exp-no-trailing-ws.qmd`          | none (pure `\n\n`)                      | Parses OK     |
| `exp-two-trailing-spaces.qmd`     | 2 spaces                                | Parses OK     |
| `exp-tab-on-blank.qmd`            | 1 tab (= 4 columns of indentation)      | **Parse error** |
| `exp-trailing-ws-text.qmd`        | 4 spaces, continuation is plain text    | **Parse error** |
| `exp-bullet-list.qmd`             | 4 spaces, bullet list (`*`) instead of `4)` | **Parse error** |

Conclusions:

1. The trigger is the **amount** of trailing whitespace on the intervening blank line, not the kind of continuation content. The threshold is exactly 4 columns (a tab counts).
2. The bug is generic across list marker kinds (ordered, bullet) and content kinds (image, text).
3. The continuation line is **not** the issue — its indent is the standard 4 spaces required for a list-item continuation. Stripping trailing whitespace from the blank line above (a no-op for the AST) makes the same continuation line parse.

## Root cause (confirmed by tree-sitter trace)

PR #194 added an `INDENTED_CODE_BLOCK_DISALLOWED` external token in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:2128`:

```c
// Parse any preceeding whitespace and remember its length.
for (;;) {
    if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        s->indentation += advance(s, lexer);
    } else {
        break;
    }
}

// Q-2-35: ...
if (s->indentation >= 4 &&
    (valid_symbols[ATX_H1_MARKER] || valid_symbols[BLANK_LINE_START]) &&
    lexer->lookahead != '\n' && lexer->lookahead != '\r') {
    mark_end(s, lexer);
    EMIT_TOKEN(INDENTED_CODE_BLOCK_DISALLOWED);
}
```

Running `pampa -v` on `repro.qmd` shows the scanner emitting that token during the recovery path while at `row:2`:

```
recover_to_previous state:345, depth:2
...
lex_external state:5, row:2, column:16
lexed_lookahead sym:_indented_code_block_error, size:0
detect_error lookahead:_indented_code_block_error
```

So the regression is specifically the new check firing where it shouldn't. The PR's own description listed the cases the conservative gate was meant to skip — and "list-item continuations after a blank line" was one of them. What the gate misses is the case where the blank line itself carries enough whitespace to push `s->indentation >= 4`: after consuming the blank line, the scanner re-enters at the continuation line with state that makes `valid_symbols[ATX_H1_MARKER] || valid_symbols[BLANK_LINE_START]` true (parser is at a block-start position) and the freshly accumulated indentation `>= 4`, so it fires the indented-block error.

@rundel's diagnosis lands exactly here: *"the regression is in the column accounting around whitespace-only lines inside list items rather than in the indented-code-block detector itself — Q-2-35 would have surfaced as `[Q-2-35] Indented code blocks are not supported`, not a bare 'Parse error'."* The Q-2-35 user-facing message is keyed on a specific `(state, sym)` pair in `quarto-parse-errors`; the recovery state hit here (`state:5`) is not in that table, which is also why the error renders as a generic "unexpected character or token" instead of the friendly Q-2-35 message.

## Severity

High. The reporter has three real-world hits in `quarto-dev/quarto-web`:

- https://github.com/quarto-dev/quarto-web/blob/baeab38627fcc3f3a9ea3ca3ea689ece413df65d/docs/authoring/diagrams.qmd#L98
- https://github.com/quarto-dev/quarto-web/blob/baeab38627fcc3f3a9ea3ca3ea689ece413df65d/docs/extensions/engine.qmd#L92
- https://github.com/quarto-dev/quarto-web/blob/baeab38627fcc3f3a9ea3ca3ea689ece413df65d/docs/publishing/netlify.qmd#L119

Editors commonly insert trailing whitespace on indented blank lines (auto-indent on Enter), so the input pattern is realistic. The failure mode is total parse rejection, not a degraded render — every document with this pattern is unreadable to Rust Quarto.

## Where the fix should land

Tree-sitter scanner in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` around line 2128. The detector needs to distinguish "true block-start with 4+ leading spaces of content" from "list-item lazy continuation whose preceding blank line contained whitespace". The PR's intent was already to skip the latter; the gate just doesn't express that condition.

Possible angles (for the fix session, not this triage):

1. **Reset/decrement `s->indentation` when entering through the BLANK_LINE_START path of the previous scan.** If the blank-line case at line 2150 cleared any accumulated indentation, the next call would start fresh. Need to verify whether that is sound elsewhere.
2. **Tighten the gate so it does not fire when inside an open list item that is awaiting a continuation paragraph.** The list-item container state should be inspectable; if the parser would accept a list continuation here, the indented-code-block check should defer.
3. **Add a corpus test for trailing-whitespace blank lines** to whichever fix is chosen, and a positive Q-2-35 regression to confirm the original PR's diagnostic still fires on real top-level indented blocks.

The reporter's note about a `(state, sym)` mapping miss (the bare "Parse error" instead of Q-2-35) is a separate observation — once the false-positive is silenced, that mapping question goes away for this input. No need to widen the error corpus to cover the recovery state if we stop emitting the token in the first place.

## What I did not do

- I did not write a fix. This is a triage record.
- I did not add a tree-sitter corpus regression test. That belongs to the fix PR (TDD: red test first).
- I did not run `cargo xtask verify` on the worktree to full green; only the Rust legs were exercised. Hub-client tests fail with `vitest: command not found` because no `npm install` has been run in this clone. The bug is scanner-only; hub-client is not in scope.
