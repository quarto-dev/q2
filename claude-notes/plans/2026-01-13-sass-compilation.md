# SASS Compilation Infrastructure for Rust Quarto

**Beads Issue**: k-685
**Created**: 2026-01-13
**Status**: In Progress (Phase 2a complete)

**Revision History**:
- 2026-01-13: Initial plan created
- 2026-01-13: Added "Web API Strategy" section after discovering deno_web cannot be used
  due to upstream yanked dependency (fqdn). Chose polyfill approach instead.
- 2026-01-23: **Major revision**: Switched native implementation from deno_core/dart-sass
  to the `grass` crate (pure Rust). WASM still uses dart-sass via JavaScript bridge.
  Added Bootstrap 5.3.1 as target version. Added VFS resource embedding strategy.
  Removed deno_web/polyfill sections (no longer needed).
- 2026-01-23: **Implementation session**: Completed Phase 1 (core types) and Phase 2a
  (native runtime with grass). Bootstrap 5.3.1 compiles successfully.

## Session Summary (2026-01-23)

### Completed Work

**Phase 1: Core Types and Infrastructure** - COMPLETE
- Created `crates/quarto-sass/` with:
  - `SassLayer`, `SassBundleLayers`, `SassBundle`, `SassBundleDark` types in `types.rs`
  - Layer parsing with exact regex from TS Quarto in `layer.rs`
  - Layer merging with correct precedence (defaults reversed)
  - `SassError` error type in `error.rs`
- 15 unit tests covering parsing and merging edge cases

**Phase 2a: Native Runtime (grass)** - COMPLETE
- Added `grass = "0.13.4"` to workspace dependencies
- Extended `SystemRuntime` trait in `quarto-system-runtime`:
  - `sass_available() -> bool`
  - `sass_compiler_name() -> Option<&'static str>`
  - `compile_sass(scss, load_paths, minified) -> RuntimeResult<String>`
- Added `RuntimeError::SassError(String)` variant
- Created `sass_native.rs` module with:
  - `RuntimeFs` adapter implementing `grass::Fs` for `SystemRuntime`
  - `compile_scss()` function
- Implemented SASS methods in `NativeRuntime`
- Tests for Bootstrap 5.3.1 compilation (both expanded and minified)

### Key Findings

1. **Bootstrap Layer Assembly**: Bootstrap in TS Quarto is NOT compiled directly from
   `bootstrap.scss`. Instead, it's assembled from separate layer files in order:
   - Functions (`_functions.scss`)
   - Variables (`_variables.scss`)
   - Mixins (`_mixins.scss`)
   - Rules (`bootstrap.scss` - which imports component rules)

2. **Bootstrap Compilation Results**:
   - Expanded: ~235KB CSS
   - Minified: ~200KB CSS, <100 newlines
   - All expected classes present (.btn, .container, .navbar, .modal, etc.)

3. **grass Compatibility**: grass (targeting dart-sass 1.54.3) successfully compiles
   Bootstrap 5.3.1 with full accuracy.

### Files Created/Modified

**New files:**
- `crates/quarto-sass/Cargo.toml`
- `crates/quarto-sass/src/lib.rs`
- `crates/quarto-sass/src/error.rs`
- `crates/quarto-sass/src/types.rs`
- `crates/quarto-sass/src/layer.rs`
- `crates/quarto-system-runtime/src/sass_native.rs`

**Modified files:**
- `Cargo.toml` (workspace deps: grass, regex, quarto-sass)
- `crates/quarto-system-runtime/Cargo.toml` (added grass dep)
- `crates/quarto-system-runtime/src/lib.rs` (added sass_native module)
- `crates/quarto-system-runtime/src/traits.rs` (added SASS methods, SassError)
- `crates/quarto-system-runtime/src/native.rs` (implemented SASS methods)

### Next Steps

1. **Phase 2b: Parity Testing** - Compare grass output to dart-sass for Bootstrap
2. **Phase 3: WASM Runtime** - Implement dart-sass via JS bridge with lazy loading
3. **Phase 4: VFS Resource Embedding** - Embed Bootstrap SCSS for offline compilation

## Executive Summary

Port the SASS bundle compilation system from TypeScript Quarto to Rust Quarto, supporting both native and WASM execution targets. This enables hub-client to render previews with custom SASS styling while maintaining bounded cache sizes in browser storage.

**Key architectural decisions:**
- **Native**: Use the `grass` crate (pure Rust, ~2x faster than dart-sass)
- **WASM**: Use dart-sass via JavaScript bridge (lazy-loaded, ~5MB)
- **Bootstrap**: Target version 5.3.1 (matches TS Quarto)
- **VFS Resources**: Embed Bootstrap/Quarto SCSS via `include_dir!`-like mechanism
- **Source maps**: Not initially required

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

### Bootstrap Version

TS Quarto ships with **Bootstrap 5.3.1** (found in `external-sources/quarto-cli/configuration`).
We will target the same version for compatibility.

### Current State in Rust Quarto

- `BinaryDependencies` already has `dart_sass` field for external SASS
- No SASS compilation logic implemented yet
- Hub-client uses IndexedDB for caching (projects, userSettings)
- WASM module already has VFS and rendering infrastructure
- `SystemRuntime` trait provides file operations abstracted across native/WASM

## Technical Design

### Compiler Strategy: grass (Native) + dart-sass (WASM)

| Target | Compiler | Method | Notes |
|--------|----------|--------|-------|
| **Native** | grass | Pure Rust crate | ~2x faster, no JS engine needed |
| **WASM** | dart-sass | Browser JS bridge | Lazy-loaded, standard npm package |

**Why This Approach:**

1. **Native with grass**:
   - Pure Rust - no JavaScript engine dependency
   - ~2x faster than dart-sass ([grass benchmarks](https://github.com/connorskees/grass))
   - Bootstrap 5 compilation verified by CI (byte-for-byte accuracy)
   - Has `Fs` trait that maps perfectly to our `SystemRuntime` abstractions
   - Avoids deno_core/deno_web dependency issues

2. **WASM with dart-sass**:
   - Reference implementation ensures exact parity with TS Quarto
   - Already available as npm package
   - Lazy-loading prevents blocking hub-client startup
   - Custom importer can read from VFS

**Architecture:**

```
┌─────────────────────────────────────────────────────────────────┐
│                     quarto-sass crate                            │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  SassLayer, SassBundleLayers, SassBundle types          │    │
│  │  Layer parsing, merging, bundle assembly                │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  SystemRuntime trait                             │
│                                                                  │
│  compile_sass(scss, options) -> RuntimeResult<String>           │
│  sass_available() -> bool                                        │
└─────────────────────────────────────────────────────────────────┘
           │                                    │
           ▼                                    ▼
┌─────────────────────────┐        ┌─────────────────────────────┐
│   NativeRuntime         │        │     WasmRuntime             │
│                         │        │                             │
│  grass crate with       │        │  wasm-bindgen call to       │
│  RuntimeFs adapter      │        │  browser JS (dart-sass)     │
│  (implements grass::Fs) │        │  with VFS-reading importer  │
└─────────────────────────┘        └─────────────────────────────┘
           │                                    │
           ▼                                    ▼
┌─────────────────────────┐        ┌─────────────────────────────┐
│  RuntimeFs              │        │  sass.js bridge             │
│  impl grass::Fs for     │        │  - jsCompileSass()          │
│  SystemRuntime          │        │  - Custom importer reads    │
│  - is_dir, is_file,     │        │    from VFS via callback    │
│    read, canonicalize   │        │  - Lazy-loads sass module   │
└─────────────────────────┘        └─────────────────────────────┘
```

### grass Crate Integration

The grass crate provides a `Fs` trait for custom file system implementations:

```rust
// grass::Fs trait (from docs.rs/grass)
pub trait Fs: Debug {
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn read(&self, path: &Path) -> Result<Vec<u8>, Error>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, Error>;  // provided
}
```

This maps directly to our `SystemRuntime` trait methods:

```rust
/// Adapter that implements grass::Fs using SystemRuntime
#[derive(Debug)]
struct RuntimeFs<'a> {
    runtime: &'a dyn SystemRuntime,
    embedded_resources: &'a EmbeddedResources,
}

impl<'a> grass::Fs for RuntimeFs<'a> {
    fn is_dir(&self, path: &Path) -> bool {
        // Check embedded resources first, then delegate to runtime
        self.embedded_resources.is_dir(path)
            || self.runtime.is_dir(path).unwrap_or(false)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.embedded_resources.is_file(path)
            || self.runtime.is_file(path).unwrap_or(false)
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, io::Error> {
        // Check embedded resources first
        if let Some(content) = self.embedded_resources.read(path) {
            return Ok(content.to_vec());
        }
        // Fall back to runtime
        self.runtime.file_read(path).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, e.to_string())
        })
    }
}
```

### grass Version Compatibility

From the [grass README](https://github.com/connorskees/grass):

> "grass currently targets dart-sass version 1.54.3"

> "This crate is capable of compiling Bootstrap 4 and 5, bulma and bulma-scss,
> Bourbon, as well as most other large Sass libraries with complete accuracy."

**Bootstrap 5.3.1 compatibility**: Bootstrap 5.3.1 was released July 2023, before the
dart-sass 1.77+ deprecation warnings. grass's CI verifies byte-for-byte accuracy
for Bootstrap 5.0.2+, so 5.3.1 should work correctly.

**Risk mitigation**: Phase 2b adds parity testing that compares grass output to
dart-sass output for our specific Bootstrap version.

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

#### 1.2 Layer Parsing

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

#### 1.4 Compilation Options

```rust
/// Compile raw SCSS to CSS
pub fn compile_scss(scss: &str, options: &CompileOptions) -> Result<String, SassError>;

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub minified: bool,
    pub load_paths: Vec<PathBuf>,
    // Note: source_map intentionally omitted for initial implementation
}
```

### Milestone 2: Native Runtime (grass)

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
```

Also add a new error variant:

```rust
pub enum RuntimeError {
    // ... existing variants ...

    /// SASS compilation error
    SassError(String),
}
```

#### 2.2 Native Implementation (grass)

```rust
// In native.rs
use grass::{Options, OutputStyle};

impl SystemRuntime for NativeRuntime {
    fn sass_available(&self) -> bool {
        true  // grass is always available on native
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        Some("grass")
    }

    async fn compile_sass(
        &self,
        scss: &str,
        options: &SassCompileOptions,
    ) -> RuntimeResult<String> {
        // Create adapter that implements grass::Fs
        let fs = RuntimeFs::new(self, &EMBEDDED_RESOURCES);

        let grass_options = Options::default()
            .fs(&fs)
            .load_paths(&options.load_paths)
            .style(if options.minified {
                OutputStyle::Compressed
            } else {
                OutputStyle::Expanded
            });

        grass::from_string(scss, &grass_options)
            .map_err(|e| RuntimeError::SassError(e.to_string()))
    }
}
```

#### 2.3 Parity Testing (grass vs dart-sass)

To ensure grass produces acceptable output for Bootstrap 5.3.1:

```rust
/// Compare grass output with dart-sass reference output
pub struct ParityTest {
    pub name: String,
    pub input_scss: String,
    pub dart_sass_output: String,  // Pre-generated fixture
    pub grass_output: String,
    pub matches: bool,
}

/// Test cases for parity validation
pub fn parity_test_cases() -> Vec<ParityTest> {
    vec![
        // Bootstrap 5.3.1 full compilation
        // All 24 Bootswatch themes
        // Layer boundary edge cases
        // Variable precedence tests
    ]
}
```

The dart-sass reference outputs are pre-generated using the npm `sass` package
and stored as test fixtures.

### Milestone 3: WASM Runtime (dart-sass via JS)

#### 3.1 JavaScript Bridge

New file: `hub-client/src/wasm-js-bridge/sass.js`

```javascript
/**
 * WASM-JS Bridge for SASS Compilation
 *
 * Uses lazy-loading to avoid blocking hub-client startup.
 * dart-sass is ~5MB, so we only load it when first needed.
 */

let sassModule = null;
let sassLoadPromise = null;

/**
 * Lazy-load the sass module
 */
async function loadSass() {
    if (sassModule) return sassModule;
    if (sassLoadPromise) return sassLoadPromise;

    sassLoadPromise = import('sass').then(module => {
        sassModule = module.default || module;
        return sassModule;
    });

    return sassLoadPromise;
}

/**
 * Check if SASS compilation is available
 */
export function jsSassAvailable() {
    return true;  // We can always try to load sass
}

/**
 * Read a file from the VFS (called by custom importer)
 * This function is set by the WASM module initialization
 */
let vfsReadFile = null;

export function setVfsReadFile(fn) {
    vfsReadFile = fn;
}

/**
 * Compile SCSS to CSS
 *
 * @param {string} scss - The SCSS source code
 * @param {string} style - Output style: "expanded" or "compressed"
 * @param {string[]} loadPaths - Paths to search for imports
 * @returns {Promise<string>} Compiled CSS
 */
export async function jsCompileSass(scss, style, loadPaths) {
    const sass = await loadSass();

    // Custom importer that reads from VFS
    const vfsImporter = {
        canonicalize(url, context) {
            // Handle relative imports
            if (context.containingUrl && !url.startsWith('/')) {
                const base = new URL(context.containingUrl);
                return new URL(url, base);
            }
            // Handle absolute VFS paths
            if (url.startsWith('/__quarto_resources__/')) {
                return new URL('vfs:' + url);
            }
            // Try each load path
            for (const loadPath of loadPaths) {
                const fullPath = loadPath + '/' + url;
                if (vfsReadFile(fullPath) !== null) {
                    return new URL('vfs:' + fullPath);
                }
            }
            return null;
        },
        load(canonicalUrl) {
            const path = canonicalUrl.pathname;
            const content = vfsReadFile(path);
            if (content === null) {
                return null;
            }
            return {
                contents: content,
                syntax: path.endsWith('.sass') ? 'indented' : 'scss'
            };
        }
    };

    const result = sass.compileString(scss, {
        style: style,
        importers: [vfsImporter],
        logger: { warn: () => {}, debug: () => {} },
        silenceDeprecations: ['global-builtin', 'color-functions', 'import']
    });

    return result.css;
}
```

#### 3.2 WASM Implementation

```rust
// In wasm.rs
#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/sass.js")]
extern "C" {
    #[wasm_bindgen(js_name = "jsSassAvailable")]
    fn js_sass_available_impl() -> bool;

    #[wasm_bindgen(js_name = "jsCompileSass", catch)]
    fn js_compile_sass_impl(
        scss: &str,
        style: &str,
        load_paths: Vec<JsValue>,
    ) -> Result<JsValue, JsValue>;
}

impl SystemRuntime for WasmRuntime {
    fn sass_available(&self) -> bool {
        js_sass_available_impl()
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        Some("dart-sass")
    }

    async fn compile_sass(
        &self,
        scss: &str,
        options: &SassCompileOptions,
    ) -> RuntimeResult<String> {
        let style = if options.minified { "compressed" } else { "expanded" };
        let load_paths: Vec<JsValue> = options.load_paths
            .iter()
            .map(|p| JsValue::from_str(&p.to_string_lossy()))
            .collect();

        let promise = js_compile_sass_impl(scss, style, load_paths)
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;

        let result = JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| RuntimeError::SassError(format!("{:?}", e)))?;

        result.as_string()
            .ok_or_else(|| RuntimeError::SassError("Expected string result".to_string()))
    }
}
```

### Milestone 4: VFS Resource Embedding

Bootstrap and Quarto SCSS files must be available in both native and WASM contexts.

#### 4.1 Embedded Resources Structure

```rust
/// Embedded SCSS resources (Bootstrap, Quarto themes, etc.)
///
/// These are compiled into the binary/WASM module and made available
/// under a special path prefix: `/__quarto_resources__/`
pub struct EmbeddedResources {
    files: HashMap<&'static str, &'static [u8]>,
    directories: HashSet<&'static str>,
}

impl EmbeddedResources {
    /// Bootstrap 5.3.1 SCSS files
    pub fn bootstrap() -> &'static Self {
        static BOOTSTRAP: OnceLock<EmbeddedResources> = OnceLock::new();
        BOOTSTRAP.get_or_init(|| {
            // Generated by build.rs using include_dir! or similar
            include_resources!("resources/bootstrap/scss")
        })
    }

    /// Check if path exists as a directory
    pub fn is_dir(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.directories.contains(path_str.as_ref())
    }

    /// Check if path exists as a file
    pub fn is_file(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.files.contains_key(path_str.as_ref())
    }

    /// Read file contents
    pub fn read(&self, path: &Path) -> Option<&'static [u8]> {
        let path_str = path.to_string_lossy();
        self.files.get(path_str.as_ref()).copied()
    }
}
```

#### 4.2 VFS Pre-population for WASM

When the WASM module initializes, embedded resources are loaded into the VFS:

```rust
/// Initialize VFS with embedded resources
pub fn init_vfs_resources(runtime: &WasmRuntime) {
    let resources = EmbeddedResources::bootstrap();
    for (path, content) in resources.files.iter() {
        let vfs_path = PathBuf::from("/__quarto_resources__").join(path);
        runtime.add_file(&vfs_path, content.to_vec());
    }
}
```

#### 4.3 Load Path Configuration

```rust
/// Standard load paths for SASS compilation
pub fn default_load_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/__quarto_resources__/bootstrap/scss"),
        PathBuf::from("/__quarto_resources__/quarto/scss"),
    ]
}
```

### Milestone 5: Hub-Client Caching

#### 5.1 IndexedDB Cache Schema

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

#### 5.2 Cache Size Management

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

#### 5.3 Integration with Rendering

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

### Milestone 6: Bootstrap Integration

Full Bootstrap theming support (required for Quarto HTML output):

#### 6.1 Bootstrap SASS Assets

Bootstrap 5.3.1 SCSS files are embedded via the resource system (Milestone 4).

#### 6.2 Theme Resolution

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

#### 6.3 Quarto Layer Assembly

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

#### 6.4 Pandoc Variable Mapping

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

### Milestone 7: Brand System

Centralized theme configuration via `_brand.yml`:

#### 7.1 Brand Configuration Parsing

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

#### 7.2 Brand-to-SASS Conversion

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

#### 7.3 Light/Dark Mode Support

```rust
/// Compile bundle with light and dark variants
pub fn compile_themed_bundle(
    bundle: &SassBundle,
    runtime: &dyn SystemRuntime,
    options: &CompileOptions,
) -> Result<ThemedCss, SassError> {
    let light_css = compile_bundle_light(bundle, runtime, options)?;
    let dark_css = bundle.dark.as_ref()
        .map(|dark| compile_bundle_dark(bundle, dark, runtime, options))
        .transpose()?;

    Ok(ThemedCss {
        light: light_css,
        dark: dark_css,
        dark_default: bundle.dark.as_ref().map(|d| d.default).unwrap_or(false),
    })
}
```

## Implementation Tasks

### Phase 1: Core Types and Infrastructure

- [x] Create `quarto-sass` crate with types (`SassLayer`, `SassBundleLayers`, `SassBundle`)
- [x] Implement `SassLayer` parsing (boundary comments regex)
- [x] Implement layer merging with correct precedence (defaults reversed)
- [x] Write unit tests for layer parsing/merging

### Phase 2a: Native Runtime (grass)

- [x] Add `grass` dependency to workspace
- [x] Add SASS methods to `SystemRuntime` trait
- [x] Add `RuntimeError::SassError` variant
- [x] Implement `RuntimeFs` adapter (grass::Fs for SystemRuntime)
- [x] Implement `NativeRuntime::compile_sass()` with grass
- [x] Basic integration test: compile simple SCSS
- [x] Test Bootstrap 5.3.1 compilation

### Phase 2b: Parity Testing

- [ ] Create parity test harness (grass vs dart-sass)
- [ ] Generate dart-sass reference fixtures for Bootstrap 5.3.1
- [ ] Generate dart-sass reference fixtures for Bootswatch themes
- [ ] Add parity tests to CI
- [ ] Document any known differences

### Phase 3: WASM Runtime (dart-sass via JS)

- [ ] Create `hub-client/src/wasm-js-bridge/sass.js` bridge
- [ ] Add `sass` npm dependency to hub-client
- [ ] Implement lazy loading for sass module
- [ ] Implement custom VFS importer
- [ ] Implement `WasmRuntime::compile_sass()` with JS bridge
- [ ] Test WASM compilation produces same output as native for simple cases
- [ ] Verify lazy loading doesn't block hub-client startup

### Phase 4: VFS Resource Embedding

- [ ] Create `EmbeddedResources` type
- [ ] Set up build.rs to embed Bootstrap 5.3.1 SCSS
- [ ] Implement VFS pre-population for WASM
- [ ] Configure default load paths
- [ ] Test embedded resource access in native runtime
- [ ] Test embedded resource access in WASM runtime

### Phase 5: Hub-Client Caching

- [ ] Add `sassCache` store to IndexedDB schema
- [ ] Create migration for new schema version
- [ ] Implement `SassCacheManager` with LRU eviction
- [ ] Add cache size configuration (default: 50MB)
- [ ] Integrate with rendering pipeline
- [ ] Test cache hit/miss scenarios
- [ ] Test eviction under size pressure

### Phase 6: Bootstrap Integration

- [ ] Embed Bootswatch themes (24 themes)
- [ ] Implement `BuiltInTheme` enum
- [ ] Implement `resolve_theme()` function
- [ ] Port `layerQuartoScss` assembly function
- [ ] Port `pandocVariablesToThemeScss` mapping
- [ ] Test all 24 Bootswatch themes compile correctly
- [ ] Integration test with Quarto HTML format

### Phase 7: Brand System

- [ ] Implement `Brand` configuration parsing (`_brand.yml`)
- [ ] Implement `brand_color_layer()` - color palette to SASS
- [ ] Implement `brand_typography_layer()` - fonts and typography
- [ ] Implement `brand_defaults_layer()` - Bootstrap defaults
- [ ] Light/dark mode variant generation
- [ ] CSS custom properties (--brand-* variables)
- [ ] Integration test with `_brand.yml` examples

### Phase 8: End-to-End Testing

- [ ] Full pipeline test: QMD → HTML with custom SASS
- [ ] Hub-client preview with themed styling
- [ ] Performance benchmarking (native grass vs WASM dart-sass)
- [ ] Cache persistence across sessions
- [ ] Cross-browser testing (Chrome, Firefox, Safari)

## Critical Finding: Import Resolution

### How TS Quarto Handles Imports

TS Quarto calls the **dart-sass binary** directly:

```bash
sass input.scss output.css --load-path=/bootstrap --load-path=/bslib
```

The approach:
1. Concatenate all SCSS layers into one string (maintaining careful ordering)
2. Write to temp file
3. Call dart-sass with `--load-path` for each resource directory
4. Let dart-sass resolve all `@use`/`@import` natively

### Our Approach

**Native (grass)**: Use the `Fs` trait to provide custom file resolution:

```rust
let fs = RuntimeFs::new(runtime, &embedded_resources);
let options = Options::default()
    .fs(&fs)
    .load_paths(&load_paths);
grass::from_string(scss, &options)?
```

**WASM (dart-sass)**: Use custom importer that reads from VFS:

```javascript
sass.compileString(source, {
    importers: [vfsImporter],
    // ...
});
```

## Decisions Made

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Native compiler | grass | Pure Rust, ~2x faster, no JS engine |
| WASM compiler | dart-sass | Reference implementation, npm package |
| Bootstrap version | 5.3.1 | Matches TS Quarto |
| dart-sass loading | Lazy | Avoid blocking startup (~5MB) |
| Resource embedding | `include_dir!`-like | Offline support, simplicity |
| VFS path prefix | `/__quarto_resources__/` | Clear separation from user files |
| Source maps | Not initially | Can add later if needed |
| Cache size | 50MB | ~100+ stylesheets, under quota |

## Dependencies

### New Crate Dependencies

```toml
# crates/quarto-sass/Cargo.toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
regex = "1.0"  # For layer boundary parsing

# Native only (grass)
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
grass = "0.13"
```

### Hub-Client Dependencies

```json
// package.json additions
{
  "dependencies": {
    "sass": "^1.77.0"
  }
}
```

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| grass/dart-sass output differences | Low | Medium | Parity testing in CI (Phase 2b) |
| Bootstrap 5.3.1 incompatible with grass | Low | High | Verified by grass CI; our parity tests |
| sass npm bundle size (~5MB) | N/A | Low | Lazy loading; acceptable for web app |
| grass `@use`/`@forward` edge cases | Medium | Low | Most Bootstrap uses simple patterns |
| Layer boundary parsing edge cases | Medium | Medium | Port TS Quarto's regex exactly |
| IndexedDB quota exceeded | Low | Low | LRU eviction; configurable limits |

## Success Criteria

### Phase 1-2 (Core + Native)
1. Can compile simple SCSS to CSS using grass
2. Layer parsing handles all TS Quarto boundary formats
3. Layer merging produces correct precedence
4. Bootstrap 5.3.1 compiles successfully
5. Parity tests pass (grass vs dart-sass)

### Phase 3-4 (WASM + Resources)
1. WASM compilation works with VFS
2. Embedded resources accessible in both native and WASM
3. Lazy loading works without blocking hub-client startup
4. Compilation completes in < 2 seconds for Bootstrap

### Phase 5+ (Full Feature)
1. Hub-client preview shows custom SASS styling correctly
2. Compilation cached with < 5ms cache hit latency
3. Cache stays within configured size limits
4. All 24 Bootswatch themes compile correctly

## Timeline Considerations

Recommended phase ordering:

1. **Phase 1** (Core types): Foundation for everything else
2. **Phase 2a** (Native/grass): Validates architecture
3. **Phase 2b** (Parity tests): Ensures grass compatibility
4. **Phase 4** (VFS resources): Needed before Bootstrap works
5. **Phase 6** (Bootstrap): Full theming support
6. **Phase 3** (WASM runtime): Enables hub-client integration
7. **Phase 5** (Caching): Performance optimization
8. **Phase 7** (Brand): Advanced theming
9. **Phase 8** (E2E): Polish and validation

Phases 1 and 4 can be worked in parallel.

## References

### Internal
- SystemRuntime trait: `crates/quarto-system-runtime/src/traits.rs`
- VFS implementation: `crates/quarto-system-runtime/src/wasm.rs`
- Hub-client JS bridge pattern: `hub-client/src/wasm-js-bridge/template.js`

### External (TS Quarto)
- SASS compilation: `external-sources/quarto-cli/src/core/sass.ts`
- Type definitions: `external-sources/quarto-cli/src/config/types.ts`
- dart-sass integration: `external-sources/quarto-cli/src/core/dart-sass.ts`
- Bootstrap theming: `external-sources/quarto-cli/src/format/html/format-html-scss.ts`
- Brand system: `external-sources/quarto-cli/src/core/sass/brand.ts`
- Bootstrap version: `external-sources/quarto-cli/configuration` (BOOTSWATCH=5.3.1)

### Libraries
- [grass crate](https://crates.io/crates/grass) - Pure Rust SASS compiler
- [grass Fs trait](https://docs.rs/grass/latest/grass/trait.Fs.html) - Custom filesystem
- [grass README](https://github.com/connorskees/grass) - Version compatibility info
- [dart-sass npm](https://www.npmjs.com/package/sass) - JavaScript API
