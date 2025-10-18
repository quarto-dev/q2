# JavaScript Runtime Dependencies in quarto-cli

This document catalogs JavaScript runtime dependencies in quarto-cli and analyzes options for porting to Rust.

## Overview

The quarto-cli codebase has four major categories of JavaScript runtime dependencies:

1. **HTML/DOM Postprocessing** - Manipulating rendered HTML output
2. **EJS Templating** - Generating HTML from templates
3. **Observable/OJS Compilation** - Compiling Observable JavaScript
4. **Browser Automation (Puppeteer)** - Headless browser for rendering

Each has different implications for a Rust port.

---

## 1. HTML/DOM Postprocessing

### Current Implementation

**Core library**: `deno-dom` (HTML parser with DOM API)
- Location: `src/core/deno-dom.ts` (~186 LOC)
- Provides: `DOMParser`, `HTMLDocument`, `Element`, `Node`
- Two modes:
  - Native plugin (faster, FFI-based)
  - WASM fallback (pure JS)

**Usage pattern**:
```typescript
export type HtmlPostProcessor = (
  doc: Document,
  options: {
    inputMetadata: Metadata;
    offset?: string;
    format: Format;
  }
) => Promise<HtmlPostProcessResult>;

export interface HtmlPostProcessResult {
  resources: string[];
  supporting: string[];
}
```

**Registered via**: `FormatExtras.html[kHtmlPostprocessors]`

### Key Postprocessors (21 files)

**Core HTML operations** (`src/core/html.ts`):
- `discoverResourceRefs()` - Find file references (images, scripts, styles) in HTML tags and CSS
- `fixEmptyHrefs()` - Fix anchor elements to have empty href="" (for CSS cursor behavior)
- `processFileResourceRefs()` - Process file references with custom handlers

**Format-specific** (`src/format/html/`):
- **format-html-bootstrap.ts** (~1800 LOC):
  - Code links rendering (GitHub repo links, etc.)
  - Bootstrap navigation components
  - Sidebar generation
  - Footer generation
- **format-html-notebook.ts**: Notebook preview links
- **format-html-title.ts**: Title block manipulation
- **codetools.ts** (365 LOC):
  - `querySelectorAll()` to find embedded source code spans
  - Manipulate code tool buttons
  - View source modal generation

**Website-specific** (`src/project/types/website/`):
- Navigation bars
- Search integration
- Analytics code injection
- Draft page handling

### DOM Manipulation Patterns

Common operations across ~98 uses in 7 HTML format files:
```typescript
// Query selectors
doc.querySelectorAll("style")
doc.querySelectorAll(".quarto-embedded-source-code > div.sourceCode > pre > code > span")
doc.querySelector("#quarto-header")

// Attribute manipulation
element.getAttribute("href")
element.setAttribute("data-bs-toggle", "dropdown")
element.removeAttribute("class")

// DOM tree manipulation
parent.appendChild(newElement)
element.innerHTML = "<div>...</div>"
element.outerHTML
element.textContent
```

### Rust Port Options

#### Option 1: html5ever + scraper (Recommended)

**Crates**:
- `html5ever` - Fast HTML5 parser (used by Servo browser engine)
- `scraper` - High-level wrapper with CSS selector API (built on html5ever + selectors)
- `markup5ever_rcdom` - DOM tree structure

**Pros**:
- ✅ Industry standard (powers Servo, used by Firefox internals)
- ✅ CSS selector support nearly identical to JS (via `scraper`)
- ✅ Excellent performance (streaming parser)
- ✅ Serialization back to HTML built-in
- ✅ Large ecosystem

**Cons**:
- ⚠️ API less ergonomic than JS DOM (more Rust-idiomatic)
- ⚠️ Requires learning different traversal patterns

**Example**:
```rust
use scraper::{Html, Selector};

fn discover_resource_refs(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let img_selector = Selector::parse("img[src]").unwrap();

    document.select(&img_selector)
        .filter_map(|el| el.value().attr("src"))
        .map(String::from)
        .collect()
}
```

**Estimated effort**: 4-6 weeks to port all postprocessors
- Week 1: Core infrastructure (html5ever integration)
- Week 2-3: Port core postprocessors (html.ts, codetools.ts)
- Week 4-5: Port format-specific postprocessors
- Week 6: Testing and edge cases

#### Option 2: lol_html (Cloudflare's streaming parser)

**Crates**:
- `lol_html` - Streaming HTML rewriter

**Pros**:
- ✅ Very fast (streaming, no full DOM tree)
- ✅ Used in production by Cloudflare Workers
- ✅ Low memory footprint

**Cons**:
- ❌ **No CSS selectors** - only tag-based callbacks
- ❌ Cannot query/navigate DOM tree arbitrarily
- ❌ Much harder to port complex manipulations

**Verdict**: Not suitable due to lack of CSS selector support needed for existing logic.

#### Option 3: Embed JavaScript engine (NOT recommended)

**Crates**:
- `deno_core` or `boa` (JS engine in Rust)

**Pros**:
- ✅ Could reuse existing TypeScript code

**Cons**:
- ❌ Defeats purpose of Rust port
- ❌ Massive dependency
- ❌ Performance overhead
- ❌ Serialization boundaries

---

## 2. EJS Templating

### Current Implementation

**Core**: Lodash template engine (EJS-like syntax)
- Location: `src/core/ejs.ts` (88 LOC)
- Uses: `lodash.template()` from `https://cdn.skypack.dev/lodash@4.17.21/`

**Template syntax**:
```ejs
<% const partial = (file, data) => print(include(file, data)); %>
<nav class="navbar">
  <% if (brand) { %>
    <a class="navbar-brand" href="<%= brand.href %>">
      <%= brand.text %>
    </a>
  <% } %>
</nav>
```

**Features**:
- Variable interpolation: `<%= variable %>`
- Code execution: `<% if/for/etc %>`
- Partials: `<% partial('file.ejs', data) %>`
- Caching: Template compilation cache based on mtime

### Template Usage

**Template files**: 20+ EJS files
- `src/resources/formats/html/templates/*.ejs` (10 files)
  - Article structure (before/after body)
  - Custom layouts
- `src/resources/projects/website/templates/*.ejs` (10 files)
  - Navigation bars
  - Footers
  - Search UI
  - Redirects

**Code using `renderEjs()`**: 13 TypeScript files
- `src/project/types/website/listing/website-listing-template.ts` - List pages
- `src/format/html/format-html.ts` - HTML format integration
- `src/format/html/format-html-bootstrap.ts` - Bootstrap components
- `src/project/types/website/website-navigation.ts` - Navigation
- `src/project/types/website/website-sitemap.ts` - Sitemap generation
- `src/format/reveal/format-reveal.ts` - Reveal.js slides

### Rust Port Options

#### Option 1: sailfish (Recommended for performance)

**Crate**: `sailfish`

**Pros**:
- ✅ **Compile-time template compilation** (fastest Rust templating)
- ✅ Very similar syntax to EJS
- ✅ Excellent performance (zero-cost)
- ✅ Type-safe

**Cons**:
- ⚠️ Templates must be known at compile time (not runtime)
- ⚠️ Requires migration from `.ejs` files to Rust code or build.rs

**Example**:
```rust
use sailfish::TemplateOnce;

#[derive(TemplateOnce)]
#[template(path = "navbar.stpl")]
struct NavbarTemplate {
    brand: Option<Brand>,
}

// navbar.stpl:
// <nav class="navbar">
//   <% if let Some(brand) = brand { %>
//     <a class="navbar-brand" href="<%= brand.href %>">
//       <%= brand.text %>
//     </a>
//   <% } %>
// </nav>
```

**Migration strategy**:
1. Convert `.ejs` files to `.stpl` (Sailfish templates)
2. Define Rust structs for template data
3. Embed templates via `include_str!()` or build.rs
4. Pre-compile during build

**Estimated effort**: 3-4 weeks
- Week 1: Core infrastructure, convert core templates
- Week 2: Convert HTML format templates
- Week 3: Convert website templates
- Week 4: Testing, edge cases

#### Option 2: tera (Recommended for flexibility)

**Crate**: `tera`

**Pros**:
- ✅ **Runtime template loading** (like EJS)
- ✅ Jinja2-like syntax (very similar to EJS)
- ✅ Supports partials/includes
- ✅ Template caching built-in
- ✅ Can load templates from filesystem at runtime

**Cons**:
- ⚠️ Slightly slower than sailfish (runtime parsing)
- ⚠️ Different syntax (more verbose)

**Example**:
```rust
use tera::Tera;

let tera = Tera::new("src/resources/**/*.tera")?;

let mut context = tera::Context::new();
context.insert("brand", &brand);

let html = tera.render("navbar.tera", &context)?;
```

**Template** (`navbar.tera`):
```jinja2
<nav class="navbar">
  {% if brand %}
    <a class="navbar-brand" href="{{ brand.href }}">
      {{ brand.text }}
    </a>
  {% endif %}
</nav>
```

**Migration strategy**:
1. Convert `.ejs` syntax to Tera/Jinja2 syntax (mostly automated)
2. Keep templates as files (minimal code changes)
3. Replace `renderEjs()` with `tera.render()`

**Estimated effort**: 2-3 weeks
- Week 1: Convert template syntax (20 templates)
- Week 2: Update all `renderEjs()` call sites (13 files)
- Week 3: Testing

#### Option 3: handlebars-rust

**Crate**: `handlebars`

**Pros**:
- ✅ Handlebars syntax familiar to many developers
- ✅ Runtime template loading

**Cons**:
- ⚠️ Less powerful than EJS (no arbitrary code execution)
- ⚠️ Would require rewriting template logic

**Verdict**: Not recommended - too much rewrite needed.

#### Recommendation: tera

**Reasoning**:
1. **Minimal migration effort** - syntax very close to EJS
2. **Runtime flexibility** - templates can live in resources/ directory
3. **Caching support** - matches current mtime-based caching
4. **Mature** - widely used in Rust web ecosystem

---

## 3. Observable/OJS Compilation

### Current Implementation

**Library**: `@observablehq/parser` (from Skypack CDN)
- Location: `src/execute/ojs/compile.ts`
- Purpose: Parse and compile Observable JavaScript cells
- Import: `import { parseModule } from "observablehq/parser"`

**What it does**:
```typescript
// Parse OJS syntax
const parsed = parseModule(ojsSource);

// Walk AST
ojsSimpleWalker(parsed.cells, {
  VariableDeclaration(node) {
    // Extract variable references
  },
  ImportDeclaration(node) {
    // Extract imports
  }
});
```

**Output**: Generates HTML/JS that runs in browser using Observable Runtime

### Rust Port Options

#### Option 1: Keep JavaScript Parser (Recommended)

**Strategy**: Shell out to Node.js or bundle parser

**Pros**:
- ✅ Zero porting effort
- ✅ Parser is maintained by Observable team
- ✅ OJS already runs in browser (not part of CLI runtime)

**Cons**:
- ⚠️ Requires Node.js or bundled JS engine
- ⚠️ Serialization overhead

**Implementation**:
```rust
use std::process::Command;

fn parse_ojs(source: &str) -> Result<ParsedModule> {
    // Option A: Shell to node
    let output = Command::new("node")
        .arg("tools/ojs-parser.mjs")
        .stdin(source)
        .output()?;

    serde_json::from_slice(&output.stdout)?
}

// Or Option B: Use deno_core to embed JS
```

**Estimated effort**: 1-2 weeks (integration only)

#### Option 2: Port Parser to Rust

**Strategy**: Port `@observablehq/parser` using swc or oxc

**Pros**:
- ✅ Pure Rust
- ✅ No external dependencies

**Cons**:
- ❌ **Very large effort** (thousands of LOC)
- ❌ Must maintain compatibility with Observable
- ❌ Observable syntax evolves

**Estimated effort**: 8-12 weeks (HIGH RISK)

**Verdict**: Not recommended for initial port.

#### Option 3: Embed deno_core

**Crate**: `deno_core`

**Pros**:
- ✅ Can run existing parser code
- ✅ Used by Deno itself

**Cons**:
- ⚠️ Large dependency
- ⚠️ Complex integration

**Verdict**: Possible middle ground if Node shelling is unacceptable.

#### Recommendation: Keep JavaScript Parser (Option 1)

**Reasoning**:
1. OJS is inherently JavaScript - keeping JS parser makes sense
2. Parser is ~1-2% of quarto-cli functionality
3. Can revisit later if needed
4. Observable maintains the parser

**Implementation note**: Bundle parser as single-file ESM module in resources/, execute with Node.js or deno_core.

---

## 4. Browser Automation (Puppeteer)

### Current Implementation

**Library**: Puppeteer (Deno port)
- Location: `src/core/puppeteer.ts` (~400 LOC)
- Import: `import puppeteer from "https://deno.land/x/puppeteer@9.0.2/mod.ts"`

**Uses**:
1. **Mermaid diagrams** (`src/core/handlers/mermaid.ts`)
   - Render Mermaid to PNG/SVG
   - Screenshot diagram elements
2. **Screenshot generation**
   - Extract images from HTML elements
   - Generate previews

**Operations**:
```typescript
const browser = await puppeteer.launch({
  headless: true,
  args: chromeArgs
});
const page = await browser.newPage();
await page.goto(url);
const elements = await page.$$("selector");
await elements[0].screenshot({ path: "out.png" });
```

### Rust Port Options

#### Option 1: headless_chrome (Recommended)

**Crate**: `headless_chrome`

**Pros**:
- ✅ Native Rust bindings to Chrome DevTools Protocol
- ✅ Very similar API to Puppeteer
- ✅ Well-maintained
- ✅ Good performance

**Example**:
```rust
use headless_chrome::{Browser, LaunchOptionsBuilder};

let browser = Browser::new(
    LaunchOptionsBuilder::default()
        .headless(true)
        .build()?
)?;

let tab = browser.new_tab()?;
tab.navigate_to(url)?;
tab.wait_for_element("selector")?
    .screenshot(Path::new("out.png"))?;
```

**Estimated effort**: 2-3 weeks
- Week 1: Core browser automation infrastructure
- Week 2: Port Mermaid rendering
- Week 3: Testing, edge cases

#### Option 2: chromiumoxide

**Crate**: `chromiumoxide`

**Pros**:
- ✅ Async/await based (tokio)
- ✅ More features than headless_chrome

**Cons**:
- ⚠️ Larger API surface
- ⚠️ Less mature

**Verdict**: Viable alternative to headless_chrome.

#### Option 3: Use Chrome/Chromium directly via CDP

**Strategy**: Implement minimal Chrome DevTools Protocol client

**Pros**:
- ✅ Minimal dependencies
- ✅ Only implement what we need

**Cons**:
- ❌ High effort
- ❌ Must maintain CDP integration

**Verdict**: Not recommended - headless_chrome exists.

#### Recommendation: headless_chrome

**Reasoning**:
1. Battle-tested in Rust ecosystem
2. API familiar to TypeScript Puppeteer users
3. Minimal migration effort

---

## Summary Table

| Category | TypeScript Library | Rust Solution | Effort | Risk |
|----------|-------------------|---------------|--------|------|
| **HTML Parsing** | deno-dom | html5ever + scraper | 4-6 weeks | Low |
| **Templating** | lodash.template (EJS) | tera | 2-3 weeks | Low |
| **OJS Parser** | @observablehq/parser | Keep JS (shell to Node) | 1-2 weeks | Low |
| **Browser** | puppeteer | headless_chrome | 2-3 weeks | Low |

**Total estimated effort**: 9-14 weeks

---

## Implementation Phases

### Phase 1: HTML Postprocessing (4-6 weeks)

**Priority**: HIGH (core functionality)

1. Set up html5ever + scraper
2. Create `HtmlPostProcessor` trait:
   ```rust
   pub trait HtmlPostProcessor {
       fn process(&self, doc: &Html, options: &ProcessOptions)
           -> Result<PostProcessResult>;
   }

   pub struct PostProcessResult {
       pub resources: Vec<String>,
       pub supporting: Vec<String>,
   }
   ```
3. Port `discoverResourceRefs()` (validation)
4. Port codetools.ts
5. Port Bootstrap navigation
6. Port website-specific processors

**Validation**: Compare rendered HTML output byte-for-byte with TypeScript version.

### Phase 2: Templating (2-3 weeks)

**Priority**: HIGH (widespread usage)

1. Set up tera
2. Convert `.ejs` templates to `.tera` templates (script this)
3. Create `render_template()` function:
   ```rust
   pub fn render_template(
       template: &str,
       data: &impl Serialize,
   ) -> Result<String>
   ```
4. Replace `renderEjs()` calls in 13 files
5. Test all formats (HTML, websites, books)

**Validation**: Diff template output with TypeScript version.

### Phase 3: Browser Automation (2-3 weeks)

**Priority**: MEDIUM (only for diagrams)

1. Set up headless_chrome
2. Port Mermaid handler
3. Port screenshot utilities
4. Add Chrome binary detection/download logic

**Validation**: Render Mermaid diagrams, compare PNG output.

### Phase 4: OJS Integration (1-2 weeks)

**Priority**: LOW (can defer)

1. Bundle `@observablehq/parser` as ESM module
2. Shell to Node.js to parse OJS
3. Parse JSON output
4. Generate HTML (already in Rust)

**Validation**: Render OJS documents, compare output.

---

## Additional JavaScript Dependencies from import_map.json

The following are **NOT runtime dependencies** but used during build/development:

- **lodash** - Utility library (many uses via `ld.template()`)
  - Rust equivalent: Write helpers as needed, or use `itertools`, `rayon`

- **js-yaml** - YAML parsing
  - Already analyzed separately (use yaml-rust2)

- **acorn** - JavaScript parser (for OJS)
  - Keep as-is (see OJS section)

- **scss-parser** - SASS/SCSS parsing
  - Rust equivalent: `grass` (already handles SCSS)

- **diff** - Text diffing
  - Rust equivalent: `similar` or `diff`

**These can be addressed as needed** - not critical path for Rust port.

---

## Risks and Mitigation

### Risk 1: HTML Manipulation Edge Cases

**Risk**: scraper API differences cause subtle bugs

**Mitigation**:
- Comprehensive test suite comparing outputs
- Port incrementally, validate each postprocessor
- Keep TypeScript version as reference

### Risk 2: Template Syntax Incompatibilities

**Risk**: EJS → Tera conversion misses edge cases

**Mitigation**:
- Automated conversion script with validation
- Side-by-side testing
- Template count is manageable (20 files)

### Risk 3: Chrome Binary Availability

**Risk**: headless_chrome requires Chrome/Chromium binary

**Mitigation**:
- Auto-download Chrome binary (like Puppeteer does)
- Use `puppeteer-core` approach (BYO Chrome)
- Fallback to system Chrome if available

### Risk 4: OJS Parser Drift

**Risk**: Observable changes parser API

**Mitigation**:
- Pin parser version initially
- Consider porting if becomes critical
- Observable is stable (low change frequency)

---

## Deferred Concerns

The following are **client-side JavaScript** (not CLI dependencies):

- Observable Runtime (runs in browser)
- Bootstrap JS (runs in browser)
- Mermaid rendering JS (runs in browser)
- Reveal.js (runs in browser)

These remain JavaScript - the CLI only generates HTML that loads them.

---

## Conclusion

JavaScript runtime dependencies in quarto-cli are **highly portable to Rust**:

1. **HTML postprocessing**: html5ever + scraper (industry standard)
2. **Templating**: tera (minimal syntax changes)
3. **OJS parsing**: Keep JavaScript parser (shell to Node)
4. **Browser automation**: headless_chrome (similar API to Puppeteer)

**Total effort**: 9-14 weeks (can parallelize some work)

**Recommended order**:
1. HTML postprocessing (highest impact, most usage)
2. Templating (widespread, well-scoped)
3. Browser automation (isolated, medium impact)
4. OJS integration (low impact, can defer)

All solutions use mature, battle-tested Rust crates with active maintenance.
