# Phase 4: JavaScript Infrastructure for Quarto HTML

**Parent Plan**: [`2026-01-24-html-rendering-parity.md`](./2026-01-24-html-rendering-parity.md)
**Beads Issue**: kyoto-aqv (labeled "Phase 3: JavaScript Dependencies" in beads)
**Created**: 2026-01-28
**Status**: Planning

---

## Overview

This phase implements the JavaScript runtime infrastructure for Quarto HTML output. Rather than directly porting TS Quarto's approach (which has accumulated technical debt), we take this opportunity to design a cleaner, more modular system that:

1. Reflects our feature-based architecture (small, independent features that can be enabled/disabled)
2. Uses modern ES6 modules to avoid global namespace pollution
3. Provides a standard mechanism for declaring JS dependencies per feature
4. Works in both native CLI and WASM (hub-client) contexts

---

## Analysis: TS Quarto's JavaScript Architecture

### Current State (Technical Debt)

After thoroughly analyzing the TypeScript Quarto codebase, here's what we found:

#### 1. quarto.js Structure

**Location**: `external-sources/quarto-cli/src/resources/formats/html/quarto.js` (26.8 KB)

It's a single ES6 module that handles:
- Margin element layout management
- TOC active link tracking
- Section change events
- Reader mode toggle
- Category activation
- Tab/Shiny event dispatching
- htmlwidgets integration

```javascript
import * as tabsets from "./tabsets/tabsets.js";
import * as axe from "./axe/axe-check.js";

const sectionChanged = new CustomEvent("quarto-sectionChanged", {...});
// ... ~800 lines of code
```

#### 2. Dependency System

Dependencies are declared as `FormatDependency` objects:

```typescript
interface FormatDependency {
  name: string;
  scripts?: DependencyHtmlFile[];      // JS files to include
  stylesheets?: DependencyHtmlFile[];  // CSS files to include
  head?: string;                       // Raw HTML for <head>
  // ...
}
```

Features conditionally add dependencies in `htmlFormatExtras()`:
```typescript
if (options.copyCode) {
  dependencies.push(clipboardDependency());
}
if (options.anchors) {
  scripts.push({ name: "anchor.min.js", path: ... });
}
```

#### 3. Problems with Current Approach

| Problem | Description |
|---------|-------------|
| **Global namespace pollution** | Libraries expose globals: `window.ClipboardJS`, `window.AnchorJS`, `window.Tabby`, `window.tippy` |
| **No bundling** | Each library is a separate HTTP request |
| **Complex initialization** | EJS templates generate inline scripts that wire libraries together |
| **Feature-JS coupling** | Feature options scattered across `htmlFormatExtras()`, templates, and inline scripts |
| **Duplicated logic** | Same feature may need logic in: Lua filter, TS postprocessor, AND inline JS |
| **Testing difficulty** | Inline EJS scripts are hard to unit test |

#### 4. JS Libraries Used

| Library | Purpose | Global | Size |
|---------|---------|--------|------|
| quarto.js | Core orchestration | (ES6 module) | 27KB |
| clipboard.min.js | Copy to clipboard | `ClipboardJS` | 8KB |
| anchor.min.js | Heading anchor links | `AnchorJS` | 6KB |
| popper.min.js | Tooltip positioning | `Popper` | 20KB |
| tippy.umd.min.js | Tooltips/popovers | `tippy` | 27KB |
| tabby.js | Tab management | `Tabby` | 4KB |
| zenscroll-min.js | Smooth scrolling | `zenscroll` | 3KB |

---

## Design: Rust Quarto JavaScript Infrastructure

### Design Principles

1. **Feature-centric**: Each document feature that needs JS should be self-contained
2. **Declarative**: Features declare their JS needs, the system handles injection
3. **Modular**: Use ES6 modules, no global namespace pollution
4. **Bundled**: Single JS file per feature set (not one per library)
5. **Testable**: JS modules are unit-testable, not inline template code
6. **WASM-compatible**: Works identically in CLI and hub-client

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        HTML Output                                   │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ <script type="module" src="quarto-runtime.js"></script>      │   │
│  │                                                              │   │
│  │ quarto-runtime.js:                                           │   │
│  │   - Feature registry                                         │   │
│  │   - Auto-initialization based on DOM markers                 │   │
│  │   - No globals, all ES6 modules                             │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    Feature Modules                                   │
│                                                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
│  │ toc.js      │  │ anchors.js  │  │ copy-code.js │                │
│  │             │  │             │  │              │                 │
│  │ - Scroll    │  │ - Add links │  │ - Clipboard  │                 │
│  │   sync      │  │   to h2-h6  │  │   button     │                 │
│  │ - Active    │  │ - Reveal on │  │ - Feedback   │                 │
│  │   link      │  │   click     │  │   tooltip    │                 │
│  └─────────────┘  └─────────────┘  └──────────────┘                 │
│                                                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
│  │ tabsets.js  │  │ hover.js    │  │ dark-mode.js│                 │
│  │             │  │             │  │              │                 │
│  │ - Tab state │  │ - Footnotes │  │ - Toggle    │                 │
│  │ - URL hash  │  │ - Citations │  │ - Persist   │                 │
│  │ - Events    │  │ - Crossrefs │  │ - System    │                 │
│  └─────────────┘  └─────────────┘  └──────────────┘                 │
└─────────────────────────────────────────────────────────────────────┘
```

### Feature Module Pattern

Each feature is a self-contained ES6 module:

```javascript
// features/copy-code.js
export const name = "copy-code";
export const selector = ".code-copy-button";

export function init(root = document) {
  const buttons = root.querySelectorAll(selector);
  if (buttons.length === 0) return;

  for (const btn of buttons) {
    btn.addEventListener("click", handleCopy);
  }
}

async function handleCopy(event) {
  const codeEl = event.target.closest(".sourceCode")?.querySelector("code");
  if (!codeEl) return;

  await navigator.clipboard.writeText(codeEl.textContent);
  showFeedback(event.target, "Copied!");
}

function showFeedback(el, message) {
  // Show temporary "Copied!" indicator
}
```

### Runtime Core

```javascript
// quarto-runtime.js
const features = new Map();

export function register(feature) {
  features.set(feature.name, feature);
}

export function init(root = document) {
  for (const [name, feature] of features) {
    if (feature.selector && root.querySelector(feature.selector)) {
      feature.init(root);
    }
  }
}

// Auto-init on DOMContentLoaded
if (typeof document !== "undefined") {
  document.addEventListener("DOMContentLoaded", () => init());
}
```

### Feature Declaration in Rust

On the Rust side, we use a trait-based approach so new features don't require modifying a central enum:

```rust
/// Trait that all JS features implement
pub trait JsFeature: Send + Sync {
    /// Unique identifier for this feature (used in template variables)
    fn name(&self) -> &'static str;

    /// CSS selector to detect if this feature is needed based on document content.
    /// If None, feature detection relies solely on format config.
    fn selector(&self) -> Option<&'static str>;

    /// Path to the JS module file (relative to resources/js/features/)
    fn js_module(&self) -> &'static str;

    /// Check if this feature is needed for a document.
    /// Called with the document AST and format configuration.
    fn is_needed(&self, doc: &Pandoc, format: &Format) -> bool;
}

// Each feature is a unit struct implementing the trait
pub struct TocFeature;

impl JsFeature for TocFeature {
    fn name(&self) -> &'static str { "toc" }
    fn selector(&self) -> Option<&'static str> { Some("nav.toc-active") }
    fn js_module(&self) -> &'static str { "toc.js" }

    fn is_needed(&self, _doc: &Pandoc, format: &Format) -> bool {
        format.toc()
    }
}

pub struct AnchorsFeature;

impl JsFeature for AnchorsFeature {
    fn name(&self) -> &'static str { "anchors" }
    fn selector(&self) -> Option<&'static str> { Some(".anchored") }
    fn js_module(&self) -> &'static str { "anchors.js" }

    fn is_needed(&self, _doc: &Pandoc, format: &Format) -> bool {
        format.anchor_sections()
    }
}

/// Registry of all available JS features.
/// Adding a new feature = implement JsFeature + add one line here.
pub fn all_features() -> Vec<Box<dyn JsFeature>> {
    vec![
        Box::new(TocFeature),
        Box::new(AnchorsFeature),
        Box::new(TabsetsFeature),
        Box::new(DarkModeFeature),
        // Future: Box::new(CopyCodeFeature),
        // Future: Box::new(CodeAnnotationsFeature),
    ]
}

/// Collect JS features needed for a specific document
pub fn collect_needed_features(doc: &Pandoc, format: &Format) -> Vec<&'static str> {
    all_features()
        .into_iter()
        .filter(|f| f.is_needed(doc, format))
        .map(|f| f.js_module())
        .collect()
}
```

This trait-based approach:
- **Extensible**: New feature = new struct + trait impl + one line in `all_features()`
- **Self-documenting**: Each feature's requirements are co-located
- **Testable**: Features can be unit tested in isolation
- **WASM-compatible**: No linker tricks, works everywhere

### Template Integration

The template system injects JS based on collected features:

```html
$if(js-features)$
<script type="module">
import { register, init } from "./quarto-runtime.js";
$for(js-feature)$
import * as $js-feature.module$ from "./features/$js-feature.file$";
register($js-feature.module$);
$endfor$
init();
</script>
$endif$
```

Or with bundling (preferred for production):

```html
$if(js-features)$
<script type="module" src="quarto-runtime.min.js"></script>
$endif$
```

---

## JavaScript Bundling: esbuild Analysis and rspack Feasibility

### Context: Self-Contained HTML Mode

TS Quarto supports `--embed-resources` (Pandoc's self-contained mode) which produces a single `.html` file with all resources embedded:
- Images → base64 data URIs
- Stylesheets → inline `<style>` tags
- Scripts → inline `<script>` tags with bundled ES6 modules

For JS bundling, TS Quarto uses **esbuild** to combine ES6 module imports into a single file.

### How esbuild Is Used in TS Quarto

#### 1. Core Functions (`src/core/esbuild.ts`)

```typescript
// Analysis: Extract dependency graph without output
esbuildAnalyze(input, workingDir, tempContext)
  // Uses: --analyze=verbose --metafile=...

// Compilation: Bundle JS/TS with format options
esbuildCompile(input, workingDir, args, format)
  // Uses: --bundle --format={esm|cjs|iife}

// Low-level: Direct command invocation
esbuildCommand(args, input, workingDir)
```

#### 2. Build-Time Dependency Analysis

At build time, Quarto pre-computes module dependencies:

```typescript
// build-artifacts/cmd.ts
const analysisCache: Record<string, ESBuildAnalysis> = {};
for (const file of inputFiles) {
  analysisCache[file] = await esbuildAnalyze(
    formatResourcePath("html", file),
    resourcePath(join("formats", "html")),
  );
}
// Writes to esbuild-analysis-cache.json
```

This cache is then used at runtime to discover all modules needed for `quarto.js`:

```typescript
// format-html.ts
function recursiveModuleDependencies(path: string): DependencyHtmlFile[] {
  const analysis = esbuildCachedAnalysis(inpRelPath);
  for (const imp of analysis.outputs[...].imports) {
    if (imp.external) {
      result.push({ name: relPath, path: ..., attribs: { type: "module" } });
    }
  }
}
```

#### 3. Self-Contained Mode Bundling

For `--embed-resources`, each module script is bundled and converted to a data URI:

```typescript
// self-contained.ts
const bundleModules = async (dom: HTMLDocument, workingDir: string) => {
  const modules = dom.querySelectorAll("script[type='module']");
  for (const module of modules) {
    const src = module.getAttribute("src");
    const jsSource = await esbuildCompile(
      Deno.readTextFileSync(join(workingDir, src)),
      dirname(srcName),
      [],
      "esm",
    );
    module.setAttribute("src", asDataUrl(jsSource, "application/javascript"));
  }
};
```

#### 4. Observable/OJS Compilation

esbuild also handles TypeScript compilation for Observable:

```typescript
// extract-resources.ts
const jsSource = await esbuildCommand([
  file,
  "--format=esm",
  "--sourcemap=inline",
  "--jsx-factory=window._ojs.jsx.createElement",
], "", fileDir);
```

### rspack: A Rust-Native Alternative

**rspack** is a Rust-based JavaScript bundler (webpack-compatible) that could replace esbuild for Rust Quarto.

#### Architecture Overview

- **92 Rust crates** in a modular workspace
- Pure Rust - no JavaScript runtime required
- Builder-pattern API designed for programmatic use
- Webpack-compatible configuration model

#### Key Crates

| Crate | Purpose |
|-------|---------|
| `rspack_core` | Main bundler engine (Compiler, Compilation, Module Graph) |
| `rspack_loader_swc` | JavaScript/TypeScript via SWC |
| `rspack_javascript_compiler` | JS code generation |
| `rspack_resolver` | Module resolution |
| `rspack_sources` | Source code/AST handling |
| `rspack_fs` | File system abstraction (supports virtual FS) |

#### Programmatic API

rspack provides an elegant builder API:

```rust
use rspack::Compiler;
use rspack_paths::Utf8Path;

let compiler = Compiler::builder()
    .context(Utf8Path::new("/path/to/project"))
    .entry("main", "./src/index.js")
    .output_filename("[name].bundle.js")
    .mode(Mode::Production)
    .build()
    .unwrap();

compiler.build().await.unwrap();

// Access compiled output directly (no file emission required)
let asset = compiler.compilation.assets().get("main.bundle.js");
let bundled_source = asset.source.as_ref().unwrap().source();
```

#### Advantages for Rust Quarto

| Aspect | esbuild (TS Quarto) | rspack (Rust Quarto) |
|--------|---------------------|----------------------|
| Language | Go binary, invoked via CLI | Pure Rust, library API |
| Integration | Subprocess with temp files | Direct function calls |
| Error handling | Parse CLI output | Native Rust Result types |
| Virtual FS | Not supported | Built-in trait abstraction |
| WASM | Would need WASI binary | Native Rust compilation |
| Async | Blocking subprocess | Native tokio async |

#### Challenges

| Challenge | Mitigation |
|-----------|------------|
| Pre-1.0 API stability | Pin version, maintain fork if needed |
| Large dependency tree (~92 crates) | Use feature flags, tree-shaking |
| Tokio runtime requirement | Already using tokio in quarto-core |
| Learning curve (webpack model) | Start with minimal config |

### Recommended Approach

#### Phase 4.0 (Now): No Bundling Required

For initial implementation, we don't need bundling:
- Multiple `<script type="module">` tags work fine
- HTTP/2 multiplexing handles parallel loading
- Simpler to implement and debug

#### Future Phase: rspack Integration

When we implement `--embed-resources`:

1. **Add rspack as optional dependency**
   ```toml
   [dependencies]
   rspack = { version = "0.100", optional = true }

   [features]
   bundling = ["rspack"]
   ```

2. **Create bundling abstraction**
   ```rust
   pub trait JsBundler: Send + Sync {
       async fn bundle(&self, entry: &str, options: BundleOptions) -> Result<String>;
   }

   // rspack implementation
   #[cfg(feature = "bundling")]
   pub struct RspackBundler { /* ... */ }
   ```

3. **Virtual file system for in-memory bundling**
   ```rust
   // rspack supports custom FS implementations
   impl rspack_fs::ReadableFileSystem for VirtualFs { /* ... */ }
   ```

4. **Integration point in self-contained mode**
   ```rust
   // In self-contained HTML generation
   if format.embed_resources() {
       let bundled = bundler.bundle("quarto-runtime.js", opts).await?;
       // Inline as data URI or <script> content
   }
   ```

### Open Questions

1. **Bundle granularity**: One bundle for all features, or per-feature bundles?
   - Recommendation: Single bundle for simplicity initially

2. **Source maps**: Include in self-contained mode?
   - Recommendation: Optional, disabled by default for size

3. **Minification**: rspack supports this via SWC
   - Recommendation: Enable for production builds

4. **WASM bundling**: Not needed. Bundling is a CLI-only feature for `--embed-resources` mode; hub-client will never need it.

---

## Implementation Strategy

### What We DON'T Need for Initial HTML Parity

Based on Phase 5 dependencies, we can **defer** these features:

| Feature | Reason to Defer |
|---------|-----------------|
| `CopyCode` | Needs AnnotatedCodeBlock (Phase 5) for copy button scaffolding |
| `CodeAnnotations` | Part of Phase 5 |
| `Hover` (tooltips) | Requires hover infrastructure, low priority |
| `SmoothScroll` | Nice-to-have, not critical |

### Phase 4.0: Infrastructure Foundation (P0)

**Goal**: Establish the JS feature system without implementing all features.

#### Work Items

- [ ] **Create JS source directory structure**
  - `crates/quarto-core/resources/js/`
  - `crates/quarto-core/resources/js/quarto-runtime.js`
  - `crates/quarto-core/resources/js/features/`

- [ ] **Implement runtime core** (`quarto-runtime.js`)
  - Feature registry
  - Auto-initialization
  - DOM-ready handling
  - ES6 module exports

- [ ] **Create JsFeature trait and registry**
  - `crates/quarto-core/src/js_features.rs`
  - `JsFeature` trait with `name()`, `selector()`, `js_module()`, `is_needed()`
  - `all_features()` registry function
  - `collect_needed_features()` for document-specific collection

- [ ] **Add JS embedding infrastructure**
  - Use `include_dir!` for JS resources
  - Create `JsResourceBundle` struct
  - Implement feature-based file selection

- [ ] **Integrate with template system**
  - Add `js-features` template variable
  - Inject script tags in template

- [ ] **Test infrastructure**
  - Verify JS loads correctly in HTML output
  - Verify feature detection works

### Phase 4.1: TOC Feature (P1)

**Goal**: First real feature - TOC scroll sync and active link highlighting.

**⚠️ Dependency**: Requires **Phase 6 (TOC Rendering)** from the main plan to be implemented first. The JS needs:
- `<nav id="TOC" role="doc-toc">` element in the HTML
- TOC links with `data-scroll-target` attributes
- Sections with matching IDs

See [Phase 6 Prerequisites](#phase-6-prerequisites-toc-rendering) below for details.

#### Work Items

- [ ] **Create `features/toc.js`**
  - Scroll tracking (200px margin like TS Quarto)
  - Active link management (`.active` class on current section's link)
  - Section change events (`quarto-sectionChanged` custom event)
  - IntersectionObserver-based (modern approach, fallback to scroll events)

- [ ] **Wire into feature system**
  - Add `TocFeature` struct with `is_needed()` checking format config
  - Test with TOC-enabled documents

---

### Phase 6 Prerequisites: TOC Rendering

**Detailed Plan**: [`2026-01-28-phase6-toc-rendering.md`](./2026-01-28-phase6-toc-rendering.md)
**Beads Issue**: kyoto-b48

Phase 4.1 (JS TOC) requires the following HTML structure to be generated:

#### Required HTML Output

```html
<nav id="TOC" role="doc-toc" class="toc-active">
  <h2 id="toc-title">Table of Contents</h2>
  <ul>
    <li>
      <a href="#section-id" class="nav-link" data-scroll-target="#section-id">
        Section Title
      </a>
      <ul>
        <li>
          <a href="#subsection-id" class="nav-link" data-scroll-target="#subsection-id">
            Subsection Title
          </a>
        </li>
      </ul>
    </li>
  </ul>
</nav>
```

#### Implementation Approach (AST-First)

In TS Quarto, Pandoc generates the TOC via `--toc` flag. For Rust Quarto, we have two options:

**Option A: pampa generates TOC** (like Pandoc)
- Add `--toc` equivalent flag to pampa
- pampa's HTML writer generates the `<nav>` structure
- Simpler, matches existing behavior

**Option B: AST transform + template** (cleaner separation)
- `TocExtractTransform` walks AST, collects heading structure
- Store TOC data in document metadata (as structured data)
- Template renders TOC from metadata
- More flexible, enables different TOC locations

**Recommendation**: Option A for initial implementation (matches Pandoc behavior), with Option B as future enhancement for complex layouts.

#### Key Attributes for JS Integration

| Attribute | Purpose | Example |
|-----------|---------|---------|
| `id="TOC"` | JS selector target | `nav#TOC` |
| `role="doc-toc"` | Semantic role, JS selector | `nav[role="doc-toc"]` |
| `class="toc-active"` | Marks TOC for scroll tracking | - |
| `data-scroll-target` | Link's target section ID | `#introduction` |
| `class="nav-link"` | Bootstrap styling | - |

#### Configuration Options to Support

| Option | Type | Default | Notes |
|--------|------|---------|-------|
| `toc` | bool | false | Enable TOC |
| `toc-depth` | number | 3 | Heading levels to include |
| `toc-title` | string | "Table of Contents" | Title text |
| `toc-location` | enum | "right" | `body`, `left`, `right` (advanced: sidebar placement) |

**Note**: `toc-location` sidebar placement is Phase 8 (Advanced Layout) work. For Phase 6, `body` location (TOC before content) is sufficient.

---

### Phase 4.2: Anchors Feature (P1)

**Goal**: Add anchor links to headings with `.anchored` class.

#### Work Items

- [ ] **Create `features/anchors.js`**
  - Generate anchor links for `.anchored` headings
  - Handle click to reveal
  - Accessible implementation

- [ ] **Ensure AST transform adds `.anchored` class**
  - `AnchoredHeadingsTransform` must run before JS can work
  - Test end-to-end

### Phase 4.3: Tabsets Feature (P1)

**Goal**: Tab interactivity for tabset panels.

#### Work Items

- [ ] **Create `features/tabsets.js`**
  - Tab switching
  - URL hash state
  - Keyboard navigation
  - Event dispatching for widgets

### Phase 4.4: Dark Mode Feature (P2)

**Goal**: Dark/light mode toggle with persistence.

#### Work Items

- [ ] **Create `features/dark-mode.js`**
  - Toggle functionality
  - localStorage persistence
  - System preference detection
  - CSS class management

---

## Comparison: TS Quarto vs. Rust Quarto JS

| Aspect | TS Quarto | Rust Quarto |
|--------|-----------|-------------|
| **Module system** | Mix of ES6 and global | Pure ES6 modules |
| **Globals** | Many (ClipboardJS, AnchorJS, etc.) | None - all via imports |
| **Initialization** | Inline EJS templates | Auto-init from runtime |
| **Feature coupling** | Scattered across files | Self-contained modules |
| **Bundling** | None (many HTTP requests) | Optional single bundle |
| **Testing** | Hard (inline code) | Easy (module exports) |
| **Third-party libs** | Direct inclusion | Minimized, native APIs |

### Third-Party Library Alternatives

| TS Quarto | Rust Quarto | Notes |
|-----------|-------------|-------|
| clipboard.min.js | `navigator.clipboard` API | Modern browsers support natively |
| anchor.min.js | Custom ~50 lines | Simple DOM manipulation |
| tabby.js | Custom ~100 lines | Simple state management |
| tippy.js + popper.js | Deferred | Only if hover features needed |

By using native APIs and small custom implementations, we reduce:
- Bundle size (no 47KB for tippy+popper)
- HTTP requests
- Maintenance burden
- Compatibility concerns

---

## File Structure

```
crates/quarto-core/
├── src/
│   ├── js_features.rs          # JsFeature trait + registry
│   └── js_features/            # Feature implementations (if complex)
│       ├── mod.rs              # Re-exports all features
│       ├── toc.rs              # TocFeature
│       ├── anchors.rs          # AnchorsFeature
│       └── ...
├── resources/
│   └── js/
│       ├── quarto-runtime.js   # Core runtime (~100 lines)
│       └── features/
│           ├── toc.js          # TOC scroll sync (~150 lines)
│           ├── anchors.js      # Anchor links (~50 lines)
│           ├── tabsets.js      # Tab management (~100 lines)
│           ├── dark-mode.js    # Theme toggle (~80 lines)
│           ├── copy-code.js    # Copy button (~60 lines) [Phase 5]
│           └── hover.js        # Tooltips (~200 lines) [Future]
```

**Note**: If feature implementations are simple (just trait impls with config checks), they can all live in `js_features.rs`. If they need complex detection logic (AST walking), they can be split into separate modules.

**Total estimated size**: ~500 lines of JS (vs. TS Quarto's thousands + third-party libs)

---

## WASM Considerations

For hub-client (WASM), the JS runs in an existing browser context:

1. **No bundling needed**: Hub-client can load modules directly
2. **Re-initialization**: Must support `init(rootElement)` for partial re-renders
3. **Event cleanup**: Features must clean up listeners on unmount
4. **Shared state**: Some features (dark mode) may need to share state with hub-client

```javascript
// quarto-runtime.js - WASM-friendly
export function init(root = document) {
  for (const [name, feature] of features) {
    if (feature.selector && root.querySelector(feature.selector)) {
      feature.init(root);
    }
  }
}

export function cleanup(root = document) {
  for (const [name, feature] of features) {
    if (feature.cleanup) {
      feature.cleanup(root);
    }
  }
}
```

---

## Testing Strategy

### Unit Tests (Jest or similar)

```javascript
// features/anchors.test.js
import { init, name, selector } from "./anchors.js";

describe("anchors feature", () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <h2 class="anchored" id="intro">Introduction</h2>
      <h2 id="no-anchor">Not Anchored</h2>
    `;
  });

  test("adds anchor links to .anchored headings", () => {
    init();
    expect(document.querySelector("#intro .anchor-link")).toBeTruthy();
    expect(document.querySelector("#no-anchor .anchor-link")).toBeFalsy();
  });
});
```

### Integration Tests

- Full HTML output with JS features enabled
- Verify correct script tags appear
- Browser-based tests for interactivity

---

## Open Questions

1. **Bundling strategy**: Should we pre-bundle features, or rely on HTTP/2 multiplexing?
   - Recommendation: Start unbundled, add bundling later if needed

2. **TypeScript for JS?**: Should our JS be written in TypeScript?
   - Recommendation: Plain JS for simplicity, TypeScript optional later

3. **Where do JS files live?**: In quarto-core, or a separate crate?
   - Recommendation: `quarto-core/resources/js/` - keeps JS with the code that injects it

4. **Source maps?**: For debugging in production?
   - Recommendation: Defer - not critical for initial implementation

---

## Success Criteria

### Phase 4.0 (Infrastructure)
- [ ] JS feature system compiles and embeds correctly
- [ ] Template injects appropriate script tags
- [ ] Feature detection runs without errors
- [ ] Works in both CLI and WASM contexts

### Phase 4.1-4.4 (Features)
- [ ] TOC scroll sync works
- [ ] Anchor links appear on headings
- [ ] Tabsets switch correctly
- [ ] Dark mode toggles and persists

### Overall
- [ ] No global namespace pollution
- [ ] Clean ES6 module structure
- [ ] < 10KB total JS (unminified, pre-compression)
- [ ] All features unit tested

---

## References

### Internal
- Parent plan: `claude-notes/plans/2026-01-24-html-rendering-parity.md`
- Phase 2 (templates): `claude-notes/plans/2026-01-26-phase2-enhanced-template.md`

### External (TS Quarto)
- quarto.js: `external-sources/quarto-cli/src/resources/formats/html/quarto.js`
- Format extras: `external-sources/quarto-cli/src/format/html/format-html.ts`
- Dependency system: `external-sources/quarto-cli/src/config/types.ts`
- After-body template: `external-sources/quarto-cli/src/resources/formats/html/templates/quarto-html-after-body.ejs`

### Web APIs
- [Clipboard API](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API)
- [IntersectionObserver](https://developer.mozilla.org/en-US/docs/Web/API/IntersectionObserver)
- [ES6 Modules](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Modules)
