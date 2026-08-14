# Index-miss href relativization (×2) + website breadcrumbs

**Date:** 2026-08-14
**Braid:**
- bd-tef2lm9j — nav hrefs to static (non-document) files not page-relativized
- bd-root-absolute-dir-link-58eh8834 — body links to directories not page-relativized
- bd-breadcrumbs-missing-1vpuqh34 — website breadcrumbs not rendered (blocked on the two above; the blocks edges encode the required ordering)
- bd-root-relative-paths-design-fc5pvkcv — parent design (related); its decisions 4/5 and helpers govern this session

**Checkout:** main checkout, `main` @ 3ac596e0. User asked for all three fixes in this session, in the order encoded in the graph.

## Triage verdict

**Ready to implement.** All context gathered; the resolver fix direction is
recorded identically on both structural strands and the helper it names
(`resolve_root_relative_resource_href`, `navigation_href.rs:434`) exists on
main. Breadcrumbs are a well-mapped feature add with one scope decision
(recorded below) — no unknowns requiring user input before code, but the
scope decision is flagged for user review in the handoff.

## The one question, two call sites (Phase A)

`crates/quarto-core/src/transforms/navigation_href.rs`:

| function | surface | lookup | miss |
|---|---|---|---|
| `resolve_href_for_html` | nav hrefs | :178 | :205 → `raw.to_string()` |
| `resolve_doc_relative_href` | body links | :334 | :358 → `raw.to_string()` |

Answer (same file, already documented with this exact rationale): on an
**index miss for a non-`.qmd` path, with an index present**, route through
the static-resource helpers instead of returning raw:

- `resolve_href_for_html` miss → `resolve_root_relative_resource_href(raw, resolver)`
  (nav hrefs are project-root-relative; leading `/` means the same thing).
- `resolve_doc_relative_href` miss → `resolve_static_resource_href(raw, source_relative, resolver)`
  (body links are doc-relative).

Deliberately unchanged:
- `.qmd`-shaped misses keep the Q-13 diagnostic **and** the verbatim raw
  return (pinned by existing tests; the dangling link stays visible, and the
  author is being told to fix it).
- No-index (standalone render) branches: verbatim, as today (pinned by
  tests 27/36/44).
- `.md` misses stay silent (bd-6d2wj4zp D6) but now relativize like any
  other static target — relative forms round-trip unchanged; root-absolute
  forms get fixed. This is the point of the change.

Trailing-slash caveat discovered while scoping: `resolve_to_project_root`
drops a trailing `/` (empty segment), but Q1 preserves it (`[dir](/target/)`
→ `../../target/`). `resolve_static_resource_href` must re-append a trailing
slash the normalizer ate, so directory links keep their canonical no-redirect
form.

## Breadcrumbs (Phase B)

Q1 behavior (verified in `external-sources/quarto-cli`):

- Trail derivation (`website-shared.ts::breadCrumbs`): find the sidebar entry
  whose `href` equals the page's href; the trail is that entry plus every
  wrapping section, outermost first, **including the current page as the
  final linked crumb**. A section without an href borrows its **first direct
  child's** href (`contents[0].href`); if that's also absent the crumb is
  unlinked text. (The "first descendant document" phrasing in the strand is
  the common case of that rule, not a deeper search.)
- Two render sites: (1) inside `.quarto-secondary-nav` (the narrow-viewport
  bar), always when enabled; (2) prepended to the title block as
  `nav.quarto-page-breadcrumbs.quarto-title-breadcrumbs.d-none.d-lg-block`,
  only when the trail has **> 1 crumbs**. Markup:
  `<nav … aria-label="breadcrumb"><ol class="breadcrumb"><li class="breadcrumb-item"><a href="…">Text</a></li>…</ol></nav>`.
  In banner mode Q1 prepends inside `.quarto-title-banner .quarto-title`;
  with `title-block-style: none` there is no `.quarto-title-block` and no
  breadcrumbs.
- Config: `website.bread-crumbs`, default **true**; page-level
  `bread-crumbs: false` also honored.

### Scope decision (flagged for user review)

**This session implements the title-block instance only.** The
`.quarto-secondary-nav` container is the whole narrow-viewport navigation
bar — sidebar-collapse toggle (`data-bs-target=".quarto-sidebar-collapse-item"`),
headroom JS hook, search button. q2 today has **none** of that mobile
machinery (no `.quarto-sidebar-collapse-item` markup, no
`quarto-secondary-nav` emitter, near-zero SCSS), so bolting the container on
would emit a toggle wired to nothing. That subsystem gets its own strand
(filed as discovered-from bd-breadcrumbs-missing-1vpuqh34 at close), and the
mobile-instance breadcrumbs land there with it, reusing this session's
trail + renderer.

### Design

- **Trail computation** in `quarto-navigation/src/sidebar.rs`:
  `breadcrumb_trail(&Sidebar, page_source) -> Vec<Crumb>` — pure sibling of
  `resolve_active_state`, same href == page_source matching rule as
  `mark_active_in`, Q1's borrow-first-child rule for section crumbs.
  `Crumb { text: String, href: Option<String> }`.
- **Renderer** in `quarto-navigation/src/render_html.rs`:
  `breadcrumbs_to_html(&[Crumb]) -> String`, pure, resolver-free, escaped
  like its siblings; emits the title-block classes.
- **Transform** (quarto-core): a `BreadcrumbsRenderTransform` beside
  `SidebarRenderTransform` (Navigation phase): finds the page's sidebar,
  computes the trail, resolves each crumb href through
  `resolve_href_for_html` (the now-settled resolver — crumb hrefs grow no
  path logic of their own), and writes
  `rendered.navigation.breadcrumbs` (skip when already set — user override,
  same convention as siblings). Diagnostics: crumb hrefs are the same hrefs
  sidebar rendering already resolves and warns about — the breadcrumb pass
  uses a discarded local diagnostics buffer to avoid duplicate Q-13-1s.
- **Template**: `TITLE_BLOCK_PARTIAL` gains
  `$if(rendered.navigation.breadcrumbs)$` slots — first child of the header
  in the default branch, inside `.quarto-title` in the banner branch, absent
  from the `none` branch (Q1 parity all three).
- **Gating**: sidebar present & page matched & trail length > 1 &
  `website.bread-crumbs` not false (default true) & page-level
  `bread-crumbs` not false.
- **SCSS**: port the `.quarto-title-breadcrumbs` / `.quarto-page-breadcrumbs`
  rules from Q1 `quarto-nav.scss` into `resources/scss/bootstrap/_bootstrap-rules.scss`
  (Bootstrap's own `_breadcrumb.scss` supplies the base component styles).

## Work items

### Phase A — index-miss relativization (bd-tef2lm9j + bd-root-absolute-dir-link-58eh8834)

- [x] Failing unit tests in `navigation_href.rs` (9 added; 7 failed as
      expected — the two round-trip/no-resolver pins passed by design)
- [x] Verify tests fail (confirmed: exactly the 7 behavior-change tests)
- [x] Implement the two miss-branch routings + trailing-slash preservation
      in `resolve_static_resource_href`
- [x] Tests green (navigation_href 71/71); full workspace
      `cargo nextest run --workspace` green (11933 tests, exit 0)
- [x] e2e through the real binary:
      `cargo run --bin q2 -- render claude-notes/plans/index-miss-relativize-investigation/repro`,
      then inspected `_site/deep/deeper/index.html`:
      navbar `assets/report.pdf` → `../../assets/report.pdf`;
      `[dir slash](/target/)` → `../../target/` (trailing slash kept);
      `[dir bare](/target)` → `../../target`;
      control `[root](/index.qmd)` → `../../index.html` unchanged.
      Root page emits `assets/report.pdf` verbatim (depth 0 correct).
      Output inspected directly.
- [x] Commit; close bd-tef2lm9j and bd-root-absolute-dir-link-58eh8834

### Phase B — breadcrumbs (bd-breadcrumbs-missing-1vpuqh34)

- [x] Failing tests: 6 `breadcrumb_trail` units + 3 `breadcrumbs_to_html`
      units (8 of 9 failed against stubs; the trivially-empty pin passed
      by design); 6 pipeline tests (3 positive failed pre-implementation,
      3 suppression pins passed by design). Banner placement covered by
      the template slot, exercised via the docs-site render below.
- [x] Verify tests fail (confirmed both layers)
- [x] Implement trail (`breadcrumb_trail` + `Crumb` in
      quarto-navigation/sidebar.rs, Q1's exact borrow rule) + renderer
      (`breadcrumbs_to_html`, class-parameterized for the future mobile
      instance) + `BreadcrumbsRenderTransform` (Navigation phase, after
      SidebarRenderTransform; discarded-diagnostics resolve pass) +
      title-block partial slots (default + banner branches; `none`
      branch skipped, Q1 parity) + `resolve_website_bool("bread-crumbs",
      true)` gate
- [x] SCSS port into `_bootstrap-rules.scss` (title-block rules only)
- [x] Unit + integration layers green (quarto-navigation 157/157;
      breadcrumbs_pipeline 6/6); full `cargo xtask verify` running at
      wrap-up
- [x] e2e through the real binary:
      `cargo run --bin q2 -- render claude-notes/plans/index-miss-relativize-investigation/breadcrumbs`;
      deep page emits
      `<nav class="quarto-page-breadcrumbs quarto-title-breadcrumbs d-none d-lg-block" aria-label="breadcrumb">`
      with `Guide → ../intro.html` (borrowed first child, page-relative),
      `Advanced → deep.html`, `Deep Page → deep.html` (self as final
      linked crumb); index page has none (length-1). Compiled theme CSS
      carries the ported rules. Also dogfooded on the real docs/ site
      (240 pages): guides/projects/breadcrumbs.html shows its own
      two-crumb trail. Output inspected.
- [x] Docs: new `docs/guides/projects/breadcrumbs.qmd`, registered in the
      docs sidebar; site renders (31 warnings, all pre-existing
      Q-13-4/Q-5-6 dangling-link/YAML diagnostics unrelated to this work)
- [x] File discovered strand: bd-26bf3j1y (quarto-secondary-nav mobile
      container — toggle, mobile breadcrumb instance, search)
- [x] Commit (`66bd2284`); closed bd-breadcrumbs-missing-1vpuqh34

### Wrap-up

- [x] `cargo xtask verify` (full, WASM leg included): all steps passed.
      First run failed on exactly one test —
      `single_doc_render_unchanged_under_scope_refactor`, the
      byte-identity baseline whose styles.css sha256 legitimately
      shifted with the new breadcrumb SCSS. Re-captured per the
      fixture's own convention (annotated in expected_hashes.txt);
      doc.html hash unchanged, confirming the template slot is inert
      without a sidebar. Second run green end to end.
- [x] Snapshot-change inventory: no `.snap` files changed in either
      commit; the one baseline fixture change is itemized in
      `66bd2284`'s message.
- [x] Report to user. **Nothing pushed** — awaiting approval.

### Final commits

- `19b50cdd` — plan skeleton
- `57f89387` — Phase A (resolver miss-branch fix, both strands)
- `66bd2284` — Phase B (breadcrumbs)

## Risks

- Phase A changes output for every non-.qmd nav/body href miss in website
  mode — relative forms round-trip identically (verified reasoning above),
  but snapshot churn is possible; itemize any.
- VFS-root preview mode: Case B's image rewrite had to be mode-gated off
  (asset manifest keys on user-written paths). Body-link *href* rewriting
  already runs in VFS mode (bd-kw93.14), and hrefs are not manifest keys,
  but check the hub-client preview tests in the full verify.
- Breadcrumb gating reads website config per-doc; must use `as_plain_text()`
  not `as_str()` (metadata-as-str lint).
