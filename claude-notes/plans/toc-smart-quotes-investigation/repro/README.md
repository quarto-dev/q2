# TOC entries lose the quote glyphs around a quoted span

**Observed with:** q2 0.19.0, re-verified 0.20.0 (binary; HEAD 0dcd7e83).
**Repro:** `q2 render` in this directory; compare with
`quarto render --output-dir _site-q1`.

The heading `## Using a "raw" volume` renders with curly quotes, but its
TOC entry drops the quote characters entirely — the two disagree within
one page. This is specific to `Inline::Quoted` nodes: apostrophes and
dashes, which are `Str`-internal smart-typography rewrites, come through
correctly in both places (headings 2 and 3 are the controls).

## Expected (Quarto 1)

```
heading:  Using a “raw” volume
TOC:      Using a “raw” volume
```

## Actual (q2 0.20.0)

```
heading:  Using a “raw” volume
TOC:      Using a raw volume          <-- glyphs gone
```

Controls, correct in both engines:

| heading source | heading | TOC |
|---|---|---|
| `repository's identifiers` | `repository’s` | `repository’s` |
| `Gallery -- really` | `Gallery – really` | `Gallery – really` |

## Root cause

`crates/pampa/src/toc.rs:424` recurses into the quoted content but never
emits the delimiters, and ignores `q.quote_type`:

```rust
Inline::Quoted(q) => text.push_str(&inlines_to_text(&q.content)),
```

The match there is exhaustive (no `_` arm), so this is a localized fix.
Note that the sibling helper in `crates/quarto-core/src/template.rs:1081`
*does* emit delimiters, but straight ones.

## Impact in the Connect docs port

One heading in the whole corpus: `Option 2: Using a “raw” NFS volume` in
`admin/getting-started/off-host-install/configure-helm-chart`. Found via
br-wu5cbkws.

## Related

The same heading also gets a wrong anchor id under q2, via a different
code path (`crates/pampa/src/utils/autoid.rs`) that drops the quoted
content outright. That is a separate bug with its own repro at
`../heading-id-drops-inline-content/`.
