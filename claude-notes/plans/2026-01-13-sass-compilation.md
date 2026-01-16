# SASS Compilation Infrastructure for Rust Quarto

**Beads Issue**: k-685
**Created**: 2026-01-13
**Status**: Planning

**Revision History**:
- 2026-01-13: Initial plan created
- 2026-01-13: Added "Web API Strategy" section after discovering deno_web cannot be used
  due to upstream yanked dependency (fqdn). Chose polyfill approach instead. See that
  section for full technical analysis and ecosystem context.

## Executive Summary

Port the SASS bundle compilation system from TypeScript Quarto to Rust Quarto, supporting both native and WASM execution targets. This enables hub-client to render previews with custom SASS styling while maintaining bounded cache sizes in browser storage.

## Background

### TypeScript Quarto SASS Architecture

The TS Quarto SASS system is built on these key abstractions:

1. **SassLayer** - Smallest unit organizing SCSS by purpose:
   - `uses` - @use imports
   - `defaults` - SASS variable defaults
   - `functions` - SASS function definitions
   - `mixins` - SASS mixin definitions
   - `rules` - CSS/SASS rules

2. **SassBundleLayers** - Groups layers by audience:
   - `framework` - Bootstrap/Reveal.js SASS
   - `quarto` - Quarto's built-in SASS
   - `user` - User-provided customizations
   - `loadPaths` - @use/@import resolution paths

3. **SassBundle** - Adds metadata:
   - `key` - Unique identifier
   - `dependency` - Which framework (e.g., "bootstrap")
   - `dark` - Dark mode variant layers
   - `attribs` - HTML attributes for compiled CSS

4. **Brand System** - Centralizes theme configuration via `_brand.yml`

5. **Layer Boundaries** - Special comments organize SCSS:
   ```scss
   /*-- scss:uses --*/
   /*-- scss:defaults --*/
   /*-- scss:functions --*/
   /*-- scss:mixins --*/
   /*-- scss:rules --*/
   ```

### EJS Template Integration Pattern (Reference)

The EJS template system provides a model for cross-platform JavaScript functionality:

- **SystemRuntime trait** defines platform-agnostic methods (`render_ejs()`)
- **Native**: Uses deno_core/V8 with bundled JavaScript
- **WASM**: Uses wasm-bindgen to call browser JavaScript
- Conditional compilation separates implementations
- Default implementations return `NotSupported` error

### Current State in Rust Quarto

- `BinaryDependencies` already has `dart_sass` field for external SASS
- No SASS compilation logic implemented yet
- Hub-client uses IndexedDB for caching (projects, userSettings)
- WASM module already has VFS and rendering infrastructure

## Technical Design

### Compiler Strategy: dart-sass Everywhere via deno_core

**The Solution**: Use dart-sass (the reference implementation) on both native and WASM targets, following the proven pattern from `external-sources/rusty_v8_experiments/crates/sass-runner/`.

| Target | Compiler | Method | Behavior |
|--------|----------|--------|----------|
| **Native** | dart-sass | Via deno_core/V8 (embedded) | Reference implementation ✅ |
| **WASM** | dart-sass | Via browser JS bridge | Reference implementation ✅ |

**Why This Approach:**

1. **Exact behavioral parity** with TS Quarto - both use the same dart-sass JavaScript
2. **No equivalence testing needed** - single compiler everywhere
3. **Proven implementation** - already working in `rusty_v8_experiments/crates/sass-runner/`
4. **Follows EJS pattern** - same architecture as existing EJS template rendering

**Architecture:**

```
┌─────────────────────────────────────────────────────────────┐
│                     quarto-sass crate                        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  SassLayer, SassBundleLayers, SassBundle types      │    │
│  │  Layer parsing, merging, bundle assembly            │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  SystemRuntime trait                         │
│                                                              │
│  compile_sass(scss, options) -> RuntimeResult<String>       │
│  sass_available() -> bool                                   │
└─────────────────────────────────────────────────────────────┘
           │                                    │
           ▼                                    ▼
┌─────────────────────┐            ┌─────────────────────────┐
│   NativeRuntime     │            │     WasmRuntime         │
│                     │            │                         │
│  deno_core/V8 with  │            │  wasm-bindgen call to   │
│  sass.dart.js       │            │  browser JS (sass.dart.js)
│  bundle embedded    │            │  loaded in hub-client   │
└─────────────────────┘            └─────────────────────────┘
```

**The sass-runner Implementation (from rusty_v8_experiments):**

> **Note**: sass-runner uses `deno_web` for Web APIs, but we cannot use this approach due to
> an upstream yanked dependency issue (see "Web API Strategy" section). Our implementation
> uses minimal V8 with bundled polyfills instead.

```rust
// Reference only - sass-runner approach (uses deno_web which we cannot use)
// See: external-sources/rusty_v8_experiments/crates/sass-runner/src/main.rs
const SASS_BUNDLE: &str = include_str!("../js/sass-bundle.js");  // ~5.8MB

fn compile_scss(scss: &str, options: &CompileOptions) -> Result<String> {
    // sass-runner uses deno_web extensions - we use minimal V8 + polyfills instead
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![
            deno_webidl::deno_webidl::init(),  // NOT used in our implementation
            deno_web::deno_web::lazy_init(),    // NOT used in our implementation
            web_bootstrap::init(),
        ],
        ..Default::default()
    });

    // Load sass bundle (initializes globalThis.sass)
    runtime.execute_script("<sass-bundle>", SASS_BUNDLE.to_string())?;

    // Compile using sass.compileString()
    let escaped_source = serde_json::to_string(&scss)?;
    let script = format!(r#"
        (function() {{
            const result = globalThis.sass.compileString({escaped_source}, {{
                style: "{style}",
                logger: {{ warn: () => {{}}, debug: () => {{}} }},
                silenceDeprecations: ['global-builtin', 'color-functions', 'import']
            }});
            return {{ success: true, css: result.css }};
        }})()
    "#, escaped_source = escaped_source, style = options.style);

    let result = eval_to_json(&mut runtime, &script)?;
    Ok(result["css"].as_str().unwrap().to_string())
}
```

**WASM Implementation (follows EJS pattern):**

```rust
// WASM: dart-sass via browser JS bridge (similar to EJS in wasm.rs)
#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/sass.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn js_compile_sass_impl(
        scss: &str,
        style: &str,
    ) -> Result<js_sys::Promise, JsValue>;

    fn js_sass_available_impl() -> bool;
}

impl SystemRuntime for WasmRuntime {
    fn sass_available(&self) -> bool {
        js_sass_available_impl()
    }

    async fn compile_sass(&self, scss: &str, options: &SassCompileOptions) -> RuntimeResult<String> {
        let style = if options.minified { "compressed" } else { "expanded" };
        let promise = js_compile_sass_impl(scss, style)
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;
        let result = JsFuture::from(promise).await
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;
        Ok(result.as_string().unwrap())
    }
}
```

**Hub-client JS Bridge (new file: src/wasm-js-bridge/sass.js):**

```javascript
// Load sass.dart.js bundle (can be lazy-loaded)
import sass from '@aspect-build/sass';  // or load sass.dart.js directly

export function js_compile_sass_impl(scss, style) {
    return Promise.resolve().then(() => {
        const result = sass.compileString(scss, {
            style: style,
            logger: { warn: () => {}, debug: () => {} },
            silenceDeprecations: ['global-builtin', 'color-functions', 'import']
        });
        return result.css;
    });
}

export function js_sass_available_impl() {
    return typeof sass !== 'undefined' && typeof sass.compileString === 'function';
}
```

**Alternative: grass as Optional Fast Path**

While dart-sass everywhere gives us perfect parity, we may want grass as an optional optimization:

```rust
/// Compiler backend selection
pub enum SassBackend {
    DartSass,  // Default: reference implementation via deno_core/JS bridge
    Grass,     // Optional: pure Rust, ~2x faster, may have minor differences
}
```

This could be enabled via env var (`QUARTO_SASS_BACKEND=grass`) for users who want faster compilation and don't need exact parity.

### Milestone 1: Core Types and Infrastructure

#### 1.1 New Crate: `quarto-sass`

Location: `crates/quarto-sass/`

```rust
/// Single SASS layer with organized sections
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SassLayer {
    pub uses: String,
    pub defaults: String,
    pub functions: String,
    pub mixins: String,
    pub rules: String,
}

/// Bundle of layers organized by audience
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SassBundleLayers {
    pub key: String,
    pub framework: Option<SassLayer>,
    pub quarto: Option<SassLayer>,
    pub user: Vec<SassLayer>,
    pub load_paths: Vec<PathBuf>,
}

/// Complete bundle with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SassBundle {
    #[serde(flatten)]
    pub layers: SassBundleLayers,
    pub dependency: String,
    pub dark: Option<SassBundleDark>,
    pub attribs: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SassBundleDark {
    pub framework: Option<SassLayer>,
    pub quarto: Option<SassLayer>,
    pub user: Vec<SassLayer>,
    pub default: bool,
}
```

#### 1.2 JavaScript Bundle Management

The sass.dart.js bundle needs to be prepared (following sass-runner pattern):

```javascript
// js/sass-bundle.js (~5.8MB bundled)
// Created by bundling: immutable.js + sass.dart.js
// See: external-sources/rusty_v8_experiments/crates/sass-runner/js/

(function() {
    // immutable.js (required dependency)
    // ... immutable library code ...

    // sass.dart.js (dart-sass compiled to JavaScript)
    // ... sass library code ...

    // Export to globalThis
    globalThis.sass = sass;
})();
```

Build process (from rusty_v8_experiments):
```bash
# 1. Get sass.dart.js from npm
npm install sass

# 2. Bundle with immutable
# sass.dart.js + immutable.js → sass-bundle.js
```

#### 1.3 Layer Parsing

```rust
/// Parse layer boundaries from SCSS content
pub fn parse_layer(content: &str) -> SassLayer {
    // Parse /*-- scss:uses --*/ etc. boundaries
}

/// Parse layer from file (single file with boundaries)
pub fn layer_from_file(path: &Path) -> Result<SassLayer, SassError>;

/// Parse layer from directory (individual files)
pub fn layer_from_dir(path: &Path) -> Result<SassLayer, SassError>;
```

#### 1.3 Layer Merging

```rust
/// Merge multiple layers into one
/// Note: defaults are reversed (first = highest priority)
pub fn merge_layers(layers: &[SassLayer]) -> SassLayer {
    SassLayer {
        uses: layers.iter().map(|l| &l.uses).join("\n"),
        defaults: layers.iter().rev().map(|l| &l.defaults).join("\n"),
        functions: layers.iter().map(|l| &l.functions).join("\n"),
        mixins: layers.iter().map(|l| &l.mixins).join("\n"),
        rules: layers.iter().map(|l| &l.rules).join("\n"),
    }
}
```

#### 1.4 Compilation

```rust
/// Compile a bundle to CSS
pub fn compile_bundle(bundle: &SassBundleLayers, options: &CompileOptions) -> Result<String, SassError>;

/// Compile raw SCSS to CSS
pub fn compile_scss(scss: &str, options: &CompileOptions) -> Result<String, SassError>;

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub minified: bool,
    pub load_paths: Vec<PathBuf>,
    pub source_map: bool,
}
```

### Milestone 2: SystemRuntime Integration

#### 2.1 Trait Methods

Add to `SystemRuntime` trait in `quarto-system-runtime`:

```rust
/// Check if SASS compilation is available
fn sass_available(&self) -> bool {
    false  // Default: not available
}

/// Get the SASS compiler backend name (for diagnostics)
fn sass_compiler_name(&self) -> Option<&'static str> {
    None
}

/// Compile SCSS to CSS
async fn compile_sass(
    &self,
    scss: &str,
    options: &SassCompileOptions,
) -> RuntimeResult<String> {
    Err(RuntimeError::NotSupported("SASS compilation not available".into()))
}

/// Compile a SASS bundle to CSS
async fn compile_sass_bundle(
    &self,
    bundle: &SassBundleLayers,
    options: &SassCompileOptions,
) -> RuntimeResult<String> {
    Err(RuntimeError::NotSupported("SASS bundle compilation not available".into()))
}
```

#### 2.2 Native Implementation (dart-sass via deno_core)

Following the pattern from EJS integration in `js_native.rs`, using minimal V8 setup with
polyfills bundled in the JavaScript (see "Web API Strategy" section for rationale):

```rust
// In native.rs (or a dedicated js_sass.rs module)
use deno_core::{JsRuntime, RuntimeOptions};

// sass-bundle.js includes: polyfills + immutable.js + sass.dart.js
const SASS_BUNDLE: &str = include_str!("../js/sass-bundle.js");

impl SystemRuntime for NativeRuntime {
    fn sass_available(&self) -> bool {
        true  // dart-sass bundle is always embedded
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        Some("dart-sass")
    }

    async fn compile_sass(&self, scss: &str, options: &SassCompileOptions) -> RuntimeResult<String> {
        // Create minimal V8 runtime (no deno_web extensions needed -
        // required Web APIs are polyfilled in sass-bundle.js)
        let mut runtime = JsRuntime::new(RuntimeOptions::default());

        // Load sass bundle (includes polyfills, initializes globalThis.sass)
        runtime.execute_script("<sass-bundle>", SASS_BUNDLE.to_string())
            .map_err(|e| RuntimeError::NotSupported(format!("Failed to load sass: {}", e)))?;

        // Compile SCSS
        let escaped_source = serde_json::to_string(&scss)
            .map_err(|e| RuntimeError::NotSupported(e.to_string()))?;
        let style = if options.minified { "compressed" } else { "expanded" };

        let script = format!(r#"
            (function() {{
                try {{
                    const result = globalThis.sass.compileString({escaped_source}, {{
                        style: "{style}",
                        logger: {{ warn: () => {{}}, debug: () => {{}} }},
                        silenceDeprecations: ['global-builtin', 'color-functions', 'import']
                    }});
                    return {{ success: true, css: result.css }};
                }} catch (e) {{
                    return {{ success: false, error: e.toString() }};
                }}
            }})()
        "#);

        let result = eval_to_json(&mut runtime, &script)
            .map_err(|e| RuntimeError::NotSupported(e.to_string()))?;

        if result["success"].as_bool() == Some(true) {
            Ok(result["css"].as_str().unwrap().to_string())
        } else {
            let error = result["error"].as_str().unwrap_or("Unknown error");
            Err(RuntimeError::NotSupported(format!("SASS compilation failed: {}", error)))
        }
    }
}
```

**Note**: This uses `RuntimeError::NotSupported` for errors. Consider adding a dedicated
`RuntimeError::SassError(String)` variant for cleaner error handling.

#### 2.3 WASM Implementation (dart-sass via browser JS bridge)

Following the EJS pattern in `wasm.rs`:

```rust
// In wasm.rs
#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/sass.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn js_compile_sass_impl(
        scss: &str,
        style: &str,
    ) -> Result<js_sys::Promise, JsValue>;

    fn js_sass_available_impl() -> bool;
}

impl SystemRuntime for WasmRuntime {
    fn sass_available(&self) -> bool {
        js_sass_available_impl()
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        Some("dart-sass-browser")
    }

    async fn compile_sass(&self, scss: &str, options: &SassCompileOptions) -> RuntimeResult<String> {
        let style = if options.minified { "compressed" } else { "expanded" };

        let promise = js_compile_sass_impl(scss, style)
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;

        let result = JsFuture::from(promise).await
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;

        result.as_string()
            .ok_or_else(|| RuntimeError::SassError("Expected string result".to_string()))
    }
}
```

#### 2.4 Hub-Client JavaScript Bridge

New file: `hub-client/src/wasm-js-bridge/sass.js`

```javascript
// Lazy-load sass to avoid blocking initial load
let sassModule = null;
let sassLoadPromise = null;

async function loadSass() {
    if (sassModule) return sassModule;
    if (sassLoadPromise) return sassLoadPromise;

    sassLoadPromise = import('sass').then(module => {
        sassModule = module.default || module;
        return sassModule;
    });

    return sassLoadPromise;
}

export async function js_compile_sass_impl(scss, style) {
    const sass = await loadSass();

    const result = sass.compileString(scss, {
        style: style,
        logger: { warn: () => {}, debug: () => {} },
        silenceDeprecations: ['global-builtin', 'color-functions', 'import']
    });

    return result.css;
}

export function js_sass_available_impl() {
    // Return true - we can always try to load sass
    // Actual availability checked when compiling
    return true;
}
```

### Phase 3: Hub-Client Caching

#### 3.1 IndexedDB Cache Schema

Add new store to `hub-client/src/services/storage/`:

```typescript
interface SassCache {
  // Key: SHA-256 hash of (scss_content + options_hash)
  key: string;

  // Compiled CSS
  css: string;

  // Metadata for cache management
  created: number;      // Unix timestamp
  lastUsed: number;     // For LRU eviction
  size: number;         // CSS size in bytes

  // Debugging
  sourceHash: string;   // Original SCSS hash
}
```

#### 3.2 Cache Size Management

```typescript
interface SassCacheConfig {
  maxSizeBytes: number;      // Default: 50MB
  maxEntries: number;        // Default: 1000
  evictionPolicy: 'lru' | 'fifo';
}

class SassCacheManager {
  async get(key: string): Promise<string | null>;
  async set(key: string, css: string): Promise<void>;
  async prune(): Promise<void>;  // Evict entries to stay under limits
  async clear(): Promise<void>;
  async getStats(): Promise<CacheStats>;
}
```

#### 3.3 Integration with Rendering

```typescript
// In wasmRenderer.ts
async function compileScss(
  scss: string,
  options: SassOptions
): Promise<string> {
  const cacheKey = await computeCacheKey(scss, options);

  // Check cache first
  const cached = await sassCache.get(cacheKey);
  if (cached) {
    await sassCache.touch(cacheKey);  // Update lastUsed
    return cached;
  }

  // Compile via WASM
  const css = await wasm.compile_sass(scss, options);

  // Cache result
  await sassCache.set(cacheKey, css);

  return css;
}
```

### Milestone 4: Bootstrap Integration

Full Bootstrap theming support (required for Quarto HTML output):

#### 4.1 Bootstrap SASS Assets

```rust
/// Embedded Bootstrap SASS files
pub struct BootstrapAssets {
    version: &'static str,  // e.g., "5.3.3"
    scss_files: HashMap<&'static str, &'static str>,  // path -> content
}

impl BootstrapAssets {
    /// Get Bootstrap core files (functions, variables, mixins, utilities)
    pub fn core() -> Self;

    /// Get a specific Bootstrap component
    pub fn component(&self, name: &str) -> Option<&str>;
}
```

#### 4.2 Theme Resolution

Port TS Quarto's theme resolution logic:

```rust
/// Built-in Bootswatch themes
pub enum BuiltInTheme {
    Cosmo, Darkly, Flatly, Journal, Litera, Lumen, Lux,
    Materia, Minty, Morph, Pulse, Quartz, Sandstone,
    Simplex, Sketchy, Slate, Solar, Spacelab, Superhero,
    United, Vapor, Yeti, Zephyr,
}

/// Resolve theme specification to SASS layers
pub fn resolve_theme(
    theme: &ThemeSpec,
    quarto_themes_dir: &Path,
) -> Result<ThemeLayers, SassError>;

/// Theme specification (matches TS Quarto's format)
pub enum ThemeSpec {
    BuiltIn(BuiltInTheme),
    Custom(PathBuf),
    LightDark { light: Box<ThemeSpec>, dark: Box<ThemeSpec> },
}
```

#### 4.3 Quarto Layer Assembly

Port `layerQuartoScss` function:

```rust
/// Assemble Quarto's SASS layers for HTML output
pub fn layer_quarto_scss(
    key: &str,
    dependency: &str,
    user_layers: Vec<SassLayer>,
    format: &Format,
    dark_layers: Option<Vec<SassLayer>>,
) -> SassBundleWithBrand {
    // 1. Bootstrap framework layer (functions, variables, mixins, rules)
    // 2. Quarto defaults layer
    // 3. User customization layers
    // 4. Quarto rules layer
}
```

#### 4.4 Pandoc Variable Mapping

```rust
/// Convert Pandoc metadata to Bootstrap SASS variables
pub fn pandoc_vars_to_scss(metadata: &Metadata) -> String {
    // Map: fontsize → $font-size-base
    //      fontcolor → $body-color
    //      linkcolor → $link-color
    //      linestretch → $line-height-base
    //      mainfont → $font-family-base
    //      monofont → $font-family-monospace
    //      backgroundcolor → $body-bg
    //      etc.
}
```

### Milestone 5: Brand System

Centralized theme configuration via `_brand.yml`:

#### 5.1 Brand Configuration Parsing

```rust
/// Parsed _brand.yml configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brand {
    pub color: Option<BrandColor>,
    pub typography: Option<BrandTypography>,
    pub defaults: Option<BrandDefaults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandColor {
    pub palette: HashMap<String, String>,  // Named colors
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    // ... theme color mappings
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandTypography {
    pub fonts: Vec<FontSpec>,
    pub base: Option<TypographyBase>,
    pub headings: Option<TypographyHeadings>,
}
```

#### 5.2 Brand-to-SASS Conversion

```rust
/// Generate SASS layers from brand configuration
pub fn brand_to_sass_layers(brand: &Brand) -> BrandSassLayers {
    BrandSassLayers {
        colors: brand_color_layer(&brand.color),
        typography: brand_typography_layer(&brand.typography),
        defaults: brand_defaults_layer(&brand.defaults),
    }
}

/// Generate CSS custom properties for brand colors
fn brand_color_layer(color: &Option<BrandColor>) -> SassLayer {
    // Generate:
    // :root {
    //   --brand-primary: #{$primary};
    //   --brand-secondary: #{$secondary};
    //   ...
    // }
}
```

#### 5.3 Light/Dark Mode Support

```rust
/// Compile bundle with light and dark variants
pub fn compile_themed_bundle(
    bundle: &SassBundle,
    compiler: &dyn SassCompiler,
    options: &CompileOptions,
) -> Result<ThemedCss, SassError> {
    let light_css = compile_bundle_light(bundle, compiler, options)?;
    let dark_css = bundle.dark.as_ref()
        .map(|dark| compile_bundle_dark(bundle, dark, compiler, options))
        .transpose()?;

    Ok(ThemedCss {
        light: light_css,
        dark: dark_css,
        dark_default: bundle.dark.as_ref().map(|d| d.default).unwrap_or(false),
    })
}
```

### Milestone 6: TS Quarto Parity Validation

Ensure Rust Quarto produces identical output to TS Quarto:

#### 6.1 Comparison Test Harness

```rust
/// Compare Rust Quarto output with TS Quarto output
pub struct ParityTest {
    pub input_scss: String,
    pub rust_output: String,
    pub ts_output: String,  // Generated by running TS Quarto
    pub matches: bool,
}

/// Run parity validation against TS Quarto fixtures
pub fn validate_parity(test_cases: &[ParityTest]) -> ParityReport;
```

#### 6.2 Test Cases

- Bootstrap v5.3 full compilation
- All 24 Bootswatch themes
- Layer boundary handling edge cases
- Variable precedence (defaults ordering)
- Quarto-specific SCSS files
- Brand configuration examples

These tests use pre-generated fixtures from TS Quarto to ensure behavioral parity.

## Implementation Tasks

### Phase 1: Core Types and Infrastructure

- [ ] Create `quarto-sass` crate with types (`SassLayer`, `SassBundleLayers`, `SassBundle`)
- [ ] Implement `SassLayer` parsing (boundary comments regex)
- [ ] Implement layer merging with correct precedence (defaults reversed)
- [ ] Write unit tests for layer parsing/merging
- [ ] Port sass-bundle.js from `rusty_v8_experiments/crates/sass-runner/js/`

### Phase 2: Native Runtime (dart-sass via deno_core)

- [ ] Add SASS methods to `SystemRuntime` trait
- [ ] Create `sass_bootstrap` extension (following EJS pattern)
- [ ] Implement `NativeRuntime::compile_sass()` with embedded dart-sass
- [ ] Implement `NativeRuntime::compile_sass_bundle()`
- [ ] Basic integration test: compile simple SCSS
- [ ] Test compilation matches TS Quarto output

### Phase 3: WASM Runtime (dart-sass via browser JS)

- [ ] Create `hub-client/src/wasm-js-bridge/sass.js` bridge
- [ ] Add `sass` npm dependency to hub-client
- [ ] Implement `WasmRuntime::compile_sass()` with JS bridge
- [ ] Implement `WasmRuntime::compile_sass_bundle()`
- [ ] Test WASM compilation matches native output
- [ ] Verify lazy loading doesn't block initial hub-client load

### Phase 4: Hub-Client Caching

- [ ] Add `sassCache` store to IndexedDB schema
- [ ] Create migration for new schema version
- [ ] Implement `SassCacheManager` with LRU eviction
- [ ] Add cache size configuration (default: 50MB)
- [ ] Integrate with rendering pipeline
- [ ] Test cache hit/miss scenarios
- [ ] Test eviction under size pressure

### Phase 5: Bootstrap Integration

- [ ] Bundle Bootstrap SASS files in crate (or reference from VFS)
- [ ] Implement `BuiltInTheme` enum (24 Bootswatch themes)
- [ ] Implement `resolve_theme()` function
- [ ] Port `layerQuartoScss` assembly function
- [ ] Port `pandocVariablesToThemeScss` mapping
- [ ] Test all 24 Bootswatch themes compile correctly
- [ ] Integration test with Quarto HTML format

### Phase 6: Brand System

- [ ] Implement `Brand` configuration parsing (`_brand.yml`)
- [ ] Implement `brand_color_layer()` - color palette to SASS
- [ ] Implement `brand_typography_layer()` - fonts and typography
- [ ] Implement `brand_defaults_layer()` - Bootstrap defaults
- [ ] Light/dark mode variant generation
- [ ] CSS custom properties (--brand-* variables)
- [ ] Integration test with `_brand.yml` examples

### Phase 7: End-to-End Testing

- [ ] Full pipeline test: QMD → HTML with custom SASS
- [ ] Hub-client preview with themed styling
- [ ] Performance benchmarking (with and without cache)
- [ ] Cache persistence across sessions
- [ ] Cross-browser testing (Chrome, Firefox, Safari)
- [ ] Compare output with TS Quarto for parity validation

## Critical Finding: Import Resolution

### How TS Quarto Handles Imports

TS Quarto does **NOT** use JavaScript's `compileString()` - it calls the **dart-sass binary** directly:

```bash
sass input.scss output.css --load-path=/bootstrap --load-path=/bslib
```

The approach:
1. Concatenate all SCSS layers into one string (maintaining careful ordering)
2. Write to temp file
3. Call dart-sass with `--load-path` for each resource directory
4. Let dart-sass resolve all `@use`/`@import` natively

### Our Approach: `compileString()` with Import Support

**Good news**: `compileString()` DOES support `loadPaths` and custom `importers`!

**Native (deno_core)**: Pass `loadPaths` option directly:

```javascript
sass.compileString(source, {
    loadPaths: [bootstrapDir, bslibDir, userThemeDir],
    style: 'compressed',
    silenceDeprecations: ['global-builtin', 'color-functions', 'import']
});
```

For embedded Bootstrap files, we can write them to a temp directory and use that as a load path.

**WASM (browser)**: Implement a custom importer that reads from VFS:

```javascript
sass.compileString(source, {
    importers: [{
        canonicalize(url, context) {
            // Resolve relative imports from containing URL
            if (context.containingUrl) {
                return new URL(url, context.containingUrl);
            }
            // Map known prefixes to VFS paths
            if (url.startsWith('bootstrap/')) {
                return new URL('vfs:///' + url);
            }
            return null;
        },
        load(canonicalUrl) {
            const path = canonicalUrl.pathname;
            const content = vfsReadFile(path);
            return {
                contents: content,
                syntax: path.endsWith('.sass') ? 'indented' : 'scss'
            };
        }
    }],
    style: 'compressed'
});
```

This custom importer:
1. Handles relative imports by resolving against the containing file's URL
2. Maps `@use "bootstrap/..."` to VFS paths
3. Reads file content from hub-client's Virtual File System

### Bootstrap Files in WASM

For WASM, Bootstrap SCSS files must be available in the VFS. Options:
1. **Pre-load on startup**: Load Bootstrap SCSS into VFS when hub-client initializes
2. **Lazy-load**: Fetch Bootstrap files on first SASS compilation
3. **Embed in WASM**: Include Bootstrap SCSS as static data in the WASM module

Recommended: **Embed in WASM** for offline support and simplicity.

## Open Questions

1. **Bootstrap Bundle Size**: Full Bootstrap SASS is ~200KB. How to handle?
   - **Native**: Write embedded files to temp directory, use as load path
   - **WASM**: Embed in WASM module, serve via custom importer
   - **Decision**: Embed in both for simplicity and offline support ✓

2. **sass.dart.js Size**: The sass bundle is ~5.8MB
   - **Native**: Acceptable - embedded in binary
   - **WASM**: Lazy-load sass bundle in browser; compile on first use ✓

3. **Cache Size Default**: 50MB
   - Holds ~100+ compiled stylesheets
   - Well under typical browser storage quotas ✓

4. **Dark Mode CSS**: Generate separate files (matches TS Quarto behavior)
   - `{key}.min.css` for light mode
   - `{key}-dark.min.css` for dark mode ✓

## Web API Strategy: Polyfills vs deno_web

### Background: Why Not deno_web?

The `sass-runner` experiment uses `deno_web` to provide Web APIs (URL, TextEncoder, etc.) that dart-sass requires. However, **deno_web currently has an unresolvable dependency issue**:

```
deno_web v0.257.0
  → deno_permissions v0.85.0
    → fqdn = "^0.4.6"
```

The `fqdn` crate author has **yanked** versions 0.4.3 through 0.4.7. "Yanking" is a crates.io mechanism where an author marks a version as "do not use for new projects" - typically due to security issues or serious bugs. Cargo will not select yanked versions when resolving dependencies for new projects (though existing `Cargo.lock` files continue to work).

This creates an impossible constraint:
- `deno_permissions` requires `>=0.4.6, <0.5.0`
- Every version in that range is yanked
- `fqdn 0.4.2` (not yanked) doesn't satisfy `>=0.4.6`
- `fqdn 0.5.2` (not yanked) doesn't satisfy `<0.5.0`

Deno's own builds work because their `Cargo.lock` predates the yanking. New projects (like ours) cannot add `deno_web` as a dependency until Deno updates their `fqdn` version constraint.

### Chosen Approach: Bundle Polyfills in sass-bundle.js

Instead of waiting for upstream fixes or maintaining complex patches, we bundle the required Web APIs directly in the JavaScript bundle. This follows the same pattern as the working EJS integration (minimal V8 + bundled JS).

**APIs dart-sass needs** (based on sass-runner's `web_bootstrap` extension):
- `URL`, `URLSearchParams` - path resolution
- `TextEncoder`, `TextDecoder` - string encoding
- `atob`, `btoa` - base64 (internal use)
- `location.href` - dart-sass uses `Uri.base`

**Implementation**: Add a polyfill preamble to `sass-bundle.js`:

```javascript
// Polyfills for bare V8 environment
(function() {
    // location shim (dart-sass Uri.base)
    if (typeof location === 'undefined') {
        globalThis.location = { href: 'file:///' };
    }

    // atob/btoa (not in bare V8)
    if (typeof atob === 'undefined') {
        globalThis.atob = function(s) { /* base64 decode */ };
        globalThis.btoa = function(s) { /* base64 encode */ };
    }

    // URL/TextEncoder - check if V8 provides them, add polyfills if not
    // (V8 may have these; verify empirically)
})();

// Then: immutable.js + sass.dart.js
```

**Benefits**:
- Self-contained: no external Rust dependencies beyond deno_core
- Consistent: same pattern as EJS integration
- Testable: can verify the bundle works in Node.js independently
- Avoids upstream issues: no dependency on deno_web fix timeline

**Size impact**: ~62KB of polyfills added to 5.8MB bundle = 1% increase. Negligible.

### Broader Context: Yanking in the Rust Ecosystem

**What is yanking?** When a crate author "yanks" a version on crates.io, they mark it as
"do not use for new projects." Cargo will not select yanked versions when resolving
dependencies for new projects, but existing `Cargo.lock` files continue to work. This is
a soft removal—the code isn't deleted, just discouraged.

**How common is this?** Yanking itself is common but usually benign. Authors typically yank for:
- Security vulnerabilities
- Accidentally published broken code
- Accidental credential/secret leaks

Most yanks affect only one version, with a newer non-yanked version available. The fqdn
situation is a **rare pathological case**: all versions in a semver range were yanked,
creating an impossible constraint for downstream crates that haven't updated yet.

**Why this case is unusual:**
1. The fqdn author yanked an entire range (0.4.3–0.4.7) plus some 0.5.x versions
2. A "semver gap" was left—0.4.2 and 0.5.2 exist but don't satisfy `^0.4.6`
3. A high-profile project (Deno) depends on the broken range
4. Deno's crates.io releases lag behind their internal monorepo fixes

**Note:** The `sass-runner` experiment in `external-sources/rusty_v8_experiments` also
cannot be built today—it fails with the same fqdn error. The yanking happened after those
experiments were created, and without a preserved `Cargo.lock`, fresh builds fail.

**Comparison to other ecosystems:**

| Ecosystem | Mechanism | Behavior |
|-----------|-----------|----------|
| Rust (crates.io) | `cargo yank` | Soft removal; existing lockfiles still work |
| npm (JavaScript) | `npm deprecate` / `npm unpublish` | Deprecation is warning; unpublish is hard delete (restricted after left-pad incident) |
| PyPI (Python) | `yank` | Similar to Rust—soft removal |
| Maven (Java) | None | Published artifacts are essentially permanent |

The npm "left-pad incident" (2016) was worse—an author deleted a package entirely,
breaking thousands of builds instantly. Rust's yanking is more conservative.

**Long-term risk assessment: Low**
- This is an unusual case; most yanks don't create impossible constraints
- The mitigation (commit `Cargo.lock` for applications) is standard practice
- Deno's crates are somewhat "second-class citizens" on crates.io—their primary use case
  is the monorepo with a committed lockfile, so external consumers can hit issues like this

**Why polyfills are a good long-term choice (not just a workaround):**
- Independence from Deno's crates.io release cadence
- Simpler Rust code (no extension initialization)
- Consistent with our EJS integration pattern
- If Deno fixes this, we *could* migrate back, but there's no compelling reason to

## Dependencies

### New Crate Dependencies

```toml
# crates/quarto-sass/Cargo.toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
regex = "1.0"  # For layer boundary parsing

# Native only (dart-sass via deno_core)
# Note: We use minimal V8 setup (no deno_web) due to upstream dependency issues.
# Required Web APIs are bundled as polyfills in sass-bundle.js instead.
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
deno_core = "0.376"
serde_v8 = "0.285"
```

### JavaScript Bundle

- `sass-bundle.js` - custom bundle built from:
  - Web API polyfills (URL, TextEncoder, atob/btoa, location shim)
  - `immutable.js` (required by dart-sass)
  - `sass.dart.js` (dart-sass compiled to JavaScript)
- Total size: ~5.9MB (5.8MB dart-sass + ~62KB polyfills)
- No external binary needed - fully self-contained
- Reference: `external-sources/rusty_v8_experiments/crates/sass-runner/js/`

### Hub-Client Dependencies

```json
// package.json additions
{
  "dependencies": {
    "sass": "^1.77.0"  // dart-sass JavaScript API
  }
}
```

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| sass bundle size (5.8MB) impacts load time | Medium | Medium | Lazy-load in browser; cache compiled CSS |
| deno_core version conflicts | Low | High | Pin versions; test with quarto-system-runtime |
| dart-sass deprecation warnings | Low | Low | Use `silenceDeprecations` option |
| Bootstrap version drift | Low | Medium | Pin Bootstrap version; document upgrade process |
| IndexedDB quota exceeded | Low | Low | LRU eviction; user-configurable limits |
| Layer boundary parsing edge cases | Medium | Medium | Port TS Quarto's regex exactly; comprehensive tests |
| Polyfill incompatibility with dart-sass | Low | Medium | Test empirically; polyfills are standard implementations |
| deno_web becomes usable (fqdn fixed) | Low | Low | Can migrate later if desired; polyfill approach still works |

## Success Criteria

### Phase 1-2 (Core + Native)
1. Can compile simple SCSS to CSS using dart-sass via deno_core
2. Layer parsing handles all TS Quarto boundary formats
3. Layer merging produces correct precedence
4. Output matches TS Quarto for same input

### Phase 3 (WASM)
1. WASM compilation produces identical output to native
2. Lazy loading works without blocking hub-client startup
3. Compilation completes in < 2 seconds for Bootstrap

### Phase 4+ (Full Feature)
1. Can compile Bootstrap SASS to CSS in both native and WASM
2. Hub-client preview shows custom SASS styling correctly
3. Compilation cached with < 5ms cache hit latency
4. Cache stays within configured size limits
5. All 24 Bootswatch themes compile correctly

## Timeline Considerations

This is a substantial feature. Recommended phase ordering:

1. **Phase 1** (Core types): Foundation for everything else
2. **Phase 2** (Native runtime): Validates architecture
3. **Phase 5** (Bootstrap): Needed before real styling works
4. **Phase 3** (WASM runtime): Enables hub-client integration
5. **Phase 4** (Caching): Performance optimization
6. **Phase 6** (Brand): Advanced theming
7. **Phase 7** (E2E): Polish and validation

Phases 1 and 5 can be worked in parallel.

## References

### Internal
- dart-sass via deno_core: `external-sources/rusty_v8_experiments/crates/sass-runner/`
- EJS integration pattern: `crates/quarto-system-runtime/src/traits.rs`
- Hub-client JS bridge pattern: `hub-client/src/wasm-js-bridge/template.js`

### External (TS Quarto)
- SASS compilation: `external-sources/quarto-cli/src/core/sass.ts`
- Type definitions: `external-sources/quarto-cli/src/config/types.ts`
- dart-sass integration: `external-sources/quarto-cli/src/core/dart-sass.ts`
- Bootstrap theming: `external-sources/quarto-cli/src/format/html/format-html-scss.ts`
- Brand system: `external-sources/quarto-cli/src/core/sass/brand.ts`

### Libraries
- [dart-sass npm](https://www.npmjs.com/package/sass) - JavaScript API
- [deno_core](https://crates.io/crates/deno_core) - V8 runtime for Rust
- [grass](https://crates.io/crates/grass) - Optional pure-Rust alternative (future)
