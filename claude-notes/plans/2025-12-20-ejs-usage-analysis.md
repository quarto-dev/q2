# EJS Template Usage Analysis in quarto-cli

## Executive Summary

**Can we skip EJS entirely for a minimal website?**

- **For a website WITH navigation:** No, the navigation structure is generated via EJS templates
- **For a website WITHOUT navigation:** Yes, we could use static HTML wrappers
- **Recommended approach:** Either pre-generate navigation HTML for common cases OR implement a minimal EJS-compatible renderer in Rust

---

## Complete EJS Usage Inventory

### 1. Core HTML Format (`format-html.ts`, `format-html-bootstrap.ts`)

| Template | Purpose | Required for Minimal? |
|----------|---------|----------------------|
| `quarto-html-before-body.ejs` | Dark mode switching script | No (only if dark mode enabled) |
| `quarto-html-after-body.ejs` | Feature init (anchors, copy code, tooltips, hover citations, code annotations) | Partially (anchors, copy code useful) |
| `before-body-article.ejs` | Page layout wrapper | Yes (trivial - just opens divs) |
| `after-body-article-preamble.ejs` | Close main tag | Yes (trivial - `</main>`) |
| `after-body-article-postamble.ejs` | Close content div | Yes (trivial - `</div>`) |
| `hypothesis/hypothesis.ejs` | Hypothesis integration | No |
| `utterances/utterances.ejs` | Utterances comments | No |
| `giscus/giscus.ejs` | Giscus comments | No |

### 2. Website Navigation (`website-navigation.ts`) - CRITICAL

| Template | Purpose | Required for Minimal? |
|----------|---------|----------------------|
| `nav-before-body.ejs` | Complete navigation wrapper (navbar + sidebar + content structure) | **YES** |
| `nav-after-body-preamble.ejs` | Close main tag | Yes |
| `nav-after-body-postamble.ejs` | Page nav (prev/next) + footer | Yes (for pagination/footer) |
| `redirect-simple.ejs` | Redirect pages | No |

**Partials included by nav-before-body.ejs:**
- `navbrand.ejs` - Logo/brand in navbar
- `navitem.ejs` - Individual nav item
- `navitem-dropdown.ejs` - Dropdown nav item
- `navitems.ejs` - List of nav items
- `navsearch.ejs` - Search button
- `navtoggle.ejs` - Mobile menu toggle
- `navcollapse.ejs` - Collapsible navbar section
- `navtools.ejs` - Navbar tools (theme toggle, etc.)
- `navdarktoggle.ejs` - Dark mode toggle
- `navreadertoggle.ejs` - Reader mode toggle
- `sidebar.ejs` - Full sidebar
- `sidebaritem.ejs` - Sidebar item (recursive)

**Partials included by nav-after-body-postamble.ejs:**
- `nav-footer-section.ejs` - Footer section
- `nav-footer-navitem.ejs` - Footer nav item

### 3. Website Optional Features

| Feature | Template(s) | Required? |
|---------|-------------|-----------|
| Sitemap | `sitemap.ejs.xml` | No |
| Redirects | `redirect-map.ejs` | No |
| Listing pages | `listing-*.ejs.md`, `_filter.ejs.md`, `_metadata.ejs.md`, `_pagination.ejs.md` | No |
| RSS feeds | `feed/*.ejs.md` | No |
| About pages | `jolla.ejs.html`, `trestles.ejs.html`, etc. | No |

---

## EJS Syntax Used

The templates use a limited subset of EJS:

```javascript
// Control flow
<% if (condition) { %>
  content
<% } %>

<% items.forEach(item => { %>
  content with <%- item.prop %>
<% }) %>

// Output
<%= escaped %>     // HTML escaped
<%- unescaped %>   // Raw HTML

// Partials (custom extension)
<% partial('template.ejs', { data }) %>
```

This is implemented via Lodash's `_.template()` with a custom `partial()` helper.

---

## What Navigation Templates Actually Generate

### nav-before-body.ejs Output (Navbar Example)

```html
<div id="quarto-search-results"></div>
<header id="quarto-header" class="headroom fixed-top">
  <nav class="navbar navbar-expand-lg" data-bs-theme="dark">
    <div class="navbar-container container-fluid">
      <div class="navbar-brand-container mx-auto">
        <a class="navbar-brand" href="./index.html">
          <span class="navbar-title">Site Title</span>
        </a>
      </div>
      <div id="quarto-search" class="" title="Search"></div>
      <button class="navbar-toggler" type="button" ...>
        <span class="navbar-toggler-icon"></span>
      </button>
      <div class="collapse navbar-collapse" id="navbarCollapse">
        <ul class="navbar-nav navbar-nav-scroll me-auto">
          <li class="nav-item">
            <a class="nav-link active" href="./index.html">
              <span class="menu-text">Home</span>
            </a>
          </li>
          <!-- more nav items -->
        </ul>
      </div>
      <div class="quarto-navbar-tools"></div>
    </div>
  </nav>
</header>
<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-article page-navbar">
  <div id="quarto-margin-sidebar" class="sidebar margin-sidebar zindex-bottom"></div>
  <main class="content" id="quarto-document-content">
```

### nav-after-body-postamble.ejs Output (Footer/Pagination)

```html
</main>
<nav class="page-navigation">
  <div class="nav-page nav-page-previous">
    <a href="./prev.html" class="pagination-link">
      <i class="bi bi-arrow-left-short"></i>
      <span class="nav-page-text">Previous</span>
    </a>
  </div>
  <div class="nav-page nav-page-next">
    <a href="./next.html" class="pagination-link">
      <span class="nav-page-text">Next</span>
      <i class="bi bi-arrow-right-short"></i>
    </a>
  </div>
</nav>
</div>
<footer class="footer">
  <div class="nav-footer">
    <div class="nav-footer-left">Footer content</div>
    <div class="nav-footer-center">&nbsp;</div>
    <div class="nav-footer-right">&nbsp;</div>
  </div>
</footer>
```

---

## Implementation Strategies

### Strategy 1: Static Pre-generated Templates

**Approach:** Pre-generate navigation HTML for specific configurations

**Pros:**
- No template engine needed
- Fastest to implement
- Works for simple sites

**Cons:**
- Only works for specific navigation structures
- Need to generate variants for different configs
- Not flexible

**Use case:** Minimal prototype, testing

### Strategy 2: Hardcoded Navigation Builder in Rust

**Approach:** Write Rust code that generates navigation HTML directly from config

```rust
fn render_navbar(config: &WebsiteConfig) -> String {
    let mut html = String::new();
    html.push_str(r#"<header id="quarto-header" class="headroom fixed-top">"#);
    html.push_str(r#"<nav class="navbar navbar-expand-lg">"#);
    // ... build from config
}
```

**Pros:**
- No separate template files
- Type-safe
- Full control

**Cons:**
- HTML embedded in Rust code (hard to maintain)
- Need to reimplement each template
- Lots of string concatenation

**Use case:** If we only need basic navigation and won't change it much

### Strategy 3: Tera Templates (Rust-native)

**Approach:** Port EJS templates to Tera (Jinja2-like Rust template engine)

```jinja
{# nav-before-body.tera #}
<header id="quarto-header" class="headroom fixed-top">
{% if navbar %}
  <nav class="navbar navbar-expand-lg">
    {% include "navbrand.tera" %}
    {% for item in navbar.left %}
      {% include "navitem.tera" %}
    {% endfor %}
  </nav>
{% endif %}
</header>
```

**Pros:**
- Maintained Rust crate (`tera`)
- Similar syntax to EJS (easy to port)
- Template files separate from code
- Supports includes, loops, conditionals

**Cons:**
- Need to port all templates (~20 files)
- Different syntax (minor changes)
- Additional dependency

**Use case:** Full-featured website rendering

### Strategy 4: Minimal EJS Interpreter in Rust

**Approach:** Implement the EJS subset used by Quarto in pure Rust

Required features:
- `<% code %>` - Execute code
- `<%= expr %>` - Escaped output
- `<%- expr %>` - Raw output
- `partial(file, data)` - Include partial

**Pros:**
- Can use existing EJS templates directly
- Only need to implement subset actually used

**Cons:**
- Need to implement JavaScript evaluation (complex!)
- Significant effort
- Would essentially be writing a JS interpreter

**Use case:** Maximum compatibility (probably not worth it in pure Rust)

### Strategy 5: Embedded QuickJS via Rust Bindings

**Approach:** Use a Rust wrapper for QuickJS to run EJS templates natively

QuickJS is a small, embeddable JavaScript engine. Several Rust crates provide bindings:

| Crate | Status | Notes |
|-------|--------|-------|
| [rquickjs](https://crates.io/crates/rquickjs) | **Most active** | High-level safe bindings to QuickJS-NG, async support, v0.10.0 (Oct 2025) |
| [quickjs-rusty](https://crates.io/crates/quickjs-rusty) | Active | Focus on Rust-JS type conversion, ES2023 support |
| [quickjs_runtime](https://lib.rs/crates/quickjs_runtime) | Active | Event loop based, supports both original QuickJS and QuickJS-NG |

**Example implementation:**

```rust
use rquickjs::{Runtime, Context, Function};

pub struct EjsRenderer {
    runtime: Runtime,
}

impl EjsRenderer {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;
        Ok(Self { runtime })
    }

    pub fn render(&self, template: &str, data: &serde_json::Value) -> Result<String> {
        let ctx = Context::full(&self.runtime)?;

        ctx.with(|ctx| {
            // Load Lodash's _.template() function (or minimal subset)
            ctx.eval(include_str!("lodash-template.min.js"))?;

            // Define the partial() helper
            ctx.eval(r#"
                const partialCache = {};
                function partial(file, data) {
                    // Would need to hook into Rust for file loading
                    return renderPartial(file, data);
                }
            "#)?;

            // Compile and execute template
            let compile: Function = ctx.eval(format!(
                "_.template(`{}`)",
                template.replace('`', "\\`")
            ))?;

            let result: String = compile.call((data,))?;
            Ok(result)
        })
    }
}
```

**Pros:**
- Can use existing EJS templates **verbatim** (no porting needed)
- Full JavaScript compatibility
- Lodash's `_.template()` works as-is
- QuickJS is small and fast
- Well-maintained Rust bindings

**Cons:**
- Adds ~1-2MB to binary size
- JavaScript engine dependency
- Need to bundle Lodash (or minimal subset)
- Cross-language data marshaling overhead

**Use case:**
- When you want **exact compatibility** with quarto-cli templates
- Good middle ground: use QuickJS only for navigation templates (complex), keep everything else in pure Rust
- Useful if templates change upstream - no need to re-port

**Binary size note:** QuickJS itself is ~700KB. With rquickjs bindings and Lodash template subset, expect ~1-2MB total impact.

---

## Recommended Approach for Minimal Render

### Phase 1: No Navigation (Immediate)

Skip all website EJS templates. Output basic HTML with:
- Bootstrap body envelope (before-body-article.ejs equivalent - hardcoded)
- No navbar or sidebar
- Document content only

```rust
const BEFORE_BODY: &str = r#"
<div id="quarto-content" class="page-columns page-rows-contents page-layout-article">
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar"></div>
<main class="content" id="quarto-document-content">
"#;

const AFTER_BODY: &str = r#"
</main>
</div>
"#;
```

### Phase 2: Basic Navbar (Short-term)

Implement hardcoded Rust functions that generate navbar HTML from config:

```rust
pub fn render_navigation(config: &WebsiteNavConfig) -> BodyEnvelope {
    BodyEnvelope {
        before: render_navbar(&config.navbar) + render_content_wrapper(),
        after_preamble: "</main>".to_string(),
        after_postamble: render_page_nav(&config) + "</div>" + render_footer(&config.footer),
    }
}
```

### Phase 3: Tera Templates (Medium-term)

Port EJS templates to Tera for full flexibility. This can be done incrementally:
1. Start with the main navigation templates
2. Add partials as needed
3. Eventually achieve full feature parity

---

## Data Flow for Navigation

```
_quarto.yml
    ↓
ProjectContext.config
    ↓
websiteNavigationConfig()  → { navbar, sidebars, footer, ... }
    ↓
navbarEjsData() / sidebarsEjsData()  → resolved navigation items
    ↓
renderEjs(nav-before-body.ejs, { nav })  → HTML string
    ↓
bodyEnvelope.before  → Pandoc include-before-body
```

The `nav` object passed to templates contains:
- `navbar`: { left: NavItem[], right: NavItem[], tools: [], collapse: bool, ... }
- `sidebar`: { title, logo, contents: SidebarItem[], search: bool, ... }
- `footer`: { left, center, right }
- `layout`: "article" | "full" | "custom"
- `language`: localization strings
- `hasToc`: boolean
- `prevPage`, `nextPage`: pagination links
- etc.

---

## Conclusion

For a minimal Quarto website render:

1. **Immediate:** Use static body envelope (no navigation) - works but minimal
2. **Short-term:** Hardcode navbar generation in Rust for basic cases
3. **Medium-term:** Choose between:
   - **Tera templates** - Port EJS to Rust-native templates (more work upfront, pure Rust)
   - **QuickJS** - Embed JS engine to run EJS verbatim (less work, adds dependency)

The EJS templates themselves are well-structured and the logic is straightforward. The main complexity is in the data preparation (`navbarEjsData`, `sidebarsEjsData`, etc.) which happens in TypeScript before templates are rendered.

### Decision Matrix

| Approach | Upfront Effort | Maintenance | Binary Size | Compatibility |
|----------|---------------|-------------|-------------|---------------|
| Static HTML | Low | Low | None | Minimal |
| Hardcoded Rust | Medium | Medium | None | Basic |
| Tera templates | High | Low | ~100KB | Full (ported) |
| QuickJS + EJS | Medium | Low | ~1-2MB | Exact |

**Recommendation:** If staying in sync with upstream quarto-cli templates is important, QuickJS is attractive because template changes can be pulled in without re-porting. If minimal dependencies and binary size are priorities, Tera is the better choice.
