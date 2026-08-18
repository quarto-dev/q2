# Site-root-relative paths — three cases, and what replaces Q1's deno-dom rewrite (bd-root-relative-paths-design-fc5pvkcv)

**Date:** 2026-08-13
**Braid:** bd-root-relative-paths-design-fc5pvkcv (type: question, priority 1, label: websites)
**Checkout:** main @ `81d31cbc` (investigation committed to `main`; implementation branch TBD by user)
**Status:** MERGED & CLOSED 2026-08-18. All four phases on `main`
(`1d17a9ce`, `0b4683fc`, `5da1e5f4`, `8bdb97c6`). Remaining field
breakage (page-footer item images dropped/unrebased) tracked in
bd-page-footer-image-items-stmpikgo.

**Session constraint (from the user, at investigation kickoff):** we must
not end up parsing HTML to rewrite root-relative paths. Instead, watch
for the things that *incentivize* users to reach for raw HTML nodes. If
those incentives go away and markdown is the natural path, the
unparsed-raw-HTML limitation stops mattering.

## Design decisions (user-aligned, 2026-08-13)

1. **Footer/navbar images: yes.** `push_inline` grows an
   `Inline::Image` arm (emit `<img>` with attr passthrough). Resolution
   happens in the quarto-core render transforms **before** the pure
   emitter runs (Option A — assessment below).
2. **Case B: rewrite ALL image targets** through
   `resolve_static_resource_href`, not just root-absolute ones.
   `../`-chain normalization is a desired side effect. Scope growth
   accepted: improving path handling generally is the point of the
   session.
3. **No diagnostic for root-absolute paths surviving in raw HTML.** A
   substring scanner would inevitably accrete bug reports whose fixes
   require real HTML parsing — the exact slippery slope this session
   exists to avoid. Docs carry the message instead.
4. **Decree: a Quarto path with a leading `/` means site-root-relative,
   uniformly.** (A future "raw path" affirmative marker may give the
   opposite escape hatch; out of scope here.) Survey of current
   leading-`/` interpretations below — three sites deviate, none for a
   principled reason.
5. **Favicon is not special.** Config-declared assets q2 knows about
   (navbar logo, footer/navbar images) get the same copy-with-
   missing-file-warning treatment `copy_favicon` pioneered.

### Q1 assessment: why resolution lives in the transforms (Option A)

- **Crate boundary (decisive).** `quarto-navigation` depends only on
  `quarto-pandoc-types`; `ResourceResolverContext` and `ProjectIndex`
  live in `quarto-core` (which depends on quarto-navigation, not vice
  versa). Plumbing the resolver into `inlines_to_html` would need a new
  callback/trait abstraction across the boundary. Resolving before the
  emitter needs nothing new.
- **Established pattern.** `FooterRenderTransform` already rewrites
  Items-region hrefs (`rewrite_items_hrefs` → `resolve_href_for_html`)
  and `NavbarRenderTransform` already rewrites `logo_href`, both before
  handing a fully-resolved struct to the pure emitter. Adding a
  PandocInlines walk to the same transforms is the same idea one level
  deeper.
- **Purity/testability.** `render_html.rs` stays a pure
  data-in-HTML-out module.
- **Bonus fix (confirmed end-to-end at HEAD).** Footer Text regions are
  today *never* link-rewritten — pinned by `footer_render.rs` test 42
  ("Text regions pass through; body-content rewriting is Phase 6") but
  Phase 6 (`link_rewrite`) walks the AST body and never sees
  `rendered.navigation.footer`. Real render:
  `page-footer.left: "![logo](/images/x.svg) and [root](/index.qmd)"`
  emits `footer-left">logo and <a href="/index.qmd">` on a deep page —
  image flattened to alt text, link still `.qmd` and still absolute.
  The same resolver-aware walk fixes both (Links via
  `resolve_doc_relative_href`, Images via
  `resolve_static_resource_href`). Test 42's pinned expectation gets
  inverted deliberately.

### Q4 survey: current leading-`/` interpretations

**Already consistent with the decree:**
- `resolve_to_project_root` (`navigation_href.rs:505`) — leading `/` =
  project root ("Q1 parity" comment).
- `resolve_include_target` (`include_expansion.rs:640`) — "`/a/b.qmd`
  means `<project>/a/b.qmd`, never the filesystem root" (documented,
  matches Q1 `resolvePath`).
- `normalize_favicon_path` (`website_config.rs:90`) — strips the
  leading `/`, i.e. favicon already implements the decree.

**Deviating sites (all conservative defaults, none principled):**
- `resource_collector.rs:424` — refuses to collect `/`-prefixed URLs
  for copying ("filesystem-root-anchored… we shouldn't copy
  `/etc/passwd`"). Under the decree: resolve to `<project>/…` and
  collect. No traversal risk — anchoring at project root with
  stack-based `..` popping cannot climb above the root, so
  `/etc/passwd` probes `<project>/etc/passwd` (normally absent, and
  then subject to the normal missing-resource behavior).
- `listing/helpers.rs:93` (`is_external_src`) — classifies `/`-src as
  external (skips listing preview rebase / copy-intent registration).
- `listing/post_render_upgrade/substitute.rs:341` — passes `/`-prefixed
  preview src through verbatim ("rare in Q2 output").

**Legitimate carve-outs the decree must not swallow:**
- `//host/x` protocol-relative URLs — already external
  (`is_external`, `navigation_href.rs:212`; also guarded in favicon
  resolution).
- `data:` URIs (handled in `is_external_src`).
- WASM VFS paths (`/project/...`) — a *filesystem-space* internal
  convention, not URL-space; no collision because URL-space
  normalization to project-root-relative happens before any VFS lookup
  (`resolve_static_resource_href` → `page_url_for`).
- OS-absolute *input-file* paths (Path::join semantics on
  filesystem-space config like a hypothetical absolute `bibliography:`)
  are filesystem-space, not URL-space; the decree governs URL space.
  `is_rooted` (`quarto-util/src/path.rs`) exists for the
  Windows-correct rooted check where needed.

**Conclusion:** no site has a reason to read a URL-space leading `/` as
anything but site-root-relative. The collector deviation is the
copy-side twin of Case B and belongs in this session; the two
listing-specific sites are separable follow-ups (filed as
discovered-from strands at implementation time unless the user pulls
them in).

## Triage verdict

**Ready to implement** (design aligned; awaiting go-ahead). All three
cases reproduce at HEAD `81d31cbc`; machinery exists; decisions above.

## Issue context

Filed 2026-08-13 from the Posit Connect docs porting project (origin
strands `br-navbar-logo-path-t9s5ppqf`, `br-root-absolute-images-sh3mtuwt`,
`br-root-absolute-assets-1o6yy4mx` in that project's skein). Three
defects share one symptom — a path that survives into rendered output
unchanged, works at a domain root, 404s under a deploy subpath (the
Connect docs deploy at `https://docs.posit.co/connect/`):

- **Case A** — `website.navbar.logo` emitted verbatim at every depth;
  `logo-href` beside it is fully resolved.
- **Case B** — root-absolute paths rebase for markdown links but not
  markdown images; `link_rewrite.rs` module docs falsely claim Q1
  parity for the exclusion.
- **Case C** — root-absolute paths inside raw HTML. Q1 catches these
  with deno-dom; q2 will not parse HTML. Resolution: remove the
  incentive (markdown images become first-class in nav/footer regions)
  rather than replace the rewriter.

## Dependency graph

**Empty in this skein.** Origin strands live in the Connect docs
project's skein; documented in the description. No incoming q2
pressure; urgency comes from the Connect docs port.

## What the code looks like today (verified at HEAD 81d31cbc)

**Case A — navbar logo:**
- `crates/quarto-navigation/src/navbar.rs:106-112` — `logo` has no
  paired `SourceInfo`; `logo_href` + `logo_href_source` sit below it.
- `crates/quarto-core/src/transforms/navbar_generate.rs:89-92` —
  `resolve_metadata_path` on `logo_href` only.
- `crates/quarto-core/src/transforms/navbar_render.rs:98-104` —
  `resolve_href_for_html` on `logo_href` only.
- `crates/quarto-navigation/src/render_html.rs:313-322` — `<img>`
  emitted with `escape_attr(logo)` verbatim.

**Case B — markdown images:**
- `crates/quarto-core/src/transforms/link_rewrite.rs:216-238` — `Link`
  resolved; `Image` deliberately skipped; module docs lines 29-30 wrong
  about Q1.
- `resolve_static_resource_href` (`navigation_href.rs:382`) is the
  right helper; sole external caller `example_embed.rs:386`.

**Case C — raw HTML and the removable incentive:**
- Footer regions preserve markdown as `PandocInlines`
  (`quarto-navigation/src/footer.rs`); `FooterRenderTransform` runs per
  page with `ctx.resource_resolver`, rewrites Items hrefs, and leaves
  Text regions untouched (test 42 pins this).
- `push_inline` (`render_html.rs:746-843`) renders `RawInline` html
  verbatim and has no `Image` arm — images flatten to alt text. This is
  what forces footer imagery into `` `<img …>`{=html} ``. All four
  `src=` offenders in the Connect docs are this pattern.
- Copy side: `copy_favicon`
  (`website_post_render.rs:75`) is the only config-declared asset copy;
  `resource_collector.rs` refuses `/`-prefixed URLs (see survey).
- The `quarto-tiers` badge (custom metadata consumed by a custom
  project type) is covered by decision 4: q2-owned path resolution
  treats its leading `/` as site-root when that project type resolves
  it through q2's helpers; nothing further this session.

## Repro

Minimal repro committed at
`claude-notes/plans/site-root-relative-paths-investigation/repro/`.
At HEAD, `cargo run --bin q2 -- render <repro>` then inspecting
`_site/deep/deeper/index.html`:

| construct | Q1 | q2 @ 81d31cbc |
|---|---|---|
| `[root](/index.qmd)` | `../../index.html` | `../../index.html` ✓ |
| `![](/images/x.svg)` | `../../images/x.svg` | **`/images/x.svg`** |
| `<img src="/images/x.svg">` | `../../images/x.svg` | **`/images/x.svg`** |
| footer text `![logo](/images/x.svg)` | `<img>` rebased | **alt text only** |
| footer text `[root](/index.qmd)` | rebased `.html` | **`/index.qmd` verbatim** |

Case A (second repro, navbar `logo: images/config-logo.svg`): deep page
emits `<img src="images/config-logo.svg">` verbatim.

## Phases

TDD per phase: each phase writes its failing tests first, verifies they
fail, implements, verifies green, then runs the full workspace suite.

### Phase 1 — Case B (markdown images) + collector

Implementation notes settled during pre-implementation reading:
- Pipeline order is `LinkRewriteTransform` (first in Finalization) →
  `ResourceCollectorTransform` (late Finalization), so in resolver-ful
  modes the collector sees already-rebased page-relative image URLs.
  The collector still gets its own leading-`/` handling (anchored at
  `ctx.project.dir` / `ctx.project.output_dir`) so its correctness does
  not depend on transform ordering, and so
  `collect_referenced_asset_urls` (preview single-file asset sync,
  bd-kpuweafo — runs on raw pre-transform blocks with empty anchors)
  implements the decree too.
- `LinkRewriteTransform`'s standalone short-circuit becomes
  "no index AND no resolver": Link rewriting still requires the index
  (Decision 7 of phase-6 unchanged); Image rewriting only needs the
  resolver. In single-doc mode `page_url_for` collapses `/x.png` to
  `x.png` — decree-correct (project root = doc dir).
- `is_external` gains `data:` (a data URI is URL-shaped, not
  path-shaped; today it would be mangled by path normalization).
  `resolve_static_resource_href` gains an empty-path guard.

Work items:
- [x] Failing tests: `link_rewrite` image cases (root-absolute →
      page-relative at depth; `../` normalization; external / `data:` /
      fragment-only untouched; `#`/`?` tails preserved; image rewrite
      without index when resolver present) — 7 failed as expected
      before implementation
- [x] Failing tests: `is_external("data:…")`; collector root-absolute
      anchored at project root; `collect_referenced_asset_urls`
      leading-`/` inversion (deliberate expectation change)
- [x] Implement: `is_external` + `data:`; empty-path guard in
      `resolve_static_resource_href`
- [x] Implement: `link_rewrite` Image arm + gating change (`index`
      now `Option`; short-circuit only when index AND resolver are
      both absent) + module-docs correction
- [x] Implement: collector leading-`/` anchoring via `ctx.project`
      (`ResourceVisitor` gained root anchors; empty anchors preserve
      `collect_referenced_asset_urls` semantics)
- [x] **Discovered + fixed en route:** `render_document_to_file` never
      canonicalized its input while `ProjectContext::discover` does,
      so a symlinked input (macOS `/var/folders` tempdirs) put
      `output_path` and project roots on different spellings and
      `page_url_for`'s pathdiff emitted `../..`-laden URLs escaping
      the site. Surfaced by `shortcode_text_contexts::image_src_substitutes`
      the moment image targets first routed through pathdiff. Fixed
      with defensive `runtime.canonicalize` at entry (idempotent for
      the CLI, which pre-canonicalizes). Also improved that test's
      assertion debug output (`<img` line, not first `src=` line).
- [x] e2e: rendered the in-tree repro fixture through
      `cargo run --bin q2 -- render`; deep page now emits
      `src="../../images/x.svg"` for the markdown image (was
      `/images/x.svg`), `href="../../index.html"` for the link
      (unchanged), raw-HTML `src="/images/x.svg"` untouched (Case C,
      by design). Copy side verified with `project.resources:`
      removed: `_site/images/x.svg` still created by the collector.
      Output inspected directly.
- [x] Full workspace tests + commit (`1d17a9ce`, merged to main)

### Phase 2 — Case A (navbar logo)

- [x] Failing tests (6 failed as expected before implementation):
      `navbar_render` units (logo rebased at depth, leading-`/` form,
      external passthrough); pipeline tests (per-page rebase at root +
      depth 2, copy without `project.resources`, leading-`/` variant);
      missing-file warning; `_metadata.yml`-authored logo resolving
      against the authoring dir (the one fixture that *discriminates*
      generate-time resolution — same-page frontmatter cases collapse
      to identity); `logo_source` capture + round-trip unit in
      quarto-navigation
- [x] Implement: `logo_source: SourceInfo` pairing in
      `quarto-navigation` (`navbar.rs`, capture in `from_config_value`,
      round-trip in `to_config_value`)
- [x] Implement: `resolve_metadata_path` on `logo` in
      `navbar_generate`; new `resolve_root_relative_resource_href`
      helper in `navigation_href` (delegates to the static helper with
      empty source — config paths are project-root-relative by
      convention) applied in `navbar_render`
- [x] Implement: `copy_navbar_logo` beside `copy_favicon` in
      `website_post_render` (decision 5), shared `copy_asset_file`
      tail; registered in the orchestrator post-render hooks
- [x] e2e (real binary): deep page emits
      `<img src="../../images/config-logo.svg" … class="navbar-logo">`
      (was verbatim `images/config-logo.svg`); logo copied to
      `_site/images/` without `project.resources`. Output inspected.
- [x] Full workspace tests (11839 green, no snapshot churn) + commit
      `0b4683fc`

### Phase 3 — Case C (incentive removal: images + links in nav/footer regions)

- [x] Failing tests (9 failed as expected before implementation):
      `push_inline` Image arm (attr passthrough, alt from content,
      title, escaping); footer Text region markdown image →
      page-relative `<img>` at depth (site-root and project-root
      forms); footer Text region `.qmd` link → page-relative `.html`
      (inverts footer_render test 42 — deliberate, documented in the
      test); leading-`/` nav href resolves (`/about.qmd` ≡
      `about.qmd`); navbar title image parity; footer image copy +
      missing-file warning pipeline tests
- [x] Implement: `Inline::Image` arm in
      `quarto-navigation/src/render_html.rs::push_inline` (src emitted
      verbatim — the emitter stays resolver-free; new
      `inlines_plain_text` helper for alt flattening)
- [x] Implement: `rewrite_config_inlines` walk in `navigation_href`
      (Links via `resolve_href_for_html` — config-space semantics,
      surface-tagged Q-13 diagnostics; Images via
      `resolve_root_relative_resource_href`), applied in
      `FooterRenderTransform` Text regions and the navbar title.
      `resolve_href_for_html` now strips a leading `/` before the
      index lookup (decision 4). Sidebar title / item-text regions
      deliberately not walked (items are already links; sidebar title
      is a plain-text surface) — noted here as the exclusion record.
- [x] Implement: `copy_footer_images` post-render hook (decision 5)
      beside favicon/logo; parses the project-config scalar with
      `pampa::pandoc::meta::parse_config_string_as_markdown` (the
      `ConfigMarkdownTransform` entry point) because markdown-izing
      happens per-doc, not at config load; warn-and-continue on
      missing files
- [x] e2e (real binary): deep page footer emits
      `<img src="../../images/x.svg" alt="logo"> and
      <a href="../../index.html">root</a>` from
      `page-footer.left: "![logo](/images/x.svg) and [root](/index.qmd)"`
      (was: alt-text flattening + verbatim `/index.qmd`); image copied
      to `_site/images/`. Output inspected. This is the markdown-native
      replacement for all four raw-HTML `src=` offenders in the
      Connect docs.
- [x] Full workspace tests (11847 green, no snapshot churn) + commit
      `5da1e5f4`

### Phase 4 — Docs + follow-ups

- [x] Website docs: new `docs/guides/projects/paths.qmd` ("Paths in
      websites") — the two path forms, per-page rewriting,
      config-declared assets with markdown-first footer guidance, the
      raw-HTML non-rewriting stance stated plainly, `project.resources:`
      for raw-HTML-referenced assets, full-URL caveats. Added to the
      docs sidebar; site renders clean (page verified in `_site`,
      rendered with q2).
- [x] File discovered-from strands:
      - `bd-epk8cmdl` — listing leading-`/` sites (`is_external_src`,
        preview-URL substitution)
      - `bd-tef2lm9j` — nav hrefs to static (non-document) files never
        page-relativize (`resolve_href_for_html` fall-through)
      - `bd-i8k7h6h1` — reveal deck footer/logo regions not walked
        (p3)
- [x] Fixed stale `pipeline.rs` doc comment attributing image-URL
      rewriting to `ResourceCollectorTransform` (it's
      `LinkRewriteTransform`).
- [x] **Discovered + fixed by the full-verify WASM leg:** the Phase 1
      image rewrite broke the q2-preview asset contract. In VFS-root
      mode (hub-client preview) images are not fetched by URL — the
      parent-side asset walker reads the VFS and mints blob URLs keyed
      by the *user-written* path
      (`hub-client/src/services/assetManifestProject.wasm.test.ts`
      pins this; 2 of its tests + 1 printableDocument test caught the
      break). Image rewriting is now mode-gated off in VFS-root mode
      (same gate `ResourceCollectorTransform` already uses), with a
      Rust regression test
      (`image_rewrite_skipped_in_vfs_root_mode`, failed before the
      gate). Links keep rewriting in VFS mode (bd-kw93.14 behavior,
      unchanged). This is exactly why the CLAUDE.md rule says plain
      workspace tests are not sufficient for quarto-core changes.
- [x] Update Connect-docs-side repro READMEs (outside this repo) —
      done during the 0.21.0/0.22.0 release sweeps over there (both
      READMEs now carry FIXED statuses for the shipped cases).
- [x] Final `cargo xtask verify` (full, WASM leg included): all
      verification steps passed.

## Risks / tradeoffs

- **Behavior changes are user-visible and mostly fixes:** images in
  nav/footer text regions start rendering as images; footer text links
  start resolving; relative image paths get `../` normalization.
  Snapshot churn expected — itemize per CLAUDE.md snapshot policy.
- `push_inline` is shared by navbar titles, footer regions, and sidebar
  text — the `Image` arm affects all; survey callers so every surface
  that can now carry an image also gets the resolver walk (or
  explicitly doesn't, with a comment).
- Collector change relaxes a deliberate refusal; the project-root
  anchor keeps it safe (no path can escape the project root), but
  reviewers should look at it with security eyes anyway.
- Test 42 inversion must be called out in the commit message.
- Pre-flight note: `cargo xtask verify --skip-hub-build` initially
  failed on stale locally-built tree-sitter artifacts; green after
  `tree-sitter generate && tree-sitter build`. Unrelated to this
  strand.
