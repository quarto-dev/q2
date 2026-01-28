# Phase 6: Table of Contents Rendering

**Parent Plan**: [`2026-01-24-html-rendering-parity.md`](./2026-01-24-html-rendering-parity.md)
**Beads Issue**: kyoto-b48
**Created**: 2026-01-28
**Status**: In Progress (Phases 6.0, 6.1, 6.3 pipeline work complete)

---

## Overview

This phase implements Table of Contents (TOC) generation for Quarto HTML output. The TOC is generated from document headings and rendered as a navigable list that integrates with client-side JavaScript for scroll tracking.

**Key insight**: Since we control pampa (unlike TS Quarto which uses Pandoc as a black box), we have the opportunity to implement TOC generation with more customization options than Pandoc currently offers.

---

## Research: How Pandoc Implements TOC

### Architecture: General Mechanism, Not Writer-Specific

Pandoc's TOC generation is a **general mechanism** shared across all writers, not HTML-specific:

```
Document blocks
    ↓
makeSections()  [Shared.hs]
    (wraps Headers in Div sections with IDs)
    ↓
toTOCTree()  [Chunks.hs]
    (builds tree structure from sections)
    ↓
tocToList()  [Chunks.hs]
    (converts tree to BulletList block)
    ↓
Writer renders BulletList
    (HTML, LaTeX, etc. each render differently)
```

### Key Functions

**`toTableOfContents`** (Shared.hs:658-665) - Main entry point:
```haskell
toTableOfContents :: WriterOptions -> [Block] -> Block
toTableOfContents opts =
  tocToList (writerNumberSections opts) (writerTOCDepth opts)
  . toTOCTree
  . makeSections (writerNumberSections opts) Nothing
```

**`toTOCTree`** (Chunks.hs:368-385) - Builds tree from sections:
- Walks section Divs created by `makeSections`
- Creates hierarchical `Tree SecInfo` structure
- Filters out headings with `unlisted` class (unless they have explicit `number` attribute)

**`tocToList`** (Chunks.hs:435-444) - Converts tree to BulletList:
- Respects `toc-depth` to limit nesting
- Creates `Link` elements pointing to section IDs
- Optionally includes section numbers

### Data Structure: SecInfo

```haskell
data SecInfo = SecInfo
  { secTitle  :: [Inline]    -- heading text (inlines for formatting)
  , secNumber :: Maybe Text  -- section number (e.g., "1.2.3")
  , secId     :: Text        -- section identifier for linking
  , secPath   :: Text        -- path to chunk (for chunked docs)
  , secLevel  :: Int         -- heading level (1-6)
  }
```

### Customization via Classes

| Class | Effect |
|-------|--------|
| `unlisted` | Excluded from TOC (but still rendered in document) |
| `unnumbered` | Included in TOC but without section number |
| `unlisted unnumbered` | Excluded from TOC, no number |

**Logic**: Include in TOC UNLESS `unlisted` AND no explicit `number` attribute.

### Writer-Specific Handling

While the TOC block is generated generically, writers can:
- **Override rendering** (LaTeX uses `\tableofcontents` command instead)
- **Add attributes** (HTML adds `role="doc-toc"`, `id="TOC"`)
- **Customize structure** (ConTeXt has special handling)

### TOC Generation Timing and Lua Filter Limitations

**Critical insight**: In Pandoc, TOC generation happens **inside the writer**, after Lua filters have completed.

**Pipeline order** (from `Text/Pandoc/App.hs`):

```
1. Read input document
2. Adjust metadata
3. Apply Lua/JSON filters          ← FILTERS RUN HERE
4. Apply built-in transforms
5. Call writer function
   └─ HTML writer generates TOC    ← TOC GENERATED HERE (inside writer)
6. Return output
```

**What this means for Lua filters:**

Lua filters can affect TOC content **indirectly**:
- ✅ Add/remove headers → affects what's in TOC
- ✅ Add `unlisted` class to headers → excludes them from TOC
- ✅ Modify header text → changes TOC entry text
- ✅ Reorder sections → TOC reflects new order

But Lua filters **cannot** modify the TOC structure directly:
- ❌ Reorder TOC entries independently of document structure
- ❌ Add custom attributes to TOC links
- ❌ Change TOC nesting independently of heading levels
- ❌ Insert non-heading items into TOC
- ❌ Apply custom formatting to specific TOC entries

**Why?** The TOC BulletList doesn't exist when filters run. It's generated inside `writeHtml5`/`writeHtml4` from the already-filtered document. By the time the TOC exists, there's no opportunity for user code to modify it.

**Code evidence** (HTML.hs:300-302):
```haskell
toc <- if writerTableOfContents opts && slideVariant /= S5Slides
         then fmap layoutMarkup <$> tableOfContents opts sects
         else return Nothing
```

This is called inside the writer, after all filters have completed.

### Opportunity: AST-First TOC in Rust Quarto

Since we control the pipeline, we can implement TOC generation as an **AST transform** rather than inside the writer. This enables:

1. **TOC as AST Block**: Generate the TOC as a `Div` containing a `BulletList` (or similar structure) and insert it into the document AST
2. **User filter entry point**: Create a filter entry point (like `quarto-post-toc`) that runs after TOC generation but before format-specific transforms
3. **Full customization**: Lua filters at this entry point can modify the TOC structure directly

**Proposed pipeline position**:

```
Format-agnostic transforms:
  ├─ MetadataNormalizeTransform
  ├─ CalloutTransform
  ├─ TitleBlockTransform
  ├─ SectionizeTransform
  ├─ ... other format-agnostic transforms ...
  ├─ TocGenerateTransform          ← navigation.toc created here
  ├─ [User filter entry point]     ← Users can modify navigation.toc here
  │
Format-specific transforms:
  ├─ TocRenderTransform            ← rendered.navigation.toc created here
  ├─ ... other HTML-specific transforms ...
  └─ Writer/Template (stateless, just renders)
```

**Benefits**:
- Users can write Lua filters that modify TOC structure
- TOC is a first-class AST element, not a writer-internal construct
- Same TOC structure available to all output formats
- Consistent with our AST-first architecture

**Example user customization** (future):
```lua
-- In a quarto-post-toc filter
function Div(el)
  if el.identifier == "TOC" then
    -- Modify TOC entries, add custom attributes, reorder, etc.
    return modified_toc
  end
end
```

**Note**: The exact filter entry point naming and positioning is a future design decision. The key architectural choice is that TOC generation is an AST transform, not embedded in the writer.

---

## Implementation Notes (Pre-Implementation Review)

This section documents corrections and clarifications identified during pre-implementation review.

### API Names

The actual codebase uses:
- **`RenderContext`** (not `TransformContext`) - defined in `crates/quarto-core/src/render.rs`
- **`ctx.format_metadata("key")`** returns `Option<&serde_json::Value>` - call `.as_str()`, `.as_bool()`, etc. to extract values

### ConfigValue Path Methods (Prerequisite)

The plan assumes methods that don't yet exist on `ConfigValue`. Before implementation, add these to `crates/quarto-pandoc-types/src/config_value.rs`:

```rust
impl ConfigValue {
    /// Get a value by path (e.g., ["navigation", "toc"])
    pub fn get_path(&self, path: &[&str]) -> Option<&ConfigValue> {
        let mut current = self;
        for key in path {
            current = current.get(key)?;
        }
        Some(current)
    }

    /// Check if a path exists
    pub fn contains_path(&self, path: &[&str]) -> bool {
        self.get_path(path).is_some()
    }

    /// Insert a value at a path, creating intermediate maps as needed
    pub fn insert_path(&mut self, path: &[&str], value: ConfigValue) {
        // Navigate/create intermediate maps, insert at leaf
        // Implementation details TBD
    }
}
```

### Architecture: pampa Core Logic + quarto-core Wrapper

**Design rationale**: `pampa --toc` should work standalone (like `pandoc --toc`), and pampa cannot depend on quarto-core.

**Pattern** (same as SectionizeTransform):

```rust
// In pampa - core logic and data structures
// Location: crates/pampa/src/toc.rs

pub struct TocConfig {
    pub depth: i32,
    pub title: Option<String>,
}

pub fn generate_toc(blocks: &[Block], config: &TocConfig) -> NavigationToc {
    // Pure function - no RenderContext dependency
}

// In quarto-core - thin wrapper implementing AstTransform
// Location: crates/quarto-core/src/transforms/toc.rs

pub struct TocGenerateTransform;

impl AstTransform for TocGenerateTransform {
    fn name(&self) -> &str { "toc-generate" }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Read config from RenderContext, call pampa's generate_toc()
        let config = TocConfig {
            depth: ctx.format_metadata("toc-depth")
                .and_then(|v| v.as_i64())
                .unwrap_or(3) as i32,
            title: ctx.format_metadata("toc-title")
                .and_then(|v| v.as_str())
                .map(String::from),
        };
        let toc = pampa::toc::generate_toc(&ast.blocks, &config);
        // Insert into ast.meta...
        Ok(())
    }
}
```

**In pampa standalone**: Config comes from document frontmatter (`ast.meta`)
**In quarto-core pipeline**: Config comes from `RenderContext` (format metadata)

### Type Handling for `toc` Configuration

**IMPORTANT**: YAML `true` (boolean) and `"true"` (string) are different types.

Accept only:
- `toc: true` (boolean) - standard Pandoc/TS Quarto compatibility
- `toc: auto` (string) - explicit auto-generation request
- `toc: false` (boolean) - disable TOC

**Do NOT** accept `toc: "true"` (string). Schema validation will catch this as a type error.

```rust
let should_generate = match ctx.format_metadata("toc") {
    Some(v) if v.as_bool() == Some(true) => true,
    Some(v) if v.as_str() == Some("auto") => true,
    _ => false,
};
```

### Two-Stage Rendering Architecture

**Design principle**: Templates decide coarse structure; complex format-specific logic lives in transforms.

Rather than using recursive template partials to render nested TOC structures, we use a **two-stage transform pipeline**:

1. **TocGenerateTransform** (format-agnostic): Walks headings, creates `navigation.toc` metadata structure
2. **TocRenderTransform** (format-specific): Reads `navigation.toc`, renders to HTML, stores at `rendered.navigation.toc`

This approach:
- Keeps templates simple (just `$rendered.navigation.toc$`)
- Puts complex recursive rendering logic in Rust (testable, debuggable)
- Allows users to override at multiple points
- Works without recursive partial support in templates

---

## Design: Rust Quarto TOC Implementation

### Core Principle: Writers Are Metadata-Driven

**The key design principle**: Writers render `navigation.toc` if it exists in metadata. They don't know or care where it came from.

This enables three sources for TOC data:
1. **TocGenerateTransform** - Automatic generation when `toc: auto`
2. **User's Lua filter** - Programmatic generation or modification
3. **Hand-written metadata** - Full manual control without any code

### Architecture: Two-Stage Transform Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CONFIGURATION                                      │
│  format:                                                                     │
│    html:                                                                     │
│      toc: auto          ← Request auto-generation                           │
│      toc-depth: 3       ← Configuration for the transform                   │
│      toc-title: "Contents"                                                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                      ┌─────────────────────────────┐
                      │   TocGenerateTransform      │
                      │   (format-agnostic)         │
                      │                             │
                      │   Skips if navigation.toc   │
                      │   already exists            │
                      └─────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                     STRUCTURED DATA (navigation.toc)                         │
│  navigation:                                                                 │
│    toc:                                                                      │
│      title: "Contents"                                                       │
│      entries:                                                                │
│        - id: "introduction"                                                  │
│          title: "Introduction"                                               │
│          level: 1                                                            │
│          number: "1"                                                         │
│          children: [...]                                                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                      ┌─────────────────────────────┐
                      │   TocRenderTransform        │
                      │   (format-specific: HTML)   │
                      │                             │
                      │   Skips if                  │
                      │   rendered.navigation.toc   │
                      │   already exists            │
                      └─────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                     RENDERED HTML (rendered.navigation.toc)                  │
│  rendered:                                                                   │
│    navigation:                                                               │
│      toc: "<ul><li><a href=\"#intro\">...</a></li>...</ul>"                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                            Template (simple)
                      $rendered.navigation.toc$
```

**User override points** (from most processed to least):

| Override | What user provides | Effect |
|----------|-------------------|--------|
| `rendered.navigation.toc` | Pre-rendered HTML string | Skip both transforms |
| `navigation.toc` | Structured TOC data | Skip TocGenerateTransform, run TocRenderTransform |
| Document headings only | Source headings | Run both transforms |

### Configuration Options

| Key | Values | Purpose |
|-----|--------|---------|
| `format.html.toc` | `auto`, `true`, `false` | Enable/disable TOC generation |
| `format.html.toc-depth` | `1`-`6` (default: `3`) | Max heading level to include |
| `format.html.toc-title` | string | Title text for TOC |
| `format.html.toc-location` | `body`, `left`, `right` | Where to render (Phase 8 for sidebars) |

**Note**: `toc: auto` is the preferred syntax. `toc: true` is supported as a synonym for backwards compatibility but may be deprecated in the future.

### Navigation Metadata Namespace

The `navigation` top-level key is reserved for all navigation scaffolding:

```yaml
navigation:
  toc:                    # In-document table of contents
    title: "Contents"
    entries: [...]

  # Future navigation types:
  sidebar: [...]          # Website sidebar
  navbar: [...]           # Website navbar
  pagination:             # Prev/next links
    prev: { ... }
    next: { ... }
  breadcrumbs: [...]      # Breadcrumb trail
```

### Conflict Resolution

**When both `toc: auto` AND `navigation.toc` exist:**

1. TocGenerateTransform detects existing `navigation.toc` metadata
2. Issues a warning: "navigation.toc already exists, skipping auto-generation"
3. Does nothing (existing metadata takes precedence)

This allows users to:
- Write `navigation.toc` by hand for full control
- Use a Lua filter to generate custom TOC
- Clear `navigation.toc` in an earlier filter if they want auto-generation to run

```rust
impl AstTransform for TocGenerateTransform {
    fn transform(&self, doc: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Check if auto-generation is requested
        // Note: toc: true (boolean) or toc: "auto" (string), NOT toc: "true"
        let should_generate = match ctx.format_metadata("toc") {
            Some(v) if v.as_bool() == Some(true) => true,
            Some(v) if v.as_str() == Some("auto") => true,
            _ => false,
        };

        if !should_generate {
            return Ok(());
        }

        // Check if navigation.toc already exists
        if doc.meta.contains_path(&["navigation", "toc"]) {
            // TODO: emit warning via appropriate mechanism
            // "navigation.toc already exists in metadata, skipping auto-generation."
            return Ok(());
        }

        // Generate TOC from headings...
    }
}
```

### Hand-Written TOC Example

Users can bypass all transforms and write TOC metadata directly:

```yaml
---
title: "My Document"
navigation:
  toc:
    title: "Quick Links"
    entries:
      - id: "tldr"
        title: "TL;DR"
        level: 1
      - id: "details"
        title: "The Details"
        level: 1
        children:
          - id: "part-a"
            title: "Part A"
            level: 2
---

## TL;DR {#tldr}
...

## The Details {#details}

### Part A {#part-a}
...
```

The writer sees `navigation.toc` and renders it - no transform needed.

### pampa CLI Integration

The `--toc` flag is implemented as an early metadata transform:

```rust
// In pampa binary, before other processing
if args.toc {
    // Insert toc: auto into format.html metadata
    doc.meta.insert_path("format.html.toc", MetaValue::String("auto".into()));
}
```

This ensures uniform code paths regardless of how TOC is enabled.

### Data Structures

```rust
/// Information about a heading for TOC generation.
/// Serializes to the `navigation.toc.entries` metadata structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TocEntry {
    /// Section ID for linking (e.g., "introduction")
    pub id: String,
    /// Heading text (as string; could be inlines for rich formatting in future)
    pub title: String,
    /// Heading level (1-6)
    pub level: i32,
    /// Section number if numbering enabled (e.g., "1.2.3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// Child entries (nested headings)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TocEntry>,
}

/// Complete TOC structure stored at `navigation.toc` in document metadata.
/// This is the generated data that writers consume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationToc {
    /// Title for the TOC (e.g., "Table of Contents")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Root entries
    pub entries: Vec<TocEntry>,
}
```

### Transform Pipeline

**Note**: See "Implementation Notes" section above for the pampa/quarto-core split.

#### Stage 1: TocGenerateTransform (format-agnostic)

```rust
/// quarto-core wrapper - reads config from RenderContext, calls pampa's generate_toc()
pub struct TocGenerateTransform;

impl AstTransform for TocGenerateTransform {
    fn name(&self) -> &str { "toc-generate" }

    fn transform(&self, doc: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Check if auto-generation is requested
        let should_generate = match ctx.format_metadata("toc") {
            Some(v) if v.as_bool() == Some(true) => true,
            Some(v) if v.as_str() == Some("auto") => true,
            _ => false,
        };

        if !should_generate {
            return Ok(());
        }

        // Check if navigation.toc already exists (user-provided or from earlier filter)
        if doc.meta.contains_path(&["navigation", "toc"]) {
            // TODO: emit warning via appropriate mechanism
            return Ok(());
        }

        // Read configuration from format metadata
        let config = pampa::toc::TocConfig {
            depth: ctx.format_metadata("toc-depth")
                .and_then(|v| v.as_i64())
                .unwrap_or(3) as i32,
            title: ctx.format_metadata("toc-title")
                .and_then(|v| v.as_str())
                .map(String::from),
        };

        // Call pampa's core TOC generation logic
        let toc = pampa::toc::generate_toc(&doc.blocks, &config);

        // Store TOC data at navigation.toc
        doc.meta.insert_path(&["navigation", "toc"], toc.to_config_value());

        Ok(())
    }
}
```

#### Stage 2: TocRenderTransform (format-specific)

```rust
/// Renders navigation.toc to HTML and stores at rendered.navigation.toc
pub struct TocRenderTransform;

impl AstTransform for TocRenderTransform {
    fn name(&self) -> &str { "toc-render" }

    fn transform(&self, doc: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Skip if already rendered (user provided pre-rendered HTML)
        if doc.meta.contains_path(&["rendered", "navigation", "toc"]) {
            return Ok(());
        }

        // Skip if no TOC data to render
        let Some(toc_data) = doc.meta.get_path(&["navigation", "toc"]) else {
            return Ok(());
        };

        // Parse the TOC structure and render to HTML
        let toc = NavigationToc::from_config_value(toc_data)?;
        let html = render_toc_entries_to_html(&toc.entries);

        // Store rendered HTML
        doc.meta.insert_path(
            &["rendered", "navigation", "toc"],
            ConfigValue::new_string(&html, dummy_source_info()),
        );

        Ok(())
    }
}

/// Render TOC entries to HTML string (recursive)
fn render_toc_entries_to_html(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut html = String::from("<ul>\n");
    for entry in entries {
        html.push_str("  <li>\n");
        html.push_str(&format!(
            "    <a href=\"#{}\" class=\"nav-link\" data-scroll-target=\"#{}\">\n",
            entry.id, entry.id
        ));
        if let Some(ref number) = entry.number {
            html.push_str(&format!("      <span class=\"toc-number\">{}</span> ", number));
        }
        html.push_str(&html_escape(&entry.title));
        html.push_str("\n    </a>\n");

        if !entry.children.is_empty() {
            // Recursive call for nested entries
            html.push_str(&render_toc_entries_to_html(&entry.children));
        }
        html.push_str("  </li>\n");
    }
    html.push_str("</ul>\n");
    html
}
```

#### Core TOC Generation (in pampa)

**In pampa** (`crates/pampa/src/toc.rs`):

```rust
/// Configuration for TOC generation
pub struct TocConfig {
    pub depth: i32,
    pub title: Option<String>,
}

/// Generate TOC from document blocks (pure function, no RenderContext dependency)
pub fn generate_toc(blocks: &[Block], config: &TocConfig) -> NavigationToc {
    let entries = collect_toc_entries(blocks, config.depth);
    NavigationToc {
        title: config.title.clone(),
        entries,
    }
}

fn collect_toc_entries(blocks: &[Block], max_depth: i32) -> Vec<TocEntry> {
    // Walk blocks, find Headers (or section Divs if sectionize ran first)
    // Filter out headings with `unlisted` class
    // Preserve `unnumbered` class (include without number)
    // Build hierarchical structure based on levels
    // Respect max_depth limit
    // ...
}
```

### HTML Template Integration

With the two-stage approach, the template is simple:

```html
$if(rendered.navigation.toc)$
<nav id="TOC" role="doc-toc" class="toc-active">
$if(navigation.toc.title)$
  <h2 id="toc-title">$navigation.toc.title$</h2>
$endif$
$rendered.navigation.toc$
</nav>
$endif$
```

The `$rendered.navigation.toc$` variable contains pre-rendered HTML like:

```html
<ul>
  <li>
    <a href="#introduction" class="nav-link" data-scroll-target="#introduction">
      <span class="toc-number">1</span> Introduction
    </a>
    <ul>
      <li>
        <a href="#background" class="nav-link" data-scroll-target="#background">
          <span class="toc-number">1.1</span> Background
        </a>
      </li>
    </ul>
  </li>
</ul>
```

**Benefits of this approach**:
- No recursive template partials needed
- Complex nesting logic is in Rust (testable, debuggable)
- Template stays simple and declarative
- Users can override with custom HTML via `rendered.navigation.toc` metadata

---

## Customization Opportunities

Since we control the implementation, we can offer features beyond Pandoc:

### 1. Custom TOC Entry Rendering (Future)

Allow users to customize how TOC entries are rendered:

```yaml
format:
  html:
    toc: true
    toc-template: custom-toc.html  # Custom partial template
```

### 2. Per-Heading TOC Customization (Future)

Beyond `unlisted` and `unnumbered`, allow:

```markdown
## Section {.toc-text="Short Title"}
```

This would show "Short Title" in TOC instead of "Section".

### 3. TOC Filtering by Class (Future)

```yaml
format:
  html:
    toc: true
    toc-include-classes: [important]  # Only include headings with .important
    toc-exclude-classes: [draft]      # Exclude headings with .draft
```

### 4. Multiple TOCs (Future)

For long documents, allow multiple TOCs with different configurations:

```markdown
::: {.toc depth=2 title="Overview"}
:::

... content ...

::: {.toc depth=4 title="Detailed Contents"}
:::
```

**Note**: These advanced features are documented for future consideration. Phase 6 focuses on basic TOC parity with TS Quarto.

---

## Implementation Phases

### Phase 6.0: Core TOC Infrastructure (P0)

**Goal**: Basic TOC generation matching Pandoc/TS Quarto behavior.

#### Prerequisites

- [x] **Add path methods to ConfigValue** (in `crates/quarto-pandoc-types/src/config_value.rs`)
  - `get_path(&self, path: &[&str]) -> Option<&ConfigValue>`
  - `contains_path(&self, path: &[&str]) -> bool`
  - `insert_path(&mut self, path: &[&str], value: ConfigValue)`
  - `get_path_mut(&mut self, path: &[&str]) -> Option<&mut ConfigValue>`
  - `get_mut(&mut self, key: &str) -> Option<&mut ConfigValue>`
  - Unit tests for each method (all 80 tests pass)

#### Work Items

- [x] **Create TocEntry, TocConfig, and NavigationToc data structures**
  - Location: `crates/pampa/src/toc.rs`
  - Implement `to_config_value()` and `from_config_value()` for metadata integration
  - Export from pampa crate via `pub mod toc` in lib.rs

- [x] **Implement heading collection logic**
  - Walk AST to find Headers (or section Divs)
  - Respect `unlisted` class (exclude from TOC)
  - Respect `unnumbered` class (no number, but include)
  - Build hierarchical structure based on levels via `build_hierarchy()`
  - Respect `toc-depth` limit via `max_depth` parameter

- [x] **Implement generate_toc() function in pampa**
  - Location: `crates/pampa/src/toc.rs`
  - Pure function: `generate_toc(blocks: &[Block], config: &TocConfig) -> NavigationToc`
  - No RenderContext dependency (pampa cannot depend on quarto-core)
  - Export from pampa crate for use by quarto-core
  - 16 unit tests all passing

- [x] **Create TocGenerateTransform wrapper in quarto-core**
  - Location: `crates/quarto-core/src/transforms/toc_generate.rs`
  - Reads config from `RenderContext` (format metadata): `toc`, `toc-depth`, `toc-title`
  - Calls `pampa::toc::generate_toc()`
  - Stores result in `ast.meta` at `navigation.toc`
  - Skips if `navigation.toc` already exists (user override)
  - 9 unit tests all passing

- [ ] **Add --toc flag to pampa binary** (DEFERRED - not needed for basic TOC)
  - Implement as early metadata transform
  - Insert `toc: true` into `format.html` metadata
  - Ensure uniform code paths

- [x] **Unit tests for TOC extraction**
  - Flat headings (h2, h2, h2) → flat list ✓
  - Nested headings (h1, h2, h3) → hierarchical ✓
  - `unlisted` headings excluded ✓
  - `unnumbered` headings included without number ✓
  - `toc-depth` limits nesting ✓
  - Empty TOC when no headings ✓

### Phase 6.1: TOC Rendering Transform (P0)

**Goal**: Render TOC structure to HTML via TocRenderTransform.

#### Work Items

- [x] **Implement TocRenderTransform in quarto-core**
  - Location: `crates/quarto-core/src/transforms/toc_render.rs`
  - Reads `navigation.toc` from metadata
  - Skips if `rendered.navigation.toc` already exists (user override)
  - Renders TOC entries to HTML string recursively
  - Stores result at `rendered.navigation.toc`
  - 10 unit tests all passing

- [x] **Implement render_toc_entries_to_html() function**
  - Recursive rendering of nested entries
  - Add `data-scroll-target` attributes for JS integration
  - Add `nav-link` class for Bootstrap styling
  - HTML-escape entry titles and IDs

- [x] **Update FULL_HTML_TEMPLATE with simple TOC support**
  - Add `<nav id="TOC">` wrapper structure
  - Conditional rendering based on `rendered.navigation.toc` presence
  - Insert pre-rendered HTML via `$rendered.navigation.toc$`
  - Support `navigation.toc.title` for heading via `$navigation.toc.title$`

- [x] **Wire both TOC transforms into pipeline**
  - Added TocGenerateTransform to `build_transform_pipeline()` in `pipeline.rs`
  - Added TocRenderTransform after TocGenerateTransform
  - Order: after SectionizeTransform, before AppendixStructureTransform
  - All 6130 workspace tests pass

- [ ] **Add CSS for TOC styling** (DEFERRED)
  - Basic TOC layout (can be minimal, Bootstrap handles most)
  - Number styling if needed

- [ ] **Integration tests** (DEFERRED for separate session)
  - Full pipeline: QMD with `toc: true` → HTML with TOC
  - Verify correct structure for JS (Phase 4.1)
  - Compare with TS Quarto output
  - Test user override of `rendered.navigation.toc`

### Phase 6.2: Configuration Options (P1)

**Goal**: Support standard TOC configuration.

#### Work Items

- [ ] **Implement toc-depth**
  - Default: 3
  - Limit heading levels included
  - Limit nesting depth in output

- [ ] **Implement toc-title**
  - Default: "Table of Contents" (or language-specific)
  - Custom title support

- [ ] **Implement toc-location: body**
  - TOC rendered before main content
  - Phase 8 will add sidebar locations

- [ ] **Configuration validation**
  - Validate toc-depth range (1-6)
  - Warn on invalid options

### Phase 6.3: Wire into quarto-core (P1)

**Goal**: Ensure TOC works in full Quarto pipeline.

#### Work Items

- [x] **Add both TOC transforms to quarto-core pipeline**
  - Added TocGenerateTransform to `build_transform_pipeline()` in `pipeline.rs`
  - Added TocRenderTransform after TocGenerateTransform
  - TocGenerateTransform runs after SectionizeTransform (needs section IDs)
  - TocRenderTransform runs after TocGenerateTransform (needs TOC data)
  - Pipeline order documented in comments

- [ ] **Test in WASM context** (DEFERRED for separate session)
  - Verify TOC generation works in hub-client
  - Test with hub-client preview

---

## Dependencies

### Depends On

- **Phase 1.0**: SectionizeTransform (provides section IDs for TOC links)
- **Phase 2**: Template system (for rendering TOC)

### Blocks

- **Phase 4.1**: JS TOC feature (needs HTML structure to work with)

---

## HTML Output Structure

For JS integration (Phase 4.1), the TOC must have this structure:

```html
<nav id="TOC" role="doc-toc" class="toc-active">
  <h2 id="toc-title">Table of Contents</h2>
  <ul>
    <li>
      <a href="#introduction" class="nav-link" data-scroll-target="#introduction">
        <span class="toc-number">1</span> Introduction
      </a>
      <ul>
        <li>
          <a href="#background" class="nav-link" data-scroll-target="#background">
            <span class="toc-number">1.1</span> Background
          </a>
        </li>
      </ul>
    </li>
    <li>
      <a href="#methods" class="nav-link" data-scroll-target="#methods">
        <span class="toc-number">2</span> Methods
      </a>
    </li>
  </ul>
</nav>
```

**Key attributes for JS**:
- `id="TOC"` - Selector target
- `role="doc-toc"` - Semantic role, selector
- `class="toc-active"` - Marks TOC for scroll tracking
- `data-scroll-target` - Links to section IDs (with `#` prefix)
- `class="nav-link"` - Bootstrap styling, JS selector

---

## Testing Strategy

### Unit Tests

1. **TOC extraction** (TocGenerateTransform):
   - Various heading structures
   - `unlisted`/`unnumbered` handling
   - Depth limiting
   - Empty document (no headings)

2. **TOC rendering** (TocRenderTransform):
   - Flat entries → flat `<ul>`
   - Nested entries → nested `<ul>` structure
   - Section numbers rendered correctly
   - HTML escaping of titles
   - Correct `data-scroll-target` attributes

3. **Data structure conversion**:
   - `TocEntry` to/from ConfigValue
   - Round-trip consistency

### Integration Tests

1. **Full pipeline**:
   - QMD → HTML with TOC
   - Verify HTML structure matches expected

2. **Configuration**:
   - `toc: false` → no TOC
   - `toc-depth: 2` → only h1, h2
   - `toc-title: "Contents"` → custom title

3. **User overrides**:
   - User-provided `navigation.toc` → TocGenerateTransform skipped
   - User-provided `rendered.navigation.toc` → TocRenderTransform skipped
   - Custom HTML output preserved

4. **Comparison**:
   - Compare output structure with TS Quarto
   - Verify JS compatibility

### WASM Tests

1. **hub-client integration**:
   - TOC renders in preview
   - Structure correct for future JS

---

## Success Criteria

### Phase 6.0-6.1 (Basic TOC)
- [x] `toc: true` generates TOC in HTML output
- [x] TOC structure matches Bootstrap/TS Quarto conventions
- [x] `unlisted` headings excluded
- [x] Section IDs link correctly (via `href` and `data-scroll-target`)
- [x] User can override `navigation.toc` to customize structure
- [x] User can override `rendered.navigation.toc` to provide custom HTML

### Phase 6.2 (Configuration)
- [x] `toc-depth` limits levels
- [x] `toc-title` customizes title
- [ ] `toc-location: body` works (currently body is the only location)

### Phase 6.3 (Integration)
- [x] Works in quarto-core pipeline
- [ ] Works in WASM/hub-client (needs testing)
- [ ] Phase 4.1 JS can use the structure (needs JS implementation)

---

## File Structure

```
crates/quarto-pandoc-types/
├── src/
│   └── config_value.rs           # Add get_path, contains_path, insert_path methods

crates/pampa/
├── src/
│   ├── toc.rs                    # TocEntry, TocConfig, NavigationToc, generate_toc()
│   ├── lib.rs                    # Export toc module
│   └── main.rs                   # --toc flag handling
├── tests/
│   └── toc/
│       ├── basic.rs              # Basic extraction tests
│       ├── unlisted.rs           # Class handling tests
│       └── depth.rs              # Depth limiting tests

crates/quarto-core/
├── src/
│   ├── transforms/
│   │   ├── mod.rs                # Add TocGenerateTransform, TocRenderTransform
│   │   ├── toc_generate.rs       # TocGenerateTransform (wrapper calling pampa::toc)
│   │   └── toc_render.rs         # TocRenderTransform (renders to HTML)
│   └── template.rs               # Updated with simple TOC template
```

---

## Open Questions

1. **Number formatting**: Should we support custom number formats (e.g., "I.A.1" vs "1.1.1")?
   - Recommendation: Defer, use Pandoc's default for now

2. **TOC for non-HTML formats**: Should pampa's TOC work for PDF/LaTeX?
   - Recommendation: Yes, but Phase 6 focuses on HTML. LaTeX can use `\tableofcontents`.

3. ~~**Recursive template partials**: Does quarto-doctemplate support recursive partials?~~
   - **RESOLVED**: Using two-stage rendering approach instead. TocRenderTransform renders
     the TOC to HTML in Rust code, then the template just inserts `$rendered.navigation.toc$`.
     This avoids the complexity of recursive template partials entirely.

---

## References

### Internal
- Parent plan: `claude-notes/plans/2026-01-24-html-rendering-parity.md`
- JS integration: `claude-notes/plans/2026-01-28-phase4-javascript-infrastructure.md`

### External (Pandoc)
- TOC entry point: `external-sources/pandoc/src/Text/Pandoc/Writers/Shared.hs` (lines 658-665)
- Tree building: `external-sources/pandoc/src/Text/Pandoc/Chunks.hs` (lines 368-444)
- Section creation: `external-sources/pandoc/src/Text/Pandoc/Shared.hs` (lines 514-578)
- Options: `external-sources/pandoc/src/Text/Pandoc/Options.hs`

### External (TS Quarto)
- TOC relocation: `external-sources/quarto-cli/src/format/html/format-html-bootstrap.ts`
- TOC template: `external-sources/quarto-cli/src/resources/formats/html/pandoc/html.template`
