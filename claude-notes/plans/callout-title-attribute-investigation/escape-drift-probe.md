# Probe: attribute values are unescaped, but their spans are raw

Evidence behind the source-mapping design in the plan. Run against
`origin/main` @ `b2b6100c`.

## Input

```markdown
::: {.callout-note title="Say \"hello\" now"}
Body.
:::

::: {.callout-tip title="Use `renv` today"}
Body.
:::
```

## Invocation

```
cargo run --bin pampa -- --to json <file>
```

## Result

The JSON carries `kvs` as `[key_source_id, value_source_id]` pairs into the
`astContext.p` table of ranges.

| | stored value | value span | raw bytes at that span |
|---|---|---|---|
| callout 1 | `Say "hello" now` (15 B) | `[25,44]` (19 B) | `"Say \"hello\" now"` |
| callout 2 | `` Use `renv` today `` (16 B) | `[81,99]` (18 B) | ``"Use `renv` today"`` |

Raw bytes confirmed with `dd if=<file> bs=1 skip=25 count=19`.

So: **the stored value is unescaped and unquoted; the span is raw and
quote-inclusive.** The two texts are not the same string, and the difference is
not a constant shift.

## Why that breaks naive nesting

`SourceInfo::substring(parent, start, end)` is affine — `resolve_byte_range`
does `parent_start + start_offset`
(`quarto-source-map-0.1.0/src/source_info.rs:388-403`) — and the nested reader
supplies offsets into the *inner* string
(`crates/pampa/src/pandoc/location.rs:213-218`).

Worked example, callout 2 (value span starts at byte 81):

```
inner:  U  s  e     `  r  e  n  v  `     t  o  d  a  y
index:  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
raw:   82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97
```

The `` `renv` `` code span is at inner `4..10`. Naive substring maps it to
`85..91`. Its true location is `86..92` — off by one, the opening quote, with
no escape involved. Each collapsed `\X` adds another byte of drift.

Drift is one-directional: unescaping only ever shrinks, so `inner_len ≤
span_len - 2` and every mapped offset stays *inside* the attribute's raw
extent. Wrong, but bounded and never spilling into a neighbouring attribute —
which is what makes the coarse fallback safe.

## Where the transformation happens

- `crates/pampa/src/pandoc/treesitter.rs:1207-1212` — `key_value_value` node:
  stores `extract_quoted_text(text)` but keeps `node_location(node)` (the full,
  quote-inclusive range).
- `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:28-59` —
  `extract_quoted_text` strips delimiters; `unescape_punctuation` collapses
  `\X` → `X` for ASCII punctuation, preserving `\` before non-punctuation and a
  lone trailing `\`.
- `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:580-585` — the
  aliased token includes the quote characters, which is why the span does.

## Corollary: the same bug exists in the YAML path

`quarto-yaml`'s `compute_scalar_len` deliberately spans the quotes ("which is
what a diagnostic wants to underline") while `parse_config_string_as_markdown`
is handed the *decoded* scalar
(`crates/pampa/src/pandoc/meta.rs:59-66`, `:240-330`). Nothing compensates. See
`claude-notes/plans/2026-07-20-ipynb-surface-syntax-design.md:73-92`, which
states the affine-only constraint and notes the YAML analogue "currently
punts."

Filed separately; not a blocker for the callout fix.
