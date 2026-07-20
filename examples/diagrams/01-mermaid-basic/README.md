# 01-mermaid-basic — Mermaid diagrams in an HTML document

A `format: html` document with two Mermaid diagrams — a flowchart and a
sequence diagram — plus an ordinary code block for contrast.

## What this demonstrates

- **` ```mermaid ` fenced blocks.** A plain fenced code block whose
  language is `mermaid` renders as a diagram; no front-matter opt-in is
  needed. This is the same syntax GitHub renders in its markdown preview.
- **Browser-side rendering.** The rendered page carries the diagram
  source in a `<pre class="mermaid">` element plus one script that loads
  mermaid.js from the jsdelivr CDN and draws every diagram at page load.
- **Ordinary code blocks are untouched.** The `python` block renders as
  highlighted code, with none of the diagram machinery attached.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/diagrams/01-mermaid-basic
```

The page is written next to the source as `document.html`.

## What to look for

- Two `<pre class="mermaid">` elements holding the escaped diagram
  source, replaced by SVG drawings when the page loads in a browser.
- Exactly one `<script type="module">` near `</body>` importing
  `mermaid@…` from `cdn.jsdelivr.net` — the script appears once no
  matter how many diagrams the page has.
- Viewing the page needs network access (the diagram library comes from
  the CDN); without it the diagram source is shown as preformatted text.
