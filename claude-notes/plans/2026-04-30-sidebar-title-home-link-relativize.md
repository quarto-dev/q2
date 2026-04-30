# Home-link relativization for sidebar title + navbar brand

**Date:** 2026-04-30
**Beads:** `bd-jgeu` (bug, P1) — title widened to cover the navbar
brand as well; see "Scope expansion" below.
**Discovered-from:** `bd-swpy` (sibling — same root-cause family).
**Parent:** `bd-0tr6` (website epic).
**Status:** Diagnosis + plan draft. Pending user review before
implementation.

## Symptom

In `examples/websites/02-auto-sidebar`, the website-title link in
the sidebar header points to the wrong place from any page that is
not at the project root:

| Page                          | Q1 output            | Q2 output (bug)     |
|-------------------------------|----------------------|---------------------|
| `_site/index.html`            | `<a href="./">`      | `<a href="./">`     |
| `_site/posts/aardvark.html`   | `<a href="../">`     | `<a href="./">`     |

From `posts/aardvark.html`, the Q2 link resolves to `posts/`
(re-loading the same directory) instead of the site root.

Reproduction (already on disk):
```
examples/websites/02-auto-sidebar/_site/    # Q2 render
examples/websites/02-auto-sidebar/q1-site/  # Q1 render for comparison
```

```
$ grep sidebar-title examples/websites/02-auto-sidebar/_site/index.html
    <div class="sidebar-title mb-0 py-0"><a href="./">Auto Sidebar</a></div>
$ grep sidebar-title examples/websites/02-auto-sidebar/_site/posts/aardvark.html
    <div class="sidebar-title mb-0 py-0"><a href="./">Auto Sidebar</a></div>   ← bug
$ grep -A1 sidebar-title examples/websites/02-auto-sidebar/q1-site/posts/aardvark.html
    <div class="sidebar-title mb-0 py-0">
      <a href="../">Auto Sidebar</a>
```

## Scope expansion (sweep of related issues)

Per user request, this fix takes the opportunity to address every
hardcoded "home"/"site-root" link the navigation renderer emits.

A full sweep of `crates/quarto-navigation/src/render_html.rs` and
the doc template (`crates/pampa/resources/templates/html/main.html`)
turned up exactly two such sites; both are in scope here:

| # | Location | Current code | Reach if user not at root |
|---|---|---|---|
| 1 | `render_html.rs:211` (sidebar title) | `<a href="./">` (hardcoded) | Reloads current directory |
| 2 | `render_html.rs:297` (`render_brand` — navbar brand) | `navbar.logo_href.as_deref().unwrap_or("/")` | Absolute `/` works only on domain-rooted deployments; breaks on `file://`, GitHub Pages project sites, etc. |

Other `unwrap_or` fallbacks reviewed and judged out-of-scope:

- `render_navbar_item` line 371, `render_dropdown_item` line 392,
  `render_sidebar_leaf` line 477, `render_footer_item` line 663
  all default to `"#"`. That's a no-op anchor (browsers stay on
  the current page). Correct fallback for an item without a link;
  no change needed.
- `render_page_nav_side` line 258 defaults to `""` (intentional —
  the prev/next anchor is suppressed entirely when no item
  exists; the `""` only feeds the aria-label fallback).

User-supplied `navbar.logo_href` also turned up an adjacent gap:
`NavbarRenderTransform::rewrite_navigation_item_hrefs`
(`crates/quarto-core/src/transforms/navbar_render.rs:114`) walks
`navbar.left` / `navbar.right` / dropdown menus through
`resolve_href_for_html`, but **does not touch
`navbar.logo_href`**. So `logo-href: about.qmd` in `_quarto.yml`
would still be emitted verbatim, with no `.qmd` → `.html`
rewrite, no relativization, no diagnostic if the target is
missing. In scope.

The doc template (`crates/pampa/resources/templates/html/main.html`)
contains no nav-related hardcoded hrefs — it slots in the
already-rendered `navigation.sidebar`/`navbar`/`footer` strings.
No template changes required.

## Root cause

### Sidebar title (#1)

`crates/quarto-navigation/src/render_html.rs:211` hardcodes the
`href`:

```rust
html.push_str(&format!(
    "    <div class=\"sidebar-title mb-0 py-0\"><a href=\"./\">{}</a></div>\n",
    render_text(title_cv)
));
```

The renderer has no awareness of the current page's depth, and
emits the same string for every page.

### Navbar brand (#2)

`crates/quarto-navigation/src/render_html.rs:296-297`:

```rust
fn render_brand(navbar: &Navbar, fallback: Option<&str>) -> Option<String> {
    let href = navbar.logo_href.as_deref().unwrap_or("/");
    ...
}
```

When the user doesn't set `logo-href`, the brand anchor falls
back to absolute `/`. That's deployment-fragile (only works when
the site is hosted at the domain root). Q1 emits the same
literal `/` from its EJS template but rewrites it via
`htmlResourceResolverPostprocessor` (see
`external-sources/quarto-cli/src/project/types/website/website-resources.ts:114`),
prepending `projectOffset` so the final HTML carries `./` from
root pages and `../` from depth-1 pages. Q2 has the equivalent
of `projectOffset` in `ResourceResolverContext`, but it isn't
plumbed into the brand renderer.

### Common shape

This is the same family of bug as `bd-swpy` (closed
2026-04-29), which threaded `ResourceResolverContext` through
`resolve_href_for_html` so that *entry* hrefs in the sidebar /
navbar / footer / page-nav relativize correctly. The two home
links (sidebar title, navbar brand) were missed by that fix
because they aren't items — they're hardcoded fallbacks in the
renderer rather than `NavigationItem`s with `.href`.

### How Q1 produces the correct output

Q1's `sidebar.ejs` template emits the literal absolute path
`<a href="/">` (see
`external-sources/quarto-cli/src/resources/projects/website/templates/sidebar.ejs:55`).
The HTML post-processor `htmlResourceResolverPostprocessor`
(`src/project/types/website/website-resources.ts:23`) then walks
all resource attributes; any `href` starting with `/` is treated
as project-root-relative and prefixed with the page's
`projectOffset` (`.` for root, `..` for depth 1, etc.), producing
`./` and `../`.

Q2 has equivalent machinery — `ResourceResolverContext::page_url_for`
already handles this conversion for entry hrefs in
`SidebarRenderTransform` and `NavbarRenderTransform`. The fix is
to plumb the same "page-relative URL of the site root directory"
string through to `sidebar_to_html` and `navbar_to_html`, and to
add `navbar.logo_href` to the navbar transform's rewrite walk.

## Fix plan (TDD)

### Phase 1 — Add a "site-root URL" helper on `ResourceResolverContext`

New method on `ResourceResolverContext` (in
`crates/quarto-core/src/resource_resolver.rs`):

```rust
/// Returns the page-relative URL that points to the **site root
/// directory** (i.e. where `index.html` lives). Always ends with
/// `/`, so HTML attributes can use it as a directory href that
/// the browser will resolve against the host's index document.
///
/// - Root page: `./`
/// - Depth-1 page: `../`
/// - Depth-N page: `../` × N
/// - VFS-root mode (hub-client): `/{vfs_root}/`
/// - Single-doc mode: `./` (degenerate but harmless — single-doc
///   renders don't draw a sidebar header in practice).
pub fn page_url_for_site_root_dir(&self) -> String { ... }
```

Implementation re-uses the same `pathdiff` + `rel_to_url` logic
already in `page_url_for`, but appends a trailing `/`.

**Tests** (in the existing `mod tests` of `resource_resolver.rs`,
mirroring the `page_url_for_*` tests):

1. `page_url_for_site_root_dir_root_page` → `"./"`
2. `page_url_for_site_root_dir_nested_page` → `"../"`
3. `page_url_for_site_root_dir_deep_nesting` (3 levels) →
   `"../../../"`
4. `page_url_for_site_root_dir_vfs_root_mode` →
   `"/<vfs_root>/"`
5. `page_url_for_site_root_dir_single_doc` → `"./"`

Write tests **first**; verify they fail; implement; verify they
pass.

### Phase 2 — Plumb `home_url` through `sidebar_to_html`

Two options considered:

| Option | Signature | Notes |
|---|---|---|
| **A. Extra parameter** | `sidebar_to_html(sidebar, home_url: &str)` | Smallest change; explicit; mirrors how renderers are usually plumbed in `quarto-navigation`. |
| **B. Render context struct** | `sidebar_to_html(sidebar, ctx: SidebarRenderCtx { home_url, ... })` | Future-proofs for additional render-time values, but YAGNI right now. |

**Decision: go with Option A** for both `sidebar_to_html` and
`navbar_to_html`. If a second piece of context shows up later
(e.g. `lang` for `aria-label`s), promote both to a struct then.
Adding the struct now is speculative.

Default for callers that don't have a resolver (unit tests,
single-doc paths, fallthrough): pass `"./"` — preserves the
current behavior at the project root, which is correct for the
single-doc / no-resolver case.

**Tests** (in `crates/quarto-navigation/src/render_html.rs` —
update existing `sidebar_render_text_title_*` tests to thread
an explicit home URL, and add new ones):

6. `sidebar_render_title_home_link_uses_provided_home_url` —
   pass `"../"`, assert `<a href="../">…</a>`.
7. `sidebar_render_title_home_url_is_attribute_escaped` —
   defensive check that arbitrary input is escaped.
8. Existing `sidebar_render_text_title_*` tests updated to pass
   `"./"` explicitly.

### Phase 3 — Plumb `home_url` through `navbar_to_html` (`render_brand`)

Sibling of Phase 2.

- `navbar_to_html` gains a `home_url: &str` parameter (passed
  alongside the existing `document_title_fallback`).
- `render_brand` uses `navbar.logo_href.as_deref().unwrap_or(home_url)`
  in place of `unwrap_or("/")`.
- All existing tests in the navbar block (e.g.
  `navbar_to_html_with_default_navbar_renders_brand`,
  `navbar_to_html_with_logo_href_overrides_default`,
  whatever they're named — survey when implementing) get
  updated to thread an explicit `"./"` and continue to pass.

**Tests** in `render_html.rs`:

9. `navbar_render_brand_uses_home_url_when_no_logo_href` —
   pass `home_url = "../"`, no `logo_href`. Assert
   `<a class="navbar-brand" href="../">…</a>`.
10. `navbar_render_brand_prefers_explicit_logo_href_over_home_url`
    — pass `home_url = "../"` and `logo_href = "about.html"`.
    Assert `<a class="navbar-brand" href="about.html">…</a>`
    (logo_href wins).
11. `navbar_render_brand_home_url_is_attribute_escaped` —
    defensive escape check.
12. Update the Phase-2 test that currently asserts
    `href=\"/\"` (the only existing direct check on this path,
    around `render_html.rs:903`).

### Phase 4 — Wire the resolver through `SidebarRenderTransform`

In
`crates/quarto-core/src/transforms/sidebar_render.rs::SidebarRenderTransform::transform`:

```rust
let home_url = ctx
    .resource_resolver
    .as_ref()
    .map(|r| r.page_url_for_site_root_dir())
    .unwrap_or_else(|| "./".to_string());
let html = sidebar_to_html(&sidebar, &home_url);
```

**Tests** (sibling to
`render_relativizes_sidebar_hrefs_via_resolver_at_depth_one`):

13. `render_relativizes_sidebar_title_home_link_at_depth_one` —
    page lives at `/project/_site/guide/installation.html`,
    sidebar title is `Site`. Assert HTML contains
    `<a href="../">Site</a>`.
14. `render_uses_dot_slash_home_link_when_no_resolver` —
    no resolver attached. Assert HTML contains
    `<a href="./">Site</a>`.

### Phase 5 — Wire the resolver through `NavbarRenderTransform`

In
`crates/quarto-core/src/transforms/navbar_render.rs::NavbarRenderTransform::transform`:

5a. **Compute `home_url`** the same way as Phase 4 and pass to
   `navbar_to_html`.

5b. **Rewrite `navbar.logo_href`** through `resolve_href_for_html`
    in the same place where `rewrite_navigation_item_hrefs` is
    called. This handles user-supplied values (`logo-href: about.qmd`,
    `logo-href: docs/intro.qmd`, etc.) so they get the same .qmd
    → .html rewrite + page-relative URL treatment as ordinary
    nav items.

**Tests** in `navbar_render.rs`:

15. `navbar_render_brand_relativizes_home_link_at_depth_one`
    — depth-1 page, no `logo_href`. Assert
    `<a class="navbar-brand" href="../">…</a>`.
16. `navbar_render_brand_rewrites_user_logo_href_qmd` —
    `logo-href: about.qmd` from a depth-1 page. Assert
    `<a class="navbar-brand" href="../about.html">…</a>` (qmd
    extension swapped, page-relative URL).
17. `navbar_render_brand_external_logo_href_passes_through` —
    `logo-href: https://example.com/`. Assert href verbatim.

### Phase 6 — End-to-end verification

Per `CLAUDE.md` "End-to-end verification before declaring success",
unit tests are not enough.

**Sidebar title (`02-auto-sidebar`):**
1. From repo root:
   ```
   cargo run --bin quarto -- render examples/websites/02-auto-sidebar
   ```
2. Inspect:
   ```
   grep sidebar-title examples/websites/02-auto-sidebar/_site/index.html
   grep -A1 sidebar-title examples/websites/02-auto-sidebar/_site/posts/aardvark.html
   ```
   Expect:
   - `index.html`: `<a href="./">Auto Sidebar</a>`
   - `posts/aardvark.html`: `<a href="../">Auto Sidebar</a>`
3. Diff against `q1-site/` to confirm parity.

**Navbar brand:** pick a fixture under `examples/websites/` that
has `website.navbar` (likely `04-navbar-footer` — confirm during
implementation; if no fixture has a `navbar` *without*
`logo-href`, add a minimal one, or temporarily strip
`logo-href` for the verification run).

4. Render the navbar fixture; grep `navbar-brand` from the root
   page and a depth-1 page; expect `./` and `../` (or
   user-rewritten URL).
5. Spot-check `examples/websites/03-nested-sidebar` for both
   sidebar title and any navbar brand at deeper depths; expect
   `../../`, `../../../` as appropriate.
6. Record actual greps in the closing message / commit body.

## Workspace verification

Per `CLAUDE.md` GIT PUSH POLICY:
- `cargo build --workspace`
- `cargo nextest run --workspace`
- `cargo xtask verify --skip-hub-build` (Rust-only fix; nothing
  hub-client-facing changed). Promote to full
  `cargo xtask verify` if this touches anything
  `wasm-quarto-hub-client` depends on. **Note:**
  `quarto-navigation`, `quarto-core/resource_resolver`, and
  `quarto-core/transforms/{sidebar_render,navbar_render}` are
  all reachable from the WASM client through the render pipeline;
  promote to full verify before pushing.

## Out of scope (verified, no action needed)

- `unwrap_or("#")` fallbacks on entry-style anchors (4
  locations) — `#` is a deliberate no-op anchor for items
  without a link.
- `render_page_nav_side` `unwrap_or("")` — empty string only
  feeds an aria-label fallback when no item exists; the anchor
  itself is gated on `Some(item)`.
- The doc template `crates/pampa/resources/templates/html/main.html`
  contains no nav-related hardcoded hrefs.

## Work items

Phase 1 — Resolver helper:
- [x] Write unit tests 1–5 for `page_url_for_site_root_dir`
- [x] Verify the new tests fail
- [x] Implement `page_url_for_site_root_dir`
- [x] Verify the new tests pass

Phase 2 — `sidebar_to_html` signature:
- [x] Update existing `sidebar_to_html` tests to pass `"./"`
- [x] Add tests 6–7
- [x] Verify the new tests fail
- [x] Add `home_url: &str` parameter to `sidebar_to_html`,
      substitute for hardcoded `./`, escape via `escape_attr`
- [x] Verify the new tests pass; run full
      `cargo nextest run -p quarto-navigation`

Phase 3 — `navbar_to_html` signature:
- [x] Update existing navbar tests (incl. the
      `href="/"` assertion at ~`render_html.rs:903`)
- [x] Add tests 9–11
- [x] Verify the new tests fail
- [x] Add `home_url: &str` parameter to `navbar_to_html`,
      have `render_brand` fall back to it instead of `"/"`,
      escape via `escape_attr`
- [x] Verify the new tests pass

Phase 4 — Sidebar transform wiring:
- [x] Add tests 13–14 in `sidebar_render.rs`
- [x] Verify they fail
- [x] Compute `home_url` from `ctx.resource_resolver` and pass
      to `sidebar_to_html`
- [x] Verify they pass

Phase 5 — Navbar transform wiring:
- [x] Add tests 15–17 in `navbar_render.rs`
- [x] Verify they fail
- [x] Compute `home_url` from `ctx.resource_resolver` and pass
      to `navbar_to_html`
- [x] Add `navbar.logo_href` to the resolver-rewrite walk so
      user-supplied .qmd / project-relative paths get the same
      treatment as ordinary nav items
- [x] Verify they pass

Phase 6 — End-to-end:
- [x] Re-render `examples/websites/02-auto-sidebar`; grep
      `sidebar-title` in root + posts; confirm `./` and `../`
- [x] Diff against `q1-site/` for parity
- [x] Render a navbar fixture (`04-navbar-footer`); grep
      `navbar-brand` at root + depth-1 (`./` and `../` confirmed)
- [x] Spot-check `examples/websites/03-nested-sidebar` (deeper
      depths) for sidebar title — all depth-1 pages emit `../`
      (root has no sidebar; both sidebars are scoped to subdirs)

Workspace verification:
- [x] `cargo build --workspace`
- [x] `cargo nextest run --workspace` — 8140/8140 pass
- [x] `cargo xtask verify` (full — touches code reachable from
      WASM client; do not skip the hub build) — all steps pass
