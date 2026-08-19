# Investigation results — bd-sidebar-dir-index-md-5khf3lds

**Date:** 2026-08-19, at `main` @ `f387bd68` (v0.24.0).

## Invocation

```
cargo run --bin q2 -- render claude-notes/plans/sidebar-dir-index-md-investigation/repro
# Rendered 4 of 4 files to .../repro/_site
```

Then inspected `_site/guides/alpha.html`'s `#quarto-sidebar`.

## Observed (HEAD, buggy)

```html
<a class="sidebar-item-text sidebar-link text-start" data-bs-toggle="collapse"
   data-bs-target="#quarto-sidebar-section-0" ...>Guides</a>
...
<ul id="quarto-sidebar-section-0" class="collapse list-unstyled sidebar-section depth1 show">
  <li>... <a href="alpha.html">Alpha Guide</a> ...</li>
  <li>... <a href="beta.html">Beta Guide</a> ...</li>
  <li>... <a href="index.html">The Guides Landing Page</a> ...</li>  <!-- BUG: should be the header -->
</ul>
```

Both symptoms in one render:

1. Section header text is the capitalized directory name **"Guides"** with **no href**,
   instead of the landing page's title "The Guides Landing Page" linking to `guides/index.html`.
2. `guides/index.md` is **not excluded** from the child list — it appears as a third sibling,
   which also shifts prev/next pagination (the landing page becomes an entry in the page order).

## Expected (Q1 behavior, and q2's own behavior when the file is `index.qmd`)

Section header "The Guides Landing Page" with `href="index.html"`, children exactly
Alpha Guide and Beta Guide. Per the strand: renaming `guides/index.md` → `index.qmd`
makes q2 match Q1 exactly (verified in the external repro at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/sidebar-dir-shorthand-index-page/`).

## Root cause (confirmed)

`crates/quarto-core/src/transforms/sidebar_auto.rs:356`:

```rust
let index_src = format!("{}/index.qmd", dir);
```

drives both the header lookup (`index.lookup_by_source`, line 357) and the child-exclusion
filter (line 383). `.md` inputs are first-class in discovery
(`FIXED_RENDERABLE = &["qmd", "md"]`, `crates/quarto-core/src/project/discovery.rs`),
so the "only .qmd is discoverable" MVP comment is stale.

## Repro contents

`repro/`: website project, `sidebar.contents: guides`, `project.render` includes `**/*.md`;
`guides/index.md` (title "The Guides Landing Page"), `guides/alpha.qmd`, `guides/beta.qmd`.
`_site/` output is gitignored; re-render to regenerate.
