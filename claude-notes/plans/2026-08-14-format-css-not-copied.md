# format.html.css files are neither copied into the site nor rebased per page (bd-format-css-not-copied-crn3bjdz)

**Date:** 2026-08-14
**Braid:** bd-format-css-not-copied-crn3bjdz (bug, p1, label `websites`)
**Checkout:** main checkout, branch `main` @ `10d86829` (investigation only — no worktree/branch created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** End-to-end website render tests driving the
  real project pipeline: (a) `_site/styles.css` exists after render;
  (b) extension-path css exists at the decided output location; (c) deep
  page's `<link href>` is depth-correct; (d) missing declared css warns
  (and root page still renders); (e) external URL entries pass through
  untouched. Verify each fails at HEAD first.
- **Phase 1 — Copy.** `copy_format_css` in `website_post_render.rs` on the
  `copy_asset_file` seam; iterate merged `format.html.css` entries; skip
  external URLs; warn on missing source (favicon parity).
- **Phase 2 — Rebase.** Per-page depth-correct hrefs via the chosen
  mechanism (design question 2).
- **Phase 3 — Diagnostic.** Warning for missing declared css; decide plain
  warning vs. Q-code (a Q-code requires the docs page in the same commit —
  `error-docs-page-missing` lint).
- **Phase 4 — End-to-end verification + docs.** Render the fixture through
  `cargo run --bin q2 -- render`, inspect output, re-check the Connect docs
  repro; user-facing docs if any behavior is documented.

## Open design questions for the user

1. **Output layout for css under `_extensions/`.** Q1 relocates
   extension-owned css to
   `site_libs/quarto-contrib/quarto-project/<org>/<ext>/…` and rewrites the
   href accordingly. The simpler alternative is to copy preserving the
   project-relative path (`_site/_extensions/acme/widget/widget.css`) and
   emit the rebased href to that. Q1 parity, or the simpler layout? (Q1
   avoids copying `_extensions/` into output wholesale; do we care about
   that convention here?)
2. **Rebase mechanism.** Two candidates with in-tree precedent:
   (a) mark project-config `format.*.css` entries as `ConfigValueKind::Path`
   (existence-driven, like extension fragments) and let the existing
   `adjust_paths_to_document_dir` merge machinery emit document-relative
   values — near-zero new code, but couples href shape to the input tree
   mirroring the output tree; or (b) resolve at template/transform time via
   the per-page `ResourceResolverContext`, like the favicon and theme
   bundle — more explicit, and the same seam a later `site_libs` relocation
   (question 1) would need anyway. Which?
3. **Scope beyond `css`.** The same boundary presumably affects sibling
   config-declared assets (`include-in-header` files are inlined so likely
   fine, but what about `format-resources`, user `js`/`scripts` entries, and
   document-level `css:` declared in a subdirectory page's front matter —
   resolved against the document dir?). Fix `css` only here and file
   follow-ups, or audit the boundary in this strand?
4. **Diagnostic shape.** Favicon uses a plain `DiagnosticMessage::warning`.
   Same here, or mint a Q-code (requires `docs/errors/` page + catalog entry
   in the same commit)?
5. **Preview parity.** `website_post_render` hooks are native-only. Does the
   in-browser preview / `q2 preview` serve declared css from the source tree
   already (VFS), or does preview need its own leg? (Fine to answer "verify
   during Phase 4 and file a follow-up if broken.")

## Risks / tradeoffs (draft)

- **Book projects** likely share the symptom (same template path); fixing
  only the website post-render hook would leave books broken. Worth checking
  where books' post-render lives before committing to the seam.
- Choosing rebase mechanism (a) silently changes the merged metadata value
  shape (`Scalar` → `Path`) for a user-visible key; downstream readers of
  `css` metadata (Lua filters, template contexts) would observe rewritten
  values. Mechanism (b) keeps metadata untouched.
- The design strand bd-root-relative-paths-design-fc5pvkcv is still
  in_progress; its remaining case C (raw HTML) is independent, but any
  decision here should cite its Decision 4/5 vocabulary (leading `/` =
  site-root-relative; config-declared assets get warn-and-continue copy) to
  stay consistent.
