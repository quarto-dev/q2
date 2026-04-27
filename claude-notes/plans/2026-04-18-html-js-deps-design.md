# JS Dependency Handling for Quarto 2 HTML Output — Design Outline

Beads: `bd-ulgr` (parent: `bd-imiw`)

**Status:** outline only; not yet scheduled for implementation. This file
exists to capture open questions and known requirements so a future session
can land with full context.

## Motivation

Q2 currently emits correct DOM for Bootstrap-dependent features but ships
zero JavaScript. Directly surfaced symptom (from `bd-imiw`): the navbar's
`Docs` dropdown is visible but does nothing. Latent cost: every future
interactive feature (tabsets, callout collapse, crossref popovers,
dark-mode toggle, search) will re-raise the same problem until we land a
general mechanism.

Q1 addresses this by bundling Bootstrap JS (and other libs) as resources
that get copied alongside the HTML and linked via `<script>` tags injected
by the pipeline. We need a Q2 equivalent that fits Q2's stage/transform
architecture.

## Concrete symptoms to unblock

- Navbar dropdowns open on click (Bootstrap JS).
- Navbar hamburger collapse toggles on narrow widths (Bootstrap JS).
- *Future:* tabset panels, callouts with collapse behavior, search widget,
  dark-mode toggle.

## Known upstream context

- `quarto-sass` already vendors Bootstrap 5.3.1 SCSS via `include_dir!`.
  JS should be version-pinned to the same Bootstrap release.
- `CompileThemeCssStage` currently writes the compiled CSS to a well-known
  artifact path (`DEFAULT_CSS_ARTIFACT_PATH =
  /.quarto/project-artifacts/styles.css`). JS would likely follow the same
  artifact pattern.
- `ApplyTemplateStage` builds a `css` list that the `FULL_HTML_TEMPLATE`
  expands through `$for(css)$ ... $endfor$`. There is already a
  `$for(scripts)$` loop in the template, but nothing populates it. That's
  the natural extension point.
- `ResourceCollectorTransform` exists and is responsible for collecting
  image dependencies today. JS deps could be collected in an analogous
  pass, or by a new stage.
- For WASM / hub-client renders, the runtime's VFS is how resources are
  surfaced to the browser; any solution must work there too.

## Decisions needed

Open questions for the implementation session:

1. **Vendored vs CDN.**
   - Vendored: reproducible, works offline, bigger distribution.
   - CDN: lighter bundle, online-only; Q1 offers a `self-contained` mode
     that matters for some users.
   - Likely answer: vendored by default (matches SCSS strategy), with a
     future knob for CDN.

2. **Who declares a JS dependency?**
   - Option A: theme compilation always emits Bootstrap JS alongside
     Bootstrap CSS. Simple; overshoots for docs with no interactive
     features.
   - Option B: individual features / transforms declare deps
     (`NavbarRenderTransform` → "needs Bootstrap JS"). Collection pass
     dedups.
   - Option C: hybrid — the theme compiles a "base" dep set; features
     opt in to additional ones.
   - Leaning B or C; A is the least-work first step.

3. **Where does the `<script>` tag end up?**
   - Template `$for(scripts)$` loop is the obvious target. Needs a
     matching `scripts` entry in the template context, populated by
     `ApplyTemplateStage` from a resolved list.
   - Some JS wants `defer` / `async`; we'll need per-dep attributes.

4. **How does the pipeline know what was declared?**
   - Likely a new struct on `StageContext` (or `artifacts`) named
     `JsDependencies` or similar, with deduplication by name.
   - Parallels whatever `CompileThemeCssStage` writes today.

5. **WASM / hub-client.** Hub-client renders run in the browser; the
   serving layer needs to surface the JS artifact somewhere the browser
   can load it. Same VFS path pattern as CSS is a safe default.

6. **Placement.** Most Bootstrap JS wants the `<body>` end, not `<head>`.
   Q1 puts the main Bootstrap JS bundle at the end of `<body>`. The
   template currently only has `$for(scripts)$` in the `<head>`; we may
   need a second loop (`$for(body-scripts)$` or `include-after` style).

7. **Which Bootstrap components do we ship?** Full `bootstrap.bundle.min.js`
   (includes Popper for tooltips/popovers) is ~80KB gzipped. Custom
   subsets (only Collapse + Dropdown) are smaller but more fragile to
   maintain as features land. Ship the full bundle initially; optimize
   later if it's a real cost.

## Proposed rough shape (not a commitment)

A future session probably ends up with:

- A new `JsDep` type in `quarto-core` describing a script (id, path
  under artifacts, placement: head-deferred / body-end, optional
  `defer`/`async`).
- A `JsDependencies` collector on `StageContext`, populated by whichever
  transforms opt in.
- A new / extended stage that copies the vendored JS bundles into artifact
  storage (paralleling how `CompileThemeCssStage` emits CSS).
- Template-side: either a second `$for(body-scripts)$` loop or a
  post-body include point; `ApplyTemplateStage` feeds both.
- `NavbarRenderTransform` / `FooterRenderTransform` (and later
  tabset/callout transforms) declare the Bootstrap-JS dep when they emit
  DOM that depends on it.

## Out of scope for this outline

- Specifics of search wiring (separate feature).
- Mermaid / other diagram lib integration.
- MathJax / KaTeX selection.
- `self-contained` / single-file mode (inline JS) — future.
- Version management across user-supplied themes that vendor different
  Bootstrap majors.

## Cross-references

- `bd-imiw` — navbar/footer feature (current session). Functional
  dropdown/collapse behavior is blocked on this issue but navbar DOM and
  static styling are not.
- `bd-ltmn` — `toc: false` parity fix (closed as part of `bd-imiw`).
