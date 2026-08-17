# Repro: website breadcrumbs

E2E fixture for bd-breadcrumbs-missing-1vpuqh34. Render with:

```bash
cargo run --bin q2 -- render claude-notes/plans/index-miss-relativize-investigation/breadcrumbs
```

then inspect `_site/guide/advanced/deep.html`. Expected (title-block
instance, Q1 parity):

```html
<nav class="quarto-page-breadcrumbs quarto-title-breadcrumbs d-none d-lg-block" aria-label="breadcrumb">
  <ol class="breadcrumb">
    <li class="breadcrumb-item"><a href="../intro.html">Guide</a></li>
    <li class="breadcrumb-item"><a href="deep.html">Advanced</a></li>
    <li class="breadcrumb-item"><a href="deep.html">Deep Page</a></li>
  </ol>
</nav>
```

Behaviors on display: the section crumbs borrow their first direct
child's href (Guide → intro, Advanced → deep), hrefs are
page-relativized, and the current page is its own final linked crumb.
`_site/index.html` has no trail (length-1). Before the fix q2 emitted
no breadcrumb markup anywhere.

The narrow-viewport `.quarto-secondary-nav` instance is not part of
this fixture — see bd-26bf3j1y.
