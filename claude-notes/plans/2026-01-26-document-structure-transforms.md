# Phase 1b: Document Structure Transforms

**Parent Plan**: `claude-notes/plans/2026-01-24-html-rendering-parity.md`
**Parent Epic**: kyoto-6jv
**Created**: 2026-01-26
**Status**: Planning

---

## Overview

This plan details the implementation of AST transforms for document structure: footnotes, bibliography placement, and appendix consolidation. The goal is to structure the AST correctly *before* HTML generation, so the HTML writer remains stateless.

### Prerequisites / Dependencies

- **Format normalization** (not yet implemented): These transforms assume document-root options like `reference-location` are lifted into format-specific configuration before transforms run. Until that's implemented, transforms will only see options explicitly set under `format: html:` in document frontmatter.

Following the AST-first principle from the parent plan:
> If a document transformation can be expressed as `Pandoc AST → Pandoc AST`, it should be an AST transform, not a DOM postprocessor.

---

## Configuration Design

### Configuration Sources

Configuration values for these transforms come from multiple sources, in priority order:
1. **Document frontmatter** - YAML in the QMD file
2. **Project configuration** - `_quarto.yml`
3. **Format defaults** - Built-in defaults for each format

The configuration is available to transforms via `RenderContext::format_metadata(key)`.

### Configuration Options

**Important**: All these options are **existing Quarto schema options** defined in `external-sources/quarto-cli/src/resources/schema/`. They are **document-level options** (placed at document root in YAML, not under `format: html:`).

#### Footnotes Configuration

**Schema source**: `document-footnotes.yml`

| Option | Type | Default | Formats | Description |
|--------|------|---------|---------|-------------|
| `reference-location` | enum | `"document"` | markdown, HTML, PDF, muse | Where footnotes are placed |
| `footnotes-hover` | bool | `true` | HTML only | Enable hover tooltips |

**`reference-location` values:**
- `"document"` - Collect all footnotes at end of document (in footnotes section)
- `"section"` - Place footnotes at end of each section (Pandoc handles this)
- `"block"` - Place footnotes at end of each top-level block (Pandoc handles this)
- `"margin"` - Convert footnotes to margin notes (requires margin layout)

**Note**: For `"block"` and `"section"`, Pandoc handles the placement during conversion. Our AST transform only needs to handle `"document"` and `"margin"` cases.

#### Bibliography Configuration

**Schema source**: `document-references.yml`

| Option | Type | Default | Formats | Description |
|--------|------|---------|---------|-------------|
| `citation-location` | enum | `"document"` | HTML doc only | Where bibliography entries appear |

**`citation-location` values:**
- `"document"` - Bibliography at end of document (moved to appendix if enabled)
- `"margin"` - Bibliography entries in margins next to citations

#### Appendix Configuration

**Schema source**: `document-layout.yml`

| Option | Type | Default | Formats | Description |
|--------|------|---------|---------|-------------|
| `appendix-style` | enum | `"default"` | HTML doc only | Appendix styling behavior |

**`appendix-style` values:**
- `"default"` - Standard appendix processing
- `"plain"` - Minimal appendix styling
- `"none"` - Disable appendix processing

### Reading Configuration in Transforms

**Assumption**: A future "format normalization" step will lift document-root options (like `reference-location`) into format-specific configuration. This means transforms can uniformly read from `ctx.format_metadata(key)` regardless of where the option was originally specified in YAML.

This keeps transforms simple - they don't need to know about multiple sources of truth for configuration.

```rust
impl FootnotesTransform {
    fn get_reference_location(&self, ctx: &RenderContext) -> ReferenceLocation {
        // Reads from format metadata - format normalization ensures
        // document-root options are lifted into format config
        ctx.format_metadata("reference-location")
            .and_then(|v| v.as_str())
            .map(ReferenceLocation::from_str)
            .unwrap_or(ReferenceLocation::Document)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferenceLocation {
    #[default]
    Document,
    Section,
    Block,
    Margin,
}

impl ReferenceLocation {
    pub fn from_str(s: &str) -> Self {
        match s {
            "section" => Self::Section,
            "block" => Self::Block,
            "margin" => Self::Margin,
            _ => Self::Document,
        }
    }
}
```

---

## Transform Specifications

### Conceptual Placement in TS Quarto's Filter Chain

These transforms don't exist in TS Quarto (the behavior is either hardcoded or handled by Pandoc), but it's useful to consider where they would conceptually fit in the Lua filter chain. This affects:
1. What user-defined Lua filters see at each injection point
2. The ordering of our transforms relative to other Quarto processing

| TS Quarto Phase | Description | Our Transforms |
|-----------------|-------------|----------------|
| `quarto_normalize_filters` | AST normalization | **FootnotesTransform** |
| `quarto_crossref_filters` | Cross-reference processing | (CrossrefTransform - future) |
| Post-crossref | After crossref resolution | **CiteprocTransform** (future - creates AND places bibliography) |
| `quarto_finalize_filters` | Final document structure | **AppendixStructureTransform** |

### 1. FootnotesTransform

**Conceptual Phase**: Normalization (early)

**Purpose**: Extract inline footnotes from the AST and create a consolidated footnotes section.

**Input AST Elements**:
- `Inline::Note` - Inline footnote with content
- `Inline::NoteReference` - Reference to a defined note
- `Block::NoteDefinitionPara` - Single-paragraph note definition
- `Block::NoteDefinitionFencedBlock` - Multi-paragraph note definition

**Output**:
- Inline notes replaced with superscript links (`<sup><a href="#fn1">1</a></sup>`)
- Note definitions collected and removed from their original positions
- Footnotes section appended to document (with class `footnotes` and role `doc-endnotes`)

**Algorithm**:

```
1. Walk the AST to find all note definitions (NoteDefinitionPara, NoteDefinitionFencedBlock)
   - Build map: note_id -> note_content
   - Remove definitions from their original positions

2. Walk the AST to find all inline notes (Note) and note references (NoteReference)
   - For Note: generate unique ID, store content, replace with superscript link
   - For NoteReference: verify ID exists in map, replace with superscript link

3. If reference-location == "document":
   - Create footnotes section div at end of document
   - Section has: id="footnotes", class="footnotes", role="doc-endnotes"
   - Each footnote is a paragraph/div with id="fn{N}" containing:
     - Original content
     - Backlink: <a href="#fnref{N}" class="footnote-back" role="doc-backlink">↩︎</a>

4. If reference-location == "margin":
   - Do NOT create footnotes section
   - Instead, wrap each replaced note in a margin container
   - The actual margin placement happens via CSS/layout
```

**Output HTML Structure** (for `reference-location: document`):

```html
<p>Some text<sup id="fnref1"><a href="#fn1" class="footnote-ref" role="doc-noteref">1</a></sup> more text.</p>

<!-- At end of document -->
<section id="footnotes" class="footnotes" role="doc-endnotes">
  <hr>
  <ol>
    <li id="fn1">
      <p>Footnote content.<a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a></p>
    </li>
  </ol>
</section>
```

**Configuration Behavior**:

| reference-location | Transform Action |
|-------------------|------------------|
| `document` | Collect all footnotes → create section at end |
| `margin` | Convert to margin notes (no section created) |
| `block` / `section` | No-op (Pandoc handles during rendering) |

### 2. CiteprocTransform (Future - Not Part of This Plan)

**Conceptual Phase**: Post-crossref (after CrossrefTransform)

**Purpose**: Process citations and create bibliography.

**Note**: This transform is NOT part of Phase 1b. It will be implemented as part of the citation/citeproc work. However, it's documented here because:
1. It replaces the originally-proposed `BibliographyPlacementTransform`
2. AppendixStructureTransform depends on its output

**Key Design Decision**: Since we control the citeproc implementation in Rust Quarto, `CiteprocTransform` will handle both:
- Citation processing (resolving `@cite` references)
- Bibliography creation AND placement (respecting `citation-location` config)

This is cleaner than having a separate "placement" transform because:
1. The transform has access to configuration
2. It creates the bibliography block
3. It can place it correctly from the start

**Expected Output**: A `Div` block with `id="refs"` placed according to `citation-location`:
- `"document"` → Placed at end of document (for AppendixStructureTransform to collect)
- `"margin"` → Margin citations handled inline (no separate bibliography block)

### 3. AppendixStructureTransform

**Conceptual Phase**: Finalization (late)

**Purpose**: Consolidate appendix sections, footnotes, and bibliography into the appendix structure.

**Input**:
- Div blocks with class `appendix`
- Footnotes section (from FootnotesTransform)
- Bibliography (from CiteprocTransform, when implemented)
- License/copyright/citation metadata

**Output**: Consolidated appendix container at end of document

**Appendix Section Order** (matching TS Quarto):
1. User-defined appendix sections (`.appendix` divs)
2. Bibliography/References (if `citation-location != "margin"`)
3. Footnotes (if `reference-location != "margin"`)
4. Reuse/License section (if `license` metadata present)
5. Copyright section (if `copyright` metadata present)
6. Citation section (if `citation` metadata present)

**Algorithm**:

```
1. Check if appendix processing is enabled:
   - Skip if format.metadata["book"] == true
   - Skip if appendix-style == "none" or appendix-style == false

2. Collect appendix contents:
   appendix_sections = []

   // User appendices
   for block in document.blocks:
       if block is Div with class "appendix":
           remove from document
           add to appendix_sections

   // Bibliography (if not margin)
   if citation-location != "margin" and bibliography exists:
       wrap bibliography in section with:
           id="quarto-bibliography"
           role="doc-bibliography"
       add to appendix_sections

   // Footnotes (if not margin)
   if reference-location != "margin" and footnotes section exists:
       remove from current position
       add to appendix_sections

   // License section (if configured)
   if license metadata exists:
       create license section
       add to appendix_sections

   // Copyright section (if configured)
   if copyright metadata exists:
       create copyright section
       add to appendix_sections

   // Citation section (if configured)
   if citation metadata exists:
       create citation section
       add to appendix_sections

3. Create appendix container:
   appendix = Div with id="quarto-appendix", class="default"
   appendix.content = appendix_sections

4. Append to document
```

**Output HTML Structure**:

```html
<div id="quarto-appendix" class="default">
  <!-- User appendix sections -->
  <section id="user-appendix-a" class="level1 appendix">
    <h1>Appendix A</h1>
    <p>User appendix content...</p>
  </section>

  <!-- Bibliography -->
  <section id="quarto-bibliography" class="level1" role="doc-bibliography">
    <h1>References</h1>
    <div id="refs">
      <!-- bibliography entries -->
    </div>
  </section>

  <!-- Footnotes -->
  <section id="footnotes" class="footnotes" role="doc-endnotes">
    <h1>Footnotes</h1>
    <ol>
      <li id="fn1">...</li>
    </ol>
  </section>
</div>
```

---

## Transform Execution Order

These transforms must run in a specific order, respecting the conceptual filter chain phases:

```
NORMALIZATION PHASE:
1. [Existing transforms: Callout, MetadataNormalize, TitleBlock, Sectionize]

2. FootnotesTransform
   - Normalizes footnote syntax into standard structure
   - Creates footnotes section that appendix will collect
   - User Lua filters (pre/post) see normalized footnote structure

CROSSREF PHASE (future):
3. [CrossrefTransform - not yet implemented]

POST-CROSSREF PHASE (future):
4. [CiteprocTransform - not yet implemented]
   - Will process citations and create bibliography
   - Will place bibliography according to citation-location config

FINALIZATION PHASE:
5. AppendixStructureTransform
   - Must run after FootnotesTransform (and CiteprocTransform when implemented)
   - Collects all appendix content into final structure

6. [ResourceCollectorTransform - unchanged]
```

Updated `build_transform_pipeline()`:

```rust
pub fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();

    // === NORMALIZATION PHASE ===
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(TitleBlockTransform::new()));
    pipeline.push(Box::new(SectionizeTransform::new()));
    pipeline.push(Box::new(FootnotesTransform::new()));  // NEW

    // === CROSSREF PHASE (future) ===
    // pipeline.push(Box::new(CrossrefTransform::new()));

    // === POST-CROSSREF PHASE (future) ===
    // pipeline.push(Box::new(CiteprocTransform::new()));

    // === FINALIZATION PHASE ===
    pipeline.push(Box::new(AppendixStructureTransform::new()));  // NEW

    // Resource collection (must be last - collects from final AST)
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}
```

---

## Implementation Phases

### Phase A: FootnotesTransform (Core)

**Goal**: Basic footnote collection with `reference-location: document`.

- [x] Create `crates/quarto-core/src/transforms/footnotes.rs`
- [x] Implement `FootnotesTransform` struct
- [x] Implement note definition collection (NoteDefinitionPara, NoteDefinitionFencedBlock)
- [x] Implement inline Note extraction and replacement
- [x] Implement NoteReference resolution
- [x] Create footnotes section block structure
- [x] Add to `transforms/mod.rs`
- [x] Wire into pipeline in `pipeline.rs`
- [x] Write unit tests for basic footnote collection

### Phase B: FootnotesTransform (Configuration)

**Goal**: Handle all `reference-location` options.

- [x] Add `ReferenceLocation` enum to a shared config module (`transforms/config.rs`)
- [x] Implement configuration reading from `RenderContext`
- [x] Implement `reference-location: margin` handling
  - No footnotes section created
  - Footnote refs get "margin-note" class
  - Full margin content placement deferred to CSS/layout
- [x] Add tests for each reference-location value
  - `test_margin_mode_no_section`
  - `test_block_section_modes_are_noop`
  - `test_document_mode_creates_section`
- [ ] Test with `footnotes-hover` metadata (passed through for client-side JS) - **DEFERRED** (metadata passthrough is a template/writer concern)

### Phase C: AppendixStructureTransform

**Goal**: Consolidate all appendix content.

- [x] Create `crates/quarto-core/src/transforms/appendix.rs`
- [x] Implement `.appendix` div collection
- [x] Implement appendix-style configuration checking (`default`, `plain`, `none`)
- [x] Implement appendix section ordering (user → bibliography → footnotes → license → copyright → citation)
- [x] Implement metadata-driven sections (license, copyright, citation)
- [x] Add to `transforms/mod.rs`
- [x] Wire into pipeline
- [x] Write unit tests (10 tests)

**Note**: Bibliography handling looks for any existing `Div` with `id="refs"`. Full citeproc integration will come when CiteprocTransform is implemented.

### Phase D: Integration Verification

**Goal**: Verify transforms are integrated into render paths.

**Already Complete** (by design):
- [x] CLI render path uses `build_transform_pipeline()` which includes FootnotesTransform and AppendixStructureTransform
- [x] WASM/hub-client render path uses the same `render_qmd_to_html()` pipeline
- [x] Both paths go through `AstTransformsStage` which applies the full transform pipeline

**Verification approach**: Rather than elaborate integration tests comparing HTML output to TS Quarto (which won't match due to missing features), we rely on:
1. Unit tests for each transform (already done - 10 footnotes tests, 10 appendix tests)
2. The unified pipeline architecture ensuring both render paths use the same transforms
3. Future full Lua filter chain integration testing when more features are complete

**Note**: Detailed integration tests comparing output to TS Quarto will be added after the full Lua filter chain is integrated.

---

## Files to Create

| File | Purpose |
|------|---------|
| `crates/quarto-core/src/transforms/footnotes.rs` | FootnotesTransform implementation |
| `crates/quarto-core/src/transforms/appendix.rs` | AppendixStructureTransform implementation |
| `crates/quarto-core/src/transforms/config.rs` | Shared configuration enums (ReferenceLocation, AppendixStyle, etc.) |

**Note**: `CiteprocTransform` (which handles bibliography creation AND placement) will be implemented separately as part of the citation/citeproc work, not in this plan.

## Files to Modify

| File | Changes |
|------|---------|
| `crates/quarto-core/src/transforms/mod.rs` | Add new transform exports |
| `crates/quarto-core/src/pipeline.rs` | Add transforms to pipeline |

---

## Testing Strategy

### Unit Tests (per transform)

**FootnotesTransform**:
- Single inline note → extracted and section created
- Multiple notes → correct numbering
- Note definitions → collected and removed
- Note references → resolved correctly
- Mixed inline notes and references
- Empty document (no notes)
- `reference-location: margin` → no section created

**AppendixStructureTransform**:
- No appendix content → no appendix created
- Only user appendices → ordered correctly
- Footnotes section → moved into appendix with correct order
- All sections present → full ordering (user → bibliography → footnotes → metadata sections)
- `appendix-style: none` → no appendix processing
- Book format → no appendix processing
- Bibliography present (from test fixture) → included in correct position

### Integration Tests

Test documents:
1. `footnotes-basic.qmd` - Simple footnotes, default config
2. `footnotes-margin.qmd` - Margin footnotes
3. `appendix-user-sections.qmd` - User-defined appendix sections
4. `appendix-with-footnotes.qmd` - Footnotes consolidated into appendix
5. `no-appendix.qmd` - `appendix-style: none`

**Note**: Bibliography integration tests will be added when CiteprocTransform is implemented.

---

## Open Questions

1. ~~**Citeproc integration**: How does bibliography get into the AST?~~
   - **RESOLVED**: `CiteprocTransform` (future) will handle both citation processing AND bibliography creation/placement. AppendixStructureTransform will look for `Div` with `id="refs"` from CiteprocTransform's output.

2. **Margin layout**: The margin transforms mark content for margins, but actual margin rendering requires:
   - CSS classes (`column-margin`)
   - Page layout activation
   - Should this be a separate concern?

3. **Footnote numbering reset**: Should footnote numbers reset per-section for multi-section documents?
   - TS Quarto uses document-wide numbering
   - Probably follow that pattern

---

## Success Criteria

- [x] Footnotes render correctly at end of document with default configuration
- [x] User appendix sections are properly consolidated
- [x] Footnotes moved into appendix when appendix processing is enabled
- [x] Configuration options (`reference-location`, `appendix-style`) work as documented
- [x] **No DOM postprocessing needed for any of these features**
- [x] All unit tests pass (27 new tests: 10 footnotes, 10 appendix, 7 config)
- [ ] Integration tests produce output matching TS Quarto structure - **DEFERRED** (will test after full Lua filter chain integration)

**Deferred to CiteprocTransform**: Bibliography placement will be tested when citation processing is implemented.
