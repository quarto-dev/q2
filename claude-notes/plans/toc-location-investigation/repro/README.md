# toc-location: left repro (bd-e2kpwy7n)

Local copy of the committed external repro from the Connect-docs port
(`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/toc-location-left/`).

Run:

```bash
cargo run --bin q2 -- render .
```

Observed at HEAD (2026-08-14, main after PR #530):

- `grep -c 'id="quarto-sidebar"' _site/index.html` → `0`
- `nav#TOC` renders inside `<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">` (the right margin)

Expected (Q1 1.10.15, same input — see `_site-q1/` in the external repro):

- `nav#TOC` inside `<nav id="quarto-sidebar" class="sidebar collapse collapse-horizontal quarto-sidebar-collapse-item sidebar-navigation floating overflow-auto">`
- empty `<div id="quarto-margin-sidebar" class="sidebar margin-sidebar zindex-bottom">`
- `<body class="floating quarto-light">`

`_site/` is gitignored (generated).

**Status:** fixed on branch `braid/bd-e2kpwy7n-toc-location`
(2026-08-14) — q2 now renders `nav#TOC` inside a synthesized
`nav#quarto-sidebar … sidebar-floating` with `body.floating`, matching
the Q1 mechanism with q2's own class idiom. The "Observed at HEAD"
section above records the pre-fix behavior.
