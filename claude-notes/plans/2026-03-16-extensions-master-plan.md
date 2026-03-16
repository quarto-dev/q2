# Quarto Extensions Master Plan

**Created**: 2026-03-16
**Status**: In Progress (Phase 1 partially complete)
**Worktree**: `worktree-extensions-phase1` branch, at `.claude/worktrees/extensions-phase1`
**Sub-plans**: Phase 1 detail at `claude-notes/plans/2026-03-16-extensions-phase1-yml-and-metadata.md`

## Codebase Context for New Agents

**READ THIS FIRST if you have zero knowledge of this codebase.**

This plan lives in the **q2** repository (`quarto-dev/q2`), a Rust monorepo that
is a ground-up rewrite of Quarto (the original is TypeScript, at `quarto-dev/quarto-cli`).
The TS version is referred to as "TS Quarto" or "Quarto 1". When we say "Quarto"
without qualification, we mean this Rust rewrite.

### Orientation

- **What is Quarto?** A scientific/technical publishing system. Users write `.qmd`
  (Quarto Markdown) files with YAML frontmatter, code cells, and markdown. Quarto
  renders them to HTML, PDF, DOCX, etc.
- **What are extensions?** Packages that customize rendering. They live in
  `_extensions/` directories and are defined by `_extension.yml` files. They can
  contribute custom format configs, Lua filters, shortcodes, etc.
- **What is this plan?** A multi-phase roadmap to implement the full extension system
  in Rust Quarto. Phase 1 (metadata contributions) is in progress.

### Key Crate Map

| Crate | Role |
|-------|------|
| `quarto-core` | Rendering pipeline orchestration, transforms, metadata merge |
| `pampa` | QMD parser, Pandoc AST, Lua/JSON filter engine, HTML writer |
| `quarto-pandoc-types` | Pandoc AST types + `ConfigValue` (the universal metadata type) |
| `quarto-yaml` | YAML parser with fine-grained source location tracking |
| `quarto-config` | `MergedConfig` (lazy multi-layer config merge), `resolve_format_config()` |
| `quarto-system-runtime` | `SystemRuntime` trait — filesystem/env abstraction for WASM compat |
| `quarto` | CLI binary, smoke tests in `tests/smoke-all/` |

### Rendering Pipeline (current)

```
ParseDocument → EngineExecution → MetadataMerge → CompileThemeCss →
  [UserPreFilters] → AstTransforms → [UserPostFilters] →
  RenderHtmlBody → ApplyTemplate
```

Stages implement the `PipelineStage` trait (`crates/quarto-core/src/stage/traits.rs`).
Per-render state lives in `StageContext` (`crates/quarto-core/src/stage/context.rs`).

### Critical Types

- **`ConfigValue`** (`quarto-pandoc-types/src/config_value.rs`): The universal
  metadata value type. Has `ConfigValueKind` (Scalar/Map/Array/Path), `SourceInfo`,
  and `MergeOp`. Constructors: `new_string()`, `new_bool()`, `new_map()`.
  Access: `.get("key")`, `.as_str()`, `.as_bool()`.
- **`ConfigMapEntry`**: `{ key: String, key_source: SourceInfo, value: ConfigValue }`.
- **`MergedConfig`** (`quarto-config/src/merged.rs`): Takes `Vec<&ConfigValue>` layers
  (lowest-priority first), lazily merges. Call `.materialize()` to get a single
  `ConfigValue`. Supports `!prefer` and `!concat` YAML tags.
- **`resolve_format_config()`** (`quarto-config/src/format.rs`): Takes a `ConfigValue`
  map and a target format string. Removes the `format` key, extracts
  `format.{target}.*` and merges on top of remaining top-level keys. Returns a
  flattened `ConfigValue`.
- **`YamlWithSourceInfo`** (`quarto-yaml/src/`): Parsed YAML with source locations.
  Convert to `ConfigValue` via `pampa::pandoc::yaml_to_config_value()`.

### How YAML Becomes ConfigValue

```rust
use quarto_yaml;
use pampa::pandoc::yaml_to_config_value;
use quarto_config::InterpretationContext;
use pampa::utils::diagnostic_collector::DiagnosticCollector;

let yaml = quarto_yaml::parse_file(content, filename)?;
let mut diagnostics = DiagnosticCollector::new();
let config_value = yaml_to_config_value(
    yaml,
    InterpretationContext::ProjectConfig, // or ::DocumentConfig
    &mut diagnostics,
);
```

This pattern is used in `project.rs` for `_quarto.yml` and `_metadata.yml`.
Extension YAML should use the same pattern.

### Metadata Merge Layers (current)

```
Project (_quarto.yml) → Directory (_metadata.yml chain) → Document (frontmatter) → Runtime
```

Each layer is format-flattened via `resolve_format_config()` before merging.
Implementation: `crates/quarto-core/src/stage/stages/metadata_merge.rs`.

### Testing Patterns

- Use `cargo nextest run` (never `cargo test`)
- Smoke tests: `crates/quarto/tests/smoke-all/` — QMD files with `_quarto.tests`
  metadata for assertions (`ensureHtmlElements`, `ensureFileRegexMatches`, etc.)
- Unit tests: inline `#[cfg(test)] mod tests` in each module
- Test helpers for `ConfigValue`: see `metadata_merge.rs` tests for `config_map()`,
  `config_str()`, `config_bool()` helpers
- `MockRuntime` pattern: implement `SystemRuntime` trait with stubs for testing
  (see `metadata_merge.rs` for a complete example)

### Build & Verify

```bash
cargo build --workspace          # Build everything
cargo nextest run --workspace    # Run all tests
cargo xtask verify               # Full verify (Rust + WASM + hub-client)
```

## Overview

Implement the Quarto extension system in q2, enabling users to install and use
extensions that contribute formats, filters, shortcodes, metadata, RevealJS plugins,
project types, and custom engines.

This plan is detailed for Phase 1 (YAML parsing + metadata contributions) and
outlines the remaining phases with open questions.

## Extension Taxonomy (from TS Quarto)

Extensions are defined by `_extension.yml` files and contribute one or more of
seven contribution types. A single extension can contribute multiple types.

### Contribution Types

| Type | What it provides | Can appear inside format? |
|------|-----------------|--------------------------|
| **formats** | Custom output format configs (metadata, templates, partials, writers) | N/A (IS the format) |
| **filters** | Lua/JSON AST filters | Yes (`formats.{name}.filters`) |
| **shortcodes** | Lua shortcode handlers | Yes (`formats.{name}.shortcodes`) |
| **revealjs-plugins** | RevealJS presentation plugins | Yes (`formats.{name}.revealjs-plugins`) |
| **project** | Project-level config (type, detect, render globs, preview) | No |
| **metadata** | Arbitrary metadata merged into project/document config | No |
| **engines** | Custom execution engines (beyond Jupyter/Knitr) | No |

### How Types Compose

A **format extension** is the most complex — it can contain all of the following
within each format definition:

```yaml
contributes:
  formats:
    common:           # merged into ALL other format keys, then deleted
      filters: [shared.lua]
      shortcodes: [shared-sc.lua]
    html:
      template: template.html
      template-partials: [partials/title.html]
      format-resources: ["*.css", "images/**"]
      filters: [html-specific.lua]
      shortcodes: [html-sc.lua]
      revealjs-plugins: [my-plugin]
      # plus any standard Quarto format options (toc, number-sections, etc.)
    pdf:
      documentclass: myclass
      template-partials: [partials/title.tex]
      format-resources: [myclass.cls]
```

Format extensions can also contain **embedded extensions** in an `_extensions/`
subdirectory within the extension itself. Filters/shortcodes/plugins referenced
by name are resolved against embedded extensions first.

## `_extension.yml` Complete Field Reference

### Top-Level Fields

```yaml
title: "My Extension"              # string, required
author: "Author Name"              # string, required
version: "1.0.0"                   # semver string, optional
quarto-required: ">=1.4.0"         # semver range, optional
contributes:                       # object, required (at least one sub-field)
  shortcodes: [...]
  filters: [...]
  formats: {...}
  revealjs-plugins: [...]
  project: {...}
  metadata: {...}
  engines: [...]
```

### `contributes.shortcodes`

```yaml
shortcodes:
  - shortcode-handler.lua          # path relative to extension dir
```

### `contributes.filters`

```yaml
filters:
  - filter.lua                     # string path
  - path: filter2.lua              # object with path
    at: pre-quarto                 # optional entry point
  - embedded-ext-name              # resolved against embedded extensions
```

Entry points: `pre-ast`, `post-ast`, `pre-quarto`, `post-quarto`, `pre-render`,
`post-render`, `pre-finalize`, `post-finalize`.

### `contributes.formats`

```yaml
formats:
  common:                          # special: merged into all other formats
    key: value
  html:                            # format-specific config
    template: template.html
    template-partials: [partial.html]
    format-resources: ["*.css"]    # glob patterns, resolved relative to ext dir
    filters: [filter.lua]          # per-format filters
    shortcodes: [sc.lua]           # per-format shortcodes
    revealjs-plugins: [plugin]     # per-format plugins
    writer: custom-writer.lua      # custom Lua writer
    # ... any standard Quarto format metadata (toc, number-sections, etc.)
```

When a format key ends with `.lua`, it's treated as a custom writer format.

### `contributes.revealjs-plugins`

```yaml
revealjs-plugins:
  - plugin-name                    # string path
  - plugin: path/to/plugin         # bundle object
    config:
      key: value
  - name: inline-plugin            # inline definition
    register: true
    script: plugin.js
    stylesheet: plugin.css
    config: { key: value }
```

### `contributes.project`

```yaml
project:
  project:
    type: website                  # project type
    detect:                        # auto-detection rules
      - ["docusaurus.config.js", "package.json"]
    render:
      - "**/*.qmd"
    output-dir: _site
  preview:
    serve:
      cmd: "npm start -- --port {port}"
      ready: "compiled successfully"
  format: html                     # default format for project
  pre-render: [script.sh]         # resolved to absolute paths
  post-render: [script.sh]
  brand: brand.yml                 # resolved to absolute path
```

### `contributes.metadata`

```yaml
metadata:
  pre-render: [script.sh]
  post-render: [script.sh]
  brand: brand.yml
  # arbitrary metadata keys merged into project/document config
```

### `contributes.engines`

```yaml
engines:
  - my-engine                      # string name
  - path: ./engine-binary          # object with path (resolved to absolute)
```

## Extension Discovery (TS Quarto Algorithm)

Search order for `_extensions/` directories:

1. **Built-in extensions**: `resourcePath("extensions")` (org = `quarto`)
2. **Built-in subtree extensions**: `resourcePath("extension-subtrees")`
3. **Project hierarchy**: Walk from input file's directory up to project root,
   checking each level's `_extensions/` directory
4. **Input directory**: `_extensions/` in the input file's directory (if no project)

Within each `_extensions/` directory:
- **Organized**: `_extensions/{org}/{name}/_extension.yml`
- **Unorganized**: `_extensions/{name}/_extension.yml`

Glob matching: If no org specified, tries both `{name}/_extension.yml` and
`*/{name}/_extension.yml`.

## Metadata Merge Order

Extension metadata is merged between defaults and user config:

```
Default Writer Format  →  Extension Format Metadata  →  User Format Metadata
    (lowest)                    (middle)                    (highest)
```

For project-level metadata from extensions: extensions contributing
`metadata.project` are parsed and merged into `context.config.project` early,
before file discovery.

---

## Implementation Phases

### Phase 1: _extension.yml Parsing and Metadata Contributions

**Split out to**: `claude-notes/plans/2026-03-16-extensions-phase1-yml-and-metadata.md`

**Goal**: Parse `_extension.yml`, discover extensions, and merge extension metadata
into the rendering pipeline. No filter/shortcode execution yet — just metadata.

**Current status** (as of 2026-03-16):

| Sub-phase | Status | Summary |
|-----------|--------|---------|
| 1.1 Data model | Done | `ExtensionId`, `Extension`, `Contributes`, `ExtensionFilter` in `extension/types.rs` |
| 1.2 YAML parser | Done | `read_extension()` in `extension/read.rs` with "common" key merging |
| 1.3 Discovery | Done | `discover_extensions()` in `extension/discover.rs`, walks `_extensions/` dirs |
| 1.4 Format parsing | Done | `parse_format_descriptor()` splits "acm-html" → extension "acm" + base "html" |
| 1.4b Format string preservation | **TODO** | `Format` struct needs `target_format`, `extension_name`, `display_name` fields |
| 1.5 Metadata merge | Done | `build_extension_metadata_layer()` in metadata_merge.rs, but **blocked by 1.4b** |
| 1.6 format-resources | Deferred | Will be a separate PR |
| 1.7 Smoke tests | TODO | Depends on 1.4b |
| 1.8 Workspace verify | Partial | Build + tests pass; WASM verify still needed |

**Key blocker**: Phase 1.4b. The `Format` struct currently has no field to preserve
the original format string (e.g., "acm-html"). `format_from_name()` in `render_to_file.rs`
silently falls back to `Format::html()`, losing the extension name. The metadata
merge layer receives "html" instead of "acm-html" and never finds an extension match.

See the Phase 1 sub-plan for full details on 1.4b implementation.

---

### Phase 2: Extension Filter Resolution

**Goal**: Resolve filter names that reference extensions (e.g., `filters: [lightbox]`
where `lightbox` is an extension contributing filters).

- [ ] Update `filter_resolve.rs` to accept extension context
- [ ] When a filter name doesn't resolve to a file path, look it up in extensions
- [ ] If found, substitute the extension's contributed filter paths
- [ ] Handle per-format filters from format extensions
- [ ] Tests: extension filter resolution, missing extension → error

**Depends on**: Phase 1 (extension discovery) + current user-filters work

### Phase 3: Extension Shortcode Resolution

**Goal**: Resolve shortcode references from extensions and wire them into the
shortcode processing pipeline.

- [ ] Discover shortcodes contributed by active extensions
- [ ] Add extension shortcode paths to shortcode resolution
- [ ] Handle per-format shortcodes from format extensions
- [ ] Tests

**Open questions**:
- How do shortcodes currently work in q2? (Need to check `ShortcodeResolveTransform`)
- Are shortcodes already resolved from file paths, or is there a registry?

### Phase 4: Template and Partial Support

**Goal**: Format extensions can provide custom templates and template partials.

- [ ] Read `template` and `template-partials` from extension format metadata
- [ ] Wire into `ApplyTemplateStage` (uses `quarto-doctemplate`)
- [ ] Template search order: extension template → default template
- [ ] Partial search order: extension partials → default partials
- [ ] Tests

**Open questions**:
- How does `quarto-doctemplate` currently resolve templates?
- Does it support a search path or just a single template file?
- How do partials compose with the base template?

### Phase 5: Custom Writers

**Goal**: Format extensions can provide custom Lua writers.

- [ ] Detect format keys ending in `.lua` → custom writer format
- [ ] Resolve writer path relative to extension directory
- [ ] Wire into rendering pipeline (likely replaces `RenderHtmlBodyStage`)
- [ ] Tests

**Open questions**:
- Does pampa support custom Lua writers currently?
- How would this interact with the WASM pipeline?

### Phase 6: RevealJS Plugin Support

**Goal**: Extensions can contribute RevealJS plugins.

- [ ] Parse `revealjs-plugins` from extensions
- [ ] Handle three forms: string path, bundle object, inline definition
- [ ] Wire plugin scripts/stylesheets into RevealJS output
- [ ] Handle per-format RevealJS plugins
- [ ] Tests

**Open questions**:
- Does q2 have RevealJS output support yet?
- What's the plan for RevealJS format?

### Phase 7: Project Extensions

**Goal**: Extensions can contribute project-level configuration.

- [ ] Parse `contributes.project` from extensions
- [ ] Merge into project config during project context creation
- [ ] Support `project.type`, `detect`, `render`, `preview` fields
- [ ] Support `pre-render` and `post-render` scripts
- [ ] Tests

**Open questions**:
- Does q2 support project types beyond the default?
- How do pre/post-render scripts execute?
- How does project type detection work?

### Phase 8: Engine Extensions

**Goal**: Extensions can provide custom execution engines.

- [ ] Parse `contributes.engines` from extensions
- [ ] Register engines in the engine execution stage
- [ ] Support external engine paths
- [ ] Tests

**Open questions**:
- What engines does q2 currently support?
- How is the engine selection mechanism implemented?

### Phase 9: Embedded Extensions

**Goal**: Extensions can contain other extensions in `_extensions/` subdirectories.

- [ ] During `read_extension()`, recursively read `_extensions/` within extension dir
- [ ] When resolving filter/shortcode/plugin names in format configs, check embedded
  extensions first
- [ ] If name matches an embedded extension, substitute its contributions
- [ ] Tests: extension with embedded extension, name resolution priority

### Phase 10: Extension Installation

**Goal**: `quarto install extension` command support.

- [ ] GitHub source detection (org/repo[@version][/subdir])
- [ ] Archive URL support
- [ ] Local path support
- [ ] Trust verification prompt
- [ ] Staging, validation, and installation to `_extensions/`
- [ ] `--embedded` flag for installing into another extension
- [ ] Tests

**Open questions**:
- Is this a q2 CLI feature or out of scope for now?
- What trust model do we want?

### Phase 11: Built-in Extensions

**Goal**: Ship built-in extensions with q2.

- [ ] Create `resources/extensions/` directory structure
- [ ] Port essential built-in extensions from TS Quarto (video, kbd, etc.)
- [ ] Built-in extension discovery (searched first, before project extensions)
- [ ] Tests

**Open questions**:
- Which built-in extensions are essential for initial release?
- Can we bundle TS Quarto's built-in extensions directly, or do they need porting?

---

## Architecture Decisions

### Where extensions plug into the pipeline

```
ParseDocument → EngineExecution → MetadataMerge(+extensions) → CompileThemeCss →
  [UserPreFilters(+ext filters)] → AstTransforms(+ext shortcodes) →
  [UserPostFilters(+ext filters)] → RenderHtmlBody(+ext writer?) →
  ApplyTemplate(+ext template/partials)
```

### Extension metadata merge layer

Extension metadata sits between Project and Directory in the merge order:

```
Project → Extension → Directory → Document → Runtime
```

This matches TS Quarto behavior: extension defaults are overridable by
directory `_metadata.yml` and document frontmatter.

### Module organization

```
crates/quarto-core/src/extension/
├── mod.rs              # public API
├── types.rs            # Extension, ExtensionId, Contributes structs
├── read.rs             # _extension.yml parsing
├── discover.rs         # extension directory discovery
└── resolve.rs          # name resolution (filters, shortcodes, etc.)
```

### Path resolution strategy

All paths in a loaded `Extension` are absolute. Resolution happens once during
`read_extension()`. This matches TS Quarto's approach and simplifies downstream
code.

### Config merging reuse

Extension format metadata is a `ConfigValue` (from `quarto-config`), which means
it automatically participates in the existing merge infrastructure with `!prefer`
and `!concat` tag support. No new merge logic needed.

---

## Open Questions (Cross-cutting)

1. **Semver validation**: Should we validate `version` and `quarto-required` fields
   during parsing, or just store them as strings? TS Quarto uses the `semver` npm
   package for range checking.

2. **Extension caching**: TS Quarto caches extensions per-context. In q2, should we
   cache per-render, per-project, or globally? (Probably per-project is sufficient.)

3. **WASM compatibility**: Which extension features should work in WASM? Metadata
   contributions should be straightforward. Lua filters and custom writers are
   problematic. Format-resources need a VFS strategy.

4. **Schema validation**: Should we validate extension format metadata against the
   Quarto schema, or just pass it through? The schema validation infrastructure
   exists but isn't fully integrated yet.

5. **Error reporting**: Extensions add another source of configuration. How do we
   report errors that trace back to extension YAML? The source-tracking
   infrastructure exists in `quarto-yaml` — we should preserve it through
   extension loading.

6. **Format name mapping**: ~~When a user writes `format: acm-pdf`, how do we
   determine that `acm` is the extension name and `pdf` is the base format?~~
   **RESOLVED**: `parse_format_descriptor()` in `extension/discover.rs` splits on
   the last hyphen matching a known base format. Matches TS Quarto's
   `parseFormatString()`. The `Format` struct needs `target_format` and
   `extension_name` fields to carry this through the pipeline (Phase 1.4b).

7. **Extension ordering**: When multiple extensions contribute to the same format,
   what's the merge order? TS Quarto uses discovery order (built-in first, then
   project hierarchy). We should match this.

## TS Quarto ↔ Rust Quarto Vocabulary

Confirmed via DeepWiki research on `quarto-dev/quarto-cli`:

| Concept | TS Quarto | Rust Quarto (q2) |
|---------|-----------|------------------|
| Full format string (`"acm-pdf"`) | "format string", stored in `FormatIdentifier["target-format"]` | `Format.target_format: String` |
| Base pandoc format (`"pdf"`) | `FormatDescriptor.baseFormat`, `FormatIdentifier["base-format"]` | `Format.identifier: FormatIdentifier` (enum) |
| Extension name (`"acm"`) | `FormatDescriptor.extension`, `FormatIdentifier["extension-name"]` | `Format.extension_name: Option<String>` |
| Human-readable label | `FormatIdentifier["display-name"]` | `Format.display_name: String` |
| Parse format string | `parseFormatString()` in `pandoc-formats.ts` | `parse_format_descriptor()` in `extension/discover.rs` |
| Read extension format | `readExtensionFormat()` in `render-contexts.ts` | `build_extension_metadata_layer()` in `metadata_merge.rs` |
| Resolve all formats | `resolveFormats()` in `render-contexts.ts` | `MetadataMergeStage` pipeline stage |
| Universal metadata type | `Format` (big object with metadata) | `ConfigValue` (from `quarto-pandoc-types`) |
| Multi-layer merge | `mergeConfigs()` | `MergedConfig::new(layers).materialize()` |

## References

- TS Quarto extension code: `~/src/quarto-cli/src/extension/`
- TS Quarto extension types: `~/src/quarto-cli/src/extension/types.ts`
- TS Quarto extension schema: `~/src/quarto-cli/src/resources/schema/extension.yml`
- TS Quarto format resolution: `~/src/quarto-cli/src/command/render/render-contexts.ts`
- TS Quarto project metadata merge: `~/src/quarto-cli/src/project/project-context.ts`
- q2 metadata merge: `crates/quarto-core/src/stage/stages/metadata_merge.rs`
- q2 filter resolve: `crates/quarto-core/src/filter_resolve.rs`
- q2 user filters plan: `claude-notes/plans/2026-03-16-user-filters-pipeline.md`
- q2 config merging design: `claude-notes/plans/2025-12-07-config-merging-design.md`
