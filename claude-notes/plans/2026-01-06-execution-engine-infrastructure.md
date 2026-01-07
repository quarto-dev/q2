# Plan: ExecutionEngine Trait and Engine Detection

**Issue**: k-oomv
**Date**: 2026-01-06
**Status**: Approved

## Key Decisions

The following decisions were made during plan review:

1. **Code block language detection**: Defer to future phases (when knitr/jupyter are implemented). For MVP, require explicit `engine:` in metadata.

2. **Pipeline placement**: `EngineExecutionStage` runs **after** include resolution but **before** most AST transformations. Engines may produce output that needs transformation (callouts, cross-refs, etc.).

3. **AST serialization**: Use pampa's QMD writer (`pampa::writers::qmd::write`).

4. **WASM error handling**: Warn and fall back to markdown engine (no configurability needed).

5. **Engine configuration**: Pass through `ExecutionContext` as a cloned `ConfigValue`.

## Overview

This plan describes the implementation of execution engine support for the Rust port of Quarto. The work includes:

1. **ExecutionEngine trait** - Core abstraction for code execution engines
2. **Engine detection** - Determine which engine to use from document metadata
3. **EngineExecutionStage** - Pipeline stage that executes code and reconciles AST
4. **Concrete engines** - markdown (no-op), knitr (placeholder), jupyter (placeholder)

## Background

### TypeScript Quarto Engine Selection

From the TypeScript implementation (`src/execute/engine.ts`), engine selection follows this algorithm:

1. **Extension-based claims**: `.ipynb` → jupyter, `.Rmd` → knitr
2. **Metadata-based selection** (for `.qmd`/`.md`):
   - Check `engine:` key in YAML frontmatter
   - Check for engine-specific keys (e.g., `jupyter:` or `knitr:`)
3. **Language-based detection**: Check code block languages
   - `{r}` → knitr
   - `{python}`, `{julia}` → jupyter
4. **Default**: markdown (no computation)

### Existing Rust Infrastructure

We have:
- **PipelineStage trait** with async `run()` method (`crates/quarto-core/src/stage/`)
- **PipelineData enum** with `DocumentAst` variant containing `Pandoc` AST
- **ConfigValue** with `get()`, `as_str()`, `is_string_value()` methods for metadata access
- **Source location reconciliation design** (`claude-notes/plans/2025-12-15-engine-output-source-location-reconciliation.md`)

### Key Design Constraint

The execution engine operates on **text** (markdown in, markdown out), but we want to preserve **source locations** in the AST. Therefore, the `EngineExecutionStage` must:

1. Serialize AST to markdown
2. Execute engine on markdown text
3. Parse result to new AST
4. Reconcile new AST with original to preserve source locations where content hasn't changed

---

## Design

### 1. ExecutionEngine Trait

```rust
// crates/quarto-core/src/engine/mod.rs

/// Execution engine for code cells in Quarto documents.
///
/// Engines transform markdown with executable code cells into markdown
/// with execution outputs. The transformation is text-in/text-out.
///
/// # Thread Safety
///
/// Engines must be `Send + Sync` for use in async pipeline contexts.
pub trait ExecutionEngine: Send + Sync {
    /// Human-readable name for this engine (e.g., "markdown", "jupyter", "knitr")
    fn name(&self) -> &str;

    /// Execute code cells in the markdown content.
    ///
    /// # Arguments
    ///
    /// * `input` - The input markdown content with code cells
    /// * `context` - Execution context providing temp dirs, project info, etc.
    ///
    /// # Returns
    ///
    /// `ExecuteResult` containing the transformed markdown and any supporting files.
    fn execute(
        &self,
        input: &str,
        context: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError>;

    /// Whether this engine supports freeze/thaw caching.
    ///
    /// If true, execution results can be cached in `_freeze/` directory.
    fn can_freeze(&self) -> bool {
        false
    }

    /// Get intermediate files produced by this engine that should be cleaned up.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input document
    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// Context provided to execution engines.
pub struct ExecutionContext {
    /// Temporary directory for engine use
    pub temp_dir: PathBuf,

    /// Working directory for execution (usually document directory)
    pub cwd: PathBuf,

    /// Project directory (if in a project)
    pub project_dir: Option<PathBuf>,

    /// Path to the source document
    pub source_path: PathBuf,

    /// Target format for rendering
    pub format: String,

    /// Whether to run quietly (suppress engine output)
    pub quiet: bool,

    /// Engine-specific configuration from document metadata.
    ///
    /// This is a clone of the ConfigValue found under the engine key, e.g.,
    /// for `engine: { jupyter: { kernel: python3 } }`, this would be
    /// `{ kernel: python3 }`.
    pub engine_config: Option<ConfigValue>,
}

/// Result of engine execution.
pub struct ExecuteResult {
    /// The transformed markdown content with execution outputs
    pub markdown: String,

    /// Supporting files produced (images, data files, etc.)
    pub supporting_files: Vec<PathBuf>,

    /// Pandoc filters to apply
    pub filters: Vec<String>,

    /// Content to inject into the document
    pub includes: PandocIncludes,

    /// Whether post-processing is needed
    pub needs_postprocess: bool,
}

/// Errors that can occur during engine execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Engine not available: {0}")]
    NotAvailable(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Execution cancelled")]
    Cancelled,
}
```

### 2. Engine Detection

Engine detection examines the document metadata to determine which engine to use.

```rust
// crates/quarto-core/src/engine/detection.rs

/// Detected engine with configuration.
#[derive(Debug, Clone)]
pub struct DetectedEngine {
    /// Engine name
    pub name: String,

    /// Engine-specific configuration from YAML
    pub config: Option<ConfigValue>,
}

impl DetectedEngine {
    /// Create a new detected engine with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config: None,
        }
    }

    /// Create with configuration.
    pub fn with_config(name: impl Into<String>, config: ConfigValue) -> Self {
        Self {
            name: name.into(),
            config: Some(config),
        }
    }
}

/// Detect the execution engine from document metadata.
///
/// Checks the following cases in order:
///
/// 1. `engine: knitr` - Simple string value
/// 2. `engine: { knitr: ... }` - Map with engine name as key
/// 3. `engine: { knitr: default }` - Map with "default" value
/// 4. Default to "markdown" if no engine specified
///
/// # Arguments
///
/// * `metadata` - The document's metadata (from `Pandoc.meta`)
///
/// # Returns
///
/// The detected engine with any configuration.
pub fn detect_engine(metadata: &ConfigValue) -> DetectedEngine {
    // Case 1: Look for explicit "engine" key
    if let Some(engine_value) = metadata.get("engine") {
        // Case 1a: engine: markdown|knitr|jupyter (string value)
        if let Some(name) = engine_value.as_str() {
            return DetectedEngine::new(name);
        }

        // Case 1b: engine: { knitr: ... } or engine: { jupyter: ... }
        if let Some(entries) = engine_value.as_map_entries() {
            // The first key should be the engine name
            if let Some(first_entry) = entries.first() {
                let engine_name = &first_entry.key;

                // Check if it's a known engine
                if is_known_engine(engine_name) {
                    return DetectedEngine::with_config(
                        engine_name.clone(),
                        first_entry.value.clone(),
                    );
                }
            }
        }
    }

    // Case 2: Look for engine-specific top-level keys
    // This handles cases like:
    //   jupyter:
    //     kernel: python3
    for engine_name in KNOWN_ENGINES {
        if let Some(config) = metadata.get(engine_name) {
            return DetectedEngine::with_config(engine_name.to_string(), config.clone());
        }
    }

    // Default: markdown engine (no execution)
    DetectedEngine::new("markdown")
}

/// Known engine names.
const KNOWN_ENGINES: &[&str] = &["markdown", "knitr", "jupyter"];

/// Check if a name is a known engine.
fn is_known_engine(name: &str) -> bool {
    KNOWN_ENGINES.contains(&name)
}
```

### 3. Engine Registry

```rust
// crates/quarto-core/src/engine/registry.rs

use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available execution engines.
pub struct EngineRegistry {
    engines: HashMap<String, Arc<dyn ExecutionEngine>>,
}

impl EngineRegistry {
    /// Create a new registry with default engines.
    pub fn new() -> Self {
        let mut registry = Self {
            engines: HashMap::new(),
        };

        // Always register markdown engine
        registry.register(Arc::new(MarkdownEngine));

        // Register native-only engines
        #[cfg(not(target_arch = "wasm32"))]
        {
            registry.register(Arc::new(KnitrEngine::new()));
            registry.register(Arc::new(JupyterEngine::new()));
        }

        registry
    }

    /// Register an engine.
    pub fn register(&mut self, engine: Arc<dyn ExecutionEngine>) {
        self.engines.insert(engine.name().to_string(), engine);
    }

    /// Get an engine by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ExecutionEngine>> {
        self.engines.get(name).cloned()
    }

    /// Get the default engine (markdown).
    pub fn default_engine(&self) -> Arc<dyn ExecutionEngine> {
        self.get("markdown").expect("markdown engine always available")
    }

    /// List available engine names.
    pub fn available_engines(&self) -> Vec<&str> {
        self.engines.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

### 4. Concrete Engine Implementations

#### 4.1 Markdown Engine (No-op)

```rust
// crates/quarto-core/src/engine/markdown.rs

/// Markdown engine - no code execution.
///
/// This is the default engine used when no computation is needed.
/// It passes markdown through unchanged.
pub struct MarkdownEngine;

impl ExecutionEngine for MarkdownEngine {
    fn name(&self) -> &str {
        "markdown"
    }

    fn execute(
        &self,
        input: &str,
        _context: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        // No execution - just return input unchanged
        Ok(ExecuteResult {
            markdown: input.to_string(),
            supporting_files: Vec::new(),
            filters: Vec::new(),
            includes: PandocIncludes::default(),
            needs_postprocess: false,
        })
    }

    fn can_freeze(&self) -> bool {
        false  // Nothing to freeze
    }
}
```

#### 4.2 Knitr Engine (Placeholder)

```rust
// crates/quarto-core/src/engine/knitr.rs

#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

/// Knitr engine for R code execution.
///
/// This engine shells out to R/knitr to execute R code cells.
#[cfg(not(target_arch = "wasm32"))]
pub struct KnitrEngine {
    /// Path to R executable (discovered or configured)
    r_path: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl KnitrEngine {
    pub fn new() -> Self {
        Self {
            r_path: Self::find_r(),
        }
    }

    fn find_r() -> Option<PathBuf> {
        // Try common paths and PATH
        which::which("R").ok()
            .or_else(|| which::which("Rscript").ok())
    }

    pub fn is_available(&self) -> bool {
        self.r_path.is_some()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ExecutionEngine for KnitrEngine {
    fn name(&self) -> &str {
        "knitr"
    }

    fn execute(
        &self,
        input: &str,
        context: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let r_path = self.r_path.as_ref()
            .ok_or_else(|| ExecutionError::NotAvailable("R/knitr not found".into()))?;

        // TODO: Implement actual knitr execution
        // For now, return a placeholder error
        Err(ExecutionError::NotAvailable(
            "knitr engine not yet implemented".into()
        ))
    }

    fn can_freeze(&self) -> bool {
        true
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // knitr produces {input}_files/ directory
        let mut files_dir = input_path.to_path_buf();
        files_dir.set_extension("");
        let files_dir_name = format!(
            "{}_files",
            files_dir.file_name().unwrap_or_default().to_string_lossy()
        );
        vec![files_dir.with_file_name(files_dir_name)]
    }
}
```

#### 4.3 Jupyter Engine (Placeholder)

```rust
// crates/quarto-core/src/engine/jupyter.rs

/// Jupyter engine for Python/Julia code execution.
///
/// This engine communicates with Jupyter kernels to execute code cells.
#[cfg(not(target_arch = "wasm32"))]
pub struct JupyterEngine {
    /// Path to jupyter executable
    jupyter_path: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl JupyterEngine {
    pub fn new() -> Self {
        Self {
            jupyter_path: Self::find_jupyter(),
        }
    }

    fn find_jupyter() -> Option<PathBuf> {
        which::which("jupyter").ok()
    }

    pub fn is_available(&self) -> bool {
        self.jupyter_path.is_some()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ExecutionEngine for JupyterEngine {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn execute(
        &self,
        input: &str,
        context: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let jupyter_path = self.jupyter_path.as_ref()
            .ok_or_else(|| ExecutionError::NotAvailable("jupyter not found".into()))?;

        // TODO: Implement actual jupyter execution
        // For now, return a placeholder error
        Err(ExecutionError::NotAvailable(
            "jupyter engine not yet implemented".into()
        ))
    }

    fn can_freeze(&self) -> bool {
        true
    }
}
```

### 5. EngineExecutionStage

This is the key pipeline stage that bridges the AST world with the text-based engines.

```rust
// crates/quarto-core/src/stage/stages/engine_execution.rs

use async_trait::async_trait;

use crate::engine::{detect_engine, EngineRegistry, ExecutionContext, ExecutionError};
use crate::stage::{
    DocumentAst, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};

/// Pipeline stage that executes code cells via the appropriate engine.
///
/// This stage:
/// 1. Detects which engine to use from document metadata
/// 2. Serializes the AST to markdown
/// 3. Executes the engine on the markdown
/// 4. Parses the result back to AST
/// 5. Reconciles source locations between original and executed ASTs
///
/// For the "markdown" engine, this is a no-op that passes through unchanged.
pub struct EngineExecutionStage {
    /// Engine registry for looking up engines by name
    registry: EngineRegistry,
}

impl EngineExecutionStage {
    pub fn new() -> Self {
        Self {
            registry: EngineRegistry::new(),
        }
    }

    /// Create with a custom registry (for testing).
    pub fn with_registry(registry: EngineRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl PipelineStage for EngineExecutionStage {
    fn name(&self) -> &str {
        "engine-execution"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(doc_ast) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Step 1: Detect engine from metadata
        let detected = detect_engine(&doc_ast.ast.meta);

        ctx.observer.on_event(
            &format!("Detected engine: {}", detected.name),
            crate::stage::EventLevel::Debug,
        );

        // Step 2: Get the engine implementation
        let engine = self.registry.get(&detected.name).ok_or_else(|| {
            PipelineError::stage_error(
                self.name(),
                format!("Unknown engine: {}", detected.name),
            )
        })?;

        // Step 3: For markdown engine, skip execution (optimization)
        if engine.name() == "markdown" {
            return Ok(PipelineData::DocumentAst(doc_ast));
        }

        // Step 4: Serialize AST to QMD for engine execution
        let qmd = serialize_ast_to_qmd(&doc_ast.ast);

        // Step 5: Prepare execution context
        let exec_context = ExecutionContext {
            temp_dir: ctx.temp_dir.clone(),
            cwd: doc_ast.path.parent().unwrap_or(&ctx.temp_dir).to_path_buf(),
            project_dir: if ctx.project.is_single_file {
                None
            } else {
                Some(ctx.project.dir.clone())
            },
            source_path: doc_ast.path.clone(),
            format: ctx.format.identifier.to_string(),
            quiet: true, // TODO: Make configurable
            engine_config: detected.config.clone(),
        };

        // Step 6: Execute the engine
        let result = engine.execute(&qmd, &exec_context).map_err(|e| {
            PipelineError::stage_error(self.name(), e.to_string())
        })?;

        // Step 7: Parse the executed markdown back to AST
        let (executed_ast, new_ast_context, parse_warnings) =
            pampa::readers::qmd::read(
                result.markdown.as_bytes(),
                false,
                &doc_ast.path.display().to_string(),
                &mut std::io::sink(),
                true,
                None,
            )
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        // Step 8: Reconcile source locations
        // For content that hasn't changed, preserve original source locations.
        // For new content (execution outputs), use locations from executed AST.
        let reconciled_ast = reconcile_source_locations(
            &doc_ast.ast,
            executed_ast,
            &doc_ast.source_context,
        );

        // Step 9: Collect warnings
        let mut warnings = doc_ast.warnings;
        warnings.extend(parse_warnings);

        // Step 10: Return updated DocumentAst
        Ok(PipelineData::DocumentAst(DocumentAst {
            path: doc_ast.path,
            ast: reconciled_ast,
            ast_context: new_ast_context,
            source_context: doc_ast.source_context,
            warnings,
        }))
    }
}

/// Serialize a Pandoc AST to QMD text.
///
/// This produces QMD that can be fed to execution engines.
/// Uses pampa's QMD writer which preserves code cell attributes.
fn serialize_ast_to_qmd(ast: &Pandoc) -> String {
    pampa::writers::qmd::write(ast)
}

/// Reconcile source locations between original and executed ASTs.
///
/// See: claude-notes/plans/2025-12-15-engine-output-source-location-reconciliation.md
fn reconcile_source_locations(
    original: &Pandoc,
    executed: Pandoc,
    source_context: &SourceContext,
) -> Pandoc {
    // TODO: Implement full reconciliation algorithm
    // For MVP, just return the executed AST
    // This loses source locations for unchanged content but is functionally correct
    executed
}
```

### 6. Module Structure

```
crates/quarto-core/src/engine/
├── mod.rs              # Public exports
├── detection.rs        # Engine detection from metadata
├── registry.rs         # Engine registry
├── traits.rs           # ExecutionEngine trait
├── error.rs            # ExecutionError type
├── context.rs          # ExecutionContext and ExecuteResult
├── markdown.rs         # Markdown engine (no-op)
├── knitr.rs            # Knitr engine (native only)
└── jupyter.rs          # Jupyter engine (native only)

crates/quarto-core/src/stage/stages/
├── mod.rs              # Add engine_execution export
└── engine_execution.rs # EngineExecutionStage
```

---

## WASM Considerations

### What Works in WASM

| Feature | Native | WASM | Notes |
|---------|--------|------|-------|
| MarkdownEngine | ✅ | ✅ | No-op, always available |
| KnitrEngine | ✅ | ❌ | Requires R subprocess |
| JupyterEngine | ✅ | ❌ | Requires jupyter subprocess |
| Engine detection | ✅ | ✅ | Pure Rust, metadata inspection |
| EngineExecutionStage | ✅ | ✅ | Falls back to markdown in WASM |

### Feature Gating

```rust
// In registry.rs
impl EngineRegistry {
    pub fn new() -> Self {
        let mut registry = Self { engines: HashMap::new() };

        // Always available
        registry.register(Arc::new(MarkdownEngine));

        // Native-only engines
        #[cfg(not(target_arch = "wasm32"))]
        {
            registry.register(Arc::new(KnitrEngine::new()));
            registry.register(Arc::new(JupyterEngine::new()));
        }

        registry
    }
}
```

### WASM Fallback Behavior

When a WASM build requests an unavailable engine (e.g., `engine: jupyter`), the stage should:

1. Log a warning that the engine is not available in WASM
2. Fall back to the markdown engine (no execution)
3. Continue rendering without execution

```rust
// In EngineExecutionStage::run()
let engine = self.registry.get(&detected.name).unwrap_or_else(|| {
    ctx.warnings.push(DiagnosticMessage::warning(format!(
        "Engine '{}' not available in this build, using markdown (no execution)",
        detected.name
    )));
    self.registry.default_engine()
});
```

---

## Implementation Phases

### Phase 1: Core Infrastructure

**Goal**: Establish the engine abstraction and detection

1. Create `crates/quarto-core/src/engine/` module
2. Implement `ExecutionEngine` trait
3. Implement `detect_engine()` function
4. Implement `EngineRegistry`
5. Implement `MarkdownEngine`
6. Add unit tests for engine detection

**Deliverables**:
- Engine detection works for all YAML variants
- Markdown engine available and tested
- Registry correctly handles native/WASM builds

### Phase 2: Pipeline Integration

**Goal**: Integrate engine execution into the render pipeline

1. Implement `EngineExecutionStage`
2. Use `pampa::writers::qmd::write()` for AST-to-QMD serialization
3. Wire stage into pipeline construction (after include resolution, before transforms)
4. Add integration tests

**Deliverables**:
- Pipeline includes engine execution stage
- Markdown engine works end-to-end (no-op passthrough)
- Tests verify engine detection and execution flow
- WASM fallback works with warning

### Phase 3: Source Location Reconciliation

**Goal**: Preserve source locations through engine execution

1. Implement `reconcile_source_locations()` function
2. Add content-equality comparison for AST nodes
3. Add block/inline matching and alignment
4. Add integration tests with source location verification

**Deliverables**:
- Unchanged content preserves original source locations
- New content (execution outputs) has proper locations
- Error messages point to correct source lines

### Phase 4: Knitr Engine (Future)

**Goal**: Execute R code cells

1. Implement R detection and subprocess invocation
2. Implement knitr script generation
3. Parse knitr output
4. Handle figure/file outputs
5. Add integration tests (requires R installation)

### Phase 5: Jupyter Engine (Future)

**Goal**: Execute Python/Julia code cells

1. Implement jupyter detection
2. Implement kernel communication
3. Parse notebook output format
4. Handle rich outputs (images, HTML, etc.)
5. Add integration tests (requires jupyter installation)

---

## Test Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_engine_simple_string() {
        let meta = config_value!({
            "engine": "knitr"
        });
        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
        assert!(detected.config.is_none());
    }

    #[test]
    fn test_detect_engine_with_config() {
        let meta = config_value!({
            "engine": {
                "jupyter": {
                    "kernel": "python3"
                }
            }
        });
        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "jupyter");
        assert!(detected.config.is_some());
    }

    #[test]
    fn test_detect_engine_default_value() {
        let meta = config_value!({
            "engine": {
                "knitr": "default"
            }
        });
        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "knitr");
    }

    #[test]
    fn test_detect_engine_top_level_key() {
        let meta = config_value!({
            "jupyter": {
                "kernel": "python3"
            }
        });
        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "jupyter");
    }

    #[test]
    fn test_detect_engine_default() {
        let meta = config_value!({
            "title": "My Document"
        });
        let detected = detect_engine(&meta);
        assert_eq!(detected.name, "markdown");
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_engine_execution_stage_markdown() {
    let source = r#"---
title: Test
---

# Hello

This is a test.
"#;

    let pipeline = Pipeline::new(vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(EngineExecutionStage::new()),
    ])?;

    let input = PipelineData::DocumentSource(DocumentSource::new(
        PathBuf::from("test.qmd"),
        source.to_string(),
        ConfigValue::null(SourceInfo::default()),
    ));

    let mut ctx = make_test_context();
    let output = pipeline.run(input, &mut ctx).await?;

    let doc_ast = output.into_document_ast().unwrap();
    assert!(!doc_ast.ast.blocks.is_empty());
}
```

---

## Pipeline Placement

The `EngineExecutionStage` should run:

1. **After** include resolution (so engines see the full document)
2. **Before** most AST transformations (engines may produce content needing transformation)

Example pipeline order:
```
ParseDocumentStage
    ↓
IncludeResolutionStage (future)
    ↓
EngineExecutionStage ← HERE
    ↓
CalloutTransform
    ↓
CrossRefTransform
    ↓
... other transforms ...
    ↓
RenderHtmlBodyStage
```

This differs from TypeScript Quarto's exact order, but the principle is the same: engine output may contain callouts, cross-refs, etc. that need processing.

---

## Resolved Questions

These questions were resolved during plan review:

### Q1: Should engine detection happen in a separate stage?

**Decision**: Engine detection in `EngineExecutionStage` (Option A).
- Single stage handles all engine logic
- Simpler to implement and test
- Can split later if needed

### Q2: How should engine configuration be passed?

**Decision**: Pass through `ExecutionContext` as a cloned `ConfigValue`.
- Simple and direct
- Engine-specific structs can be added later as needed

### Q3: How do we handle engine errors in WASM?

**Decision**: Warn and fall back to markdown engine.
- No configurability needed
- Consistent behavior across platforms
- User sees warning but rendering continues

### Q4: Should we support code block language detection?

**Decision**: Defer to Phase 4/5 (when knitr/jupyter are implemented).
- For MVP, require explicit `engine:` in metadata
- Language detection adds complexity
- Can be added alongside actual engine implementations

---

## Related Documents

- [Quarto Render Prototype](2025-12-20-quarto-render-prototype.md) - Overall rendering plan (Phase 5)
- [Source Location Reconciliation](2025-12-15-engine-output-source-location-reconciliation.md) - AST reconciliation design
- [Pipeline Stage Design](2026-01-06-pipeline-stage-design.md) - PipelineStage abstraction
- [TypeScript Engine Selection](../render-pipeline/single-document/stages/05-engine-selection.md) - Analysis of TS implementation
- [TypeScript Engine Execution](../render-pipeline/single-document/stages/07-engine-execution.md) - Analysis of TS implementation

---

## Success Criteria

### Phase 1 (Core Infrastructure) ✅
- [x] Engine detection works for all YAML variants
- [x] MarkdownEngine available and passes through unchanged
- [x] EngineRegistry correctly gates native-only engines
- [x] Unit tests pass

### Phase 2 (Pipeline Integration) ✅
- [x] EngineExecutionStage integrates into pipeline
- [x] End-to-end test with markdown engine works
- [x] WASM build works with fallback behavior

### Phase 3 (Reconciliation) ✅
- [x] Source locations preserved for unchanged content
- [x] Error messages point to correct source locations

### Future Phases
- [ ] Knitr engine executes R code
- [ ] Jupyter engine executes Python code
- [ ] Freeze/thaw caching works
