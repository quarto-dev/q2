# Phase 7: SASS Render Integration

**Parent Plan**: `2026-01-13-sass-compilation.md`
**Created**: 2026-01-24
**Status**: In Progress (Phase 7.5 mostly complete, needs browser testing)

## Session Summary (2026-01-24, continued)

### Latest Session Work

**Phase 7.5: Hub-Client UI Integration** - MOSTLY COMPLETE
- Added new WASM exports to `WasmModuleExtended` TypeScript interface
- Created `compileDocumentCss()` TypeScript function with IndexedDB caching
- Created `compileThemeCssByName()` and `compileDefaultBootstrapCss()` functions
- Created `extractThemeConfigForCacheKey()` for cache key computation
- Modified `renderToHtml()` to compile theme CSS and inject into VFS
- Created `compileAndInjectThemeCss()` internal helper
- All hub-client tests pass (46 tests)
- WASM build successful

### Remaining Work
- Consider optimizations for first-load experience
- Epic for HTML structure compatibility with TypeScript Quarto (Bootstrap container classes, sections, etc.)

### Additional Work Done (Later in Session)
- Fixed SASS VFS callbacks not being initialized (hub-client couldn't read Bootstrap SCSS files)
- Fixed UTF-8 base64 encoding error in iframePostProcessor
- Implemented native render theme support:
  - Added `quarto-sass` dependency to quarto binary
  - Created `extract_theme_config()` to parse YAML frontmatter
  - Created `write_themed_resources()` to use SASS compilation
  - `quarto render` now compiles theme CSS (e.g., `theme: darkly` produces full Bootstrap+Darkly CSS)

---

## Session Summary (2026-01-24, initial)

### Completed Work

**Phase 7.1: Theme Configuration Module** - COMPLETE
- Created `quarto-sass/src/config.rs` with `ThemeConfig` struct
- Implemented `ThemeConfig::from_config_value()` for extracting theme config from `ConfigValue`
- Handles: string theme, array theme, null/absent theme (default Bootstrap)
- Added `InvalidThemeConfig` error variant
- 13 unit tests

**Phase 7.2: High-Level Compilation API** - COMPLETE
- Created `quarto-sass/src/compile.rs` with native-only compilation functions
- `compile_theme_css()` - Main entry point for render pipeline
- `compile_css_from_config()` - Convenience function combining extraction and compilation
- `compile_default_css()` - Cached default Bootstrap compilation
- Created `CombinedResources` provider to bundle all embedded resources
- 10 integration tests

**Phase 7.3: Native Render Integration** - PARTIAL
- Added `quarto-sass` as native-only dependency to `quarto-core`
- Created `write_html_resources_with_sass()` in resources.rs
- 2 new integration tests
- All 495 quarto-core tests pass

**Phase 7.4: WASM Render Integration** - MOSTLY COMPLETE
- Added WASM implementations of compile functions in `compile.rs` (async, uses JS dart-sass bridge)
- New WASM exports: `compile_document_css()`, `compile_theme_css_by_name()`, `compile_default_bootstrap_css()`
- Frontmatter parsing helpers for extracting theme config from QMD content
- Caching delegated to TypeScript via existing `SassCacheManager`
- Render functions NOT modified (better to handle CSS separately for caching)
- Integration tests pending (requires WASM build environment)

**Test Summary**: 626 tests passing (131 quarto-sass + 495 quarto-core)

### Remaining Work
- Phase 7.5: Hub-Client UI Integration
- Phase 7.6: Testing & Validation (including WASM integration tests)

## Overview

Integrate the SASS compilation infrastructure (Phases 1-6) with the `quarto render` pipeline for both native (CLI) and WASM (hub-client) targets. This enables:

- Bootstrap theming via `format.html.theme` configuration
- Custom SCSS files alongside built-in themes
- Proper CSS generation instead of static `DEFAULT_CSS`

## Design Decisions

### D1: Default Behavior (No Theme Specified)

**Decision**: Compile Bootstrap's `default` theme instead of using static `DEFAULT_CSS`.

**Rationale**: The goal is feature parity with TypeScript Quarto. Even without explicit theme configuration, documents should get properly compiled Bootstrap CSS.

### D2: Theme Configuration Source

**Decision**: Use Rust Quarto's configuration system (`ConfigValue`) to extract theme from:
1. Document frontmatter: `format.html.theme`
2. Project config (`_quarto.yml`): `format.html.theme`

Document config overrides project config via standard merge semantics.

### D3: Theme Configuration Module

**Decision**: Create a dedicated `quarto-sass-config` module (initially in `quarto-sass` crate) for:
- Extracting theme configuration from `ConfigValue`
- Converting to `ThemeSpec` array for compilation
- This pattern will be reusable for other subsystems

### D4: Integration Points

| Target | Integration Point | Action |
|--------|-------------------|--------|
| Native | `quarto-core/src/resources.rs` | Replace `DEFAULT_CSS` write with compiled CSS |
| Native | `quarto/src/commands/render.rs` | Pass theme config to resource writing |
| WASM | `wasm-quarto-hub-client/src/lib.rs` | Add SCSS compilation to render functions |

---

## Architecture

### Theme Configuration Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Document Frontmatter                            │
│  ---                                                                 │
│  format:                                                             │
│    html:                                                             │
│      theme: cosmo                                                    │
│  ---                                                                 │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      ConfigValue (merged)                            │
│  Project config + Document frontmatter merged via MergeOp            │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      ThemeConfig (new type)                          │
│  Extracted from ConfigValue:                                         │
│  - theme: Vec<ThemeSpec>   // [cosmo, custom.scss, ...]             │
│  - minified: bool          // Production vs development              │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      SASS Compilation Pipeline                       │
│  process_theme_specs() → assemble_themes() → grass/dart-sass        │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Compiled CSS                                    │
│  Written to {stem}_files/styles.css                                  │
└─────────────────────────────────────────────────────────────────────┘
```

### Module Structure

```
quarto-sass/
├── src/
│   ├── config.rs       # NEW: ThemeConfig extraction from ConfigValue
│   ├── compile.rs      # NEW: High-level compilation API
│   ├── bundle.rs       # Existing: SCSS assembly
│   ├── themes.rs       # Existing: Theme loading
│   └── ...
└── ...

quarto-core/
├── src/
│   ├── resources.rs    # MODIFIED: Use compiled CSS
│   ├── sass.rs         # NEW: SASS integration for render pipeline
│   └── ...
└── ...
```

---

## Implementation Plan

### Phase 7.1: Theme Configuration Module

Create `quarto-sass/src/config.rs` to extract theme configuration from `ConfigValue`.

**New types:**

```rust
/// Extracted theme configuration from document/project metadata.
#[derive(Debug, Clone, Default)]
pub struct ThemeConfig {
    /// Theme specifications (built-in names or file paths).
    /// Empty means use default Bootstrap theme.
    pub themes: Vec<ThemeSpec>,

    /// Whether to produce minified CSS.
    pub minified: bool,
}

impl ThemeConfig {
    /// Extract theme config from merged ConfigValue.
    ///
    /// Looks for `format.html.theme` in the config.
    /// Supports:
    /// - String: single theme name or path
    /// - Array: multiple themes to layer
    /// - Null/absent: use default Bootstrap theme
    pub fn from_config_value(config: &ConfigValue) -> Result<Self, SassError>;

    /// Create config for default Bootstrap theme.
    pub fn default_bootstrap() -> Self;
}
```

**Work items:**

- [x] Create `config.rs` module
- [x] Implement `ThemeConfig` struct
- [x] Implement `from_config_value()` - parse `format.html.theme`
- [x] Handle string theme: `theme: cosmo`
- [x] Handle array theme: `theme: [cosmo, custom.scss]`
- [x] Handle absent theme (default to Bootstrap default)
- [x] Add unit tests for all config formats (13 tests)
- [x] Export from `lib.rs`

### Phase 7.2: High-Level Compilation API

Create `quarto-sass/src/compile.rs` with a simplified API for the render pipeline.

**New functions:**

```rust
/// Compile CSS from theme configuration.
///
/// This is the main entry point for the render pipeline.
pub fn compile_theme_css(
    config: &ThemeConfig,
    context: &ThemeContext,
) -> Result<String, SassError>;

/// Compile CSS from ConfigValue directly.
///
/// Convenience function that combines config extraction and compilation.
pub fn compile_css_from_config(
    config: &ConfigValue,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<String, SassError>;

/// Compile the default Bootstrap CSS.
///
/// Used when no theme is specified.
pub fn compile_default_css(runtime: &dyn SystemRuntime) -> Result<String, SassError>;
```

**Work items:**

- [x] Create `compile.rs` module
- [x] Implement `compile_theme_css()`
- [x] Implement `compile_css_from_config()`
- [x] Implement `compile_default_css()` - cached for performance
- [x] Add integration tests (10 tests)
- [x] Export from `lib.rs`
- [x] Create `CombinedResources` provider for bundling all embedded resources

### Phase 7.3: Native Render Integration

Modify `quarto-core` to use SASS compilation during rendering.

**Changes to `quarto-core/src/resources.rs`:**

```rust
/// Write HTML resources with compiled SASS.
///
/// If theme_config is provided, compiles SCSS to CSS.
/// Otherwise, compiles default Bootstrap.
pub fn write_html_resources_with_sass(
    output_dir: &Path,
    stem: &str,
    config: &ThemeConfig,
    context: &ThemeContext,
    runtime: &dyn SystemRuntime,
) -> Result<HtmlResourcePaths>;
```

**Changes to `quarto/src/commands/render.rs`:**

1. Extract theme config from `RenderContext`
2. Create `ThemeContext` from document directory
3. Call `write_html_resources_with_sass()` instead of `write_html_resources()`

**Work items:**

- [x] Add `quarto-sass` dependency to `quarto-core` (native-only)
- [x] Create `write_html_resources_with_sass()` in resources.rs
- [x] Add tests for compiled CSS output (2 tests)
- [x] Verify all existing tests still pass (495 tests pass)
- [x] Implement `extract_theme_config()` from frontmatter in render.rs
- [x] Update `render_document()` to call `write_html_resources_with_sass()`
- [x] Test with various theme configurations

**Note**: Implemented frontmatter parsing directly rather than waiting for full ConfigValue system.
Theme compilation now works in native `quarto render` command - CSS file contains full Bootstrap with theme colors.

### Phase 7.4: WASM Render Integration

Modify WASM rendering to use SASS compilation with caching.

**Changes to `quarto-sass/src/compile.rs`:**

Added WASM implementations of compile functions using the JS dart-sass bridge:
- `compile_theme_css()` - async version using `runtime.compile_sass()`
- `compile_css_from_config()` - async version
- `compile_default_css()` - async version (no caching - delegated to JS side)

**Changes to `wasm-quarto-hub-client/src/lib.rs`:**

1. Added theme config extraction from YAML frontmatter
2. Added new WASM exports for theme-aware CSS compilation
3. Helper functions for JSON-to-ConfigValue conversion

**New WASM exports:**

```rust
/// Compile CSS for a document's theme configuration.
/// Extracts theme from the document's YAML frontmatter and compiles.
#[wasm_bindgen]
pub async fn compile_document_css(content: &str) -> String;

/// Compile CSS for a specific theme name (e.g., "cosmo", "darkly").
#[wasm_bindgen]
pub async fn compile_theme_css_by_name(theme_name: &str, minified: bool) -> String;

/// Compile default Bootstrap CSS (no theme customization).
#[wasm_bindgen]
pub async fn compile_default_bootstrap_css(minified: bool) -> String;
```

**Work items:**

- [x] Add WASM implementations of compile functions in `compile.rs`
- [x] Add theme config extraction in WASM via frontmatter parsing
- [x] Implement `compile_document_css()` WASM export
- [x] Implement `compile_theme_css_by_name()` WASM export
- [x] Implement `compile_default_bootstrap_css()` WASM export
- [ ] Modify `render_qmd()` to compile and include CSS (deferred - better handled by TypeScript for caching)
- [ ] Modify `render_qmd_content()` similarly (deferred - better handled by TypeScript for caching)
- [x] Caching delegated to TypeScript via existing `SassCacheManager`
- [ ] Add integration tests (requires WASM build environment)

#### Design Decision: Separate CSS Compilation from Rendering

The render functions (`render_qmd()`, `render_qmd_content()`) were NOT modified to inline CSS compilation. Instead, CSS compilation is exposed as separate WASM exports that TypeScript calls independently. Here's the detailed rationale:

**How CSS Currently Works in the Render Pipeline:**

1. During `render_qmd_to_html()` (in `apply_template.rs`), a static `DEFAULT_CSS` is stored as an artifact at `/.quarto/project-artifacts/styles.css`
2. After rendering, all artifacts (including CSS) are written to the VFS
3. The HTML output references the CSS via a `<link>` tag pointing to that artifact path

For theme support, we need to compile SCSS instead of using static CSS.

**Option A: Compile Inside `render_qmd()` (NOT chosen)**

```rust
pub async fn render_qmd(path: &str) -> String {
    // Extract theme from frontmatter
    let theme_config = extract_theme_from_content(&content)?;
    // Compile CSS (takes ~1-2 seconds)
    let css = compile_theme_css(&theme_config, &context).await?;
    // Replace DEFAULT_CSS artifact with compiled CSS
    // ... continue with render ...
}
```

Problems:
- **Every render recompiles CSS** - even if the same theme is used
- **No caching** - Rust/WASM has no access to IndexedDB
- **Slow previews** - typing in the editor triggers re-renders, each waiting 1-2s for SCSS
- **Wasteful** - the same `theme: cosmo` compiles to identical CSS every time

**Option B: Separate CSS Compilation (CHOSEN)**

```typescript
// TypeScript in hub-client
async function renderWithTheme(qmdContent: string) {
    const cache = getSassCache();
    const cacheKey = await cache.computeKey(extractTheme(qmdContent), true);

    // Check cache first
    let css = await cache.get(cacheKey);
    if (!css) {
        const result = JSON.parse(await compile_document_css(qmdContent));
        css = result.css;
        await cache.set(cacheKey, css, ...);
    }

    // Render HTML (fast, no SCSS compilation)
    const renderResult = JSON.parse(await render_qmd_content(qmdContent, ""));

    // Inject CSS and HTML into preview
    applyCSS(css);
    injectHTML(renderResult.html);
}
```

**Comparison:**

| Aspect | Option A (inline) | Option B (separate) |
|--------|-------------------|---------------------|
| Implementation | Simpler Rust | More TypeScript |
| Caching | None | Full IndexedDB persistence |
| First render | 1-2s | 1-2s |
| Subsequent (same theme) | 1-2s | <10ms (cache hit) |
| User types in editor | Each keystroke waits | Only recompiles if theme changes |

**Why Option B is Better:**

1. **User experience** - Preview updates instantly when editing text
2. **Existing infrastructure** - `SassCacheManager` is already built with LRU eviction
3. **Theme switching** - Changing themes only recompiles once, then cached
4. **Offline support** - IndexedDB cache works without network
5. **Granular control** - TypeScript decides when to recompile (theme changed vs. text edited)

### Phase 7.5: Hub-Client UI Integration

Wire up hub-client TypeScript to use compiled CSS in previews.

**Changes to `hub-client/src/services/wasmRenderer.ts`:**

```typescript
/// Render QMD with theme CSS compilation.
export async function renderQmdWithTheme(
    content: string,
    options?: RenderOptions
): Promise<RenderResult>;
```

**Work items:**

- [x] Add `compile_document_css()` to WasmModuleExtended interface
- [x] Create `compileDocumentCss()` TypeScript function with caching
- [x] Update `renderToHtml()` to compile and inject theme CSS
- [x] Inject compiled CSS into VFS before post-processing
- [ ] Verify cache is used for repeated compilations (requires WASM build)
- [ ] Test with various theme configurations (requires WASM build)

### Phase 7.6: Testing & Validation

Comprehensive testing for the integration.

**Test scenarios:**

1. **No theme specified** → Default Bootstrap CSS
2. **Single built-in theme** → Compiled theme CSS (e.g., `cosmo`)
3. **Array of themes** → Merged/layered CSS
4. **Custom SCSS file** → Custom + Bootstrap merged
5. **Project + document config** → Proper merge behavior
6. **WASM caching** → Verify cache hits

**Work items:**

- [ ] Native integration tests in `quarto-sass`
- [ ] WASM integration tests (browser testing)
- [ ] End-to-end test: QMD with theme → HTML with correct CSS
- [ ] Performance test: compilation time acceptable
- [ ] Cache verification: repeated renders use cache

---

## Dependencies

### New Crate Dependencies

```toml
# quarto-core/Cargo.toml
[dependencies]
quarto-sass = { workspace = true }
```

### Existing Infrastructure Used

| Component | Location | Usage |
|-----------|----------|-------|
| ThemeSpec | quarto-sass/themes.rs | Theme specification parsing |
| ThemeContext | quarto-sass/themes.rs | Path resolution, runtime access |
| process_theme_specs | quarto-sass/themes.rs | Layer assembly with customization |
| assemble_themes | quarto-sass/bundle.rs | SCSS bundle generation |
| ConfigValue | quarto-pandoc-types | Config extraction |
| SystemRuntime | quarto-system-runtime | Cross-platform file access |
| SassCacheManager | hub-client/sassCache.ts | WASM caching |

---

## Migration Notes

### Breaking Changes

None expected. The `write_html_resources()` function signature may change, but it's internal to quarto-core.

### Backward Compatibility

- Documents without theme config get default Bootstrap (behavior change from static CSS)
- This is intentional for TS Quarto parity

---

## Open Questions

### Q1: Pre-compiled Default CSS

Should we pre-compile the default Bootstrap CSS for faster time-to-first-view?

**Options:**
1. Always compile at runtime (current plan)
2. Ship pre-compiled CSS, compile only for custom themes
3. Compile once and cache in IndexedDB (WASM only)

**Current decision**: Option 1 (compile at runtime). Consider Option 3 as future optimization.

### Q2: CSS Minification

When should CSS be minified?

**Options:**
1. Always minified (smaller files)
2. Never minified (easier debugging)
3. Configurable via `format.html.css-minify`
4. Based on environment (dev vs prod)

**Current decision**: Option 1 (always minified) for consistency with TS Quarto.

---

## Files to Create/Modify

### New Files

| File | Purpose |
|------|---------|
| `quarto-sass/src/config.rs` | Theme config extraction from ConfigValue |
| `quarto-sass/src/compile.rs` | High-level compilation API |
| `quarto-core/src/sass.rs` | SASS integration for render pipeline |

### Modified Files

| File | Changes |
|------|---------|
| `quarto-sass/src/lib.rs` | Export new modules |
| `quarto-sass/Cargo.toml` | Add quarto-pandoc-types dependency |
| `quarto-core/Cargo.toml` | Add quarto-sass dependency |
| `quarto-core/src/resources.rs` | Use compiled CSS |
| `quarto-core/src/lib.rs` | Export sass module |
| `quarto/src/commands/render.rs` | Theme config extraction, pass to resources |
| `wasm-quarto-hub-client/src/lib.rs` | Add CSS compilation to render |
| `hub-client/src/services/wasmRenderer.ts` | Wire up theme CSS compilation |

---

## Success Criteria

1. **Native rendering** produces compiled Bootstrap CSS (not static DEFAULT_CSS)
2. **Theme configuration** from frontmatter is respected
3. **Project config** merges correctly with document config
4. **All 25 Bootswatch themes** work via `theme: <name>`
5. **Custom SCSS files** work via `theme: custom.scss`
6. **WASM rendering** uses cached CSS compilation
7. **No regression** in existing render tests
8. **Performance**: First compile < 2s, cached < 100ms

---

## Estimated Scope

- **Phase 7.1** (Config): ~150 lines new code
- **Phase 7.2** (Compile API): ~100 lines new code
- **Phase 7.3** (Native): ~200 lines modified
- **Phase 7.4** (WASM): ~150 lines modified
- **Phase 7.5** (Hub-Client): ~100 lines modified
- **Phase 7.6** (Testing): ~300 lines tests

**Total**: ~1000 lines of new/modified code

---

## Resume Instructions

### For Next Session

**Current state**: Phases 7.1-7.5 mostly complete. Browser testing pending.

**To resume**:
1. Read this plan file
2. The SASS compilation infrastructure is fully working - 626 Rust tests passing
3. Hub-client TypeScript integration is complete - 46 tests passing
4. Next steps:
   - Run `npm run dev` in hub-client to test manually in browser
   - Test theme configurations: no theme, single theme, array of themes
   - Verify IndexedDB caching works (check browser DevTools > Application > IndexedDB)
   - Phase 7.6: Comprehensive testing & validation

### Key Files to Review

```bash
# Core compile module (has both native and WASM implementations)
crates/quarto-sass/src/config.rs      # ThemeConfig extraction
crates/quarto-sass/src/compile.rs     # compile_theme_css(), compile_default_css() (native + WASM)

# WASM exports for hub-client
crates/wasm-quarto-hub-client/src/lib.rs  # compile_document_css(), compile_theme_css_by_name(), etc.

# TypeScript integration (NEW)
hub-client/src/services/wasmRenderer.ts   # compileDocumentCss(), renderToHtml() integration
hub-client/src/services/sassCache.ts      # SassCacheManager for IndexedDB caching
```

### Quick Verification

```bash
# Verify everything still works
cargo nextest run -p quarto-sass -p quarto-core
# Expected: 626 tests pass

# Build workspace (native)
cargo build --workspace

# Build WASM module (from hub-client directory)
cd hub-client && npm run build:all
```

### Native API Summary

```rust
// Theme configuration extraction
use quarto_sass::ThemeConfig;
let config = ThemeConfig::from_config_value(&merged_config)?;

// High-level compilation (synchronous on native)
use quarto_sass::{compile_theme_css, ThemeContext};
let context = ThemeContext::new(document_dir, &runtime);
let css = compile_theme_css(&config, &context)?;

// Resource writing with SASS (quarto-core)
use quarto_core::resources::write_html_resources_with_sass;
let paths = write_html_resources_with_sass(
    &output_dir, "document", &theme_config, &context, &runtime
)?;
```

### WASM API Summary (for hub-client TypeScript)

```typescript
// Import from WASM module
import { compile_document_css, compile_theme_css_by_name, compile_default_bootstrap_css } from 'wasm-quarto-hub-client';

// Compile CSS from document content (extracts theme from frontmatter)
const result = JSON.parse(await compile_document_css(qmdContent));
if (result.success) {
    applyCSS(result.css);
}

// Compile a specific theme by name
const cosmoResult = JSON.parse(await compile_theme_css_by_name("cosmo", true));

// Compile default Bootstrap (no theme)
const defaultResult = JSON.parse(await compile_default_bootstrap_css(true));
```

### Dependencies Added

```toml
# quarto-sass/Cargo.toml
quarto-pandoc-types.workspace = true

# quarto-core/Cargo.toml (native-only)
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
quarto-sass.workspace = true
```
