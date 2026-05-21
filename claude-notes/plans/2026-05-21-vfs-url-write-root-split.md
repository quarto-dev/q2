# Plan — Split `vfs_root` into write-root + url-root in `ResourceResolverContext`

**Date:** 2026-05-21
**Branch:** `beads/bd-rz2we-plan-3-q2-preview` → integrates into `feature/provenance`
**Status:** Implementation plan
**Beads:** bd-rz2we
**Blocks:** closing Plan 3 (q2-preview idempotence gate)

## Goal

Decouple two roles `ResourceResolverContext::vfs_root_mode` currently
plays as a single `PathBuf`:

1. **Disk-write root** — where `runtime.file_write` / `OutputSink`
   put artifacts (theme CSS, copied resources, site libs).
2. **URL prefix** — what gets embedded in HTML link / asset URLs.

In production WASM these are intentionally identical
(`"/.quarto/project-artifacts"` for both, a synthetic VFS path the
service worker serves from memory). On native test runs they have to
diverge: the write root has to be a real tempdir so the runtime can
actually write, but URLs must be path-independent so the AST is
idempotent across runs.

## Why we can't defer this to a later plan

Plan 3 locks in the idempotence + structural-hash-stability contract.
Right now `website_links` produces:

```text
target: ("/private/var/folders/.../T/.tmpXXX/.quarto/project-artifacts/other.html", "")
```

Two runs in two tempdirs → two distinct URLs → block-hash divergence.

Plans 4–8 (typed source-info, wire format, audit, incremental writer,
include round-trip) all assume Plan 3's gate is green on the
fixtures they care about. None of them name URL canonicalization in
scope. Unlike bd-3odjm (whose fix-owner is Plan 5 because Plan 5
rewrites the wire format anyway), bd-rz2we has no natural fix-owner
downstream of Plan 3. Fixing it here is the right scope.

It's also wrong-output, not just non-determinism. Any in-process
caller of `RenderToPreviewAstRenderer::new(real_disk_path)` (test
helpers today, anything else that wants to host the q2-preview
pipeline natively tomorrow) gets links whose URLs leak the host
machine's tempdir into the AST. The browser's iframe service worker
doesn't intercept `/private/var/...`, so those links would 404 if
served.

## Where the bug lives (verified 2026-05-21)

- `LinkRewriteTransform` calls
  `resolve_doc_relative_href("other.qmd", "index.qmd", resolver, idx, …)`
  which delegates to `resolver.page_url_for(profile.output_href)`.
- In **VFS-root mode**, `page_url_for` is just
  `rel_to_url(&root.join(target))` where `root` is whatever was passed
  to `ResourceResolverContext::vfs_root(...)`
  (`crates/quarto-core/src/resource_resolver.rs:210-218`). No
  relativization, no synthetic prefix — the URL is literally the
  joined path.
- `RenderToPreviewAstRenderer` builds its per-doc resolver with
  `ResourceResolverContext::vfs_root(self.vfs_root.clone())`
  (`pass2_renderer.rs:661`). It also writes theme CSS to
  `self.vfs_root.join("styles.css")` directly via
  `runtime.file_write` (`pass2_renderer.rs:739`).
- WASM caller passes `"/.quarto/project-artifacts"`
  (`wasm-quarto-hub-client/src/lib.rs:1512,1696,1786`) — synthetic
  string, identity URL.
- Native test helpers pass `project.dir.join(".quarto/project-artifacts")`
  (`tests/render_page_in_project.rs:80`,
  `tests/idempotence.rs:243`) — real tempdir, leaks into URL.

A naive fix in the test (pass `"/.quarto/project-artifacts"` for both
roles) fails because `runtime.file_write("/.quarto/project-artifacts/styles.css")`
hits the read-only root filesystem (verified empirically: `os error
30`). So the split must really be a split, not a single-arg switch.

## Existing pinned contract

`crates/quarto-core/src/project/website_post_render.rs:638-653`:
> On VFS-root mode the html_url is absolute (`/<vfs_root>/<p>`)
> and the on-disk path is the same with the leading `/` dropped.
> The browser fetches the URL and the hub-client serves from VFS
> at the matching synthetic path.

This is a **WASM-only** invariant. After the split, the single-arg
`vfs_root(path)` constructor preserves it (write_root == url_root by
construction). The two-arg form intentionally breaks it (write to
tempdir, URL stays synthetic) — but only the native test helpers
take that form, so no production code is affected.

## Design

### Resolver field

Replace the single `Option<PathBuf>` field with a small struct:

```rust
struct VfsRootMode {
    /// Absolute disk path. `runtime.file_write` and
    /// `OutputSink::allowed_roots` use this. In WASM this is a
    /// synthetic VFS path (the runtime serves it from memory); in
    /// native tests it's a real tempdir subdirectory.
    write_root: PathBuf,
    /// URL prefix embedded in HTML links / asset srcs. In WASM this
    /// matches `write_root`. In native tests it's a fixed synthetic
    /// string (e.g. `/.quarto/project-artifacts`) so URLs don't
    /// capture the host machine's tempdir.
    url_root: String,
}
```

`page_url_for`, `html_url_for`, `page_url_for_site_root_dir` use
`url_root`; `on_disk_path_for` and `allowed_output_roots` use
`write_root`. `is_vfs_root_mode` is unchanged.

### Resolver constructor

Existing:
```rust
pub fn vfs_root(vfs_root: impl Into<PathBuf>) -> Self { … }
```
keeps its signature and semantics. Internally it stores the path as
both `write_root` and `url_root` (via `to_string_lossy().replace('\\', '/')`).
Production WASM callers don't change.

New constructor:
```rust
pub fn vfs_root_with_url_root(
    write_root: impl Into<PathBuf>,
    url_root: impl Into<String>,
) -> Self { … }
```

Native test helpers switch to this form.

### Renderer side

`RenderToPreviewAstRenderer` and `RenderToHtmlRenderer` each currently
hold a single `vfs_root: PathBuf` and pass it verbatim to the
resolver constructor + theme-CSS write. Add:

```rust
pub struct RenderToPreviewAstRenderer {
    vfs_root: PathBuf,          // unchanged — used for disk writes
    vfs_url_root: Option<String>, // None → derive from vfs_root (today's behavior)
    …
}

impl RenderToPreviewAstRenderer {
    pub fn with_url_root(mut self, url_root: impl Into<String>) -> Self {
        self.vfs_url_root = Some(url_root.into());
        self
    }

    fn build_resolver(&self) -> ResourceResolverContext {
        match &self.vfs_url_root {
            Some(url) => ResourceResolverContext::vfs_root_with_url_root(
                self.vfs_root.clone(), url.clone(),
            ),
            None => ResourceResolverContext::vfs_root(self.vfs_root.clone()),
        }
    }
}
```

Same shape on `RenderToHtmlRenderer` for symmetry (its native callers
aren't currently testing URL determinism, but the API stays consistent
and the surface area is identical).

The three `ResourceResolverContext::vfs_root(self.vfs_root.clone())`
call sites in `pass2_renderer.rs` (lines 437, 552, 661, 798) all
become `self.build_resolver()`.

The theme-CSS write at `pass2_renderer.rs:739` keeps `self.vfs_root.join("styles.css")`
unchanged — that's the disk write, write_root is correct.

### Test-helper updates

`crates/quarto-core/tests/idempotence.rs:243`:
```rust
let vfs_root = project.dir.join(".quarto/project-artifacts");
let renderer = RenderToPreviewAstRenderer::new(&vfs_root)
    .with_url_root("/.quarto/project-artifacts");
```

`crates/quarto-core/tests/render_page_in_project.rs:80-81` gets the
same treatment so the HTML-test path produces deterministic link
URLs too. (Not required by Plan 3, but matches the resolver-level
guarantee. Optional in this plan; do it if regression-cheap.)

## Phases

### Phase 1 — Regression tests (failing first)

- [x] Run `cargo nextest run -p quarto-core --test idempotence website_links`
  and confirm it fails today with the absolute-path symptom (already
  verified; record in the plan and move on).
- [x] Add a unit test in `resource_resolver.rs` that asserts: given
  `ResourceResolverContext::vfs_root_with_url_root("/tmp/abc", "/synthetic")`,
  `html_url_for(Project, p)` returns `"/synthetic/<p>"` and
  `on_disk_path_for(Project, p)` returns `"/tmp/abc/<p>"`. Confirm
  it fails to compile (the constructor doesn't exist yet).

### Phase 2 — Resolver split

- [x] Define the private `VfsRootMode` struct inside `resource_resolver.rs`.
- [x] Change `vfs_root_mode` field from `Option<PathBuf>` to
  `Option<VfsRootMode>`.
- [x] Update the four match sites (`html_url_for`, `page_url_for`,
  `allowed_output_roots`, `on_disk_path_for`) to read the right field.
- [x] Add the `vfs_root_with_url_root` constructor.
- [x] Update the existing `vfs_root` constructor to populate both
  fields from the single arg (preserves the WASM identity contract).
- [x] Run the Phase-1 unit test — should pass.
- [x] Re-run the existing pinned contract test
  (`vfs_root_resolver_url_matches_on_disk_path` in `website_post_render.rs`).
  Should still pass — single-arg constructor still gives URL == disk.

### Phase 3 — Renderer split

- [x] Add `vfs_url_root: Option<String>` field + `with_url_root` builder
  to `RenderToPreviewAstRenderer`.
- [x] Mirror on `RenderToHtmlRenderer`.
- [x] Replace the four `ResourceResolverContext::vfs_root(self.vfs_root.clone())`
  call sites with `self.build_resolver()`.
- [x] `cargo build --workspace` should succeed — no callers have
  changed yet, the new field defaults to `None` which derives the
  URL root from `vfs_root` exactly as before.

### Phase 4 — Wire up test helpers

- [x] `tests/idempotence.rs::render_active_page_preview` adds
  `.with_url_root("/.quarto/project-artifacts")`.
- [x] `tests/render_page_in_project.rs::render_active_page` adds the
  same (optional but consistent).
- [x] Re-run `cargo nextest run -p quarto-core --test idempotence website_links`.
  Should now pass.
- [x] Re-run the full idempotence suite — confirm no other fixtures
  regress.

### Phase 5 — Workspace verification

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace`.
- [x] `cargo xtask verify --skip-hub-build` (matches CI's `-D warnings`
  strictness on the Rust leg).
- [x] Cross-check: WASM hub-client callers still pass single-arg
  `vfs_root("/.quarto/project-artifacts")` and produce identical
  URLs to today (no behavior change). The
  `vfs_root_resolver_url_matches_on_disk_path` test in
  `website_post_render.rs` is the regression sentinel — it stays
  green by construction.

### Phase 6 — Beads housekeeping

- [x] `br close bd-rz2we --reason "fixed: split vfs_root into write-root + url-root in ResourceResolverContext + per-renderer override"`.
- [x] Update Plan 3's Phase-4 checklist line for `website_links` (mark
  green, drop the queue note).
- [x] `br sync --flush-only`, then commit `.beads/` from the main
  repo.

## Out of scope

- `RenderToHtmlRenderer`'s native HTML-output tests aren't currently
  asserting on link URLs; this plan touches them only for API
  symmetry. If they have latent path-leakage in their assertions
  (unlikely — they test HTML content shape), that's a separate ticket.
- The wider `vfs_root` naming question (whether the field should be
  renamed from `vfs_root` to `vfs_write_root` everywhere). Holding off
  to keep the diff small; rename is a no-op refactor that can land
  separately.
- bd-3odjm (FilterProvenance wire-format bug). Owned by Plan 5,
  unrelated.

## Touch list

- `crates/quarto-core/src/resource_resolver.rs` — field, constructor,
  4 match-site updates, 1 new unit test.
- `crates/quarto-core/src/project/pass2_renderer.rs` — 2 renderers ×
  (1 new field, 1 builder method, 1 helper, 4 call-site swaps).
- `crates/quarto-core/tests/idempotence.rs` — 1 helper line.
- `crates/quarto-core/tests/render_page_in_project.rs` — 1 helper
  line (optional).

No production-code callers change.
