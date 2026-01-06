# Plan: PipelineStage Abstraction for Full Render Pipeline

**Issue**: k-m46n
**Date**: 2026-01-06
**Status**: Implementation Complete (Phase 3)

## Implementation Progress

### Phase 1: Core Infrastructure ✅ Complete
- [x] Core abstractions implemented in `crates/quarto-core/src/stage/`
- [x] Dependencies added: `async-trait`, `tokio-util`, `pollster`
- [x] All 259 quarto-core tests pass
- [x] WASM compatibility verified for both `wasm-qmd-parser` and `wasm-quarto-hub-client`

### Phase 2: Concrete Stages ✅ Complete
- [x] `ParseDocumentStage`: LoadedSource → DocumentAst (parses QMD using pampa)
- [x] `AstTransformsStage`: DocumentAst → DocumentAst (runs transform pipeline)
- [x] `RenderHtmlBodyStage`: DocumentAst → RenderedOutput (renders HTML body)
- [x] `ApplyTemplateStage`: RenderedOutput → RenderedOutput (applies HTML template)

### Phase 3: Unified Async Pipeline ✅ Complete
- [x] `render_qmd_to_html` refactored to be async using Pipeline
- [x] Single unified implementation for both CLI and WASM (no conditional compilation)
- [x] CLI uses `pollster::block_on()` for sync wrapper
- [x] WASM render functions are now async (`pub async fn render_qmd(...)`)
- [x] All tests pass, both native and WASM builds verified

### Phase 4: Future Work
- [ ] PipelinePlanner - construct pipelines based on document analysis
- [ ] Engine execution stages (Jupyter, Knitr)

## Problem Statement

The current Rust implementation has `TransformPipeline` which represents a sequence of AST transformations. However, the full Quarto rendering pipeline involves many stages that happen *before* and *after* AST transformation:

- **Before AST**: File type detection, `.ipynb` → `.qmd` conversion, parsing, engine selection, metadata merging
- **AST transforms**: Callouts, cross-references, layout, etc. (current `TransformPipeline`)
- **After AST**: Output rendering (HTML/PDF/etc.), template application, postprocessing, DOM manipulation

The TypeScript Quarto has 10 major stages with ~78 internal filter steps. We need an explicit abstraction that can represent all of these stages, validate stage sequences, and enable orchestration of different pipelines for different file types.

## Goals

1. **Design the PipelineStage data structure** - the building block for pipeline construction
2. **Design pipeline data types** - what flows between stages
3. **Design execution infrastructure** - Pipeline, StageContext
4. **Enable future PipelinePlanner** - the structure must support arbitrary pipeline construction
5. **Maintain CLI/WASM parity** - the design must work in both native and WASM environments

## Non-Goals (This Session)

- Full decomposition of Quarto's rendering pipeline into stages
- Implementing the `PipelinePlanner` that creates pipelines
- Engine selection logic (that's PipelinePlanner's responsibility)
- Conditional stage logic (that's PipelinePlanner's responsibility)
- Project vs single-document distinction (that's PipelinePlanner's responsibility)
- Full implementation of all stages
- Engine execution (Jupyter, Knitr)
- PDF-specific compilation (latexmk)

## Design Philosophy

The `PipelineStage` abstraction is a **building block**, not a builder. A future `PipelinePlanner` object will:
- Analyze the document/project
- Select appropriate engine
- Determine format-specific stages
- Construct a `Vec<Box<dyn PipelineStage>>` accordingly

Our job is to design the stage abstraction flexible enough that PipelinePlanner can construct any pipeline it needs.

---

## Analysis: TypeScript Pipeline Stages

From our [render pipeline analysis](../render-pipeline/single-document/README.md), the TypeScript Quarto has 10 stages:

| # | Stage | Input | Output |
|---|-------|-------|--------|
| 1 | CLI Entry | Raw args | RenderFlags, Services |
| 2 | Main Coordinator | Flags | ProjectContext |
| 3 | File Rendering Setup | ProjectContext | TempContext, Lifetime |
| 4 | Render Context Creation | File path | ExecutionTarget, Format[], RenderContext[] |
| 5 | Engine Selection | File + metadata | Engine, Target |
| 6 | YAML Validation | Target | Validated metadata |
| 7 | Engine Execution | Target | ExecuteResult (markdown + supporting files) |
| 8 | Language Cell Handlers | Markdown | Modified markdown + includes |
| 9 | Pandoc Conversion | Markdown | Output file (HTML/PDF/etc.) |
| 10 | Postprocessing | Output file | Final file + supporting files |

### Key Data Structures

```
ExecutionTarget {
  source: Path,           // Original input file
  input: Path,            // Actual input (may differ for .ipynb)
  markdown: MappedString, // Content with source tracking
  metadata: Metadata,     // Parsed YAML front matter
}

ExecuteResult {
  engine: String,
  markdown: String,
  supporting: Vec<Path>,
  filters: Vec<String>,
  includes: PandocIncludes,
  ...
}

RenderedFile {
  input: Path,
  output: Path,
  format: Format,
  supporting: Vec<Path>,
  ...
}
```

---

## Proposed Design

### Core Insight: Runtime-Flexible Pipeline with Validation

Since `PipelinePlanner` will construct pipelines at runtime based on document analysis, we need:
1. **Runtime flexibility** - stages stored in `Vec<Box<dyn PipelineStage>>`
2. **Validation** - check that stages compose correctly (output type matches next input type)
3. **Simple trait** - easy to implement new stages

Rather than compile-time type safety (which would require complex GATs), we use an **enum-based data model** with **runtime type checking**.

### Pipeline Data Enum

All data types that flow through the pipeline are variants of a single enum. We use **6 variants** (merging the original SourceFile and LoadedSource for simplicity):

```rust
/// All possible data types flowing through the pipeline
#[derive(Debug)]
pub enum PipelineData {
    /// Loaded source with detected type (entry point after loading)
    LoadedSource(LoadedSource),

    /// Markdown content with metadata (after notebook conversion, etc.)
    DocumentSource(DocumentSource),

    /// Parsed AST ready for transformation
    DocumentAst(DocumentAst),

    /// Result after engine execution (future: Jupyter, Knitr)
    ExecutedDocument(ExecutedDocument),

    /// Rendered output (HTML, LaTeX, etc.) before relocation
    RenderedOutput(RenderedOutput),

    /// Final output after relocation (for SSG slug support)
    FinalOutput(FinalOutput),
}

/// Type tag for validation (avoids matching on data)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineDataKind {
    LoadedSource,
    DocumentSource,
    DocumentAst,
    ExecutedDocument,
    RenderedOutput,
    FinalOutput,
}

impl PipelineData {
    pub fn kind(&self) -> PipelineDataKind {
        match self {
            Self::LoadedSource(_) => PipelineDataKind::LoadedSource,
            Self::DocumentSource(_) => PipelineDataKind::DocumentSource,
            Self::DocumentAst(_) => PipelineDataKind::DocumentAst,
            Self::ExecutedDocument(_) => PipelineDataKind::ExecutedDocument,
            Self::RenderedOutput(_) => PipelineDataKind::RenderedOutput,
            Self::FinalOutput(_) => PipelineDataKind::FinalOutput,
        }
    }
}
```

**Design Note**: We keep `RenderedOutput` and `FinalOutput` separate to support future "output relocation" functionality. TypeScript Quarto has limitations about where to place output files in a project (the "slug" functionality of many SSGs). The separation allows `PipelinePlanner` to explicitly reason about file relocation between these stages.

### Individual Data Types

```rust
/// Loaded file with detected source type
#[derive(Debug)]
pub struct LoadedSource {
    pub path: PathBuf,
    pub content: Vec<u8>,
    pub source_type: SourceType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Qmd,
    Markdown,
    Ipynb,
    Rmd,
}

/// Markdown content with metadata (after any format conversion)
#[derive(Debug)]
pub struct DocumentSource {
    pub path: PathBuf,
    pub markdown: MappedString,
    pub metadata: ConfigValue,
    pub original_source: PathBuf,  // Original path before conversion
    pub source_context: SourceContext,
}

/// Parsed Pandoc AST
///
/// Note: `ast_context` is mutable throughout the pipeline because
/// AST transforms may create new objects that need source info tracking.
#[derive(Debug)]
pub struct DocumentAst {
    pub path: PathBuf,
    pub ast: Pandoc,
    pub ast_context: pampa::pandoc::ASTContext,
    pub source_context: SourceContext,
    pub warnings: Vec<DiagnosticMessage>,
}

/// Result of engine execution (future: Jupyter, Knitr)
#[derive(Debug)]
pub struct ExecutedDocument {
    pub path: PathBuf,
    pub markdown: MappedString,
    pub supporting_files: Vec<PathBuf>,
    pub filters: Vec<String>,
    pub includes: PandocIncludes,
    pub source_context: SourceContext,
}

/// Rendered output before relocation/postprocessing
#[derive(Debug)]
pub struct RenderedOutput {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub format: Format,
    pub content: String,
    pub is_intermediate: bool,  // True for LaTeX before PDF compilation
    pub supporting_files: Vec<PathBuf>,
}

/// Final output after relocation
#[derive(Debug)]
pub struct FinalOutput {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub format: Format,
    pub supporting_files: Vec<PathBuf>,
    pub warnings: Vec<DiagnosticMessage>,
}
```

### PipelineStage Trait

```rust
use async_trait::async_trait;

/// A single stage in the render pipeline
#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Human-readable name for logging/debugging
    fn name(&self) -> &str;

    /// What input type this stage expects
    fn input_kind(&self) -> PipelineDataKind;

    /// What output type this stage produces
    fn output_kind(&self) -> PipelineDataKind;

    /// Run the stage
    ///
    /// Note: We use "run" instead of "execute" to avoid confusion with
    /// Quarto's engine execution (Jupyter, Knitr).
    ///
    /// Stages receive `&mut StageContext` which provides:
    /// - Owned data (format, project, document info)
    /// - Mutable artifact storage
    /// - Observer for tracing/progress
    /// - Cancellation token
    ///
    /// Stages can produce both:
    /// - Fatal errors (returned as `Err`)
    /// - Non-fatal warnings (stored in `ctx.warnings`)
    ///
    /// # Errors
    /// Returns error if input is wrong type or stage fails
    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError>;
}
```

---

## Stage Context (The "Activation Frame" Pattern)

A key design challenge is managing lifetimes with async stages. The original `RenderContext<'a>` has complex lifetime parameters because it borrows immutable config while owning mutable artifacts.

We solve this with an **owned `StageContext`** that has no lifetime parameters. This "activation frame" pattern:
- Works cleanly with async (no lifetime complexity in futures)
- Supports potential task spawning for parallelization
- Allows stages to clone what they need without lifetime constraints

```rust
use tokio_util::sync::CancellationToken;

/// Owned context passed to all pipeline stages.
///
/// This is the "activation frame" for stage execution - it contains
/// all the data a stage needs without lifetime parameters, making it
/// work cleanly with async and potential parallelization.
pub struct StageContext {
    // === Immutable shared data ===

    /// System runtime (filesystem, env, subprocesses)
    /// Arc for potential task spawning
    pub runtime: Arc<dyn SystemRuntime>,

    // === Owned data (no lifetime complexity) ===

    /// Target format for this render
    pub format: Format,

    /// Project context (configuration, paths)
    pub project: ProjectContext,

    /// Information about the document being rendered
    pub document: DocumentInfo,

    /// Temporary directory for this pipeline run
    pub temp_dir: PathBuf,

    // === Mutable state ===

    /// Artifact store for dependencies and intermediates
    pub artifacts: ArtifactStore,

    /// Non-fatal warnings collected during execution
    pub warnings: Vec<DiagnosticMessage>,

    // === Observation & Control ===

    /// Observer for tracing, progress reporting, and WASM callbacks
    pub observer: Arc<dyn PipelineObserver>,

    /// Cancellation token for graceful shutdown (Ctrl+C)
    pub cancellation: CancellationToken,
}

impl StageContext {
    /// Create a new stage context
    pub fn new(
        runtime: Arc<dyn SystemRuntime>,
        format: Format,
        project: ProjectContext,
        document: DocumentInfo,
        observer: Arc<dyn PipelineObserver>,
    ) -> Result<Self, PipelineError> {
        let temp_dir = runtime
            .temp_dir("quarto-pipeline")?
            .into_path();

        Ok(Self {
            runtime,
            format,
            project,
            document,
            temp_dir,
            artifacts: ArtifactStore::new(),
            warnings: Vec::new(),
            observer,
            cancellation: CancellationToken::new(),
        })
    }

    /// Create with a specific cancellation token (for CLI integration)
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation = token;
        self
    }
}
```

### Relationship to RenderContext

The existing `RenderContext<'a>` (in `crates/quarto-core/src/render.rs`) is still needed for `AstTransform` implementations, which expect it. Stages that run AST transforms temporarily construct a `RenderContext` from `StageContext` data using `std::mem::take`:

```rust
/// Wrapper for existing TransformPipeline
pub struct AstTransforms {
    pipeline: TransformPipeline,
}

#[async_trait]
impl PipelineStage for AstTransforms {
    fn name(&self) -> &str { "ast-transforms" }
    fn input_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }
    fn output_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(self.name(), input.kind()));
        };

        // Temporarily take ownership of artifacts for RenderContext
        let binaries = BinaryDependencies::discover(ctx.runtime.as_ref());
        let mut render_ctx = RenderContext {
            artifacts: std::mem::take(&mut ctx.artifacts),
            project: &ctx.project,
            document: &ctx.document,
            format: &ctx.format,
            binaries: &binaries,
            options: RenderOptions::default(),
        };

        // Run the transform pipeline (existing API)
        self.pipeline.execute(&mut doc.ast, &mut render_ctx)
            .map_err(|e| PipelineError::StageError {
                stage: self.name().to_string(),
                diagnostics: vec![DiagnosticMessage::error(e.to_string())],
            })?;

        // Return artifacts to context
        ctx.artifacts = render_ctx.artifacts;

        Ok(PipelineData::DocumentAst(doc))
    }
}
```

This pattern works because:
- `&mut StageContext` is borrowed for the duration of `run()`
- When `run()` completes, the borrow ends
- `async_trait` desugars this to a pinned future with the right lifetime bounds
- Stages that spawn external processes clone what they need (`PathBuf`, `Arc`, `CancellationToken`) rather than capturing `&mut ctx`

---

## PipelineObserver: Unified Abstraction for Tracing & Progress

Rather than tightly coupling to OpenTelemetry (which has WASM compatibility issues), we use a trait abstraction that supports multiple backends:

```rust
/// Observer for pipeline execution events.
///
/// This abstraction unifies:
/// - OpenTelemetry tracing (native builds)
/// - Progress bar updates (CLI)
/// - JavaScript callbacks (WASM)
///
/// Implementations should be lightweight - the pipeline calls
/// these methods frequently.
pub trait PipelineObserver: Send + Sync {
    /// Called when a stage begins execution
    fn on_stage_start(&self, name: &str, index: usize, total: usize);

    /// Called when a stage completes successfully
    fn on_stage_complete(&self, name: &str, index: usize, total: usize);

    /// Called when a stage fails
    fn on_stage_error(&self, name: &str, index: usize, error: &PipelineError);

    /// Called for arbitrary events during execution
    fn on_event(&self, message: &str, level: EventLevel);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Trace,
    Debug,
    Info,
    Warn,
}
```

### Observer Implementations

```rust
/// No-op observer (default, minimal overhead)
pub struct NoopObserver;

impl PipelineObserver for NoopObserver {
    fn on_stage_start(&self, _: &str, _: usize, _: usize) {}
    fn on_stage_complete(&self, _: &str, _: usize, _: usize) {}
    fn on_stage_error(&self, _: &str, _: usize, _: &PipelineError) {}
    fn on_event(&self, _: &str, _: EventLevel) {}
}

/// OpenTelemetry tracing observer (native builds only)
#[cfg(feature = "otel")]
pub struct OtelObserver {
    tracer: opentelemetry::global::BoxedTracer,
}

#[cfg(feature = "otel")]
impl PipelineObserver for OtelObserver {
    fn on_stage_start(&self, name: &str, index: usize, total: usize) {
        tracing::info_span!("pipeline.stage",
            stage.name = name,
            stage.index = index,
            stage.total = total
        ).entered();
    }
    // ... other methods emit tracing events
}

/// Progress bar observer (CLI)
pub struct ProgressBarObserver {
    bar: indicatif::ProgressBar,
}

impl PipelineObserver for ProgressBarObserver {
    fn on_stage_start(&self, name: &str, index: usize, total: usize) {
        self.bar.set_length(total as u64);
        self.bar.set_position(index as u64);
        self.bar.set_message(name.to_string());
    }

    fn on_stage_complete(&self, _: &str, index: usize, _: usize) {
        self.bar.set_position((index + 1) as u64);
    }
    // ...
}

/// JavaScript callback observer (WASM builds)
#[cfg(target_arch = "wasm32")]
pub struct WasmObserver {
    callback: js_sys::Function,
}

#[cfg(target_arch = "wasm32")]
impl PipelineObserver for WasmObserver {
    fn on_stage_start(&self, name: &str, index: usize, total: usize) {
        let event = js_sys::Object::new();
        js_sys::Reflect::set(&event, &"type".into(), &"stage_start".into()).ok();
        js_sys::Reflect::set(&event, &"name".into(), &name.into()).ok();
        js_sys::Reflect::set(&event, &"index".into(), &(index as u32).into()).ok();
        js_sys::Reflect::set(&event, &"total".into(), &(total as u32).into()).ok();
        self.callback.call1(&JsValue::NULL, &event).ok();
    }
    // ...
}
```

### Feature-Gated Tracing Macro

For stages that want detailed tracing in native builds without WASM overhead:

```rust
/// Emit a trace event through the observer.
///
/// In builds with the `otel` feature, this also emits a tracing event.
/// This macro provides a unified way to instrument stages that works
/// across all environments.
#[cfg(feature = "otel")]
macro_rules! trace_event {
    ($ctx:expr, $level:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        match $level {
            EventLevel::Trace => tracing::trace!("{}", msg),
            EventLevel::Debug => tracing::debug!("{}", msg),
            EventLevel::Info => tracing::info!("{}", msg),
            EventLevel::Warn => tracing::warn!("{}", msg),
        }
        $ctx.observer.on_event(&msg, $level);
    }};
}

#[cfg(not(feature = "otel"))]
macro_rules! trace_event {
    ($ctx:expr, $level:expr, $($arg:tt)*) => {{
        $ctx.observer.on_event(&format!($($arg)*), $level);
    }};
}

pub(crate) use trace_event;
```

---

## Cancellation Support

Long-running stages (especially engine execution) should support graceful cancellation via Ctrl+C:

```rust
use tokio_util::sync::CancellationToken;

impl Pipeline {
    pub async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let mut data = input;
        let total = self.stages.len();

        for (idx, stage) in self.stages.iter().enumerate() {
            // Check cancellation before each stage
            if ctx.cancellation.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }

            ctx.observer.on_stage_start(stage.name(), idx, total);

            match stage.run(data, ctx).await {
                Ok(output) => {
                    ctx.observer.on_stage_complete(stage.name(), idx, total);
                    data = output;
                }
                Err(e) => {
                    ctx.observer.on_stage_error(stage.name(), idx, &e);
                    return Err(e);
                }
            }
        }

        Ok(data)
    }
}
```

### Cancellation in Engine Execution Stages

Stages that spawn external processes use `tokio::select!` for cancellation:

```rust
impl ExecuteJupyter {
    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let child = tokio::process::Command::new("jupyter")
            .args(&["nbconvert", "--execute", ...])
            .spawn()?;

        tokio::select! {
            result = child.wait_with_output() => {
                // Process completed normally
                self.process_output(result?, ctx)
            }
            _ = ctx.cancellation.cancelled() => {
                // User pressed Ctrl+C
                child.kill().await?;
                Err(PipelineError::Cancelled)
            }
        }
    }
}
```

### CLI Integration

```rust
#[tokio::main]
async fn main() {
    let cancellation = CancellationToken::new();

    // Ctrl+C handler
    let cancel = cancellation.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        eprintln!("\nCancelling...");
        cancel.cancel();
    });

    // Create context with cancellation token
    let ctx = StageContext::new(...)
        .with_cancellation(cancellation);

    match pipeline.run(input, &mut ctx).await {
        Ok(output) => { /* success */ }
        Err(PipelineError::Cancelled) => {
            eprintln!("Render cancelled by user");
            std::process::exit(130);  // Standard exit code for Ctrl+C
        }
        Err(e) => { /* other error */ }
    }
}
```

---

## WASM Compatibility

Maintaining CLI/WASM parity is critical for this project. The pipeline design makes several choices to ensure WASM compatibility:

### Feature Flags

```toml
[features]
default = []
otel = [
    "opentelemetry",
    "opentelemetry-sdk",
    "tracing-opentelemetry",
]
```

### What Works in WASM

| Feature | CLI | WASM | Notes |
|---------|-----|------|-------|
| Pipeline execution | ✅ | ✅ | Core abstractions are platform-agnostic |
| Stage validation | ✅ | ✅ | Pure Rust, no dependencies |
| PipelineObserver | ✅ | ✅ | Trait abstraction with platform-specific impls |
| Progress reporting | ✅ | ✅ | CLI uses indicatif, WASM uses JS callbacks |
| Cancellation | ✅ | ⚠️ | WASM uses different signaling mechanism |
| OpenTelemetry | ✅ | ❌ | Feature-gated behind `otel` |
| `#[instrument]` | ✅ | ❌ | Only in `otel` builds |
| async-trait | ✅ | ✅ | Works with wasm-bindgen-futures |
| tokio-util CancellationToken | ✅ | ✅ | Atomics-based, no tokio runtime required |

**WASM Crate Dependencies:**
- `wasm-qmd-parser` → pampa (no quarto-core, no stage module)
- `wasm-quarto-hub-client` → pampa + **quarto-core** (includes stage module with async-trait, tokio-util)

### WASM-Specific Considerations

1. **SystemRuntime**: The `WasmRuntime` implementation (in `quarto-system-runtime`) provides browser-compatible alternatives for file operations.

2. **Async Runtime**: WASM uses `wasm-bindgen-futures` instead of tokio. The `async fn run()` signature works with both.

3. **Cancellation**: In WASM, cancellation is signaled through the `WasmObserver` callback rather than tokio signals:

```rust
#[cfg(target_arch = "wasm32")]
impl StageContext {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}
```

4. **Progress Reporting**: The `WasmObserver` converts events to JavaScript objects that can update a UI.

### Validating WASM Compatibility

**IMPORTANT: WASM builds require special setup.** The project has two WASM crates:

1. **`wasm-qmd-parser`** - uses pampa only (no quarto-core)
2. **`wasm-quarto-hub-client`** - uses both pampa AND quarto-core

Both crates depend on `tree-sitter`, which is a C library. Building for WASM requires a custom `wasm-sysroot` directory containing stub C headers. Each WASM crate has its own `wasm-sysroot/` directory.

**Why the sysroot is needed:** The `tree-sitter` crate compiles C code, which requires C standard library headers (`stdio.h`, `stdlib.h`, etc.). These headers don't exist for the `wasm32-unknown-unknown` target by default. The project provides minimal stub headers that satisfy the compiler.

#### Correct WASM Build Process

```bash
# Build wasm-qmd-parser (does NOT include quarto-core)
cd crates/wasm-qmd-parser
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
cargo build --target wasm32-unknown-unknown

# Build wasm-quarto-hub-client (INCLUDES quarto-core with async-trait and tokio-util)
cd crates/wasm-quarto-hub-client
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
cargo build --target wasm32-unknown-unknown

# Build for native (no special setup required)
cargo build -p quarto-core
```

#### Common Mistake

Running `cargo build --target wasm32-unknown-unknown` without the `CFLAGS_wasm32_unknown_unknown` environment variable will fail with:

```
error: fatal error: 'stdio.h' file not found
```

This is NOT a problem with the code - it's a missing environment setup.

#### CI Workflow

The `.github/workflows/build-wasm.yml` workflow shows the complete setup including clang installation and wasm-pack usage. For production builds, use wasm-pack:

```bash
cd crates/wasm-qmd-parser
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
wasm-pack build --target web --dev
```

---

## Pipeline Struct

```rust
/// A validated sequence of pipeline stages
pub struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
    expected_input: PipelineDataKind,
    expected_output: PipelineDataKind,
}

impl Pipeline {
    /// Create a new pipeline from stages
    ///
    /// Validates that stages compose correctly (output type matches next input type)
    pub fn new(stages: Vec<Box<dyn PipelineStage>>) -> Result<Self, PipelineValidationError> {
        if stages.is_empty() {
            return Err(PipelineValidationError::Empty);
        }

        // Validate composition
        for window in stages.windows(2) {
            let output = window[0].output_kind();
            let input = window[1].input_kind();
            if output != input {
                return Err(PipelineValidationError::TypeMismatch {
                    stage_a: window[0].name().to_string(),
                    stage_b: window[1].name().to_string(),
                    output,
                    input,
                });
            }
        }

        let expected_input = stages.first().unwrap().input_kind();
        let expected_output = stages.last().unwrap().output_kind();

        Ok(Self { stages, expected_input, expected_output })
    }

    /// What input type this pipeline expects
    pub fn expected_input(&self) -> PipelineDataKind {
        self.expected_input
    }

    /// What output type this pipeline produces
    pub fn expected_output(&self) -> PipelineDataKind {
        self.expected_output
    }

    /// Get stage names for debugging
    pub fn stage_names(&self) -> Vec<&str> {
        self.stages.iter().map(|s| s.name()).collect()
    }

    /// Run the pipeline (see Cancellation Support section for full implementation)
    pub async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        // Validate input type
        if input.kind() != self.expected_input {
            return Err(PipelineError::UnexpectedInput {
                stage: "pipeline".to_string(),
                expected: self.expected_input,
                got: input.kind(),
            });
        }

        let mut data = input;
        let total = self.stages.len();

        for (idx, stage) in self.stages.iter().enumerate() {
            if ctx.cancellation.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }

            ctx.observer.on_stage_start(stage.name(), idx, total);

            match stage.run(data, ctx).await {
                Ok(output) => {
                    ctx.observer.on_stage_complete(stage.name(), idx, total);
                    data = output;
                }
                Err(e) => {
                    ctx.observer.on_stage_error(stage.name(), idx, &e);
                    return Err(e);
                }
            }
        }

        Ok(data)
    }
}

#[derive(Debug)]
pub enum PipelineValidationError {
    Empty,
    TypeMismatch {
        stage_a: String,
        stage_b: String,
        output: PipelineDataKind,
        input: PipelineDataKind,
    },
}
```

---

## Error Handling

```rust
/// Pipeline error with stage context
#[derive(Debug)]
pub enum PipelineError {
    /// Wrong input type for stage
    UnexpectedInput {
        stage: String,
        expected: PipelineDataKind,
        got: PipelineDataKind,
    },

    /// Stage execution failed with diagnostics
    StageError {
        stage: String,
        diagnostics: Vec<DiagnosticMessage>,
    },

    /// Pipeline was cancelled (Ctrl+C)
    Cancelled,

    /// Pipeline validation failed
    ValidationError(PipelineValidationError),

    /// IO error
    Io(std::io::Error),

    /// Parse error
    Parse(ParseError),
}

impl PipelineError {
    pub fn unexpected_input(stage: &str, got: PipelineDataKind) -> Self {
        Self::UnexpectedInput {
            stage: stage.to_string(),
            expected: PipelineDataKind::LoadedSource, // Would be looked up from stage
            got,
        }
    }

    pub fn stage_error(stage: &str, message: impl Into<String>) -> Self {
        Self::StageError {
            stage: stage.to_string(),
            diagnostics: vec![DiagnosticMessage::error(message.into())],
        }
    }
}
```

---

## Example Stage Implementations

### LoadSource Stage

```rust
/// Load source file from filesystem
pub struct LoadSource;

#[async_trait]
impl PipelineStage for LoadSource {
    fn name(&self) -> &str { "load-source" }
    fn input_kind(&self) -> PipelineDataKind { PipelineDataKind::LoadedSource }
    fn output_kind(&self) -> PipelineDataKind { PipelineDataKind::LoadedSource }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::LoadedSource(source) = input else {
            return Err(PipelineError::unexpected_input(self.name(), input.kind()));
        };

        trace_event!(ctx, EventLevel::Debug, "loaded {} bytes from {:?}",
            source.content.len(), source.path);

        Ok(PipelineData::LoadedSource(source))
    }
}
```

### ParseDocument Stage

```rust
/// Parse document to Pandoc AST
pub struct ParseDocument;

#[async_trait]
impl PipelineStage for ParseDocument {
    fn name(&self) -> &str { "parse-document" }
    fn input_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentSource }
    fn output_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentSource(source) = input else {
            return Err(PipelineError::unexpected_input(self.name(), input.kind()));
        };

        trace_event!(ctx, EventLevel::Debug, "parsing {} bytes of markdown",
            source.markdown.len());

        let (pandoc, ast_context, warnings) = pampa::readers::qmd::read(
            source.markdown.value().as_bytes(),
            false,
            &source.path.display().to_string(),
            &mut std::io::sink(),
            true,
            None,
        ).map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        // Collect non-fatal warnings
        ctx.warnings.extend(warnings.clone());

        Ok(PipelineData::DocumentAst(DocumentAst {
            path: source.path,
            ast: pandoc,
            ast_context,
            source_context: source.source_context,
            warnings,
        }))
    }
}
```

---

## Example: PipelinePlanner Usage (Future)

```rust
// This is how PipelinePlanner will use these abstractions (future work)
impl PipelinePlanner {
    pub fn plan_html_render(&self, doc: &DocumentInfo) -> Result<Pipeline> {
        let mut stages: Vec<Box<dyn PipelineStage>> = vec![];

        // Conditional: add notebook conversion for .ipynb
        if doc.source_type == SourceType::Ipynb {
            stages.push(Box::new(ConvertNotebook));
        } else {
            stages.push(Box::new(ExtractMarkdown));  // Direct markdown extraction
        }

        stages.push(Box::new(ParseDocument));

        // Conditional: add engine execution if needed
        if self.needs_execution(doc) {
            match self.select_engine(doc) {
                Engine::Jupyter => stages.push(Box::new(ExecuteJupyter::new(...))),
                Engine::Knitr => stages.push(Box::new(ExecuteKnitr::new(...))),
                Engine::Markdown => {} // No execution needed
            }
            stages.push(Box::new(ParseExecutedMarkdown));
        }

        stages.push(Box::new(AstTransforms::standard()));
        stages.push(Box::new(RenderHtml));
        stages.push(Box::new(HtmlPostprocessor::standard()));
        stages.push(Box::new(Finalize));

        Pipeline::new(stages)
    }
}
```

---

## Design Decisions Summary

| Decision | Resolution | Rationale |
|----------|------------|-----------|
| Data ownership | Duplication acceptable | Enables parallelization without shared reference complexity |
| Context lifetimes | Owned `StageContext`, no lifetime params | Works cleanly with async; "activation frame" pattern |
| ASTContext | Mutable throughout pipeline | Transforms may create new objects needing source tracking |
| Async | Everywhere | Engine execution is inherently async; overhead acceptable given Rust performance gains |
| Tracing/Progress | `PipelineObserver` trait | Unified abstraction for CLI, WASM, and OpenTelemetry |
| OpenTelemetry | Feature-gated (`otel`) | WASM compatibility is higher priority |
| Cancellation | `CancellationToken` + `tokio::select!` | Graceful Ctrl+C handling |
| PipelineData variants | 6 (merged SourceFile+LoadedSource) | Simplified entry point; kept RenderedOutput vs FinalOutput for relocation support |
| RenderContext integration | `std::mem::take` pattern in AstTransforms | Bridges new StageContext to existing AstTransform API |

### Additional Design Rationale

**Q: Why async `run()` everywhere?**
**A**: Engine execution (Jupyter, Knitr) and external tool invocation (Pandoc, latexmk) are inherently async. The async overhead is acceptable: even with 50k documents × 20 stages = 1M async calls, Rust performance gains (10-30x over TypeScript) far outweigh this cost.

**Q: Why enum-based PipelineData instead of associated types?**
**A**: `PipelinePlanner` constructs pipelines at runtime based on document analysis. This requires storing heterogeneous stages in `Vec<Box<dyn PipelineStage>>`. An enum-based approach with runtime validation is simpler and more flexible than GATs for compile-time safety.

**Q: Should stages be stateless?**
**A**: Stages can hold **configuration** (e.g., `AstTransforms` holds a `TransformPipeline`) but should not hold **mutable state** between executions. All mutable state goes in `StageContext`.

**Q: Where does conditional logic live?**
**A**: All conditional logic (engine selection, format branching, project vs single-doc) lives in `PipelinePlanner`. Stages are unconditional - they always run when included in a pipeline.

---

## Future Considerations

### Parallel Stage Execution

The current design is sequential within a single pipeline. For parallel execution:
- Multiple documents → multiple `Pipeline` instances run concurrently
- `PipelinePlanner` would create one pipeline per document
- A `PipelineExecutor` could run multiple pipelines in parallel
- Separate `StageContext` per document, shared kernel daemons via Planner

### Stage Granularity

TypeScript Quarto's 78 Lua filter stages will be consolidated into a smaller set of Rust stages. Users can still define Lua filters, so we will need a `RunLuaFilter` stage.

### Incremental Rendering

Future enhancement: checkpoint after stages and resume:

```rust
pub struct Pipeline {
    // ...
    checkpoints: HashMap<String, PipelineData>,  // stage_name → output
}
```

---

## Implementation Plan

### This Session (Design Only)
- [x] Design `PipelineStage` trait
- [x] Design `PipelineData` enum and individual data types
- [x] Design `Pipeline` struct with validation
- [x] Design `StageContext` with owned data (activation frame pattern)
- [x] Design `PipelineObserver` for tracing/progress abstraction
- [x] Design cancellation support with `CancellationToken`
- [x] Design `PipelineError` with `Cancelled` variant
- [x] Document WASM compatibility strategy
- [x] Review and refine design with user feedback

### Future Sessions
1. **Implement core abstractions** - trait, enum, Pipeline, StageContext
2. **Implement PipelineObserver** - NoopObserver, ProgressBarObserver, (OtelObserver behind feature)
3. **Implement basic stages** - LoadSource, ExtractMarkdown, ParseDocument, AstTransforms
4. **Wire up render_qmd_to_html** - refactor to use pipeline
5. **Implement HTML pipeline** - RenderHtml, HtmlPostprocessor, Finalize
6. **Design PipelinePlanner** - conditional pipeline construction, Format integration
7. **Implement engine stages** - ExecuteJupyter, ExecuteKnitr
8. **Implement RunLuaFilter stage** - for user-defined Lua filters
9. **Implement output relocation** - logic for RenderedOutput → FinalOutput transition
10. **Implement PDF pipeline** - RenderViaPandoc, LatexPostprocessor, PdfCompile

---

## Summary

This design provides:

1. **`PipelineData` enum (6 variants)** - All data types that flow through the pipeline
2. **`PipelineDataKind`** - Type tags for validation without pattern matching
3. **`PipelineStage` trait** - Simple interface: `name()`, `input_kind()`, `output_kind()`, `run()`
4. **`Pipeline` struct** - Validated sequence of stages with cancellation-aware `run()` method
5. **`StageContext`** - Owned context with no lifetime params (activation frame pattern)
6. **`PipelineObserver` trait** - Unified abstraction for tracing, progress, and WASM callbacks
7. **`PipelineError`** - Rich error type with stage context and `Cancelled` variant
8. **WASM compatibility** - Feature-gated OpenTelemetry, platform-agnostic core

The key insights are:
- **Conditional complexity lives in PipelinePlanner** (future work), not in the stage system
- **Owned StageContext eliminates lifetime complexity** in async code
- **PipelineObserver abstracts over tracing/progress** for CLI/WASM parity
- **Cancellation support** enables graceful Ctrl+C handling

---

## References

- [Single Document Render Pipeline Analysis](../render-pipeline/single-document/README.md)
- [Lua Filter Pipeline Analysis](lua-filter-pipeline/00-index.md)
- [Unified Render Pipeline Plan](2025-12-27-unified-render-pipeline.md)
- Current implementation: `crates/quarto-core/src/pipeline.rs`, `crates/quarto-core/src/transform.rs`
- Existing types: `crates/quarto-core/src/render.rs` (RenderContext, BinaryDependencies), `crates/quarto-core/src/project.rs` (ProjectContext, DocumentInfo), `crates/quarto-core/src/artifact.rs` (ArtifactStore)
- [OpenTelemetry Rust](../../../external-sources/opentelemetry-rust/README.md) - Cloned for reference
- [tracing-opentelemetry](https://docs.rs/tracing-opentelemetry/) - Bridge between tracing and OpenTelemetry
- [tokio-util CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)
