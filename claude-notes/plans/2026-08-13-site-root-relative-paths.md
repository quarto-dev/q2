# Site-root-relative paths — three cases, and what replaces Q1's deno-dom rewrite (bd-root-relative-paths-design-fc5pvkcv)

**Date:** 2026-08-13
**Braid:** bd-root-relative-paths-design-fc5pvkcv (type: question, priority 1, label: websites)
**Checkout:** main @ `81d31cbc` (investigation committed to `main`; implementation branch TBD by user)
**Status:** Design aligned 2026-08-13 (decisions below). **Awaiting explicit go-ahead before implementation.**

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

- **Phase 0 — Test plan (TDD, failing tests first).**
  - `link_rewrite`: image target root-absolute → page-relative;
    relative `../` chain → normalized; external / `data:` / `//` /
    fragment untouched; query/fragment tails preserved.
  - Navbar logo at depth: unit (generate + render transforms) and
    end-to-end (`render_document_to_file`-level) — logo src
    page-relative at depth 2; missing logo file warns; logo copied.
  - Footer text regions: markdown image renders as `<img>` with
    page-relative src (root-absolute and relative sources); markdown
    link resolves `.qmd` → page-relative `.html`; invert test 42's
    expectation deliberately.
  - Resource collector: `/images/x.svg` in body collects a copy intent
    anchored at project root.
  - In-tree e2e fixture mirroring the repro (render through the real
    binary path; inspect output).
- **Phase 1 — Case B + collector.** Route `Image::target.0` through
  `resolve_static_resource_href` in `LinkRewriteTransform` (all
  targets, per decision 2); correct the module docs; make
  `resource_collector` treat leading `/` as project-root and collect.
- **Phase 2 — Case A.** Pair `logo` with `SourceInfo` (bd-qor9a
  pattern), resolve via `resolve_metadata_path` (generate) +
  page-relative resolution (render); copy logo to output with
  missing-file warning (decision 5), sharing the favicon copy shape.
- **Phase 3 — Case C (incentive removal).** `Inline::Image` arm in
  `push_inline` (attr passthrough); shared resolver-aware inline walk
  in quarto-core applied to footer Text regions (and navbar
  title/text regions) before the pure emitters: Links via
  `resolve_doc_relative_href`, Images via
  `resolve_static_resource_href`; register copy intents +
  missing-file warnings for config-declared images (decision 5).
- **Phase 4 — Docs + follow-ups.** Website docs: path portability
  (leading-`/` decree, markdown-first guidance for footer imagery,
  `project.resources:` for raw-HTML-referenced assets, raw-HTML
  limitation stated plainly). File discovered-from strands for the two
  listing leading-`/` sites. Update the Connect-docs-side repro READMEs
  (outside this repo) after landing.

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
