# Observed output at HEAD `0dcd7e83` (2026-08-13)

Both fixtures were rendered with both engines; the `_site` / `_site-q1` trees are
*not* committed (build artifacts). The excerpts below are the inspected output.

Reproduce with:

```bash
cargo run --bin q2 -- render claude-notes/plans/toc-smart-quotes-investigation/repro
cargo run --bin q2 -- render claude-notes/plans/toc-smart-quotes-investigation/markup-probe
# Quarto 1 comparison (run inside the fixture directory):
quarto render --output-dir _site-q1
```

## `repro/` — the strand's case

**q2 @ `0dcd7e83`**, `_site/index.html`:

```html
<nav id="TOC" role="doc-toc" class="toc-active">
<h2 id="toc-title">Table of contents</h2>
<ul>
<li>
<a href="#using-a-volume" class="nav-link" data-scroll-target="#using-a-volume">
Using a raw volume
</a>
</li>
<li>
<a href="#finding-your-repositorys-identifiers" class="nav-link" data-scroll-target="#finding-your-repositorys-identifiers">
Finding your repository’s identifiers
</a>
</li>
<li>
<a href="#whats-in-the-gallery-really" class="nav-link" data-scroll-target="#whats-in-the-gallery-really">
What’s in the Gallery – really
</a>
</li>
</ul>
</nav>
...
<section id="using-a-volume" class="section level2">
<h2>Using a “raw” volume</h2>
```

The heading keeps U+201C/U+201D; the TOC label loses them. Both controls
(`repository’s`, `Gallery – really`) are correct.

**Quarto 1**, `_site-q1/index.html`:

```html
<li><a href="#using-a-raw-volume" id="toc-using-a-raw-volume" class="nav-link active" data-scroll-target="#using-a-raw-volume">Using a “raw” volume</a></li>
<li><a href="#finding-your-repositorys-identifiers" ...>Finding your repository’s identifiers</a></li>
<li><a href="#whats-in-the-gallery-really" ...>What’s in the Gallery – really</a></li>
```

(The `#using-a-raw-volume` vs. `#using-a-volume` id difference is the sibling
strand bd-heading-id-drops-inline-content-fl84n3ql, not this one.)

## `markup-probe/` — does the TOC keep inline markup at all?

Headings: `## Use \`code\` and *em* and **strong**` and
`## Math $x+y$ and a [link](https://example.com)`.

**Quarto 1** keeps the markup:

```html
<li><a href="#use-code-and-em-and-strong" ...>Use <code>code</code> and <em>em</em> and <strong>strong</strong></a></li>
<li><a href="#math-xy-and-a-link" ...>Math <span class="math inline">\(x+y\)</span> and a link</a></li>
```

**q2 @ `0dcd7e83`** flattens it:

```html
<a href="#use-code-and-em-and-strong" class="nav-link" data-scroll-target="#use-code-and-em-and-strong">
Use code and em and strong
</a>
<a href="#math-and-a" class="nav-link" data-scroll-target="#math-and-a">
Math x+y and a link
</a>
```

Two things here:

1. `TocEntry.title` is a `String` by construction, so *all* inline markup is lost
   in q2 TOC entries — the missing quote glyphs are one symptom of that.
2. `#math-and-a` (q2) vs. `#math-xy-and-a-link` (Q1) is the sibling autoid bug
   again, this time swallowing `Math` and `Link` content.

---

# After the fix — q2 @ phase 3 (2026-08-13)

Same invocations as above. All three fixtures re-rendered and inspected.

## `repro/` — the strand's own case

```html
<a href="#using-a-volume" class="nav-link" data-scroll-target="#using-a-volume">
Using a “raw” volume
</a>
<a href="#finding-your-repositorys-identifiers" ...>
Finding your repository’s identifiers
</a>
<a href="#whats-in-the-gallery-really" ...>
What’s in the Gallery – really
</a>
```

Matches Quarto 1. (The `#using-a-volume` id is still wrong — sibling strand
bd-heading-id-drops-inline-content-fl84n3ql, deliberately out of scope.)

## `markup-probe/` — byte-identical to Quarto 1

```html
<a href="#use-code-and-em-and-strong" ...>
Use <code>code</code> and <em>em</em> and <strong>strong</strong>
</a>
<a href="#math-and-a" ...>
Math <span class="math inline">\(x+y\)</span> and a link
</a>
```

Note `and a link`, not `and a <a href=…>link</a>`: links are unwrapped at render
time (`strip_links_and_notes`, mirroring pandoc's `deLink`) because the TOC entry
is itself an `<a>` and anchors cannot nest. Quarto 1 does the same.

## `toc-title-probe/` — the two config sources now agree

```html
<!-- index.html — _quarto.yml: toc-title: "On **this** page" -->
<h2 id="toc-title">On <strong>this</strong> page</h2>

<!-- frontmatter.html — front matter: toc-title: "In *this* document" -->
<h2 id="toc-title">In <em>this</em> document</h2>
```

Before the fix these diverged: the project-config form rendered the asterisks
literally (never markdown-parsed, because `toc-title` was not in
`MARKDOWN_CONFIG_PATHS`), while the front-matter form rendered `On this page`
with the emphasis silently flattened away by `as_plain_text()`. Same YAML, two
different failures — the `InterpretationContext` split.
