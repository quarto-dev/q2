# repo-actions footer copy ships no CSS

**Strand:** bd-repo-actions-footer-unstyled-80xtt35y
**Discovered from:** bd-repo-actions-missing-99ezd2fe (closed — shipped in 0.27.0)
**Branch:** `braid/bd-repo-actions-footer-unstyled-80xtt35y-repo-actions-footer-unstyled`

## Overview

repo-actions emits two copies of the `.toc-actions` block: one in the TOC
sidebar, one in the page footer. The markup for both is byte-for-byte Q1's.
The *footer* copy, however, inherits no styles — every `.toc-actions` rule
q2 ships is scoped under `.sidebar` — so those links render as a
browser-default bulleted list instead of Q1's centred horizontal row.

The missing source is one self-contained block in Q1's
`src/resources/projects/website/navigation/quarto-nav.scss:770-802`, inside
that file's `/*-- scss:rules --*/` layer. **No SCSS variables are involved** —
every value is a literal, so it ports verbatim with nothing lost.

### Why this is not a dedup opportunity

q2 already has `.toc-actions` rules at
`resources/scss/bootstrap/_bootstrap-rules.scss:1816-1948`, but they are all
`.sidebar .toc-actions` and are variable-driven/themed. The sidebar and footer
treatments are **deliberately different in kind** — a vertical themed list vs.
a centred flex row. They must not be unified.

### Scope is exactly these rules

Checked before starting, so the fix does not land in a broken shell: q2 already
has the surrounding three-column footer layout. Rendered-CSS selector counts,
q2 vs Q1: `.nav-footer-center` 8 vs 8, `.footer-items` 7 vs 7. The one
difference, `.nav-footer-contents` (0 vs 1), appears in zero HTML files in
either engine and is inert. The `d-sm-block d-md-none` responsive behaviour is
stock Bootstrap utility classes, not Quarto CSS.

## Work items

- [x] Reproduce: confirm the markup/CSS asymmetry at current `main`, not just
      at the 0.27.0 release build
- [x] Extend the `repo_actions_pipeline` harness to expose the `_site` root so
      a test can read the compiled theme CSS
- [x] **Failing test first**: `footer_repo_actions_ship_their_css` asserts each
      of the eight rules reaches the rendered CSS; a second test asserts the
      sidebar rules are undisturbed and that no footer rule leaks into `.sidebar`
- [x] Verify the tests fail at HEAD for the right reason
- [x] Port `quarto-nav.scss:770-802` verbatim into the page-footer section of
      `resources/scss/bootstrap/_bootstrap-rules.scss`
- [x] Verify the tests pass
- [x] End-to-end: render the strand's repro website with `q2 render` and inspect
      the emitted CSS and the rendered page
- [x] Workspace verify (`cargo xtask verify --skip-hub-build --skip-hub-tests`)
- [x] Re-capture the `phase5-single-doc-baseline` styles.css hash (see below)

## Test strategy

The defect is invisible to markup assertions — the existing suite passes while
the bug ships, which is precisely how it escaped. The regression test therefore
reads the *compiled CSS* out of a real rendered `_site`, parses the rule blocks
whose selector mentions both `.nav-footer` and `toc-action`, and asserts on the
declarations. Matching is structural (`selector{decls}`) rather than on
formatting, because the emitted CSS is minified.

## End-to-end verification (2026-08-25)

Rendered a two-page website fixture with `repo-actions: [edit, source, issue]`
through the real binary:

    cargo run --bin q2 -- render <fixture>
    Rendered 2 of 2 files to <fixture>/_site

The strand's own repro measurement, run against that output:

    toc-action links             6      (was 6 — markup was never the problem)
    toc-actions containers       2      (was 2)
    .nav-footer …toc-action rules  8    (was 0)

The eight emitted rules were diffed against Q1's compiled
`bootstrap-*.min.css` from the strand's repro and are **byte-for-byte
identical**:

    .nav-footer .toc-actions a,.nav-footer .toc-actions a:hover{text-decoration:none}
    .nav-footer .toc-actions ul :first-child{margin-left:auto}
    .nav-footer .toc-actions ul :last-child{margin-right:auto}
    .nav-footer .toc-actions ul li i.bi{padding-right:.4em}
    .nav-footer .toc-actions ul li:last-of-type{padding-right:0}
    .nav-footer .toc-actions ul li{padding-right:1.5em}
    .nav-footer .toc-actions ul{display:flex;list-style:none}
    .nav-footer .toc-actions{padding-bottom:.5em;padding-top:.5em}

The rendered footer markup was inspected and every selector matches it:
`.nav-footer > .nav-footer-center > .toc-actions.d-sm-block.d-md-none > ul >
li > a.toc-action > i.bi`.

No regression to the sidebar treatment: 9 `.sidebar …toc-actions` rules are
still emitted, 0 of the new footer rules mention `.sidebar`, and the TOC copy's
container is still present in the HTML.

## Pinned-hash re-capture

`artifact_scoping_pipeline::single_doc_render_unchanged_under_scope_refactor`
pins a sha256 of the compiled `doc_files/styles.css` against
`tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`. Adding rules
to the SCSS necessarily shifts it, so the baseline was re-captured with a
documented note, per that file's standing convention for every prior SCSS port:

    doc_files/styles.css  60291dc1…  ->  a184a291…

`doc.html` is **unchanged**, and that was confirmed empirically rather than
assumed: it is the first entry the test checks, and it passed before the
styles.css assertion fired. A single-doc render has no footer at all, so the
new selectors match nothing in the fixture.

Note on attribution: the first post-fix verify reported all tests passing, which
was misleading — SCSS is embedded via `include_dir!` at compile time, and that
run's test binary had been built before the SCSS edit. Only the second run,
which recompiled, surfaced the hash shift.

## Final verification

`cargo xtask verify --skip-hub-build --skip-hub-tests` — **all 14 steps green**,
13426 tests passed, 199 skipped, custom lints + clippy clean under `-D warnings`.

