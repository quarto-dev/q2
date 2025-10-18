# Surface Syntax Converter Design

**Date**: 2025-10-13
**Topic**: Separating surface syntax conversion from execution engines
**Status**: Design proposal and analysis

## Executive Summary

**Proposal**: Separate surface syntax conversion (`.ipynb`, percent scripts, R spin scripts → qmd) from execution engines by creating a centralized converter registry that operates independently of engines.

**Verdict**: ✅ **Strongly Recommended** - This separation significantly improves architecture, extensibility, and maintainability. Key challenges are solvable with careful API design.

**Key Insight**: Surface syntax conversion is **orthogonal to execution**. Current architecture conflates these concerns by requiring engines to know about all input formats they support, even though conversion is pure text transformation independent of computation.

## Current Architecture Analysis

### How It Works Today (quarto-cli TypeScript)

#### File Claiming Chain

```typescript
// 1. Engine claims file by extension OR content inspection
jupyterEngine.claimsFile(file, ext)
  → checks: .ipynb OR isJupyterPercentScript(file)

knitrEngine.claimsFile(file, ext)
  → checks: .rmd/.rmarkdown OR isKnitrSpinScript(file)

// 2. Engine provides conversion via markdownForFile()
jupyterEngine.markdownForFile(file)
  → if .ipynb: markdownFromNotebookJSON()
  → if percent: markdownFromJupyterPercentScript()
  → else: mappedStringFromFile()

knitrEngine.markdownForFile(file)
  → if R spin: markdownFromKnitrSpinScript() [calls R]
  → else: mappedStringFromFile()
```

**Location findings** (quarto-cli/src/execute/):
- `types.ts:33-34` - ExecutionEngine interface defines `claimsFile()` and `claimsLanguage()`
- `types.ts:35` - ExecutionEngine interface defines `markdownForFile()`
- `jupyter/jupyter.ts:151-174` - Jupyter engine implementation
  - Claims `.ipynb` files (line 152)
  - Claims files where `isJupyterPercentScript()` returns true (line 153)
  - Converts via `markdownFromNotebookJSON()` (line 166) or `markdownFromJupyterPercentScript()` (line 169)
- `rmd.ts:68-83` - Knitr engine implementation
  - Claims `.rmd`/`.rmarkdown` files (line 69)
  - Claims files where `isKnitrSpinScript()` returns true (line 70)
  - Converts via `markdownFromKnitrSpinScript()` (line 80)

**Conversion function locations**:
- `core/jupyter/jupyter-filters.ts:33` - `markdownFromNotebookJSON()` (pure JS, ~10 lines)
- `execute/jupyter/percent.ts:34` - `markdownFromJupyterPercentScript()` (pure JS, ~60 lines)
- `execute/rmd.ts:428` - `markdownFromKnitrSpinScript()` (calls R's `knitr::spin()`)

### Current Coupling Points

#### 1. Engines Must Know All Surface Syntaxes

```
JupyterEngine knows about:
├── .ipynb files (via isJupyterNotebook check)
├── .py/.jl/.r percent scripts (via isJupyterPercentScript)
└── .qmd files (default)

KnitrEngine knows about:
├── .rmd/.rmarkdown files (by extension)
├── .R spin scripts (via isKnitrSpinScript)
└── .qmd files (default)
```

**Problem**: Adding a new surface syntax (e.g., Observable notebooks) requires modifying engine code, even though engines don't care about input format - they only execute code.

#### 2. Conversion Logic Scattered Across Codebase

- Notebook conversion: `src/core/jupyter/jupyter-filters.ts` (core utilities)
- Percent scripts: `src/execute/jupyter/percent.ts` (engine-specific directory)
- Spin scripts: `src/execute/rmd.ts` (embedded in engine file)

**Problem**: No single place to understand or extend surface syntax support.

#### 3. Two-Stage Conversion for Jupyter

```
.ipynb → qmd → transient .quarto_ipynb notebook → execute
   ↑              ↑
   |              └─ Engine-specific (jupyter needs notebooks)
   └─ Should be independent!
```

The second stage (qmd → notebook) is legitimately engine-specific (jupyter's execution model requires notebooks). But the first stage (ipynb → qmd) is pure syntax transformation.

#### 4. Testing Complexity

Current tests must:
- Mock entire engine infrastructure
- Test conversion + execution together
- Duplicate test files across engine implementations

### Conversion Implementation Characteristics

| Format | Implementation | Lines | Complexity | Engine Dependency |
|--------|---------------|-------|------------|-------------------|
| `.ipynb` | Pure JS | ~10 | Low | None (just extracts markdown/raw cells) |
| Percent scripts | Pure JS | ~60 | Medium | None (text parsing) |
| R spin scripts | Calls R | ~20 | Medium | **R runtime required** |

**Key observation**: 2 of 3 converters are pure text transformation with NO runtime dependencies. R spin could be implemented in Rust (similar complexity to percent scripts).

## Proposed Architecture

### Core Concept

```
┌─────────────────────────────────────────────────────────────┐
│                       Input File                             │
└─────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────┐
│              SourceConverterRegistry                         │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ IpynbConverter           .ipynb → qmd                   │ │
│  │ PercentScriptConverter   .py/.jl/.r (with %%) → qmd    │ │
│  │ RSpinConverter          .R (with #' ---) → qmd         │ │
│  │ [Future: RustdocConverter, ObservableConverter, ...]  │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                           ↓
                   ConvertedSource
                  {qmd, metadata, source_map, suggested_engine}
                           ↓
┌─────────────────────────────────────────────────────────────┐
│              ExecutionEngineRegistry                         │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ JupyterEngine   (executes qmd with python/julia/...)   │ │
│  │ KnitrEngine     (executes qmd with R)                  │ │
│  │ MarkdownEngine  (no execution)                         │ │
│  │ [Future: third-party engines...]                       │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Rust API Design

```rust
// ============================================================================
// CONVERTER TRAIT
// ============================================================================

pub trait SourceConverter: Send + Sync {
    /// Unique converter name (e.g., "ipynb", "percent-script", "r-spin")
    fn name(&self) -> &str;

    /// Check if this converter can handle the file
    /// content_hint: Optional first ~1KB for fast inspection
    fn claims_file(&self, path: &Path, content_hint: Option<&str>) -> bool;

    /// Convert source file to qmd
    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource>;
}

pub struct ConverterInput {
    pub path: PathBuf,
    pub content: String,
    pub options: ConvertOptions,  // e.g., ipynb-filters, project context
}

pub struct ConvertedSource {
    /// QMD content (markdown with YAML front matter + code cells)
    pub qmd: String,

    /// Source location mapping for error reporting and LSP
    /// Maps qmd positions → original file positions
    pub source_map: SourceMap,

    /// Suggested engine based on source inspection
    /// e.g., .ipynb with python kernel → Some("jupyter")
    pub suggested_engine: Option<String>,

    /// Metadata extracted from source (merged with YAML)
    /// e.g., ipynb kernelspec, percent script language
    pub metadata: Metadata,

    /// Original format identifier (for special handling)
    /// e.g., "ipynb" → preserve `execute.ipynb: false` default
    pub original_format: String,
}

// ============================================================================
// CONVERTER REGISTRY
// ============================================================================

pub struct ConverterRegistry {
    converters: Vec<Box<dyn SourceConverter>>,
}

impl ConverterRegistry {
    pub fn with_defaults() -> Self {
        Self {
            converters: vec![
                Box::new(IpynbConverter),
                Box::new(PercentScriptConverter),
                Box::new(RSpinConverter),
            ],
        }
    }

    /// Register custom converter (for library users, custom builds)
    pub fn register(&mut self, converter: Box<dyn SourceConverter>) {
        self.converters.push(converter);
    }

    /// Find converter for file (returns first match)
    pub fn find_converter(&self, path: &Path) -> Result<Option<&dyn SourceConverter>> {
        // Read first 1KB for content inspection (cheap)
        let content_hint = read_file_prefix(path, 1024)?;

        for converter in &self.converters {
            if converter.claims_file(path, Some(&content_hint)) {
                return Ok(Some(converter.as_ref()));
            }
        }

        Ok(None)
    }
}

// ============================================================================
// ENGINE TRAIT (SIMPLIFIED)
// ============================================================================

pub trait ExecutionEngine: Send + Sync {
    fn name(&self) -> &str;

    /// Check if engine can handle this language
    /// Only used for .qmd files with code blocks
    fn claims_language(&self, language: &str) -> bool;

    /// Execute code in qmd
    /// No more need for claimsFile() or markdownForFile()!
    fn execute(&self, target: &ExecutionTarget) -> Result<ExecuteResult>;
}

pub struct ExecutionTarget {
    pub source_path: PathBuf,         // Original file path
    pub qmd: String,                   // QMD content (converted or original)
    pub metadata: Metadata,            // Merged metadata
    pub original_format: Option<String>, // e.g., Some("ipynb")
    pub source_map: Option<SourceMap>, // For error reporting
}
```

### File Processing Flow

```rust
pub async fn render_file(path: &Path, options: &RenderOptions) -> Result<RenderOutput> {
    // ========================================================================
    // STAGE 1: OPTIONAL SURFACE SYNTAX CONVERSION
    // ========================================================================

    let converted = if let Some(converter) = converter_registry.find_converter(path)? {
        info!("Converting {} using {} converter", path.display(), converter.name());

        let input = ConverterInput {
            path: path.to_path_buf(),
            content: read_to_string(path)?,
            options: ConvertOptions {
                project: options.project.clone(),
                ipynb_filters: options.ipynb_filters.clone(),
            },
        };

        Some(converter.convert(&input)?)
    } else {
        None
    };

    // ========================================================================
    // STAGE 2: PREPARE QMD CONTENT
    // ========================================================================

    let (qmd, metadata, original_format, source_map) = if let Some(conv) = converted {
        (conv.qmd, conv.metadata, Some(conv.original_format), Some(conv.source_map))
    } else {
        // No conversion needed - already qmd
        let content = read_to_string(path)?;
        let metadata = extract_yaml_metadata(&content)?;
        (content, metadata, None, None)
    };

    // ========================================================================
    // STAGE 3: APPLY FORMAT-SPECIFIC DEFAULTS
    // ========================================================================

    // Special handling based on original format
    let mut format = resolve_format(&metadata, options)?;
    if original_format.as_deref() == Some("ipynb") {
        // .ipynb files default to execute: false
        format.execute.ipynb = format.execute.ipynb.or(Some(false));
    }

    // ========================================================================
    // STAGE 4: SELECT ENGINE
    // ========================================================================

    let engine = select_engine(
        &qmd,
        &metadata,
        converted.as_ref().and_then(|c| c.suggested_engine.as_deref()),
        &engine_registry,
    )?;

    info!("Selected engine: {}", engine.name());

    // ========================================================================
    // STAGE 5: EXECUTE
    // ========================================================================

    let target = ExecutionTarget {
        source_path: path.to_path_buf(),
        qmd,
        metadata,
        original_format,
        source_map,
    };

    engine.execute(&target)
}

fn select_engine(
    qmd: &str,
    metadata: &Metadata,
    suggested_engine: Option<&str>,
    registry: &EngineRegistry,
) -> Result<&dyn ExecutionEngine> {
    // 1. Explicit engine in YAML
    if let Some(engine_name) = metadata.get("engine").and_then(|v| v.as_str()) {
        return registry.get_engine(engine_name)
            .ok_or_else(|| anyhow!("Unknown engine: {}", engine_name));
    }

    // 2. Converter suggestion (e.g., .ipynb with python → jupyter)
    if let Some(name) = suggested_engine {
        if let Some(engine) = registry.get_engine(name) {
            return Ok(engine);
        }
    }

    // 3. Inspect code block languages
    let languages = extract_code_block_languages(qmd);
    for lang in languages {
        for engine in registry.engines() {
            if engine.claims_language(&lang) {
                return Ok(engine);
            }
        }
    }

    // 4. Default to markdown (no computation)
    Ok(registry.get_engine("markdown").unwrap())
}
```

### Example Converter Implementation

```rust
// ============================================================================
// PERCENT SCRIPT CONVERTER
// ============================================================================

pub struct PercentScriptConverter;

impl SourceConverter for PercentScriptConverter {
    fn name(&self) -> &str {
        "percent-script"
    }

    fn claims_file(&self, path: &Path, content_hint: Option<&str>) -> bool {
        // Check extension
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(ext, "py" | "jl" | "r") {
            return false;
        }

        // Check for percent script marker in first 1KB
        if let Some(content) = content_hint {
            content.contains("#%%")
        } else {
            false
        }
    }

    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource> {
        let ext = input.path.extension().and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("No file extension"))?;

        let language = match ext {
            "py" => "python",
            "jl" => "julia",
            "r" => "r",
            _ => return Err(anyhow!("Unsupported extension: {}", ext)),
        };

        // Parse cells
        let cells = parse_percent_cells(&input.content)?;

        // Convert to qmd
        let mut qmd = String::new();
        let mut source_map = SourceMap::new();

        for cell in cells {
            let start_pos = qmd.len();

            match cell.kind {
                CellKind::Code => {
                    qmd.push_str(&format!("```{{{}}}\n", language));
                    qmd.push_str(&cell.content);
                    qmd.push_str("\n```\n\n");
                }
                CellKind::Markdown => {
                    qmd.push_str(&cell.content);
                    qmd.push_str("\n\n");
                }
                CellKind::Raw => {
                    qmd.push_str("```{=html}\n");
                    qmd.push_str(&cell.content);
                    qmd.push_str("\n```\n\n");
                }
            }

            // Record mapping: qmd range → original file range
            source_map.add_mapping(
                start_pos..qmd.len(),
                cell.source_range.clone(),
            );
        }

        Ok(ConvertedSource {
            qmd,
            source_map,
            suggested_engine: Some("jupyter".to_string()),
            metadata: Metadata::new(), // Could extract from cells
            original_format: format!("percent-{}", ext),
        })
    }
}

// Parsing logic (~60 lines, same as TypeScript)
fn parse_percent_cells(content: &str) -> Result<Vec<Cell>> {
    // Implementation details...
}
```

### Future Converter Example: Rustdoc

**Motivation**: Rust source files with rustdoc comments could serve as Quarto source, expanding output format options beyond HTML.

**Current situation**:
- Rustdoc (Rust's documentation tool) generates HTML from doc comments
- No way to produce PDF, presentations, or other formats
- Doc comments use well-specified syntax: `///` and `//!`

**Value proposition with Quarto**:
- Convert `.rs` → qmd → any Quarto format (revealjs, PDF, typst, docx, etc.)
- Leverage Quarto's rich feature set (citations, cross-refs, diagrams, executable code)
- Maintain source code as single source of truth

**Example converter implementation**:

```rust
pub struct RustdocConverter;

impl SourceConverter for RustdocConverter {
    fn name(&self) -> &str {
        "rustdoc"
    }

    fn claims_file(&self, path: &Path, content_hint: Option<&str>) -> bool {
        // Check extension
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            return false;
        }

        // Check for rustdoc markers in first 1KB
        if let Some(content) = content_hint {
            content.contains("///") || content.contains("//!")
        } else {
            false
        }
    }

    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource> {
        let mut qmd = String::new();
        let mut source_map = SourceMap::new();
        let mut in_doc_comment = false;
        let mut metadata = Metadata::new();

        for (line_num, line) in input.content.lines().enumerate() {
            let start_pos = qmd.len();

            if let Some(doc_content) = line.strip_prefix("///") {
                // Outer doc comment (documents following item)
                qmd.push_str(doc_content.trim_start());
                qmd.push('\n');
                in_doc_comment = true;
            } else if let Some(doc_content) = line.strip_prefix("//!") {
                // Inner doc comment (documents enclosing item)
                qmd.push_str(doc_content.trim_start());
                qmd.push('\n');
                in_doc_comment = true;
            } else if line.trim().starts_with("//") {
                // Regular comment - skip
                continue;
            } else if !line.trim().is_empty() {
                // Code line
                if in_doc_comment {
                    // Insert separator between doc and code
                    qmd.push('\n');
                    in_doc_comment = false;
                }

                // Wrap in Rust code block
                qmd.push_str("```rust\n");
                qmd.push_str(line);
                qmd.push_str("\n```\n\n");
            }

            // Map qmd range → source line
            source_map.add_mapping(
                start_pos..qmd.len(),
                SourceRange {
                    file: input.path.clone(),
                    start: LineCol { line: line_num, col: 0 },
                    end: LineCol { line: line_num, col: line.len() },
                },
            );
        }

        // Extract YAML front matter from top-level doc comments if present
        // (e.g., //! ---\n//! title: My Module\n//! ---)
        if let Some(yaml) = extract_yaml_from_doc_comments(&input.content) {
            metadata = yaml;
        }

        Ok(ConvertedSource {
            qmd,
            source_map,
            suggested_engine: Some("markdown".to_string()), // No execution by default
            metadata,
            original_format: "rustdoc".to_string(),
        })
    }
}
```

**Example input** (`lib.rs`):

```rust
//! ---
//! title: "My Rust Library"
//! author: "Jane Developer"
//! format: revealjs
//! ---

//! # Introduction
//!
//! This is a presentation about my Rust library.

/// This function adds two numbers together.
///
/// # Examples
///
/// ```rust
/// assert_eq!(add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

//! ## Performance
//!
//! Our implementation is highly optimized.

/// Internal helper function
fn helper() -> bool {
    true
}
```

**Converted QMD**:

```markdown
---
title: "My Rust Library"
author: "Jane Developer"
format: revealjs
---

# Introduction

This is a presentation about my Rust library.

## API

This function adds two numbers together.

# Examples

```rust
assert_eq!(add(2, 3), 5);
```

```rust
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

## Performance

Our implementation is highly optimized.

Internal helper function

```rust
fn helper() -> bool {
    true
}
```
```

**Rendered outputs**:
- `quarto render lib.rs --to revealjs` → Reveal.js presentation
- `quarto render lib.rs --to pdf` → PDF documentation
- `quarto render lib.rs --to typst` → Typst document
- `quarto render lib.rs --to html` → HTML (like rustdoc, but with Quarto features)

**Benefits over standard rustdoc**:

1. **Multiple output formats**: Not limited to HTML
2. **Presentation support**: Create slides directly from code documentation
3. **Rich features**: Citations, cross-references, diagrams, custom layouts
4. **Executable examples**: Could optionally execute `rust` code blocks via an engine
5. **Customization**: Full Quarto theming and styling

**Design implications**:

This example demonstrates several key design points:

1. **Converter independence**: No changes to engines needed - works with existing markdown engine
2. **Source mapping**: Errors in rendered output map back to specific lines in `.rs` file
3. **Metadata extraction**: YAML front matter can be embedded in doc comments
4. **Format flexibility**: User controls output via `format:` field, not converter
5. **Third-party extension**: Someone outside core Quarto team could implement this

**Similar future converters**:

Following this pattern, converters could be built for:
- **JSDoc** (`.js` with `/** */` comments → qmd)
- **Python docstrings** (`.py` with `"""` strings → qmd)
- **Julia Pluto notebooks** (`.jl` Pluto format → qmd)
- **Observable notebooks** (`.ojs` format → qmd)
- **Go godoc** (`.go` with doc comments → qmd)

Each follows the same architecture:
1. Check file extension + content inspection
2. Parse format-specific syntax
3. Convert to qmd with source mapping
4. Let Quarto handle rendering to any format

**Implementation timeline**: Could be added as a third-party crate after core converters are stable (Phase 2+).

## Benefits of This Design

### 1. ✅ Clearer Separation of Concerns

**Before**:
- Engines must know: file formats + conversion + execution
- Mixing syntax knowledge with computation logic

**After**:
- Converters know: file formats + conversion (pure transformation)
- Engines know: execution only (computation)

**Impact**: Code is easier to understand, test, and modify.

### 2. ✅ Independent Extension

**Add new surface syntax**:
```rust
// No engine changes needed!
registry.register(Box::new(JuliaPlutoConverter));
```

**Add new engine**:
```rust
// No converter changes needed!
registry.register(Box::new(DenoEngine));
```

**Impact**: Third parties can contribute converters OR engines independently.

### 3. ✅ Better Testing

**Converter tests** (pure functions):
```rust
#[test]
fn test_percent_script_conversion() {
    let input = r#"
#%% [markdown]
# Hello

#%%
print("world")
"#;
    let result = PercentScriptConverter.convert(...);
    assert_eq!(result.qmd, "# Hello\n\n```{python}\nprint(\"world\")\n```\n");
}
```

**Engine tests** (mock-free):
```rust
#[test]
fn test_jupyter_execution() {
    let qmd = "```{python}\n1 + 1\n```";
    let result = JupyterEngine.execute(...);
    assert!(result.markdown.contains("2"));
}
```

**Impact**: Tests are simpler, faster, more focused.

### 4. ✅ Performance Opportunities

**Conversion caching**:
```rust
// Hash source file → cache converted qmd
if let Some(cached) = cache.get(file_hash) {
    return cached;
}
```

**Parallel conversion**:
```rust
// In project rendering, convert all files in parallel
let converted: Vec<_> = files.par_iter()
    .map(|f| converter.convert(f))
    .collect();
```

**Impact**: Faster project renders, especially with many files.

### 5. ✅ Enhanced LSP Support

**Multi-view editing**:
- LSP can show "original source" (.ipynb) OR "converted qmd" views
- Source maps enable jump-to-definition across formats
- Error reporting shows original file locations

**Example**:
```
Error in cell 3, line 5 of notebook.ipynb:
    NameError: name 'x' is not defined

    File "notebook.ipynb", cell 3, line 5
        print(x)
              ^
```

**Impact**: Better developer experience, especially for notebook workflows.

### 6. ✅ Simpler Engine Implementation

**Before**: Engines need 3 methods for file handling
```typescript
claimsFile(file, ext) { ... }      // Know all surface syntaxes
markdownForFile(file) { ... }      // Convert them
execute(options) { ... }           // Execute
```

**After**: Engines need 1 method for execution
```rust
execute(target) { ... }            // Just execute qmd!
```

**Impact**: Third-party engines are much easier to write.

### 7. ✅ Fits Extensible Pipeline Design

User-configurable pipelines (future):
```yaml
# .quarto/pipeline.yml
steps:
  - converter: ipynb
    options:
      filters: [remove-empty-cells]
  - engine: jupyter
  - handler: diagrams
  - pandoc: {}
  - postprocess: cleanup
```

**Impact**: Users can customize conversion step independently of execution.

## Challenges and Solutions

### Challenge 1: File Claiming Coordination

**Problem**: Who claims `.ipynb` files?
- Converter says "I can convert .ipynb → qmd"
- Engine says "I can execute qmd with jupyter"
- Need to coordinate selection

**Solution**: Two-phase selection (converter first, engine second)

```rust
// Phase 1: Find converter (based on file inspection)
let converter = registry.find_converter(path)?;

// Phase 2: Select engine (based on qmd content + suggestion)
let engine = select_engine(qmd, metadata, converter.suggested_engine)?;
```

**Why this works**: Converters can suggest engines, but qmd metadata always wins (user intent).

### Challenge 2: Metadata Preservation

**Problem**: `.ipynb` files have special semantics (e.g., `execute.ipynb: false` default). How do we preserve this after conversion?

**Solution**: `ConvertedSource.original_format` field + format defaults

```rust
if original_format == "ipynb" {
    format.execute.ipynb.get_or_insert(false);
}
```

**Impact**: Backward compatibility maintained.

### Challenge 3: Source Mapping Complexity

**Problem**: Errors in executed code must map back to original file (not converted qmd).

**Solution**: `SourceMap` through entire pipeline

```rust
pub struct SourceMap {
    /// Mappings from qmd positions → original file positions
    mappings: Vec<Mapping>,
}

// Usage in error reporting
let error_pos = 150;  // Position in qmd
let original_pos = source_map.map_to_original(error_pos)?;
report_error(&original_file, original_pos, message);
```

**Impact**: Errors are reported in terms users understand (original file).

### Challenge 4: R Spin Converter Performance

**Problem**: Current implementation calls R (slow). Do we block Rust port on pure Rust implementation?

**Solution**: Phased approach

**Phase 1** (MVP): Call R via subprocess
```rust
impl SourceConverter for RSpinConverter {
    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource> {
        // Call R: knitr::spin(input.path)
        let output = Command::new("Rscript")
            .arg("-e")
            .arg(format!("knitr::spin('{}')", input.path.display()))
            .output()?;

        // Parse result
        Ok(ConvertedSource { qmd: String::from_utf8(output.stdout)?, ... })
    }
}
```

**Phase 2** (optimization): Pure Rust implementation
- Parse R spin syntax (similar to percent scripts)
- ~100-200 lines of parsing code
- 10-100x faster than calling R

**Impact**: Don't block on optimization, ship with R subprocess initially.

### Challenge 5: Converter-Specific Options

**Problem**: Some converters need options (e.g., `ipynb-filters`). Where do these come from?

**Solution**: `ConvertOptions` struct + format metadata

```rust
pub struct ConvertOptions {
    pub project: Option<ProjectContext>,
    pub ipynb_filters: Vec<String>,
    // Add more as needed
}

// Populated from format metadata
let options = ConvertOptions {
    ipynb_filters: format.execute.ipynb_filters.clone(),
    ...
};
```

**Impact**: Converters remain flexible and configurable.

### Challenge 6: Migration Path

**Problem**: Large codebase to refactor. How do we migrate incrementally?

**Solution**: Adapter pattern during transition

```rust
// Temporary: Make old engines work with new system
impl SourceConverter for LegacyEngineAdapter {
    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource> {
        // Call old engine.markdownForFile() internally
        let qmd = self.old_engine.markdown_for_file(&input.path)?;
        Ok(ConvertedSource { qmd, ... })
    }
}
```

**Migration steps**:
1. Introduce `SourceConverter` trait + registry
2. Wrap existing engines as converters (adapter)
3. Gradually port converters to pure Rust
4. Remove old engine conversion methods
5. Delete adapters

**Impact**: Incremental migration, working system at each step.

## Implementation Roadmap

### Phase 1: Foundation (2-3 weeks)

**Deliverables**:
- `SourceConverter` trait + registry
- `ConvertedSource` + `SourceMap` types
- `ConverterRegistry` with registration API

**Code location**: `crates/quarto-converters/`

### Phase 2: Core Converters (2-3 weeks)

**Deliverables**:
- `IpynbConverter` (pure Rust, ~100 lines)
- `PercentScriptConverter` (pure Rust, ~150 lines)
- `RSpinConverter` (calls R subprocess, ~50 lines)

**Tests**: Each converter gets comprehensive test suite with fixtures.

### Phase 3: Engine Simplification (1-2 weeks)

**Deliverables**:
- Remove `claimsFile()` from engine trait
- Remove `markdownForFile()` from engine trait
- Update engine implementations (jupyter, knitr, markdown)

**Impact**: ~200-300 lines removed from engine code.

### Phase 4: Pipeline Integration (2-3 weeks)

**Deliverables**:
- Update `render_file()` to use converter registry
- Implement two-phase selection (converter → engine)
- Add format-specific defaults based on original format
- Source map propagation through pipeline

### Phase 5: Optimization (1-2 weeks)

**Deliverables**:
- Conversion caching
- Parallel conversion in project rendering
- Pure Rust R spin converter (optional, can defer)

**Total: 8-13 weeks** (parallelizable with other work)

## Comparison with Current System

| Aspect | Current (TypeScript) | Proposed (Rust) |
|--------|---------------------|-----------------|
| **Converter location** | Scattered (engines + core) | Centralized registry |
| **Engine responsibilities** | File claiming + conversion + execution | Execution only |
| **Extensibility** | Modify engines | Register converters |
| **Testing** | Coupled | Isolated |
| **Performance** | Sequential | Cacheable + parallel |
| **Third-party support** | Complex | Simple trait impl |
| **LSP support** | Limited | Full source mapping |
| **Code size** | ~500 lines | ~800 lines (more explicit) |

## Critical Design Questions

### Q1: Should converters be lazy or eager?

**Option A (Lazy)**: Convert on demand during render
```rust
let converter = registry.find_converter(path)?;
if let Some(conv) = converter {
    let qmd = conv.convert(...)?;  // Convert now
}
```

**Option B (Eager)**: Convert during file discovery
```rust
let project = discover_project()?;
project.convert_all()?;  // Convert all before rendering
```

**Recommendation**: Lazy (Option A)
- Simpler mental model (convert when needed)
- Enables streaming/incremental rendering
- Caching provides eager-like performance

### Q2: Should SourceMap be required or optional?

**Option A**: Required - all converters must provide source maps
```rust
pub struct ConvertedSource {
    pub source_map: SourceMap,  // Required
}
```

**Option B**: Optional - converters can skip source mapping
```rust
pub struct ConvertedSource {
    pub source_map: Option<SourceMap>,  // Optional
}
```

**Recommendation**: Optional (Option B)
- Enables simple converters (just string transformation)
- Can add source mapping incrementally
- LSP and error reporting degrade gracefully

### Q3: Should converters be async?

**Option A (Async)**: Converters return `Future`
```rust
async fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource>;
```

**Option B (Sync)**: Converters return directly
```rust
fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource>;
```

**Recommendation**: Sync (Option B) initially
- Most converters are pure computation (no I/O)
- R spin converter can use blocking subprocess
- Can add async later if needed (e.g., network-based converters)

## Recommendations

### ✅ DO: Implement this design

**Rationale**:
1. Significantly better architecture (separation of concerns)
2. Easier to extend (independent converters and engines)
3. Better testing (isolated components)
4. Fits future extensible pipeline design
5. Third-party friendly

**Risk**: Moderate (refactoring existing code, API design complexity)
**Reward**: High (cleaner codebase, extensibility, performance)

### ✅ DO: Start with core converters only

**Phase 1 converters**:
- IpynbConverter (pure Rust)
- PercentScriptConverter (pure Rust)
- RSpinConverter (calls R subprocess)

**Defer**:
- **Rustdoc** (`.rs` with doc comments → qmd for multi-format output)
- **Observable notebooks** (`.ojs` format → qmd)
- **Julia Pluto notebooks** (`.jl` Pluto format → qmd)
- **JSDoc/Python docstrings** (source code documentation → qmd)
- Other language-specific documentation formats

**Rationale**: Prove design with existing formats before extending. These future converters demonstrate the architecture's extensibility.

### ✅ DO: Use adapter pattern during migration

**Temporary bridge**:
```rust
impl SourceConverter for LegacyEngineAdapter {
    // Wraps old engine.markdownForFile()
}
```

**Rationale**: Enables incremental migration with working system at each step.

### ⚠️ DON'T: Block on pure Rust R spin converter

**Initial implementation**: Call R subprocess
**Future optimization**: Pure Rust parser

**Rationale**: Ship sooner, optimize later. R spin is relatively rare.

### ⚠️ DON'T: Over-engineer source mapping initially

**Start simple**: Line number mappings only
**Enhance later**: Column numbers, multiple files, transformations

**Rationale**: 80/20 rule - simple source maps provide most value.

## Open Questions for Discussion

1. **Converter naming**: Should converters use file extensions (`ipynb`) or descriptive names (`jupyter-notebook`)?

2. **Engine suggestion strength**: Should converter suggestions be hints or strong preferences?

3. **Metadata merging**: How should converter-extracted metadata merge with YAML front matter? (Last wins? Deep merge? Explicit priority?)

4. **Error handling**: Should converter errors be fatal or fall back to treating file as qmd?

5. **Caching strategy**: Hash-based cache in `_quarto/` directory, or in-memory only?

## Conclusion

**This design is strongly recommended.** It addresses real architectural problems in the current system and provides clear benefits:

- ✅ **Cleaner separation**: Converters handle syntax, engines handle execution
- ✅ **Better extensibility**: Add converters or engines independently
- ✅ **Improved testing**: Isolated, focused tests
- ✅ **Performance gains**: Caching and parallelization opportunities
- ✅ **Third-party friendly**: Simple trait implementation
- ✅ **Future-proof**: Fits extensible pipeline vision

The key challenges (file claiming coordination, metadata preservation, source mapping) are solvable with careful API design. The migration path is clear with incremental refactoring using adapter pattern.

**Next steps**:
1. Refine API based on discussion
2. Create `quarto-converters` crate
3. Implement core converters (ipynb, percent, r-spin)
4. Update engine trait and implementations
5. Integrate into rendering pipeline

**Estimated effort**: 8-13 weeks (parallelizable with other development)

## References

- Quarto CLI source: `external-sources/quarto-cli/src/execute/`
  - Engine trait: `types.ts:27-70`
  - Jupyter engine: `jupyter/jupyter.ts:118-634`
  - Knitr engine: `rmd.ts:53-447`
  - Conversion functions: `core/jupyter/jupyter-filters.ts`, `jupyter/percent.ts`
- Quarto documentation: https://quarto.org/docs/computations/render-scripts.html
- Related design: `claude-notes/explicit-workflow-design.md` (extensible pipelines)
