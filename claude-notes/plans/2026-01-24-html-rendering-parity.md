# HTML Rendering Parity: Rust Quarto ↔ TypeScript Quarto

**Beads Epic**: kyoto-6jv
**Created**: 2026-01-24
**Updated**: 2026-01-26 (added Phase 1b detailed subplan reference)
**Status**: Planning

---

## Architectural Rationale: Why AST-First?

### The Fundamental Difference

**TypeScript Quarto** uses Pandoc as a black-box converter. The pipeline is:

```
QMD → [Lua Filters] → Pandoc AST → [PANDOC] → HTML → [Post-processing] → Final HTML
```

Because TS Quarto cannot modify Pandoc's HTML writer, it must:
1. Accept whatever HTML structure Pandoc produces
2. Fix that structure via DOM manipulation (TypeScript postprocessors)
3. Move elements around, add classes, restructure the document

This creates inherent limitations:
- Postprocessors are format-specific (an HTML fix doesn't help PDF or Word output)
- DOM manipulation is complex, error-prone, and hard to test
- The same logical operation may need implementation in multiple places (Lua filter + TS postprocessor)
- WASM targets face impedance mismatch when doing DOM manipulation

**Rust Quarto** controls pampa (our Pandoc AST parser and HTML writer). The pipeline is:

```
QMD → [pampa parser] → Pandoc AST → [AST Transforms] → Pandoc AST → [pampa writer] → HTML
```

Because we control the writer, we can:
1. Structure the AST correctly *before* HTML generation
2. Emit correct HTML from the start—no post-processing needed
3. Keep transforms format-agnostic when possible

### The Design Principle

> **If a document transformation can be expressed as `Pandoc AST → Pandoc AST`, it should be an AST transform, not a DOM postprocessor.**

This principle has several benefits:

1. **Format independence**: An AST transform that restructures footnotes works for HTML, PDF, Word, and any other format. A DOM postprocessor only works for HTML.

2. **Single source of truth**: The transformed AST is the canonical representation. All output formats render the same logical document.

3. **Stateless writers**: The HTML writer's only job is to convert AST nodes to HTML strings. It doesn't collect footnotes, move sections, or track state. This makes the writer simple, fast, and correct by construction.

4. **WASM compatibility**: AST transforms work identically in native CLI and browser WASM. DOM postprocessors in WASM require either shipping a DOM parser to the browser or implementing format-specific logic in JavaScript—both add complexity.

5. **Testability**: AST transforms can be unit-tested with `input AST → expected output AST`. DOM postprocessors require parsing HTML, which is more fragile.

### When DOM Manipulation Is Still Needed

Some operations truly depend on the rendered HTML structure:
- Complex column/margin layouts (Phase 8) where element positioning depends on rendered sizes
- Operations that must analyze the full HTML tree structure

These are deferred to Phase 8 (Advanced Layout) and may not be needed at all for basic HTML parity.

### Implications for Feature Development

When implementing any TS Quarto feature in Rust Quarto:

1. **First ask**: Can this be done as a format-agnostic AST transform?
2. **If yes**: Implement it in `AstTransformsStage`. It will work for all formats.
3. **If no**: Ask why. Is it inherently format-specific, or just implemented that way in TS Quarto because of Pandoc limitations?
4. **Only if truly HTML-specific**: Consider template logic or client-side JS.

This approach means features implemented for Rust Quarto's native CLI automatically work in hub-client (WASM) without additional effort.

---

## Executive Summary

This plan outlines the work needed to bring Rust Quarto's HTML rendering to feature parity with TypeScript Quarto. The goal is to produce HTML output that is structurally and visually equivalent to TS Quarto, while maintaining code sharing between native (CLI) and WASM (hub-client) targets.

### Architectural Strategy: AST-First, Emit Correct HTML

After analyzing TS Quarto's HTML postprocessors, we've adopted a **pure AST-first approach**:

1. **Maximize AST transforms** - All structural decisions happen at the Pandoc AST level, before HTML generation
2. **Stateless HTML writer** - pampa's HTML writer simply renders the AST as-is, with no collection or rearrangement
3. **Leverage client-side JS** - Defer interactive features to quarto.js (same as TS Quarto)
4. **Minimal DOM manipulation** - Only for operations that truly depend on rendered HTML structure

**Key insight**: Unlike TS Quarto (which uses Pandoc and must fix its output), Rust Quarto controls pampa. We can emit correct HTML from the start rather than generating incorrect HTML and then fixing it via DOM manipulation.

**Design principle**: If an operation can be expressed as a `Pandoc → Pandoc` transformation, it should be an AST transform, not a DOM postprocessor. The HTML writer's only job is to convert AST nodes to HTML strings.

### Current State

**Rust Quarto** has a working 5-stage async pipeline:
1. ParseDocumentStage - QMD → Pandoc AST
2. EngineExecutionStage - Code execution via knitr/jupyter/markdown
3. AstTransformsStage - Quarto-specific transforms
4. RenderHtmlBodyStage - AST → HTML body
5. ApplyTemplateStage - Body → complete HTML

**Working features**:
- Basic HTML rendering
- SASS/Bootstrap theme compilation
- Template system with metadata
- Resource directory structure
- WASM rendering for hub-client
- **Code execution engines**:
  - `markdown` - No-op passthrough (all platforms)
  - `knitr` - R code execution (native only)
  - `jupyter` - Python/Julia execution (native only)
  - Engine detection from metadata
  - AST serialization/deserialization for engine round-trip
  - Source location reconciliation via quarto-ast-reconcile
  - Graceful fallback with warnings in WASM
- **AST transforms** (Lua filter equivalents):
  - `CalloutTransform` - Callout blocks processing
  - `CalloutResolveTransform` - Callout reference resolution
  - `MetadataNormalizeTransform` - Metadata normalization
  - `TitleBlockTransform` - Title block handling
  - `ResourceCollectorTransform` - Resource collection

**Major gaps** compared to TS Quarto:
1. Limited AST transforms (only callouts, metadata, title block, resources - missing ~25 Lua filter equivalents)
2. No HTML postprocessors (DOM manipulation for code buttons, anchors, etc.)
3. Simplified HTML template (missing partials, dependencies)
4. No JavaScript dependencies (quarto.js, tippy, popper, etc.)
5. Limited metadata injection
6. No navigation (navbar, sidebar, footer)

---

## Architecture Comparison

### TypeScript Quarto Pipeline

```
QMD File
    ↓
[Lua Filters: quarto-pre/*]  ←── ~30 filters: figures, panels, code annotations, etc.
    ↓
Enhanced Pandoc AST
    ↓
[Pandoc Converter]
    ↓
HTML (from template.html + partials)
    ↓
[TypeScript Postprocessors]  ←── DOM manipulation: code buttons, anchors, metadata
    ↓
[Lua Filters: quarto-post/*]  ←── Format-specific cleanup
    ↓
Final HTML
    ↓
[Dependencies Injection]  ←── JS/CSS resources
    ↓
Output File
```

### Rust Quarto Pipeline (Current)

```
QMD File
    ↓
[Parse Stage]  ←── pampa parser
    ↓
Pandoc AST
    ↓
[AST Transforms Stage]  ←── Basic transforms only
    ↓
[Render HTML Stage]  ←── pampa HTML writer
    ↓
[Apply Template Stage]  ←── Minimal template
    ↓
Output File
```

### Target: Rust Quarto Pipeline (AST-First)

```
QMD File
    ↓
[Parse Stage]
    ↓
Pandoc AST
    ↓
[AST Transforms Stage]  ←── ENHANCED: All structural decisions here
    │                        - Class/attribute transforms
    │                        - Footnotes extraction
    │                        - Bibliography placement
    │                        - Appendix structure
    ↓
[Render HTML Stage]  ←── STATELESS: Just renders the AST
    ↓
[Apply Template Stage]  ←── ENHANCED: Richer template + dependencies
    ↓
Output File + quarto.js  ←── Client-side interactivity
```

**Note**: No HTML Post-Processing Stage needed for basic parity. DOM manipulation only required for Phase 8 (Advanced Layout) features like column/margin processing.

---

## HTML Semantic Structure: `<main>`, `<section>`, `<header>`

### Where These Elements Come From

TS Quarto HTML output uses semantic HTML5 elements (`<main>`, `<section>`, `<header>`). Understanding their origins is critical for achieving parity:

| Element | Source | Notes |
|---------|--------|-------|
| `<section>` | **Pandoc HTML writer** | Pandoc's `makeSectionsWithOffsets` wraps headers+content in Div blocks with `class="section"`. In HTML5 mode, these render as `<section>` tags. |
| `<header id="title-block-header">` | **Pandoc template partial** | The `title-block.html` partial generates this from document metadata. |
| `<main class="content">` | **TS Quarto EJS templates** | `before-body-article.ejs` opens this; Pandoc does NOT generate `<main>`. |
| `<nav id="TOC" role="doc-toc">` | **Pandoc template partial** | The `toc.html` partial generates this; TS Quarto post-processing moves it to sidebars. |

### Pandoc's Section Wrapping (HTML5 Mode)

In Pandoc's Haskell source (`Text/Pandoc/Shared.hs:makeSectionsWithOffsets`), before HTML rendering, headers are transformed:

**Before**:
```
Header 2 "intro" ["section"] [] [Str "Introduction"]
Para [Str "Some content"]
Header 2 "methods" ["section"] [] [Str "Methods"]
Para [Str "More content"]
```

**After**:
```
Div ("intro", ["section"], [])
  [ Header 2 "intro" [] [] [Str "Introduction"]
  , Para [Str "Some content"]
  ]
Div ("methods", ["section"], [])
  [ Header 2 "methods" [] [] [Str "Methods"]
  , Para [Str "More content"]
  ]
```

In HTML5 mode, `Div` blocks with `"section"` class emit as `<section>` tags, not `<div>`.

### TS Quarto's Page Structure

TS Quarto's EJS templates create this structure (simplified):

```html
<body>
  <header id="title-block-header">
    <h1 class="title">...</h1>
    <!-- subtitle, authors, date, abstract -->
  </header>

  <div id="quarto-content" class="page-columns ...">
    <div id="quarto-sidebar-toc-left" class="sidebar toc-left">
      <nav id="TOC">...</nav>  <!-- moved here by post-processing -->
    </div>

    <main class="content" id="quarto-document-content">
      <section id="intro" class="level2">
        <h2>Introduction</h2>
        <p>Some content</p>
      </section>
      <section id="methods" class="level2">
        <h2>Methods</h2>
        <p>More content</p>
      </section>
    </main>

    <div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
      <!-- margin notes, citations -->
    </div>
  </div>
</body>
```

### How Pandoc Enables Section Wrapping

**Important**: Pandoc only emits `<section>` elements when the `--section-divs` flag is passed (or `section-divs: true` in YAML metadata). Without this flag, Pandoc emits flat headers with no wrapping.

TS Quarto explicitly enables this in `src/format/html/format-html-bootstrap.ts:189`:
```typescript
return {
  pandoc: {
    [kSectionDivs]: true,  // Enables --section-divs
    // ...
  },
}
```

For Rust Quarto, since we control pampa, we implement this as an AST transform that runs before HTML rendering.

### Code Sharing Architecture: pampa ↔ quarto-core

`SectionizeTransform` should be **implemented once in the `pampa` crate** and used by both:
1. `pampa -t html` (standalone tool)
2. `quarto-core`'s render pipeline

**Design principles:**

1. **Transform lives in `pampa` crate**: Since pampa is the lower-level crate that quarto-core depends on, shared transforms belong in pampa.

2. **HTML writer is stateless**: The writer NEVER applies transforms. It just renders the AST it receives. This is critical to avoid double-application.

3. **Transform application is the caller's responsibility**:
   - `pampa -t html` binary: checks frontmatter for `format: html: section-divs: true`, applies transform before calling writer
   - `quarto-core` pipeline: applies transform in `AstTransformsStage`, then calls pampa's writer

**Code organization:**

```
crates/pampa/
├── src/
│   ├── transforms/
│   │   ├── mod.rs           # pub mod sectionize;
│   │   └── sectionize.rs    # SectionizeTransform implementation
│   ├── writers/
│   │   └── html.rs          # Stateless HTML writer (no transforms)
│   └── lib.rs               # pub use transforms::sectionize::*;

crates/quarto-core/
├── src/
│   ├── transform/
│   │   └── mod.rs           # Uses pampa::SectionizeTransform
│   └── pipeline.rs          # AstTransformsStage includes SectionizeTransform
```

**Avoiding double-application:**

| Path | Who applies SectionizeTransform | When |
|------|--------------------------------|------|
| `pampa -t html` | pampa binary | Before calling `html::write()`, if `section-divs: true` in frontmatter |
| `quarto render` | quarto-core's `AstTransformsStage` | Before `RenderHtmlBodyStage` calls `html::write()` |

Since quarto-core never uses pampa's binary (it uses the library directly), there's no risk of double-application. The transform runs exactly once in each path.

**Configuration:**

- `pampa -t html`: Reads `format: html: section-divs: true` from document frontmatter
- `quarto render`: Bootstrap HTML format always enables it (via format configuration)

**Testing strategy:**

1. **Unit tests in pampa**: Test `SectionizeTransform` in isolation with various AST inputs
2. **Integration tests in pampa**: Test `pampa -t html` with `section-divs: true` documents
3. **Integration tests in quarto-core**: Test full pipeline produces sectioned HTML

This architecture allows `SectionizeTransform` to be developed and tested independently in pampa, then reused by quarto-core without code duplication.

### Implementation Plan for Rust Quarto

To achieve this structure, we need three components:

#### 1. AST Transform: `SectionizeTransform`

Analogous to Pandoc's `makeSectionsWithOffsets`. This transform:
- Walks the document collecting blocks at each header level
- Wraps each header and its following content in a `Div` with:
  - ID **moved** from the header (header loses its ID)
  - Class `section` plus `level{N}` (e.g., `level2`)
  - Other attributes from the header
- **Critically**: Sections nest hierarchically based on header level

**ID and Class Handling**:
- **ID**: Moves from header to section. For `## Intro {#intro}`, output is `<section id="intro"><h2>Intro</h2>...</section>`.
- **Classes**: Duplicated on both section AND header. For `## Intro {.special}`, output is `<section class="level2 special"><h2 class="special">Intro</h2>...</section>`.

This matters for anchor links (ID on section) and styling (classes on both).

**Nesting Rule**: A section at level N contains all following content until the next header at level ≤ N. Headers at level > N become nested child sections.

Example input:
```markdown
## Section A
Content A.
### Subsection A.1
Sub content.
## Section B
Content B.
```

Example output structure:
```
<section id="section-a" class="level2">
  <h2>Section A</h2>
  <p>Content A.</p>
  <section id="subsection-a.1" class="level3">
    <h3>Subsection A.1</h3>
    <p>Sub content.</p>
  </section>
</section>
<section id="section-b" class="level2">
  <h2>Section B</h2>
  <p>Content B.</p>
</section>
```

**Algorithm** (stack-based to handle nesting):

```rust
// Pseudocode for SectionizeTransform
fn sectionize(blocks: Vec<Block>) -> Vec<Block> {
    // Stack of (level, attr, content) for open sections
    let mut section_stack: Vec<(i32, Attr, Vec<Block>)> = vec![];
    let mut output: Vec<Block> = vec![];

    for block in blocks {
        if let Block::Header(level, attr, content) = &block {
            // Close all sections at level >= this header's level
            // (i.e., same level or deeper)
            while let Some((stack_level, _, _)) = section_stack.last() {
                if *stack_level >= *level {
                    let (_, section_attr, section_content) = section_stack.pop().unwrap();
                    let section_div = Block::Div(Div {
                        attr: section_attr,
                        content: section_content,
                    });
                    // Add closed section to parent, or output if no parent
                    if let Some((_, _, parent_content)) = section_stack.last_mut() {
                        parent_content.push(section_div);
                    } else {
                        output.push(section_div);
                    }
                } else {
                    break;
                }
            }

            // Create attributes for new section
            // - ID moves from header to section
            // - Classes are duplicated (on both section and header)
            // - level{N} class added to section
            let mut section_classes = vec!["section".into(), format!("level{}", level)];
            section_classes.extend(attr.1.clone());  // Add header's classes
            let section_attr = (
                attr.0.clone(),  // ID moves to section
                section_classes,
                attr.2.clone(),  // Other attributes
            );

            // Create header with ID removed but classes preserved
            let header_without_id = Block::Header(Header {
                level: *level,
                attr: (String::new(), attr.1.clone(), vec![]),  // Empty ID, keep classes
                content: content.clone(),
            });

            // Push new section onto stack (with ID-less header as first content)
            section_stack.push((*level, section_attr, vec![header_without_id]));
        } else {
            // Non-header block: add to innermost section, or output if none
            if let Some((_, _, content)) = section_stack.last_mut() {
                content.push(block);
            } else {
                output.push(block);
            }
        }
    }

    // Close all remaining open sections (innermost first)
    while let Some((_, section_attr, section_content)) = section_stack.pop() {
        let section_div = Block::Div(Div {
            attr: section_attr,
            content: section_content,
        });
        if let Some((_, _, parent_content)) = section_stack.last_mut() {
            parent_content.push(section_div);
        } else {
            output.push(section_div);
        }
    }

    output
}
```

**Key invariant**: At any point, `section_stack` contains sections ordered by level (lowest/outermost first). When we encounter a header at level N, we close all sections at level ≥ N before opening a new one.

#### 2. HTML Writer Enhancement

Modify pampa's HTML writer to recognize `Div` blocks with `section` class:

```rust
// In write_block for Block::Div
Block::Div(div) => {
    let (id, classes, attrs) = &div.attr;
    let tag = if classes.contains(&"section".to_string()) {
        "section"
    } else {
        "div"
    };
    write!(ctx, "<{}", tag)?;
    write_attr(&div.attr, ctx)?;
    writeln!(ctx, ">")?;
    write_blocks(&div.content, ctx)?;
    writeln!(ctx, "</{}>", tag)?;
}
```

#### 3. Template Enhancement

Update the HTML template to:
- Wrap body content in `<main class="content" id="quarto-document-content">`
- Include sidebar structure (even if initially empty)
- Generate `<header id="title-block-header">` from metadata

```html
$if(title)$
<header id="title-block-header">
  <h1 class="title">$title$</h1>
  $if(subtitle)$<p class="subtitle">$subtitle$</p>$endif$
  <!-- ... authors, date, abstract ... -->
</header>
$endif$

<div id="quarto-content" class="page-columns page-rows-contents">
  <main class="content" id="quarto-document-content">
$body$
  </main>
</div>
```

### Work Items

- [ ] **Phase 1a**: Create `SectionizeTransform` to wrap headers in section Divs
- [ ] **Phase 1a**: Modify pampa HTML writer to emit `<section>` for Divs with section class
- [ ] **Phase 2**: Update template with `<main>` wrapper and sidebar structure
- [ ] **Phase 2**: Generate `<header id="title-block-header">` from metadata

### Testing Strategy

1. **Unit tests for SectionizeTransform**:
   - Flat sections at same level (h2, h2, h2) → sibling sections
   - Nested sections (h2, h3, h3, h2) → h3s nested inside first h2, second h2 is sibling
   - Deep nesting (h1, h2, h3, h4) → each level nested inside parent
   - Mixed levels (h2, h4, h3) → verify correct closing/opening behavior
   - Content before first header → output before any section
   - Empty sections (header with no following content)
   - Headers with custom IDs and classes → preserved in section attributes

2. **HTML output tests**:
   - Verify `<section>` tags appear with correct IDs and `levelN` classes
   - Verify nesting structure matches Pandoc's `--section-divs` output
   - Verify header IDs are **moved** to section (header has no ID, section has it)
   - Verify `## Foo {#bar .baz}` produces `<section id="bar" class="section level2 baz"><h2 class="baz">Foo</h2>...` (classes duplicated)

3. **Comparison tests**:
   - Run same input through `pandoc --section-divs` and our pipeline
   - Compare DOM structure (ignoring whitespace differences)

---

## TS Quarto HTML Postprocessor Analysis

### Classification System

For each postprocessor operation, we classify feasibility:

| Classification | Meaning |
|----------------|---------|
| **AST-POSSIBLE** | Can definitely be done at AST level before HTML generation |
| **AST-DIFFICULT** | Could be done at AST but awkward/complex |
| **DOM-REQUIRED** | Must be done at HTML DOM level |
| **CLIENT-JS** | Should be handled by client-side JavaScript |
| **TEMPLATE** | Can be handled in the HTML template |

### Postprocessor-by-Postprocessor Analysis

#### 1. Main Format Postprocessor (`htmlFormatPostprocessor`)

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Body classes | **TEMPLATE** | Add classes via template metadata |
| Code copy button scaffolding | **AST-POSSIBLE** | Wrap code blocks in scaffold div during AST transform |
| Code copy buttons | **CLIENT-JS** | quarto.js injects buttons at runtime |
| Code example iframes | **DOM-REQUIRED** | Needs `data-code-preview` attribute parsing |
| Anchor sections (`.anchored` class) | **AST-POSSIBLE** | Mark headings during AST traversal |
| Code cell class hoisting | **AST-POSSIBLE** | Move classes during code block transform |
| Table cell restoration (th/td) | **DOM-REQUIRED** | Pandoc generates HTML; we must fix it |
| Draft alert banner | **TEMPLATE** | Conditional template partial based on metadata |

#### 2. Code Annotations Processor

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Annotation parsing | **AST-POSSIBLE** | Parse `# <1>` markers during code block processing |
| Definition list creation | **AST-POSSIBLE** | Create DL structure in AST |
| Annotation mode (hover/select) | **AST-POSSIBLE** | Add data attributes to AST elements |
| Gutter/anchor injection | **CLIENT-JS** | Let quarto.js handle interactive elements |

#### 3. Metadata Postprocessor (`metadataPostProcessor`)

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Canonical URL | **TEMPLATE** | `<link rel="canonical">` in template |
| Google Scholar meta tags | **TEMPLATE** | Generate from metadata in template |
| Author/citation meta | **TEMPLATE** | All metadata-driven, no DOM needed |

#### 4. Title Block Processor

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Title canonicalization | **AST-POSSIBLE** | Already have TitleBlockTransform |
| Banner processing | **AST-DIFFICULT** | Image resources need tracking |
| Header positioning | **AST-POSSIBLE** | Structure in AST, style in CSS |

#### 5. Document Appendix Processor

| Operation | Classification | Notes |
|-----------|----------------|-------|
| References section | **AST-POSSIBLE** | Place bibliography block in appendix during AST transform |
| Footnotes section | **AST-POSSIBLE** | `FootnotesTransform` extracts inline notes, creates section |
| License/copyright | **TEMPLATE** | Metadata-driven template partial |
| Citation box | **TEMPLATE** | Generate from CSL metadata |
| User appendix sections | **AST-POSSIBLE** | `AppendixStructureTransform` collects `.appendix` divs |

**Note**: Unlike TS Quarto (which must move Pandoc-generated HTML), we control pampa and can structure the AST correctly before emission. All appendix operations become AST transforms.

#### 6. Bootstrap Postprocessor

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Text styling (display-7, lead) | **AST-POSSIBLE** | Add classes during AST transform |
| Figure/image classes | **AST-POSSIBLE** | Add during figure processing |
| Table Bootstrap classes | **AST-POSSIBLE** | Add during table processing |
| TOC relocation | **DOM-REQUIRED** | Complex DOM manipulation |
| Column layout processing | **DOM-REQUIRED** | Requires full tree analysis |
| Margin content processing | **DOM-REQUIRED** | Complex element movement |
| Tabset margin extraction | **DOM-REQUIRED** | Post-render structure analysis |
| Alternate format links | **TEMPLATE** | Metadata-driven |
| Notebook previews | **DOM-REQUIRED** | Async file discovery |

#### 7. Bootstrap Finalizer

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Nested column cleanup | **DOM-REQUIRED** | Post-layout tree analysis |
| Content mode detection | **DOM-REQUIRED** | Requires full document scan |
| Dark mode default | **TEMPLATE** | Body class from metadata |
| Z-order fixes | **DOM-REQUIRED** | Layout-dependent |

#### 8. Notebook View Postprocessor

| Operation | Classification | Notes |
|-----------|----------------|-------|
| Body class | **TEMPLATE** | Add via metadata |
| Cell wrapping | **AST-POSSIBLE** | Wrap during cell processing |
| Execution count decorator | **AST-POSSIBLE** | Add from cell metadata |
| Code/output grouping | **AST-POSSIBLE** | Structure during AST transform |

#### 9. Other Processors

| Processor | Classification | Notes |
|-----------|----------------|-------|
| KaTeX script handling | **TEMPLATE** | Script injection in template |
| Alternate format links | **TEMPLATE** | Data computed, rendered in template |
| Code links (repo/binder) | **TEMPLATE** | Metadata-driven |

---

## Summary: AST vs DOM Split

### Operations to Move to AST Transforms

These can and should be done at the Pandoc AST level:

| Operation | New Transform | Priority |
|-----------|---------------|----------|
| Anchor section classes | `AnchoredHeadingsTransform` | P0 |
| Code block scaffolding | `CodeBlockTransform` | P0 |
| Footnote extraction | `FootnotesTransform` | P1 |
| Bibliography placement | `BibliographyPlacementTransform` | P1 |
| Appendix structure | `AppendixStructureTransform` | P1 |
| Code annotation parsing | `CodeAnnotationTransform` | P1 |
| Bootstrap text classes | `BootstrapClassesTransform` | P1 |
| Figure/image classes | `FigureTransform` | P1 |
| Table Bootstrap classes | `TableTransform` | P1 |
| Notebook cell wrapping | `NotebookCellTransform` | P2 |
| Code cell class hoisting | `CodeBlockTransform` | P1 |

**Document Structure Transforms** (new):

| Transform | Responsibility |
|-----------|----------------|
| `FootnotesTransform` | Extract `Inline::Note` content → create footnotes section at end |
| `BibliographyPlacementTransform` | Position bibliography block in document/appendix |
| `AppendixStructureTransform` | Collect `.appendix` divs, create appendix container |

These transforms embody the principle: **structure the AST correctly, then the writer just renders it**.

### Operations for Template

These should be handled in the HTML template:

| Operation | Template Section | Priority |
|-----------|------------------|----------|
| Body classes | Head metadata | P0 |
| Draft alert | Conditional partial | P2 |
| Canonical URL | Head links | P2 |
| Google Scholar meta | Head meta | P3 |
| Author/citation meta | Head meta | P2 |
| License/copyright | Footer partial | P3 |
| Dark mode class | Body attributes | P1 |
| Format links | Header/footer | P2 |

### Operations for Client-Side JS (quarto.js)

These should be handled by JavaScript at runtime:

| Operation | JS Module | Priority |
|-----------|-----------|----------|
| Code copy button injection | clipboard.js | P0 |
| Anchor link insertion | anchor.js | P1 |
| Code annotation interactivity | annotations.js | P2 |
| Tooltip initialization | tippy.js | P2 |
| TOC scroll sync | toc.js | P2 |

### Operations Requiring DOM Postprocessing

**With the AST-first approach, very few operations truly require DOM manipulation:**

| Operation | Postprocessor | Priority | Why DOM Required |
|-----------|---------------|----------|------------------|
| Column layout finalization | `ColumnLayoutProcessor` | P2 | Requires analyzing rendered tree structure |
| Margin content movement | `MarginProcessor` | P3 | Complex element repositioning based on layout |
| TOC relocation (advanced) | `TocProcessor` | P3 | Position-dependent move for complex layouts |

**Operations moved to AST transforms** (no longer DOM-required):
- ~~Table cell restoration~~ → pampa emits correct th/td from AST
- ~~References consolidation~~ → `BibliographyPlacementTransform` positions in AST
- ~~Footnotes consolidation~~ → `FootnotesTransform` restructures AST
- ~~Basic TOC~~ → Extract headings in AST, render via template

**Note**: The remaining DOM operations are all related to Bootstrap's complex column/margin layout system, which requires analyzing how content flows in the rendered HTML. These can be deferred to Phase 8 (Advanced Layout).

---

## Gap Analysis: Lua Filter Equivalents

**Detailed Analysis**: See `claude-notes/plans/2026-01-24-lua-filter-analysis.md` (beads issue: kyoto-dy3)

TS Quarto has **7 filter groups** with 50+ individual filters executed in this order:
1. `quarto_init_filters` - Initialization
2. `quarto_normalize_filters` - AST normalization (creates custom nodes like FloatRefTarget)
3. `quarto_pre_filters` - Format-agnostic transforms
4. `quarto_crossref_filters` - Cross-reference processing
5. `quarto_layout_filters` - Layout processing
6. `quarto_post_filters` - Format-specific rendering
7. `quarto_finalize_filters` - Dependency injection

In Rust Quarto, we implement these as:
- **Format-agnostic** → AST transforms in `AstTransformsStage`
- **Format-specific** → Minimal HTML postprocessors + client-side JS

### Pre-processing Filters (quarto-pre/) - Format-Agnostic

| TS Quarto Filter | Function | Priority | Rust Strategy |
|------------------|----------|----------|---------------|
| `parsefiguredivs.lua` | Figure parsing, captions, layout | P0 | AST transform |
| `code-annotation.lua` | Code annotations (parsing only) | P1 | AST transform |
| `shortcodes-handlers.lua` | Shortcode expansion | P0 | AST transform |
| `table-captions.lua` | Table caption positioning | P1 | AST transform |
| `table-colwidth.lua` | Table column widths | P2 | AST transform |
| `panel-sidebar.lua` | Sidebar panel layouts | P2 | AST transform |
| `code-filename.lua` | Filename labels on code blocks | P1 | AST transform |
| `hidden.lua` | Hidden/visible toggle | P2 | AST transform |
| `callout.lua` | Callout blocks | P0 | ✅ CalloutTransform + CalloutResolveTransform |
| `layout.lua` | Layout columns/rows | P1 | AST transform |
| `book-numbering.lua` | Book section numbering | P3 | AST transform |
| `resourcefiles.lua` | Resource file handling | P2 | AST transform |
| `shiny.lua` | Shiny server setup | P4 | (defer) |

### Post-processing Filters (quarto-post/) - Format-Specific

| TS Quarto Filter | Function | Priority | Rust Strategy |
|------------------|----------|----------|---------------|
| `html.lua` | Table rows, figure alignment, image attrs | P0 | AST transform + DOM (table only) |
| `responsive.lua` | Responsive images/tables | P0 | AST transform |
| `foldcode.lua` | Code folding with `<details>` | P1 | AST transform |
| `cellcleanup.lua` | Clean up cell outputs | P1 | AST transform |
| `bibliography.lua` | Bibliography rendering | P2 | AST transform |
| `cites.lua` | Citation link formatting | P2 | AST transform |
| `dashboard.lua` | Dashboard layouts | P3 | (defer - large subsystem) |
| `ojs.lua` | Observable JS | P3 | (defer - large subsystem) |

### Crossref Filters - Format-Agnostic

| TS Quarto Filter | Function | Priority | Rust Strategy |
|------------------|----------|----------|---------------|
| `crossref/preprocess.lua` | Mark subfloats | P1 | AST transform |
| `crossref/sections.lua` | Section numbering | P1 | AST transform |
| `crossref/figures.lua` | Figure references | P1 | AST transform |
| `crossref/equations.lua` | Equation references | P2 | AST transform |
| `crossref/refs.lua` | Resolve @ref syntax | P1 | AST transform |
| `crossref/meta.lua` | Crossref metadata | P1 | AST transform |

---

## Implementation Phases

### Phase 1: AST Transform Infrastructure

**Goal**: Establish the pattern for new AST transforms and implement high-priority ones.

#### Phase 1.0: SectionizeTransform (in pampa - independent, testable first)

This can be implemented and tested in isolation before other transforms.

**Work items**:

##### Step 1: Verify Pandoc's exact behavior
- [x] Run `pandoc --section-divs` on test cases to document exact ID/class handling
- [x] Document: Does ID move from header to section?
- [x] Document: Are classes duplicated on both section and header?
- [x] Document: What class names does Pandoc use? (`section`, `level2`, etc.)
- [x] Document: How does Pandoc handle headers with no content following?
- [x] Save reference outputs for comparison tests

**Verified Pandoc Behavior (pandoc 3.8.3 with `--section-divs`):**

| Aspect | Behavior |
|--------|----------|
| **ID** | Moves from header to section. Header has NO ID in HTML output. |
| **Classes** | Duplicated on both section AND header. |
| **Attributes** | Key-value attributes (data-*, style, etc.) duplicated on both section AND header. |
| **levelN class** | Added ONLY to section, NOT to header. Format: `level2`, `level3`, etc. |
| **"section" class** | Pandoc does NOT add a "section" class. Uses `<section>` HTML tag directly. |
| **Empty sections** | Valid - section contains only the header with no other content. |
| **Content before headers** | Preserved outside any section (not wrapped). |
| **Nesting** | Based on header level. Section at level N closes when encountering header at level ≤ N. |
| **Auto-generated IDs** | Generated from header text (e.g., "Section A" → `section-a`). |
| **AST modification** | `--section-divs` does NOT modify AST (verified via `-t json`). It's purely an HTML writer feature. |

**Our implementation difference**: We add a `section` class to Divs so our HTML writer can identify them and emit `<section>` tags. This enables the transform to work at the AST level.

##### Step 2: Create pampa transforms module
- [x] Create `crates/pampa/src/transforms/mod.rs` module
- [x] Export module from `crates/pampa/src/lib.rs`

##### Step 3: Implement SectionizeTransform
- [x] Create `crates/pampa/src/transforms/sectionize.rs`
- [x] Implement `sectionize_blocks(blocks: Vec<Block>) -> Vec<Block>` function
- [x] Handle nested sections correctly (stack-based algorithm)
- [x] Move ID from header to section Div
- [x] Handle class duplication (if Pandoc does this)
- [x] Add `section` and `levelN` classes to section Divs

##### Step 4: Write unit tests for transform
- [x] Test flat sections at same level (h2, h2, h2) → sibling sections
- [x] Test nested sections (h2, h3, h3, h2) → h3s nested inside first h2
- [x] Test deep nesting (h1, h2, h3, h4) → each level nested inside parent
- [x] Test mixed levels (h2, h4, h3) → verify correct closing/opening
- [x] Test content before first header → preserved outside sections
- [x] Test empty sections (header with no following content)
- [x] Test headers with custom IDs → ID moved to section
- [x] Test headers with custom classes → verify class handling

##### Step 5: Modify HTML writer
- [x] Modify `crates/pampa/src/writers/html.rs` `Block::Div` handler
- [x] Check if classes contain "section"
- [x] Emit `<section>` tag instead of `<div>` when section class present
- [x] Write unit tests for the HTML writer change

##### Step 6: Wire into pampa binary
- [x] Update `crates/pampa/src/main.rs` to check for `format: html: section-divs: true`
- [x] Apply transform before HTML writing when enabled
- [x] Test manually with sample documents

##### Step 7: Integration tests
- [x] Create test files in `crates/pampa/tests/writers/html/section-divs/`
- [x] Write comparison tests against `pandoc --section-divs` output
- [x] Verify HTML output structure matches Pandoc

**Phase 1.0 Complete!** All work items have been implemented and tested.

**Success criteria**:
- `pampa -t html` with `format: html: section-divs: true` produces same section structure as Pandoc
- All unit tests pass (flat sections, nested sections, ID/class handling)
- Transform is exported from pampa crate for quarto-core to use

#### Phase 1.1: Quarto-specific Transforms (in quarto-core)

**Work items**:
- [x] Wire `pampa::transforms::sectionize_blocks` into quarto-core's `AstTransformsStage`
  - Created `SectionizeTransform` wrapper in `quarto-core/src/transforms/sectionize.rs`
  - Added to `build_transform_pipeline()` in `pipeline.rs`
  - Runs after TitleBlockTransform, before ResourceCollectorTransform
- [ ] Create `AnchoredHeadingsTransform` - add `.anchored` class to h2-h6 (DEFERRED)
- [ ] Create `CodeBlockTransform` - scaffold structure, class hoisting (DEFERRED)
- [ ] Create `ResponsiveTransform` - add Bootstrap responsive classes (DEFERRED)
- [ ] Create `BootstrapClassesTransform` - text styling classes (DEFERRED)
- [ ] Update `FigureTransform` (if exists) or create - figure/image classes (DEFERRED)
- [ ] Create `TableTransform` - Bootstrap table classes (DEFERRED)

**Success criteria**:
- [x] SectionizeTransform runs in correct order
- [x] Document body uses `<section>` tags for header regions
- [ ] Other transforms deferred until testing strategy is clearer

### Phase 1b: Document Structure Transforms

**Beads Issue**: kyoto-8ws
**Detailed Plan**: [`claude-notes/plans/2026-01-26-document-structure-transforms.md`](./2026-01-26-document-structure-transforms.md)

**Goal**: Implement AST transforms for document structure (footnotes, bibliography, appendix).

**Configuration Options** (detailed in subplan):
- `reference-location`: `document` | `section` | `block` | `margin` (default: `document`)
- `citation-location`: `document` | `margin` (default: `document`)
- `appendix-style`: `default` | `plain` | `none` (default: `default`)
- `footnotes-hover`: boolean (default: `true`, HTML only)

**Work items**:
- [x] Create `FootnotesTransform` (Phases A & B in subplan)
  - Walk AST to find all `Inline::Note` elements
  - Extract content, replace with reference link (e.g., `[^1]`)
  - Create footnotes section block at configurable location
  - Handle `reference-location` configuration
- [x] Create `AppendixStructureTransform` (Phase C in subplan)
  - Collect `Div` blocks with class `appendix`
  - Create appendix container structure
  - Position footnotes (and bibliography when available) within appendix
  - Handle `appendix-style` configuration
- [x] Verify integration into render paths (Phase D in subplan)
  - Both CLI and WASM paths use same `build_transform_pipeline()`

**Note**: Bibliography creation and placement will be handled by `CiteprocTransform` (future work), not a separate `BibliographyPlacementTransform`. Since we control the citeproc implementation, that transform will handle both citation processing AND bibliography placement according to `citation-location` config.

**Success criteria**:
- [x] Footnotes render correctly at end of document
- [ ] Bibliography appears in correct location - **DEFERRED** (requires CiteprocTransform)
- [x] Appendix sections are properly consolidated
- [x] **No DOM postprocessing needed for any of these**

### Phase 2: Enhanced Template System

**Goal**: Enrich the HTML template to handle metadata-driven content and semantic structure.

**Work items**:
- [ ] Wrap body content in `<main class="content" id="quarto-document-content">`
- [ ] Add `<header id="title-block-header">` generation from metadata (title, subtitle, authors, date, abstract)
- [ ] Add `<div id="quarto-content">` wrapper with page layout classes
- [ ] Add sidebar structure placeholders (for future TOC/margin features)
- [ ] Add body classes from format metadata
- [ ] Add draft alert conditional partial
- [ ] Add canonical URL support
- [ ] Add author/citation meta tags
- [ ] Add dark mode body class support
- [ ] Create template partials structure
- [ ] Add dependency injection markers

**Success criteria**:
- Template handles all metadata-driven output
- No DOM manipulation needed for these features
- Template is maintainable and extensible

### Phase 3: Minimal HTML Postprocessing Infrastructure (Deferred)

**Goal**: Create lean infrastructure for the few DOM-required operations.

**Status**: With the AST-first approach, this phase may not be needed for basic HTML parity. The remaining DOM operations (column layout, margin content) are all Phase 8 (Advanced Layout) concerns.

**If needed later**, work items would be:
- [ ] Add `scraper` crate to workspace
- [ ] Create `HtmlDocument` wrapper with minimal API
- [ ] Create `HtmlPostProcessor` trait
- [ ] Create `HtmlPostProcessingStage` pipeline stage
- [ ] Wire into pipeline between render and template stages
- [ ] Ensure WASM compatibility

**Key design**: Keep the API surface small. We only need:
- `select(selector)` - Query elements
- `set_attribute(el, name, value)` - Set attributes
- `add_class(el, class)` - Add CSS class
- `move_element(el, new_parent)` - Move element in tree
- `serialize()` - Output HTML string

**Decision point**: Evaluate after Phase 1/1b/2 whether DOM postprocessing is actually needed, or if all requirements can be met via AST transforms + template + client-side JS.

### Phase 4: JavaScript Dependencies

**Goal**: Embed and inject required JS libraries for client-side interactivity.

**Work items**:
- [ ] Create resource embedding system for JS files
- [ ] Embed quarto.js (or create minimal equivalent)
- [ ] Embed clipboard.js for copy buttons
- [ ] Embed anchor.js for anchor links
- [ ] Add script injection to template
- [ ] Test in native and WASM contexts

**Success criteria**:
- Copy buttons work via client-side JS
- Anchor links work via client-side JS
- No DOM postprocessing needed for these features

### Phase 5: Code Annotations (AST-First)

**Goal**: Implement code annotations at AST level.

**Work items**:
- [ ] Create `CodeAnnotationTransform`
  - Parse `# <1>` style markers in code blocks
  - Create definition list structure
  - Add data attributes for JS interactivity
- [ ] Create client-side JS for annotation hover/select
- [ ] Test with various annotation modes

**Success criteria**:
- Annotations work without DOM postprocessing
- Interactive features work via client-side JS

### Phase 6: Table of Contents (AST + Template)

**Goal**: Generate TOC from document headings.

**Work items**:
- [ ] Extract heading structure during AST traversal
- [ ] Store TOC data in document metadata
- [ ] Create TOC template partial
- [ ] Support TOC configuration options (depth, location)
- [ ] Add TOC scroll sync JS

**Success criteria**:
- TOC generated from AST, rendered via template
- No DOM manipulation for basic TOC
- Interactive features via JS

### Phase 7: Cross-References (AST Transform)

**Goal**: Implement cross-reference processing for figures, tables, equations, sections.

**Work items**:
- [ ] Create `CrossrefIndexTransform` - build index of referenceable elements
- [ ] Create `CrossrefResolveTransform` - resolve `@ref` syntax to links
- [ ] Create `SectionNumberingTransform` - number sections if enabled
- [ ] Create `FigureNumberingTransform` - number figures/tables
- [ ] Support crossref configuration options
- [ ] Write comprehensive tests

**Success criteria**:
- Cross-references resolve correctly
- Numbering matches TS Quarto behavior
- All done at AST level, no DOM manipulation

### Phase 8: Advanced Layout (Future)

**Goal**: Full Bootstrap layout support (marked as future work).

**Work items**:
- [ ] Column layout processing
- [ ] Margin content handling
- [ ] Tabset margin extraction
- [ ] Navbar implementation
- [ ] Sidebar implementation
- [ ] Footer implementation

---

## Technical Design

### AST Transform Pattern

```rust
/// Base trait for AST transforms
pub trait AstTransform: Send + Sync {
    /// Name for diagnostics
    fn name(&self) -> &'static str;

    /// Transform the document AST in place
    fn transform(&self, doc: &mut Pandoc, ctx: &TransformContext) -> Result<(), TransformError>;
}

/// Example: Add .anchored class to headings
pub struct AnchoredHeadingsTransform;

impl AstTransform for AnchoredHeadingsTransform {
    fn name(&self) -> &'static str {
        "anchored-headings"
    }

    fn transform(&self, doc: &mut Pandoc, ctx: &TransformContext) -> Result<(), TransformError> {
        // Walk AST, find Header blocks, add "anchored" to classes
        for block in doc.blocks.iter_mut() {
            if let Block::Header(level, attr, _) = block {
                if *level >= 2 && !attr.classes.contains(&"no-anchor".to_string()) {
                    attr.classes.push("anchored".to_string());
                }
            }
        }
        Ok(())
    }
}
```

### Document Structure Transform Pattern

```rust
/// Extract inline footnotes and create footnotes section
pub struct FootnotesTransform;

impl AstTransform for FootnotesTransform {
    fn name(&self) -> &'static str {
        "footnotes"
    }

    fn transform(&self, doc: &mut Pandoc, ctx: &TransformContext) -> Result<(), TransformError> {
        let mut footnotes: Vec<(String, Vec<Block>)> = vec![];
        let mut counter = 0;

        // 1. Walk AST, find all Inline::Note, extract content
        walk_inlines_mut(&mut doc.blocks, |inline| {
            if let Inline::Note(content) = inline {
                counter += 1;
                let id = format!("fn{}", counter);
                footnotes.push((id.clone(), std::mem::take(content)));

                // Replace with superscript link
                *inline = Inline::Superscript(vec![
                    Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(counter.to_string())],
                        Target { url: format!("#{}", id), title: String::new() },
                    )
                ]);
            }
        });

        // 2. Create footnotes section if any found
        if !footnotes.is_empty() {
            let footnotes_section = create_footnotes_block(footnotes);
            doc.blocks.push(footnotes_section);
        }

        Ok(())
    }
}
```

**Key principle**: The transform restructures the AST so that pampa's HTML writer can render it directly. The writer remains stateless - it just renders what's in the AST.

### Minimal HTML Postprocessor (Phase 8 Only - If Needed)

The following infrastructure would only be implemented if Phase 8 (Advanced Layout) requires DOM manipulation for column/margin processing:

```rust
/// Result of running an HTML postprocessor.
pub struct HtmlPostProcessResult {
    /// Relative paths to resources needed by this postprocessor.
    pub resources: Vec<String>,
}

/// Trait for HTML postprocessors (minimal surface area).
pub trait HtmlPostProcessor: Send + Sync {
    fn name(&self) -> &'static str;
    fn process(&self, doc: &mut HtmlDocument, ctx: &PostProcessContext) -> Result<HtmlPostProcessResult>;
}

/// Minimal HTML document wrapper
pub struct HtmlDocument {
    inner: scraper::Html,
}

impl HtmlDocument {
    pub fn parse(html: &str) -> Result<Self>;
    pub fn serialize(&self) -> String;
    pub fn select(&self, selector: &str) -> Vec<ElementRef>;
    pub fn move_element(&mut self, el: ElementRef, new_parent: ElementRef);
    pub fn set_attribute(&mut self, el: ElementRef, name: &str, value: &str);
    pub fn add_class(&mut self, el: ElementRef, class: &str);
}
```

**Note**: This infrastructure is NOT needed for basic HTML parity (Phases 1-7).

### Pipeline Integration

```rust
pub fn build_html_pipeline(config: &PipelineConfig) -> HtmlPipeline {
    pipeline![
        ParseDocumentStage::new(),
        EngineExecutionStage::new(),
        AstTransformsStage::new(vec![
            // Existing transforms
            Box::new(MetadataNormalizeTransform),
            Box::new(CalloutTransform),
            Box::new(TitleBlockTransform),

            // Class/attribute transforms
            Box::new(AnchoredHeadingsTransform),
            Box::new(CodeBlockTransform),
            Box::new(ResponsiveTransform),
            Box::new(BootstrapClassesTransform),
            Box::new(TableTransform),

            // Document structure transforms (new)
            Box::new(FootnotesTransform),
            Box::new(BibliographyPlacementTransform),
            Box::new(AppendixStructureTransform),
        ]),
        RenderHtmlBodyStage::new(),  // Stateless: just renders the AST
        ApplyTemplateStage::new(),
        // NOTE: No HtmlPostProcessingStage needed for basic HTML parity!
        // DOM postprocessing only added if Phase 8 (Advanced Layout) is implemented
    ]
}
```

**Key insight**: With proper AST transforms, the pipeline no longer needs a DOM postprocessing stage for basic HTML rendering. The HTML writer is stateless and simply renders the well-structured AST.

---

## Dependencies

### New Crate Dependencies

**For basic HTML parity (Phases 1-7)**: No new crate dependencies needed! All work is AST transforms.

**For Phase 8 (Advanced Layout), if needed**:
```toml
# Workspace Cargo.toml
[workspace.dependencies]
scraper = "0.18"  # HTML parsing (only for complex layout postprocessing)
```

### Files to Create

**In pampa crate (shared transforms):**

| File | Purpose |
|------|---------|
| `crates/pampa/src/transforms/mod.rs` | Module root for AST transforms |
| `crates/pampa/src/transforms/sectionize.rs` | Wrap headers in section Divs (analogous to Pandoc's `makeSectionsWithOffsets`) |

**In quarto-core crate (Quarto-specific transforms):**

| File | Purpose |
|------|---------|
| `crates/quarto-core/src/transform/anchored_headings.rs` | Anchored headings transform |
| `crates/quarto-core/src/transform/code_block.rs` | Code block scaffolding transform |
| `crates/quarto-core/src/transform/responsive.rs` | Responsive classes transform |
| `crates/quarto-core/src/transform/bootstrap_classes.rs` | Bootstrap text classes |
| `crates/quarto-core/src/transform/table.rs` | Table Bootstrap classes |
| `crates/quarto-core/src/transform/footnotes.rs` | Footnote extraction and section creation |
| `crates/quarto-core/src/transform/bibliography.rs` | Bibliography placement |
| `crates/quarto-core/src/transform/appendix.rs` | Appendix structure creation |
| `crates/quarto-core/resources/js/quarto.js` | Client-side interactivity |

### Files to Modify

| File | Changes |
|------|---------|
| `crates/pampa/src/lib.rs` | Export `transforms` module |
| `crates/pampa/src/writers/html.rs` | Emit `<section>` for Divs with `section` class |
| `crates/pampa/src/main.rs` | Apply `SectionizeTransform` when `section-divs: true` in frontmatter |
| `crates/quarto-core/src/template.rs` | Enhanced template with `<main>`, `<header>`, sidebar structure |
| `crates/quarto-core/src/transform/mod.rs` | Import and use `pampa::transforms::SectionizeTransform` |

**Note**: No `scraper` dependency or postprocessor infrastructure needed for basic HTML parity. These would only be added in Phase 8 (Advanced Layout) if required.

---

## Testing Strategy

### AST Transform Tests

For each transform:
- Input AST fixture → Transform → Expected output AST
- Edge cases (empty documents, nested structures)
- Class preservation (don't clobber existing classes)

### Integration Tests

- Full pipeline tests: QMD → HTML with expected structure
- Verify classes are present without DOM manipulation
- Comparison tests: Rust vs TS Quarto output similarity

### Client-Side JS Tests

- Browser tests for copy button functionality
- Browser tests for anchor link injection
- Verify JS works in both contexts

---

## Success Criteria

### Phase 1/1b Success (AST Transforms)
- New transforms run correctly in pipeline
- HTML output has expected classes and structure
- Footnotes render correctly without DOM manipulation
- Bibliography and appendix structure correct without DOM manipulation
- No DOM postprocessing needed for basic HTML parity

### Overall Success
- **All basic features implemented via AST transforms**
- **Zero DOM postprocessing for basic HTML parity**
- Client-side JS handles interactivity (copy buttons, anchors, tooltips)
- Rendered HTML matches TS Quarto structurally
- pampa HTML writer remains stateless

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| AST transforms can't cover all cases | Low | Medium | Identified DOM-required ops upfront; all are Phase 8 |
| Complex layout requires DOM | Medium | Low | Defer to Phase 8; basic parity doesn't need it |
| Client-side JS behavior differences | Medium | Low | Use same libraries as TS Quarto |
| pampa HTML writer needs changes | Low | Low | Writer is simple; AST changes are upstream |

---

## Open Questions (Resolved)

1. **How much DOM manipulation?**
   - **Decision**: Zero for basic HTML parity. All structural operations are AST transforms.

2. **Footnotes: writer state or AST transform?**
   - **Decision**: AST transform. `FootnotesTransform` extracts inline notes and creates a footnotes section. Writer stays stateless.

3. **Appendix consolidation: DOM or AST?**
   - **Decision**: AST transform. Since we control pampa, we structure the AST correctly before emission rather than fixing Pandoc's output.

4. **Client-side JS strategy?**
   - **Decision**: Embed quarto.js equivalent for interactive features (copy buttons, anchors, tooltips).

5. **When to add DOM postprocessing infrastructure?**
   - **Decision**: Defer until Phase 8 (Advanced Layout). Evaluate if actually needed after Phase 1/1b/2.

---

## References

### Internal
- Lua filter analysis: `claude-notes/plans/2026-01-24-lua-filter-analysis.md`
- SASS compilation plan: `claude-notes/plans/2026-01-13-sass-compilation.md`
- Rust Quarto pipeline: `crates/quarto-core/src/pipeline.rs`
- Current template: `crates/quarto-core/src/template.rs`

### External (TS Quarto)
- HTML postprocessors: `external-sources/quarto-cli/src/format/html/format-html.ts`
- Bootstrap postprocessor: `external-sources/quarto-cli/src/format/html/format-html-bootstrap.ts`
- Metadata postprocessor: `external-sources/quarto-cli/src/format/html/format-html-meta.ts`
- Notebook postprocessor: `external-sources/quarto-cli/src/format/html/format-html-notebook.ts`
- Lua filters: `external-sources/quarto-cli/src/resources/filters/`
- HTML template: `external-sources/quarto-cli/src/resources/formats/html/pandoc/template.html`
