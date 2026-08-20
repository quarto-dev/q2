# Probe: which headings inside non-section containers reach the TOC?

Purpose: establish the *actual* rule Quarto 1 / Pandoc use to decide what
lands in the TOC, because bd-tabset-headings-in-toc-t04ie7f7's stated root
cause ("Q1 rewrites the whole tabset to raw HTML, so nothing inside it can
reach the TOC") is **wrong** — Q1's `render_tabset` returns real AST
(`Div(.panel-tabset)[Plain(nav RawInlines), Div(.tab-content)[Div(.tab-pane)…]]`),
the same shape q2 emits.

## Run

```bash
quarto render . --output-dir _site-q1     # Q1 (dev checkout, reported 99.9.9)
cargo run --bin q2 -- render .            # q2 @ 5b6774d1
```

(`_site/` and `_site-q1/` are not committed — regenerate them.)

## Results captured 2026-08-18 (q2 @ 5b6774d1, quarto-cli dev 99.9.9)

TOC `data-scroll-target` ids:

| container            | Q1  | q2  |
| -------------------- | --- | --- |
| plain `::: {.my-wrapper}` | **included** | included |
| `::: {.callout-note}`     | excluded | excluded |
| `> blockquote`            | **excluded** | **included** ← q2-only leak |

Rendered body structure for the plain-div case:

```html
<!-- Q1: the Div is ABSORBED into the section, class merged -->
<section id="heading-inside-a-plain-div" class="level4 my-wrapper">
  <h4 …>Heading inside a plain div</h4>

<!-- q2: the Div survives, the header is never sectionized -->
<div class="my-wrapper">
  <h4 id="heading-inside-a-plain-div">Heading inside a plain div</h4>
```

## Conclusion — the real rule

Pandoc's TOC is *exactly the section tree*. `makeSections` recurses into
Divs and, when the Div has an empty id, **merges** the Div's attributes into
the section it wraps; it does **not** descend into `BlockQuote`. Then
`sectionToListItem` matches only `Div(_, _, Header : rest)` — a Div whose
first child is not a Header terminates the walk, with no recursion past it.

That single rule explains every row above *and* the tabset case:

- plain div → absorbed into a real section → in the TOC;
- callout → filter replaced it with non-section structure → gone;
- blockquote → never sectionized → bare `Header`, never matched → gone;
- tabset → the section *does* exist inside the pane
  (`<section id="create-the-integration" class="level4">` is present in Q1's
  render of the strand's repro), but it is buried under
  `Div(.panel-tabset)` whose first child is `Plain(nav)` — the walk stops
  there.

q2 instead recurses into *every* Div and into `BlockQuote`
(`collect_toc_entries`, `crates/pampa/src/toc.rs:341`), and its
`sectionize_blocks` (`crates/pampa/src/transforms/sectionize.rs:75`) never
recurses into Divs at all. The two divergences currently cancel out for the
plain-div case and compound for the tabset and blockquote cases.
