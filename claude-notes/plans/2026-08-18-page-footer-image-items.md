# page-footer item text: lone image dropped; no link/image target resolved (bd-page-footer-image-items-stmpikgo)

**Date:** 2026-08-18
**Braid:** bd-page-footer-image-items-stmpikgo
**Branch:** `main` (investigation committed in place; implementation branch TBD by user)
**Status:** Design aligned 2026-08-18 (user answered all five questions; see
§ Design decisions). Ready to turn into implementation phases.

## Triage verdict

**Ready to design.** Both defects are confirmed at HEAD by code inspection and
end-to-end repro; the mechanisms are exactly as the strand describes, the fix
sites are small and known, and the only open questions are scope choices
(fix level for defect 1, whether defect 2's fix covers navbar/sidebar too).

## Issue context

Filed 2026-08-18 (today) by Carlos, P2 bug, labels `parity`, `websites`. Two
independent silent defects in page-footer *item* `text:` (the `- text: "..."`
list form), with the *region*-level `text:` form as a working control:

1. **A lone-image item renders empty.** `text: '![x](/images/logo.svg)'` →
   `<li class="nav-item"></li>`. Wrapping the image in a link or adding any
   sibling inline makes it survive.
2. **Nothing inside item `text:` is resolved.** Link/Image targets in item text
   are never rebased (`/images/logo.svg` stays root-absolute; `.qmd` never
   becomes `.html`), while the identical markdown in a region-level `text:`
   is fully resolved. 404s on any page below the site root; no diagnostic
   (body-content equivalents raise Q-5-6).

Real-world driver: the Posit Connect docs port (served from a subdirectory,
`https://docs.posit.co/connect/`) — its footer carries four images whose
obvious markdown-image workaround is defeated by exactly these two defects.
Origin strand in the connect-docs skein: `br-page-footer-image-items-sjto1juf`.
Filed P2 but is the blocker under a P1 there.

## Dependency graph

No parent/children; two `related` edges, both `in_progress`:

- **related: bd-page-footer-items-f4th80mj** (P1) — the strand that *made*
  item `text:` markdown-parsed at all (five defects: escaping, bare-string
  items, shortcodes, entities, href-less anchors). This new strand is the
  next layer of the same surface: text now parses, but images/links inside it
  aren't first-class. The `MARKDOWN_CONFIG_PATHS` entries for
  `page-footer.**.text` / navbar / sidebar carry its bd id in a comment.
- **related: bd-root-relative-paths-design-fc5pvkcv** (P1, design) — the
  three-case design for site-root-relative paths. Defect 2's machinery
  (`rewrite_config_inlines`, "Case C") was built under it; the call-site
  comment in `footer_render.rs` states the intent that this transform "owns"
  footer-inline resolution — item text was simply never routed through it.

No `discovered-from` edge in braid, but the description records the origin
session. No incoming `blocks` edges; urgency comes from the connect-docs port.

## What the code looks like today

Everything the strand points at exists unchanged at `main` @ 5b6774d1. Full
trace with line numbers: `page-footer-image-items-investigation/code-trace.md`.
Condensed:

- **Defect 1** is a three-hop interaction: `ConfigMarkdownTransform` parses
  item text via `pampa::pandoc::meta::parse_config_string_as_markdown`; the qmd
  reader's postprocess desugars a single-image paragraph into `Block::Figure`
  (`postprocess.rs:978`); `meta.rs:75` only unwraps a lone `Paragraph` to
  `PandocInlines`, so the value stays `PandocBlocks([Figure])`; and
  `render_text`'s `block_inlines` (`render_html.rs:913`) matches only
  `Plain|Paragraph|Header` → empty string.
- **Defect 2** is one missing call: `rewrite_items_hrefs`
  (`footer_render.rs:147`) rewrites `item.href` and recurses into `item.menu`
  but never touches `item.text`/`item.bare_text`, while the `Text` region
  branch one match-arm up calls `rewrite_config_inlines` (which already
  handles Link + Image recursively, `navigation_href.rs:488`).
- **Bonus finding (a):** the `Text` region branch matches only `PandocInlines`
  — a `PandocBlocks` text region (multi-paragraph `!md`, or a lone image
  today) silently skips the rewrite as well.
- **Bonus finding (b):** navbar (and presumably sidebar) item `text:` has the
  same defect-2 gap — `navbar_render.rs` rewrites item hrefs and the navbar
  *title's* inlines, but not item text inlines. Since f4th80mj blessed
  `navbar.**.text` / `sidebar.contents.**.text` as markdown, the same repro
  shape should misbehave there. Not yet verified end-to-end.

**Reproduced end-to-end at HEAD** (see § End-to-end verification below):
`repro/` under the investigation dir is a copy of the external repro
(`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/page-footer-image-items/`).

## Proposed phases

Per-phase test-first per CLAUDE.md TDD; each phase lands with its failing
tests written and verified first.

- [x] **Phase 1 — Defect 1: lone-Figure unwrap at the config parse.**
  *Done 2026-08-18.* `unwrap_lone_figure` in `crates/pampa/src/pandoc/meta.rs`,
  applied only in `parse_config_string_as_markdown` (the `!md` path keeps
  Figure semantics; pinned by test). Five unit tests (failing-first); full
  workspace suite green, zero snapshot churn. End-to-end verified: lone-image
  footer items now render `<img>` in `repro/` (src still unrebased — Phase 2).
  In `parse_yaml_string_as_markdown_to_config` (`crates/pampa/src/pandoc/meta.rs`),
  after the existing lone-`Paragraph` unwrap, also unwrap
  `blocks == [Figure]` back to its `Image` → `PandocInlines([Image])`.
  Tests: pampa unit tests for `parse_config_string_as_markdown("![x](y)")`
  → inlines (both with alt text and `![](y)`, which never desugars);
  `render_text` on the resulting value is non-empty. Review snapshot churn
  across the workspace (shared parse path; `!md` values included — see risks).
- [x] **Phase 2 — Defect 2: route item text through the inline rewriter.**
  *Done 2026-08-18.* Shared helpers `rewrite_config_text` +
  `rewrite_item_text` in `navigation_href.rs`, wired into all three
  surfaces: footer `rewrite_items_hrefs`, navbar
  `rewrite_navigation_item_hrefs` (menus included), sidebar
  `rewrite_hrefs` (Link item text, Section titles, Headings — all
  markdown-blessed by `sidebar.contents.**.text`). `bare_text` needs no
  render-time handling: the Generate transform demotes it into `text`
  before Render runs and the emitter never reads it. Failing-first tests
  per surface at a depth-1 resolver; workspace green (12319). End-to-end:
  `repro/` deep page's item-level targets now match the region control
  (`../../images/logo.svg`, `../../index.html`); only the raw-HTML
  `{=html}` variant remains untouched (fc5pvkcv Case B, out of scope).
- [x] **Phase 3 — Blocks walker.** *Done 2026-08-18.*
  `rewrite_config_blocks` in `navigation_href.rs` (container coverage
  mirrors `ResourceCollectorTransform`'s visitor; Figures persist —
  targets inside content and caption rewrite, no unwrap). Wired via
  `rewrite_config_text`'s `PandocBlocks` arm, which now also serves the
  footer Text-region and navbar-title call sites (refactored off their
  inlines-only matches). Two additional gaps found and fixed along the way:
  (a) `FooterRegion::from_config_value` classified a `PandocBlocks` region
  as `Empty` (because `as_plain_text` is `None` for blocks), silently
  dropping the whole region — now classifies as `Text`; (b) `render_text`
  dropped persisted `Figure`s — `push_blocks_text` now renders a figure's
  image content (caption stays out of the one-line region). Failing-first
  tests at the walker, classifier, and renderer levels plus a blocks-shaped
  footer transform test; workspace green (12323). End-to-end fixture
  `repro-md-blocks/`: an `!md` two-block center region renders
  `<img src="../../images/logo.svg">` and `<a href="../../index.html">`
  on the deep page.
- [ ] **Phase 4 — Uniform Q-5-6 for footer images (decision 4).**
  Extend `copy_footer_images` to walk `Items` regions (item `text:` +
  `bare_text`, recursing into `menu`) and `PandocBlocks` values using the
  shared collectors; build a `ResourceCopyIntent` per collected URL with
  `origin` = the Image node's span (remaps into `_quarto.yml`), and report
  misses via `missing_resource_diagnostic` (Q-5-6) instead of the uncoded
  string warning. Once per project (post-render) to avoid per-page warning
  duplication. Tests: `repro-missing/` end-to-end — all four rows of the
  matrix produce exactly one spanned Q-5-6 each (plus the body control).
- [ ] **Phase 5 — Verification + docs.** Full `cargo xtask verify`;
  end-to-end render of both repros recorded in this plan; changelog/strand
  notes. Check whether `docs/errors/quarto/Q-5-6.qmd` needs wording updates
  for the config-reference case.

Possible follow-up strands (file, don't do here): upgrade `copy_navbar_logo`'s
generic warning to the same Q-5-6 shape; sidebar `logo`; favicon.

## Design decisions (2026-08-18, aligned with user)

1. **Fix level for defect 1: (a), confirmed.** Unwrap a lone `Figure` back to
   its image in `parse_yaml_string_as_markdown_to_config` (`meta.rs`) —
   "a figure with caption semantics is arguably never wanted there." The value
   becomes `PandocInlines([Image])`, so every consumer (render, inline
   rewriters, `copy_footer_images`' image collection) works on it for free.
2. **Scope of defect 2: all three surfaces.** Route item `text:`/`bare_text`
   inlines through the rewriter for page-footer, navbar, and sidebar items in
   this pass (shared helper), not footer-only.
3. **Blocks walker: yes, in this pass** — with the explicit note that **in
   block settings `Figure` nodes should persist**: the blocks walker rewrites
   Link/Image targets *inside* figures (and other block containers), it does
   not unwrap them. Only the config-string parse (decision 1) unwraps, and
   only for the lone-image case. The inline and block walkers are
   intentionally different in this respect.
4. **Diagnostics: raise Q-5-6 uniformly.** The user's framing: footer content
   should behave "more or less equivalent to what would happen in our pipeline
   if it lived inside a `::: footer` div" in the body. Investigation findings
   (see § Q4 investigation below): `rewrite_config_inlines` emits Q-13-x for
   *links* only; *images* get a pure URL rewrite with no existence check. The
   missing-file story is owned by the copy machinery, which today is a
   three-way asymmetry. The fix direction: footer/nav config images go through
   `ResourceCopyIntent` + `missing_resource_diagnostic` (Q-5-6, spanned at the
   YAML reference) instead of the bespoke uncoded warning.
5. **Priority: stays P2.**

## Q4 investigation: the current diagnostic + copy matrix

Verified by code reading and the `repro-missing/` fixture (all referencing the
same nonexistent `/images/nope.svg`; `cargo run --bin q2 -- render …/repro-missing`):

| reference site | copy attempted? | diagnostic today |
|---|---|---|
| body image (control) | yes (`ResourceCollectorTransform` → intent) | **Q-5-6 warning, spanned at the reference** |
| region-level `Text`, image with sibling inline | yes (`copy_footer_images`) | generic `Warning: page-footer image refers to missing file '…'` — **no code, no span, no docs URL** |
| region-level `Text`, **lone** image | **no** — the Figure gap hits `copy_footer_images` too (its `PandocInlines` match fails on `PandocBlocks([Figure])`) | **silent** |
| item-level `text:` image | **no** — `copy_footer_images` walks only `FooterRegion::Text`, skipping `Items` entirely (the same asymmetry as defect 2) | **silent** |

Mechanics established:

- **Q-5-6 producer/consumer split.** Producers push
  `ResourceCopyIntent { src, dest, origin: SourceInfo }` onto
  `RenderContext::resource_copies`; the shared drain
  (`enqueue_resource_copies`, `resource_copy_diagnostics.rs:121`) probes
  existence and emits `missing_resource_diagnostic` (Q-5-6, located at
  `origin`) for missing sources. The body producer is
  `ResourceCollectorTransform` (Finalization), which walks **`ast.blocks`
  only** — footer inlines live in `ast.meta`, so they never produce intents.
- **A config-image precedent exists**: `title_banner.rs:148` pushes a
  `ResourceCopyIntent` for the banner image (with a generated span;
  ours can do better — see below).
- **Spans can point into `_quarto.yml`.** `parse_config_string_as_markdown`
  threads the YAML scalar's `SourceInfo` into the qmd reader, so Image nodes
  in parsed config text carry remappable spans — a footer Q-5-6 can underline
  the reference inside `_quarto.yml` the way the body one underlines the qmd.
- **`copy_footer_images`** (`website_post_render.rs:183`, native-only,
  post-render, once per project) re-parses raw scalars itself via
  `parse_config_string_as_markdown` — so decision 1's Figure unwrap
  automatically fixes its lone-image gap too. Its missing-file warning is
  an uncoded, span-less `DiagnosticMessage::warning`. `copy_navbar_logo`
  has the same generic-warning style (out of scope here, but the same
  upgrade applies if we want full uniformity later).
- **Duplication consideration.** The footer appears on every page: if the
  per-doc pipeline emitted the intents, a missing footer image would warn
  once *per rendered page* (352× on the Connect docs). The post-render hook
  runs once per project, which is the natural dedup point. Proposed shape:
  keep collection/copy in `copy_footer_images` (extended to Items regions
  and blocks via the shared walkers), but build a `ResourceCopyIntent` per
  URL and report misses through `missing_resource_diagnostic` so the
  user-visible warning is the same Q-5-6, spanned at the YAML reference.
  (A body `::: footer` div would technically warn per page; once-per-project
  with the same code+span is the "more or less equivalent" reading.)

## Risks / tradeoffs (draft)

- **Fix 1(a) changes a shared parse path** (`parse_config_string_as_markdown`
  feeds every blessed config key). Unwrapping lone figures there could change
  snapshot output for other keys — e.g. a `website.title` or `about` text that
  is a lone image (unlikely but possible). Snapshot churn must be reviewed per
  the CLAUDE.md snapshot policy. Note `!md`-tagged values take a different
  path (already `PandocBlocks` at load); decide whether they get the same
  unwrap or keep figure semantics.
- **The lone-figure unwrap must not fire for genuinely block contexts** — the
  same pampa parse is used for document metadata (`!md` in frontmatter?);
  verify the unwrap lives only on the config-string path (or is behaviorally
  safe everywhere).
- **Both related strands are `in_progress`** — f4th80mj (item-text rendering)
  and fc5pvkcv (path design) touch the same files; coordinate to avoid
  conflicting in-flight edits.
- The empty-caption edge (`![](x)`) does *not* desugar to Figure (postprocess
  requires non-empty caption) — tests should pin both variants.

## End-to-end verification (investigation)

Run at HEAD (`main` @ 5b6774d1, 2026-08-18), pre-flight
`cargo xtask verify --skip-hub-build` green first. Output inspected directly.

- Invocation: `cargo run --bin q2 -- render claude-notes/plans/page-footer-image-items-investigation/repro`
  ("Rendered 2 of 2 files", exit 0, **no diagnostics** — confirming the
  silent-failure claim).
- Observed footer in `_site/deep/deeper/index.html` (page two levels deep):

```html
<div class="nav-footer-left">
  <ul class="nav footer-items">
    <li class="nav-item"></li>                                     <!-- lone image: DROPPED (defect 1) -->
    <li class="nav-item"></li>                                     <!-- lone image, relative: DROPPED -->
    <li class="nav-item"><a href="https://posit.co"><img src="images/logo.svg" alt="wrapped in a link"></a></li>  <!-- survives but src unrebased -->
    <li class="nav-item"><img src="images/logo.svg" alt="image"> beside text</li>                                 <!-- survives but src unrebased -->
    <li class="nav-item"><img src="/images/logo.svg" alt="raw html"></li>
  </ul>
</div>
<div class="nav-footer-center"><img src="../../images/logo.svg" alt="region-level"> and a <a href="../../index.html">region-level link</a></div>  <!-- CONTROL: fully resolved -->
<div class="nav-footer-right">
  <ul class="nav footer-items">
    <li class="nav-item"><img src="/images/logo.svg" alt="item-level"> and an <a href="/index.qmd">item-level link</a></li>  <!-- defect 2: untouched -->
  </ul>
</div>
```

Matches the strand's tables exactly: lone-image items render empty; item-level
targets are untouched (root-absolute stays root-absolute, relative stays
page-relative-wrong, `.qmd` never becomes `.html`); the region-level control
in the same footer is fully resolved.
