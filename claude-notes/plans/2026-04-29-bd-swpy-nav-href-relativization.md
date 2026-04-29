# Fix `bd-swpy` — Sidebar/navbar/footer/page-nav hrefs not relativized to current page

**Date:** 2026-04-29
**Beads:** `bd-swpy` (bug, P1).
**Discovered-from:** `bd-2jwk` (website example projects).
**Parent:** `bd-0tr6` (website epic, closed).
**Status:** Diagnosis + plan draft. Pending user review before
implementation.

## Symptom

In `examples/websites/03-nested-sidebar` and
`examples/websites/04-navbar-footer`, sidebar / navbar / dropdown
/ page-footer / prev-next links are emitted in
**project-root-relative** form (e.g. `guide/installation.html`),
not relative to the current page. From any page that lives in a
subdirectory (`_site/guide/installation.html`,
`_site/tools/converter.html`), clicking a navigation link
404s — the browser resolves `guide/installation.html` against the
current URL as `_site/guide/guide/installation.html`.

Body links don't have the bug. The body `[Home](index.qmd)` link
inside `_site/guide/installation.html` is rendered correctly as
`<a href="index.html">`, which resolves to
`_site/guide/index.html`.

Reproduction (saved in this session):
- `_site/guide/installation.html` — sidebar `<a>` links read
  `guide/index.html`, `guide/installation.html`, `guide/first-steps.html`,
  `guide/tuning.html`. None are page-relative.
- `_site/reference/api.html` — same problem; sidebar links read
  `reference/...`.
- `_site/tools/converter.html` — navbar dropdown links read
  `tools/index.html`, `tools/converter.html`; navbar left items
  read `index.html`, `about.html`. The latter two are *correct
  by accident* (the page is one level deep, and going up to
  `_site/tools/index.html` is exactly what one wants). The bug is
  that the resolution doesn't *know* it's correct — flatten the
  project (move the page to root) and the same code produces a
  404 for everything.

## Root cause

Two helpers exist in
`crates/quarto-core/src/transforms/navigation_href.rs`:

| Helper | Used by | Output URL form |
|---|---|---|
| `resolve_href_for_html` | sidebar / navbar / footer / page-nav Render transforms | **project-root-relative** (`profile.output_href` verbatim) |
| `resolve_doc_relative_href` | `LinkRewriteTransform` (body links, Phase 6) | **page-relative** (via `ResourceResolverContext::page_url_for`) |

The two helpers were built at different times (Phase 3 for the
nav helper, Phase 6 for the body-link helper), and the body-link
work introduced `page_url_for` *after* the nav helper was already
in place. Nobody went back and threaded the resolver through the
nav helper.

`navigation_href.rs:60-63`:

```rust
if let Some(idx) = index {
    if let Some(profile) = idx.lookup_by_source(Path::new(path_part)) {
        return format!("{}{}", profile.output_href, tail);
    }
    ...
}
```

Compare to `navigation_href.rs:194-203` in `resolve_doc_relative_href`:

```rust
if let Some(profile) = idx.lookup_by_source(Path::new(&project_relative)) {
    let url = match resolver {
        Some(r) => r.page_url_for(&profile.output_href),
        None => profile.output_href.clone(),
    };
    return format!("{}{}", url, tail);
}
```

That's the missing piece — for nav hrefs we just emit
`profile.output_href` straight, never asking the resolver to
relativize it.

`ResourceResolverContext::page_url_for` already exists, already
handles all three context shapes (single-doc, website, VFS-root for
hub-client), and is already on `RenderContext` (`render.rs:148`,
optional). The body-link path consumes it; the nav path doesn't.

## Constraints / non-goals

- **Don't change the body-link path.** It already works. The fix
  is purely additive on the nav helper.
- **Don't change the *input* shape of nav hrefs.** Today the nav
  Render transforms receive *project-root-relative* source paths
  (e.g. `guide/installation.qmd`) — that's the contract Phase 2
  Decision 7/8 set up: Generate keeps things format-agnostic in
  source-path space; Render rewrites them. We don't need to
  re-architect this. We only need to relativize the *output*.
- **Don't break standalone (no-index) renders.** When there's no
  `ProjectIndex`, the helper passes hrefs through verbatim. That
  path stays. Single-doc / revealjs UX preserved.
- **Don't break the no-resolver fallback.** Like `resolve_doc_relative_href`,
  fall back to bare `profile.output_href` when no resolver is
  attached. Defensive — production callers always pass a
  resolver, but unit tests / out-of-band callers may not.
- **No YAML-surface changes.** No new config, no new flags.
- **Diagnostics behaviour stays identical** (same source labels,
  same miss warnings).

## Fix sketch

### Step 1 — extend `resolve_href_for_html` to accept a resolver

Add a `resolver: Option<&ResourceResolverContext>` parameter.
On a hit, route the output through `resolver.page_url_for(...)`
(matching the body-link helper). On a miss, on no-index, on
external/fragment — behaviour unchanged.

```rust
pub fn resolve_href_for_html(
    raw: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    source_label: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    // ...external + fragment short-circuits unchanged...

    if let Some(idx) = index {
        if let Some(profile) = idx.lookup_by_source(Path::new(path_part)) {
            let url = match resolver {
                Some(r) => r.page_url_for(&profile.output_href),
                None => profile.output_href.clone(),
            };
            return format!("{}{}", url, tail);
        }
        // ...miss diagnostic unchanged...
    }
    raw.to_string()
}
```

### Step 2 — pass resolver from each Render transform

Four call sites need the new argument. Each has
`ctx.resource_resolver.as_ref()` available right next to the
existing `ctx.project_index.as_deref()` plumbing.

| File | Existing call | New call |
|---|---|---|
| `sidebar_render.rs:116, 121` | `resolve_href_for_html(href, index, source_label, diags)` | `resolve_href_for_html(href, resolver, index, source_label, diags)` |
| `navbar_render.rs:118` | same | same |
| `footer_render.rs:127` | same | same |
| `page_nav_render.rs:80, 90` | same | same |

The argument needs to flow through the transform's `rewrite_*`
helper functions; each currently takes `index`, `source_label`,
`diagnostics` — extend to also take `resolver:
Option<&ResourceResolverContext>`.

### Step 3 — argument-order convention

I'll put `resolver` immediately before `index`, matching the
order in `resolve_doc_relative_href` (which has `resolver` then
`index`). This keeps the two helpers visually parallel and
reduces the mental friction of recalling which goes where.

### Step 4 — tests

The existing tests for `resolve_href_for_html` (lines 285–395)
all pass `None` for the index — they exercise the
external/fragment/no-index branches and don't touch the lookup +
relativize path. Those stay valid by passing `None` for the new
resolver argument.

The lookup tests (`qmd_href_rewrites_via_index`,
`query_and_fragment_preserved_across_rewrite`,
`render_rewrites_qmd_hrefs_to_output_href` in `sidebar_render.rs`,
`navbar_render_rewrites_qmd_hrefs_to_output_href` etc.) currently
assert against project-root-relative output (e.g.
`href="about.html"`). They pass either `None` resolver (in which
case the fallback returns the bare `output_href`, identical to
today's behaviour) or a website resolver pinned at
`index.html` (depth 0, where page-relative == project-relative).
Either way the existing assertions hold.

**New tests** — add to `navigation_href.rs`:

1. **`nav_href_relativizes_via_resolver_at_depth_one`** — page
   is `docs/api.html`; href is `about.qmd`; profile maps to
   `about.html`; resolver is website-flavored. Assert output is
   `../about.html`.
2. **`nav_href_relativizes_via_resolver_at_depth_two`** — page
   is `docs/internals/architecture.html`; href is
   `guide/installation.qmd`; profile maps to
   `guide/installation.html`; resolver is website. Assert output
   is `../../guide/installation.html`.
3. **`nav_href_relativizes_subdir_to_subdir`** — page is
   `guide/installation.html`; href is `reference/api.qmd` (i.e.
   the "switch sidebars" case in `03-nested-sidebar`). Assert
   output is `../reference/api.html`.
4. **`nav_href_no_resolver_falls_back_to_bare_output_href`** —
   regression of today's defensive branch. Pass `None` resolver,
   pass an index with a hit; assert output is bare `about.html`.
5. **`nav_href_preserves_query_and_fragment_through_resolver`** —
   `about.qmd#bio` from depth-1 page → `../about.html#bio`. The
   tail is appended after the resolver call, same as today.

**Render-transform regression tests** — extend existing tests in
`sidebar_render.rs`, `navbar_render.rs`, `footer_render.rs`,
`page_nav_render.rs` with one new case each: page lives at
`/project/_site/guide/installation.html`, href in nav is a
sibling like `index.qmd`, resolver attached, assert that the
rendered HTML contains `href="index.html"` (depth-1 relative)
not `href="guide/index.html"`.

The Render transforms today don't construct a resolver in their
test scaffolding — `RenderContext::new(...)` leaves
`resource_resolver: None`. We'll add a `with_resource_resolver`
helper or set the field directly so each new test can pin a
website-flavored resolver to a specific page output. The
`website_resolver` helper already exists in
`navigation_href.rs:488` — we can copy or extract it.

**End-to-end smoke** — re-render
`examples/websites/03-nested-sidebar`. Inspect:

- `_site/guide/installation.html` should contain
  `<a href="index.html"` (one-level-relative) for the
  "User Guide" sidebar entry, not `<a href="guide/index.html">`.
- `_site/reference/api.html` should contain
  `<a href="cli.html"` for the "CLI" sidebar entry.

Update the README "Known gap" sections of `03-nested-sidebar` and
`04-navbar-footer` to reflect the fix.

### Step 5 — what about the "double `..`" case?

Since `LinkRewriteTransform` (body links) and the nav transforms
will now both feed `page_url_for` for project-root-relative
output hrefs, behaviour converges. Anything `LinkRewriteTransform`
gets right at depth 2 (e.g. `../../about.html` from
`docs/internals/architecture.html`) the nav transforms will also
get right. We're not inventing any new path math — we're routing
through the same existing helper.

## Surface area / risk

**Files touched:**

1. `crates/quarto-core/src/transforms/navigation_href.rs` — add
   `resolver` parameter to `resolve_href_for_html`, route through
   `page_url_for` on hit; add 5 tests.
2. `crates/quarto-core/src/transforms/sidebar_render.rs` — pass
   resolver from `ctx.resource_resolver.as_ref()`; thread through
   `rewrite_hrefs`; extend tests.
3. `crates/quarto-core/src/transforms/navbar_render.rs` — same.
4. `crates/quarto-core/src/transforms/footer_render.rs` — same.
5. `crates/quarto-core/src/transforms/page_nav_render.rs` — same.

**Files NOT touched:** anything outside the four Render transforms.
Generate transforms (which produce the source-path-relative input
for these Renderers) are unchanged. `ResourceResolverContext` is
unchanged. The body-link path is unchanged.

**Public API:** `resolve_href_for_html`'s signature changes (one
new `Option<&ResourceResolverContext>` parameter). The function
is `pub` but only called from inside this crate's `transforms`
module, so the blast radius is limited. Grep confirms 4 internal
callers and none external.

**Snapshot tests:** any rendered-HTML snapshot that captures
nav links from a non-root page will change. The `smoke-all`
suite renders single-doc fixtures; sidebars/navbars there are
likely either absent or rooted at depth 0 (`index.html`-level)
where page-relative == project-relative, in which case no change.
We'll know after running the suite. Per CLAUDE.md
§"Snapshot Test Changes", any update gets explicit call-out in
the commit message.

## Behavioural matrix (what changes, what doesn't)

| Page depth | Today's nav href | Fixed nav href | Diff? |
|---|---|---|---|
| Root (`index.html`) | `about.html` | `about.html` | No |
| Root (`index.html`) | `docs/api.html` | `docs/api.html` | No |
| Depth 1 (`docs/api.html`) | `about.html` | `../about.html` | **Yes** |
| Depth 1 (`docs/api.html`) | `docs/api.html` | `api.html` | **Yes** |
| Depth 1 (`docs/api.html`) | `docs/intro.html` | `intro.html` | **Yes** |
| Depth 2 (`a/b/c.html`) | `e/f.html` | `../../e/f.html` | **Yes** |
| Standalone (no index) | `about.qmd` (verbatim) | `about.qmd` (verbatim) | No |
| External / fragment | passes through | passes through | No |
| Hub-client / VFS-root | (currently broken in some places) | `/{vfs_root}/about.html` | **Yes** (alignment with body-link path) |

The hub-client row is interesting: `page_url_for` already special-cases
VFS-root mode, returning a `/{vfs_root}/...` absolute URL that the
hub iframe can resolve. Today's nav helper bypasses that, so nav
links inside the hub-preview iframe may have the same kind of
issue body links did before Phase 6. After this fix, both paths
agree.

## Testing plan (TDD, per CLAUDE.md)

1. Add new tests 1–5 to `navigation_href.rs`. Confirm they
   compile and **fail** against today's signature.
2. Extend the new signature; tests 1–5 pass.
3. Update the four Render transforms' call sites to pass the
   resolver. Confirm existing tests still pass.
4. Add Render-transform regression tests (one per transform, page
   at depth 1, resolver attached). Confirm they pass.
5. `cargo nextest run --workspace`. Document any snapshot diffs.
6. `cargo xtask verify --skip-hub-tests` (or full
   `cargo xtask verify` since `quarto-core` is touched and
   hub-client depends on it).
7. Re-render `examples/websites/03-nested-sidebar` and
   `examples/websites/04-navbar-footer`. Confirm links
   relativize correctly. Update each example's README to remove
   the "Known gap (bd-swpy)" section.
8. Close `bd-swpy` with the commit hash.

## Decisions to confirm

1. **Argument order in `resolve_href_for_html`.** Proposal:
   `(raw, resolver, index, source_label, diagnostics)`, matching
   the order in `resolve_doc_relative_href`. Alternative: keep
   `index` first to minimise diff at the call sites. I lean
   toward the parallel order; the call-site diff is the same
   either way (one new argument), and parallel arg orders ease
   future maintenance.
2. **Helper extraction.** Both `resolve_href_for_html` and
   `resolve_doc_relative_href` will now look identical on the
   "lookup + maybe relativize + re-append tail" branch. We could
   extract a small private helper. I propose **not** doing that
   in this fix — the two helpers differ in input normalization
   (`resolve_to_project_root` for body links; nothing for nav
   hrefs since they're already root-relative), so factoring out
   the common middle adds a function of dubious shape. Keep them
   parallel by convention; revisit if a third caller appears.
3. **Documentation.** Update the doc comment on
   `resolve_href_for_html` to describe the new resolver
   semantics, and update the comparison table at
   `resolve_doc_relative_href`'s comment to reflect that nav
   output is now also page-relative when a resolver is attached.

Open question for the user before implementation: should the
fixed example READMEs (`03-nested-sidebar`, `04-navbar-footer`)
keep a small *historical* note pointing at this fix, or should
they just remove the "Known gap" section entirely? My default
is "remove entirely; the fix is the documentation". Tell me
otherwise.

## Out of scope (separate follow-ups, do NOT do here)

- **`bd-jbml` / `bd-bobp` / `bd-fo1r` (index-forgiveness).** All
  three are about treating `docs/` as `docs/index.qmd`. That's
  orthogonal to relativization — once the index-forgiveness work
  lands, it will use the same resolver-aware path this fix
  builds, and benefit automatically.
- **`bd-td2a` (footer text-region link rewriting).** Already
  has its own design referencing
  `resolve_doc_relative_href`; not affected here.
- **Cross-format URL resolution (`bd-gdrv`).** Out of website-epic
  scope.

## Work items

### Tests first (TDD)
- [x] Add 5 new unit tests to `navigation_href.rs` covering depth-1,
      depth-2, subdir-to-subdir, no-resolver fallback, and
      query/fragment-with-resolver. All failed against the 4-arg
      signature, then passed after the helper change.
- [x] Add 4 new Render-transform tests (one per Render transform —
      `sidebar_render`, `navbar_render`, `footer_render`,
      `page_nav_render`) with a page at depth 1 and a website
      resolver attached. Asserts page-relative output (`../about.html`,
      `index.html` for siblings). Added `RenderContext::with_resource_resolver`
      builder for test scaffolding.

### Implementation
- [x] Extend `resolve_href_for_html` signature with
      `resolver: Option<&ResourceResolverContext>` (inserted at
      position 2 to mirror `resolve_doc_relative_href`). Route
      hits through `page_url_for`; preserve no-resolver fallback
      to bare `output_href`.
- [x] Update 4 Render transforms to pass
      `ctx.resource_resolver.as_ref()` to the helper, threading
      through their `rewrite_*` private functions.
- [x] Update doc comments on both helpers to reflect the new
      symmetry. Updated the comparison table on
      `resolve_doc_relative_href` to read "page-relative when
      resolver attached" for both helpers.

### Verification
- [x] `cargo nextest run --workspace` — 8081 tests pass.
- [x] `cargo xtask verify --skip-rust-tests --skip-hub-tests` —
      WASM build + trace-viewer green. Pre-existing tests in
      `render_page_in_project.rs` (Phase 9 hub-smoke fixture)
      needed assertion updates: pre-fix they were checking
      project-relative `href="about.html"` produced by the no-resolver
      branch of `resolve_href_for_html`, but in vfs_root mode
      hub-client URLs are absolute (`/{vfs_root}/about.html`).
      Post-fix the nav helper unifies on `page_url_for`, so vfs_root
      mode produces absolute URLs at every call site (sidebar, body
      link, page-nav). Updated assertions to suffix-match `about.html"`
      (the same pattern `website_sidebar_includes_sibling_pages`
      already used). User confirmed this is correct: in hub-client
      we own the deployment (synthetic VFS rooted at `/`), and a
      post-processor / future service worker handles the URL space;
      native renders still produce page-relative URLs and remain
      portable across deploy roots.
- [x] Re-render `examples/websites/03-nested-sidebar`. Sidebar
      links from `_site/guide/installation.html` now read
      `index.html`, `installation.html`, `first-steps.html`,
      `tuning.html` (page-relative). Cross-subtree pagination
      and sibling links also page-relative.
- [x] Re-render `examples/websites/04-navbar-footer`. From
      `_site/tools/converter.html`, navbar Home/About read
      `../index.html`, `../about.html`; dropdown Overview reads
      `index.html` (sibling); footer entries also page-relative.
- [x] Update both example READMEs: replaced "Known gap (bd-swpy)"
      with a "Notes" section documenting the page-relative output
      and deployment-root portability.

### Close-out
- [ ] `br close bd-swpy --reason "..."` with the commit hash.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.
- [ ] Ask user permission before pushing.

## Note on stash recovery (2026-04-29)

Mid-implementation, a `git stash` / `git stash pop` cycle silently
lost the source-file changes (kept the test-file changes). The
stash was recovered from `git fsck --no-reflogs --unreachable`
via the dropped stash hash, and `git checkout <hash> -- <files>`
restored the source files. All tests then re-passed. This is
worth flagging because a "stash pop succeeded silently" message
is an easy thing to trust — but the stash entry's continued
presence after the pop was the actual signal that something went
wrong.
