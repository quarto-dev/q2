# Source Information Tracking for Structured Input Formats

**Date**: 2025-12-15
**Issue**: k-zr88
**Status**: Design finalized

## Problem Statement

Quarto supports input formats beyond plain `.qmd` files, including `.ipynb` (Jupyter notebooks) and percent scripts (`.py` with `#%%` markers). When errors occur in content from these formats, we need to:

1. Track source locations through the conversion pipeline
2. Display errors in a coordinate system meaningful to users
3. Support both human-readable terminal output and machine-readable `--json-errors`

The current `SourceInfo` infrastructure assumes "original" sources are plain text files where byte offsets translate naturally to line:column positions. This assumption breaks for structured formats like ipynb.

## Critical Constraints

1. **qmd files must exist on disk** - The qmd is the intermediate format. This is non-negotiable.

2. **SourceContext serialization is limited** - SourceContext is only serialized for PandocAST JSON. We cannot rely on in-memory state surviving across process boundaries.

3. **The qmd IS the working file** - SourceInfo will point to the qmd. We cannot change this fundamentally.

## Two Distinct Problems

### Problem 1: Storage of Source Mapping Data

When converting any format to qmd, we produce plain text on disk. Unlike Pandoc AST nodes (which have fields for SourceInfo), plain text has no place to store source mapping data inline.

**Question**: Where does the source mapping data live?

**Answer**: In a sidecar file alongside the qmd.

### Problem 2: Presentation of Structured Source Locations (ipynb-specific)

For ipynb files:
- The JSON representation is an implementation detail users shouldn't see
- Byte offsets in JSON are meaningless (escape sequences like `\n` distort positions)
- Users think in terms of cells, not JSON structure
- Error display should use the cell's *logical* content (unescaped strings)

**Question**: How do we represent and display locations in the "cell coordinate system"?

**Answer**: Use a new `NotebookCell` variant in `SourceInfo` for first-class notebook support. At error display time, create a custom ariadne Source from the cell content stored in `SourceContext`.

## Design Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Ariadne integration for ipynb | Custom ariadne Source from cell content | Allows proper snippet display using logical cell content |
| FileId assignment | Use `add_file_with_id()` | Ensures FileIds in sidecar match reconstructed context |
| Multi-cell span errors | Require well-formed cells | New syntax enforces this; detailed handling deferred |
| Errors in synthesized syntax | Point to qmd file | Indicates Quarto bug, not user error |
| Error-reporting integration | Approach 2 (store in SourceContext) | Cleaner API, follows existing patterns |
| Sidecar format | Unified envelope with format-specific mapping | "plain_text" for readable sources, "jupyter_notebook" for ipynb |
| NotebookCell in SourceInfo | Yes, first-class variant | ipynb is important enough to warrant core support |
| Column precision for percent scripts | Use Concat with per-line Original pieces | No additional information needed |
| LSP integration | Out of scope | Deferred to future work |
| Cleanup/caching | Out of scope | `.quarto/source-maps/` should be in `.gitignore` |
| Cell content storage | Ephemeral files in SourceContext | Cleanest integration with existing infrastructure |

## Analysis

### Plain-Text Source Formats (Percent Scripts, R Spin)

For percent scripts like:
```python
#%% [markdown]
# Hello World

#%%
print("hi")
```

The source of truth IS the `.py` file. Users see this file in their editor. Showing errors with `.py` file line numbers makes sense.

The conversion to qmd:
1. Strips `#%%` markers
2. Strips `# ` prefix from markdown lines
3. Wraps code in fences

The existing SourceInfo mechanism (Concat/Original/Substring) can track this, pointing back to the original `.py` file. The sidecar file stores this mapping.

**Key insight**: For plain-text sources, we can point directly to the original file. The sidecar stores the SourceInfo mapping; at parse time, we load it and construct appropriate SourceInfo structures.

#### Column Precision with Concat

When `# ` is stripped from markdown lines, we preserve precise column information by creating a Concat with per-line Original pieces that reference the post-prefix portions:

```python
# Line one
# Line two
# Line three
```

Becomes:
```rust
SourceInfo::Concat {
    pieces: [
        SourcePiece { source_info: Original { file_id, start: 2, end: 10 }, ... },  // "Line one"
        SourcePiece { source_info: Original { file_id, start: 13, end: 21 }, ... }, // "Line two"
        SourcePiece { source_info: Original { file_id, start: 24, end: 34 }, ... }, // "Line three"
    ]
}
```

Each Original skips the `# ` prefix (2 bytes), so column numbers map correctly. The SourceInfo object grows proportionally to line count, but this is acceptable. Future optimization could use a more efficient storage format if needed.

### Structured Source Formats (ipynb)

For ipynb, the situation is fundamentally different:

```json
{
  "cells": [{
    "cell_type": "markdown",
    "source": ["# Hello\n", "World"]
  }]
}
```

1. **Users never see this JSON** - they see cells in Jupyter
2. **JSON byte offsets are useless** - `\n` is 2 bytes in JSON, 1 byte when parsed
3. **The "logical content" is what matters** - the unescaped, joined string

When displaying an error, we want:
```
Error: Invalid syntax
  --> notebook.ipynb [cell 3, markdown]
   |
 2 | some broken code
   |      ^^^^^^ error here
```

NOT:
```
Error: Invalid syntax
  --> notebook.ipynb:847:23
   |
847|     "source": ["some broken code\n",
   |                      ^^^^^^ error here
```

**Key insight**: For ipynb, we need:
- A new `NotebookCell` variant in `SourceInfo` for first-class representation
- Cell logical content stored as ephemeral files in `SourceContext` (for ariadne snippets)
- Custom ariadne Source created from cell content at display time

## New SourceInfo Variant: NotebookCell

Add a new variant to `SourceInfo` in `quarto-source-map`:

```rust
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Rc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    FilterProvenance { filter_path: String, line: usize },

    /// Location within a Jupyter notebook cell
    ///
    /// Used for ipynb files where the JSON representation is not meaningful
    /// to users. The cell content is stored as an ephemeral file in SourceContext.
    NotebookCell {
        /// Path to the original .ipynb file
        notebook_path: String,
        /// Cell index (0-based)
        cell_index: usize,
        /// Cell ID (if available in notebook format)
        cell_id: Option<String>,
        /// Cell type: "code", "markdown", or "raw"
        cell_type: String,
        /// FileId of the ephemeral file containing cell content
        content_file_id: FileId,
        /// Byte offset within the cell content
        start_offset: usize,
        /// Byte offset within the cell content (exclusive)
        end_offset: usize,
    },
}
```

### NotebookCell Behavior

- **`map_offset()`**: Returns `MappedLocation` using `content_file_id` - the cell content is stored as an ephemeral file, so standard offset mapping works
- **Serialization**: Fully serializable via serde like other variants
- **Error display**: Special formatting shows `notebook.ipynb [cell 3, markdown]:2:5`
- **Ariadne integration**: Create custom ariadne Source from the ephemeral file content

### Why First-Class Support?

1. **ipynb is critical for Quarto** - Jupyter notebooks are a primary use case
2. **Type safety** - Notebook locations are clearly distinguished from file locations
3. **Rich metadata** - Cell index, ID, and type are first-class fields
4. **Consistent API** - `map_offset()` works uniformly (unlike FilterProvenance which returns None)

## Sidecar File Format

### Unified Envelope

All sidecar files share a common envelope structure:

```json
{
  "version": 1,
  "original_file": "path/to/source",
  "original_format": "plain_text" | "jupyter_notebook",
  "mapping": { /* format-specific */ }
}
```

- **`plain_text`**: For percent scripts, R spin scripts, and other human-readable formats where errors should point to the original file
- **`jupyter_notebook`**: For ipynb files where errors should use cell coordinates

### Sidecar for Plain-Text Formats

```json
{
  "version": 1,
  "original_file": "analysis.py",
  "original_format": "plain_text",
  "mapping": {
    "source_info": {
      "Concat": {
        "pieces": [
          {
            "source_info": {
              "Original": {
                "file_id": 0,
                "start_offset": 45,
                "end_offset": 120
              }
            },
            "offset_in_concat": 0,
            "length": 75
          }
        ]
      }
    },
    "files": [
      {
        "id": 0,
        "path": "analysis.py"
      }
    ]
  }
}
```

At parse time:
1. Load qmd
2. Check for sidecar
3. If present with `original_format: "plain_text"`:
   - Use `add_file_with_id()` to register files with their specified IDs
   - Reconstruct SourceInfo from the serialized structure
   - SourceInfo now points to original `.py` file

### Sidecar for Jupyter Notebooks

```json
{
  "version": 1,
  "original_file": "notebook.ipynb",
  "original_format": "jupyter_notebook",
  "mapping": {
    "cells": [
      {
        "qmd_byte_range": [0, 150],
        "cell_index": 0,
        "cell_id": "abc123",
        "cell_type": "markdown",
        "content": "# Title\n\nThis is the cell's logical content..."
      },
      {
        "qmd_byte_range": [165, 300],
        "cell_index": 1,
        "cell_id": "def456",
        "cell_type": "code",
        "content": "import pandas as pd\ndf = pd.read_csv('data.csv')"
      }
    ]
  }
}
```

Key fields:
- `qmd_byte_range`: Where this cell's content appears in the qmd (byte offsets)
- `cell_index`, `cell_id`, `cell_type`: Cell identification
- `content`: The cell's logical content (unescaped, joined) - stored as ephemeral file in SourceContext

At parse time:
1. Load qmd
2. Check for sidecar
3. If present with `original_format: "jupyter_notebook"`:
   - For each cell, add content as ephemeral file using `add_file_with_id()`
   - Create `NotebookCell` SourceInfo variants pointing to these ephemeral files
   - Map qmd byte ranges to NotebookCell SourceInfo

## Integration with SourceContext

### New Field: sourcemap_paths

Add to `SourceContext`:

```rust
pub struct SourceContext {
    files: Vec<SourceFile>,
    file_id_map: HashMap<usize, usize>,
    /// Maps qmd file paths to their sourcemap paths
    sourcemap_paths: HashMap<PathBuf, PathBuf>,
}
```

Methods:
```rust
impl SourceContext {
    /// Register a sourcemap for a qmd file
    pub fn register_sourcemap(&mut self, qmd_path: PathBuf, sourcemap_path: PathBuf) {
        self.sourcemap_paths.insert(qmd_path, sourcemap_path);
    }

    /// Get the sourcemap path for a qmd file
    pub fn get_sourcemap(&self, qmd_path: &Path) -> Option<&PathBuf> {
        self.sourcemap_paths.get(qmd_path)
    }
}
```

When loading a qmd that has a sourcemap, register it in the context. Error display code can then look up the sourcemap without needing extra parameters.

### Cell Content as Ephemeral Files

For ipynb cells, store the logical content as ephemeral files:

```rust
// During sourcemap loading for ipynb
for cell in sourcemap.cells {
    // Create ephemeral file for cell content
    let file_id = ctx.add_file(
        format!("{}#cell-{}", notebook_path, cell.cell_index),
        Some(cell.content.clone()),
    );

    // Now NotebookCell can reference this file_id
    let source_info = SourceInfo::NotebookCell {
        notebook_path: notebook_path.to_string(),
        cell_index: cell.cell_index,
        cell_id: cell.cell_id,
        cell_type: cell.cell_type,
        content_file_id: file_id,
        start_offset: 0,
        end_offset: cell.content.len(),
    };
}
```

This allows `map_offset()` to work uniformly - it finds the ephemeral file and computes row/column from the cell content.

## Conversion Flow

### Plain-Text Format Conversion

```rust
fn convert_percent_script(source_path: &Path, output_dir: &Path) -> Result<ConversionResult> {
    let content = fs::read_to_string(source_path)?;
    let mut qmd = String::new();
    let mut pieces = Vec::new();

    for cell in parse_percent_cells(&content) {
        let qmd_start = qmd.len();

        match cell.kind {
            CellKind::Code { language } => {
                qmd.push_str(&format!("```{{{}}}\n", language));
                // Track content (not fence) for source mapping
                let content_start = qmd.len();
                qmd.push_str(&cell.content);
                let content_end = qmd.len();
                qmd.push_str("\n```\n\n");

                // Create per-line Original pieces for precise column mapping
                for line_range in cell.line_ranges() {
                    pieces.push(SourcePiece {
                        source_info: SourceInfo::Original {
                            file_id: FileId(0),
                            start_offset: line_range.start,
                            end_offset: line_range.end,
                        },
                        offset_in_concat: /* computed */,
                        length: line_range.end - line_range.start,
                    });
                }
            }
            CellKind::Markdown => {
                // Similar, with per-line pieces that skip "# " prefix
            }
        }
    }

    let source_info = SourceInfo::Concat { pieces };

    // Write qmd
    let qmd_path = output_dir.join(source_path.with_extension("qmd").file_name().unwrap());
    fs::write(&qmd_path, &qmd)?;

    // Write sidecar
    let sidecar = Sidecar {
        version: 1,
        original_file: source_path.to_string_lossy().to_string(),
        original_format: "plain_text".to_string(),
        mapping: PlainTextMapping {
            source_info,
            files: vec![FileEntry { id: 0, path: source_path.to_string_lossy().to_string() }],
        },
    };
    let sidecar_path = sourcemap_path_for(&source_path);
    fs::write(&sidecar_path, serde_json::to_string_pretty(&sidecar)?)?;

    Ok(ConversionResult { qmd_path, sidecar_path })
}
```

### Jupyter Notebook Conversion

```rust
fn convert_notebook(notebook_path: &Path, output_dir: &Path) -> Result<ConversionResult> {
    let notebook = parse_ipynb(notebook_path)?;
    let mut qmd = String::new();
    let mut cells = Vec::new();

    for (index, cell) in notebook.cells.iter().enumerate() {
        let cell_content = cell.source_as_string();

        match cell.cell_type.as_str() {
            "code" => {
                qmd.push_str(&format!("```{{{}}}\n", cell.language()));
                let content_start = qmd.len();
                qmd.push_str(&cell_content);
                let content_end = qmd.len();
                qmd.push_str("\n```\n\n");

                cells.push(CellMapping {
                    qmd_byte_range: (content_start, content_end),
                    cell_index: index,
                    cell_id: cell.id.clone(),
                    cell_type: "code".to_string(),
                    content: cell_content,
                });
            }
            "markdown" => {
                let content_start = qmd.len();
                qmd.push_str(&cell_content);
                let content_end = qmd.len();
                qmd.push_str("\n\n");

                cells.push(CellMapping {
                    qmd_byte_range: (content_start, content_end),
                    cell_index: index,
                    cell_id: cell.id.clone(),
                    cell_type: "markdown".to_string(),
                    content: cell_content,
                });
            }
            "raw" => {
                // Similar handling
            }
            _ => {}
        }
    }

    // Write qmd file
    let qmd_path = output_dir.join(notebook_path.with_extension("qmd").file_name().unwrap());
    fs::write(&qmd_path, &qmd)?;

    // Write sidecar file
    let sidecar = Sidecar {
        version: 1,
        original_file: notebook_path.to_string_lossy().to_string(),
        original_format: "jupyter_notebook".to_string(),
        mapping: NotebookMapping { cells },
    };
    let sidecar_path = sourcemap_path_for(&notebook_path);
    fs::write(&sidecar_path, serde_json::to_string_pretty(&sidecar)?)?;

    Ok(ConversionResult { qmd_path, sidecar_path })
}
```

## Error Display Flow

### For NotebookCell SourceInfo

When displaying an error with `NotebookCell` source info:

```rust
fn render_notebook_cell_error(
    source_info: &SourceInfo,
    ctx: &SourceContext,
) -> String {
    if let SourceInfo::NotebookCell {
        notebook_path,
        cell_index,
        cell_id,
        cell_type,
        content_file_id,
        start_offset,
        end_offset,
    } = source_info {
        // Get cell content from ephemeral file
        let file = ctx.get_file(*content_file_id).unwrap();
        let content = file.content.as_ref().unwrap();

        // Create custom ariadne Source from cell content
        let source = ariadne::Source::from(content.as_str());

        // Build ariadne report with cell-aware header
        let report = Report::build(ReportKind::Error, (), *start_offset)
            .with_message("Error message")
            .with_label(
                Label::new(*start_offset..*end_offset)
                    .with_message("error details")
            )
            .finish();

        // Render with custom header showing cell info
        let mut output = Vec::new();
        report.write(source, &mut output).unwrap();

        // Prepend cell location header
        let header = format!(
            "  --> {} [cell {}, {}]\n",
            notebook_path,
            cell_index + 1,  // 1-indexed for users
            cell_type
        );

        header + &String::from_utf8(output).unwrap()
    } else {
        // Standard error display
        // ...
    }
}
```

### Human-Readable Output

```
Error: Invalid YAML syntax
  --> notebook.ipynb [cell 3, markdown]
   |
 2 | format: htlm
   |         ^^^^ unknown format 'htlm', did you mean 'html'?
```

### JSON Error Output (`--json-errors`)

```json
{
  "severity": "error",
  "message": "Invalid YAML syntax",
  "location": {
    "file": "notebook.ipynb",
    "type": "notebook_cell",
    "cell": {
      "index": 3,
      "id": "abc123",
      "type": "markdown"
    },
    "line": 2,
    "column": 9
  },
  "details": [
    "unknown format 'htlm', did you mean 'html'?"
  ]
}
```

Compare with standard text file:

```json
{
  "severity": "error",
  "message": "Invalid YAML syntax",
  "location": {
    "file": "document.qmd",
    "type": "text",
    "line": 42,
    "column": 9
  }
}
```

## Where to Store Sidecar Files

**Decision**: Store in `.quarto/source-maps/`, keyed by the original file path.

### Path Resolution

Sourcemaps are stored relative to the original file path:
```
project/
  notebooks/
    analysis.ipynb
    chapter1/
      intro.ipynb
  .quarto/
    source-maps/
      notebooks/
        analysis.ipynb.json
        chapter1/
          intro.ipynb.json
```

The sourcemap path mirrors the original file's path structure within `.quarto/source-maps/`.

### Linking qmd to Sourcemap

The render pipeline tracks which original file is being rendered. The sourcemap path is deterministic:

- Rendering: `notebooks/analysis.ipynb`
- Sourcemap: `.quarto/source-maps/notebooks/analysis.ipynb.json`

At error display time:
1. We have an error with SourceInfo
2. For `NotebookCell`: Cell info is in the variant itself
3. For `Original`/`Concat`: Check if qmd has registered sourcemap in SourceContext
4. Load sourcemap if needed, translate coordinates

**No scanning or reverse lookup needed** - the original file path is known throughout the pipeline, and the sourcemap path is derived directly from it.

## Errors in Synthesized Syntax

When converting ipynb to qmd, the converter adds syntax that doesn't exist in the original:
- Code fences (`` ```{python} ``)
- Cell separators
- YAML front matter formatting

If an error occurs in this synthesized syntax (e.g., invalid fence language), the error points to the qmd file. This is acceptable because:

1. The qmd is generated by Quarto - errors here indicate a Quarto bug
2. The user didn't write this syntax, so pointing to it is appropriate
3. The error message can still be actionable (e.g., "internal error in cell 3 conversion")

The sidecar's `qmd_byte_range` covers only the cell content, not the surrounding syntax. Errors outside cell content ranges fall back to qmd file coordinates.

## Multi-Cell Span Errors

The new qmd syntax requires markdown cells to be well-formed individually. This constraint:

1. Simplifies error reporting - an error is always within a single cell
2. Matches user expectations - cells are conceptually independent
3. Enables better error messages - we can say "cell 3 has unclosed code fence"

Detailed handling of edge cases (e.g., cross-cell reference errors) is deferred to a separate design session.

## Staleness Detection

**Decision**: Not implementing. Editing generated qmd is "at your own risk."

The qmd typically only exists on disk during rendering anyway (for third-party engine compatibility). If someone edits it, they're on their own for error locations.

## Out of Scope

The following are explicitly deferred:

1. **LSP integration** - Hover, go-to-definition, and diagnostics in original notebook format
2. **Cache management** - Automatic cleanup of `.quarto/source-maps/`
3. **`quarto clean` command** - Does not exist yet

Note: `.quarto/source-maps/` should be added to the default `.gitignore` for Quarto projects as it contains derived artifacts.

## Summary

**Architecture**:
- `SourceInfo` gains new `NotebookCell` variant for first-class ipynb support
- `SourceContext` gains `sourcemap_paths` field for qmd-to-sourcemap lookup
- Conversion produces qmd file + sidecar file (both on disk)
- Error display uses custom ariadne Source for notebook cells

**For ipynb**:
- New `NotebookCell` variant holds cell metadata and references ephemeral file with content
- Cell content stored as ephemeral files in SourceContext
- Custom ariadne Source created from cell content for error snippets
- Display shows: `notebook.ipynb [cell 3, markdown]:2:5`

**For plain-text formats** (percent scripts, R spin):
- Use existing `Concat`/`Original` with per-line pieces for precise column mapping
- Sidecar stores serialized SourceInfo pointing to original file
- SourceInfo reconstructed at parse time using `add_file_with_id()`
- Display shows original file coordinates

**Key benefit**: ipynb gets first-class support in the type system, while plain-text formats reuse existing infrastructure with no core changes beyond the sidecar mechanism.
