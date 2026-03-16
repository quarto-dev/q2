# Extensions Phase 1: _extension.yml Parsing and Metadata Contributions

**Created**: 2026-03-16
**Status**: Complete (1.1-1.5, 1.4b, 1.7-1.8 done; 1.6 deferred)
**Parent Plan**: `claude-notes/plans/2026-03-16-extensions-master-plan.md`

## Codebase Context for New Agents

**READ FIRST**: The master plan (`claude-notes/plans/2026-03-16-extensions-master-plan.md`)
has a "Codebase Context" section with the full crate map, type reference, and
testing patterns. Read that before starting this work.

### What has already been built (committed to this branch)

All code is in `crates/quarto-core/src/extension/`:

- **`types.rs`** — Data model: `ExtensionId`, `Extension`, `Contributes`, `ExtensionFilter`.
  An `Extension` represents a parsed `_extension.yml` with all paths resolved to absolute.
- **`read.rs`** — `read_extension(path, runtime)` parses an `_extension.yml` file.
  Uses `quarto_yaml::parse_file()` → `yaml_to_config_value()` → extract fields.
  Merges the "common" format key into all sibling format keys using `MergedConfig`.
- **`discover.rs`** — `discover_extensions(input, project_dir, runtime)` walks
  `_extensions/` directories from input up to project root. Also contains
  `find_extension(name, extensions)` and `parse_format_descriptor(format_str)`.
- **`mod.rs`** — Re-exports the public API.

Integration points already wired:

- **`stage/context.rs`** — `StageContext` has `pub extensions: Vec<Extension>`,
  populated in `::new()` via `discover_extensions()`.
- **`stage/stages/metadata_merge.rs`** — `build_extension_metadata_layer()` looks
  up extension format metadata by parsing the target format string. Inserts as a
  layer between Project and Directory in the merge order.

### What is currently broken

The `Format` struct (`format.rs`) only has `identifier: FormatIdentifier` (an enum
like `Html`, `Pdf`, etc.) and no field for the original format string. When a
document uses `format: acm-html`:

1. `format_from_name("acm-html")` in `render_to_file.rs` falls back to `Format::html()`
2. The pipeline sees `ctx.format.identifier.as_str()` → `"html"`
3. `build_extension_metadata_layer(extensions, "html")` gets no extension match
4. Extension metadata is never applied

**Phase 1.4b fixes this** by adding `target_format`, `extension_name`, and
`display_name` fields to `Format`.

### Quick Reference — Files You'll Touch

| File | What's in it |
|------|-------------|
| `crates/quarto-core/src/lib.rs` | Module registration — add `pub mod extension;` here |
| `crates/quarto-core/src/format.rs` | `FormatIdentifier` enum, `Format` struct — add `parse_format_descriptor()` here |
| `crates/quarto-core/src/stage/context.rs` | `StageContext` struct + `::new()` — add `extensions` field here |
| `crates/quarto-core/src/stage/stages/metadata_merge.rs` | `MetadataMergeStage` — insert extension layer here |
| `crates/quarto-core/src/project.rs` | `ProjectContext`, `DocumentInfo`, `directory_metadata_for_document()` |

### Key APIs You'll Use

**Parsing YAML to ConfigValue** (the pattern used everywhere):
```rust
use quarto_yaml;
use pampa::pandoc::yaml_to_config_value;
use quarto_config::InterpretationContext;
use pampa::utils::diagnostic_collector::DiagnosticCollector;

// 1. Parse YAML string → YamlWithSourceInfo (preserves source locations)
let yaml = quarto_yaml::parse_file(&content, &filename)?;

// 2. Convert to ConfigValue (the universal metadata type)
let mut diagnostics = DiagnosticCollector::new();
let config_value = yaml_to_config_value(
    yaml,
    InterpretationContext::ProjectConfig,
    &mut diagnostics,
);
```

**Building ConfigValue manually** (for tests):
```rust
use quarto_pandoc_types::{ConfigValue, ConfigMapEntry, ConfigValueKind, MergeOp};
use quarto_source_map::SourceInfo;

fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let map_entries: Vec<ConfigMapEntry> = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: SourceInfo::default(),
            value: v,
        })
        .collect();
    ConfigValue::new_map(map_entries, SourceInfo::default())
}
fn config_str(s: &str) -> ConfigValue {
    ConfigValue::new_string(s, SourceInfo::default())
}
fn config_bool(b: bool) -> ConfigValue {
    ConfigValue::new_bool(b, SourceInfo::default())
}
```

**Merging ConfigValue layers**:
```rust
use quarto_config::MergedConfig;

let layers: Vec<&ConfigValue> = vec![&project_layer, &extension_layer, &doc_layer];
let merged = MergedConfig::new(layers);  // lowest priority first
let result: ConfigValue = merged.materialize()?;
```

**Format flattening** (extracts `format.html.*` and merges on top):
```rust
use quarto_config::resolve_format_config;
let flattened = resolve_format_config(&some_config_value, "html");
// Now flattened has no "format" key; format.html.* values override top-level
```

**Reading filesystem via SystemRuntime** (for WASM compatibility):
```rust
// In quarto-core, filesystem access goes through SystemRuntime trait
let content = runtime.file_read_string(path)?;
let exists = runtime.path_exists(path, Some(PathKind::File))?;
let entries = runtime.dir_list(dir)?;
```

### StageContext Fields (current, lines 45-78 of context.rs)

```rust
pub struct StageContext {
    pub runtime: Arc<dyn SystemRuntime>,
    pub format: Format,
    pub project: ProjectContext,
    pub document: DocumentInfo,
    pub temp_dir: PathBuf,
    pub artifacts: ArtifactStore,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub observer: Arc<dyn PipelineObserver>,
    pub cancellation: Cancellation,
}
```

You'll add `pub extensions: Vec<Extension>` to this struct.

### MockRuntime for Tests

`metadata_merge.rs` (lines 234-369) has a complete `MockRuntime` implementing
all `SystemRuntime` methods. Copy and extend it for extension discovery tests.
For discovery tests, you'll need `dir_list()` and `path_exists()` to return
meaningful values based on a test directory structure. Consider using real temp
dirs (`std::env::temp_dir()` + `std::fs`) for discovery tests instead of mocking,
since discovery involves complex path walking.

### ConfigValue Navigation

```rust
// Get a nested value
let toc = config.get("toc");                    // Option<&ConfigValue>
let toc_bool = config.get("toc").and_then(|v| v.as_bool()); // Option<bool>

// Iterate map entries
if let ConfigValueKind::Map(entries) = &config.value {
    for entry in entries {
        println!("{}: {:?}", entry.key, entry.value);
    }
}

// Check array
if let ConfigValueKind::Array(items) = &config.value {
    for item in items { /* ... */ }
}
```

## Overview

Parse `_extension.yml` files, discover extensions in the project hierarchy, and
merge extension format metadata into the rendering pipeline. This phase covers
the data model, YAML parsing, directory discovery, and metadata merge integration.

No filter/shortcode/plugin *execution* — just loading extensions and contributing
their metadata to the merge pipeline.

### Goals

1. A `_extension.yml` with `contributes.formats.html.toc: true` causes `toc: true`
   to appear in the merged metadata when rendering to HTML
2. The `common` key in `contributes.formats` merges into all sibling format keys
3. Extension metadata sits between Project and Directory layers in the merge order
4. Document and directory metadata can override extension metadata
5. format-resources from extensions are copied to the output directory

### Non-Goals (deferred to later phases)

- Filter name resolution against extensions (Phase 2)
- Shortcode resolution (Phase 3)
- Template/partial resolution (Phase 4)
- Custom writers (Phase 5)
- RevealJS plugins (Phase 6)
- Embedded extensions (Phase 9)
- Extension installation CLI (Phase 10)
- Built-in extensions (Phase 11)

### Design Decisions

**Format name mapping**: When a user writes `format: acm-html`, the format name
is parsed as `{extension}-{base-format}`. The extension name is `acm`, the base
format is `html`. This is how TS Quarto works (see `formatDescriptor()` in
`render-contexts.ts`). For Phase 1, we need a basic version of this to look up
the right extension and base format. We'll implement `parse_format_descriptor()`
that splits on the last hyphen matching a known base format.

**Extension layer precedence**: Extension metadata is inserted between Project and
Directory layers:

```
Project → Extension → Directory → Document → Runtime
  (1)       (2)         (3)         (4)        (5)
```

This matches TS Quarto: extension defaults are overridable by `_metadata.yml` and
document frontmatter, but override project defaults.

**Path resolution**: All paths in a loaded `Extension` are resolved to absolute
paths during `read_extension()`. This simplifies all downstream code.

**Module location**: New module at `crates/quarto-core/src/extension/` with
submodules for types, reading, and discovery.

---

## Work Items

### Phase 1.1: Extension Data Model

- [x] **1.1.1** Create `crates/quarto-core/src/extension/mod.rs` with public API:
  ```rust
  pub mod types;
  pub mod read;
  pub mod discover;
  ```

- [x] **1.1.2** Create `crates/quarto-core/src/extension/types.rs` with structs:
  ```rust
  /// Identifies an extension by name and optional organization.
  ///
  /// Examples:
  /// - `ExtensionId { name: "lightbox", organization: None }`
  /// - `ExtensionId { name: "acm", organization: Some("quarto-journals") }`
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub struct ExtensionId {
      pub name: String,
      pub organization: Option<String>,
  }

  /// A parsed and resolved Quarto extension.
  #[derive(Debug, Clone)]
  pub struct Extension {
      pub id: ExtensionId,
      pub title: String,
      pub author: String,
      pub version: Option<String>,
      pub quarto_required: Option<String>,
      pub path: PathBuf,                   // absolute path to extension dir
      pub contributes: Contributes,
  }

  /// What an extension contributes.
  ///
  /// All path fields contain absolute paths (resolved during read_extension).
  /// Format metadata is stored as ConfigValue for direct use in the merge pipeline.
  #[derive(Debug, Clone, Default)]
  pub struct Contributes {
      /// Format-specific metadata, keyed by format name (e.g., "html", "pdf").
      /// The "common" key has already been merged into siblings and removed.
      pub formats: HashMap<String, ConfigValue>,

      /// Top-level filter contributions (absolute paths).
      pub filters: Vec<ExtensionFilter>,

      /// Top-level shortcode contributions (absolute paths).
      pub shortcodes: Vec<PathBuf>,

      /// Raw metadata contribution (merged into project config).
      pub metadata: Option<ConfigValue>,

      /// Raw project contribution.
      pub project: Option<ConfigValue>,

      /// RevealJS plugin specs (stored as raw ConfigValue for later phases).
      pub revealjs_plugins: Vec<ConfigValue>,

      /// Engine specs (stored as raw ConfigValue for later phases).
      pub engines: Vec<ConfigValue>,
  }

  /// A filter contributed by an extension.
  #[derive(Debug, Clone)]
  pub struct ExtensionFilter {
      pub path: PathBuf,
      pub at: Option<String>,  // entry point
  }
  ```

- [x] **1.1.3** Register `extension` module in `crates/quarto-core/src/lib.rs`

- [x] **1.1.4** Write basic tests for `ExtensionId` (Display, equality, etc.)

### Phase 1.2: _extension.yml Parser

- [x] **1.2.1** Create `crates/quarto-core/src/extension/read.rs` with:
  ```rust
  /// Read and parse an _extension.yml file.
  ///
  /// All relative paths are resolved to absolute paths relative to the
  /// extension directory (parent of the _extension.yml file).
  pub fn read_extension(extension_file: &Path) -> Result<Extension>
  ```

  Implementation steps:
  1. Read YAML with `quarto_yaml::parse_file()` (preserves source locations)
  2. Extract top-level fields: `title` (required), `author` (required),
     `version` (optional), `quarto-required` (optional)
  3. Extract `contributes` object (required, must have at least one sub-field)
  4. Process `contributes.formats`:
     - Extract `common` key if present
     - For each non-common format key, deep-merge `common` into it
       (format-specific values override common values)
     - Delete `common` from the result
     - Store each format's metadata as a `ConfigValue`
  5. Process `contributes.filters`: resolve paths to absolute
  6. Process `contributes.shortcodes`: resolve paths to absolute
  7. Store `contributes.metadata`, `contributes.project`,
     `contributes.revealjs-plugins`, `contributes.engines` as raw ConfigValue
     (path resolution for these deferred to later phases)

- [x] **1.2.2** Implement `common` key merging:
  The `common` key's values serve as defaults for all other format keys.
  Use `quarto_config::MergedConfig` to merge `[common, format_specific]`,
  then materialize. This gives us proper merge semantics including `!prefer`
  and `!concat` tag support.

- [x] **1.2.3** Write tests for `read_extension()`:
  - Minimal extension: just title + author + one shortcode → parses OK
  - Format extension with `common` key:
    ```yaml
    contributes:
      formats:
        common:
          toc: true
          number-sections: true
        html:
          theme: cosmo
        pdf:
          documentclass: article
    ```
    → html has toc + number-sections + theme; pdf has toc + number-sections +
    documentclass; common key is gone
  - Format-specific overrides common: `common.toc: true`, `html.toc: false`
    → html has `toc: false`
  - Extension with filters:
    ```yaml
    contributes:
      filters:
        - filter.lua
        - path: other.lua
          at: post-render
    ```
    → paths resolved to absolute; entry points preserved
  - Missing title → error with source location
  - Missing contributes → error
  - Empty contributes (no sub-fields) → error
  - Path resolution: relative paths joined with extension dir

### Phase 1.3: Extension Discovery

- [x] **1.3.1** Create `crates/quarto-core/src/extension/discover.rs` with:
  ```rust
  /// Discover all extensions available for a document.
  ///
  /// Searches _extensions/ directories in the project hierarchy,
  /// walking from the input file's directory up to the project root.
  pub fn discover_extensions(
      input: &Path,
      project_dir: Option<&Path>,
      runtime: &dyn SystemRuntime,
  ) -> Result<Vec<Extension>>
  ```

  Search algorithm:
  1. If `project_dir` is provided, walk from `input.parent()` up to
     `project_dir`, collecting `_extensions/` directories at each level
  2. If no `project_dir`, check only `input.parent()/_extensions/`
  3. For each `_extensions/` dir found:
     - List subdirectories (each is potentially an extension or organization)
     - For each subdir, check for `_extension.yml` (unorganized extension)
     - If no `_extension.yml`, check subdirs of subdir (organized: `org/name/`)
  4. Call `read_extension()` for each found `_extension.yml`
  5. Return all successfully parsed extensions

- [x] **1.3.2** Implement `find_extension()`:
  ```rust
  /// Find a specific extension by name among discovered extensions.
  pub fn find_extension<'a>(
      name: &str,
      extensions: &'a [Extension],
  ) -> Option<&'a Extension>
  ```
  Match logic:
  - If `name` contains `/`, split into `org/name` and match both
  - If no `/`, match by name only (any organization)

- [x] **1.3.3** Write tests for discovery:
  - Create temp dir structure with `_extensions/test-ext/_extension.yml`
    → discovers the extension
  - Organized layout: `_extensions/org/ext/_extension.yml` → discovers with
    organization = "org"
  - Multiple levels: extensions at both project root and subdirectory
    → both discovered
  - Empty `_extensions/` dir → returns empty vec
  - No `_extensions/` dir → returns empty vec
  - Invalid `_extension.yml` → skipped with warning (doesn't fail the whole
    discovery)

### Phase 1.4: Format Name Parsing

- [x] **1.4.1** Implement format descriptor parsing:
  ```rust
  /// Parse a format string into extension name and base format.
  ///
  /// Examples:
  /// - "html" → FormatDescriptor { extension: None, base: "html" }
  /// - "acm-html" → FormatDescriptor { extension: Some("acm"), base: "html" }
  /// - "acm-pdf" → FormatDescriptor { extension: Some("acm"), base: "pdf" }
  /// - "my-journal-pdf" → FormatDescriptor { extension: Some("my-journal"), base: "pdf" }
  pub struct FormatDescriptor {
      pub extension_name: Option<String>,
      pub base_format: String,
  }

  pub fn parse_format_descriptor(format: &str) -> FormatDescriptor
  ```

  Algorithm: Split on the last `-` where the suffix is a known base format
  (html, pdf, docx, epub, typst, revealjs, gfm, commonmark). If no match,
  the entire string is the base format (no extension).

- [x] **1.4.2** Write tests:
  - "html" → no extension, base = "html"
  - "acm-pdf" → extension = "acm", base = "pdf"
  - "my-cool-journal-html" → extension = "my-cool-journal", base = "html"
  - "unknown" → no extension, base = "unknown"
  - "foo-bar" (bar not a known format) → no extension, base = "foo-bar"

### Phase 1.4b: Format String Preservation (BLOCKER)

The extension metadata merge layer (Phase 1.5) is currently dead code because the
original format string (e.g., `"acm-html"`) is lost when `Format` is constructed.
The pipeline only sees the base format `"html"`, so `build_extension_metadata_layer`
never finds a matching extension.

**TS Quarto reference** (confirmed via DeepWiki):
- TS Quarto uses `parseFormatString()` in `pandoc-formats.ts` → `FormatDescriptor`
- The `Format` object carries a `FormatIdentifier` with fields:
  - `base-format` ("pdf") — the Pandoc output format
  - `target-format` ("acm-pdf") — the full format string from YAML
  - `extension-name` ("acm") — just the extension part
  - `display-name` — human-readable name
- `readExtensionFormat()` uses the descriptor to look up extension metadata

**Our approach**: Mirror TS Quarto's `FormatIdentifier` fields on our `Format` struct.

- [x] **1.4b.1** Add fields to `Format` struct (`format.rs`):
  ```rust
  pub struct Format {
      pub identifier: FormatIdentifier,   // existing: the base format enum
      pub target_format: String,          // NEW: full format string, e.g. "acm-pdf"
      pub extension_name: Option<String>, // NEW: extension part, e.g. Some("acm")
      pub display_name: String,           // NEW: human-readable, e.g. "ACM PDF"
      pub output_extension: String,       // existing
      pub native_pipeline: bool,          // existing
  }
  ```

- [x] **1.4b.2** Update `Format` constructors (`Format::html()`, `Format::pdf()`, etc.)
  to populate the new fields with sensible defaults:
  ```rust
  Format::html() → target_format: "html", extension_name: None, display_name: "HTML"
  Format::pdf()  → target_format: "pdf",  extension_name: None, display_name: "PDF"
  ```

- [x] **1.4b.3** Add `Format::from_format_string()` constructor:
  ```rust
  /// Create a Format from a format string like "acm-html" or "html".
  /// Uses parse_format_descriptor() to split extension from base format.
  pub fn from_format_string(format_str: &str) -> Self
  ```
  This replaces the private `format_from_name()` in `render_to_file.rs`.

- [x] **1.4b.4** Update `format_from_name()` in `render_to_file.rs` (lines 310-317)
  to call `Format::from_format_string()`.

- [x] **1.4b.5** Update `MetadataMergeStage::run()` to use `ctx.format.target_format`
  instead of `ctx.format.identifier.as_str()` when calling
  `build_extension_metadata_layer()`. This is the line that currently reads:
  ```rust
  let target_format = ctx.format.identifier.as_str();
  ```

- [x] **1.4b.6** Update smoke test harness (`quarto-test/src/runner.rs`) — no changes needed, it passes format string through to render_to_file which now calls Format::from_format_string
  constructs `Format` directly — it should use the new constructor.

- [x] **1.4b.7** Write tests:
  - `Format::from_format_string("html")` → identifier=Html, target_format="html",
    extension_name=None, display_name="HTML"
  - `Format::from_format_string("acm-pdf")` → identifier=Pdf, target_format="acm-pdf",
    extension_name=Some("acm"), display_name="acm-pdf"
  - `Format::from_format_string("my-journal-html")` → identifier=Html,
    target_format="my-journal-html", extension_name=Some("my-journal")
  - Existing `Format::html()` etc. still work unchanged
  - Integration: MetadataMergeStage with extension + format "acm-html" now applies
    extension metadata (currently fails because format string is lost)

### Phase 1.5: Extension Metadata Merge into Pipeline

This is the key integration point. Extension metadata must be inserted as a new
layer in `MetadataMergeStage`.

- [x] **1.5.1** Add extension discovery to the pipeline setup:

  Option A (lazy, in MetadataMergeStage): The stage itself discovers extensions
  and extracts the relevant format metadata.

  Option B (eager, in StageContext): Extensions are discovered when `StageContext`
  is created and stored as a field.

  **Decision**: Option B — discover once, store in `StageContext`. This avoids
  re-discovering on every stage and makes extensions available to other stages
  later (filter resolution, shortcode resolution, etc.).

  Add to `StageContext`:
  ```rust
  /// Extensions discovered for this document
  pub extensions: Vec<Extension>,
  ```

- [x] **1.5.2** Update `StageContext::new()` to discover extensions:
  ```rust
  let extensions = discover_extensions(
      &document.input,
      if project.is_single_file { None } else { Some(&project.dir) },
      runtime.as_ref(),
  )?;
  ```

- [x] **1.5.3** Update `MetadataMergeStage::run()` to insert extension layer:

  After project layer (Layer 1) and before directory layers (Layer 2):

  ```rust
  // Layer 1.5: Extension metadata (flattened for format)
  // Find extensions that contribute to the target format and merge their
  // format-specific metadata.
  let extension_layer = build_extension_metadata_layer(
      &ctx.extensions,
      target_format,
  );
  ```

  Implementation of `build_extension_metadata_layer()`:
  1. Parse `target_format` with `parse_format_descriptor()` to get extension
     name and base format
  2. If there's an extension name, find it in `ctx.extensions`
  3. Look up `extension.contributes.formats[base_format]`
  4. Also look up `extension.contributes.formats[target_format]` (exact match)
  5. If both exist, merge them (exact match overrides base)
  6. Flatten for format (call `resolve_format_config()`)
  7. Return as a `ConfigValue` layer

  Update merge layer construction:
  ```rust
  let mut layers: Vec<&ConfigValue> = Vec::new();
  if let Some(ref proj) = project_layer {
      layers.push(proj);
  }
  if let Some(ref ext) = extension_layer {
      layers.push(ext);
  }
  for dir_meta in &dir_layers {
      layers.push(dir_meta);
  }
  layers.push(&doc_layer);
  if let Some(ref rt) = runtime_layer {
      layers.push(rt);
  }
  ```

- [x] **1.5.4** Write integration tests for extension metadata merge:
  - Extension contributes `formats.html.toc: true`, document has no toc setting
    → merged metadata has `toc: true`
  - Extension contributes `formats.html.toc: true`, document has `toc: false`
    → merged metadata has `toc: false` (document wins)
  - Extension contributes `formats.html.toc: true`, project has `toc: false`
    → merged metadata has `toc: true` (extension wins over project)
  - Extension contributes `formats.html.theme: cosmo`, document renders PDF
    → extension metadata not applied (wrong format)
  - Extension with `common` + format-specific → common merged before layer merge
  - No extensions found → existing behavior unchanged (regression test)
  - Multiple extensions contributing to same format → merge in discovery order

### Phase 1.6: format-resources Support (DEFERRED)

**Deferred to a later PR.** format-resources requires glob resolution, pipeline
resource copying, and possibly a new crate dependency. It's only needed for
extensions that bundle CSS/CLS/other files, which is not required for the core
extension metadata flow to work end-to-end.

Tracked items (for future work):
- Resolve `format-resources` glob patterns during `read_extension()`
- Add resource copying step in the pipeline (design TBD)
- Tests for file copying, glob patterns, missing files

### Phase 1.7: Smoke Tests

**Depends on**: Phase 1.4b (format string preservation)

Smoke tests auto-discover `.qmd` files under `crates/quarto/tests/smoke-all/`.
The format key under `_quarto.tests` is passed to `render_to_file()`. Each test
directory needs a `_quarto.yml` for project context (required for extension
discovery to walk the directory tree).

- [x] **1.7.1** Create smoke test directory structure:
  ```
  crates/quarto/tests/smoke-all/extensions/
  ├── _quarto.yml                          # needed for project context
  ├── simple-metadata/
  │   ├── _extensions/
  │   │   └── test-meta/
  │   │       └── _extension.yml
  │   └── test.qmd
  └── common-key/
      ├── _extensions/
      │   └── test-common/
      │       └── _extension.yml
      └── test.qmd
  ```

- [x] **1.7.2** `simple-metadata` test:
  - `_extension.yml`:
    ```yaml
    title: Test Meta
    author: Test
    contributes:
      formats:
        html:
          toc: true
          number-sections: true
    ```
  - `test.qmd` — note: the `_quarto.tests` format key must be the extension
    format name `test-meta-html`, which the test harness passes to
    `render_to_file()`:
    ```yaml
    ---
    title: Test
    _quarto:
      tests:
        test-meta-html:
          ensureHtmlElements:
            - ["nav#TOC"]
    ---
    ## Section 1
    Content here.
    ## Section 2
    More content.
    ```

- [x] **1.7.3** `common-key` test:
  - Extension with `common` key contributing to both html and pdf
  - Verify html output includes common + html-specific settings

### Phase 1.8: Workspace Verification

- [x] **1.8.1** `cargo build --workspace`
- [x] **1.8.2** `cargo nextest run --workspace`
- [x] **1.8.3** `cargo xtask verify` (extension metadata changes affect quarto-core
  which is used by WASM)

---

## Implementation Notes

### TS Quarto Vocabulary (confirmed via DeepWiki)

Aligning our naming with TS Quarto for consistency:

| Concept | TS Quarto name | Our Rust name |
|---------|---------------|---------------|
| Full format string | "format string" | `target_format: String` |
| Parsed format parts | `FormatDescriptor` | `FormatDescriptor` (in `discover.rs`) |
| Extension part of format | `extension` | `extension_name: Option<String>` |
| Base Pandoc format | `baseFormat` | `FormatIdentifier` enum / `base_format` |
| Identity fields on Format | `FormatIdentifier` (TS interface) | Fields on `Format` struct |
| Human label | `display-name` | `display_name: String` |

TS Quarto's `FormatIdentifier` interface (in `config/types.ts`):
```typescript
interface FormatIdentifier {
  "base-format"?: string;     // "pdf"
  "target-format"?: string;   // "acm-pdf"
  "display-name"?: string;    // "ACM PDF"
  "extension-name"?: string;  // "acm"
}
```

TS Quarto's `FormatDescriptor` (in `pandoc-formats.ts`):
```typescript
interface FormatDescriptor {
  baseFormat: string;          // "pdf"
  extension?: string;          // "acm"
  variants: string[];          // Pandoc +/- variants
  modifiers: string[];         // Additional modifiers
  formatWithVariants: string;  // baseFormat + variants
}
```

Key TS functions:
- `parseFormatString()` — splits "acm-pdf" into FormatDescriptor
- `readExtensionFormat()` — finds extension, reads contributes.formats metadata
- `resolveFormats()` — merges extension metadata into format config

### ConfigValue as extension metadata

Extension format metadata is stored as `ConfigValue` directly. This means:
- It has source location tracking (errors point to `_extension.yml`)
- It participates in `MergedConfig` merge with `!prefer`/`!concat` support
- No conversion needed — it's the same type used by project/directory/document layers

### How read_extension() should parse YAML

Follow the pattern in `project.rs` (`parse_project_config`, line ~490):

```rust
pub fn read_extension(extension_file: &Path, runtime: &dyn SystemRuntime) -> Result<Extension> {
    let content = runtime.file_read_string(extension_file)?;
    let filename = extension_file.display().to_string();
    let yaml = quarto_yaml::parse_file(&content, &filename)?;

    // yaml is a YamlWithSourceInfo. Convert to ConfigValue:
    let mut diagnostics = DiagnosticCollector::new();
    let config = yaml_to_config_value(
        yaml,
        InterpretationContext::ProjectConfig,
        &mut diagnostics,
    );

    // Now config is a ConfigValue (a Map). Extract fields:
    let title = config.get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| /* error with source location */)?
        .to_string();

    // ... extract other fields ...

    // For contributes.formats, each value is already a ConfigValue,
    // so just clone it into the HashMap<String, ConfigValue>.
}
```

**Important**: `read_extension` needs `&dyn SystemRuntime` to read the file,
keeping it WASM-compatible. Pass it through from the discovery function.

### Discovery uses SystemRuntime

Extension discovery needs to read the filesystem. It uses `SystemRuntime` for
WASM compatibility (where the filesystem is a VFS). This means discovery works
in both CLI and WASM contexts.

Key `SystemRuntime` methods for discovery:
- `runtime.path_exists(path, Some(PathKind::Dir))` — check if `_extensions/` exists
- `runtime.dir_list(path)` — list subdirectories
- `runtime.file_read_string(path)` — read `_extension.yml` content

### The "common" key merge — step by step

Given this `_extension.yml`:
```yaml
contributes:
  formats:
    common:
      toc: true
      number-sections: true
    html:
      theme: cosmo
    pdf:
      documentclass: article
```

After parsing, `contributes.formats` is a ConfigValue map with keys
`common`, `html`, `pdf`. The merge process:

1. Extract the `common` ConfigValue
2. For `html`: `MergedConfig::new(vec![&common, &html]).materialize()`
   → `{toc: true, number-sections: true, theme: cosmo}`
3. For `pdf`: `MergedConfig::new(vec![&common, &pdf]).materialize()`
   → `{toc: true, number-sections: true, documentclass: article}`
4. Delete `common` from the result HashMap

This uses the same `MergedConfig` that powers the metadata merge pipeline,
so `!prefer` and `!concat` tags work correctly.

### Error handling strategy

- `read_extension()` returns `Result<Extension>` — a malformed `_extension.yml`
  is an error
- `discover_extensions()` collects all valid extensions and logs warnings for
  malformed ones (doesn't fail the whole render)
- Missing `_extensions/` directory is not an error (most projects don't have one)

### Test strategy for discovery

For discovery tests, **use real temp directories** rather than mocking the
filesystem. The `SystemRuntime` mocking is complex (need `dir_list`, `path_exists`,
`file_read` to return consistent results). Instead:

```rust
use std::fs;
use tempfile::TempDir;  // or std::env::temp_dir()

let tmp = TempDir::new()?;
let ext_dir = tmp.path().join("_extensions/my-ext");
fs::create_dir_all(&ext_dir)?;
fs::write(ext_dir.join("_extension.yml"), r#"
title: My Extension
author: Test
contributes:
  formats:
    html:
      toc: true
"#)?;

// Use StandaloneRuntime (the real filesystem runtime) for tests
use quarto_system_runtime::StandaloneRuntime;
let runtime = StandaloneRuntime::new();
let extensions = discover_extensions(
    &tmp.path().join("test.qmd"),
    None,
    &runtime,
)?;
assert_eq!(extensions.len(), 1);
```

Check if `StandaloneRuntime` or similar exists in `quarto-system-runtime`. If not,
use the `NativeRuntime` or similar concrete implementation — grep for
`impl SystemRuntime for` to find all implementations.

### What we DON'T need yet

- Semver validation of `version` and `quarto-required` (store as strings)
- Extension caching (discovery is cheap enough per-render for Phase 1)
- Embedded extension loading (Phase 9)
- Trust verification (Phase 10)

---

## Files to Create/Modify

| File | Action | Status | Description |
|------|--------|--------|-------------|
| `crates/quarto-core/src/extension/mod.rs` | Create | Done | Module root, public API |
| `crates/quarto-core/src/extension/types.rs` | Create | Done | Extension, ExtensionId, Contributes structs |
| `crates/quarto-core/src/extension/read.rs` | Create | Done | `_extension.yml` parser |
| `crates/quarto-core/src/extension/discover.rs` | Create | Done | Extension directory discovery + FormatDescriptor |
| `crates/quarto-core/src/lib.rs` | Modify | Done | Register `extension` module |
| `crates/quarto-core/src/stage/context.rs` | Modify | Done | Add `extensions` field to `StageContext` |
| `crates/quarto-core/src/stage/stages/metadata_merge.rs` | Modify | Done | Insert extension layer + `build_extension_metadata_layer()` |
| `crates/quarto-core/src/format.rs` | Modify | TODO | Add `target_format`, `extension_name`, `display_name` to `Format`; add `from_format_string()` |
| `crates/quarto-core/src/render_to_file.rs` | Modify | TODO | Update `format_from_name()` to use `Format::from_format_string()` |
| `crates/quarto-test/src/runner.rs` | Modify | TODO | Update format construction if needed |
| `crates/quarto/tests/smoke-all/extensions/` | Create | TODO | Smoke test fixtures |

## References

- Master plan: `claude-notes/plans/2026-03-16-extensions-master-plan.md`
- TS Quarto format descriptor: `src/core/pandoc/pandoc-formats.ts` (`parseFormatString()`)
- TS Quarto format identifier: `src/config/types.ts` (`FormatIdentifier` interface)
- TS Quarto extension reading: `src/extension/extension.ts`
- TS Quarto format resolution: `src/command/render/render-contexts.ts` (`readExtensionFormat()`)
- Current metadata merge: `crates/quarto-core/src/stage/stages/metadata_merge.rs`
- Current format types: `crates/quarto-core/src/format.rs`
- Current stage context: `crates/quarto-core/src/stage/context.rs`
- Config merging design: `claude-notes/plans/2025-12-07-config-merging-design.md`
