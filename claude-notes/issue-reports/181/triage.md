# Issue #181 — Display math inside a blockquote has its `> ` prefix doubled on round trip

Upstream: https://github.com/quarto-dev/q2/issues/181
Reporter: Colin Rundel (@rundel), 2026-05-11
Worktree branch: `issue-181`
Beads fix issue: bd-q6ed

## Verdict

**Confirmed bug, in the tree-sitter grammar.** Reporter's diagnosis is correct: the parser produces `Math DisplayMath` whose content string includes the literal `> ` block-continuation prefix on every line of the math body. The qmd writer is doing the right thing — when emitting a multi-line math block inside a blockquote, every body line needs a `> ` prefix — but because the parser already left `> ` in the content, the writer's correct re-prefix yields `> > ` and each round trip adds one more level.

## Reproduction (at branch HEAD on `main` = 90e29165)

Input (`repro.qmd`):

```
> Before
> $$
> p = q
> $$
> After
```

```
$ cargo run --bin pampa -- claude-notes/issue-reports/181/repro.qmd
[ BlockQuote [Para [Str "Before", SoftBreak, Math DisplayMath "\n> p = q\n> ", SoftBreak, Str "After"]] ]
```

Note the `"\n> p = q\n> "` — the `> ` continuation markers from lines 3 and 4 of the input are inside the math content.

Round-tripping `repro.qmd -> qmd` then re-parsing:

```
$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/181/repro.qmd
> Before
> $$
> > p = q
> > $$
> After

$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/181/repro.qmd | cargo run --bin pampa --
[ BlockQuote [Para [Str "Before", SoftBreak, Math DisplayMath "\n> > p = q\n> > ", SoftBreak, Str "After"]] ]
```

## Investigation

### Localised to the parser, not the writer

Feeding a clean `DisplayMath` value (no `> ` bytes inside) directly into the writer through Pandoc-JSON input produces the correct output (see `claude-notes/issue-reports/181/exp-only-math.qmd` and the JSON-driven round trip in this session):

```
$ printf '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"BlockQuote","c":[{"t":"Para","c":[{"t":"Math","c":[{"t":"DisplayMath"},"\\np = q\\n"]}]}]}]}\n' \
    | cargo run --bin pampa -- -f json -t qmd
> $$
> p = q
> $$
```

So the writer's blockquote-aware re-prefix logic is correct. The defect is entirely upstream — the parser hands the writer dirty content.

### CST shows the parser swallowing the prefix bytes

`pampa -v` on `exp-only-math.qmd`:

```
pandoc_block_quote: {Node pandoc_block_quote (0, 0) - (3, 0)}
  block_quote_marker: {Node block_quote_marker (0, 0) - (0, 2)}
  pandoc_paragraph: {Node pandoc_paragraph (0, 2) - (3, 0)}
    pandoc_display_math: {Node pandoc_display_math (0, 2) - (2, 4)}
      $$: {Node $$ (0, 2) - (0, 4)}
      $$: {Node $$ (2, 2) - (2, 4)}
```

There is exactly **one** `block_quote_marker` token for the entire blockquote — only the first line. The continuation `> ` characters on lines 1 and 2 are never matched as `block_continuation`; they fall inside `pandoc_display_math`'s body span, which is the literal byte range `(0, 2) - (2, 4)`.

### Root cause in `grammar.js`

`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:367`:

```js
pandoc_display_math: $ => seq(
    '$$',
    /([^$]|[$][^$]|\\\$)+/,
    '$$'
),
```

`pandoc_display_math` is registered as an **inline** element (`grammar.js:511`, inside `_inline_element`). Its body is a single regex match that consumes every byte (including `\n` and `> ` continuation prefixes) between the two `$$` delimiters. None of the block-continuation machinery (`$._newline` / `optional($.block_continuation)`) ever runs while the body regex is matching.

By contrast, the body of `pandoc_code_block` (`grammar.js:828`) is `code_fence_content: repeat1(choice($._newline, $._code_line))`. `$._newline` is `seq($._line_ending, optional($.block_continuation))` (`grammar.js:886`), so on each line of a fenced code block, the `block_continuation` (the leading `> ` inside a blockquote) is consumed as its own token and never ends up in the captured content. That is why the analogous round trip with fenced code blocks works correctly — verified with `exp-fenced-code-in-bq.qmd`:

```
$ cargo run --bin pampa -- claude-notes/issue-reports/181/exp-fenced-code-in-bq.qmd
[ BlockQuote [Para [Str "Before"], CodeBlock ( "" , [] , [] ) "p = q", Para [Str "After"]] ]
```

The code-block body is the clean `"p = q"` — no `> ` bytes leak through.

### Where the AST extraction reads the content

`crates/pampa/src/pandoc/treesitter.rs:502-513`:

```rust
"pandoc_display_math" => {
    let full_text = node.utf8_text(input_bytes).unwrap();
    let content = &full_text[2..full_text.len() - 2]; // Strip leading and trailing $$
    ...
    Inline::Math(Math { math_type: MathType::DisplayMath, text: content.to_string(), ... })
}
```

It reads the node text verbatim from the input bytes and strips only the `$$` delimiters. There is no awareness of the surrounding blockquote indentation, so any `> ` prefix bytes that the grammar didn't consume are passed straight into `Math.text`.

## Fix shape (not implemented by this triage)

Two viable approaches, both at the grammar layer:

1. **Make `pandoc_display_math` line-structured like `pandoc_code_block`.** Replace the single regex body with `repeat($._newline | $._math_line)` (or similar), so that `block_continuation` is consumed on each interior line and never enters the captured content. This is the structurally correct fix and mirrors the existing pattern used for code fences and fenced divs.

2. **Keep the regex body but strip `block_continuation` markers in `treesitter.rs` at AST construction time.** This is a smaller patch and may be appealing as a quick fix, but it duplicates blockquote-awareness logic that the grammar should already own, and it's fragile if math ends up inside nested blockquotes or list continuations.

Either way: also verify `pandoc_math` (inline `$...$`) does not have a similar latent issue when an inline-math span gets soft-broken across a `> ` boundary. (Not investigated here — flagging for the implementor.)

Once a fix lands, a round-trip regression test belongs in `crates/pampa/tests/roundtrip_tests/qmd-json-qmd` (per `crates/pampa/CLAUDE.md`) using `repro.qmd` as the input.

## Fix applied (this session)

After attempting approach (1) — grammar restructuring — I hit a structural conflict between `_inlines`-level soft line breaks and `pandoc_display_math` as an inline element spanning multiple `_line`s, with downstream tests (`Display math with list markers`, `Display math inside fenced div`) regressing into `ERROR` nodes. The fix that landed is a refined version of approach (2):

**Column-based prefix strip in the AST extractor** (`crates/pampa/src/pandoc/treesitter.rs`):

The opening `$$` sits at some source column `C` = `node.start_position().column`. The math body "should" start at column `C` on every interior line; bytes at columns `0..C` on those lines are the accumulated continuation prefix added by the chain of enclosing blocks (any combination of blockquotes, list items, fenced divs, etc.). The new `strip_continuation_prefix(content, C)` helper:

- Splits the body on `\n`.
- Leaves the first piece (content immediately following the opening `$$` on the same line) untouched.
- For every subsequent piece, strips the first `C` bytes — but **only if** every one of those bytes is in `{>, space, tab}`. Otherwise the line was matched via lazy continuation and we leave it alone rather than chewing real content off it.

This handles arbitrarily-nested combinations (`> - $$`, `- > $$`, `> - > $$`, `> > $$`, `> ::: ... > $$`, etc.) uniformly without enumerating block types or computing per-ancestor offsets, because column position already encodes the cumulative prefix width.

**Files changed:**
- `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` — unchanged (the attempted grammar restructuring was reverted).
- `crates/pampa/src/pandoc/treesitter.rs` — added `strip_continuation_prefix` helper; `pandoc_display_math` arm now passes the extracted body through it.

**Regression coverage** in `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`:
- `display_math_in_blockquote.qmd` (the reporter's exact input)
- `display_math_in_nested_blockquote.qmd`
- `display_math_in_list_in_blockquote.qmd`
- `display_math_in_blockquote_in_list.qmd`
- `display_math_in_bq_list_bq.qmd`

All five fixtures previously diverged on `qmd → JSON → qmd → JSON` and now round-trip cleanly.

**End-to-end check.** Reporter's repro:

```
$ cargo run --bin pampa -- claude-notes/issue-reports/181/repro.qmd
[ BlockQuote [Para [Str "Before", SoftBreak, Math DisplayMath "\np = q\n", SoftBreak, Str "After"]] ]

$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/181/repro.qmd
> Before
> $$
> p = q
> $$
> After

$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/181/repro.qmd | cargo run --bin pampa --
[ BlockQuote [Para [Str "Before", SoftBreak, Math DisplayMath "\np = q\n", SoftBreak, Str "After"]] ]
```

Output inspected: `Math DisplayMath` content is clean (`\np = q\n`), round-tripped qmd has the correct `> ` prefix on every line, and re-parsing the output yields the same AST as the original — fully idempotent.

**Verification:** `cargo nextest run -p pampa` (3685 passed, 2 skipped); `cargo xtask verify --skip-hub-tests` (full Rust workspace + WASM hub-client build + trace-viewer tests) all pass. Hub-client tests were not run because there is a pre-existing `ERR_MODULE_NOT_FOUND` issue in `vitest run` on `main` HEAD that is unrelated to this change.

## Scope decision

Issue contains a single defect; no scope question. Triaging the whole thing.

## Investigative artifacts

- `repro.qmd` — exactly the reporter's input.
- `exp-no-blockquote.qmd` — display math at top level, parses cleanly (baseline).
- `exp-only-math.qmd` — minimal blockquoted display math, reproduces the bug.
- `exp-blank-lines.qmd` — blockquote with blank `>` separators around the math block, still buggy.
- `exp-fenced-code-in-bq.qmd` — fenced code in a blockquote, parses cleanly (contrast).

## Outcome

- Beads issue filed for the fix (see commit footer).
- No upstream documentation needs updating; this is purely a parser correctness defect.
