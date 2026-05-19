# Attribution: auto-inject viewer CSS/JS

## Overview

When a user runs `quarto render --attribution=git` (or sets `attribution: git`
in YAML), Quarto emits per-node `data-attr-*` attributes on wrapping `<span>`s.
Those attributes are inert: without CSS and JS to react to them, the rendered
page is visually identical to one rendered without `--attribution=git`. The
feature feels broken unless the user also copy-pastes the ~70-line snippet
currently documented under "Adding a viewer overlay" in
`docs/authoring/attribution.qmd`.

This plan replaces that copy-paste section with automatic injection of a small
CSS + JS pair into the rendered HTML whenever attribution is active for an
HTML render. The defaults are deliberately conservative — a dotted underline
on attributed text and a hover badge — so the feature is discoverable without
overriding theme-set body colours.

## Decisions pinned before implementation

- **Auto-inject by default; opt out via YAML.** When effective attribution
  mode is `git` and output is HTML, CSS + JS ship automatically. The opt-out
  is the rich YAML form: `attribution: { source: git, viewer: false }`.
  No new CLI flag — the use case for "data attributes but no presentation" is
  rare and the YAML knob covers it.
- **Neutral by default.** The injected JS does **not** repaint each wrapped
  element in its author's colour (the doc snippet does that today, at
  `attribution.qmd:200-206`). The wrapper inherits whatever colour the host
  theme assigns; only the hover badge is author-coloured. This minimizes
  visual interference with site themes.
- **Inline `<style>` and `<script>`, not external files.** Total payload
  ~2 KB; inlining keeps single-file HTML output single-file and avoids
  `resources:` ceremony.
- **Asset source of truth: two compile-time files** under repo-root
  `resources/attribution/` (`viewer.css`, `viewer.js`), loaded from
  `crates/quarto-core` via `include_str!("../../../../resources/attribution/viewer.css")`.
  Matches the existing `resources/scss/` precedent (CLAUDE.md "External Sources
  Policy" → "Current Local Resource Directories"), keeps the asset accessible
  to both Rust and hub-client without crossing crate-internal paths, and
  removes the need to widen Vite's `server.fs.allow` (the file would otherwise
  sit outside hub-client's project root).
- **Injection mechanism: new AST transform** (`AttributionViewerTransform`)
  registered alongside `AttributionRenderTransform`. Follows the
  `WebsiteFaviconTransform` precedent: append HTML literals to
  `rendered.includes.header` and `rendered.includes.after-body`. The HTML
  template already wires those slots into `<head>` / before-`</body>`.
  Kept as a separate transform from `AttributionRenderTransform` (rather
  than folded in) to preserve one-concern-per-transform — wrapping inline
  AST nodes and appending include literals are distinct jobs, matching
  the `crossref_resolve`/`crossref_render` and
  `sidebar_generate`/`sidebar_render` precedents. CLI-only by design: the
  hub-client doesn't go through the template's `rendered.includes.*`
  slots — it renders React components and binds events on props, so the
  transform has nothing to do there. Only the CSS asset crosses the
  surface boundary (see next section).
- **Gating:** the transform runs only when the same condition that produced
  wrappers is true (`AttributionRenderTransform` populated
  `ctx.format_options.html.attribution_by_node`, i.e. its value is `Some(_)`)
  **AND** the new `ctx.format_options.html.attribution_viewer_enabled` boolean
  is `true` (default; flipped to `false` only by `attribution: { source: git,
  viewer: false }` in YAML). The `Some(_)` check covers both the "matches
  exist" and "attribution on but doc has no matches yet" cases — we deliberately
  inject in the latter so the feature feels alive on documents the author has
  just started. This makes "render-transform ran ⇔ assets emitted (unless
  opted out)" an invariant — there is no configuration where the CSS is
  injected without the wrapping infrastructure also running, or vice versa.

  **CLI-only by design — q2-preview exclusion:** because new transforms
  default to *included* in the q2-preview pipeline (per the deny-list
  inversion in `build_q2_preview_transform_pipeline`), the
  `"attribution-viewer"` name MUST be added to `Q2_PREVIEW_TRANSFORM_EXCLUDED`
  (pipeline.rs:1102-1125) alongside the existing `"website-favicon"` entry.
  Hub-client ignores `rendered.includes.*` so the absence is invisible to
  it; the exclusion enforces the design statement rather than relying on
  surface-level no-op.

## Hub-client shares the same CSS asset

The same CSS that the CLI auto-injects is consumed by the hub-client. Today
`hub-client/src/components/render/framework/attribution.tsx` (line 54,
`attributionStyles` const) hard-codes a near-identical copy. After this plan,
the hub-client imports the shared file via Vite's `?raw` mechanism — already
used elsewhere in the project for `changelog.md?raw` and `more-info.md?raw`
(see `hub-client/src/components/tabs/AboutTab.tsx:12-13`), and supported
by the existing `vite/client` types reference in `hub-client/src/vite-env.d.ts`.
No type-declaration changes needed.

**Important — file lives outside hub-client's Vite root.** With the asset at
repo-root `resources/attribution/viewer.css`, the relative import from
`hub-client/src/components/render/framework/attribution.tsx` is roughly
`../../../../../resources/attribution/viewer.css?raw` (five `..` segments).
Vite's `server.fs.strict` is `true` by default, so the parent path must be
whitelisted: add `server.fs.allow: ['..']` (or the explicit
`resolve(__dirname, '../resources')`) to `hub-client/vite.config.ts`. Both
`vite dev` and `vite build` need this — the `npm run build:all` check in
Phase D catches the production-build path. Confirmed: the precedents
(`changelog.md?raw`, `more-info.md?raw`) all live inside hub-client's tree,
so they're not evidence that out-of-tree `?raw` works without config tweaks.

**Only the CSS is shared, not the JS.** The CLI ships raw DOM listeners
(`document.addEventListener('mouseover', …)`) because the rendered output is
static HTML with no React runtime. The hub-client uses a React hook
(`useAttributionHover`) with `onMouseOver` / `onMouseOut` event props on
component boundaries. The two event-handling models can't share code; only
the visual presentation (badge classes) is genuinely common.

**Behavioural convergence with the CLI.** Today hub-client's
`AttributionWrap` sets `style={{ color: attribution.color }}` on each
wrapper (attribution.tsx line 114), repainting body text in the author's
colour. This plan **drops that inline style** so hub-client matches the
CLI's "neutral text, author colour only on the hover badge" default. The
behaviour becomes uniform across both surfaces: attributed regions are
underlined dotted; hover reveals an author-coloured badge; body text
stays theme-controlled. The badge itself still uses
`record.color` via `AttributionBadge` — only the wrapper-level repaint
goes away.

This is a visible change for current hub-client users — call it out in
the commit message and the hub-client changelog entry.

## Out of scope (deferred to a follow-up beads issue)

- **Theming knobs.** No `viewer: { color: ..., underline-style: ... }` map.
  Authors who want to customize override the four CSS rules in their own
  theme; the `data-attr-*` attributes remain the stable contract.
- **Non-HTML formats.** PDF / docx / etc. get no viewer injection (they get
  no wrappers either, today). No work needed; this falls out of the gating.

## Test plan (write tests first — TDD)

All tests live under `crates/quarto-core/tests/` unless otherwise noted.
Each test below should be written and observed to fail before any
implementation lands.

### Phase A: transform-level tests

- [ ] `attribution_viewer_emits_includes_when_active`:
      build a `RenderContext` with `format_options.html.attribution_by_node =
      Some(...)` and `format_options.html.attribution_viewer_enabled = true`;
      run the transform; assert `ast.meta.rendered.includes.header` contains
      a `<style>` block matching `q2-attr-badge` and
      `rendered.includes.after-body` contains a `<script>` block matching
      `data-attr-actor`. Mirrors `website_favicon.rs` test shape.
- [ ] `attribution_viewer_skips_when_attribution_off`:
      `format_options.html.attribution_by_node = None` (the unflagged path)
      → the include arrays are unchanged (no `<style>` / `<script>` added).
- [ ] `attribution_viewer_skips_when_viewer_disabled`:
      `attribution_by_node = Some(...)` (render transform ran) but
      `attribution_viewer_enabled = false` (YAML opt-out) → the include
      arrays are unchanged. Pins the YAML opt-out path.
- [ ] `attribution_viewer_emits_when_no_matches`:
      `attribution_by_node = Some(empty HashMap)` (attribution on but
      no body nodes matched runs) → CSS/JS still injected. Pins the
      "feature feels alive on empty docs" decision against future
      "only inject when non-empty" drift.
- [ ] `attribution_viewer_idempotent_on_rerun`:
      running the transform twice on the same `ast.meta` does **not**
      double-inject. Requires the transform body to check for the
      sentinel comment (`<!-- quarto-attribution-viewer-css -->`) in
      the existing `rendered.includes.header` strings before appending;
      Phase 3 spells out this dedup logic.

### Phase B: end-to-end render tests

- [ ] `attribution_cli_e2e_viewer_default_on`:
      extend `crates/quarto/tests/attribution_cli_e2e.rs` — render a small
      fixture with `--attribution=git`, grep the produced HTML for
      `q2-attr-badge` (CSS class) and `data-attr-actor` references inside a
      `<script>` block. Both must be present.
- [ ] `attribution_cli_e2e_viewer_opt_out`:
      same fixture with YAML `attribution: { source: git, viewer: false }` and
      no CLI override → wrappers present, no `q2-attr-badge` substring.
- [ ] `attribution_cli_e2e_off_byte_identical`:
      render twice with `attribution: off` (or unset) and confirm output is
      byte-identical to a baseline snapshot — no incidental whitespace from
      the new transform leaking through the off path.
- [ ] Snapshot survey before implementation: run
      `rg -l 'q2-attr-badge|attribution-viewer' crates/*/tests/snapshots`
      to enumerate existing attribution snapshots. **Expect** the on-path
      ones to grow the `<style>`/`<script>` block; off-path ones to be
      byte-identical. If no on-path snapshot exists today, this item
      becomes "add an on-path snapshot capturing the new injection";
      otherwise it's "update and document the diff in the commit message
      per the CLAUDE.md snapshot policy".

### Phase C: asset-content invariants

- [ ] Compile-time check: `include_str!` of `viewer.css` and `viewer.js`
      compiles (no rename / file-removed regression).
- [ ] `viewer_js_does_not_recolor_body`: the embedded `viewer.js` string does
      not contain `el.style.color = ` against a wrapper — pin the
      "neutral by default" decision against future drift. The only color
      assignment in the file should be against the floating badge.
- [ ] `viewer_css_matches_hub_client_classes`: the embedded `viewer.css`
      mentions `q2-attr-badge`, `q2-attr-badge-dot`, `q2-attr-badge-time` —
      pin the shared-class-name contract with hub-client.

### Phase D: hub-client tests against the shared asset + neutral wrapper

All tests in `hub-client/src/components/render/framework/`.

- [ ] `attribution_styles_matches_shared_file`: import the shared CSS via
      `?raw` in a new unit test and assert `attributionStyles` exports it
      verbatim. Guards against a future refactor that silently breaks the
      import while leaving an old hard-coded fallback in place.
- [ ] `attribution_wrap_does_not_recolor_body`: render an `AttributionWrap`
      with a populated lookup and assert the resulting element has **no**
      inline `color` style. Pins the neutral default against re-introduction
      of the line-114 repaint. The badge (`AttributionBadge`) is unaffected
      and continues to render `record.color`.
- [ ] Existing `attribution.integration.test.tsx` (both `q2-debug` and
      `q2-preview` copies) — **expect updates**. The wrapper-level inline
      `color` goes away; assertions / DOM snapshots that depend on it
      need to be updated to match the neutral state. Treat any
      "wrapper has color X" assertion as load-bearing on the old
      behaviour and revise it to "badge has color X" instead.
- [ ] `npm run build:all` from `hub-client/` succeeds — per CLAUDE.md
      this is the production-build invariant that `tsc --noEmit` and
      `vitest` don't catch.

### Phase E: documentation update

- [ ] Render `docs/authoring/attribution.qmd` locally and confirm the
      revised section reads coherently. Verify by **end-to-end** render of a
      fixture with `attribution: git` (no `css:`/`include-after-body:` YAML),
      open in a browser, hover an attributed run, see the badge. Record the
      invocation + screenshot evidence in this plan's "Verification log"
      section at the bottom.

## Work items

### Phase 1 — schema + types

- [ ] Extend the rich-form YAML parser to accept
      `attribution.viewer: bool`. Today the parser reads `source` and
      `identities`; the new key is the third recognized field. Default
      `true` when effective mode is `git` (covers both short form
      `attribution: git` and rich form `attribution: { source: git }`);
      ignored when effective mode is `off`. Touchpoints:
      `crates/quarto-core/src/attribution/types.rs` (`identity_map_from_meta`
      at line 196 is the nearby precedent — add a sibling
      `attribution_viewer_enabled_from_meta` reader that returns `bool`
      with `true` as the default).
- [ ] Pre-implementation check: confirm no current YAML schema entry for
      `attribution` exists in `crates/quarto-yaml-validation` (the
      pre-implementation grep on 2026-05-14 returned no hits). If one
      surfaces during implementation, extend it to accept `viewer:
      boolean`; otherwise no schema work is required.
- [ ] Add `attribution_viewer_enabled: bool` to `HtmlConfig` in
      `crates/pampa/src/writers/html.rs` (or the equivalent
      `FormatOptions.html` struct in quarto-core if that's the layering)
      — it lives next to `attribution_by_node` / `attribution_identities`
      so transforms can read it without re-parsing metadata. **HTML-only**;
      the JSON writer ignores it. The viewer transform consults
      `ctx.format_options.html.attribution_viewer_enabled` as its second
      gating signal.
- [ ] Populate `attribution_viewer_enabled` from merged metadata inside
      `AttributionRenderTransform` (same place that sets the other
      `format_options.html.*` fields, after the mode-resolution and
      identity-merge steps). Mirrors how identities are read once and
      threaded through.

### Phase 2 — embedded assets

- [ ] Create repo-root directory `resources/attribution/` with a short
      `README.md` (one paragraph: "Shared viewer CSS/JS, consumed by both
      `quarto-core`'s `AttributionViewerTransform` via `include_str!` and
      hub-client via Vite's `?raw` import. Edit this single source; both
      surfaces re-pick it up.") Matches the layout/voice of
      `resources/scss/README.md`.
- [ ] Create `resources/attribution/viewer.css`. Content: dotted
      underline on `[data-attr-actor]`, badge classes (`.q2-attr-badge`,
      `.q2-attr-badge-dot`, `.q2-attr-badge-time`) lifted verbatim from
      `hub-client/.../framework/attribution.tsx` lines 55–82. **Omit**
      any rule that sets `color` on a wrapper — the underline announces
      attribution; the body colour stays theme-controlled.
- [ ] Create `resources/attribution/viewer.js`. Content: the
      `formatRelativeTime`, `buildBadge`, `mouseover`, and `mouseout`
      blocks from the doc snippet (`attribution.qmd:182-262`). **Delete**
      the `forEach` block at lines 200-206 that paints wrappers with
      `el.style.color`; that's the "neutral by default" decision.
- [ ] Module wiring: `crates/quarto-core/src/attribution/mod.rs` exports
      two `pub(crate) const`s holding the file contents, loaded with
      `include_str!("../../../../resources/attribution/viewer.css")`
      and `viewer.js` (verify the `..` count during implementation —
      from `crates/quarto-core/src/attribution/mod.rs` to repo root is
      four `..`s; adjust if the module ends up elsewhere). Tests in
      Phase C grep against these constants.

### Phase 3 — transform implementation

- [ ] Add `crates/quarto-core/src/transforms/attribution_viewer.rs`
      implementing `AttributionViewerTransform`. Follow the
      `WebsiteFaviconTransform` shape (lines 43–116 of `website_favicon.rs`):
      gate inside `transform()`, append two `ConfigValue::new_string` HTML
      literals to `rendered.includes.header` and
      `rendered.includes.after-body` respectively. The gating check reads
      `ctx.format_options.html.attribution_by_node.is_some()` **and**
      `ctx.format_options.html.attribution_viewer_enabled` (defaults `true`).
- [ ] Wrap the `<style>` / `<script>` payloads in fixed sentinels
      (`<!-- quarto-attribution-viewer-css -->` and
      `<!-- quarto-attribution-viewer-js -->`).
- [ ] Implement dedup explicitly: before pushing, scan the existing
      `rendered.includes.header` (resp. `rendered.includes.after-body`)
      array for any string containing the matching sentinel; if found,
      skip the push. This is what the Phase A idempotency test
      verifies — without this dedup the test fails. Two cheap helper
      `fn has_sentinel(items: &[ConfigValue], sentinel: &str) -> bool`
      keeps the transform body legible.
- [ ] Register the transform in `crates/quarto-core/src/pipeline.rs`
      inside `build_transform_pipeline`, immediately after the
      `AttributionRenderTransform::new()` push at line 1069. (NOT
      `stage/stages/ast_transforms.rs` — that file is the stage
      wrapper; transform registration lives in `pipeline.rs`.)
- [ ] Add `"attribution-viewer"` to `Q2_PREVIEW_TRANSFORM_EXCLUDED`
      (pipeline.rs:1102-1125) under the **"HTML-pipeline-specific outputs"**
      comment, alongside `"website-favicon"`. Without this addition, the
      transform runs in the q2-preview pipeline too — harmless because
      hub-client ignores `rendered.includes.*`, but it violates the
      "CLI-only by design" decision and the deny-list inversion
      defaults new transforms to *included*. The
      `q2_preview_transform_excluded_names_exist_in_html_pipeline` test
      verifies the name is real.

### Phase 4 — hub-client converges with the CLI

This phase has two coordinated edits to
`hub-client/src/components/render/framework/attribution.tsx` — adopt the
shared CSS, and drop the wrapper-level recolouring so the visible
behaviour matches the CLI.

- [ ] Replace the `attributionStyles` const (current lines 54-82) with a
      `?raw` import of the shared file:
      ```ts
      import viewerCss from '../../../../../resources/attribution/viewer.css?raw';
      export const attributionStyles = viewerCss;
      ```
      Verify the relative path during implementation — five `..` segments
      from `hub-client/src/components/render/framework/` to repo root,
      then down into `resources/attribution/`. Adjust to whatever the
      filesystem actually requires. Keep the named export so call sites
      in `q2-debug` `AstRenderer` and `q2-preview` `PreviewDocument` need
      no edits.
- [ ] In the same file, remove the inline-colour repaint on
      `AttributionWrap` (current line 114: `const style = { color: attribution.color };`
      and its two consumers on lines 117 and 123). The two return
      branches simplify to:
      ```tsx
      if (as === 'div') {
          return (
              <div className="q2-attr-wrap" data-sid={sid}>
                  {children}
              </div>
          );
      }
      return (
          <span className="q2-attr-wrap" data-sid={sid}>
              {children}
          </span>
      );
      ```
      The badge component (`AttributionBadge`) is untouched and still
      uses `record.color` — author colour shows only on hover, matching
      the CLI default.
- [ ] Update the JSDoc on `AttributionWrap` to drop the now-stale "inline
      `color` payload" sentence and note that wrappers are neutral; colour
      lives on the hover badge.
- [ ] **Widen Vite's filesystem allow-list** (required, not optional —
      the asset lives outside hub-client's project root). In
      `hub-client/vite.config.ts`, set
      `server: { fs: { allow: ['..', resolve(__dirname, '../resources')] } }`
      (or just `['..']` to allow the whole monorepo parent, mirroring
      common practice). The TS declaration side is fine —
      `hub-client/src/vite-env.d.ts` already pulls in `vite/client`, which
      types `*?raw` imports. The precedents (`changelog.md?raw`,
      `more-info.md?raw` in `AboutTab.tsx:12-13`) all live *inside*
      hub-client, so they don't exercise the fs.strict path. Without this
      change, `vite build` fails with a `Restricted` error.
- [ ] Optional polish (not required for correctness): also define a
      Vite alias such as
      `'@quarto-resources': resolve(__dirname, '../resources')` and
      import as `@quarto-resources/attribution/viewer.css?raw`. Shorter
      and renames-tolerant.
- [ ] Run the Phase D tests: hub-client unit suite + `npm run build:all`
      from `hub-client/`. The latter is the only check that catches
      vite-resolution failures of an out-of-tree path.
- [ ] Add a `hub-client/changelog.md` entry (second commit, per
      CLAUDE.md two-commit workflow) calling out the visible change:
      attribution wrappers no longer repaint body text; author colour
      appears on the hover badge only.

### Phase 5 — documentation

- [ ] In `docs/authoring/attribution.qmd`, **delete** the "Adding a viewer
      overlay" section (lines 129–288 — heading, two code blocks, wiring
      YAML, closing prose).
- [ ] Insert a shorter "Default viewer" section in its place, covering:
    - The underline + hover badge ship automatically with
      `attribution: git`.
    - To suppress them and supply your own theme:
      ```yaml
      attribution:
        source: git
        viewer: false
      ```
    - The four `data-attr-*` attributes remain the stable contract for
      site themes and custom filters.
    - (Optional) one sentence linking to a future "Customizing attribution
      colours" appendix once one exists.
- [ ] Verify locally per the Phase E test plan (browser inspection of a
      rendered fixture), record evidence in the Verification log below.

### Phase 6 — verification + commit

- [ ] `cargo nextest run -p quarto-core` — all attribution tests pass.
- [ ] `cargo nextest run --workspace` — no regressions in downstream crates.
- [ ] `cargo xtask verify --skip-hub-build` — Rust-strict CI parity.
- [ ] `cargo xtask verify` — full run including hub-client build.
      **Non-optional this time**, for two reasons:
      (1) hub-client now imports the shared CSS via `?raw` — only the
      vite build catches an out-of-tree resolution failure;
      (2) `FormatOptions` gains an `attribution_viewer_enabled` field that
      the WASM build will pull in.
- [ ] Stage, summarize snapshot changes per the CLAUDE.md snapshot policy
      (expect on-path snapshots to drop the manual CSS/JS scaffolding from
      any fixtures that had it, and to grow the auto-injected
      `<style>`/`<script>` blocks). Commit and wait for explicit push
      approval.

## Pre-implementation verification

Questions raised in earlier drafts of this plan have been resolved
before implementation by direct tree inspection (searches recorded
2026-05-14):

- **No tracked fixtures paste the manual viewer snippet.** A full-tree
  grep for `q2-attr-badge`, `attribution-viewer.css`,
  `attribution-viewer.js`, `attribution-viewer-include` returns only
  (a) `docs/authoring/attribution.qmd` (being rewritten in Phase 5),
  (b) the three hub-client files in Phase 4 scope, and
  (c) `.tmp-attr-demo/` — a local untracked sandbox (`git ls-files`
  returns zero entries for that path). No `tests/fixtures/`,
  `crates/*/tests/`, or `examples/` carries the snippet. **No fixture
  cleanups are required as part of this commit.**

  Existing test files that grep for `data-attr-actor`
  (`crates/quarto/tests/attribution_cli_e2e.rs`,
  `crates/quarto-core/tests/attribution_render.rs`) only assert on the
  wrapper writer's output; they don't paste scaffolding. They are
  exactly the tests Phase A and Phase B extend.

- **`{source: off, viewer: true}` resolves automatically by the
  architecture.** The viewer transform's first gating signal is
  `ctx.format_options.html.attribution_by_node.is_some()`. The render
  transform sets that field to `Some(...)` only when
  `ctx.attribution_data` is `Some`, which in turn requires a provider —
  installed only when the resolved mode is `Git`. With `source: off`,
  no provider, no `attribution_data`, no `attribution_by_node`, and the
  viewer transform skips. The `viewer: true` boolean is silently
  ignored, mirroring how `identities: {...}` is ignored today when
  `source: off`. No special resolution code is needed; the Phase A test
  `attribution_viewer_skips_when_attribution_off` pins this invariant.

- **YAML schema location.** A grep for `attribution` under
  `crates/quarto-yaml-validation` returned zero hits, so there is no
  existing schema entry to extend. Validation is currently structural
  (the `identity_map_from_meta` reader silently ignores unknown keys).
  Phase 1 reflects this — no schema work required for v1; revisit if a
  schema entry lands later.

## Follow-ups (file as `discovered-from` when implementation lands)

- A `Customizing attribution` doc appendix that gives recipes for keying
  off `data-attr-actor` in a theme, plus a "high-contrast" example for
  accessibility. Useful but not needed for v1.

## Verification log

- **Invocation** (CLI, single-commit git repo, fixture
  `crates/quarto-core/tests/fixtures/attribution-blame/doc.qmd`):

  ```bash
  git init -q -b main && git add doc.qmd
  git commit -q --date="@1700000000" -m "initial" \
      --author="Charlie <charlie@example.com>"
  cargo run --bin q2 -- render doc.qmd --to html --attribution=git
  ```

- **Observed output snippet** (`doc.html`, abbreviated):

  ```html
  <head>
    ...
    <!-- quarto-attribution-viewer-css -->
    <style>
    [data-attr-actor] { text-decoration: underline dotted; ... }
    .q2-attr-wrap { position: relative; }
    .q2-attr-badge { ... color: var(--attr-color); ... }
    .q2-attr-badge-dot { ... }
    .q2-attr-badge-time { font-weight: 400; opacity: 0.7; }
    </style>
  </head>
  <body class="fullcontent">
    ...
    <p data-attr-actor="charlie@example.com" data-attr-time="1700000000"
       data-attr-name="charlie" data-attr-color="hsl(234, 60%, 55%)">
      <span data-attr-actor="..." ...>Alice wrote this paragraph.</span>
    </p>
    ...
    <!-- quarto-attribution-viewer-js -->
    <script> /* mouseover/mouseout badge logic */ </script>
  </body>
  ```

- **Inspection notes**: rendered HTML was read directly (not via a
  browser; this is a headless verification). All four contract
  artefacts present: dotted-underline CSS, `q2-attr-badge` classes
  inside the auto-injected `<style>`, both sentinel comments
  (`quarto-attribution-viewer-{css,js}`), and per-paragraph
  `data-attr-*` attributes. The injected JS body retains its hover
  build/teardown logic and does not reassign wrapper `.style.color`
  (Phase C invariant `viewer_js_does_not_recolor_wrapper_text`).
  Headless end-to-end through the production CLI binary; an
  interactive browser hover test was not run from this session.
