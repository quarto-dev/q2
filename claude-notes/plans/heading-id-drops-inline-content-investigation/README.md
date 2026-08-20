# Auto-generated heading ids silently drop most inline content

**Observed with:** q2 0.20.0 (binary; HEAD 0dcd7e83).
**Repro:** `q2 render` in this directory; compare with
`quarto render --output-dir _site-q1`.

When a heading has no explicit `{#id}`, q2 derives one from the heading
text — but the collector handles only `Str`, `Space`, `Emph`, `Strong`
and `Code`. Every other inline kind hits a catch-all that discards it
without recursing, so the *entire subtree* vanishes from the id: not just
the markup, the words inside it. The heading text itself renders fine;
only the anchor is wrong.

## Expected (Quarto 1) vs actual (q2 0.20.0)

| heading source | Quarto 1 id | q2 id |
|---|---|---|
| `## Using a "raw" volume` | `using-a-raw-volume` | `using-a-volume` |
| `## See [the docs](…) now` | `see-the-docs-now` | `see-now` |
| `## Use ~~strike~~ here` | `use-strike-here` | `use-here` |
| `## Math $x+y$ inline` | `math-xy-inline` | `math-inline` |
| `## Small [caps]{.smallcaps} here` | `small-caps-here` | `small-here` |
| `## Use *emphasis* and **strong** and `code` here` | `use-emphasis-and-strong-and-code-here` | same (control) |

In q2 the id lands on the wrapping `<section>`; in Quarto 1 on the
heading's `data-anchor-id`. That placement difference is unrelated and
not what this repro is about — compare the id *values*.

## Root cause

`crates/pampa/src/utils/autoid.rs:9`, `collect_text`:

```rust
Inline::Str(s)    => { write!(result, "{}", s.text).unwrap(); }
Inline::Space(_)  => { write!(result, " ").unwrap(); }
Inline::Emph(e)   => { collect_text(&e.content, result); }
Inline::Strong(s) => { collect_text(&s.content, result); }
Inline::Code(c)   => { write!(result, "{}", c.text).unwrap(); }
_ => {
    // Skip other inline types for ID generation
}
```

`Quoted`, `Link`, `Span`, `SmallCaps`, `Strikeout`, `Super`/`Subscript`,
`Math`, `Underline` and `Cite` all fall through. Called from
`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:943` (only when
`header.attr.0.is_empty()`) and from `crates/pampa/src/writers/qmd.rs:649`,
where the qmd writer decides whether a round-tripped explicit `{#id}` is
redundant — so the same gap makes qmd round-tripping asymmetric for these
headings.

Contrast `crates/pampa/src/toc.rs:409`, which recurses through all
container inlines. The two disagree, which is why the TOC label and the
anchor it points at can diverge.

## Impact in the Connect docs port

One heading in the corpus: `Option 2: Using a "raw" NFS volume` in
`admin/getting-started/off-host-install/configure-helm-chart`, whose id
goes from `option-2-using-a-raw-nfs-volume` to
`option-2-using-a-nfs-volume`. No page in the corpus links to that
anchor, so nothing breaks internally — but published Q1 URLs pointing at
it would 404 on the anchor. No heading in the corpus contains a link,
span, strikeout or math, so the wider bug class is currently unexercised
here.

## Related

Same heading, different code path: the TOC entry drops the quote glyphs
(`../toc-smart-quotes/`).
