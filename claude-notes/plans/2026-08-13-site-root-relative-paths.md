# Site-root-relative paths — three cases, and what replaces Q1's deno-dom rewrite (bd-root-relative-paths-design-fc5pvkcv)

**Date:** 2026-08-13
**Braid:** bd-root-relative-paths-design-fc5pvkcv (type: question, priority 1, label: websites)
**Checkout:** main @ `81d31cbc` (investigation committed to `main`; implementation branch TBD by user)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

**Session constraint (from the user, at investigation kickoff):** we must
not end up parsing HTML to rewrite root-relative paths. Instead, watch
for the things that *incentivize* users to reach for raw HTML nodes. If
those incentives go away and markdown is the natural path, the
unparsed-raw-HTML limitation stops mattering.

## Triage verdict

**Ready to design.** All three cases reproduce at HEAD `81d31cbc`
exactly as the strand describes; the machinery for Cases A and B already
exists with the right documented rationale
(`resolve_static_resource_href`, `resolve_metadata_path`); and the
investigation found a concrete, removable raw-HTML incentive that
reframes Case C in line with the session constraint: **the navbar/footer
inline renderer cannot render `Inline::Image` at all**, so markdown
images in config-declared regions silently flatten to alt text — which
is what forces authors into `` `<img …>`{=html} `` in the first place.

## Issue context

Filed 2026-08-13 from the Posit Connect docs porting project (origin
strands `br-navbar-logo-path-t9s5ppqf`, `br-root-absolute-images-sh3mtuwt`,
`br-root-absolute-assets-1o6yy4mx` in that project's skein). Three
defects share one symptom — a path that survives into rendered output
unchanged, works at a domain root, 404s under a deploy subpath (the
Connect docs deploy at `https://docs.posit.co/connect/`, a subdirectory):

- **Case A** — `website.navbar.logo` is emitted verbatim at every page
  depth; `logo-href` right next to it is fully resolved.
- **Case B** — root-absolute paths rebase for markdown *links* but not
  markdown *images*; the exclusion comment in `link_rewrite.rs` asserts
  Q1 parity it does not have (Q1 rewrites image paths too).
- **Case C** — root-absolute paths inside raw HTML (footer logos, cookie
  icons, a custom `quarto-tiers` badge href in the Connect docs). Q1
  catches these by parsing rendered output with deno-dom; q2 will not
  parse HTML, so the question is what replaces that — possibly "nothing,
  plus a diagnostic".

The strand argues (correctly, I think) that fixing A and B without
settling C produces a site that is *almost* portable, which is worse
than one that is visibly not.

## Dependency graph

**Empty in this skein.** No `blocks`, `related`, or `discovered-from`
edges — the origin strands live in the Connect docs project's separate
skein and are documented in the description instead. No incoming
pressure from other q2 strands; the urgency comes from the Connect docs
port (production 404s that survived six release sweeps unseen).

## What the code looks like today (all verified at HEAD 81d31cbc)

Every file/line the strand cites still exists with the described shape.
Both repros re-run and reproduce (see § Repro).

**Case A — navbar logo:**
- `crates/quarto-navigation/src/navbar.rs:106-112` — `logo: Option<String>`
  has no paired `SourceInfo`; `logo_href` + `logo_href_source` sit right
  below it (bd-qor9a pattern).
- `crates/quarto-core/src/transforms/navbar_generate.rs:89-92` —
  `resolve_metadata_path` on `logo_href` only.
- `crates/quarto-core/src/transforms/navbar_render.rs:98-104` —
  `resolve_href_for_html` on `logo_href` only.
- `crates/quarto-navigation/src/render_html.rs:313-322` — the `<img>` is
  emitted with `escape_attr(logo)` verbatim, no resolution.

**Case B — markdown images:**
- `crates/quarto-core/src/transforms/link_rewrite.rs:216-238` — `Link`
  targets go through `resolve_doc_relative_href`; `Image` targets are
  deliberately skipped (only alt-text inlines are walked). Module docs
  lines 29-30 carry the factually wrong "(Q1 doesn't rewrite them
  either)" parenthetical — must be corrected regardless of design outcome.
- `crates/quarto-core/src/transforms/navigation_href.rs:382`
  (`resolve_static_resource_href`) is the exact helper this needs — no
  index lookup, no `.qmd` diagnostic, and its doc comment's stated
  purpose is precisely this portability property. Sole external caller:
  `example_embed.rs:386`.
- `crates/quarto-core/src/transforms/navigation_href.rs:505-516`
  (`resolve_to_project_root`) already treats a leading `/` as
  project-root-absolute ("Q1 parity" comment in place).

**Case C — raw HTML, and the incentive discovered during investigation:**
- `crates/quarto-navigation/src/footer.rs` — footer regions preserve
  markdown as `ConfigValue`/`PandocInlines`; the AST flows intact into
  rendering. `FooterRenderTransform`
  (`crates/quarto-core/src/transforms/footer_render.rs`) runs per page
  **with `ctx.resource_resolver` in hand** and already rewrites item
  hrefs per page.
- `crates/quarto-navigation/src/render_html.rs:746-843` (`push_inline`)
  renders `RawInline` html **verbatim** (line 819-824) and has **no
  `Inline::Image` arm** — images fall into the plain-text fallback
  (lines 834-841) and flatten to alt text. So an author who writes
  `![](/images/logo.svg)` in `page-footer.left` gets *text*, not an
  image. **This is the removable incentive:** the only way to put an
  image in a footer region today is raw HTML, and raw HTML is exactly
  where paths can't be rebased. All four `src=` offenders in the Connect
  docs are this pattern.
- Asset-copy boundary: `copy_favicon` in
  `crates/quarto-core/src/project/website_post_render.rs:75` is the
  **only** config-declared asset copy (with a missing-file warning);
  navbar logo gets neither copy nor warning. The copy side of the
  raw-HTML blind spot has a supported answer (`project.resources:`);
  only the URL side is stuck.
- The `quarto-tiers` badge href is a custom metadata key consumed by
  that project's project type — no q2-owned AST node exists for it, so
  neither A- nor B-style fixes reach it. It needs either option 1 (a
  site-root-relative config marker q2 rebases) or a documented answer.

## Repro

Minimal repro committed at
`claude-notes/plans/site-root-relative-paths-investigation/repro/`
(trimmed copy of the strand's
`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/root-absolute-paths/`).
At HEAD, `cargo run --bin q2 -- render <repro>` then inspecting
`_site/deep/deeper/index.html` (page two levels down) shows:

| construct | Q1 | q2 @ 81d31cbc |
|---|---|---|
| `[root](/index.qmd)` | `../../index.html` | `../../index.html` ✓ |
| `![](/images/x.svg)` | `../../images/x.svg` | **`/images/x.svg`** |
| `<img src="/images/x.svg">` | `../../images/x.svg` | **`/images/x.svg`** |

Case A (navbar `logo: images/config-logo.svg`, second repro): the deep
page emits `<img src="images/config-logo.svg" …>` — verbatim, wrong at
any depth > 0. Output inspected directly; both renders done through the
real `q2 render` binary.

## Proposed phases (draft — contents wait on the design discussion)

- **Phase 0 — Test plan (TDD).** Failing tests first: navbar logo at
  depth (unit + `render_document_to_file`-level); `link_rewrite` image
  target cases (root-absolute, relative-untouched, external-untouched,
  query/fragment); footer-region markdown image renders as `<img>` with
  page-relative src; in-tree e2e fixture mirroring the repro.
- **Phase 1 — Case B.** Route `Image::target.0` through
  `resolve_static_resource_href` in `LinkRewriteTransform`; correct the
  module-docs parenthetical.
- **Phase 2 — Case A.** Pair `logo` with a `SourceInfo` (bd-qor9a
  pattern), resolve via `resolve_metadata_path` at generate time and
  page-relative at render time, mirroring `logo_href`.
- **Phase 3 — Case C mechanism (per design decision).** Likely: render
  `Inline::Image` in `push_inline` with per-page src resolution, making
  markdown the natural form for footer/navbar imagery; plus whatever
  diagnostic/marker option the design session settles.
- **Phase 4 — Docs.** Website docs on path portability (what to write
  where, `project.resources:` for copying); error-catalog page in the
  same commit if a Q-code is added; update repro READMEs upstream.

## Open design questions for the user

1. **Footer/navbar images (the incentive removal).** Should
   `push_inline` grow an `Inline::Image` arm (emitting `<img>` with
   attr passthrough), with the src resolved per page? If yes, where does
   resolution happen — a resolver-aware walk in `FooterRenderTransform`
   before `page_footer_to_html`, or plumbing the resolver into
   `inlines_to_html`? (The former keeps `render_html.rs` pure.) Note
   this also changes navbar `text:` regions that contain images —
   acceptable?
2. **Case B scope.** Rebase *only* root-absolute image targets, or route
   every image target through `resolve_static_resource_href` (which also
   normalizes `../` chains — a behavior change for currently-working
   relative paths, though output should be equivalent)? Q1 rewrites all;
   minimal-change says leading-`/` only.
3. **Diagnostic for surviving root-absolute paths (option 3).** Do we
   want a Q-code warning when a root-absolute path reaches output from
   raw HTML? Honest detection requires at least a substring scan of
   `RawInline`/`RawBlock` text for `src="/`/`href="/` — is
   diagnostic-only substring scanning acceptable under the no-HTML-parsing
   principle, or do we skip detection entirely and rely on docs?
   (Warning suppression via `diagnostics:` exists for root-deployed sites.)
4. **Site-root-relative config marker (option 1).** Does q2 want a
   general facility for marking a config-declared path as
   site-root-relative and rebasing per page — the only option that
   reaches the custom `quarto-tiers` metadata case — or is that the
   project type's problem, documented as such?
5. **Asset-copy boundary.** Is favicon deliberately special, with
   `project.resources:` as the documented answer for everything else
   (navbar logo, footer images), or should config-declared assets q2
   knows about (logo, footer images) also be copied with a
   missing-file warning like favicon's?

## Risks / tradeoffs (draft)

- **Rendering images in nav/footer text regions changes existing
  output** for any site that (perhaps unknowingly) has an image
  flattening to alt text today. Probably a fix, not a regression, but
  it is user-visible.
- `push_inline` is shared by navbar titles, footer regions, and sidebar
  text — an `Image` arm affects all of them; needs a survey of callers.
- Case B touches every website render; snapshot churn expected. Any
  snapshot diffs must be itemized per the CLAUDE.md snapshot policy.
- The two full repros live outside the repo (in the Connect docs
  checkout); the in-tree copy under the investigation dir is the
  durable version — keep e2e fixtures in-tree only.
- Pre-flight note: `cargo xtask verify --skip-hub-build` initially
  failed on stale locally-built tree-sitter artifacts; after
  `tree-sitter generate && tree-sitter build` in
  `crates/tree-sitter-qmd/tree-sitter-markdown` it passes green at this
  HEAD. Not related to this strand.
