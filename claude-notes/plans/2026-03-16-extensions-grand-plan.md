# Quarto Extensions Grand Plan

**Created**: 2026-03-16
**Status**: In Progress (Phases 1, 2, 3, 4 complete)
**Sub-plans**:
- Phase 1: `claude-notes/plans/2026-03-16-extensions-phase1-yml-and-metadata.md`
- Phase 2: `claude-notes/plans/2026-03-17-extensions-phase2-filter-resolution.md`
- Phase 3: `claude-notes/plans/2026-03-20-extensions-phase3-shortcode-resolution.md`
- Phase 4: `claude-notes/plans/2026-03-16-extensions-phase4-templates-partials.md`

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

**Status**: Complete (merged as `68420002`)

All sub-phases done. `Format` struct has `target_format`, `extension_name`,
`display_name` fields. Extension metadata merges correctly into the pipeline.
`format-resources` deferred to a later phase. See sub-plan for details.

---

### Phase 2: Extension Filter Resolution ✅

**Goal**: Resolve filter names that reference extensions (e.g., `filters: [lightbox]`
where `lightbox` is an extension contributing filters).

- [x] Update `filter_resolve.rs` to accept extension context
- [x] When a filter name doesn't resolve to a file path, look it up in extensions
- [x] If found, substitute the extension's contributed filter paths
- [x] Handle per-format filters from format extensions
- [x] Tests: extension filter resolution, missing extension → error

**Status**: Complete (merged as `cffc2e6c`)

Two mechanisms implemented:
1. **Per-format filters**: `mark_path_valued_keys()` converts filter paths in
   extension format metadata to `ConfigValueKind::Path`, which gets rebased by
   `adjust_paths_to_document_dir()` during metadata merge.
2. **Name-based resolution**: `resolve_filters()` accepts `&[Extension]` and
   `&dyn SystemRuntime`. Uses file-first resolution (matching TS Quarto): check
   if path exists on disk, only try extension lookup if it doesn't. Supports
   both string and map forms with `at` propagation.

**Detail plan**: `claude-notes/plans/2026-03-17-extensions-phase2-filter-resolution.md`

### Phase 3: Extension Shortcode Resolution ✅

**Goal**: Resolve shortcode references from extensions and wire them into the
shortcode processing pipeline. Includes block-level shortcode support and a new
`LuaShortcodeEngine` in pampa for loading and dispatching Lua shortcode handlers.

- [x] Mark per-format shortcode paths as `ConfigValueKind::Path` in `mark_path_valued_keys()`
- [x] Create `LuaShortcodeEngine` in pampa (single Lua state, load scripts, dispatch by name)
- [x] Add block-level shortcode support (`ShortcodeResult::Blocks`, two-pass resolution)
- [x] Wire extensions and Lua engine into `ShortcodeResolveTransform`
- [x] Name-based extension lookup for unknown shortcode names
- [x] `quarto.shortcode` Lua API (`read_arg`, `error_output`)
- [x] Integration and smoke tests

**Detail plan**: `claude-notes/plans/2026-03-20-extensions-phase3-shortcode-resolution.md`

**Depends on**: Phase 1 (extension discovery), Phase 2 (filter resolution pattern)

### Phase 4: Template and Partial Support ✅

**Goal**: Format extensions can provide custom templates and template partials.

- [x] Read `template` and `template-partials` from extension format metadata
- [x] Wire into `ApplyTemplateStage` (uses `quarto-doctemplate`)
- [x] Template search order: extension template → default template
- [x] Partial search order: extension partials → default partials
- [x] Tests (unit + smoke)

**Completed**: Phase 4 implemented across 7 sub-phases. Key changes:
- **Dead code cleanup**: Removed unused `ApplyTemplateConfig.template`,
  `HtmlRenderConfig.template`, `render_with_custom_template()`, and TODO at
  `pipeline.rs:375`.
- **Path resolution**: `template`/`template-partials` values in extension YAML
  are converted to `ConfigValueKind::Path` in `parse_formats()`, then rebased
  by `adjust_paths_to_document_dir()` during metadata merge.
- **Template compilation**: `ApplyTemplateStage` extracts template/partials from
  merged metadata, compiles with `RuntimeResolver` (WASM-compatible) and/or
  `ChainedResolver` + `MemoryResolver` for explicit partials.
- **Rendering refactor**: Extracted `render_with_compiled_template()` as shared
  core. `render_with_format()` and `render_with_resources()` delegate to it.
  Full-template extras (version, page-layout) always injected.
- **Context stripping**: `template` and `template-partials` excluded from
  template context alongside `css`.
- **Detail plan**: `claude-notes/plans/2026-03-16-extensions-phase4-templates.md`

### Phase 5a: Format Extensions (resolution & apply)

**Goal**: Extension-contributed output formats — the **common** Quarto 1 case
(journal templates like ACM/AGU/JSS, presentation themes) — resolve and apply
end-to-end. `--to acm-pdf` finds the `acm` extension and layers its
`contributes.formats.pdf` (+ `common`) bundle (metadata, per-format filters,
shortcodes, `template-partials`, `format-resources`, SCSS/theme) over the `pdf`
base.

**Distinct from Phase 5 (Custom Writers):** a format extension targets a
*known* base format and layers config on it; it does **not** define a new
Pandoc target. A custom writer (Phase 5) is a `.lua` writer that *does*. They
are orthogonal — a format extension may also carry `writer: x.lua` (→ Phase
5) — and most real Q1 extensions are format extensions, not custom writers.

- [ ] Wire extension context into format resolution so `<ext>-<base>` loads the
  extension's `formats[base]` (+ `common`) bundle
- [ ] Apply the bundle (metadata merge base→ext→user; per-format
  filters/shortcodes; `template-partials`; `format-resources` copying;
  SCSS/theme layering)
- [ ] Validate the extension actually contributes the requested base format
  (loud error if not)
- [ ] Tests against a real journal fixture (ACM/AGU)

**Detail plan**: `claude-notes/plans/2026-06-22-format-extensions.md` (STUB — needs research)

**Status**: STUB — ingredients exist (Phases 1–4 + `Contributes.formats` +
`parse_format_descriptor`); the resolution-and-apply glue is unbuilt.

### Phase 5: Custom Writers

**Goal**: Extensions can provide custom Pandoc **Lua writers** (`.lua` format
keys that define a *new* output target) — distinct from Phase 5a format
extensions, which layer on a *known* base. A format extension may carry a
custom writer via `writer: x.lua`; this phase handles that `.lua`-writer path.

- [ ] Detect format keys ending in `.lua` → custom writer format
- [ ] Resolve writer path relative to extension directory
- [ ] Wire into rendering pipeline (likely replaces `RenderHtmlBodyStage`)
- [ ] Tests

**Open questions**:
- Does pampa support custom Lua writers currently?
- How would this interact with the WASM pipeline?

### Phase 6: RevealJS Plugin Support (extension-contributed)

**Goal**: Extensions can contribute reveal.js plugins via
`contributes: revealjs-plugins:` (Q1 shape: plugin `name` + `script[]` +
`stylesheet[]`, with a `plugin.yml` carrying name/scripts/stylesheets/config).
At render, the listed plugins' assets are registered and their globals injected
into the `Reveal.initialize({ plugins: [...] })` call, with config merged
(plugin defaults → user front-matter).

**RevealJS output now exists** (this answers the original open questions). The
`q2 render` path produces **static reveal.js HTML via Rust AST transforms** —
`RevealSlidesTransform` + `render_revealjs_document`, with `reveal_config_json()`
in `crates/quarto-core/src/revealjs/assemble.rs` emitting
`<script>Reveal.initialize({config})</script>`; reveal.js 6 is vendored at
`resources/revealjs/`. (The render path is **Rust, not React** — only the
hub-client *preview* renders the same shared slide-split AST via
`@revealjs/react`, kept in parity by golden tests.) See
`claude-notes/plans/2026-06-08-revealjs-presentations.md`.

**Hard dependency — there is no plugin plumbing yet.** Today q2 loads **zero**
reveal.js plugins: `reveal_config_json()` has no `plugins:` key, no asset
registration for plugin JS/CSS, and not even the core plugins (Notes/Search/
Zoom/Math). The **revealjs epic's own Phase 6 (Plugins/chrome)** is what vendors
+ wires the *built-in* plugins and adds the `plugins: [...]` init plumbing.
**Extension-contributed plugins (this phase) depend on that plumbing** — so
sequence this after (or co-design it with) the revealjs epic's plugin work, and
reuse its registration + init-emission seam rather than building a parallel one.

- [ ] Add a `revealjs_plugins` field to the `Contributes` struct + parse
  `contributes: revealjs-plugins:` (Q1 shape: `name`, `script[]`,
  `stylesheet[]`; read the plugin's `plugin.yml` for name/scripts/stylesheets/config)
- [ ] Discover plugin contributions via the extension system when a document
  lists `revealjs-plugins: [...]`
- [ ] Register plugin JS/CSS as artifacts (reuse the artifact store / theme-CSS
  keying; copy to `site_libs/revealjs/plugin/<name>/`)
- [ ] Emit plugin globals + merged config into `Reveal.initialize({ plugins: [...] })`
  in `assemble.rs`, reusing the built-in-plugin init seam
- [ ] Render/preview parity: the `@revealjs/react` preview path must load the
  same plugins (or be documented as not-yet-at-parity)
- [ ] Per-format reveal plugins; tests with a real plugin extension (menu/chalkboard)

**Open questions**:
- Sequencing/co-design with the revealjs epic's built-in-plugin Phase 6 — share
  one registration + `plugins: [...]` init seam for built-ins and extensions.
- Preview path (`@revealjs/react`): how reveal.js plugins (which target the
  global `Reveal`) load under the React wrapper.

### Phase 7: Project Extensions

**Status: STUB / research.** We've researched what Q1 project extensions *are*
(below); we have **not** designed the q2 implementation. The items under "To
research" are open questions, not a vetted checklist.

**What a project extension is (from Q1).** `contributes: project:` is
**external-toolchain integration glue** — not config layering, and not a new
project type. The marquee cases (docusaurus, hugo) make Quarto a *renderer
inside another tool's project*: Quarto renders `.qmd → markdown` that an
external static-site generator then consumes. An extension supplies:
- **detection** — `project.detect` glob-sets that auto-recognize a directory
  (e.g. `hugo.toml` + `content/` ⇒ a hugo project);
- a **target format** to render to (`format: hugo-md` / `docusaurus-md`);
- a **preview command** — `preview.serve` (cmd/env/ready) launching the
  external dev server (`hugo serve`, `npm run docusaurus start`);
- **pre-render / post-render scripts** — arbitrary executables run around the
  render with a defined env-var contract;
- plus passive config layered onto a **built-in** type (`project.type`,
  `website.*`, `render` globs, `output-dir`, …).

**It does NOT define new project types.** Q1 has four built-in types
(default/website/book/manuscript); an extension's "type" (e.g. `docusaurus`) is
an extension *id* that detects a directory, **remaps to a built-in type** (via
its own `project.type` field), and layers config. `ProjectType` *behavior* (the
render hooks) stays built-in. The active parts — detection, the serve command,
the pre/post-render scripts — are the real work; the config layering is the
easy part. So the honest framing is "integrate an external SSG," not "add
metadata."

**What exists in q2 (verified).**
- `Contributes.project: Option<ConfigValue>` is **parsed** (`extension/read.rs:179`)
  and then **stored-and-ignored** — nothing consumes it. That is the entire
  current implementation (ghostware).
- q2 has built-in project types via the two-pass orchestrator (at least
  `DefaultProjectType`; the website-project epic added website handling), and
  `ProjectType` exposes `pre_render` / `post_render` hooks the orchestrator runs
  between/after passes. Whether those are the right home for *user* scripts is
  not yet researched.

**To research (open — needed before this is plannable).**
- **Scope first:** is external-SSG integration even a near-term q2 goal, or is
  the near-term target just honoring extension-contributed project *config*
  (sidebar/format/render) layered onto built-in types — deferring serve +
  scripts? This decides how large Phase 7 is.
- **Resolution:** how would q2 resolve an extension-as-project-type and merge
  its config, and where does that sit relative to `ProjectContext` / the
  orchestrator? (Q1 does this in `resolveProjectExtension` +
  `mergeProjectMetadata`, project-context.ts:678 — a *reference*, not a q2
  design.)
- **Detection:** Q1's "extension-id-as-type + `detect` globs + auto-detect
  resolver" model, or something simpler? q2 has no project-type detection today.
- **Scripts:** are the orchestrator's `pre_render`/`post_render` hooks the
  execution point (via `SystemRuntime::execute`, project cwd, Q1's env-var
  contract `QUARTO_PROJECT_OUTPUT_DIR` / `_INPUT_FILES` / `_OUTPUT_FILES`)? And
  the WASM story — scripts can't run in the browser preview.
- **`preview.serve`:** how would an external dev-server command coexist with
  `q2 preview` (which serves its own SPA)? Likely the hardest piece.

**References.** Q1: `src/project/project-context.ts` (`resolveProjectExtension`,
`projectExtensionsConfigResolver`), `src/command/render/project.ts` (script
execution + env vars), `src/resources/schema/project.yml`. q2:
`extension/read.rs:179`, the orchestrator's `ProjectType`
`pre_render`/`post_render` hooks.

### Phase 8: Engine Extensions

**Goal**: Extensions can provide custom execution engines.

**Status**: Superseded by the TypeScript Engine Extensions grand plan:
`claude-notes/plans/2026-04-16-ts-engine-extensions-subprocess.md`

That plan covers engine extension parsing (adding `engines` to `Contributes`),
subprocess-based execution via Deno, engine discovery/claiming, and registration.
Plan 1 Phase 1D within it handles the `_extension.yml` parsing and `EngineRegistry`
integration that was originally scoped here.

**Answered open questions**:
- q2 currently supports markdown, knitr, and jupyter engines (all built-in)
- Engine selection uses metadata-based detection (`detect_engine()` in `detection.rs`);
  the TS engine plan adds a 4-phase claiming algorithm (file ext → YAML → language scan → fallback)

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

### Phase 12: Semver Validation for `quarto-required`

**Goal**: Validate `quarto-required` against the running Quarto version. q2 has **two surfaces that
share one semver gate**: (1) the **extension-package** field `quarto-required` in `_extension.yml`
(applies to every extension type; Q1 `extension.ts` `validateExtension`); (2) the
**engine-discovery** `quarto_required` an engine module declares (carried on `LoadEngineResult` by
RTQ ENG-1; Q1 `engine.ts` `checkEngineVersionRequirement`). Both check a semver range against
`cli_version()`.

- [ ] Add `semver` crate dependency (dtolnay's — the de facto Rust standard)
- [ ] **Extension-package gate:** parse `quarto-required` as `VersionReq` during `read_extension()`;
      check against `quarto_util::version::cli_version()` during extension discovery; emit a
      diagnostic when unsatisfied.
- [ ] **Engine-discovery gate:** when `LoadEngineResult.quarto_required` (RTQ ENG-1) is set, check it
      at engine registration — the q2 analogue of Q1's `checkEngineVersionRequirement` (`engine.ts:62`).
      Reuse the same `VersionReq` / `cli_version()` helper.
- [ ] Optionally parse and store the extension's `version` field as `semver::Version`
- [ ] Tests (both gates)

**Notes**:
- `cli_version()` returns `"99.9.9-dev"` during development. Under strict semver,
  prereleases don't satisfy range constraints like `>=1.4.0`. Either strip the
  `-dev` suffix before checking or use `99.9.9` as the dev version.
- **Engine-gate compat version (the 0.x problem).** Released q2 is `0.x`, but Q1 engine modules
  declare Quarto-**1** ranges (e.g. julia's `quartoRequired: ">=1.9"`). Enforcing the *engine* gate
  with q2's real version would reject **every** Q1 engine. So the engine gate must check against a
  **Q1-compatible "engine compat version"** (e.g. `"1.11.0"`), distinct from q2's own
  `cli_version()` — a deliberate spoof q2 presents to engine `quarto_required` checks. (Surfaced
  while consolidating plan1c, which originally built this gate in 1c with the spoof; the gate + spoof
  now live here.) **Compat-version source/value — DECIDED (lifted from plan1c, 2026-06-29):** use a
  fixed `"1.11.0"`, isolated behind a single `fn engine_compat_version() -> &str { "1.11.0" }` so
  there is exactly one place to revisit; do **not** scatter the literal. The engine gate compares the
  engine's `quartoRequired` against `engine_compat_version()` (the spoof), **not** `cli_version()`;
  `cli_version()` stays the source for the *extension-package* gate. This is a clearly-commented
  stopgap until q2 settles its real version-compat story with the Q1 engine ecosystem.
- **Severity — decide deliberately.** Current Q1 source **throws** on *both* surfaces — the
  extension gate (`extension.ts` `validateExtension`: "… is incompatible with this quarto version")
  and the engine gate (`engine.ts` `checkEngineVersionRequirement`: hard `throw`). (An earlier note
  here claimed Q1 *warns*; that is **stale** — the only `…AndWarn` path is for deprecated-now-built-in
  extensions, not version mismatch.) q2 may keep throw for the engine gate (an unrunnable engine is a
  hard failure) and choose warn-vs-throw for non-engine contributions.
- The extension's own `version` field is currently stored as a plain string.
  Could optionally be parsed as `semver::Version` for consistency.

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

1. ~~**Semver validation**~~: Moved to Phase 12.

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

## Design Decisions (Resolved)

6. **Format name mapping**: `parse_format_descriptor()` in `extension/discover.rs`
   splits on the last hyphen matching a known base format (e.g., `"acm-pdf"` →
   extension `"acm"`, base `"pdf"`). The `Format` struct carries `target_format`,
   `extension_name`, and `display_name` fields. Matches TS Quarto's
   `parseFormatString()`. (Resolved in Phase 1.4b.)

7. **Extension ordering**: Multiple extensions contributing to the same format are
   merged in discovery order: built-in first, then project hierarchy (closest to
   project root first, closest to document last). This matches TS Quarto's behavior.
   Implemented in `discover_extensions()`. (Resolved in Phase 1.3.)

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
- Metadata pipeline / Lua detection: `claude-notes/plans/2026-03-18-metadata-pipeline-lua-detection.md`
- q2 config merging design: `claude-notes/plans/2025-12-07-config-merging-design.md`
