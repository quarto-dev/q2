# Phase 6 — Cross-document link rewriting

**Date:** 2026-04-24
**Beads:** `bd-v30t` (parent `bd-0tr6`). Follow-ups TBD at close-out.
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-24-websites-phase-5.md`
**Status:** Draft — pending user review.

## Goal of this phase

Rewrite **body-content** `[link](other.qmd)` references so they
resolve to the right output URL on disk, with a relative path that
accounts for the current page's depth in the site tree. Concretely:

- `[About](about.qmd)` in `index.qmd` → `<a href="about.html">About</a>`
- `[About](../about.qmd)` in `docs/api.qmd` → `<a href="../about.html">About</a>`
- `[API](docs/api.qmd)` in `index.qmd` → `<a href="docs/api.html">API</a>`
- `[Section](other.qmd#sec)` → `<a href="other.html#sec">Section</a>`
- `[Search](other.qmd?q=x)` → `<a href="other.html?q=x">Search</a>`
- `[Site root](/about.qmd)` in `docs/api.qmd` → `<a href="../about.html">Site root</a>`

Today the navigation transforms (sidebar, navbar, page-footer,
page-nav) already rewrite `.qmd` hrefs through the shared
`navigation_href::resolve_href_for_html`. Phase 6 extends this rewrite
to **body content** — the inline `Link` nodes that come from the
markdown source itself.

This phase adds:

1. A new `LinkRewriteTransform` that walks the AST body, finds every
   `Inline::Link`, and rewrites its `target.0` URL when it points at
   another project document.
2. A new helper (working name `resolve_doc_relative_href`) that
   handles the **source-doc-relative** path math the existing
   `resolve_href_for_html` doesn't cover. Body links are written
   relative to the current source file's directory; navigation links
   live in `_quarto.yml` and are project-root-relative. The two helpers
   share the lookup + diagnostics path but differ in their input
   normalization.
3. A new `page_url_for(target_output_href: &str) -> String` method on
   `ResourceResolverContext` that turns a target page's project-
   relative output href into a relative URL from the current page,
   using the same `pathdiff` + `rel_to_url` machinery Phase 5 uses
   for shared assets.
4. A small additive change to `RenderContext`: a
   `resource_resolver: Option<ResourceResolverContext>` field so the
   new transform can read the resolver Phase 5 already builds in
   `render_to_file.rs`. Populated alongside `project_index`.

This phase does **not** implement:

- **Draft handling.** Q1 hides links to draft pages (replacing the
  `<a>` with its inner content) when `draftMode != "visible"`. Q2's
  `DocumentProfile.draft` field exists, but draft-mode config doesn't.
  Defer the visibility-mode behavior to a follow-up bead. Phase 6 still
  rewrites href targets that happen to be draft pages — they just
  remain reachable through the link. Filing as `bd-<draft-mode>`.
- **Index-forgiveness** (`docs/` matching `docs/index.qmd`). Same
  deferral as Phase 3's `bd-jbml` and Phase 4's `bd-bobp` — file as
  a single follow-up that covers nav + body uniformly.
- **`.md` / `.ipynb` extension recognition.** Q1 warns on
  unresolvable links to all "engine valid" extensions; Q2 currently
  only renders `.qmd` (per `bd-xxul`). Match: warn on `.qmd` only.
  When `bd-xxul` lands, that work extends Phase 6's heuristic at the
  same time.
- **Cross-format link awareness.** A `[link](other.qmd)` from an
  HTML page targeting a doc with `format: pdf` should produce a
  `.pdf` URL. MVP assumes single-format projects (HTML); cross-
  format URL resolution is out of epic scope.
- **Image rewriting.** `Image.target.0` is left unchanged. Images
  point at static resources, not project documents; Q1 doesn't
  rewrite them either.
- **`<base href>` / `offset`-prefix output.** Q1 emits relative URLs
  from the current page; Q2 matches. No `/`-rooted URLs.
- **Footer Text-region link rewriting.** Phase 5's follow-up
  `bd-jfyl` is the right bead to consume the helper this phase adds.
  Tracked separately.
- **Custom-node body links.** Walking is recursive (mirrors
  `crossref_render`'s `render_inlines` pattern), so any `Inline::Link`
  reachable through `Inline::Custom`'s `Slot` content is rewritten too.
  No special-case work, but no proactive design for novel custom-node
  shapes that bypass Inlines.

## Reference material

- **Parent epic plan** §"Phase 6 — Cross-document link rewriting"
  and §"Cross-document index".
- **Phase 5 sub-plan** §Decision 6 (`ResourceResolverContext`) and
  §Decision 7 (URL resolution at template-apply time).
- **Phase 3 sub-plan** §Decision 3 (shared `navigation_href` helper)
  and the resulting `crates/quarto-core/src/transforms/navigation_href.rs`.
- **Q2 current code:**
  - `crates/quarto-core/src/transforms/navigation_href.rs` —
    `resolve_href_for_html`, `is_external`. Project-root-relative
    input, project-root-relative output. Phase 6 wraps / extends.
  - `crates/quarto-core/src/transforms/resource_collector.rs` —
    template for the recursive AST walk (block / inline / custom
    slots).
  - `crates/quarto-core/src/transforms/crossref_render.rs:177-214` —
    `render_inline` recursive walk pattern. Phase 6 mirrors.
  - `crates/quarto-core/src/resource_resolver.rs` — Phase 5 resolver.
    Phase 6 adds one method (`page_url_for`).
  - `crates/quarto-core/src/render.rs:134` — `RenderContext.project_index`
    field. Phase 6 adds a sibling `resource_resolver` field.
  - `crates/quarto-core/src/render_to_file.rs:217-235` — where the
    project index and resolver are constructed. Phase 6 wires the
    resolver into `RenderContext` here.
  - `crates/quarto-core/src/pipeline.rs:622-645` — `build_transform_pipeline`.
    Phase 6 inserts one new transform near the end.
  - `crates/quarto-core/src/project/index.rs` — `ProjectIndex` and
    `lookup_by_source`. Already used by `resolve_href_for_html`.
  - `crates/quarto-pandoc-types/src/inline.rs:191-199` — `Link`
    struct. `target: Target = (String, String)`; `target.0` is the URL.
  - `crates/quarto-core/src/transforms/navigation_active.rs:36-49` —
    `page_relative_source(ctx)`. Phase 6 reuses to know "what doc am
    I in?" for resolving doc-relative hrefs.
- **Q1 reference:**
  - `external-sources/quarto-cli/src/project/types/website/website-utils.ts:61-122`
    — `resolveProjectInputLinks`. The Deno-DOM-based body-link
    rewriter. Phase 6 mirrors its semantics in AST-space:
    - leading-`/` strips to project-relative
    - else `join(dirname(sourceRelative), linkHref)` resolves
      relative to source-doc dir
    - hash split / re-append
    - `resolveInputTarget` lookup → `outputHref`
    - `offset + outputHref + hash` to produce the final href
  - `external-sources/quarto-cli/src/project/types/website/website-navigation.ts:218`
    and `:577` — call sites. Q1 invokes the rewriter post-render
    (Deno-DOM walk over the emitted HTML); Q2 does it pre-render
    (AST walk before the format renderer runs).

## Key decisions (to confirm with user)

These are proposed — please push back on anything that looks wrong
before we start.

### Decision 1 — Rewrite at AST level, not post-HTML

Walk `Inline::Link` nodes in the AST body, mutate `target.0` in
place. Do **not** add an HTML post-processor.

**Rationale.**
- Format-agnostic by construction. Today only HTML cares; tomorrow a
  PDF-ish renderer can consume the same AST and choose to ignore /
  use the rewritten hrefs.
- No new dependency on a DOM library. Q1's Deno DOM round-trip is
  expensive (parse → walk → serialize); the AST already has
  structured `Link` nodes.
- Composes with the existing `resolve_href_for_html` helper used by
  navigation transforms (sidebar / navbar / page-nav / footer).
  Same lookup, same diagnostics shape, same source label
  convention.

**Trade-off.** A Lua filter that *generates* `.qmd` hrefs after Phase
6 would not get its hrefs rewritten. Phase 6 sits in the Finalization
Phase, after most filters have run; specifically users get one chance
to rewrite hrefs *before* Phase 6 (in pre-engine / engine / generic
AstTransforms) but not after. Q1 has an analogous limitation (any
post-rewrite Lua filter rewriting hrefs would bypass Q1's HTML
post-processor too). If a real workflow surfaces filter-emitted .qmd
hrefs, we add a second-pass transform and document the ordering
contract.

### Decision 2 — Pipeline placement: first transform in Finalization Phase

Insert `LinkRewriteTransform` between the navigation Render
transforms and `AppendixStructureTransform`:

```
TocRenderTransform
NavbarRenderTransform
SidebarRenderTransform
PageNavRenderTransform
FooterRenderTransform
                          ← end of Navigation Phase
LinkRewriteTransform      ← NEW (start of Finalization Phase)
AppendixStructureTransform
CrossrefRenderTransform
ResourceCollectorTransform
```

**Rationale for this slot:**
- After all transforms that *generate* navigation HTML — those
  rewrite hrefs in their own subtrees, not in body content, so they
  don't conflict.
- After `CrossrefResolveTransform` — crossref rewrites turn `@fig-1`
  into `Inline::Link` nodes pointing at intra-doc fragments
  (`#fig-1`). Those have empty path / fragment-only hrefs, so the
  `is_external | starts_with('#')` shortcut in the helper skips
  them. Verified by reading `crossref_render.rs:704+`.
- Before `AppendixStructureTransform` — appendix consolidation
  doesn't touch link hrefs, but moving the transform afterwards would
  give it body-link inlines reorganized into the appendix container,
  which doesn't change behavior. Either ordering works; we pick the
  earlier slot for predictability ("rewrite finishes before any
  reshuffle starts").
- Before `CrossrefRenderTransform` — actually, this is when crossref
  custom-node Inlines become `Inline::Link`. Does that matter? Read:
  `crossref_render.rs` produces `Inline::Link` for resolved refs,
  but those have `#`-anchored hrefs (`#fig-1`), which Phase 6 skips
  via the fragment-anchor short-circuit. So order doesn't matter for
  intra-doc crossrefs. **Cross-document crossrefs** (e.g.
  `@chapter-2` resolving to `chapter-2.qmd#sec`) would matter, but
  that's `bd-xxxx` / book scope, out of this epic.

If the ordering reveals a corner case during implementation
(e.g. some Finalization-phase transform that *does* emit
`.qmd` hrefs after Phase 6), we'll move the transform. Easy to
reorder in `pipeline.rs`.

### Decision 3 — New helper `resolve_doc_relative_href`

Add to `crates/quarto-core/src/transforms/navigation_href.rs`
alongside the existing `resolve_href_for_html`:

```rust
/// Resolve a body-content href to a relative URL.
///
/// `raw` is the link href as written by the user (e.g.
/// `"../about.qmd#bio"`, `"docs/api.qmd"`, `"/about.qmd"`).
/// `source_relative` is the current document's project-relative
/// source path (forward-slash form), used to resolve doc-relative
/// references.
/// `resolver` is the per-page resource resolver (Phase 5) used to
/// turn a target output href into a page-relative URL.
/// `index` is the project's `ProjectIndex`; the function is a no-op
/// (returns `raw.to_string()`) when `None`.
pub fn resolve_doc_relative_href(
    raw: &str,
    source_relative: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    source_label: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String { … }
```

Algorithm:
1. **External / fragment-only short-circuit** — same as
   `resolve_href_for_html`. Pass through.
2. **Split path / tail** — `path_part = raw[..i]`, `tail = raw[i..]`
   where `i` is the first `#` or `?`. Same as today.
3. **Source-relative resolution** — compute
   `project_relative_path`:
   - If `path_part.starts_with('/')`: strip leading `/`. (Q1 parity.)
   - Else: join with `dirname(source_relative)` and **normalize**
     `.` / `..` components. (`PathBuf::join` doesn't normalize, so
     a small helper does the walk-and-pop.) Forward-slash result.
4. **Lookup** — `index.lookup_by_source(project_relative_path)`.
5. **Hit:**
   - `target_output_href = profile.output_href` (project-relative,
     forward-slash, e.g. `"docs/api.html"`).
   - `relative_url = resolver.page_url_for(target_output_href)`
     when a resolver is available; falls back to
     `target_output_href` verbatim otherwise (no relative-depth
     math).
   - Return `relative_url + tail`.
6. **Miss:**
   - If `path_part.ends_with(".qmd")` and `index.is_some()`:
     emit a warning diagnostic `"<source_label> references unknown
     document '<path_part>'"`. (Mirrors `resolve_href_for_html`.)
   - Return `raw.to_string()` so the dangling link renders visibly.
7. **No `index`:** return `raw.to_string()` verbatim, no diagnostic.
   (Mirrors today's standalone-render behavior.)

**Why a separate helper, not extend `resolve_href_for_html`:**
- Different input normalization: navigation hrefs are project-root-
  relative as written (`about.qmd`, `docs/api.qmd`); body hrefs are
  source-relative (`../about.qmd`, `subdir/foo.qmd`). Conflating the
  two would silently mis-route nav configs that happen to start with
  `..`.
- Different output: navigation produces project-root URLs (consumed
  by `<a href="about.html">` where the template's `<base>` or static
  assumptions make root-relative work); body links need page-relative
  URLs that account for the current page's depth.
- Same lookup + diagnostics infrastructure though, so the two
  helpers share `is_external`, the `path_part` / `tail` split, and
  the diagnostic shape.

**Naming alternatives considered:**
- `resolve_body_link_href` — possibly too narrow if a future caller
  wants to use the helper outside of body content.
- `resolve_relative_qmd_href` — too tied to the `.qmd` extension.
- `resolve_doc_relative_href` — picked. Matches Q1's
  `resolveProjectInputLinks` semantics (the "input" name in Q1 means
  "input to the renderer" = source doc).

### Decision 4 — New `page_url_for` method on `ResourceResolverContext`

Add to `crates/quarto-core/src/resource_resolver.rs`:

```rust
impl ResourceResolverContext {
    /// Compute a relative URL from the current page to another
    /// page in the project, given the target's project-relative
    /// output href (e.g. `"docs/api.html"`).
    ///
    /// In VFS-root mode returns `{vfs_root}/{target_output_href}`.
    /// In single-doc mode returns `target_output_href` verbatim
    /// (no project structure to relate against).
    /// Otherwise returns the relative URL from the current page's
    /// directory to `{site_root}/{target_output_href}`.
    pub fn page_url_for(&self, target_output_href: &str) -> String {
        if let Some(root) = &self.vfs_root_mode {
            return rel_to_url(&root.join(target_output_href));
        }
        let target_abs = self.site_root.join(target_output_href);
        let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
        let rel = pathdiff::diff_paths(&target_abs, page_dir)
            .unwrap_or_else(|| target_abs.clone());
        rel_to_url(&rel)
    }
}
```

Same shape as `html_url_for`, except the input is a project-relative
output href (a `String`) rather than an `(ArtifactScope, &Path)`
pair. Reuses the private `rel_to_url` helper Phase 5 introduced.

**Single-doc fallback rationale:** in `single_doc` mode `site_root ==
page_output.parent()`, so the math collapses to `target_output_href`
itself — which is correct for the (uncommon) case where a single-doc
render somehow has an index. In practice the transform's standalone-
render branch returns early before it ever calls `page_url_for`, so
this branch is defensive, not load-bearing.

### Decision 5 — `RenderContext.resource_resolver: Option<ResourceResolverContext>`

The Phase 5 resolver is constructed in `render_to_file.rs:229` and
passed to `HtmlRenderConfig::with_resolver`. AST transforms today
have no way to read it.

Add a field to `RenderContext`:

```rust
pub struct RenderContext<'a> {
    // existing fields …
    pub resource_resolver: Option<ResourceResolverContext>,    // NEW
}
```

Populate it in `render_to_file.rs` immediately after `project_index`:

```rust
let resolver = ResourceResolverContext::website(…);
ctx.project_index = Some(index);
ctx.resource_resolver = Some(resolver.clone());            // NEW
let config = HtmlRenderConfig::with_resolver(resolver);
```

Bridge it through `pipeline.rs:415` the same way `project_index` is
threaded into `StageContext` for stages that need it. (For Phase 6,
only the AST transform reads it via `RenderContext`; no stage
plumbing needed beyond the existing `RenderContext` field.)

**Why on `RenderContext` and not a transform-constructor argument:**
- Symmetric with `project_index` (already on `RenderContext`).
- Lets the helper signature stay narrow — the transform passes
  `ctx.resource_resolver.as_ref()` and `ctx.project_index.as_deref()`
  in a single call site.
- Future consumers (Phase 5 follow-up `bd-jfyl` for footer text-
  region links, Phase 7's site-url qualifier) can read it without
  re-plumbing.

**Risk.** `RenderContext` already has a lot of fields; adding another
ratchets the struct size and `RenderContext::new`. Mitigation: the
field has a `None` default and is purely additive — no caller
literal needs to change (verified by grep of `RenderContext { …
}` literals). The `.with_*` builder pattern stays.

### Decision 6 — `LinkRewriteTransform` walks blocks + inlines, including custom slots

Mirror the recursive pattern in
`crates/quarto-core/src/transforms/crossref_render.rs:171-214`. The
transform owns the walk; `resolve_doc_relative_href` is the
per-link helper.

```rust
pub struct LinkRewriteTransform;

#[async_trait::async_trait(?Send)]
impl AstTransform for LinkRewriteTransform {
    fn name(&self) -> &str { "link-rewrite" }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let Some(index) = ctx.project_index.as_deref() else {
            // Standalone render: no project context, no rewrites.
            return Ok(());
        };
        let resolver = ctx.resource_resolver.as_ref();
        let source = page_relative_source(ctx);
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        let mut rewriter = LinkRewriter {
            source: &source,
            index,
            resolver,
            diagnostics: &mut local_diags,
        };
        for block in &mut ast.blocks {
            rewriter.visit_block(block);
        }
        ctx.diagnostics = local_diags;
        Ok(())
    }
}

struct LinkRewriter<'a> { … }
impl<'a> LinkRewriter<'a> {
    fn visit_block(&mut self, block: &mut Block) { … }
    fn visit_inline(&mut self, inline: &mut Inline) {
        match inline {
            Inline::Link(link) => {
                // Recurse into content first (in case nested
                // Inline::Link or Inline::Custom contains rewritable
                // children — uncommon but possible).
                for child in link.content.iter_mut() {
                    self.visit_inline(child);
                }
                // Rewrite target.0 (URL); target.1 (title) untouched.
                let new_url = resolve_doc_relative_href(
                    &link.target.0,
                    self.source,
                    self.resolver,
                    Some(self.index),
                    Some("Body link"),
                    self.diagnostics,
                );
                link.target.0 = new_url;
            }
            // … recurse through other Inline variants (Emph, Strong,
            // Span, Custom, Image content, Note content, …) per the
            // resource_collector pattern.
        }
    }
}
```

**Image targets are not rewritten.** Image content (alt text /
captions) is walked recursively in case it contains `Inline::Link`,
but `Image::target.0` (the image URL) is left as-is — Q1 doesn't
rewrite it either; images point at static resources, not project
documents.

**Custom nodes** are walked through their `Slot`s, mirroring
`resource_collector.rs:191-213`.

### Decision 7 — Standalone render = no-op

When `ctx.project_index` is `None`, the transform returns
immediately without touching the AST. Body links pass through
verbatim (no rewriting, no diagnostics).

This matches the existing single-doc-render contract for sidebar /
navbar / page-nav / footer (Phase 2/3/4 Decision 1). A revealjs slide
deck or a one-off `.qmd` file with `[link](other.qmd)` keeps its
literal href.

Cost: a body-link author who *expects* their `[link](other.qmd)` to
become `other.html` in a standalone render gets surprised. Mitigation:
docs note + the diagnostic is silent (no warning to ignore). Q1 is
the same here: it only runs `resolveProjectInputLinks` inside
`renderForPrint` when a project context exists.

### Decision 8 — Diagnostic shape

`source_label = "Body link"`. Matches Phase 3's `"Sidebar"` /
`"Navbar"` / `"Page footer"` and Phase 4's `"Page navigation"`
convention. Diagnostic title:

```
Body link references unknown document 'docs/missing.qmd'
```

Same warning-level severity as the navigation diagnostics. The href
is preserved so the broken link is visibly broken at render time.

**No diagnostic is emitted** for non-`.qmd` misses (e.g.
`assets/foo.png`, `mailto:`, …). External URLs and fragment-only
anchors short-circuit before the lookup. Non-qmd-shaped misses are
indistinguishable from intentional static-resource references — Q1
takes the same stance.

**Future:** when `bd-xxul` lands `.md` / `.ipynb` support, the same
diagnostic fires for those extensions. The decision of "which
extensions warrant a warning" lives with the renderable-extensions
list, not in this helper.

### Decision 9 — Path normalization helper

Body hrefs can contain `..` and `.` components. `PathBuf::join`
doesn't resolve these:
- `Path::new("docs").join("../about.qmd")` produces `docs/../about.qmd`,
  not `about.qmd`.

We need a lossless walk-and-pop normalizer. Two options:

1. **Inline helper in `navigation_href.rs`** that walks
   `Path::components()`, pushing `Normal` components onto a stack
   and popping on `ParentDir`. Returns a `String` (forward-slash).
2. **Reuse a crate.** `path-clean` or `pathdiff` (already in
   workspace) — `pathdiff` doesn't normalize, only diffs;
   `path-clean` would be a new dep.

Recommendation: **inline helper**, ~15 lines. Minimal dependency
footprint, exact semantics we control, easy to test.

```rust
/// Join `linkHref` (a forward-slash, doc-relative or absolute path
/// expression) against `source_relative`'s directory, normalize
/// `.` / `..` components, and return a project-relative
/// forward-slash path.
///
/// - `link_href` starting with `/` strips the leading slash.
/// - Otherwise joins with `dirname(source_relative)`.
/// - Components that walk above the project root are clamped at
///   the root (matches `Path::canonicalize` behavior, no error).
fn resolve_to_project_root(source_relative: &str, link_href: &str) -> String { … }
```

Tests cover: leading `/`, `..` to parent dir, multiple `..`, walking
above root (clamp), `.` no-op, mixed-case forward slashes.

### Decision 10 — Final URL is page-relative

Q1 uses `offset + outputHref + hash` where `offset` is the relative
prefix from current page's depth. Q2 uses `pathdiff::diff_paths` from
current page dir to target page abs path. Equivalent result.

Examples (pages in `_site/…`):

| Source page | Target output href | Result URL |
|-------------|---------------------|------------|
| `index.html` | `about.html` | `about.html` |
| `index.html` | `docs/api.html` | `docs/api.html` |
| `docs/api.html` | `about.html` | `../about.html` |
| `docs/api.html` | `docs/intro.html` | `intro.html` |
| `a/b/c/d.html` | `e/f.html` | `../../../e/f.html` |

**Trailing-tail re-attach:** `resolver.page_url_for("docs/api.html")
+ "#sec"` → `"docs/api.html#sec"`. The `+` is plain string
concatenation; the helper doesn't insert any extra `/`.

## Architecture sketch

### Data flow

```
ast.blocks (after navigation Render transforms)
     │
     ▼
LinkRewriteTransform
     │
     ├── for each Inline::Link in body:
     │       ├── resolve_doc_relative_href(
     │       │       link.target.0,
     │       │       page_relative_source(ctx),
     │       │       ctx.resource_resolver.as_ref(),
     │       │       ctx.project_index.as_deref(),
     │       │       Some("Body link"),
     │       │       &mut local_diags,
     │       │   )
     │       └── link.target.0 = new_url
     │
     └── ctx.diagnostics absorbs warnings for missing docs
```

### Module shape

```
crates/quarto-core/src/
    resource_resolver.rs         # add `page_url_for` method
    render.rs                    # add `resource_resolver` field
    render_to_file.rs            # populate `ctx.resource_resolver`
    pipeline.rs                  # insert LinkRewriteTransform

crates/quarto-core/src/transforms/
    navigation_href.rs           # add `resolve_doc_relative_href` +
                                 # private `resolve_to_project_root`
                                 # path-normalization helper
    link_rewrite.rs              # NEW — LinkRewriteTransform
    mod.rs                       # re-export LinkRewriteTransform
```

### Single-doc behavior (regression check)

For a default project (single-file or directory without
`_quarto.yml`), `ctx.project_index` is `None`. The transform's first
line returns `Ok(())` and the AST is untouched. `[link](other.qmd)`
in body content keeps its `.qmd` href verbatim — same as
pre-Phase-6.

### Multi-doc website behavior

For a website project, every Pass-2 doc receives a populated
`project_index` and `resource_resolver`. The transform walks every
`Inline::Link`, rewrites internal `.qmd` hrefs, and leaves
external/fragment/non-qmd hrefs untouched. The downstream HTML
renderer emits `<a href="…">` from `link.target.0` without further
processing.

## DocumentProfile change

**None.** Phase 6 reads only `output_href`, `source_path`, and
`draft` (the last not yet — drafts deferred). All three are
profile-version 1 fields. No bump.

## Tests (TDD: write and fail first)

Every test authored before the code that makes it pass. Failing
baseline captured before implementation.

### Unit tests — `resource_resolver::page_url_for`

1. `page_url_for_root_page_root_target` — page at
   `_site/index.html`, target `about.html` → `"about.html"`.
2. `page_url_for_root_page_nested_target` — page at
   `_site/index.html`, target `docs/api.html` →
   `"docs/api.html"`.
3. `page_url_for_nested_page_root_target` — page at
   `_site/docs/api.html`, target `about.html` →
   `"../about.html"`.
4. `page_url_for_nested_page_sibling_target` — page at
   `_site/docs/api.html`, target `docs/intro.html` →
   `"intro.html"`.
5. `page_url_for_deep_nesting` — page at `_site/a/b/c/d.html`,
   target `e/f.html` → `"../../../e/f.html"`.
6. `page_url_for_vfs_root_mode` — `vfs_root("/.quarto/proj")`,
   target `about.html` → `"/.quarto/proj/about.html"` (matches
   `html_url_for` VFS conventions).
7. `page_url_for_single_doc_returns_target_verbatim` —
   `single_doc("/tmp/doc.html", "doc")`, target `about.html` →
   `"about.html"` (single-doc fallback).

### Unit tests — `resolve_to_project_root` (path normalization)

8. `path_normalize_leading_slash_strips` — `"/about.qmd"` from any
   source → `"about.qmd"`.
9. `path_normalize_doc_relative_no_dotdot` — `"foo.qmd"` from
   `docs/api.qmd` → `"docs/foo.qmd"`.
10. `path_normalize_dotdot_to_parent` — `"../about.qmd"` from
    `docs/api.qmd` → `"about.qmd"`.
11. `path_normalize_multiple_dotdot` — `"../../about.qmd"` from
    `a/b/c.qmd` → `"about.qmd"`.
12. `path_normalize_dot_no_op` — `"./foo.qmd"` from `docs/api.qmd`
    → `"docs/foo.qmd"`.
13. `path_normalize_clamp_above_root` — `"../../../foo.qmd"` from
    `a/b.qmd` → `"foo.qmd"` (clamp at root, no error).
14. `path_normalize_subdir` — `"sub/foo.qmd"` from `docs/api.qmd`
    → `"docs/sub/foo.qmd"`.
15. `path_normalize_root_source` — `"about.qmd"` from `index.qmd`
    → `"about.qmd"`.

### Unit tests — `resolve_doc_relative_href`

16. `body_href_external_passes_through` — `"https://example.com"`
    from any source → `"https://example.com"`.
17. `body_href_fragment_only_passes_through` — `"#section"` from
    any source → `"#section"`.
18. `body_href_qmd_hits_index` — `"about.qmd"` from `index.qmd`,
    project has `about.qmd → about.html` → `"about.html"`.
19. `body_href_doc_relative_qmd_hits_index` — `"../about.qmd"`
    from `docs/api.qmd`, project has `about.qmd → about.html`,
    resolver page at `_site/docs/api.html`, site root `_site` →
    `"../about.html"`.
20. `body_href_absolute_qmd_hits_index` — `"/about.qmd"` from
    `docs/api.qmd` → `"../about.html"`.
21. `body_href_subdir_qmd` — `"docs/api.qmd"` from `index.qmd` →
    `"docs/api.html"`.
22. `body_href_preserves_fragment` — `"about.qmd#bio"` from
    `index.qmd` → `"about.html#bio"`.
23. `body_href_preserves_query` — `"about.qmd?x=1"` →
    `"about.html?x=1"`.
24. `body_href_preserves_query_and_fragment` —
    `"about.qmd?x=1#bio"` → `"about.html?x=1#bio"`.
25. `body_href_qmd_miss_emits_diagnostic` — `"missing.qmd"` with
    `source_label = "Body link"`: diagnostic title starts with
    `"Body link"` and contains `"missing.qmd"`. Returned href is
    `"missing.qmd"` (verbatim — broken link visible).
26. `body_href_non_qmd_miss_no_diagnostic` —
    `"assets/logo.png"` with no matching profile: returned
    verbatim, no diagnostic.
27. `body_href_no_index_passes_through` — `"about.qmd"` with
    `index = None`: returned verbatim, no diagnostic.
28. `body_href_no_resolver_falls_back_to_output_href` —
    `"about.qmd"` from `index.qmd` with `index` set but
    `resolver = None`: returns the bare `output_href`
    (`"about.html"`) — no relative-depth math, but no panic.

### Unit tests — `LinkRewriteTransform`

29. `link_rewrite_skips_when_no_index` — single-doc render
    (`project_index = None`): every `Inline::Link.target.0`
    survives unchanged.
30. `link_rewrite_walks_paragraph_inlines` — `Para [Link …, Link
    …]`: both rewritten.
31. `link_rewrite_walks_nested_emph_link` — `Para [Emph [Link …]]`:
    rewritten.
32. `link_rewrite_walks_div_blocks` — `Div [Para [Link …]]`:
    rewritten.
33. `link_rewrite_walks_lists` — `BulletList [[Para [Link …]]]`:
    rewritten.
34. `link_rewrite_walks_custom_node_slots` — `Custom { slots: [
    Inlines [Link …]] }`: rewritten.
35. `link_rewrite_external_pass_through` — link with
    `target.0 = "https://example.com"` survives unchanged.
36. `link_rewrite_fragment_pass_through` — `target.0 = "#sec"`
    survives unchanged.
37. `link_rewrite_image_url_unchanged` — paragraph with `Image`
    pointing at `"img.png"`: image target unchanged. Image's alt-
    text content is walked (so a `Link` *inside* the alt would
    be rewritten); the image's own URL is not.
38. `link_rewrite_diagnostic_uses_body_link_label` — broken
    `.qmd` link in body produces diagnostic starting with
    `"Body link"`.

### Integration tests — `crates/quarto-core/tests/`

New file `link_rewriting_pipeline.rs`:

39. `pipeline_body_link_rewrites_simple_qmd` — three-page website
    `[index, about, docs/api]`. `index.qmd` body has
    `[About](about.qmd)`. After render, `index.html` contains
    `<a href="about.html">About</a>`.
40. `pipeline_body_link_rewrites_doc_relative` — `docs/api.qmd`
    body has `[About](../about.qmd)`. After render,
    `docs/api.html` contains `<a href="../about.html">About</a>`.
41. `pipeline_body_link_rewrites_subdir` — `index.qmd` body has
    `[API](docs/api.qmd)`. After render, `index.html` contains
    `<a href="docs/api.html">API</a>`.
42. `pipeline_body_link_preserves_fragment` — `index.qmd` body has
    `[Bio](about.qmd#bio)`. After render, `index.html` contains
    `<a href="about.html#bio">Bio</a>`.
43. `pipeline_body_link_preserves_query_string` — body has
    `[Search](search.qmd?q=foo)`. Output href contains
    `"search.html?q=foo"`.
44. `pipeline_body_link_external_unchanged` — body has
    `[GitHub](https://github.com)`. Output href is
    `https://github.com`, no diagnostic.
45. `pipeline_body_link_broken_qmd_emits_diagnostic` — body has
    `[Missing](nope.qmd)`. After render, output href is `nope.qmd`
    verbatim, and the render result's diagnostics list contains
    a "Body link" warning naming `nope.qmd`.
46. `pipeline_body_link_single_doc_unchanged` — bare `.qmd`
    rendered without a `_quarto.yml`. Body has `[X](other.qmd)`.
    Output href is `other.qmd` verbatim. No diagnostic. Confirms
    the standalone-render no-op contract.
47. `pipeline_body_link_absolute_path` — body has
    `[Home](/index.qmd)` from `docs/api.qmd`. Output href is
    `../index.html`.
48. `pipeline_body_link_in_list` — body has a bullet list with
    a `.qmd` link. Output href is rewritten.
49. `pipeline_body_link_no_cross_contamination` — rendering
    `index.qmd` does not affect `about.qmd`'s body links
    (regression guard, mirrors Phase 3's navbar
    cross-contamination test).

### CLI end-to-end (per CLAUDE.md §End-to-end verification)

50. **Body-link smoke** at `/tmp/q2-phase6-smoke/`:
    ```
    _quarto.yml:
      project: { type: website }
      website:
        title: "Phase 6 Smoke"
    index.qmd:    "[About me](about.qmd) — see also [API](docs/api.qmd)."
    about.qmd:    "Back to the [home page](index.qmd)."
    docs/api.qmd: "See [the about page](../about.qmd) or [home](/index.qmd)."
    ```
    Run `cargo run --bin q2 -- render /tmp/q2-phase6-smoke/` and
    inspect each rendered HTML:
    - `_site/index.html`: `<a href="about.html">About me</a>` and
      `<a href="docs/api.html">API</a>`.
    - `_site/about.html`: `<a href="index.html">home page</a>`.
    - `_site/docs/api.html`: `<a href="../about.html">…</a>` and
      `<a href="../index.html">home</a>`.
    Record observed snippets in the close-out.
51. **Broken-link smoke** at `/tmp/q2-phase6-broken-smoke/`:
    `index.qmd` body has `[X](missing.qmd)`. Verify diagnostic
    `"Body link references unknown document 'missing.qmd'"`
    appears in stderr; rendered HTML has `<a href="missing.qmd">`.
52. **Regression smokes**: re-run
    `/tmp/q2-phase2-smoke/`, `/tmp/q2-phase3-smoke/`,
    `/tmp/q2-phase4-smoke/`, `/tmp/q2-phase5-website-test/`.
    Sidebar / navbar / page-nav / `site_libs` behavior unchanged.
    Body-link rewriting now active — pages with `.qmd` body links
    (if any in those fixtures) get them rewritten.

### Snapshot tests

None — inline asserts over the emitted HTML cover the vocabulary
(consistent with Phase 2 / 3 / 4 / 5 choices).

## Work items (checklist)

### Preparation

- [x] Re-read `claude-notes/instructions/testing.md`,
      `coding.md`, `review.md`.
- [x] Confirm user agreement with Decisions 1–10. **DONE 2026-04-27.**
- [x] Create `bd` issue `Phase 6 — Cross-document link rewriting`,
      parent `bd-0tr6`, parent-child dependency linked. (`bd-v30t`.)
- [x] Commit directly on `feature/websites` (Phase 1–5 precedent).

### Resolver extension (`quarto-core/src/resource_resolver.rs`)

- [x] Add `page_url_for(target_output_href: &str) -> String`
      method (Decision 4).
- [x] Tests 1–7 (all passing).

### `RenderContext` extension (`quarto-core/src/render.rs`)

- [x] Add `resource_resolver: Option<ResourceResolverContext>`
      field (Decision 5).
- [x] Default to `None` in `RenderContext::new`.
- [x] Verified no `RenderContext { ... }` struct-literal callers
      exist (only `CslRenderContext` literals, which is a different
      type). All construction goes through `RenderContext::new`.

### Resolver wiring (`quarto-core/src/render_to_file.rs`)

- [x] Populate `ctx.resource_resolver = Some(resolver.clone())`
      immediately after the existing `ctx.project_index` assignment.
- [x] Hub-client / WASM `render_qmd_with_options` and
      `render_qmd_with_resources` also wire `ctx.resource_resolver
      = Some(resolver.clone())` for the VFS-root resolver. Two
      callsites in `crates/wasm-quarto-hub-client/src/lib.rs`
      updated (lines 977 and 1089).

### Helper module (`quarto-core/src/transforms/navigation_href.rs`)

- [x] Add `pub fn resolve_doc_relative_href(...)` per Decision 3.
- [x] Add private `fn resolve_to_project_root(...)` path
      normalizer per Decision 9. Implementation walks
      forward-slash segments (not `Path::components`) to dodge
      Windows-specific path surprises (drive prefixes, backslash
      separators) — URL paths are forward-slash by convention.
- [x] Tests 8–28 (all 21 passing).

### `LinkRewriteTransform`
      (`quarto-core/src/transforms/link_rewrite.rs` — NEW)

- [x] New module. Standalone-render skip per Decision 7.
- [x] Recursive `LinkRewriter` visitor mirroring
      `crossref_render::render_inline` and
      `resource_collector::ResourceVisitor` (Decision 6). Walks
      Block / Inline / `Inline::Custom` slots with full coverage
      of body-bearing variants (Lists, Tables, Figures, Notes,
      Captions, etc.).
- [x] Use `page_relative_source(ctx)` for the source-relative
      basis.
- [x] `mod.rs` re-export.
- [x] Tests 29–38 (all 10 passing).

### Pipeline wiring (`quarto-core/src/pipeline.rs`)

- [x] Insert `LinkRewriteTransform::new()` as the first
      transform in the Finalization Phase, before
      `AppendixStructureTransform` (Decision 2). Comment
      explains the placement contract and links to the sub-plan.
- [x] Update the doc-block enumerating Finalization Phase
      transforms (now lists Link rewrite, Appendix, Crossref,
      Resource collector).
- [x] Full quarto-core test suite green (1230 tests pass) — no
      regressions in existing transforms or integration tests.

### Integration tests
      (`quarto-core/tests/link_rewriting_pipeline.rs`)

- [x] Tests 39–49 written following the `sidebar_pipeline.rs`
      pattern (11 tests, all passing). Test 46's
      strictly-standalone "no `_quarto.yml`" contract was
      reframed: `ProjectPipeline::run` always builds a
      `ProjectIndex`, so the no-index branch is exclusively
      exercised by the unit tests
      (`link_rewrite_skips_when_no_index`,
      `body_href_no_index_passes_through`). The integration test
      that replaces it (`pipeline_body_link_unresolvable_in_website_warns`)
      covers the user-visible "broken link in a website project"
      case.

### Discovered during integration: bridge resolver through stages

- [x] Adding the resolver to `RenderContext` was not enough —
      `AstTransformsStage` rebuilds a fresh `RenderContext` from
      `StageContext` data, so the resolver had to live on
      `StageContext` too and be re-bridged. Adds:
      * `StageContext.resource_resolver: Option<ResourceResolverContext>`
        with the same docstring contract as on `RenderContext`.
      * `run_pipeline` clones `ctx.resource_resolver` into
        `stage_ctx.resource_resolver` next to `project_index`.
      * `AstTransformsStage::run` clones it back into
        `render_ctx.resource_resolver` next to `project_index`.
      Without this bridge, `LinkRewriteTransform` saw
      `ctx.resource_resolver = None` and emitted bare
      `output_href` strings instead of page-relative URLs.
      Caught by integration test
      `pipeline_body_link_rewrites_doc_relative` failing.

### CLI end-to-end + regression

- [x] Smoke fixture at `/tmp/q2-phase6-smoke/` (3 pages, root
      + nested + 2-level mix). Observed rendered HTML body links:
      * `index.html`: `href="about.html"`, `href="docs/api.html"`
      * `about.html`: `href="index.html"`
      * `docs/api.html`: `href="../about.html"`,
        `href="../index.html"`
      Matches the plan's example table 1:1.
- [x] Smoke fixture at `/tmp/q2-phase6-broken-smoke/` exercises
      the broken-link path. Rendered HTML body has
      `href="missing.qmd"` (verbatim); stderr contains
      `Warning: Body link references unknown document 'missing.qmd'`.
- [x] Re-rendered `/tmp/q2-phase2-smoke/`, `/tmp/q2-phase3-smoke/`,
      `/tmp/q2-phase4-smoke/`, `/tmp/q2-phase5-website-test/`
      under Phase 6 wiring. Sidebar / page-nav HTML structure
      unchanged in all of them (the existing fixtures don't have
      body-`.qmd` links to rewrite, so the only diff would be in
      navigation regions which Phase 6 doesn't touch). All 4
      regression fixtures render cleanly.

### Hub-client / WASM impact check

- [x] Audited `crates/wasm-quarto-hub-client/src/lib.rs`. Two
      callsites of `render_qmd_to_html` exist (in
      `render_qmd_with_options` and `render_qmd_with_resources`);
      both now wire
      `ctx.resource_resolver = Some(resolver.clone())` next to the
      VFS-root resolver creation. `hub-client/src/services/wasmRenderer.ts`
      is consumer-side only; it doesn't need changes.
- [x] `cargo xtask verify` (full, including WASM build, hub-client
      test suite, and trace-viewer build/tests) — all 9 steps
      green on a clean run. The verify-failed-but-fresh-run-passes
      transient was a vitest race in a parallel test (likely a
      pre-existing flake unrelated to Phase 6); the full re-run
      passes 562/562.

### Verification and close-out

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — **7876 tests pass** (up
      from 7827 pre-Phase-6; net +49 tests covering resolver
      `page_url_for`, path normalization, `resolve_doc_relative_href`,
      `LinkRewriteTransform` walker, and integration plumbing).
- [x] `cargo xtask lint` passes (632 files checked).
- [x] `cargo fmt --check` clean.
- [x] `cargo xtask verify` (full, including WASM build,
      hub-client `npm run build:all`, hub-client tests, and
      trace-viewer build/tests) — all 9 steps green.
- [x] No snapshot files added or modified.
- [x] Follow-ups filed (each `discovered-from:bd-v30t`,
      verified via `br dep tree`):
      * `bd-p4sc` — Body-link draft-mode visibility (priority 3,
        epic-scoped via parent-child to bd-0tr6).
      * `bd-fo1r` — Body-link index-forgiveness (priority 3,
        epic-scoped — could be unified with `bd-jbml` /
        `bd-bobp` from Phases 3 / 4).
      * `bd-nb32` — `data-noresolveinput` escape hatch
        (priority 4, epic-scoped — Q1 parity).
      * `bd-j3a0` — Diagnostic dedup by (page, href) (priority 3,
        epic-scoped — UX polish).
      * `bd-gdrv` — Cross-format URL resolution (priority 4,
        `related` to epic — out of website-epic scope, multi-
        format projects are a future epic).
      * `bd-td2a` — Footer Text-region project-link rewriting
        (priority 3, epic-scoped — `related` to `bd-jfyl` from
        Phase 5; replaces it once both are reconciled).
- [x] Updated the epic plan's "Work items" checklist — Phase 6
      marked done, sub-plan linked, `bd-v30t` referenced;
      follow-up beads logged in the running report section.
- [ ] `br close bd-v30t` with reason citing commit hash
      (deferred to commit time).
- [ ] `br sync --flush-only && git add .beads/ && git commit`
      (deferred to commit time).
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** A Lua filter that emits `.qmd` hrefs after Phase 6
  bypasses rewriting. *Mitigation:* documented limitation; if real
  workflows need this, file a second-pass transform after
  AstTransforms. Q1 has the same gap.

- **Risk:** Body-link rewriting affects standalone (single-doc)
  renders. *Mitigation:* Decision 7 standalone no-op; Test 29 + Test
  46 lock it in.

- **Risk:** Path normalization (`..` / `.`) mishandles edge cases
  (Windows paths, trailing slashes, empty segments).
  *Mitigation:* Tests 8–15 cover the main cases; cross-platform
  rule (CLAUDE.md): forward-slash everywhere on the URL side, even
  on Windows. The `Path::components()` walk is inherently OS-aware.

- **Risk:** `ctx.resource_resolver` not populated in some
  `RenderContext` construction site, causing transforms to fall
  back to the no-resolver path silently. *Mitigation:* the helper's
  no-resolver fallback is correct behavior (returns bare
  `output_href` — same as Phase 3's `resolve_href_for_html` does
  today). Audit task at close-out.

- **Risk:** Diagnostic spam — every internal misspelled `.qmd`
  link produces a warning, and a real site might have hundreds.
  *Mitigation:* matches Q1 behavior. Users can fix the typo. If
  this proves loud, file a follow-up to dedupe diagnostics by
  href.

- **Risk:** Cross-format hrefs (HTML page linking to a doc that
  outputs PDF) get rewritten as `.html` regardless. *Mitigation:*
  out of scope (non-goals). Single-format projects work; multi-
  format is a future epic.

- **Risk:** Rewriting custom-node body-link targets affects a
  custom-node consumer that expected source-relative hrefs.
  *Mitigation:* by the Finalization Phase placement, the only
  custom nodes still in the AST are the post-resolve crossref
  nodes (which use fragment hrefs, skipped) and engine outputs
  (which don't typically carry `.qmd` links). If a real
  custom-node user surfaces an issue, add a `data-noresolveinput`
  attribute escape hatch (Q1 has one).

- **Risk:** Phase 6 affects body links inside callouts /
  theorems / proof / footnotes (custom-node Slots).
  *Mitigation:* this is the *intended* behavior — body links
  inside any wrapper should rewrite. Test 34 locks this in.

- **Risk:** Performance — walking every `Inline::Link` adds a
  per-link helper call. *Mitigation:* the walk is O(N) over
  Inlines, the helper is cheap (string ops + one HashMap lookup
  via `ProjectIndex`). Compared to the engine + theme-CSS
  compilation that dominates render time, negligible.

## Explicit non-goals for this phase

- No draft-mode visibility handling (link removal for drafts).
- No index-forgiveness (`docs/` matching `docs/index.qmd`).
- No `.md` / `.ipynb` / `.Rmd` extension support (rides with
  `bd-xxul`).
- No cross-format link awareness (HTML→PDF resolution).
- No `Image::target.0` rewriting.
- No HTML post-processing path. AST-side only.
- No `data-noresolveinput` escape hatch (Q1 feature).
- No diagnostic deduplication.
- No `<base href>` / root-relative URL output.
- No incremental-rebuild interaction (Phase 8).

## Follow-up beads (to file at close-out)

- **Draft-mode visibility for body links** — when
  `DocumentProfile.draft && draftMode != "visible"`, replace the
  `<a>` with its inner content. Needs draft-mode YAML config first
  (currently no Q2 surface for it).
- **Index-forgiveness for body links** — `docs/` matches
  `docs/index.qmd`. Mirrors Phase 3's `bd-jbml` and Phase 4's
  `bd-bobp`. Consider unifying as a single epic-wide bead.
- **`data-noresolveinput` escape hatch** — Q1 lets a Lua filter or
  hand-authored HTML opt out of rewriting via this attribute.
  Phase 6 doesn't honor it; file once a real workflow surfaces.
- **Diagnostic deduplication** — if a site has many broken `.qmd`
  links, the warning list grows fast. A simple "first occurrence
  wins" dedupe per (page, href) would tame the output.
- **Cross-format link resolution** — once Q2 supports per-doc
  `format:` overrides in the project, body links targeting a
  PDF-output doc should produce `.pdf` hrefs.
- **Rich-source link rewriting** — the helper assumes
  `link.target.0` is a plain forward-slash string. If
  `Inlines`-bearing link targets ever land (Pandoc has talked
  about it), audit the rewriter.

## Open questions (resolved during implementation)

1. **Active custom nodes at Phase 6's slot — do any wrap body
   links?** *Resolved 2026-04-27.* By the start of Finalization
   Phase, `CalloutResolveTransform` and `CrossrefResolveTransform`
   have run; `FloatRefTargetSugarTransform`, `EquationLabelTransform`,
   `TheoremSugarTransform`, `ProofSugarTransform`, and shortcode
   outputs *can* leave `Inline::Custom` / `Block::Custom` nodes
   live in the AST. Those slots may carry `Inlines` containing
   `Link`s. The recursive walk handles them correctly; the unit
   test `link_rewrite_walks_custom_node_slots` locks this in.
   No trimming, no special-case test added — the contract holds.
2. **Hub-client behavior.** *Resolved 2026-04-27.* The two
   WASM callsites (`render_qmd_with_options`,
   `render_qmd_with_resources`) wire
   `ResourceResolverContext::vfs_root("/.quarto/project-artifacts")`
   into `ctx.resource_resolver`; the `page_url_for` VFS branch
   returns absolute `/.quarto/project-artifacts/<output_href>`
   URLs (test `page_url_for_vfs_root_mode` confirms the shape).
   The actual hub-client multi-doc preview flow doesn't yet pass
   a `project_index`, so body-link rewriting is a no-op in the
   browser today — Phase 9 lights it up.
3. **Diagnostic source-info.** Deferred. Today the helper still
   produces a plain `DiagnosticMessage::warning(text)` (matches
   the navigation helpers' shape from Phases 2/3/4). When source
   info is plumbed through, both helpers should switch together
   to keep diagnostic shape consistent across navigation /
   body links. Not blocking Phase 6.
4. **Per-page output href format.** `DocumentProfile.output_href`
   is forward-slash, project-relative, non-empty for renderable
   docs (verified by reading the profile contract doc and
   inspecting `DocumentProfileStage`'s output). The resolver's
   `page_url_for` and the helper both treat it as a string and
   pass it through `pathdiff` / segment-walks — no path
   reinterpretation needed.

## Decisions log (confirmed 2026-04-27)

1. **AST-side rewrite** (not HTML post-processing). Walk
   `Inline::Link` nodes; mutate `target.0` in place.
2. **Pipeline placement**: start of Finalization Phase, between
   Navigation Render transforms and `AppendixStructureTransform`.
3. **New helper** `resolve_doc_relative_href` in
   `navigation_href.rs`, alongside `resolve_href_for_html`.
4. **`page_url_for` method** added to `ResourceResolverContext`
   for page-relative URL math (mirrors `html_url_for`).
5. **`resource_resolver` field** added to `RenderContext` (and,
   discovered during integration testing, also to `StageContext`
   with bridging in `run_pipeline` and `AstTransformsStage`).
6. **`LinkRewriteTransform`** walks blocks / inlines / custom
   slots recursively, mirroring `crossref_render` and
   `resource_collector` traversal patterns.
7. **Standalone (no `project_index`) render** is a no-op.
8. **Diagnostic shape**: `source_label = "Body link"`, message
   matches `<label> references unknown document '<path>'`.
9. **Inline path-normalization helper** (~30 lines) instead of
   adding a `path-clean` crate dep. Walks forward-slash segments
   to dodge OS-specific path surprises.
10. **Page-relative output URLs** (Q1 parity); the resolver
    handles depth via `pathdiff::diff_paths`.

## Epic-level impact

Phase 6 closes the **link-resolution surface** for websites:

- Navigation hrefs (sidebar / navbar / footer / page-nav) — Phase 2/3/4
- Shared-asset URLs (`<link>` / `<script>`) — Phase 5
- Body-content links — Phase 6

After Phase 6, every `.qmd` reference in a website project — config-
sourced or content-sourced — resolves to the correct rendered URL
on every page, regardless of nesting depth.

The shared resolver (`ResourceResolverContext::page_url_for`) is the
**third consumer** of the relative-URL math Phase 5 introduced
(after `html_url_for` for assets and the on-disk-path side). When
Phase 7 lands `<link rel="canonical">` and sitemap URLs, those will
likely become a fourth consumer using `site_url + output_href`.

Phase 5's follow-up `bd-jfyl` (footer Text-region project-link
rewriting) was deferred precisely because Phase 6's helper is its
natural home: once Phase 6 ships, `bd-jfyl` is "call
`resolve_doc_relative_href` from the footer renderer's text walker".

Phase 6 also unblocks a real Q2 docs-site authoring loop: until body
links rewrite, every `[See chapter X](chapter-x.qmd)` would be a
broken link in the rendered output, blocking the docs epic
(`bd-tr81`).
