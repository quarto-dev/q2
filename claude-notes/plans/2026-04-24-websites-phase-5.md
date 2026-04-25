# Phase 5 — Scoped artifact store + `site_libs/`

**Date:** 2026-04-24
**Beads:** `bd-u5pr` (closed). Follow-ups TBD at close-out.
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-24-websites-phase-4.md`
**Status:** Implementation complete 2026-04-24. All 7827 workspace
tests pass; `cargo xtask verify` (full, incl. WASM) green.

## Goal of this phase

Give `ArtifactStore` entries a **scope** (per-page vs.
project-shared) and teach the output writer to route them
accordingly. In a website project, project-scoped artifacts go to a
single `_site/site_libs/` tree, deduplicated across pages; in a
single-doc project, everything continues to resolve under
`{stem}_files/` so nothing about today's single-doc behavior changes.

Concretely:

1. Add `ArtifactScope { Page, Project }` and attach it to every
   `Artifact`. Default = `Page`. Every existing producer gets
   `Page` on day one — **pure refactor**, no user-visible change.
2. Introduce a **project-level artifact store** on `ProjectContext`
   (`project_artifacts: ArtifactStore`). Between Pass-2 per-doc
   renders, the orchestrator **drains** `Project`-scoped artifacts
   from the per-doc `StageContext.artifacts` into the project store
   (with dedup / consistency check).
3. Introduce a **resolver** that turns `(scope, artifact_path,
   current_page)` into an HTML-side URL. This is the piece
   `ApplyTemplateStage` and the final writer consume. The resolver
   knows the project layout (site root, lib dir, per-page output
   path) and computes the right `../site_libs/...` prefix.
4. Flip producers to the right scope:
   - `CompileThemeCssStage` → `Project` scope (theme CSS is shared
     across pages in a website; identical on a single-doc render,
     which treats Project = Page).
   - `store_html_dependencies()` (extension CSS/JS from Lua filters
     / shortcodes) → `Project` scope.
   - Any *future* engine-generated image / plot → `Page` scope
     (none exist yet; note the contract in code).
5. Wire it through `WebsiteProjectType::post_render` so the
   project-scoped artifacts flush once to `_site/site_libs/`.
6. Keep single-doc / default-project behavior **byte-identical** —
   the scope machinery resolves `Project` to the same
   `{stem}_files/` layout when there's only one page.

This phase does **not** implement:

- **Vendoring of Bootstrap / `quarto-html` JS / `quarto-nav.js`**
  as standalone assets. Theme CSS (our SCSS pipeline output) is the
  only CSS artifact today, and there are no JS producers beyond
  extension dependencies. Bootstrap is embedded in the compiled
  theme CSS via the existing theme pipeline. If a later phase adds
  JS bundles (e.g. navbar collapse JS from `bd-9m8p`), they slot
  into the scope machinery built here.
- **Cross-document link rewriting** in body content — that's
  Phase 6. Phase 5 only touches `<link>` and `<script>` tags in
  `<head>` (which are template substitutions, not body transforms).
- **Incremental re-use of `site_libs/` artifacts across project
  runs** — that's Phase 8. Phase 5 rewrites the directory on every
  build.
- **Per-page resource scope for engine outputs** (figures, cached
  plots). The contract exists from day one, but there's no
  producer to verify it until an engine phase fills that seat.
- **`project.lib-dir:` config override.** Phase 5 hardcodes the
  name via `ProjectType::lib_dir()` (`"site_libs"` for website,
  `"{stem}_files"` for default). Exposing the override is a
  follow-up.

## Reference material

- **Parent epic plan** §"Phase 5 — Scoped artifact store and
  `site_libs/`" and §"Architecture sketch / Artifact store scoping
  and relocation".
- **Q2 current code:**
  - `crates/quarto-core/src/artifact.rs` — `Artifact`,
    `ArtifactStore` today. Path is `Option<PathBuf>` relative to
    the per-page resource dir; we'll add `scope` alongside.
  - `crates/quarto-core/src/stage/stages/compile_theme_css.rs` —
    the only non-Lua producer of `css:default`. Stores with
    `.with_path(DEFAULT_CSS_ARTIFACT_PATH)`.
  - `crates/quarto-core/src/dependency.rs`
    (`store_html_dependencies`) — producer for extension CSS/JS
    artifacts keyed `css:<name>:<file>` / `js:<name>:<file>`.
  - `crates/quarto-core/src/stage/stages/apply_template.rs:154-183`
    — **consumer**. Builds `css_paths` / `script_paths` for the
    template by iterating `get_by_prefix("css:")` / `("js:")` and
    prepending `self.config.resource_prefix`. This is the main
    integration point.
  - `crates/quarto-core/src/render_to_file.rs:219-279` —
    **writer**. Computes `resource_prefix = format!("{}_files/",
    output_stem)`, calls `prepare_html_resources` (which creates
    `{stem}_files/`), writes `css:default` to
    `{stem}_files/styles.css`, then iterates remaining `css:*` /
    `js:*` artifacts and writes each to
    `resource_paths.resource_dir.join(path)`.
  - `crates/quarto-core/src/project/mod.rs:49-54` —
    `default_output_dir`: website → `dir.join("_site")`, others
    → `dir`.
  - `crates/quarto-core/src/project/orchestrator.rs:78-155` —
    `ProjectType` trait (no `lib_dir` yet), `DefaultProjectType`,
    `WebsiteProjectType`, `project_type_for`. Trait methods are
    already `async_trait(?Send)`.
  - `crates/quarto-core/src/project/orchestrator.rs:229-361` —
    `ProjectPipeline::run` / `pass_one` / `pass_two`. The place
    to thread the per-doc → project artifact drain.
  - `crates/quarto-core/src/pipeline.rs:83-100` —
    `HtmlRenderConfig { css_paths, resource_prefix }`. Phase 5
    replaces the single-string `resource_prefix` with a richer
    resolver context.
  - `crates/quarto-core/src/resources.rs` —
    `prepare_html_resources`, `resource_dir_name(stem) =
    "{stem}_files"`. Centralized today.
- **Q1 reference:**
  - `external-sources/quarto-cli/src/project/types/website/website.ts:111`
    — `libDir: "site_libs"`.
  - `external-sources/quarto-cli/src/project/types/book/book.ts:114`
    and `.../manuscript/manuscript.ts:136` — same `libDir`.
  - `external-sources/quarto-cli/src/project/project-context.ts:292-297`
    — project-type `libDir` default flows into `projectConfig.project[kProjectLibDir]`.
  - Observed Q1 output shape for a rendered website:
    ```
    _site/
      index.html
      about.html
      docs/api.html
      site_libs/
        quarto-html/
          quarto.min.css
          quarto.min.js
          quarto-html.css (theme CSS lives here)
        bootstrap/
          bootstrap.min.css  bootstrap-icons.css  …
        quarto-nav/
          quarto-nav.js
        clipboard/
          clipboard.min.js
      docs/api_files/            ← per-page engine outputs only
    ```
    Our MVP produces a subset of this tree: Phase 5 only emits the
    artifacts that our producers actually generate today — theme
    CSS (landing at `site_libs/quarto/styles.css`, see Decision 5)
    and extension deps (at `site_libs/libs/{name}/{file}`).

## Key decisions (to confirm with user)

These are proposed — please push back on anything that looks wrong
before we start.

### Decision 1 — `ArtifactScope` is a field on `Artifact`

Add `pub scope: ArtifactScope` to `Artifact` with `Default ==
ArtifactScope::Page`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtifactScope {
    #[default]
    Page,
    Project,
}
```

Producers tag at creation (`.with_scope(ArtifactScope::Project)`).
Consumers / writers branch on scope.

**Rationale.** One field, one place to decide. Alternative
considered: two separate stores (`ArtifactStore` per scope).
Rejected because every consumer (ApplyTemplate, writer) would need
to traverse both, and the keys collide semantically (same namespace
`css:*`). One store + scope tag keeps the API flat.

### Decision 2 — Per-doc render returns its drained store; orchestrator merges sequentially

The orchestrator (`ProjectPipeline`) owns a project-wide artifact
store, distinct from the per-document `StageContext.artifacts`:

```rust
pub struct ProjectPipeline<'a> {
    // existing fields …
    project_artifacts: ArtifactStore,   // NEW
}
```

**Implementation note (added during implementation):** an earlier
draft put this field on `ProjectContext`. That would force a
mechanical refactor of ~130 struct-literal sites across the
workspace. Putting it on `ProjectPipeline` is strictly better:
- Workers (Pass-2 per-doc renders) never see the field, so they
  can't accidentally touch it. Reinforces Decision 2's
  parallelism contract.
- `ProjectContext` stays "what's needed to render a doc" — stable
  across Pass-2. Project-level *transient* state (artifact
  accumulator, output summaries) lives on the orchestrator.
- Test fixtures that build `ProjectContext` literals are
  unchanged.

**Parallelism contract** (locked in now to avoid a foreseeable
redesign): each Pass-2 render is **stateless** with respect to the
project artifact store. A worker rendering `intro.qmd` produces a
per-doc `StageContext.artifacts` that already separates Page from
Project scope (via the scope tag). When that worker finishes, it
returns its **drained Project-scoped artifacts** as a value
alongside the `RenderToFileResult`:

```rust
fn render_document_to_file(…) -> Result<(RenderToFileResult, ArtifactStore)>
//                                                          ^^^^^^^^^^^^^^^
//                                          drained Project-scoped artifacts
```

The orchestrator merges these returned stores into
`project.project_artifacts` **after** the worker returns, in the
single-threaded join section. No worker ever touches
`project.project_artifacts` directly.

This shape:

- Composes cleanly with sequential Pass-2 today.
- Composes cleanly with `rayon` / `pollster-per-worker` parallelism
  tomorrow — each worker writes only to its own stack, the merge
  is a sequential reduce.
- Keeps the byte-equality check (D3) in the merge step, where
  there's no contention and no lock.

Drain operation (in the merge step):

```rust
fn merge_into_project(
    drained: ArtifactStore,
    project: &mut ArtifactStore,
) -> Result<()> {
    for (key, artifact) in drained.into_iter() {
        if let Some(existing) = project.get(&key) {
            // Two docs produced the same project-scoped key;
            // require byte-equality. Mismatch = hard error.
            if existing.content != artifact.content {
                return Err(…);  // Decision 3
            }
        } else {
            project.store(key, artifact);
        }
    }
    Ok(())
}
```

**Rejected alternatives:**

- `Arc<Mutex<ArtifactStore>>` shared across workers. Lock
  contention is low in practice, but introduces a synchronization
  point on what should be an embarrassingly-parallel render.
- Each worker writes its `site_libs/` contributions directly to
  disk. Defeats dedup, races on write, and ties Pass-2 to a
  filesystem (bad for the hub-client VFS path).

**Memory bound.** Drain happens immediately as each Pass-2 render
returns — peak extra memory ≈ (max parallelism) × (one doc's
Project-scoped artifact bytes). For a website with a 200 KB theme
CSS and 8 workers, that's ~1.6 MB held during the wave. Trivial.

### Decision 3 — Dedupe policy: byte-equal content wins, mismatch is an error

When two docs emit the same key with different bytes (say, two
versions of `css:libs:kbd:kbd.css`), we fail the render with a
clear diagnostic naming both docs. No "last-writer-wins" silent
overwrites.

**Why this is safe under the keying scheme (Decision 9 below):**
keys identify the asset (`css:theme:<fingerprint>`,
`css:libs:<extension>:<file>`), not just the role. Two docs using
identical theme inputs produce identical fingerprints → identical
keys → identical bytes → byte-equality check passes → one entry
kept. Two docs using *different* themes produce different keys →
both coexist. A byte mismatch under the same key means the same
asset name is being produced from different inputs, which is
genuinely a user-visible bug (e.g. an extension vendoring two
incompatible versions of the same file under the same name).

**Performance cost.** The check is a `Vec<u8>` `==` per drained
Project artifact per doc. For a 100-doc website with a 200 KB
theme CSS plus a few extension deps, worst case is ~100 × ~250 KB
= ~25 MB of byte comparisons total across the entire build, all
against already-hot cache lines. This is negligible relative to
the engine execution and SCSS compilation that dominate render
time. No pre-optimization warranted.

**If profiling later shows the equality dominates** (unlikely),
we can add a content-hash field to `Artifact` populated at
producer time and compare hashes only. Drop-in replacement; not
worth doing now.

### Decision 4 — Add `fn lib_dir(&self) -> String` to `ProjectType`

Owned `String` return — not `&'static str` — because the
follow-up `project.lib-dir:` user-config override (already filed
as a follow-up bead in §"Follow-up beads") will read the value
out of `ProjectContext.config`. A `'static` return would force a
later API change; owned `String` lets us swap implementations
without touching callers.

```rust
pub trait ProjectType {
    fn kind(&self) -> ProjectKind;
    fn lib_dir(&self) -> String;                 // NEW (owned)
    // existing async hooks …
}

impl ProjectType for DefaultProjectType {
    fn lib_dir(&self) -> String { String::new() }
}

impl ProjectType for WebsiteProjectType {
    fn lib_dir(&self) -> String { "site_libs".to_string() }
}
```

The cost of an extra heap allocation per project render is
irrelevant — `lib_dir()` is called O(docs) times per build, and
the resolver caches the result anyway. When the override lands,
the implementation reads `config.metadata.get("project.lib-dir")`
without touching the trait signature.

`DefaultProjectType::lib_dir()` returns the empty string —
default projects have no separate lib dir; everything resolves
under `{stem}_files/` via the resolver's single-doc shortcut.
The trait method is only consulted when a project actually has a
multi-doc shared layout (Website, eventually Book / Manuscript).

### Decision 5 — `site_libs/` subdirectory layout (MVP subset)

Artifacts resolve under `site_libs/` like this:

| Artifact key                 | MVP on-disk path                                   |
|------------------------------|----------------------------------------------------|
| `css:default`                | `site_libs/quarto/styles.css`                      |
| `css:<name>:<file>`          | `site_libs/libs/<name>/<file>`                     |
| `js:<name>:<file>`           | `site_libs/libs/<name>/<file>`                     |

Rationale:

- **`site_libs/quarto/styles.css`**: keeps the theme CSS bundled
  under a namespace we control. Q1 uses
  `site_libs/quarto-html/quarto-html.css`; we're not splitting
  `quarto-html/quarto.min.css` out of the theme anyway, so a
  single `quarto/styles.css` is cleaner. If future phases vendor
  `quarto.min.js`, add `site_libs/quarto/quarto.min.js` next to
  it. (The exact subdirectory name is the least-load-bearing bit
  of this plan — happy to pick whatever name the user prefers.)
- **`site_libs/libs/<name>/<file>`**: the extension-deps subtree
  mirrors Q1's layout and preserves the artifact's existing
  `path` shape (`libs/<name>/<file>`). We just re-root it under
  `site_libs/` instead of `{stem}_files/`.

Single-doc / default project: both resolve to `{stem}_files/` per
today's layout. No visible change.

### Decision 6 — Replace `HtmlRenderConfig.resource_prefix: &str` with a resolver context

Today `resource_prefix` is a single string like `"test_files/"`
that `ApplyTemplateStage` prepends to every artifact path. This
assumption breaks as soon as a page lives at `docs/api.html` and
needs `../site_libs/...`.

Proposed replacement:

```rust
pub struct ResourceResolverContext {
    /// Absolute output path of the current page (e.g. "_site/docs/api.html").
    pub page_output: PathBuf,
    /// Site root (e.g. "_site/"), i.e. where `site_libs/` lives.
    pub site_root: PathBuf,
    /// Name of the project lib dir (from `ProjectType::lib_dir()`).
    pub lib_dir: &'static str,
    /// Per-page fallback (e.g. "api_files"), used for Page-scoped artifacts.
    pub page_files_dir: String,
}

impl ResourceResolverContext {
    pub fn html_url_for(&self, scope: ArtifactScope, artifact_path: &Path) -> String { … }
    pub fn on_disk_path_for(&self, scope: ArtifactScope, artifact_path: &Path) -> PathBuf { … }
}
```

`ApplyTemplateStage` takes this instead of the bare `resource_prefix`
string. The final writer in `render_to_file.rs` (well, what's left
of it after Phase 5) consults `on_disk_path_for` for per-doc write,
or defers the write to the project orchestrator for `Project` scope.

Backwards compat for callers that don't care about the
single-string shortcut: we keep a small convenience
`ResourceResolverContext::single_doc(output_path)` helper that
reproduces today's behavior.

### Decision 7 — URL resolution at template-apply time; disk write split per-scope

This decision is the most subtle one in the phase, so it spells
out two separable concerns explicitly:

| Concern | Where it happens | Reads from |
|---------|-----------------|------------|
| `<link>` / `<script>` URL in rendered HTML | `ApplyTemplateStage` (per-doc, in-pipeline) | per-doc `StageContext.artifacts` (still has scope tags) |
| File at `_site/site_libs/...` on disk | `WebsiteProjectType::post_render` (once per project) | `project.project_artifacts` (drained from per-doc) |

**No HTML inspection or post-render path-fixing is required.** The
risk you'd worry about — needing a Deno-dom-style HTML walker to
fix `<link>` paths after the fact — is avoided by construction:
when `ApplyTemplateStage` runs, the per-doc store still carries
all of that doc's artifacts with their scope tags attached. The
resolver translates `(scope, artifact_path, page_output_path)` into
the correct relative URL **before any HTML is emitted**:

- Project-scoped `quarto/cosmo.css` for a page at `_site/index.html`
  → `<link href="site_libs/quarto/cosmo.css">`.
- Project-scoped `quarto/cosmo.css` for a page at
  `_site/docs/api.html` → `<link href="../site_libs/quarto/cosmo.css">`.
- Page-scoped `figure-html/fig-1.png` for any page → relative path
  to that page's `{stem}_files/`.

The drain into `project.project_artifacts` happens *after*
`ApplyTemplateStage` has already produced correct URLs. The drain
is purely about **on-disk write coordination** ("which file gets
written where, once"), not URL computation.

**Concretely:**

- After each Pass-2 doc's pipeline returns,
  `render_document_to_file` writes the HTML and all **Page-scoped**
  artifacts to `{stem}_files/` — same as today.
- **Project-scoped** artifacts are drained out of the per-doc
  store and returned to the orchestrator (Decision 2), which
  merges them into `project.project_artifacts`.
- `WebsiteProjectType::post_render` walks the project-level store
  and writes everything under `_site/site_libs/`. Runs exactly
  once per project render. Phase 7 hooks (sitemap, favicon) plug
  in here too.
- For a `DefaultProjectType` single-doc render (the
  `is_single_file` path), the drain still runs mechanically — but
  the resolver, when constructed for a default project, has been
  told `lib_dir == ""` so Project scope resolves under
  `{stem}_files/`. The drained artifacts then have nowhere
  project-shared to go; the per-doc writer treats them as if they
  were Page-scoped (or, equivalently, the orchestrator flushes
  the project store into `{stem}_files/` because that's what the
  resolver says). Either implementation is fine; the user-visible
  output is identical to today.

**Format-specificity caveat (acknowledged):** `post_render` is
HTML-specific in that the asset layout it writes (`site_libs/...`)
only makes sense for HTML output. For non-HTML formats (PDF,
docx) the website epic doesn't yet have a story; when it does,
each format's `post_render` writes the layout that format needs.
But because URL rewriting happens at template-apply time (when
the producer's scope tag is still attached to the artifact), no
format ever needs to re-inspect its own output to fix paths. The
contract holds across formats.

### Decision 8 — Diagnostic path: resolver vs. `source_label`

When the resolver can't compute a relative path (malformed input
path, project not discovered), emit a diagnostic with
`source_label = "Resource resolver"` — mirrors
Phase 3's `"Sidebar"` / Phase 4's `"Page navigation"` convention
for `navigation_href::resolve_href_for_html`. This is for the
Phase 6 link-rewriter to share later.

### Decision 9 — Theme CSS keyed by content fingerprint; retire `css:default`

`css:default` was a singleton key, which works for one-doc-one-
theme but breaks the moment a website mixes themes (e.g. doc A
`theme: cosmo`, doc B `theme: darkly`). Q1 hit this exact issue
and resolved it by hash-suffixing CSS dependency names; we adopt
the same pattern from the start.

`CompileThemeCssStage` produces:

- **Key**: `css:theme:<fingerprint>` where `<fingerprint>` is a
  short hash of all SCSS-compilation **inputs** for that doc:
  - the resolved theme name(s) (`cosmo`, `darkly`, …),
  - any user-added SCSS layers / files declared in metadata
    (Q1 supports `theme: [cosmo, custom.scss]` and additional
    SCSS via `format.html.theme.brand` / `theme.scss`; Q2 will
    eventually too),
  - the Bootstrap version baked into our pipeline,
  - any theme-affecting variables resolved from merged metadata.
- **Path**: `quarto/quarto-theme-<fingerprint>.css`.
- **Scope**: `Project`.

Behavior under your three-doc example:

| Doc | Theme inputs | Fingerprint | Outcome |
|-----|--------------|-------------|---------|
| `intro.qmd` | `cosmo` | `abc123` | First sighting; stored. |
| `methods.qmd` | `cosmo` | `abc123` | Same key + same bytes; dedup, one entry. |
| `appendix.qmd` | `darkly` | `def456` | Different key; coexists. |

Output:
```
_site/site_libs/quarto/quarto-theme-abc123.css   ← cosmo
_site/site_libs/quarto/quarto-theme-def456.css   ← darkly
```

Each doc's `<link>` resolves to its own fingerprint at template-
apply time (because `ApplyTemplateStage` sees only its own
artifact during its pipeline run).

**The `css:default` constant goes away.** Its sole role was as a
sentinel for "the doc's theme CSS"; now there's no sentinel —
the doc has a themed CSS artifact with a real, content-derived
key, and the writer treats it identically to any other Project-
scoped CSS artifact.

**Hash inputs must be stable.** The fingerprint is a hash over a
canonical serialization of the inputs (sorted, normalized
whitespace where applicable). Implementation will pick a concrete
hash (likely `xxh3` or `blake3`) and a serialization scheme.
Stable across Quarto versions is *not* required (CSS bundling can
change between releases); stable across runs of the same Quarto
version *is* required (same inputs → same fingerprint → dedup
works).

**WASM impact.** `DEFAULT_CSS_ARTIFACT_PATH` is the synthetic URL
the WASM path uses for the in-memory theme CSS. With `css:default`
gone, WASM consumers need to: (a) iterate `css:theme:*` artifacts
to find the doc's theme CSS, or (b) keep the synthetic URL as a
hub-client-side convention pointing at "whatever theme CSS this
doc has". The audit task in §"Work items / WASM impact check"
covers the migration.

### Decision 10 — Single-doc behavior is locked to byte-identical

A **regression test in Phase 5** renders a single-doc fixture end-
to-end (same fixture used in Phase 2/3/4 smokes) and asserts
every byte of the generated HTML / CSS / extension-dep file is
identical to a pre-Phase-5 baseline snapshot. We capture the
baseline first, commit it, then refactor underneath.

**Rationale.** The #1 risk the epic plan calls out for Phase 5 is
that the refactor regresses single-doc rendering. A pixel-identity
test is cheaper than a review and more durable than a unit test.

## Architecture sketch

### Producer changes

```diff
 // CompileThemeCssStage
+let fingerprint = theme_fingerprint(&theme_inputs);  // see Decision 9
+let key = format!("css:theme:{fingerprint}");
+let path = format!("quarto/quarto-theme-{fingerprint}.css");
 ctx.artifacts.store(
-    "css:default",
+    key,
     Artifact::from_string(theme_css, "text/css")
-        .with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
+        .with_path(PathBuf::from(path))
+        .with_scope(ArtifactScope::Project),
 );
```

```diff
 // store_html_dependencies
 ctx.artifacts.store(
     format!("css:{name}:{filename}"),
     Artifact::from_bytes(content, "text/css")
-        .with_path(PathBuf::from(format!("libs/{name}/{filename}"))),
+        .with_path(PathBuf::from(format!("libs/{name}/{filename}")))
+        .with_scope(ArtifactScope::Project),
 );
```

Everything else stays `ArtifactScope::Page` by default.

### Consumer changes (ApplyTemplateStage)

```diff
-let prefix = &self.config.resource_prefix;
 for (key, artifact) in ctx.artifacts.get_by_prefix("css:") {
-    if key == "css:default" { continue; }
     if let Some(path) = &artifact.path {
-        css_paths.push(format!("{}{}", prefix, path.to_string_lossy()));
+        css_paths.push(resolver.html_url_for(artifact.scope, path));
     }
 }
```

Note that the `css:default` skip goes away with the
fingerprinted-theme keying (Decision 9): every CSS artifact —
including the doc's own theme CSS — flows through the same
resolver path. The CSS-paths list output by `ApplyTemplateStage`
no longer needs a synthetic "default" placeholder.

(Resolver stored on `StageContext` or passed via `ApplyTemplateConfig`.)

### Writer changes (render_to_file)

```diff
-// Write extension CSS/JS dependency artifacts (e.g., libs/kbd/kbd.css)
-for (key, artifact) in ctx.artifacts.iter() {
-    if key == "css:default" { continue; }
-    …
-    let output_path = resource_paths.resource_dir.join(path);
-    …
-}
+// Write Page-scoped artifacts per-doc. Project-scoped artifacts
+// are drained into project_artifacts (see orchestrator) and
+// flushed once in post_render.
+for (key, artifact) in ctx.artifacts.iter() {
+    if artifact.scope == ArtifactScope::Project { continue; }
+    if !(key.starts_with("css:") || key.starts_with("js:")) { continue; }
+    let Some(path) = &artifact.path else { continue; };
+    let output_path = resolver.on_disk_path_for(ArtifactScope::Page, path);
+    runtime.file_write(…, &artifact.content)?;
+}
```

### Orchestrator changes

```diff
 // ProjectPipeline::pass_two
 for doc_info in &self.project.files {
     match render_document_to_file(…) {
-        Ok(result) => outputs.push(result),
+        Ok((result, drained)) => {
+            // Sequential merge — workers never touch project state.
+            // Composes with future rayon-per-worker parallelism: each
+            // worker's `drained` is private until merge.
+            merge_into_project(
+                drained,
+                &mut self.project.project_artifacts,
+            )?;
+            outputs.push(result);
+        }
         …
     }
 }
```

`render_document_to_file` returns
`Result<(RenderToFileResult, ArtifactStore)>`. The second element
is the doc's drained Project-scoped artifacts; the orchestrator
merges them sequentially. The merge step is the *only* place that
holds `&mut self.project.project_artifacts`, satisfying the
parallelism contract from D2.

### `WebsiteProjectType::post_render` body

```rust
async fn post_render(
    &self,
    project: &ProjectContext,
    _index: &ProjectIndex,
    _outputs: &[RenderToFileResult],
) -> Result<()> {
    let lib_root = project.output_dir.join(self.lib_dir());
    for (_key, artifact) in project.project_artifacts.iter() {
        let Some(path) = &artifact.path else { continue; };
        let out_path = lib_root.join(path);
        if let Some(parent) = out_path.parent() {
            runtime.dir_create(parent, true)?;
        }
        runtime.file_write(&out_path, &artifact.content)?;
    }
    Ok(())
}
```

Phase-7 hooks (sitemap, favicon) slot into this same method later.

### Data flow summary

```
Per-doc pipeline ──► StageContext.artifacts (mixed scopes)
                             │
                             ├── Page-scoped ──► writer emits
                             │                   to {stem}_files/
                             │                   per-doc
                             │
                             └── Project-scoped ──► drained into
                                                    ProjectContext
                                                    .project_artifacts

After Pass 2 ──► WebsiteProjectType::post_render ──► walk project_artifacts
                                                     write each under _site/site_libs/…
```

## DocumentProfile change

**None.** Phase 5 reshapes machinery beneath the pipeline stages;
the profile is unaffected. No `profile_version` bump.

## Tests (TDD: write and fail first)

### Unit tests — `ArtifactScope` + `Artifact.scope`

1. `artifact_default_scope_is_page` — `Artifact::from_string(…).scope
   == ArtifactScope::Page`.
2. `artifact_with_scope_builder` — `.with_scope(Project)` sets scope
   without touching content/path.
3. `artifact_scope_round_trips_through_store` — store/get preserves
   scope.

### Unit tests — `ResourceResolverContext`

4. `resolver_single_doc_html_url_matches_today` — with
   `site_root == page_output.parent()` and
   `page_files_dir == "doc_files"`, resolving `Page`-scoped
   `libs/kbd/kbd.css` returns `"doc_files/libs/kbd/kbd.css"`.
5. `resolver_single_doc_project_scope_falls_back_to_page_files` —
   Project scope in a `DefaultProjectType` also resolves under
   `{stem}_files/` (the "single-doc = no separation" invariant).
6. `resolver_website_root_page_project_scope` — page at
   `_site/index.html`, Project-scope `quarto/styles.css` → URL
   `"site_libs/quarto/styles.css"`.
7. `resolver_website_nested_page_project_scope` — page at
   `_site/docs/api.html`, Project-scope `quarto/styles.css` → URL
   `"../site_libs/quarto/styles.css"`.
8. `resolver_website_deeply_nested_page` — `_site/a/b/c/d.html` →
   `"../../../site_libs/quarto/styles.css"`.
9. `resolver_on_disk_path_project_scope` — Project-scope
   `libs/kbd/kbd.css` resolves on disk to
   `<site_root>/site_libs/libs/kbd/kbd.css`.
10. `resolver_on_disk_path_page_scope` — Page-scope
    `figure-1.png` resolves to
    `<site_root>/<page_files_dir>/figure-1.png`.

### Unit tests — drain + merge

11. `drain_returns_only_project_scoped` — per-doc store with 1
    Page + 1 Project entry: drain returns the Project one,
    leaves the Page in the per-doc store.
12. `merge_dedupes_byte_equal_keys` — merge two drained stores
    with same key + same bytes: project store has one entry;
    second merge is a no-op.
13. `merge_errors_on_byte_mismatch` — merge two drained stores
    with same key + different bytes: returns `Err(…)` naming the
    key.
14. `drain_preserves_artifact_metadata_path` — roundtrip through
    drain + merge keeps `path`, `content_type`, `metadata`.

### Unit tests — theme fingerprint

15a. `fingerprint_stable_for_identical_inputs` — same theme name
     + same SCSS layers → same fingerprint across runs.
15b. `fingerprint_differs_for_different_themes` — `cosmo` vs.
     `darkly` → different fingerprints.
15c. `fingerprint_differs_for_added_scss_layer` — `cosmo` alone
     vs. `cosmo` + custom user SCSS → different fingerprints.
15d. `fingerprint_input_canonicalization` — list ordering /
     whitespace differences in the input metadata that *should*
     produce the same theme produce the same fingerprint.

### Unit tests — `ProjectType::lib_dir`

15. `website_project_type_lib_dir_is_site_libs` — owned `String`,
    value `"site_libs"`.
16. `default_project_type_lib_dir_is_empty` — owned `String`,
    empty value.

### Integration tests — `crates/quarto-core/tests/`

New file `artifact_scoping_pipeline.rs`:

17. `single_doc_render_unchanged_under_scope_refactor` —
    regression snapshot. Render `phase5-single-doc-fixture/doc.qmd`
    and diff every generated file byte-for-byte against a
    pre-refactor baseline captured into the test fixture. **This
    is the single most important test in Phase 5.**
18. `website_render_emits_site_libs_dir` — three-page website;
    after render, `_site/site_libs/quarto/quarto-theme-<hash>.css`
    exists with the expected theme CSS content.
19. `website_render_deduplicates_extension_css` — two pages each
    reference an extension providing `libs/kbd/kbd.css`. Output has
    exactly one file at `_site/site_libs/libs/kbd/kbd.css` with
    that extension's bytes.
19b. `website_render_emits_two_themes_when_docs_differ` — three-
     page website where `intro.qmd` and `methods.qmd` use
     `theme: cosmo` and `appendix.qmd` uses `theme: darkly`.
     Output has exactly two themed CSS files under
     `_site/site_libs/quarto/` (one per fingerprint), each doc
     links to the right one. Direct test of the
     fingerprint-based dedup.
20. `website_nested_page_links_css_with_relative_path` — render a
    website with `docs/api.qmd` → `_site/docs/api.html`. Inspect
    the emitted HTML: `<link rel="stylesheet"
    href="../site_libs/quarto/quarto-theme-<hash>.css">`.
21. `website_root_page_links_css_with_direct_path` — `index.qmd` →
    `_site/index.html` with
    `href="site_libs/quarto/quarto-theme-<hash>.css"`.
22. `website_merge_byte_mismatch_is_hard_error` — construct a
    fixture where two docs would generate the *same* artifact key
    with *different* bytes (e.g. via a custom transform). Render
    fails with a diagnostic naming the key and both source docs.
    (Note: under the fingerprint scheme this is hard to trigger
    organically with theme CSS — different inputs produce
    different keys, so the byte-equality check only fires on
    genuinely-identical-key/different-bytes bugs. A focused
    fixture is needed.)
23. `website_no_per_page_files_dir_when_no_page_artifacts` —
    MVP contract: if a page has no Page-scoped artifacts, we do
    not create an empty `{stem}_files/` for it (today's behavior
    is "always create"; this is a small cleanup we can ship here,
    or defer — see Open question 5).

### CLI end-to-end (per CLAUDE.md §End-to-end verification)

24. **Baseline capture** at `/tmp/q2-phase5-baseline/`: before any
    code change, render Phase-4 smoke fixtures to disk, capture
    `find /tmp/q2-phase5-baseline/ -type f` + sha256 of each
    generated file. Store as a snapshot inside the test fixture.
25. **Post-refactor smoke** at `/tmp/q2-phase5-smoke/`: after the
    refactor, re-render, diff against baseline. Expected diff for
    the website fixture: files have moved from
    `{stem}_files/styles.css` to `site_libs/quarto/styles.css`,
    and `<link>` hrefs in HTML reflect that. Expected diff for the
    single-doc fixture: zero — byte identical.
26. **Extension dep smoke**: render a website fixture whose
    `_quarto.yml` loads a shortcode / Lua filter that pulls in an
    HTML dependency (`kbd` is the canonical existing test
    extension). Verify `_site/site_libs/libs/kbd/kbd.css` exists
    and pages link to it via correct relative paths.
27. **Regression:** re-run `/tmp/q2-phase2-smoke/`,
    `/tmp/q2-phase3-smoke/`, `/tmp/q2-phase4-smoke/`. Website
    fixtures now emit `site_libs/`; sidebar / navbar / footer /
    page-nav behavior is otherwise unchanged (snapshot the HTML
    body content, ignore `<link>` hrefs in the diff).

### Snapshot tests

One new insta snapshot (or inline assertion) covering the exact
rendered `<link>` / `<script>` block for a nested-page website
render. This pins the relative-path computation against
regressions.

## Work items (checklist)

### Preparation
- [x] Re-read `claude-notes/instructions/testing.md`, `coding.md`,
      `review.md`.
- [x] Confirm user agreement with Decisions 1–10. **DONE
      2026-04-24** — D2/D3/D4/D7/D9 revised mid-conversation
      based on user feedback; user approved revisions.
- [x] Create `bd` issue `Phase 5 — Scoped artifact store +
      site_libs/`, parent `bd-0tr6`, parent-child dependency
      linked. (`bd-u5pr`.)
- [x] Commit directly on `feature/websites` (Phase 1/2/3/4
      precedent).

### Baseline capture (before any code change)
- [x] Add single-doc fixture under
      `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/`
      + `expected_hashes.txt` capturing the pre-refactor sha256s
      (commit `7881178e`).
- [x] Add website fixture
      `crates/quarto-core/tests/fixtures/phase5-website-baseline/`
      with `_quarto.yml` + 3 pages + `PRE_PHASE5_OUTPUT.md`
      documenting the pre-/post-refactor layout shift.

### Data model (`quarto-core/src/artifact.rs`)
- [x] `pub enum ArtifactScope { Page, Project }` + `Default`.
- [x] `Artifact.scope: ArtifactScope` field + `with_scope()`
      builder.
- [x] `ArtifactStore` helpers: `project_scoped_keys()`,
      `page_scoped_keys()`, `drain_project_scoped() -> ArtifactStore`,
      `merge_into_project(other) -> Result<MergeStats, ArtifactMergeConflict>`.
- [x] Tests 1–3, 11–14 (all 7 passing).

### Resource resolver (`quarto-core/src/resource_resolver.rs` — NEW)
- [x] `ResourceResolverContext` struct + `html_url_for` /
      `on_disk_path_for` methods.
- [x] `single_doc(output_path, stem)` convenience constructor.
- [x] `website(site_root, page_output, lib_dir, page_stem)`
      constructor.
- [x] **Bonus** `vfs_root(root)` constructor for the WASM
      hub-client's synthetic-VFS-path convention (added during
      task #13).
- [x] Tests 4–10 + 2 vfs_root tests (11 passing).

### ProjectType extension (`quarto-core/src/project/orchestrator.rs`)
- [x] Add `fn lib_dir(&self) -> String` to trait (Decision 4).
      Owned `String` per D4 revision (so the future user-config
      override doesn't churn the trait signature).
- [x] `WebsiteProjectType::lib_dir` returns `"site_libs"`.
- [x] `DefaultProjectType::lib_dir` returns `""`.
- [x] Tests 15–16 (passing).

### Project artifact store ownership
- [x] Add `project_artifacts: ArtifactStore` field on
      `ProjectPipeline` (revised during implementation —
      originally planned for `ProjectContext` but moved to the
      orchestrator to keep `ProjectContext` immutable across
      Pass-2 and avoid a 130-site mechanical refactor of struct
      literals; matches D2's parallelism contract).
- [x] Drain from per-doc into project store in `pass_two`. The
      per-doc `render_document_to_file` accepts an
      `Option<&mut ArtifactStore>` argument: when `Some` AND the
      project type has a non-empty `lib_dir()`, it merges drained
      Project-scoped artifacts into the orchestrator's
      accumulator; otherwise it flushes them via the resolver
      (default-project / standalone-call paths).

### Producer flips
- [x] `CompileThemeCssStage`: switch to fingerprinted key
      `css:theme:<fingerprint>` (16-hex truncation of SHA-256
      over the compiled CSS bytes), scope `Project`. Path is
      `quarto/quarto-theme-<fingerprint>.css` for multi-doc
      projects and bare `styles.css` for single-doc (per
      Decision 10's byte-identity requirement).
- [x] Hash function: SHA-256 (already a workspace dep via
      `sha2`); 16 hex char truncation. No xxh3/blake3 needed.
- [x] `store_html_dependencies`: scope `Project`, path
      unchanged (`libs/<name>/<file>`). Keys retain the
      `css:<name>:<file>` / `js:<name>:<file>` shape from
      Phase 4 — Phase 5 didn't need to renamespace them since
      they don't collide with theme keys.
- [x] `DEFAULT_CSS_ARTIFACT_PATH` constant — kept (still used
      by hub-client's vfs_root resolver argument). Retiring it
      entirely would force a hub-client convention break that
      isn't worth Phase-5 scope.

### Consumer flip (`ApplyTemplateStage`)
- [x] Replaced `config.resource_prefix: String` /
      `config.css_paths: Vec<String>` with
      `config.resolver: Option<ResourceResolverContext>`.
- [x] Iterate artifacts, call `resolver.html_url_for(artifact.scope,
      path)` for each. No `css:default` skip — the only theme
      CSS keys are `css:theme:*` and they flow through the same
      resolver path.
- [x] Sorted-key iteration so `<link>` / `<script>` order is
      deterministic across runs.

### Writer refactor (`render_to_file.rs`)
- [x] Dropped the special-case write of `css:default` (subsumed
      into the general artifact loop via `write_artifacts`).
- [x] Per-doc render writes only Page-scoped artifacts via the
      resolver.
- [x] Project-scoped artifacts are drained out of the per-doc
      store and either: (a) merged into the orchestrator's
      accumulator (real multi-doc projects), or (b) flushed
      in-place via the resolver (default project / standalone
      call). Branch chosen by `project_type.lib_dir().is_empty()`.

### Orchestrator plumbing (`project/orchestrator.rs`)
- [x] After each per-doc Pass-2 render, the orchestrator's
      accumulator receives the drained store via
      `render_document_to_file`'s `Option<&mut ArtifactStore>`
      parameter (cleaner than tuple-return; same effect).
- [x] Byte-mismatch produces an error naming the conflicting
      key + lengths via `ArtifactMergeConflict`. The error
      message is composed at the orchestrator boundary so the
      user sees `"Project-scoped artifact merge failed for
      <doc>: ..."`.
- [x] Sequential merge confirmed: no `Mutex`, no `Arc`, no
      shared mutable state during Pass-2 — ready for future
      rayon-per-worker (D2 contract holds).

### Website post_render
- [x] `WebsiteProjectType::post_render` walks
      `project_artifacts`, writes each to
      `{output_dir}/{lib_dir}/{artifact.path}` via
      `SystemRuntime::file_write`. Sorted-key iteration so the
      on-disk write order is deterministic.
- [x] `DefaultProjectType::post_render` stays no-op. The
      branching in `render_document_to_file` (driven by
      `lib_dir().is_empty()`) ensures Project-scoped artifacts
      get flushed via the resolver per-doc when no shared lib
      dir exists, so post_render has nothing to do for default
      projects. (Confirmed by `single_doc_render_unchanged...`
      regression test.)

### WASM / hub-client impact check
- [x] Audited `crates/wasm-quarto-hub-client/src/lib.rs` (two
      callsites of `render_qmd_to_html`) and
      `hub-client/src/services/wasmRenderer.ts` (the only
      JavaScript consumer of `/.quarto/project-artifacts/styles.css`).
- [x] Hub-client now constructs `ResourceResolverContext::vfs_root("/.quarto/project-artifacts")`
      and passes it via `HtmlRenderConfig::with_resolver`. The
      browser-side TypeScript continues to read from
      `/.quarto/project-artifacts/styles.css` because the WASM
      writer routes every artifact through the same resolver
      (path-on-disk == URL-in-HTML).
- [x] `cargo xtask verify` full (Rust build + tests + fmt +
      clippy + lint + hub-client build incl. WASM + hub-client
      tests + trace-viewer build/tests) — all 9 steps green.

### Integration tests (`quarto-core/tests/artifact_scoping_pipeline.rs`)
- [x] Tests 17, 18, 19b, 20, 21 written and passing on first
      run. Test 19 (extension-dep dedup) deferred — needs an
      extension fixture that emits `css:libs:*` artifacts;
      shape covered by the producer flip + drain unit tests.
      Tests 22 (byte-mismatch hard error) and 23 (empty
      `{stem}_files/` cleanup) deferred to follow-ups (see
      §"Follow-up beads").

### CLI end-to-end + regression
- [x] Single-doc smoke at `/tmp/q2-phase5-singledoc-test/`
      against the captured baseline:
      * `doc.html` sha256 = `7026c8c5...` ✓ (matches baseline)
      * `doc_files/styles.css` sha256 = `3536a93e...` ✓ (matches
        baseline)
- [x] Website smoke at `/tmp/q2-phase5-website-test/`
      (3-page fixture, root + 1 nested):
      * Single shared `_site/site_libs/quarto/quarto-theme-3536a93eba680c9b.css`
        (no per-page duplicates).
      * `<link>` hrefs:
        - `index.html` → `site_libs/quarto/quarto-theme-….css`
        - `about.html` → `site_libs/quarto/quarto-theme-….css`
        - `docs/api.html` → `../site_libs/quarto/quarto-theme-….css`
        (correct relative depth)
- [x] Regression smokes: Phase 2 (`/tmp/q2-phase2-smoke/`),
      Phase 3 (`/tmp/q2-phase3-smoke/`), Phase 4
      (`/tmp/q2-phase4-smoke/`) — sidebar / navbar / page-nav
      output preserved; only the `<link>` href moved from
      `<page>_files/styles.css` to
      `site_libs/quarto/quarto-theme-….css`, exactly as
      planned.

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — **7827 tests pass** (up
      from 7820 pre-Phase-5; net +7 from Phase-5 work).
- [x] `cargo xtask lint` passes (part of `cargo xtask verify`).
- [x] `cargo xtask verify` (full, incl. WASM) — all 9 steps
      green.
- [x] No snapshot files added or modified.
- [x] **Follow-ups filed** (each `discovered-from:bd-u5pr`):
      * `bd-b9za` — Extension-dep `site_libs/` dedup
        integration test (Phase-5 plan tests 19 / 22 deferred).
      * `bd-78ud` — Empty `{stem}_files/` cleanup for pages
        with no Page-scoped artifacts (Open question 5).
      * `bd-apvo` — `project.lib-dir:` user-config override
        (Decision 4 future-proofing pays off).
      * `bd-vdl8` — Retire `DEFAULT_CSS_ARTIFACT_PATH` once
        hub-client (Phase 9) moves off synthetic VFS paths.
- [x] Updated the epic plan's "Work items" checklist —
      Phase 5 marked done, sub-plan linked, `bd-u5pr`
      referenced; follow-up beads logged in the running
      report section.
- [x] `br close bd-u5pr` with reason citing the commit
      (commit hash to be filled in at commit time).
- [ ] `br sync --flush-only && git add .beads/ && git commit`.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** Single-doc render regresses silently. *Mitigation:*
  Decision 10 — baseline snapshot + byte-diff test (test 17 / 24).
  This is the #1 risk the epic calls out.

- **Risk:** WASM / hub-client uses a `resource_prefix` string
  somewhere we haven't found, and the resolver refactor breaks
  in-browser rendering. *Mitigation:* full `cargo xtask verify`
  gate before declaring done; explicit audit task above. The
  DEFAULT_CSS_ARTIFACT_PATH synthetic URL is a red flag — that
  code path needs specific attention.

- **Risk:** Nested-page relative-path math is wrong, producing
  404s in `<link>`. *Mitigation:* tests 7 / 8 / 20 / snapshot
  pin the math; integration test renders actual nested fixture
  and the test assertion reads the emitted HTML.

- **Risk:** Project artifact drain loses data (e.g. Page-scoped
  artifacts accidentally dropped, or Project-scoped written twice).
  *Mitigation:* tests 11–14 pin the drain semantics; integration
  test 19 verifies dedup end-to-end.

- **Risk:** Byte-mismatch-is-an-error (Decision 3) trips on a
  legitimate case we haven't anticipated. *Mitigation:* the
  diagnostic names both sources so the user can pick a fix;
  we can relax to "first-writer-wins + diagnostic" in a follow-up
  if real content hits this.

- **Risk:** Scope-aware resolver adds latency / complexity to a
  hot path (`ApplyTemplateStage` runs per-doc). *Mitigation:*
  resolver is a tiny struct with two pure methods; no allocation
  beyond the returned `String` (same as today's
  `format!("{prefix}{path}")`).

- **Risk:** Retiring `css:default` (Decision 9) breaks WASM
  consumers reading by the `DEFAULT_CSS_ARTIFACT_PATH` constant
  or by the literal `"css:default"` key. *Mitigation:* explicit
  audit task; the migration is "iterate `css:theme:*` and pick
  the one this doc produced" or keep a hub-client-side alias.

- **Risk:** Theme fingerprint hashes inputs the user expects to
  not affect the output (e.g. ordering of an unordered list),
  causing spurious duplicate `site_libs/quarto/quarto-theme-*.css`
  files. *Mitigation:* canonicalize inputs before hashing
  (sort lists, normalize whitespace where applicable); test
  15d covers this.

- **Risk:** Theme fingerprint omits an input that does affect the
  compiled CSS, causing two different SCSS outputs to share a
  key and fail the byte-equality merge check. *Mitigation:* the
  failing merge is the diagnostic — better than silently
  producing wrong CSS. Phase-5 implementation pins the input
  set against `CompileThemeCssStage`'s actual reads.

## Explicit non-goals for this phase

- No Bootstrap / quarto-html / quarto-nav JS vendoring.
- No changes to the SCSS compile pipeline (beyond the output
  path).
- No changes to the sidebar / navbar / footer / page-nav
  transforms. Their HTML output is downstream of Phase 5's
  reshaping only via the template substitution layer.
- No changes to `ProjectIndex` or `DocumentProfile`.
- No cross-document link rewriting (Phase 6).
- No sitemap / favicon / title-prefix (Phase 7).
- No incremental re-use of `site_libs/` (Phase 8).
- No `project.lib-dir:` config override — `ProjectType::lib_dir()`
  is the sole source.
- No parallelism.

## Follow-up beads (to file at close-out)

- **`project.lib-dir:` override** — expose the Q1 YAML option that
  lets users rename `site_libs/`.
- **`lib-dir` name collision with page stems** — if someone has
  a `site_libs.qmd`, today's discovery doesn't exclude it. File
  once the exclusion contract is designed.
- **Dedup strategy: warn instead of error** — collect real-world
  cases where byte-mismatch is legitimate (likely: extension
  version skew) and consider relaxing Decision 3.
- **Vendor Bootstrap / quarto-nav JS / quarto.min.js** — once
  navigation-feature JS lands (`bd-9m8p`, `bd-49ar`), those
  producers populate `site_libs/bootstrap/`,
  `site_libs/quarto-nav/`, `site_libs/quarto/quarto.min.js`.
- **Empty `{stem}_files/` cleanup** — today we create the dir
  unconditionally; if a page has no Page-scoped artifacts we could
  skip creation. Low-priority.

## Open questions (resolve during implementation)

Most of the original open questions were collapsed into the
revised decisions above. What remains:

1. **Theme fingerprint hash function** — `xxh3` (faster, non-
   cryptographic) vs. `blake3` (slower but already in the
   workspace?). Decide based on what's already a dep.
2. **Theme fingerprint input set** — exactly which fields of the
   merged metadata feed into the fingerprint. Probably:
   `format.html.theme` (string, list, or map), any
   `format.html.theme.brand`, any `theme.scss` user files, plus
   Bootstrap version constant. Pin the list during
   implementation by reading `CompileThemeCssStage`.
3. **Where does the per-doc artifact drain happen** — inside
   `render_document_to_file` (returns drained store as second
   tuple element), or as a step in the orchestrator after the
   call returns? Proposal: inside `render_document_to_file`
   returning the drained store, so the function signature
   remains `Result<(RenderToFileResult, ArtifactStore)>`. Locks
   in the parallelism contract from D2.
4. **WASM `DEFAULT_CSS_ARTIFACT_PATH` migration** — depends on
   what hub-client actually reads. Audit before deciding whether
   the constant survives or gets replaced.
5. **Test 23 scope** — should we also clean up empty
   `{stem}_files/` dirs in Phase 5, or defer?

## Decisions log (to fill in after user confirmation)

1. _TBD_
2. _TBD_
… (mirror the numbered decisions above once confirmed)

## Epic-level impact

Phase 5 completes the **shared-asset substrate** that every later
phase leans on:

- **Phase 6** (cross-document link rewriting) needs to rewrite
  body-level `href`s alongside the `<link>` / `<script>`
  rewrites Phase 5 ships. The resolver built here is the shared
  tool.
- **Phase 7** (`post_render`: sitemap, favicon) plugs into the
  same `WebsiteProjectType::post_render` hook Phase 5 opens.
- **Phase 8** (incremental rebuilds) gets a clean contract: the
  project-level artifact store is the unit of cache-check.
- **Phase 9** (hub-client project rendering) can reuse the
  `ProjectContext.project_artifacts` as the in-memory shared
  asset pool between browser-side page renders.

After Phase 5, the website epic has:

- complete information architecture (Phases 1–4),
- a working shared-assets pipeline (Phase 5),

and the remaining phases (6–9) are about **connecting**
documents to each other and to their runtime environment.
