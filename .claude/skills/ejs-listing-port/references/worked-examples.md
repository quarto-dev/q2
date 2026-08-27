# Worked examples — Q1 EJS → Q2 doctemplate

Three ports in increasing difficulty. Each states the Q1 shape, the Q2
template, and the decisions that were not mechanical.

The canonical reference for style is q2's own built-ins,
`crates/quarto-core/src/project/listing/templates/`. The user-facing
treatment is `docs/guides/projects/listing-templates.qmd`; this file is the
agent-facing companion with the parts a porter hits in practice.

---

## 1. Minimal — one link, one description

The most common Q1 listing template: a `{=html}` fence wrapping a loop that
emits an anchor and a paragraph.

**Before:**

````
```{=html}
<% for (const item of items) { %>
    <a href="<%- item.path %>"><%= item.title %></a><br/>
    <p><%= item.description %></p>
<% } %>
```
````

**After:**

````
$for(items)$
[$it.title$]($it.path$)

::: {.listing-description}
```{=html}
$it.description-placeholder-begin$
```

$if(it.description)$
$it.description$
$endif$

```{=html}
$it.description-placeholder-end$
```
:::

$endfor$
````

Two deliberate changes:

- **The link is markdown, not raw HTML,** so `$it.path$` (a source path) is
  rewritten to the output URL. Q1's EJS received already-resolved `.html`
  hrefs, so raw HTML was the norm there and carries over as a dead `.qmd`
  href.
- **The description is an envelope, not a plain variable.** Q1 auto-filled a
  missing description from the item page's first paragraph; Q2 does the same
  only inside the marker pair, and only if the markers are emitted
  unconditionally. `$if(it.description)$` guards the *explicit* description
  (reading it bare would warn `Q-12-10`); the markers sit outside it.

One cosmetic consequence worth knowing: markdown wraps a standalone link in
`<p>`, so where Q1 emitted a bare `<a>…</a><br/>`, this emits
`<p><a>…</a></p>`. That also makes the first `<p>` of a listing-only page an
item *title* — so if that page is itself an item in another listing, its
derived preview becomes a title rather than a description. Give such pages a
real `description:` in front matter.

This template is the in-repo fixture for
`custom_template_it_spelling_derives_description_without_front_matter` in
`crates/quarto-core/tests/integration/listing_pipeline.rs`.

---

## 2. A card grid that was reimplementing the `grid` built-in

A very common Q1 shape: a template that hand-rolls Bootstrap card markup with
the same class names the built-in `grid` layout uses, plus a JavaScript
prologue of layout constants.

**Before** (abridged):

````
```{=html}
<%
const cols = 3;
const align = "left";
const hideBorders = false;
%>
<div class="list grid quarto-listing-cols-<%=cols%>">
<% for (const item of items) { %>
  <div class="g-col-1" <%= metadataAttrs(item) %>>
    <a href="<%= item.exercise %>" class="grid-item-link">
      <div class="quarto-grid-item card h-100 <%-`card-${align}`%><%= hideBorders ? ' borderless' : '' %>">
        <% if (item.image) { %>
          <img src="<%= item.image %>" class="card-img" alt="<%= item['image-alt'] %>">
        <% } %>
        <div class="card-body post-contents">
          <h5 class="card-title listing-title"><%= item.title %></h5>
          <div class="card-text listing-description"><%= item.description %></div>
        </div>
      </div>
    </a>
  </div>
<% } %>
</div>
```
````

**Recognise this before porting it.** If the class names match a built-in
layout, the cheapest correct port is often not a port at all — it is
`type: grid` with the relevant `grid-columns` / `grid-item-align` /
`grid-item-border` options, or a thin template that calls the built-in
partial:

```
::: {.list .grid .quarto-listing-grid .quarto-listing-cols-3}
$items:item-grid()$
:::
```

Port it faithfully only when the template genuinely differs. Then:

**After:**

```
::: {.list .grid .quarto-listing-cols-$listing.template-params.columns$}
$for(items)$
::: {.g-col-1}
::: {.quarto-grid-item .card .h-100 .card-left}
$if(it.image)$
![$it.image-alt$]($it.image$){.card-img}
$endif$

::: {.card-body .post-contents}
##### [$it.title$]($it.exercise$){.no-anchor .card-title .listing-title}

$if(it.description)$
[$it.description$]{.card-text .listing-description}
$endif$
:::
:::
:::

$endfor$
:::
```

with

```yaml
listing:
  type: custom
  template: card.template
  template-params:
    columns: 3
```

The decisions:

- **The JS prologue became `template-params:`.** `cols`, `align` and
  `hideBorders` were per-listing constants that happened to live in the
  template. Doctemplates cannot declare variables, and these were never
  per-item.
- **The ternary and template literal are gone.** `` `card-${align}` `` and
  `hideBorders ? ' borderless' : ''` are expressions. Either fix the class
  (as above) or branch explicitly with `$if(listing.template-params.borderless)$`.
- **The custom field `exercise` is the link target,** so it must be a markdown
  link. Custom-field values are passed through verbatim, so the YAML must
  write them relative to the page declaring the listing.
- **The image became a markdown image,** which is what gets it copied into
  the output tree. A record- or custom-field image referenced only from a raw
  `<img>` is never copied at all.
- **`metadataAttrs(item)` was dropped.** `$it.metadata-attrs$` is the
  equivalent and must go inside a ```` ```{=html} ```` block (interpolated as
  markdown, its quotes are curled into invalid HTML) — but no Q2 built-in
  layout emits it, so dropping it is usually right. Decide, don't translate.

Beware a Q1 wart in templates of this shape: guards like
`<% if ('title' || 'subtitle') { %>` are **constant-true** — they are string
literals, typically left behind when an `otherFields.includes(…)` test was
stripped. Do not faithfully reproduce them. If the intent was "show this field
when the listing asks for it", that is `$if(it.show.title)$` — noting that
`type: custom` has no default field set, so every `show.*` is false unless the
listing declares `fields:` explicitly.

---

## 3. A whole-card link — phrasing content only

The hardest common shape: Q1 templates that wrap an entire card in one anchor,
so the whole card is clickable.

**Before:**

````
```{=html}
<% for (const item of items) { %>
  <a href="<%- item.link %>" class="custom-card-wrapper">
    <div class="custom-card">
      <div class="custom-card-icon"><i class="<%= item.icon %>"></i></div>
      <h3 class="custom-card-title"><%= item.title %></h3>
      <p class="custom-card-description"><%= item.description %></p>
    </div>
  </a>
<% } %>
```
````

The anchor must become a markdown link so `item.link` is rewritten. But a
standalone markdown link is auto-wrapped in `<p>`, and **`<p>` may only
contain phrasing content** — so the card's `<div>`, `<h3>` and `<p>` cannot
survive inside it.

This is not a style preference. Run the invalid nesting through a
spec-compliant HTML5 parser and the tree comes out wrong:

```
<p><a class="wrap"><div class="card"><h3>Title</h3><p>desc</p></div></a></p>

parses as:
  <p>
    <a class=wrap>          <- empty
  <div class=card>          <- SIBLING of the <p>, outside the anchor
    <h3>
      <a class=wrap>        <- anchor reconstructed
    <p>
      <a class=wrap>        <- and again
  <p>
```

The `<p>` is force-closed before the `<div>`, the card is reparented out of
the anchor, and the adoption-agency algorithm re-opens the anchor three
times. The whole-card link is destroyed and replaced by fragments.

**After** — the card body is built from `<span>`s:

```
$for(items)$
[`<span class="custom-card"><span class="custom-card-icon"><i class="$it.icon$"></i></span><span class="custom-card-title" role="heading" aria-level="3">$it.title$</span><span class="custom-card-description">$it.description$</span></span>`{=html}]($it.link$){.custom-card-wrapper}

$endfor$
```

The same markup verified with a parser:

```
<p><a class="wrap"><span class="card"><span class="t">Title</span>…</span></a></p>

parses as written — card inside the anchor.
```

The decisions:

- **`<div>`/`<h3>`/`<p>` became `<span>`.** CSS selects by class, not tag, so
  this is visually a no-op — but only if the stylesheet's selectors are
  class-based. Check for `div.custom-card` or bare-tag rules first, and for
  `display` assumptions (a `<span>` needs `display: block`/`flex` where a
  `<div>` had it for free).
- **The heading's semantics are preserved explicitly** with
  `role="heading" aria-level="3"`, since the `<h3>` is gone. Do not skip
  this; it is the accessibility contract the original `<h3>` carried.
- **The card body stays raw HTML** — that is allowed, because it is link
  *text*. Only the anchor had to be markdown.

If the card contains a genuinely block-level region that cannot be flattened,
the whole-card link is not portable as-is. The honest options are a
title-and-image link pair (what the built-ins do) or a small amount of
site JavaScript — not a raw `<a>`, which silently ships a dead `.qmd` href.

### Related: a per-item id built by slugifying a title

Q1 templates sometimes generated `id="…-<slug(item.title)>"` to wire
`aria-labelledby`. Doctemplates have no slugify pipe and Q2 binds no per-item
slug, so the id cannot be reproduced. Use an id-free association instead —
`aria-label="$it.title$"` on the card — rather than dropping the
accessibility wiring.
