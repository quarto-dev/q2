# HTML Writer Source Location Tracking

**Issue:** k-02o9
**Created:** 2025-12-21
**Status:** Design Phase

## Goal

Enable "click in HTML preview → highlight in source editor" functionality by embedding source location information in HTML output. This supports interactive preview features in IDEs and LSP integrations.

## Background

### Current State

1. **AST nodes have source info**: Every Pandoc AST node has a `source_info: SourceInfo` field
2. **JSON writer has infrastructure**: The JSON writer already:
   - Builds a pool of unique `SourceInfo` objects with IDs
   - Can resolve locations to `{file_id, line, col}` via `resolve_location()`
   - Serializes the pool in `astContext.sourceInfoPool`
3. **HTML writer has no tracking**: Currently emits plain HTML with no source correlation

### Key Types

- `SourceInfo` (quarto-source-map): Enum tracking location with transformation history
  - `Original { file_id, start_offset, end_offset }`
  - `Substring { parent, start_offset, end_offset }`
  - `Concat { pieces }`
  - `FilterProvenance { filter_path, line }`
- `ASTContext`: Contains `SourceContext` and `filenames` for mapping
- `resolve_location()`: Maps `SourceInfo` → `{file_id, begin: {offset, line, col}, end: {offset, line, col}}`

## Design Options

### Option A: Direct Inline Locations

Emit resolved locations directly on HTML elements:
```html
<p data-loc="0:5:1-5:42">Some paragraph text</p>
<h1 data-loc="0:1:1-1:15">Title</h1>
```

Format: `data-loc="file_id:start_line:start_col-end_line:end_col"`

**Pros:**
- Self-contained HTML, no external data needed
- Simple to consume in JavaScript
- Directly actionable for editor integration

**Cons:**
- Increases HTML size (adds ~20-30 bytes per element)
- Loses transformation chain information (usually not needed)
- Some synthetic nodes have no meaningful location

### Option B: Source Info Pool (like JSON writer)

Emit pool IDs on elements, embed pool in HTML:
```html
<p data-source-id="42">Text</p>
<script type="application/json" id="quarto-source-map">
  {"pool": [...], "files": [...]}
</script>
```

**Pros:**
- Compact HTML elements (just an integer)
- Preserves full transformation chain
- Reuses JSON writer serialization code

**Cons:**
- Requires two-step lookup (ID → pool → location)
- Pool can be large for complex documents
- More complex JavaScript consumption

### Option C: Sidecar Source Map File

Write HTML normally, emit source map to separate file:
```
output.html
output_files/source-map.json
```

**Pros:**
- HTML remains unchanged for production use
- Source map only loaded when needed
- Clean separation of concerns

**Cons:**
- Two files to manage
- Need to correlate elements (requires IDs anyway)
- Cache invalidation complexity

### Option D: Hybrid Approach (Recommended)

Combine inline locations with optional embedded pool:

1. **Default mode**: Emit compact `data-loc` attributes with resolved locations
2. **Detailed mode**: Also embed the source info pool for advanced use cases

```html
<p data-loc="0:5:1-5:42" data-sid="42">Text</p>
<!-- Only in detailed mode: -->
<script type="application/json" id="quarto-source-map">...</script>
```

**Pros:**
- Simple use case works with just `data-loc`
- Advanced use cases have full pool available
- Configurable based on needs

## Recommended Design: Option D (Hybrid)

### Configuration

Add to `HtmlConfig` (new struct for HTML writer):
```rust
pub struct HtmlConfig {
    /// Include source locations on HTML elements
    pub include_source_locations: bool,
    /// Include full source info pool (for detailed debugging)
    pub include_source_pool: bool,
}
```

### HTML Output Format

For `include_source_locations = true`:
```html
<p data-loc="0:5:1-5:42">Paragraph text</p>
<h2 data-loc="0:10:1-10:20" id="section-title">Section Title</h2>
<div class="callout" data-loc="0:15:1-25:4">
  <div class="callout-header" data-loc="0:15:1-15:30">Warning</div>
  <div class="callout-body" data-loc="0:16:1-24:4">Content...</div>
</div>
```

Location format: `file_id:start_line:start_col-end_line:end_col`
- Lines are 1-based (matching editor conventions)
- Columns are 1-based
- file_id maps to filenames in the pool

### JavaScript API

```javascript
// Parse data-loc attribute
function parseLocation(dataLoc) {
  const match = dataLoc.match(/(\d+):(\d+):(\d+)-(\d+):(\d+)/);
  if (!match) return null;
  return {
    fileId: parseInt(match[1]),
    start: { line: parseInt(match[2]), col: parseInt(match[3]) },
    end: { line: parseInt(match[4]), col: parseInt(match[5]) }
  };
}

// Get filename (if pool embedded)
function getFilename(fileId) {
  const pool = document.getElementById('quarto-source-map');
  if (!pool) return null;
  const data = JSON.parse(pool.textContent);
  return data.files[fileId]?.name;
}
```

### Implementation Architecture

1. **Create `HtmlWriterContext`** (similar to `JsonWriterContext`)
   - Holds `ASTContext` reference for location resolution
   - Optionally builds source info pool
   - Tracks config

2. **Modify `write_block` / `write_inline` signatures**
   - Add context parameter
   - Emit `data-loc` when configured

3. **Add `resolve_and_format_location` helper**
   - Takes `SourceInfo` and `ASTContext`
   - Returns formatted string `"file:line:col-line:col"` or `None`

4. **Embed pool at end of document** (if configured)
   - Serialize pool as JSON
   - Wrap in `<script type="application/json">`

## Implementation Plan

### Phase 1: Infrastructure
- [ ] Create `HtmlConfig` struct with source tracking options
- [ ] Create `HtmlWriterContext` to hold config and AST context
- [ ] Add `resolve_and_format_location()` helper function
- [ ] Update `write_blocks` / `write_inlines` signatures to accept context

### Phase 2: Location Emission
- [ ] Update `write_attr` to optionally emit `data-loc`
- [ ] Add location emission to block-level elements (p, div, pre, etc.)
- [ ] Add location emission to inline elements (span, a, code, etc.)
- [ ] Handle elements without meaningful locations (synthetic nodes)

### Phase 3: Pool Embedding
- [ ] Port `SourceInfoSerializer` from JSON writer (or extract to shared module)
- [ ] Add pool embedding at document end
- [ ] Include file metadata in embedded JSON

### Phase 4: Integration
- [ ] Update render pipeline to pass config through
- [ ] Add CLI flags for source tracking (--source-map, --source-locations)
- [ ] Add tests for location accuracy

### Phase 5: Consumer Support
- [ ] Document JavaScript API for consuming locations
- [ ] Create example preview integration
- [ ] Test with LSP/editor workflows

## Open Questions

1. **Granularity**: Should we emit locations on every element or only "structural" elements?
   - Every element: More precise but verbose
   - Structural only: Cleaner but may miss inline elements
   - **Tentative answer**: Emit on all elements that map to AST nodes

2. **Synthetic elements**: What about HTML elements that don't directly map to AST nodes?
   - Callout wrapper divs, list items, table cells, etc.
   - **Tentative answer**: Inherit location from parent AST node, or omit

3. **Performance**: Is location resolution expensive?
   - Need to benchmark `resolve_location()` on large documents
   - May need caching if expensive

4. **Transformation tracking**: Do we need the full pool, or are resolved locations sufficient?
   - For basic "jump to source", resolved locations are sufficient
   - For debugging transform pipelines, full pool is valuable
   - **Tentative answer**: Make pool optional, default to just resolved locations

## Related Work

- **Source maps in browsers**: The CSS/JS source map format (`.map` files) is more complex but solves similar problems
- **Pandoc's --track-changes**: Tracks document revisions, different goal
- **CodeMirror/Monaco source mapping**: Editors use similar concepts for syntax highlighting

## Next Steps

1. Prototype `resolve_and_format_location()` helper
2. Test location accuracy on sample documents
3. Decide on granularity (all elements vs structural only)
4. Implement Phase 1 infrastructure
