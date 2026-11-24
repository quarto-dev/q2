# Issue #1297 — nav landmarks have no names; sidebar is a second `doc-toc`

- **GitHub**: https://github.com/quarto-dev/q2/issues/1297
- **Reporter**: cscheid (Carlos Scheidegger), 2026-11-24, on behalf of a downstream Posit documentation site
- **Triage date**: 2026-11-24
- **Branch**: `issue-1297` (topic branch in the main checkout, not a worktree, at the user's request), based on `main` @ `5eb36805b`
- **Braid**: none new. The issue duplicates two existing strands: bd-erecx4j8 (landmark labels) and bd-eczdzfqo (sidebar `doc-toc` role). See Outcome.
- **Scope**: the whole issue. It is one report with two parts (unnamed landmarks, extra `doc-toc` role), and both parts have one fix site.

## Summary

Every `<nav>` landmark on a q2 website page except the breadcrumbs is
unnamed, and `nav#quarto-sidebar` carries `role="doc-toc"`. A page
therefore has two table-of-contents landmarks and four unnamed
"navigation" landmarks. Reproduced at `main`. Quarto 1 renders the same
sources with the labels "Site", "Toolbar", "Breadcrumbs", "Section" and
"Page", gives only `#TOC` the `doc-toc` role, and names `#TOC` with
`aria-labelledby="toc-title"`. The report is accurate, with two
corrections to its proposed fix:

1. Q1's labels are **localized** language terms
   (`navigation-{site,section,toolbar,page,breadcrumbs}-label`), not
   English literals. q2 already vendors those terms in every
   `resources/language/_language-*.yml`, so the port reads them the way
   `secondary_nav_render.rs` already reads `toggle-sidebar`.
2. The sidebar's label **depends on whether the page has a navbar**.
   Q1's `sidebar.ejs:1` uses "Section" with a navbar and "Site" without
   one, because the sidebar is then the site's primary navigation. The
   issue proposes "Section" unconditionally. That label would misdescribe
   sidebar-only sites.

The work is already filed. The issue duplicates bd-erecx4j8 (labels) and
bd-eczdzfqo (sidebar role), which should land together.

## Reproduction

Fixtures:

- `repro-site/`: a website with a navbar, a sidebar with a section,
  `toc: true` and `page-navigation: true`.
- `exp-sidebar-only/`: the same site without a navbar, to show the
  Site/Section switch.
- `list-navs.py`: prints the landmark attributes of every `<nav>`.

```bash
cd claude-notes/issue-reports/1297
q2 render repro-site
q2 render exp-sidebar-only
python3 -I list-navs.py repro-site/_site/guide/one.html exp-sidebar-only/_site/guide/one.html
```

q2 at `main` (`navs-q2.txt`), `repro-site`:

```
tag  id              class                                     role     aria-label   aria-labelledby
nav  -               navbar navbar-expand-lg                   -        -            -
nav  -               quarto-secondary-nav                      -        -            -
nav  -               quarto-page-breadcrumbs                   -        breadcrumb   -
nav  quarto-sidebar  sidebar collapse collapse-horizontal qua  doc-toc  -            -
nav  TOC             toc-active                                doc-toc  -            -
nav  -               quarto-page-breadcrumbs quarto-title-bre  -        breadcrumb   -
nav  -               page-navigation                           -        -            -
   role=doc-toc elements: 2
```

Quarto 1 (dev build, `external-sources/quarto-cli` @ `cda410214`) on a
copy of the same fixtures (`navs-q1.txt`, `quarto-required` removed so
that Q1 accepts them), `repro-site`:

```
tag  id              class                                     role     aria-label   aria-labelledby
nav  -               navbar navbar-expand-lg                   -        Site         -
nav  -               quarto-secondary-nav                      -        Toolbar      -
nav  -               quarto-page-breadcrumbs                   -        Breadcrumbs  -
nav  quarto-sidebar  sidebar collapse collapse-horizontal qua  -        Section      -
nav  TOC             toc-active                                doc-toc  -            toc-title
nav  -               quarto-page-breadcrumbs quarto-title-bre  -        Breadcrumbs  -
nav  -               page-navigation                           -        Page         -
   role=doc-toc elements: 1
```

In `exp-sidebar-only` with Q1, the sidebar row reads `Site` instead of
`Section`. The q2 output for that variant is unchanged apart from the
missing navbar row. I inspected both outputs.

## Localization

Landmark emitters, all in `crates/quarto-navigation/src/render_html.rs`:

| Region | Emitter | q2 today | Q1 |
|---|---|---|---|
| Navbar | `navbar_to_html`, L77 | unnamed | `navigation-site-label` (`nav-before-body.ejs:22`) |
| Secondary nav | `secondary_nav_to_html`, L449 | unnamed | `navigation-toolbar-label` (`nav-before-body.ejs:64`) |
| Breadcrumbs | `breadcrumbs_to_html`, L369 | literal `breadcrumb` | `navigation-breadcrumbs-label`, default "Breadcrumbs" (`website-navigation.ts:927`) |
| Sidebar | `sidebar_to_html_with_options`, L607 | `role="doc-toc"`, unnamed | navbar ? `navigation-section-label` : `navigation-site-label` (`sidebar.ejs:1`) |
| Page nav | `page_navigation_to_html`, L732 | unnamed | `navigation-page-label` (`nav-after-body-postamble.ejs:4`) |

The callers live in `crates/quarto-core/src/transforms/`:
`navbar_render.rs:150`, `sidebar_render.rs:201` and `:308`,
`secondary_nav_render.rs:151–164`, `breadcrumbs_render.rs` and the
page-nav transform. Model to copy: `secondary_nav_render.rs:115–117`
reads `toggle-sidebar` through `LanguageTerms::from_meta`, with an
English fallback. The labels must be passed into the emitters as
parameters, as `secondary_nav_to_html` already takes `toggle_label`.
The `quarto-navigation` emitters cannot see document metadata.

The `<nav id="TOC" role="doc-toc">` emitters, which all lack
`aria-labelledby`:

- `crates/quarto-core/src/template.rs:828` (`TOC_BLOCK_PARTIAL`, emits
  `<h2 id="toc-title">` at L830)
- `crates/quarto-core/src/transforms/sidebar_render.rs:234`
  (`toc_block_html`, for a TOC merged into the left sidebar)
- `crates/pampa/resources/templates/html/main.html:58` (pampa's
  standalone template; Q1's `toc.html:1` is the model:
  `$if(toc-title)$ aria-labelledby="$idprefix$toc-title"$endif$`)
- `ts-packages/preview-renderer/src/q2-preview/chromeSlots.tsx:206`
  (preview `TocSlot`; `titleHtml` decides whether the `<h2>` exists)

`crates/quarto-core/src/revealjs/toc.rs:188` also emits
`<nav role="doc-toc">`. It belongs to the revealjs TOC slide and is out
of scope.

The landmark language terms are already vendored: `resources/language/_language.yml:96–100`
and every translated `_language-*.yml` carry the five
`navigation-*-label` keys, so the port needs no language-file changes.

## Open questions — resolved during triage

**Does any CSS or JS rely on `#quarto-sidebar[role="doc-toc"]`?**
I read every `doc-toc` selector, listed by
`grep -rn doc-toc resources/scss resources/js ts-packages/preview-renderer/src`:

- `_bootstrap-rules.scss:2179–2305`, the `.sidebar nav[role="doc-toc"]`
  rules. They are descendant selectors: they need a `nav` *inside* a
  `.sidebar`. `#quarto-sidebar` is itself the `.sidebar` and has no
  `.sidebar` ancestor, so these rules match only `#TOC`, inside
  `#quarto-margin-sidebar.sidebar` or merged into `#quarto-sidebar`.
  Removing the role leaves them unaffected.
- `_bootstrap-rules.scss:297`, `nav#TOC[role="doc-toc"]` (narrow-screen
  hiding). It is already scoped to `#TOC` because of this very overlap
  (see the comment at L286, which cites bd-eczdzfqo). Unaffected.
- `_bootstrap-rules.scss:2839–2851`, the unscoped `nav[role="doc-toc"]`
  rules: `z-index: $zindex-sticky` (1020) and the opacity/border
  transitions. These do match the sidebar today. The transitions are
  duplicated by explicit `#quarto-sidebar > *` selectors, so nothing is
  lost there. The **z-index is lost** on the desktop sidebar. Q1's
  `_bootstrap-rules.scss:1927` has the same rule and Q1's sidebar has no
  role, so removing it is parity with Q1. The narrow-viewport drawer
  keeps its own `z-index: 1000` (`.sidebar.sidebar-navigation.show`,
  L741–744). Still, check visually that nothing overlaps the sticky
  desktop sidebar after the change. Headroom navbar and dropdowns are
  the likely candidates.
- `resources/js/quarto-toc/quarto-toc.js:68`,
  `nav.toc-active[role="doc-toc"]`. The sidebar has no `toc-active`
  class, so this already matches only `#TOC`. Unaffected; scrollspy keeps
  working.
- `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts:332`,
  `nav[role="doc-toc"]:not(.sidebar-navigation)`. The exclusion exists
  only because of the sidebar's role (bd-f5yi). After the fix it is dead
  but harmless. Simplify it and its comment (L326–331) in the same
  change.
- `crates/quarto-core/src/transforms/repo_actions_render.rs:117`. The
  comment says "exactly one `nav[role=doc-toc]` is emitted per page",
  which is false today and becomes true after the fix. No code change
  needed.

**The page has two breadcrumb `<nav>`s with the same label. Does that
also fail `landmark-unique`?** Q1 emits both as well
(`makeBreadCrumbs` is called twice: one in the secondary nav, one in the
title block with `d-none d-lg-block`), and both carry "Breadcrumbs". They
are responsive duplicates. The secondary nav is hidden at `lg` and up,
and the title copy is hidden below `lg`, so a given viewport exposes only
one. axe skips hidden landmarks. Matching Q1 is enough here; this needs
no extra work.

**Does preview need its own fix?** Only for the TOC. Preview's navbar,
sidebar, breadcrumbs, secondary-nav and page-nav HTML are Rust fragments
from the same transforms, injected opaquely by `chromeSlots.tsx`. They
pick up the fix after the WASM rebuild chain in AGENTS.md § "Verifying
Rust changes in `q2 preview`". The TOC wrapper is the exception: it is
hand-written JSX in `TocSlot` and needs its own `aria-labelledby`.

## Outcome / recommended next step

**Duplicate of existing strands; no new strand filed.**

- bd-erecx4j8 (P2, open): "give every website/book nav landmark a
  distinct, localizable aria-label (Q1 #14813 parity)". It covers the
  labels, including the TOC and breadcrumbs.
- bd-eczdzfqo (P3, open): "nav#quarto-sidebar carries role=doc-toc". It
  covers the role removal and asks for exactly the selector audit done
  above.

The two strands should land as one change. bd-erecx4j8 replaces the
role with a label, which is what bd-eczdzfqo suggests. During triage I
linked them as `related` and commented on each with a pointer to this
doc and to GH #1297.

Fix scope, for whoever picks it up:

1. Thread the localized labels into the five `render_html.rs` emitters
   (parameters, as `toggle_label` already is), with the Site/Section
   switch on navbar presence for the sidebar. The English fallbacks
   match `_language.yml`.
2. Drop `role="doc-toc"` from `#quarto-sidebar`.
3. Add `aria-labelledby="toc-title"` to the four `#TOC` emitters when
   the title `<h2>` is emitted. Use `$idprefix$toc-title` in pampa's
   template.
4. Simplify `iframePostProcessor.ts:332`, and update the SCSS comment at
   L286 that refers to bd-eczdzfqo.
5. Tests first (AGENTS.md TDD): string tests in `render_html.rs` for each
   label, including a non-English language and the navbar-less sidebar;
   a website-level test through `render_document_to_file` that asserts
   exactly one `role="doc-toc"` and named landmarks; then update the
   existing assertions (`render_html.rs:3022`, `:3705`, `:3832`, `:3982`;
   `sidebar_render.rs:446`, `:456`; and the `quarto-core`/`quarto`
   integration snapshots that contain these navs).
6. End-to-end: re-run `list-navs.py` on both fixtures and expect
   `navs-q1.txt`, check scrollspy in `q2 render` and `q2 preview`, and
   run axe `landmark-unique` on `repro-site` in a browser.

## Verification commands used

Pre-flight: `cargo xtask verify --skip-hub-build` at `5eb36805b`
passed all Rust legs (20430 tests). It then stopped at the
`editors/vscode` typecheck because the local `node_modules` predated
#1255 and lacked `@types/vscode`. After `npm install`, a re-run with
`--skip-rust-tests` passed. One run of that re-run hit a timing flake in
`quarto-sync-client`'s `file-collision-recovery.test.ts`, which passed
3/3 in isolation and on the next full run.

```bash
gh issue view 1297 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
braid show bd-erecx4j8; braid show bd-eczdzfqo; braid search aria; braid search landmark
q2 render claude-notes/issue-reports/1297/repro-site     # target/debug/q2 at 5eb36805b
q2 render claude-notes/issue-reports/1297/exp-sidebar-only
python3 -I claude-notes/issue-reports/1297/list-navs.py <html>...
quarto render repro-site; quarto render exp-sidebar-only # Q1 dev build, on a scratch copy
grep -rn doc-toc resources/scss resources/js ts-packages/preview-renderer/src crates/quarto-core/src crates/pampa/resources
grep -n 'navigation-.*-label' resources/language/_language.yml
```

## Cross-references

- bd-erecx4j8: landmark labels (Q1 #14813 parity); main work item
- bd-eczdzfqo: sidebar `doc-toc` role; land together with the above
- bd-3bg8h1rx: sidebar section toggles as buttons (the a11y work that
  discovered bd-erecx4j8)
- bd-f5yi: the preview exclusion in `iframePostProcessor.ts`
- `.claude/rules/cross-platform.md`, `claude-notes/instructions/testing.md`
- AGENTS.md § "Verifying Rust changes in `q2 preview`" (WASM rebuild chain)
