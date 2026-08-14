# format.html.css files are neither copied into the site nor rebased per page (bd-format-css-not-copied-crn3bjdz)

**Date:** 2026-08-14
**Braid:** bd-format-css-not-copied-crn3bjdz (bug, p1, label `websites`)
**Checkout:** main checkout, branch `main` @ `10d86829` (investigation only — no worktree/branch created)
**Status:** Implementation in progress on branch
`braid/bd-format-css-not-copied-crn3bjdz` (Phases 0–3 complete, full
workspace suite green 12182/12182; Phase 4 verification underway — see
"Phase 4 evidence").

## Phase 4 evidence (end-to-end, real binary)

Run 2026-08-14 on the implementation branch:

```
cargo run --bin q2 -- render claude-notes/plans/format-css-not-copied-investigation/repro
# → "Rendered 2 of 2 files", exit 0
```

Output inspected directly (matches Q1's expected table from the repro
README exactly):

- `_site/styles.css` exists (marker property `--repro-project-css` present);
- `_site/site_libs/quarto-contrib/quarto-project/acme/widget/widget.css`
  exists (marker `--repro-extension-css` present); no `_site/_extensions/`;
- `index.html`: `href="styles.css"`,
  `href="site_libs/quarto-contrib/quarto-project/acme/widget/widget.css"`,
  linked after the theme bundle;
- `deep/deeper/index.html`: `href="../../styles.css"`,
  `href="../../site_libs/quarto-contrib/quarto-project/acme/widget/widget.css"`.

Also verified through the binary: single-doc `fancyfmt-html` extension
render relocates+copies bundled css to
`test_files/quarto-contrib/quarto-project/fancyfmt/fmt-style.css` (its
smoke-all fixture now pins this — previously the link only worked because
output landed beside the source tree).

## Implementation shape (as landed)

- **Marking** (`crates/quarto-core/src/project/format_css.rs`):
  `mark_css_path_values` — existence-driven, per layer, called from
  `MetadataMergeStage` for the project layer (after the `!path`
  adjustment; diagnostics dropped there), directory-metadata layers, and
  the document layer (diagnostics pushed per document).
  `missing_project_css_diagnostics` runs once per project render from
  `ProjectPipeline::run` into `project_diagnostics` (Q-5-29).
- **Transform** (`crates/quarto-core/src/transforms/format_css.rs`):
  `FormatCssTransform` (Normalization phase, self-gated to HTML-family,
  no-op in VFS mode) — consumes only marked Path entries; mirrors
  project-relative paths; relocates `_extensions/**` to
  `quarto-contrib/quarto-project/**` via `ArtifactScope::Project`
  resolver queries; pushes `ResourceCopyIntent`s (skipping src==dest);
  rewrites entries to per-page hrefs. Never diagnoses.
- **Revealjs**: `apply_template.rs` reveal branch appends
  `user_css_urls(&metadata)` after the vendored deck assets.
- **Q-5-29**: catalog entry + `docs/errors/project/Q-5-29.qmd`.

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, both halves of the fix
have direct in-tree precedent (favicon copy + Path-kind metadata rebase /
resource-resolver href resolution), and the remaining decisions are genuine
design choices (output layout, which rebase mechanism, scope), not missing
information.

## Issue context

Filed 2026-08-14 by the q2-connect-docs porting session (origin strand in that
skein: br-format-css-not-copied-4jnxbq38). A website project declaring

```yaml
format:
  html:
    css:
      - styles.css
      - _extensions/acme/widget/widget.css
```

gets a `<link>` to each file on every page, but:

1. **Not copied** — neither file is written into `_site/`, so every link
   404s. No diagnostic; exit 0. Q1 copies both (project css to
   `_site/styles.css`; extension-owned css to
   `site_libs/quarto-contrib/quarto-project/acme/widget/widget.css`).
2. **Not rebased** — the href is emitted verbatim at every depth
   (`styles.css` on `deep/deeper/index.html`, where Q1 emits
   `../../styles.css`). The built-in theme stylesheets on the same page *are*
   rebased — that asymmetry is the control.

Real-world impact: all 352 rendered pages of the Posit Connect docs port link
two nonexistent stylesheets (704 broken references). Invisible to text-diff
sweeps because CSS contributes no text.

Not the same as bd-of20unsb (extension `contributes.formats` fragment paths,
fixed in 0.21.0): here the paths live in the project's own `_quarto.yml`; the
second one merely points into `_extensions/`. Notably, the repro's
`_extensions/acme/widget/` contains **only** `widget.css` — no
`_extension.yml` — so no extension machinery is involved at all.

## Dependency graph

**Empty** — no edges in the skein (strand is hours old). Context instead
comes from the strands the description references:

- **bd-root-relative-paths-design-fc5pvkcv** (in_progress, design) — the
  navbar-logo/root-absolute-path design session. Its Decision 5 ("favicon is
  not special — config-declared assets q2 knows about get the same
  warn-and-continue copy treatment") is the stated policy this bug falls
  under. Its case-A fix (0.21.0) built `copy_navbar_logo` /
  `copy_footer_images` on the shared `copy_asset_file` helper — the exact
  seam to extend.
- **bd-of20unsb** (in_progress; fix shipped in 0.21.0 per repro README) —
  extension-fragment path rebasing. Its mechanism (mark values as
  `ConfigValueKind::Path`, existence-driven, then let the metadata merge
  rebase them per document) is one of the two candidate mechanisms for the
  rebase half here.

## What the code looks like today

All paths verified at `main` @ `10d86829`:

- **Link emission**: `extract_css_from_meta`
  (`crates/quarto-core/src/template.rs:928`) reads the `css` metadata key
  (scalar / PandocInlines / array) and appends the strings **verbatim** to
  the template `css` list (`render_with_compiled_template`,
  `template.rs:699-709`). No resolver, no copy, no existence check.
- **Why theme css *is* rebased**: built-in stylesheets are artifacts;
  `ApplyTemplateStage` computes their URLs via the per-page
  `ResourceResolverContext` (`apply_template.rs:166`,
  `collect_artifact_urls`). User css never touches that path.
- **Copy boundary**: `crates/quarto-core/src/project/website_post_render.rs`
  has `copy_favicon`, `copy_navbar_logo`, `copy_footer_images`, all sharing
  `copy_asset_file` and the warn-on-missing-source pattern. `format.html.css`
  has no counterpart. (All native-only; the in-browser preview has no on-disk
  output dir.)
- **Href precedent (per-page transform)**: `WebsiteFaviconTransform`
  (`transforms/website_favicon.rs`) resolves a page-relative href through
  `ctx.resource_resolver` and appends the `<link>` to
  `rendered.includes.header`.
- **Rebase precedent (metadata merge)**: `FRAGMENT_PATH_PATTERNS`
  (`project/mod.rs:696`) already lists `["format", "*", "css"]` — but only
  for **extension** `contributes.project` fragments. Values marked
  `ConfigValueKind::Path` are rebased project-root → document-dir by
  `adjust_paths_to_document_dir` during the metadata merge
  (`metadata_merge.rs:256` applies it to the project-config layer). The
  project's own `_quarto.yml` values are never *marked* Path-kind, so the
  machinery never fires for them.
- **Resource collector**: `resource_collector.rs` walks the AST only;
  metadata-declared css is invisible to it (same blind spot the design
  strand documents for raw HTML).

### Repro at HEAD

Fixture: `claude-notes/plans/format-css-not-copied-investigation/repro/`
(mirrors the external repro at
`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/format-css-not-copied/`).

Run 2026-08-14 at `main` @ `10d86829` (pre-flight `cargo xtask verify
--skip-hub-build` green, 12167/12167):

```
cargo run --bin q2 -- render claude-notes/plans/format-css-not-copied-investigation/repro
# → "Rendered 2 of 2 files", exit 0, no diagnostic
```

Observed output (inspected directly):

- `_site/styles.css`: **does not exist**; no css file anywhere in `_site`
  besides `site_libs/` assets. Marker custom properties absent from the
  theme bundle — the declared css is dropped entirely.
- `_site/index.html` links: `site_libs/…` (fine), then verbatim
  `href="styles.css"` and `href="_extensions/acme/widget/widget.css"` —
  both 404.
- `_site/deep/deeper/index.html`: `../../site_libs/…` (rebased correctly)
  immediately beside verbatim `href="styles.css"` /
  `href="_extensions/acme/widget/widget.css"` — the asymmetry the strand
  describes, confirmed on one page.

Both defects confirmed; matches the external repro's table for q2 0.21.0.

## Evidence pass 1: how and why Q1 organizes the output (external-sources/quarto-cli)

Two separate machineries, plus a website-only patch; `css:` straddles them.

**Rebase (metadata-time, not DOM-time).** Q1 runs pandoc with
`cwd = dirname(input)` (`src/command/render/pandoc.ts:371-372`), so every
relative path in the defaults file must be input-dir-relative. Project-config
paths are authored project-relative, so Q1 rewrites them at metadata-merge
time: `toInputRelativePaths` (`src/project/project-shared.ts:138-207`), called
from `projectMetadataForInputFile` (`render-contexts.ts:741-757`). It is a
blind recursive walk over the whole merged config: any non-absolute string
that names an existing file under the project dir becomes
`<offset>/<value>` (`offset = relative(inputDir, baseDir)`) — that is the
entire story of `../../styles.css`. **Existence-driven, layer-aware (the
same function runs per layer: project config, `_metadata.yml` with the
metadata file's dir, extension config with the extension dir as base).**
The HTML template then emits the string verbatim
(`resources/formats/html/pandoc/html.template:23-24`). The deno-dom
postprocessor never rewrites a plain relative `css` href — it only records
it as a resource ref.

**Copy (resource-ref mirror).** The website HTML postprocessor collects every
resource-tag href; refs are resolved to absolute paths and mirrored:
`join(formatOutputDir, relative(projDir, file))`
(`src/command/render/project.ts:735-766`). So project-root css lands at
`_site/styles.css` purely because resource copying is a project-relative
mirror. `copyResourceFile` also chases `url()`/`@import` inside copied css
(`project-resources.ts:110-138`).

**The `_extensions` relocation (website/book-only).**
`projectExtensionPathResolver` (`src/extension/extension.ts:158-189`) is a
resolver injected into the website postprocessor: an href resolving under
`_extensions/` is copied to
`<lib_dir>/quarto-contrib/quarto-project/<path-with-_extensions-stripped>`
and the DOM attribute rewritten. Rationale, from the code and introducing
commits (`364eb9a2d`, `b1e866553`):

1. Underscore-prefixed dirs are systematically non-output
   (`projectHiddenIgnoreGlob`, `project-context.ts:878-886` — the same list
   that excludes `README.*`, `CLAUDE.md`, `AGENTS.md`). Publishing
   `_site/_extensions/` would leak Lua sources, `_extension.yml`, READMEs;
   relocating means only individually-referenced files ship. This matches
   the user's stated constraint exactly.
2. The lib dir is freezer-managed and pruned by a whitelist of known names
   (`formatLibDirs`, `project-default.ts:23-29`); `quarto-contrib` is the
   single reserved namespace that protects third-party content without
   enumerating extension names. `quarto-project` is a fixed literal meaning
   "the project itself is the contributor" (used when a file is referenced
   by raw path rather than through a named HTML dependency). There is no
   per-org namespace logic — the `_extensions` prefix is stripped and the
   remainder (org segment included) preserved verbatim.

**Book = website machinery** (`book.ts:74` inherits, `book.ts:225-243`
merges website formatExtras), so the relocation applies there too.
Single-doc renders never rebase (no project offset) and never relocate;
lib assets go to `<stem>_files/libs/`. Notably, a *default*-type project
with an `output-dir` mirrors `_extensions/...` css into the output
verbatim in Q1 — the relocation is deliberately a website/book behavior.

**Other keys (Q1):** `include-*` files are inlined by pandoc (never
copied); `format-resources` are flattened **basename-only** next to the
output — they exist for writers that want sibling files (LaTeX `.cls`,
Typst), not for `<link>`s; `theme`/SCSS compiles to a cache then ships as a
FormatDependency under `site_libs/bootstrap|quarto-html` with hrefs built
from the input-relative lib dir; document `resources:`/`project.resources`
mirror project-relative paths, no hrefs. Four placement policies for four
consumption models.

## Evidence pass 2: q2 boundary audit

Per-key status at `main` @ `10d86829` (file:line cites in the agent record;
load-bearing ones inline):

| key | q2 today |
|---|---|
| `format.html.css` | linked verbatim (`extract_css_from_meta`, `template.rs:928`); never copied; only other references are the two extension pattern tables. **On revealjs, user css is dropped entirely** — the scaffold only takes artifact URLs (`revealjs/assemble.rs:369-410`, `apply_template.rs:302-306`), so not even a broken `<link>` is emitted. |
| `theme` | compiled to Project-scoped artifact `css:theme:<fp>` (`compile_theme_css.rs:707-730`); per-page URL via resolver (`apply_template.rs:166` → `html_url_for`); flushed to `site_libs` (website) or per-page `<stem>_files` (default/book). Healthy. |
| `include-in-header`/`-before-body`/`-after-body` | inlined by `IncludeResolveStage` into `rendered.includes.*` — Q1 parity. Raw `<script>`/`<link>` written inside them is emitted byte-for-byte (same as Q1; authors own those paths). |
| `format-resources` | **accepted but never consumed** — only the two pattern tables mention it; silently inert, no diagnostic. (Q1 flattens basename-only next to output for LaTeX/Typst-style consumers.) Follow-up strand. |
| user js | no `scripts:` metadata key in Q1 either; user js arrives via includes. Parity — no gap beyond includes being verbatim. |
| favicon / navbar logo / footer images | copied (post-render hooks, website-only) + separately href-resolved per page by their transforms (`apply_favicon` → `page_url_for`; navbar logo → `resolve_root_relative_resource_href`, `navbar_render.rs:121`). |
| `project.resources` / doc `resources` | glob → mirror copy `copy_resources_to_output_dir` (`project_resources.rs:1093`), all project types, post-render, copy-only. Healthy. |
| `brand` | styling rides the theme artifact; brand files ride favicon/navbar paths. No separate gap. |

**Structural facts that constrain the fix:**

- **Books don't share `website_post_render.rs`.** `ProjectKind::Book |
  Manuscript → DefaultProjectType` (`orchestrator.rs:517-522`), which
  inherits the default no-op-ish `post_render`. A website-only hook leaves
  books broken. Cross-type channels: (i) per-page `ResourceCopyIntent`
  (`render.rs:186`, flushed in both `render_to_file.rs:413` and
  `pass2_renderer.rs:896,1180` — the `title_banner.rs:139-150` precedent,
  which does copy-intent + href in one transform), or (ii) Project-scoped
  artifacts.
- **The artifact route** would give copy + per-page href for free
  (`collect_artifact_urls` picks up every `css:` key), but relocates
  everything under `site_libs/` — diverging from Q1's mirror layout for
  project-root css, and key collisions demand a fingerprint in the key.
- **`resolve_root_relative_resource_href`** (`navigation_href.rs:467`) is
  the purpose-built call for config-declared asset hrefs (external-URL
  passthrough, query/fragment handling, project-root anchoring).
- **Layer provenance only exists at merge time.** A document-front-matter
  `css:` entry is authored document-relative; a `_quarto.yml` entry is
  project-relative. After the merge flattens layers, template-time code
  cannot tell them apart. Q1 solves this by rebasing per layer inside the
  merge (each `toInputRelativePaths` call gets the right base dir). q2's
  equivalent seam is `adjust_paths_to_document_dir`, already applied per
  layer (`metadata_merge.rs:256,266`) — and `css: !path styles.css`
  already rebases today (test at `project/mod.rs:2795-2831`).

## Synthesis: proposed mechanism (pending user confirmation)

Q1's evidence cuts both ways on the earlier question 2: Q1 itself uses
merge-time string rebasing (mechanism (a)) for `css`, but only because its
pandoc-cwd constraint forces every path to be input-relative; the
copy side is a separate mirror. q2 has no such constraint, and the user's
concern (input tree won't always mirror output tree) stands. But pure
template-time resolution (mechanism (b)) loses layer provenance.

Proposed hybrid — **(a)'s marking, (b)'s emission**:

1. **Merge time (provenance-aware):** existence-driven marking of
   `format.*.css` entries as Path-kind per layer — normalizing every entry
   to a *document-dir-relative* value exactly as the existing
   `adjust_paths_to_document_dir` machinery already does for `!path` and
   extension-fragment values. Near-zero new code: extend the marking that
   extension fragments already get to the project's own config layer (and
   directory metadata), scoped to the audited key list.
2. **Render time (output-aware):** a transform (Navigation/Finalization
   phase, all HTML formats) that walks the merged `css` list and, per
   entry: skips external URLs; resolves to a project-root-relative source;
   chooses the output location — **mirror of the project-relative path**,
   except entries under `_extensions/` relocate to
   `<lib_dir>/quarto-contrib/quarto-project/<rest>` (Q1 parity, and the
   only way to avoid shipping `_extensions/` paths); pushes a
   `ResourceCopyIntent` (works for every project type); rewrites the
   entry to the per-page href via `resolve_root_relative_resource_href` /
   `page_url_for`; emits the missing-file Q-code when the source doesn't
   exist. `extract_css_from_meta` then emits pre-resolved values unchanged.

This keeps Q1's output layout (`_site/styles.css`;
`site_libs/quarto-contrib/quarto-project/acme/widget/widget.css`), works
for websites *and* books/default projects (copy-intent channel), and keeps
the resolver as the single source of href truth so a future
input≠output-tree world only touches step 2.

For books/single-doc, `lib_dir` is empty — the relocation target falls back
to the resolver's Project scope root (`<stem>_files/`), matching where
theme css already lands there (Q1 analog: `<stem>_files/libs/`).

## Work items

Implementation runs on branch `braid/bd-format-css-not-copied-crn3bjdz`
(remote will be `bugfix/bd-format-css-not-copied-crn3bjdz`), branched from
`main` @ `10d86829` with the investigation commits.

### Phase 0 — failing tests first

- [x] Integration: website render copies project-root css to `_site/styles.css`
- [x] Integration: hrefs depth-correct (`styles.css` at root, `../../styles.css` two deep)
- [x] Integration: `_extensions/**` css relocated to `site_libs/quarto-contrib/quarto-project/**`, hrefs point there
- [x] Integration: document front-matter `css:` in a subdirectory resolves against the document dir (copied + linked)
- [x] Integration: missing declared css → Q-code diagnostic, link still emitted, render completes
- [x] Integration: external URL entries pass through verbatim, no copy, no diagnostic
- [x] Integration: default-project (DefaultProjectType — books' dispatch) render copies css + correct hrefs
- [x] Integration: revealjs single-doc render links user `css:` (currently dropped)
- [x] All of the above verified failing at HEAD

### Phase 1 — merge-time marking

- [x] Unit tests: project-config `css` scalar/array/inlines marked Path-kind when file exists; untouched otherwise
- [x] Mark `format.*.css` in the project's own config layer (existence-driven, base = project dir)
- [x] Same for directory metadata (`_metadata.yml`) and document front matter layers as applicable

### Phase 2 — render-time transform

- [x] New transform: copy intents + per-page hrefs via resolver; `_extensions/**` relocation; external-URL passthrough
- [x] Missing-file Q-code emission
- [x] Revealjs: user css list appended to `css_urls` in the apply-template reveal branch
- [x] Pipeline wiring + phase-ordering test still green

### Phase 3 — Q-code + docs

- [x] Catalog entry in `quarto-error-catalog/error_catalog.json`
- [x] `docs/errors/<subsystem>/<code>.qmd` page (same commit; `error-docs-page-missing` lint)

### Phase 4 — end-to-end verification + follow-ups

- [x] `cargo run --bin q2 -- render` on the investigation fixture; inspect output (record snippet in plan)
- [ ] Re-check the Connect docs repro
- [ ] Verify `q2 preview` behavior; file preview follow-up strand if broken
- [ ] Full `cargo xtask verify` green
- [ ] User-facing docs if applicable

## Proposed phases (draft)

Refined after the two evidence passes; still pending user sign-off on the
follow-up questions below.

- **Phase 0 — Test plan (TDD).** End-to-end tests driving the real render
  path (`render_document_to_file` / project orchestrator, per the
  end-to-end policy): (a) website render writes `_site/styles.css`;
  (b) `_extensions/`-owned entry lands at the relocation target and its
  href points there; (c) deep page's `<link href>` is depth-correct;
  (d) document-front-matter `css:` in a subdirectory resolves against the
  document dir, is copied, and links correctly; (e) missing declared css
  emits the new Q-code and the render completes; (f) external URL entries
  pass through untouched; (g) book/default-project render copies the css
  (copy-intent channel, no website hook); (h) `url()`-referenced assets:
  decide + pin behavior (question F3). Each verified failing at HEAD first.
- **Phase 1 — Merge-time marking.** Extend existence-driven Path-kind
  marking to the project's own config layer (and directory metadata) for
  `format.*.css`, so entries normalize per layer like `!path` values
  already do (layer provenance).
- **Phase 2 — Render-time transform.** New transform (all HTML formats):
  per css entry — external-URL passthrough, output-location choice (mirror
  of the project-relative path; `_extensions/**` relocates to
  `<lib_dir>/quarto-contrib/quarto-project/**`), `ResourceCopyIntent`,
  per-page href rewrite via the resolver, Q-code on missing source. Wire
  the emitted list into revealjs's `css_urls` too so user css stops being
  dropped there (question F2).
- **Phase 3 — Q-code + docs page.** New catalog entry + `docs/errors/`
  page in the same commit (`error-docs-page-missing` lint enforces).
- **Phase 4 — End-to-end verification + follow-ups.** Render the
  investigation fixture and the Connect docs repro through the real
  binary, inspect output; verify `q2 preview` behavior and file the
  preview follow-up strand if broken; file the `format-resources` strand;
  user-facing docs.

## Design decisions (user, 2026-08-14)

Answers to the questions below, recorded before the quarto-cli evidence pass:

1. **Output layout:** never `cp -r _extensions/` into output (authors ship
   README.md etc. that don't belong). Individual declared assets may be
   copied "along the same path"; study `external-sources/quarto-cli` for
   the rationale behind Q1's `site_libs/quarto-contrib/quarto-project/…`
   relocation before settling the exact layout.
2. **Rebase mechanism:** leaning (b) (per-page `ResourceResolverContext`),
   because q2 will not always be able to replicate input-tree paths in the
   output tree. Not held strongly — check quarto-cli for evidence either
   way; follow-up question allowed.
3. **Scope:** audit the whole config-declared-asset boundary in this
   strand, not just `css`.
4. **Diagnostic:** mint a new Q-code (project direction: remove plain
   warnings). Requires catalog entry + `docs/errors/` page in the same
   commit (`error-docs-page-missing` lint).
5. **Preview:** this strand targets render parity; verify preview behavior
   during Phase 4 and file a follow-up strand if broken.

## Resolved by the evidence passes

(The original five questions are preserved with the user's answers in
"Design decisions" above. The evidence passes resolved their open parts:)

- **Layout:** Q1 parity — mirror project-relative paths; `_extensions/**`
  relocates to `<lib_dir>/quarto-contrib/quarto-project/**`. Q1's rationale
  (underscore dirs are systematically non-output; relocation ships only the
  individually-referenced files; `quarto-contrib` is the reserved,
  freezer-protected namespace) matches the user's constraint exactly.
- **Mechanism:** hybrid proposed — merge-time layer-aware Path marking +
  render-time resolver-driven copy/href transform (see Synthesis). Pending
  F1.
- **Scope:** boundary audited (table in Evidence pass 2). Real gaps found:
  revealjs drops user css entirely (F2); `format-resources` accepted but
  inert (F4).
- **Diagnostic:** new Q-code (Phase 3).
- **Preview:** verify in Phase 4; follow-up strand if broken.

## Settled follow-ups (user, 2026-08-14)

- **F1 — mechanism: hybrid approved.** Merge-time per-layer Path-kind
  marking (normalization + provenance) + render-time transform owning
  `ResourceCopyIntent` and per-page hrefs via the resolver.
- **F2 — revealjs: in this strand.** Phase 2's transform wires the
  resolved css list into revealjs's `css_urls`, ending the silent drop.
- **F3 — css `url()`/`@import` chasing: follow-up strand.** Filed as
  **bd-dxp854dw** (task, p2, discovered-from this strand). Copied user css
  ships as-is; the limitation gets documented. (Evidence: Connect docs css
  uses only `data:` URIs in `url()` — verified by grep — so the motivating
  case is unaffected.)
- **F4 — `format-resources`: filed as bd-ptb0v2lk** (bug, p2,
  discovered-from this strand): silent no-op today; minimum fix is a
  diagnostic, full fix is Q1's basename-flatten copy.

## Risks / tradeoffs (draft)

- **Books:** confirmed — `ProjectKind::Book → DefaultProjectType` with no
  post-render of its own, so the fix must use a cross-type channel
  (`ResourceCopyIntent`), not `website_post_render.rs`. Phase 0(g) pins
  this.
- Merge-time Path marking changes the merged metadata value shape
  (`Scalar` → `Path`) for a user-visible key; downstream readers (Lua
  filters, template contexts) observe normalized values. Q1 mutates
  metadata the same way (`toInputRelativePaths`), so filter-visible
  rewritten paths are Q1-compatible behavior — but worth stating in docs.
- The relocation choice hard-codes Q1's `quarto-contrib/quarto-project`
  literal into q2 output layout; freeze does not exist in q2 yet, so the
  freezer-protection rationale is speculative here — we adopt the layout
  for output parity, not for freezer semantics.
- The design strand bd-root-relative-paths-design-fc5pvkcv is still
  in_progress; its remaining case C (raw HTML) is independent, but any
  decision here should cite its Decision 4/5 vocabulary (leading `/` =
  site-root-relative; config-declared assets get warn-and-continue copy) to
  stay consistent.
