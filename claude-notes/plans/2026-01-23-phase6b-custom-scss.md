# Phase 6b: Custom SCSS Theme Support

**Parent Plan**: `2026-01-13-sass-compilation.md`
**Created**: 2026-01-23
**Status**: Completed

## Completion Summary

All Phase 6b work items (6b.1-6b.5) are complete. Phase 6b.6 (Light/Dark support) is deferred.

**WASM Support (Subplan 2026-01-23-phase6b-wasm-support.md)**: Completed 2026-01-23. `ThemeContext` now holds a `&'a dyn SystemRuntime` for cross-platform file access. On native targets, use `ThemeContext::native()` convenience constructor. On WASM, pass a `WasmRuntime` instance.

### Implemented Features

1. **ThemeSpec type** (`themes.rs`): Enum to distinguish built-in themes from custom file paths. Parses strings based on `.scss`/`.css` extension detection.

2. **ThemeContext** (`themes.rs`): Context for resolving relative paths in custom themes and tracking load paths for @import resolution.

3. **Custom theme loading** (`themes.rs`): `load_custom_theme()` function reads SCSS from filesystem and parses layer boundaries.

4. **Theme layer processing** (`themes.rs`): `process_theme_specs()` processes specs in order with customization injection after each built-in theme (or at beginning for custom-only).

5. **Bundle assembly** (`bundle.rs`): `assemble_with_user_layers()` and `assemble_themes()` for assembling SCSS with multiple user layers.

6. **Integration tests** (`tests/custom_theme_test.rs`): Tests for single custom file, mixed ordering, and @import from custom theme directory.

### Key Implementation Details

- Customization layer injection follows TS Quarto semantics exactly
- `merge_layers()` reverses defaults so later layers take precedence via `!default`
- `CombinedFs` adapter allows @import to resolve from both filesystem and embedded resources
- Error types support future Monaco editor integration (file/line/column info)

---

## Overview

This subphase extends Phase 6 to support custom SCSS files, matching TS Quarto's `theme:` configuration. Currently we only support built-in theme names (e.g., `theme: cosmo`). This adds support for:

- Custom SCSS file paths (`theme: custom.scss`)
- Multiple theme layers (`theme: [cosmo, custom.scss]`)
- Light/dark theme pairs

## Current State

**What we have:**
- `BuiltInTheme` enum with 25 Bootswatch themes
- `load_theme_layer()` - loads from embedded resources only
- `assemble_scss(framework, quarto, theme)` - single theme layer
- `assemble_with_theme(BuiltInTheme)` - convenience for built-in themes
- All 25 built-in themes compile successfully

**What's missing:**
- `ThemeSpec` enum to represent different theme sources
- Loading SCSS from filesystem (not just embedded resources)
- Combining multiple theme layers
- Customization layer injection logic (inject after built-in, or at start)
- Light/dark theme pair support

## TS Quarto Behavior Reference

### Theme Specification Formats

```yaml
# Single built-in theme
theme: cosmo

# Single custom file (relative to document)
theme: custom.scss

# Multiple layers (combined left-to-right)
theme: [cosmo, custom.scss]

# Light/dark pairs
theme:
  light: [cosmo, brand.scss]
  dark: [darkly, brand-dark.scss]
```

### Resolution Rules

1. **Has `.scss` or `.css` extension** → treat as file path
2. **No extension** → treat as built-in theme name
3. **Relative paths** → resolved relative to input document directory
4. **Absolute paths** → used directly

### Customization Layer Injection

- If **built-in theme present**: inject `_bootstrap-customize.scss` **after** that theme
- If **only custom files**: inject at **beginning** of user layers
- Ensures Quarto's heading sizes and other defaults apply

### Layer Merging

When multiple themes specified:
- `uses`, `functions`, `mixins`, `rules`: concatenated in order
- `defaults`: **reversed** (first theme's defaults take precedence via `!default`)

---

## Critical Architectural Insight

The key behavior in TS Quarto is that **the customization layer is injected immediately after each built-in theme**, not at a fixed position. This means order matters significantly:

**`[default, custom.scss]`** produces:
```
layers = [defaultLayer, quartoCustomizeLayer, customLayer]
merged.defaults = custom + quartoCustomize + default  (reversed)
merged.rules    = default + quartoCustomize + custom  (in order)
```
→ Custom variables win; custom CSS can override.

**`[custom.scss, default]`** produces:
```
layers = [customLayer, defaultLayer, quartoCustomizeLayer]
merged.defaults = quartoCustomize + default + custom  (reversed)
merged.rules    = custom + default + quartoCustomize  (in order)
```
→ Quarto's `!default` variables win; Quarto CSS rules can override custom.

Our `merge_layers()` already reverses defaults correctly. The architectural change needed:

- **Current**: `assemble_scss(framework, quarto, Option<theme>)` - single theme
- **Needed**: `assemble_with_user_layers(user_layers: &[SassLayer])` - array of layers that get merged

---

## Implementation Plan

### 6b.1 ThemeSpec Type

Create a `ThemeSpec` enum to represent theme sources:

```rust
/// A theme specification - either a built-in name or a file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSpec {
    /// Built-in Bootswatch theme (e.g., "cosmo")
    BuiltIn(BuiltInTheme),
    /// Custom SCSS file path (absolute or relative)
    Custom(PathBuf),
}

impl ThemeSpec {
    /// Parse a theme string into a ThemeSpec.
    /// - Strings with .scss/.css extension → Custom path
    /// - Other strings → Built-in theme name lookup
    pub fn parse(s: &str) -> Result<Self, SassError>;

    /// Check if this is a built-in theme
    pub fn is_builtin(&self) -> bool;
}
```

**Design decisions:**
- Only support `.scss` and `.css` extensions (matching TS Quarto)
- Invalid built-in names → return `SassError::UnknownTheme` (fail fast)

### 6b.2 Theme Resolver

Create a resolver that loads themes from different sources:

```rust
/// Context for resolving theme paths
pub struct ThemeContext {
    /// Directory containing the input document (for relative path resolution)
    pub document_dir: PathBuf,
    /// Additional load paths for @import resolution
    pub load_paths: Vec<PathBuf>,
}

/// Resolved theme with its layer and metadata
pub struct ResolvedTheme {
    pub layer: SassLayer,
    pub is_builtin: bool,
    pub source_path: Option<PathBuf>,  // For custom files
}

/// Resolve a ThemeSpec to a SassLayer
pub fn resolve_theme_spec(
    spec: &ThemeSpec,
    context: &ThemeContext,
) -> Result<ResolvedTheme, SassError>;
```

**File loading:**
- Built-in: use existing `THEMES_RESOURCES.read_str()`
- Custom: read from filesystem, parse with `parse_layer()`

### 6b.3 Theme Layer Processing (layerTheme equivalent)

Process theme specs into an ordered array of layers with customization injection:

```rust
/// Process theme specifications into layers with customization injection.
///
/// This is the Rust equivalent of TS Quarto's `layerTheme()` function.
///
/// # Customization Injection Rules
/// - After EACH built-in theme, inject `quartoBootstrapCustomizationLayer()`
/// - If NO built-in themes, inject customization at the BEGINNING
///
/// # Example
/// Input: `["cosmo", "custom.scss"]`
/// Output: `[cosmoLayer, customizeLayer, customLayer]`
///
/// Input: `["custom.scss", "default"]`
/// Output: `[customLayer, defaultLayer, customizeLayer]`
pub fn process_theme_specs(
    specs: &[ThemeSpec],
    context: &ThemeContext,
) -> Result<ThemeLayerResult, SassError>;

pub struct ThemeLayerResult {
    /// Layers in order (with customization already injected)
    pub layers: Vec<SassLayer>,
    /// Load paths collected from custom theme directories
    pub load_paths: Vec<PathBuf>,
}
```

**Customization injection (matching TS Quarto exactly):**
```rust
fn process_theme_specs(...) -> Result<ThemeLayerResult, SassError> {
    let mut layers = Vec::new();
    let mut load_paths = Vec::new();
    let mut any_builtin = false;
    let customize_layer = load_quarto_customization_layer()?;

    for spec in specs {
        match spec {
            ThemeSpec::BuiltIn(theme) => {
                layers.push(load_builtin_theme(*theme)?);
                layers.push(customize_layer.clone());  // Inject AFTER each built-in
                any_builtin = true;
            }
            ThemeSpec::Custom(path) => {
                let (layer, theme_dir) = load_custom_theme(path, context)?;
                layers.push(layer);
                load_paths.push(theme_dir);
            }
        }
    }

    // If no built-in themes, inject customization at beginning
    if !any_builtin {
        layers.insert(0, customize_layer);
    }

    Ok(ThemeLayerResult { layers, load_paths })
}
```

This preserves the exact ordering semantics of TS Quarto.

### 6b.4 Light/Dark Theme Support

Support dual-theme configurations:

```rust
/// Theme configuration with optional dark mode
pub struct DualThemeConfig {
    pub light: ThemeConfig,
    pub dark: Option<ThemeConfig>,
    /// If true, dark mode is the default
    pub dark_default: bool,
}

/// Parse theme YAML into DualThemeConfig
pub fn parse_theme_yaml(value: &serde_yaml::Value) -> Result<DualThemeConfig, SassError>;
```

**YAML parsing rules:**
- String → single light theme
- Array → multiple light themes
- Object with `light`/`dark` keys → dual theme

### 6b.5 Updated Bundle Assembly

Update assembly to handle multiple user layers:

```rust
/// Assemble SCSS with processed theme layers.
///
/// Takes the output of `process_theme_specs()` and produces compilable SCSS.
///
/// # Assembly Order
/// 1. USES: framework → quarto → merged_user
/// 2. FUNCTIONS: framework → quarto → merged_user
/// 3. DEFAULTS: merged_user → quarto → framework (reversed in merge_layers)
/// 4. MIXINS: framework → quarto → merged_user
/// 5. RULES: framework → quarto → merged_user
pub fn assemble_with_user_layers(
    user_layers: &[SassLayer],
) -> Result<String, SassError> {
    let framework = load_bootstrap_framework()?;
    let quarto = load_quarto_layer()?;

    // Merge user layers - merge_layers() reverses defaults automatically
    let merged_user = merge_layers(user_layers);

    assemble_scss(&framework, &quarto, Some(&merged_user))
}

/// High-level: compile themes from specs
pub fn compile_themes(
    specs: &[ThemeSpec],
    context: &ThemeContext,
    minified: bool,
) -> Result<String, SassError> {
    // 1. Process specs into layers (with customization injection)
    let result = process_theme_specs(specs, context)?;

    // 2. Assemble SCSS
    let scss = assemble_with_user_layers(&result.layers)?;

    // 3. Compile with load paths
    compile_scss(&scss, minified, &result.load_paths)
}
```

Note: The existing `assemble_scss(framework, quarto, Option<theme>)` remains for single-theme cases and backward compatibility.

---

## Work Items

### Phase 6b.1: ThemeSpec Type
- [x] Create `ThemeSpec` enum in `themes.rs`
- [x] Implement `ThemeSpec::parse()` with extension detection (.scss, .css)
- [x] Add `ThemeSpec::is_builtin()` helper
- [x] Add unit tests for parsing: `"cosmo"`, `"custom.scss"`, `"/abs/path.scss"`, `"invalid"`

### Phase 6b.2: Theme Context and Loading
- [x] Create `ThemeContext` struct (document_dir, load_paths)
- [x] Implement `load_custom_theme()` - read from filesystem + parse_layer()
- [x] Add error types: `FileNotFound`, `InvalidScssFile` (no boundaries)
- [x] Add unit tests for loading custom files

### Phase 6b.3: Theme Layer Processing
- [x] Create `ThemeLayerResult` struct
- [x] Implement `process_theme_specs()` with customization injection
- [x] Test: customization injected after each built-in theme
- [x] Test: customization at beginning when only custom files
- [x] Test: ordering `[builtin, custom]` vs `[custom, builtin]`

### Phase 6b.4: Bundle Assembly Update
- [x] Create `assemble_with_user_layers()` function
- [x] Create `assemble_themes()` high-level function (renamed from compile_themes)
- [x] Verify merge_layers() reverses defaults correctly for multi-layer
- [x] Add integration tests

### Phase 6b.5: Integration Tests
- [x] Create test SCSS files in `test-fixtures/custom/`
- [x] Test: single custom file only
- [x] Test: `[cosmo, custom.scss]` - custom overrides built-in
- [x] Test: `[custom.scss, cosmo]` - built-in overrides custom defaults
- [x] Test: custom file that uses `@import` from its directory

### Phase 6b.6: Light/Dark Support (DEFERRED)
- [ ] Create `DualThemeConfig` struct
- [ ] Implement YAML parsing
- [ ] Support `dark_default` flag
(Deferred to future phase per user request)

---

## Error Handling Design

Errors should be structured to support Monaco editor diagnostics. Design:

```rust
/// SCSS compilation error with source location
#[derive(Debug)]
pub struct ScssError {
    /// Error message
    pub message: String,
    /// Source file path (if known)
    pub file: Option<PathBuf>,
    /// Line number (1-indexed, if known)
    pub line: Option<usize>,
    /// Column number (1-indexed, if known)
    pub column: Option<usize>,
    /// Error kind for categorization
    pub kind: ScssErrorKind,
}

#[derive(Debug)]
pub enum ScssErrorKind {
    /// File not found
    FileNotFound,
    /// File doesn't have layer boundaries
    NoBoundaryMarkers,
    /// Unknown theme name
    UnknownTheme,
    /// SASS compilation error
    CompilationFailed,
}
```

This structure can be serialized and sent to the hub-client for display in:
1. The error pane (existing QMD parse errors location)
2. Monaco editor squiggles (future enhancement)

For now, we'll use the error pane. The `file`, `line`, `column` fields enable future Monaco integration.

---

## Resolved Questions

1. **WASM support for custom files**: Pre-populate VFS with project files. *(User decision)*

2. **Load path handling**: Yes, custom theme directories are automatically added to load paths (matching TS Quarto).

3. **Error messages**: Issue errors (matching TS Quarto). Structure errors to support future Monaco editor diagnostics via file/line/column fields.

4. **Light/dark priority**: Deferred to future phase. *(User decision)*

---

## Testing Strategy

**Unit tests:**
- `ThemeSpec::parse()` - various inputs including edge cases
- `load_custom_theme()` - filesystem loading
- `process_theme_specs()` - customization injection logic
- Layer merging order verification

**Integration tests (order-sensitive):**
Create test fixtures that verify ordering behavior:

```scss
// test-fixtures/custom/override.scss
/*-- scss:defaults --*/
$test-var: "from-custom" !default;

/*-- scss:rules --*/
.test-rule { content: "custom"; }
```

Test scenarios:
1. `["cosmo"]` - baseline
2. `["override.scss"]` - custom only (customization at beginning)
3. `["cosmo", "override.scss"]` - custom defaults win
4. `["override.scss", "cosmo"]` - built-in defaults win (reversed!)
5. Verify CSS rule order matches expectation

**Ordering verification test:**
```rust
#[test]
fn test_ordering_builtin_then_custom() {
    // [cosmo, custom.scss]
    // Expect: custom.defaults → customize.defaults → cosmo.defaults
    // Expect: cosmo.rules → customize.rules → custom.rules
}

#[test]
fn test_ordering_custom_then_builtin() {
    // [custom.scss, cosmo]
    // Expect: customize.defaults → cosmo.defaults → custom.defaults
    // Expect: custom.rules → cosmo.rules → customize.rules
}
```

---

## Files to Create/Modify

**Modified files:**
- `crates/quarto-sass/src/themes.rs` - Add `ThemeSpec`, `ThemeContext`, `process_theme_specs()`
- `crates/quarto-sass/src/bundle.rs` - Add `assemble_with_user_layers()`, `compile_themes()`
- `crates/quarto-sass/src/error.rs` - Add `ScssError` with location info, new error kinds
- `crates/quarto-sass/src/lib.rs` - Export new types

**New test files:**
- `crates/quarto-sass/tests/custom_theme_test.rs` - Integration tests
- `crates/quarto-sass/test-fixtures/custom/override.scss` - Test fixture
- `crates/quarto-sass/test-fixtures/custom/with_import.scss` - Test @import from theme dir
- `crates/quarto-sass/test-fixtures/custom/_partials.scss` - Partial for @import test
