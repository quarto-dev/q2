# 2026-05-12 — Fix displaymath column-strip to use enclosing paragraph column

- **Beads:** [bd-qpa2](https://example/none) — *Display math column-strip uses wrong column source, mishandles inline-wrapped and labeled math (issue #181 follow-up)*
- **Related:** bd-q6ed (the original column-strip fix), upstream GH #181
- **Worktree:** `.worktrees/bd-qpa2-display-math-column-strip/`
- **Branch:** `beads/bd-qpa2-display-math-column-strip`
- **Triage record:** `claude-notes/issue-reports/181/triage.md` on the `issue-181` branch

## Overview

bd-q6ed added a column-strip in `pampa/src/pandoc/treesitter.rs` to remove the block-continuation prefix (`> `, list-item indent, etc.) that the `pandoc_display_math` grammar regex captures verbatim from the source bytes. The strip uses `node.start_position().column` (the column of the opening `$$`) as the prefix width. That column equals the cumulative block-continuation prefix width **only** when `$$` is the leftmost non-prefix character on its line. As soon as anything precedes `$$` on the same line — `_`, `**`, `[`, the writer's own `[` for `quarto-math-with-attribute` Spans, or an explicit ID/label expression — the column is wrong and the strip mis-fires.

Pandoc's markdown reader handles every probed case correctly while keeping `DisplayMath` as an inline AST node (constraint from the user: there are large existing corpora with display math nested inside paragraphs, so a grammar restructure that promoted display math to a block element is off the table). Pandoc strips the cumulative block-continuation prefix width *in columns* from each interior line, where the width is derived from the enclosing block context — not from where `$$` sits on the opening line.

**Fix shape:** change the strip-width source from the math node's start column to the **enclosing block-leaf ancestor's** start column (`pandoc_paragraph` / `pandoc_plain`). One single-input change to the existing helper. No grammar change, no AST shape change.

This subsumes:

- The canonical bd-q6ed case (paragraph col equals math col, identical result).
- bd-qpa2 edge A: `> $$ ... $$ {#eq-p}` round-tripping with `> ` leakage on second parse.
- bd-qpa2 edge B: `$$ ... $$ {#eq-x}` losing interior whitespace on each round trip.
- `_$$\nmulti\n$$_` and similar inline-wrapping (which fail on **first** parse, no writer needed).
- `> _$$\nmulti\n$$_` (combined blockquote + inline wrap).

Pandoc reference behaviour table is in `claude-notes/issue-reports/181/triage.md` § *Second-pass investigation*.

## Constraints

1. **`DisplayMath` must remain an inline AST node.** Large existing corpora put display math inside paragraphs.
2. **No regressions in the existing bd-q6ed fixtures** (canonical blockquote + nested + list combinations).
3. **Cross-platform:** the helper must not assume Unix-only behaviour. Existing byte-level helper is already cross-platform; the change is purely in input sourcing.
4. **Pandoc compatibility:** the goal is to match Pandoc's reader behaviour on the probed cases.

## Out of scope

- **Writer change** to emit `$$\n…\n$$ {attr}` instead of `[$$…$$]{attr}` for `quarto-math-with-attribute` Spans. Nice-to-have for readability / Pandoc-output compatibility, but not required for round-trip correctness once the reader fix lands. Should be its own beads issue at lower priority.
- Grammar restructure to make `pandoc_display_math` a block element. Ruled out by constraint 1.

## Work items

### Phase 0 — Setup

- [x] Triage doc updated on `issue-181` branch with second-pass investigation
- [x] bd-qpa2 description rewritten with unified diagnosis
- [x] Worktree created (`.worktrees/bd-qpa2-display-math-column-strip/`)
- [x] HEAD green: `cargo xtask verify --skip-hub-build --skip-hub-tests` passes
- [ ] Plan file written (this document) and pointer added to CLAUDE.local.md

### Phase 1 — Inspect tree-sitter CST for representative inputs

- [x] `> _$$\n> a\n>   b\n> $$_` (blockquote + emph + multiline math)
- [x] `_$$\na\n  b\n$$_` (top-level emph + multiline math)
- [x] `- $$\n  a\n    b\n  $$` (list item + multiline math)
- [x] `> [$$\n> p(x)\n> $$]{#eq-p}` (writer's bd-qpa2 round-trip output)
- [x] `> > $$\n> > a\n> > $$` (nested blockquote)
- [x] `| $$x$$ |` in pipe table cell

Confirmed: `pandoc_paragraph` is the right ancestor for every multi-line case. Tight and loose lists both use `pandoc_paragraph` (no `pandoc_plain` rule in this grammar). Inline ancestor kinds between math and paragraph: `pandoc_emph`, `pandoc_span`. Helper should walk `parent()` past anything that isn't `pandoc_paragraph`. Full table in *Implementation notes → CST inspection notes* below.

### Phase 2 — Add failing fixtures (TDD)

- [x] Add `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/labeled_display_math_in_blockquote.qmd`
- [x] Add `labeled_display_math_top_level_indented.qmd`
- [x] Add `emph_around_multiline_display_math.qmd`
- [x] Add `emph_around_multiline_display_math_in_blockquote.qmd`
- [x] Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` and confirm:
  - new fixtures fail with divergent JSON between parses 1 and 3 ✓ (all 4 fail before fix)
  - existing fixtures (bd-q6ed and the rest) still pass ✓

### Phase 3 — Implementation

- [x] Read `crates/pampa/src/pandoc/treesitter.rs` around the existing `pandoc_display_math` arm and `strip_continuation_prefix` helper
- [x] Add `block_continuation_column(&Node) -> usize` helper that walks `parent()` until it finds a `pandoc_paragraph` ancestor and returns its start column; falls back to math node's own column if no such ancestor (single-line contexts like table cells / captions)
- [x] Update the `pandoc_display_math` arm to source `start_col` from the new helper
- [x] Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` — all fixtures pass ✓
- [x] Run `cargo nextest run -p pampa` — full pampa suite passes (3687/3687, 2 skipped) ✓
- [x] Inspect any newly-emitted code paths for cross-platform correctness — pure byte-level, no platform calls

### Phase 4 — Regression sweep

- [x] `cargo nextest run --workspace` — full workspace passes (8851 tests, 195 skipped) ✓
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests` — all verification steps passed ✓ (hub-client tests skipped per pre-existing `ERR_MODULE_NOT_FOUND` on HEAD; unrelated to this change)

### Phase 5 — End-to-end CLI verification

Per `CLAUDE.md` § *End-to-end verification before declaring success*. All commands run from the worktree root.

#### Fixture 1 — `labeled_display_math_in_blockquote.qmd` (bd-qpa2 edge A)

```
$ cargo run --bin pampa -- crates/pampa/tests/roundtrip_tests/qmd-json-qmd/labeled_display_math_in_blockquote.qmd
[ BlockQuote [Para [Str "Let", Space, Str "x.", SoftBreak,
              Span ( "eq-p" , ["quarto-math-with-attribute"] , [] )
                   [Math DisplayMath "\np(x)\n"], SoftBreak, Str "Done."]] ]

$ … | cargo run --bin pampa -- -t qmd
> Let x.
> [$$
> p(x)
> $$]{#eq-p .quarto-math-with-attribute}
> Done.

$ … -t qmd | cargo run --bin pampa --
[ BlockQuote [Para [Str "Let", Space, Str "x.", SoftBreak,
              Span ( "eq-p" , ["quarto-math-with-attribute"] , [] )
                   [Math DisplayMath "\np(x)\n"], SoftBreak, Str "Done."]] ]
```

Math.text on parse 1 and 3 are identical (`"\np(x)\n"`) — the `> ` leak is gone.

#### Fixture 2 — `labeled_display_math_top_level_indented.qmd` (bd-qpa2 edge B)

```
$ cargo run --bin pampa -- … (top-level $$ a\n  b\n$$ {#eq-x})
[ Para [Span ( "eq-x" , ["quarto-math-with-attribute"] , [] )
             [Math DisplayMath "\na\n  b\n"]] ]

$ … -t qmd
[$$
a
  b
$$]{#eq-x .quarto-math-with-attribute}

$ … -t qmd | … 
[ Para [Span ( "eq-x" , ["quarto-math-with-attribute"] , [] )
             [Math DisplayMath "\na\n  b\n"]] ]
```

`  b` (two leading spaces) preserved across the round trip.

#### Fixture 3 — `emph_around_multiline_display_math.qmd` (inline-wrap, top)

```
$ cargo run --bin pampa -- …
[ Para [Emph [Math DisplayMath "\na\n  b\n"]] ]

$ … -t qmd
*$$
a
  b
$$*

$ … -t qmd | …
[ Para [Emph [Math DisplayMath "\na\n  b\n"]] ]
```

The two-space indent on `  b` survives both parses — the brittleness on **first** parse that the second-pass investigation surfaced is gone.

#### Fixture 4 — `emph_around_multiline_display_math_in_blockquote.qmd`

```
$ cargo run --bin pampa -- …
[ BlockQuote [Para [Emph [Math DisplayMath "\na\n  b\n"]]] ]

$ … -t qmd
> *$$
> a
>   b
> $$*

$ … -t qmd | …
[ BlockQuote [Para [Emph [Math DisplayMath "\na\n  b\n"]]] ]
```

Both the `> ` blockquote prefix and the `  b` interior indent are handled correctly.

#### Regression check — bd-q6ed canonical case

```
$ cargo run --bin pampa -- crates/pampa/tests/roundtrip_tests/qmd-json-qmd/display_math_in_blockquote.qmd
[ BlockQuote [Para [Str "Before", SoftBreak,
              Math DisplayMath "\np = q\n", SoftBreak, Str "After"]] ]

$ … -t qmd
> Before
> $$
> p = q
> $$
> After
```

Identical behaviour to before this change. ✓

#### In-the-wild — Poisson (quarto-web `_equations.qmd`)

```
> Let $X_1, X_2, \ldots, X_n$ be a Poisson random variable, then
>
> $$
> p(x) = e^{-\lambda} \frac{\lambda^x}{x!}, x = 0, 1, 2 ,\ldots, n
> $$ {#eq-poisson}
```

First parse and reparse both produce `Math DisplayMath "\np(x) = e^{-\lambda} \\frac{\\lambda^x}{x!}, x = 0, 1, 2 ,\\ldots, n\n"`. Round-trip stable. ✓

#### In-the-wild — Black-Scholes (quarto-web `cross-references.qmd`)

```
$$
\frac{\partial \mathrm C}{ \partial \mathrm t } + \frac{1}{2}\sigma^{2} \mathrm S^{2}
  + \mathrm r \mathrm S \frac{\partial \mathrm C}{\partial \mathrm C}
  \mathrm r \mathrm C
$$ {#eq-black-scholes}
```

The interior `  + \mathrm r …` and `  \mathrm r \mathrm C` lines (each with two leading spaces) are preserved on both parses — confirmed by inspecting the `DisplayMath` content in the AST output. ✓

All outputs inspected. All checks pass.

- [x] Each new fixture exercised through the CLI end-to-end; outputs match expected values
- [x] bd-q6ed canonical fixture re-verified
- [x] Poisson + Black-Scholes in-the-wild patterns verified

### Phase 6 — Commit and report

- [ ] Stage and commit on the bd-qpa2 worktree branch
- [ ] Sync beads on main (close bd-qpa2 with reference to the commit)
- [ ] Report to user: diff summary, end-to-end verification output, snapshot/fixture counts
- [ ] Optionally file the writer-polish issue (out-of-scope) at lower priority

## Implementation notes

(filled in as we work)

### CST inspection notes

Confirmed `pandoc_paragraph` is the right block-leaf ancestor for every multi-line display math case. Math under a single-line context (table cell, caption, heading) sits in containers that don't need stripping (no interior body lines).

| Input | Path math → block ancestor | Math col | Paragraph col | Notes |
|---|---|---|---|---|
| `> _$$\n> a\n>   b\n> $$_` | math → emph → **paragraph** → bq | 3 | **2** | math col wrong (after `> _`); paragraph col right (after `> `) |
| `_$$\na\n  b\n$$_` (top) | math → emph → **paragraph** | 1 | **0** | math col wrong (after `_`); paragraph col right |
| `- $$\n  a\n    b\n  $$` | math → **paragraph** → list_item → list | 2 | 2 | same — canonical |
| `> [$$\n> p(x)\n> $$]{#eq-p ...}` | math → span → **paragraph** → bq | 3 | **2** | math col wrong (after `> [`); paragraph col right |
| `> > $$\n> > a\n> > $$` | math → **paragraph** → bq → bq | 4 | 4 | same — canonical |
| pipe table cell `\| $$x$$ \|` | math → pipe_table_cell → … | 2 | n/a | single-line, no body splits, no strip needed |

**Decision**: walk `parent()` from the math node and return the start column of the first ancestor with kind `"pandoc_paragraph"`. If no such ancestor is found (table cells, captions, headings — all single-line for display math purposes), fall back to the math node's own column. The conservative `{>, space, tab}` byte check in `strip_continuation_prefix` continues to guard against mis-stripping in any unforeseen edge case.

Other inline ancestor kinds encountered between math and paragraph: `pandoc_emph`, `pandoc_span`. The loop walks past these transparently — only the `pandoc_paragraph` kind is the target.

### Helper signature decision

*(pending Phase 3)*

### Edge cases discovered

*(pending — add anything we run into while implementing)*
