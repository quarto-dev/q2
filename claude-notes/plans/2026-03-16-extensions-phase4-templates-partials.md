# Extensions Phase 4: Template and Partial Support

**Created**: 2026-03-16
**Status**: Complete
**Parent Plan**: `claude-notes/plans/2026-03-16-extensions-master-plan.md`
**Depends on**: Phase 1 (complete)

## Overview

Extensions can declare custom templates and template-partials in their
`contributes.formats.<format>` section. These override or supplement the
built-in templates used by `ApplyTemplateStage`.

### Goals

1. An extension declaring `template: template.html` in its format config
   causes that template to be used instead of the built-in HTML template
2. An extension declaring `template-partials: [title-block.html]` causes
   those partials to be available when compiling the template (either the
   extension's custom template or the built-in template)
3. Extension partials override same-named built-in partials
4. Document/directory metadata `template` and `template-partials` override
   extension values (higher precedence in the merge order)

### Non-Goals

- PDF/LaTeX templates (q2 only renders HTML natively for now)
- Pandoc template staging (q2 has its own template engine)
- Template validation or schema checking

## How TS Quarto Does It

In TS Quarto, `template` and `template-partials` are standard format metadata
keys. After format resolution (which merges extension config), they appear in
the merged format config. During `runPandoc()`:

1. `readPartials(metadata)` extracts `template-partials` from merged metadata,
   expands globs, resolves paths
2. Each format defines a `templateContext` with default partials
3. User/extension partials are appended (same-name = override)
4. Everything is staged as files for Pandoc's template engine

Key difference: TS Quarto delegates to Pandoc for template rendering, so it
stages files to a temp directory. q2 has its own `quarto-doctemplate` engine,
so we can resolve partials in-memory or from the filesystem directly.

## What q2 Already Has

### Template engine (`quarto-doctemplate`)

- `PartialResolver` trait with `get_partial(name, base_path) -> Option<String>`
- `FileSystemResolver` -- reads partials from disk via `std::fs::read_to_string`
- `MemoryResolver` -- in-memory map of name -> content (for tests/bundled)
- `NullResolver` -- returns nothing (used for built-in templates today)
- `Template::compile_with_resolver(source, path, resolver, depth)` -- compiles
  with partial resolution
- `Template::compile_from_file(path)` -- compiles from disk with FileSystemResolver

**WASM limitation**: Both `FileSystemResolver` and `Template::compile_from_file`
use `std::fs` directly, not `SystemRuntime`. They cannot access the WASM VFS.
Phase 4.0 addresses this.

### Template integration (`quarto-core/src/template.rs`)

- `MINIMAL_HTML_TEMPLATE` and `FULL_HTML_TEMPLATE` -- built-in string constants
- `minimal_html_template()` / `full_html_template()` -- compile via `Template::compile()`
  which uses `NullResolver` (no partials in built-in templates today)
- `render_with_format(body, meta, format, css_paths)` -- selects minimal vs full
  based on `is_minimal_html(meta)`, adds CSS paths, renders
- `render_with_custom_template(template, body, meta)` -- renders with a
  pre-compiled Template object

**Metadata filtering**: `render_with_format()` (line 338) excludes only `css`
from the template context (via `add_metadata_to_context_except` at line 387).
`render_with_custom_template()` (line 267) excludes nothing (uses
`add_metadata_to_context`). Both are in `template.rs`. This means `template`
and `template-partials` will leak into the template context as `$template$` /
`$template-partials$` unless we add them to the exclusion list. Phase 4.4
addresses this.

### Apply template stage (`quarto-core/src/stage/stages/apply_template.rs`)

- `ApplyTemplateConfig` has `template: Option<Template>` and
  `HtmlRenderConfig` has `template: Option<&Template>` -- but both are
  **dead code**: never set to `Some(...)`, never wired through. Introduced
  in `806703bd` as aspirational plumbing. Phase 4.pre removes them.
- The stage receives `RenderedOutput` which includes `metadata: ConfigValue`
  (the fully merged metadata from MetadataMergeStage)
- After Phase 4.pre cleanup, the stage will only use `render_with_format()`
  for built-in selection. Phase 4.3 adds metadata-driven template selection.

### Extension metadata flow (Phase 1, complete)

- `template` and `template-partials` from `_extension.yml` already flow
  through as format metadata keys in `ConfigValue`
- After `MetadataMergeStage`, they appear in `doc.ast.meta` alongside
  `toc`, `theme`, etc.
- Path values in extension YAML are plain strings relative to the extension
  dir. They are NOT yet `ConfigValueKind::Path` -- only filter/shortcode paths
  are resolved to absolute in `read_extension()`.

### The `!path` tag system

q2 has a `!path` YAML tag that marks values as `ConfigValueKind::Path`. These
are automatically adjusted by `adjust_paths_to_document_dir()` during metadata
merge -- relative paths are rebased from the metadata source dir to the document
dir. This is exactly what we need for extension template paths.

However, extension authors won't write `!path` tags in their YAML. Instead,
`read_extension()` should convert known path-valued keys (`template`,
`template-partials`) from `ConfigValueKind::Scalar(String)` to
`ConfigValueKind::Path(String)` during parsing. Then the existing merge
pipeline handles path resolution automatically -- no special-casing needed
in `ApplyTemplateStage`.

## Design Decisions

**Where to resolve extension template paths**: Convert them to
`ConfigValueKind::Path` values in `parse_formats()` (`extension/read.rs`).
The function already accepts `ext_dir: &Path` (currently unused with `_`
prefix). Then add `adjust_paths_to_document_dir()` on the extension layer in
`MetadataMergeStage`, using the extension's directory as the source dir.
This reuses the existing `!path` resolution machinery and means all
path-valued metadata keys in extension format config are handled uniformly.

To make the extension dir available at merge time, `build_extension_metadata_layer`
returns `Option<(ConfigValue, PathBuf)>` instead of `Option<ConfigValue>`.
The `PathBuf` comes from `ext.path.clone()` on the matched `Extension`.

See also: `claude-notes/investigations/2026-03-16-investigate-extension-path-resolution.md` for whether
filters/shortcodes should migrate to this pattern too (conclusion: no).

**Where to compile the template**: In `ApplyTemplateStage::run()`. The stage
already has access to the merged metadata. After merge, `template` is a
path string **relative to the document dir** (rebased by
`adjust_paths_to_document_dir` in Phase 4.1). The stage resolves it to
absolute via `ctx.document.input.parent().join(template_path)`, reads it, compiles
with partials, renders.

**How to read template/partial files**: Always use `ctx.runtime.file_read_string()`
(never `std::fs`). This ensures WASM VFS compatibility. The template content is
read into a String, then compiled with `Template::compile_with_resolver()`.
`Template::compile_from_file()` is NOT used (it calls `std::fs` internally).

**Important**: After `adjust_paths_to_document_dir`, all template/partial
paths are relative to the document dir. They must be joined with
`ctx.document.input.parent()` before passing to `runtime.file_read_string()`.

**How to render with a custom template**: Refactor `render_with_format()` to
extract the shared context-building logic into a new `render_with_template()`:

```rust
pub fn render_with_template(
    template: &Template,
    body: &str,
    meta: &ConfigValue,
    format: &Format,
    css_paths: &[String],
) -> Result<String>
```

This function builds the `TemplateContext` (body, metadata via
`add_metadata_to_context_except`, combined CSS list), then renders with the
given template. **Full-template extras** (`version`, `page-layout`) are
**always injected** — custom templates may reference `$version$` or
`$page-layout$`, and unused variables are harmlessly ignored. This avoids
needing a `is_full` flag and gives custom templates the same rich context as
built-in ones.

`render_with_format()` becomes a thin wrapper: select built-in template, call
`render_with_template()`. `render_with_resources()` also delegates to
`render_with_template()` (passing `default_html_template()`). Custom templates
from metadata call `render_with_template()` directly with their compiled
template. This ensures all rendering paths share the same context-building
logic.

**Missing partial files are errors**: If an extension declares
`template-partials: [foo.html]` and `foo.html` cannot be read, this is a hard
error with a clear message (extension name, partial path, underlying IO error).
TS Quarto treats this as an error too. Silent fallback would hide
misconfigured extensions.

**Resolver strategy**: There are three cases:

1. **Custom template, no explicit `template-partials`**: Read template content
   via `ctx.runtime.file_read_string()`. Compile with a `RuntimeResolver`
   (new resolver backed by `SystemRuntime`) that loads partials from the
   template's directory via the runtime. This replaces the `FileSystemResolver`
   path for WASM compatibility.

2. **Custom template + explicit `template-partials`**: Need a `ChainedResolver`
   that tries explicit partials first, then falls back to `RuntimeResolver`
   for any partials the template references that aren't in the explicit list.
   Build a `MemoryResolver` from the explicit partial files (read content via
   runtime, key by filename **stem**), then chain: explicit -> runtime.

3. **No custom template + explicit `template-partials`**: Compile the built-in
   template (minimal or full) with a `MemoryResolver` containing the explicit
   partials. Since built-in templates currently don't use partials, this is
   a no-op today but the infrastructure should support it for when we add
   partials to the built-in templates.

**Partial name keying**: Partials are keyed by **stem** (e.g.,
`title-block.html` -> key `"title-block"`). The template parser extracts the
stem from `$title-block()$` and passes it to `PartialResolver::get_partial`.
The `MemoryResolver` tests confirm this convention. This matches Pandoc's
behavior. Confirmed via deepwiki and `quarto-doctemplate` test suite.

**Stripping `template` from the template context**: Add `"template"` and
`"template-partials"` to the exclusion list in `add_metadata_to_context_except()`
in `template.rs` (line 387). Currently only `"css"` is excluded. This affects:
- `render_with_format()` (line 338) -- already uses `_except` variant
- `render_with_resources()` (line 294) -- already uses `_except` variant
- `render_with_custom_template()` is removed in Phase 4.pre.5; the new
  custom template rendering path in Phase 4.3 must use
  `add_metadata_to_context_except()` with the same exclusion list

## Work Items

### Phase 4.pre: Remove dead template plumbing

`ApplyTemplateConfig.template` and `HtmlRenderConfig.template` were introduced
in `806703bd` (Jan 6, 2026) as forward-looking infrastructure. They have
**never been used**: no caller sets `template: Some(...)`, the wire-through
from `HtmlRenderConfig` → `ApplyTemplateConfig` was never implemented
(see TODO at `pipeline.rs:375`), and no tests exercise them. Phase 4 replaces
this dead plumbing with metadata-driven template selection.

- [x] **4.pre.1** Remove `template: Option<Template>` from `ApplyTemplateConfig`
  and the `with_template()` builder method in `apply_template.rs`.

- [x] **4.pre.2** Remove the `match &self.config.template` branch in
  `ApplyTemplateStage::run()` (lines 153-176). Replace with direct call to
  `render_with_format()` (the current `None` branch). Phase 4.3 will later
  add metadata-driven template selection here.

- [x] **4.pre.3** Remove `template: Option<&'a Template>` from
  `HtmlRenderConfig` and the `with_template()` builder method in `pipeline.rs`.
  Update the condition at line 373 to only check `!config.css_paths.is_empty()`.
  Remove the TODO comment at line 375.

- [x] **4.pre.4** Update `render_to_file.rs:206` to remove `template: None`
  from the `HtmlRenderConfig` literal.

- [x] **4.pre.5** Remove `render_with_custom_template()` from `template.rs`.
  It was only called from the `ApplyTemplateConfig.template` branch removed
  in 4.pre.2. Phase 4.3 will introduce a new metadata-driven rendering path
  that compiles templates on-the-fly with resolvers, replacing this function.

- [x] **4.pre.6** Verify `cargo build --workspace` passes after removal.
  Also verified: `cargo nextest run --workspace` — 6684 tests pass, 0 failures.

### Phase 4.0: RuntimeResolver -- WASM-compatible partial resolution

This is a prerequisite: the existing `FileSystemResolver` uses `std::fs`
and cannot work in WASM. We need a resolver that uses `SystemRuntime`.

Since `PartialResolver` lives in `quarto-doctemplate` (which has no dependency
on `quarto-system-runtime`), the `RuntimeResolver` must live in `quarto-core`
(which depends on both).

- [x] **4.0.1** Add `RuntimeResolver` in `quarto-core/src/template.rs`
- [x] **4.0.2** Add `ChainedResolver` to `quarto-doctemplate/src/resolver.rs`
- [x] **4.0.3** Tests for `RuntimeResolver`: loads partial via runtime,
  returns None when file missing, resolves extension from base path
- [x] **4.0.4** Tests for `ChainedResolver`: primary wins, fallback used when
  primary returns None, None when both missing

### Phase 4.1: Path resolution for extension template values

- [x] **4.1.1** In `parse_formats()` (`extension/read.rs`), added
  `mark_path_valued_keys()` helper that converts `template` (scalar) and
  `template-partials` (array of scalars) from `ConfigValueKind::Scalar` to
  `ConfigValueKind::Path`. Applied after merge, before inserting into result.

- [x] **4.1.2** Changed `build_extension_metadata_layer()` in `metadata_merge.rs`
  to return `Option<(ConfigValue, PathBuf)>`. The `PathBuf` is
  `ext.path.clone()` from the matched `Extension` struct.

- [x] **4.1.3** In `MetadataMergeStage::run()`, the extension layer call now
  uses `.map()` to destructure the tuple and call
  `adjust_paths_to_document_dir(&mut config, &ext_dir, &document_dir)`.

- [x] **4.1.4** Tests (3 new in read.rs, 1 updated in metadata_merge.rs):
  - `test_template_converted_to_path_kind`: verifies `template` is `Path`
  - `test_template_partials_converted_to_path_kind`: verifies array elements are `Path`
  - `test_non_path_metadata_unaffected_by_path_conversion`: verifies `toc`, `theme` etc unchanged
  - Updated `test_build_extension_metadata_layer_basic` to destructure tuple and verify ext_path

### Phase 4.2: Extract template config from merged metadata

- [x] **4.2.1** In `ApplyTemplateStage::run()`, extract `template` and
  `template-partials` from `rendered.metadata`. Implemented inline in the
  `run()` method alongside the Phase 4.3 template selection logic.

- [x] **4.2.2** Tests: covered by Phase 4.5 integration tests (extraction is
  tested implicitly through the full rendering pipeline).

### Phase 4.3: Compile and apply extension templates

- [x] **4.3.0** Refactored into `render_with_compiled_template()` in
  `template.rs`. Both `render_with_format()` and `render_with_resources()`
  now delegate to it. Full-template extras (version, page-layout) always
  injected. All 66 template tests pass unchanged.

- [x] **4.3.1** Custom template, no explicit partials: implemented in
  `ApplyTemplateStage::run()` using `RuntimeResolver`.

- [x] **4.3.2** Custom template + explicit partials: implemented with
  `ChainedResolver` (MemoryResolver → RuntimeResolver). `build_partial_resolver()`
  helper reads partial content via runtime, keys by file stem.

- [x] **4.3.3** No custom template + explicit partials: implemented via
  `compile_builtin_template_with_partials()` in `template.rs`.

- [x] **4.3.4** When neither is set: falls through to `render_with_format()`
  — existing behavior unchanged.

- [x] **4.3.5** Error handling: all three cases produce hard errors with
  descriptive messages (path + underlying error).

### Phase 4.4: Strip template keys from template context

- [x] **4.4.1** Updated `render_with_compiled_template()` to exclude
  `["css", "template", "template-partials"]` from the template context.
  All rendering paths go through this single function.

- [x] **4.4.3** Test: `test_template_key_not_in_output` verifies `$template$`
  resolves to empty, not the template path

### Phase 4.5: Integration tests

- [x] **4.5.1** `test_custom_template_from_metadata`: custom template -> output
  uses that template's structure
- [x] **4.5.2** `test_custom_template_with_partials`: custom template + explicit
  partials -> partial content appears in output
- [x] **4.5.3** (Deferred — built-in templates don't use partials yet, so the
  "only partials, no custom template" path is a no-op today. Infrastructure
  is in place for when built-in templates add partials.)
- [x] **4.5.4** `test_document_template_overrides_extension`: document metadata
  `template` value wins after merge
- [x] **4.5.5** `test_no_template_no_partials_existing_behavior`: existing
  built-in template behavior unchanged
- [x] **4.5.6** `test_template_key_not_in_output`: verifies `$template$` is
  stripped from context (Phase 4.4.3)
- [x] **4.5.7** `test_missing_template_file_errors`: hard error when template
  file doesn't exist

### Phase 4.6: Smoke Tests

- [x] **4.6.1** Created `extensions/custom-template/` smoke test with
  `custom-tmpl` extension providing `template.html`. Verifies
  `CUSTOM-TEMPLATE-ACTIVE` marker and `div.custom-template-marker` element.

- [x] **4.6.2** Created `extensions/template-partials/` smoke test with
  `partial-ext` extension providing template + `header.html` partial.
  Verifies `PARTIAL-HEADER-CONTENT` and `header.ext-header` element.

### Phase 4.7: Workspace Verification

- [x] **4.7.1** `cargo build --workspace` — clean build
- [x] **4.7.2** `cargo nextest run --workspace` — 6693 tests pass, 0 failures

### Phase 4.8: Update master plan

- [x] **4.8.1** Updated `claude-notes/plans/2026-03-16-extensions-master-plan.md`
  to mark Phase 4 complete with summary of changes.

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Modify | (4.pre) Remove dead `ApplyTemplateConfig.template`; (4.2-4.3) extract template from metadata, compile with resolver via runtime |
| `crates/quarto-core/src/pipeline.rs` | Modify | (4.pre) Remove dead `HtmlRenderConfig.template`, clean up TODO |
| `crates/quarto-core/src/render_to_file.rs` | Modify | (4.pre) Remove `template: None` from config literal |
| `crates/quarto-core/src/template.rs` | Modify | (4.pre) Remove `render_with_custom_template()`; (4.0) Add `RuntimeResolver`; (4.3) Refactor: extract `render_with_template()` from `render_with_format()` and `render_with_resources()`, add `compile_builtin_template_with_partials()`; (4.4) Update exclusion list in `render_with_template()` |
| `crates/quarto-doctemplate/src/resolver.rs` | Modify | (4.0) Add `ChainedResolver` |
| `crates/quarto-core/src/extension/read.rs` | Modify | (4.1) Convert `template`/`template-partials` to `ConfigValueKind::Path` in `parse_formats()` |
| `crates/quarto-core/src/stage/stages/metadata_merge.rs` | Modify | (4.1) Return `(ConfigValue, PathBuf)` from `build_extension_metadata_layer`, call `adjust_paths_to_document_dir` on extension layer |
| `crates/quarto/tests/smoke-all/extensions/custom-template/` | Create | (4.6) Smoke test |
| `crates/quarto/tests/smoke-all/extensions/template-partials/` | Create | (4.6) Smoke test |

## Key APIs

**Resolving metadata paths to absolute** (required before any file read):
```rust
let document_dir = ctx.document.input.parent()
    .unwrap_or_else(|| Path::new("."));
let abs_template_path = document_dir.join(template_path);
```

All template/partial paths from metadata are relative to the document dir
after `adjust_paths_to_document_dir`. They must be joined with
the document dir before passing to `runtime.file_read_string()`.
Use the same fallback as `MetadataMergeStage` (line 190-194).

**Reading partial files into MemoryResolver** (via runtime, errors on missing):
```rust
fn build_partial_resolver(
    partial_paths: &[String],
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<MemoryResolver> {
    let mut resolver = MemoryResolver::new();
    for path_str in partial_paths {
        let path = Path::new(path_str);
        let abs_path = document_dir.join(path);
        let content = runtime.file_read_string(&abs_path)
            .map_err(|e| /* error with partial path and IO details */)?;
        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path_str);
        resolver.add(name, content);
    }
    Ok(resolver)
}
```

**Compiling template from runtime-read content**:
```rust
let document_dir = ctx.document.input.parent().unwrap_or_else(|| Path::new("."));
let abs_path = document_dir.join(template_path);
let template_content = ctx.runtime.file_read_string(&abs_path)?;
let runtime_resolver = RuntimeResolver::new(ctx.runtime.as_ref());
let template = Template::compile_with_resolver(
    &template_content,
    &abs_path,
    &runtime_resolver,
    0,
)?;
```

**Rendering** (both custom and built-in go through the same function):
```rust
// Custom template from metadata:
let html = template::render_with_template(
    &compiled_template, &rendered.content, &metadata, &rendered.format, &css_paths,
)?;

// Built-in template (render_with_format now delegates to render_with_template):
let html = template::render_with_format(
    &rendered.content, &metadata, &rendered.format, &css_paths,
)?;
```

## Risks and Open Questions

1. **SystemRuntime in ApplyTemplateStage**: The stage has access to `ctx.runtime`
   for reading extension template/partial files. No new plumbing needed.

2. **WASM compatibility**: Fully addressed by `RuntimeResolver` (Phase 4.0).
   All file reads go through `ctx.runtime.file_read_string()`, which handles
   the VFS in WASM context. `FileSystemResolver` and `compile_from_file` are
   not used for extension templates.

3. **Partial name resolution**: Partials are keyed by **stem** (e.g.,
   `title-block.html` -> `"title-block"`). The template parser passes the stem
   to `get_partial`. `MemoryResolver` looks up by exact name match. This
   matches Pandoc/TS Quarto conventions. Confirmed via deepwiki and
   `quarto-doctemplate` test suite.

4. **What about `template` in document metadata?** If a user writes
   `template: !path my-template.html` in their frontmatter, the `!path` tag
   makes it a `ConfigValueKind::Path` which `adjust_paths_to_document_dir()`
   resolves automatically. If they write `template: my-template.html`
   (no tag), it's a plain string and won't be adjusted -- it would need to
   be resolved relative to the document dir in `ApplyTemplateStage`. For
   Phase 4, we only need to handle the extension case (where we add the
   `!path` conversion in `read_extension`). Document-level template support
   can be added later.

5. **Which keys need `!path` conversion?** In TS Quarto, `template` and
   `template-partials` are the path-valued keys in format config. Other
   format-resources paths (`format-resources`, `css`) may also need this
   treatment in later phases, but Phase 4 only covers template/partials.

6. **Should filters/shortcodes also use `!path` instead of absolute resolution?**
   See separate investigation:
   `claude-notes/investigations/2026-03-16-investigate-extension-path-resolution.md`
   Conclusion: No. Absolute is correct for execution paths, `!path` for metadata.

---

## Codebase Reference

This section provides the concrete details an implementer needs without
requiring independent codebase exploration.

### Key structs and their fields

**`StageContext`** (`quarto-core/src/stage/context.rs:46`):
```rust
pub struct StageContext {
    pub runtime: Arc<dyn SystemRuntime>,  // filesystem, env, subprocesses
    pub format: Format,                    // output format (e.g., html)
    pub project: ProjectContext,           // project root, config, files
    pub document: DocumentInfo,            // input/output paths
    pub extensions: Vec<Extension>,        // discovered extensions
    pub artifacts: ArtifactStore,          // stored artifacts (CSS, etc.)
    pub diagnostics: Vec<DiagnosticMessage>,
    // ...
}
```

**`DocumentInfo`** (`quarto-core/src/project.rs:305`):
```rust
pub struct DocumentInfo {
    pub input: PathBuf,           // absolute input file path
    pub output: Option<PathBuf>,  // absolute output path
    pub title: Option<String>,
    pub id: Option<String>,
}
```
**No `dir()` method.** Get document dir via `ctx.document.input.parent()`.
This matches how `MetadataMergeStage` computes it at line 190:
```rust
let document_dir = doc.path.parent()
    .map(|p| p.to_path_buf())
    .unwrap_or_else(|| ctx.project.dir.clone());
```

**`Extension`** (`quarto-core/src/extension/types.rs:55`):
```rust
pub struct Extension {
    pub id: ExtensionId,          // name + optional organization
    pub title: String,
    pub author: String,
    pub version: Option<String>,
    pub quarto_required: Option<String>,
    pub path: PathBuf,            // ABSOLUTE path to extension directory
    pub contributes: Contributes,
}
```

**`Contributes`** (`quarto-core/src/extension/types.rs:71`):
```rust
pub struct Contributes {
    pub formats: HashMap<String, ConfigValue>,  // format metadata
    pub filters: Vec<ExtensionFilter>,          // absolute paths
    pub shortcodes: Vec<PathBuf>,               // absolute paths
}
```

**`RenderedOutput`** (`quarto-core/src/stage/data.rs:327`):
```rust
pub struct RenderedOutput {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub format: Format,
    pub content: String,          // HTML body content
    pub is_intermediate: bool,
    pub supporting_files: Vec<PathBuf>,
    pub metadata: ConfigValue,    // fully merged metadata from MetadataMergeStage
}
```

### ConfigValue API (`quarto-pandoc-types/src/config_value.rs`)

```rust
// Key methods used in this plan:
impl ConfigValue {
    pub fn get(&self, key: &str) -> Option<&ConfigValue>    // map lookup
    pub fn as_str(&self) -> Option<&str>                     // Scalar(String), Path, Glob, Expr
    pub fn as_array(&self) -> Option<&[ConfigValue]>         // Array variant
    pub fn as_bool(&self) -> Option<bool>                    // Scalar(Boolean)
}

// ConfigValueKind variants relevant to this plan:
enum ConfigValueKind {
    Scalar(Yaml),          // plain YAML values
    Path(String),          // !path tag — adjusted by adjust_paths_to_document_dir
    Array(Vec<ConfigValue>),
    Map(Vec<ConfigMapEntry>),
    // ... others
}
```

**Important**: `as_str()` returns `Some` for both `Scalar(String)` AND `Path`
variants. So after converting to `ConfigValueKind::Path` in Phase 4.1, the
extraction in Phase 4.2 (`metadata.get("template").and_then(|v| v.as_str())`)
works without changes.

### Template engine API (`quarto-doctemplate`)

```rust
// Compile a template with partial resolution:
Template::compile_with_resolver(
    source: &str,           // template content as string
    template_path: &Path,   // path (for partial resolution base dir)
    resolver: &impl PartialResolver,
    depth: usize,           // 0 for top-level
) -> TemplateResult<Template>

// Resolver types:
MemoryResolver::new() -> MemoryResolver
MemoryResolver::add(&mut self, name: impl Into<String>, content: impl Into<String>)
ChainedResolver::new(primary: A, fallback: B) -> ChainedResolver<A, B>
RuntimeResolver::new(runtime: &dyn SystemRuntime) -> RuntimeResolver  // in quarto-core

// Resolve partial path from name + base template path:
resolve_partial_path(name: &str, base_path: &Path) -> PathBuf
// e.g., ("header", "/ext/template.html") → "/ext/header.html"
```

### Current `render_with_format()` logic (`template.rs:338-383`)

This is one of two functions that Phase 4.3.0 refactors (the other is
`render_with_resources()` at line 294, which has similar logic but uses
`default_html_template()` and omits step 6). Current steps:
1. `is_minimal_html(meta)` → select minimal or full built-in template
2. Create `TemplateContext`, insert `"body"`
3. `add_metadata_to_context_except(meta, &mut ctx, &["css"])` — all metadata
   except `css`
4. Build combined CSS: `css_paths` param + `extract_css_from_meta(meta)`
5. Insert `"css"` as list
6. If full template: insert `"version"` (from `CARGO_PKG_VERSION`), default
   `"page-layout"` to `"article"` if not already set
7. `template.render(&ctx)`

After refactoring, `render_with_template()` performs steps 2-7 (always
including step 6 — unused variables are harmlessly ignored). Both
`render_with_format()` and `render_with_resources()` become thin wrappers
that select the template (step 1) and delegate.

### Current `ApplyTemplateStage::run()` logic (`apply_template.rs:119-189`)

1. Extract `RenderedOutput` from input
2. Store CSS artifact if not already set by `CompileThemeCssStage`
3. Clone metadata from rendered output
4. Branch on `self.config.template` (dead code — always `None`):
   - `Some(template)`: call `render_with_custom_template` **(removed in 4.pre)**
   - `None`: compute CSS paths (default or from config), call `render_with_format`
5. Replace `rendered.content` with full HTML

CSS path computation in the `None` branch (lines 161-165):
```rust
let css_paths: Vec<String> = if self.config.css_paths.is_empty() {
    vec![DEFAULT_CSS_ARTIFACT_PATH.to_string()]
} else {
    self.config.css_paths.clone()
};
```

### Current `parse_formats()` (`extension/read.rs:179-211`)

```rust
fn parse_formats(
    formats_cv: &ConfigValue,
    _ext_dir: &Path,           // ← unused, needs underscore removed in 4.1.1
) -> Result<HashMap<String, ConfigValue>>
```
Iterates format entries, merges "common" key into each format. Returns
format name → ConfigValue map. Phase 4.1.1 adds a post-processing step
to walk each format's ConfigValue and convert `template` / `template-partials`
from `Scalar` to `Path`.

### Current `build_extension_metadata_layer()` (`metadata_merge.rs:86-112`)

```rust
fn build_extension_metadata_layer(
    extensions: &[Extension],
    target_format: &str,
) -> Option<ConfigValue>
```
Parses format descriptor, finds matching extension, looks up format metadata
by base format and exact match, merges if both exist. Phase 4.1.2 changes
return to `Option<(ConfigValue, PathBuf)>` where `PathBuf` is `ext.path.clone()`.

### Current `MetadataMergeStage::run()` extension layer usage (line 202)

```rust
let extension_layer = build_extension_metadata_layer(&ctx.extensions, target_format);
// ...
if let Some(ref ext) = extension_layer {
    layers.push(ext);
}
```
Phase 4.1.3 destructures the tuple and adds `adjust_paths_to_document_dir`:
```rust
let (extension_layer, extension_dir) = match build_extension_metadata_layer(...) {
    Some((mut config, dir)) => {
        adjust_paths_to_document_dir(&mut config, &dir, &document_dir);
        (Some(config), Some(dir))
    }
    None => (None, None),
};
```

### `adjust_paths_to_document_dir()` (`project.rs:180-186`)

```rust
pub(crate) fn adjust_paths_to_document_dir(
    metadata: &mut ConfigValue,
    metadata_dir: &Path,    // where the paths are relative to (e.g., ext dir)
    document_dir: &Path,    // where to rebase to (document's parent dir)
)
```
Recursively walks ConfigValue. For `ConfigValueKind::Path` values that are
relative: joins with `metadata_dir` to get absolute, then `pathdiff::diff_paths`
against `document_dir` to get relative-to-document.

Example: extension at `/project/_extensions/acm/`, document at `/project/posts/`:
- Input: `ConfigValueKind::Path("template.html")`
- `metadata_dir.join("template.html")` → `/project/_extensions/acm/template.html`
- `diff_paths(abs, document_dir)` → `../_extensions/acm/template.html`
- Result: `ConfigValueKind::Path("../_extensions/acm/template.html")`

Then in `ApplyTemplateStage`, resolve back to absolute:
`document_dir.join("../_extensions/acm/template.html")` →
`/project/_extensions/acm/template.html`

### Smoke test pattern (`crates/quarto/tests/smoke-all/`)

Each `.qmd` file embeds assertions in `_quarto.tests` frontmatter. The
`smoke_all` test runner (`crates/quarto/tests/smoke_all.rs`) discovers all
`.qmd` files via `walkdir` and runs each through `quarto_test::run_test_file`.

Example fixture:
```yaml
---
title: Basic Render Test
format: html
_quarto:
  tests:
    html:
      noErrors: true
      ensureFileRegexMatches:
        - ["<!DOCTYPE html>", "<title>Basic Render Test</title>"]
        - ["ERROR"]
---
Content here.
```

For extension smoke tests, the fixture directory needs:
- `_quarto.yml` with project config
- `_extensions/<name>/_extension.yml` with extension config
- `_extensions/<name>/template.html` (or partials)
- A `.qmd` file with assertions

Run with: `cargo nextest run -p quarto --test smoke_all`

### Testing guidelines

- Use `cargo nextest run` (never `cargo test`)
- Do NOT pipe nextest through `tail` — it hangs
- Write tests BEFORE implementation (TDD)
- After changes, run full workspace: `cargo nextest run --workspace`
- For quarto-core changes, also run `cargo xtask verify`
- See `claude-notes/instructions/testing.md` for detailed conventions
