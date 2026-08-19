# `page-footer` item `text:`: a lone image is dropped, and no link or image target is resolved

**Observed with:** q2 0.23.0.
**Repro:** `q2 render` in this directory; look at
`_site/deep/deeper/index.html`, a page two directories down, where a
correct path has to read `../../images/logo.svg`.

q2 parses a `page-footer` item's `text:` as markdown — bold, emphasis,
code and links all render. Two things then go wrong, independently: a
lone image is dropped, and nothing inside an item's `text:` has its
paths resolved.

## Defect 1 — a lone image renders nothing

| footer item `text:` | rendered `<li>` |
|---|---|
| `![lone image](/images/logo.svg)` | *(empty)* |
| `![lone image, relative](images/logo.svg)` | *(empty)* |
| `[![wrapped in a link](images/logo.svg)](https://posit.co)` | `<a …><img …></a>` |
| `![image](images/logo.svg) beside text` | `<img …> beside text` |
| `` `<img src="/images/logo.svg" …>`{=html} `` | `<img …>` |

An image that is the item's *only* content disappears; wrap it in a
link, or put any other inline beside it, and it survives.

The mechanism is a two-step. `with_paragraph` in
`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` desugars a
single-image paragraph into a `Figure`. Then `block_inlines` in
`crates/quarto-navigation/src/render_html.rs`, which is how
`render_text` gets inlines out of a parsed-markdown `ConfigValue`,
matches only `Plain`, `Paragraph` and `Header` and returns `None` for
everything else — including `Figure`. The item renders as the empty
string. Adding any sibling inline keeps the block a `Paragraph`, which
is exactly why the workarounds work.

A footer item is an inline context, so the fix is presumably to unwrap
the `Figure` back to its image rather than to teach `block_inlines`
about figure captions.

## Defect 2 — nothing inside an *item's* `text:` is resolved

Not just images, and not just root-absolute paths: no `Link` or `Image`
target inside a footer **item's** `text:` is rewritten at all. The
control is a *region-level* `text:` carrying the identical markdown, in
the same footer of the same render, two directories deep:

```
page-footer.center: '![x](/images/logo.svg) … [y](/index.qmd)'
  ->  src="../../images/logo.svg"   href="../../index.html"      resolved

page-footer.right[0].text: (the same string)
  ->  src="/images/logo.svg"        href="/index.qmd"            untouched
```

The region-level side rebases the path *and* rewrites `.qmd` to
`.html`. The item-level side does neither, so every such reference
404s on any page below the site root.

`crates/quarto-core/src/transforms/footer_render.rs` shows why, and
that the intent was the opposite. Its call site is commented "Rewrite
hrefs in each Items region, **and Link/Image targets inside Text
regions' parsed markdown**", and `rewrite_region_hrefs` splits:

- `FooterRegion::Text(cv)` → `rewrite_config_inlines(...)`, which walks
  the inlines and rewrites both node kinds. This is the control above.
- `FooterRegion::Items(items)` → `rewrite_items_hrefs(...)`, which
  touches `item.href` and recurses into `item.menu` — and never looks
  at `item.text`, which is where an item's markdown inlines live.

So the machinery exists and is invoked one branch over. Item text was
simply never routed through it.

**Nothing warns, and the asymmetry extends to diagnostics.** Write
`![x](images/logo.svg)` in the body of `deep/deeper/index.qmd` and q2
raises `Q-5-6 Referenced resource not found`, correctly, because
`deep/deeper/images/logo.svg` does not exist. The footer items here
point at exactly that missing file and the render is clean, exit 0.

## Why this matters for the Connect port

It closes off what looked like the docs-side fix for
br-root-absolute-assets-1o6yy4mx. That strand's four surviving
root-absolute paths are `<img>` tags inside `` `…`{=html} `` in
`_quarto.yml`'s `page-footer`, and the recorded plan was to "move the
markup out of raw HTML into a form q2 owns and resolves". Measured
here, that form does not currently exist: written as markdown the
images are either dropped (lone) or left unrebased (everything else),
which is no better than the raw HTML they would replace.

## A note on the Quarto 1 comparison

`_site-q1/` is **not** a clean control for defect 1. The Quarto 1 dev
build used here (99.9.9) does not parse footer `text:` as markdown at
all — it emits the literal `![lone image](…)` source — so it has no
opinion on how a lone image should render. The frozen Connect docs Q1
render *does* show the footer images working, so released Q1 behaves
differently again; treat the markdown half of this repro as a statement
about q2 only.

For defect 2 it *is* a clean control, and a striking one: Q1 rewrote
`src="/images/logo.svg"` to `src="../../images/logo.svg"` **inside a
string it was otherwise emitting literally**, because its deno-dom pass
rewrites the rendered HTML rather than the AST. That is the mechanism
q2 deliberately does not have, and the reason
br-root-absolute-assets-1o6yy4mx needed a design decision rather than a
patch.
