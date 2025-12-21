# Minimal `quarto render` Prototype Design

**Date**: 2025-12-20
**Status**: Proposal - Awaiting Review (Revision 3)
**Epic**: k-xlko

**Revision 3 Changes**:
- Added full pipeline architecture with explicit typed stages (section 0)
- Added `ArtifactStore` for unified key-value storage of intermediates (section 0.1)
- Added project-level orchestration design based on quarto-cli book analysis (section 0.2)
- Added `RenderPipeline` struct with converter, parser, transforms, writers
- Added `Writer` trait for output stage
- Updated `RenderContext` to include `ArtifactStore`
- Added multi-document rendering flow for books/websites
- Added design decisions #5-7 for typed stages, artifact store, orchestration

## Executive Summary

This plan describes a minimal, incrementally-improvable implementation of `quarto render` for the Rust port. The key insight is that for QMD→HTML rendering, **we should avoid calling Pandoc entirely** by leveraging our existing Rust infrastructure (pampa parser, quarto-doctemplate). Pandoc is only needed for formats we haven't implemented readers/writers for, or for features requiring Pandoc's Lua API.

## Design Philosophy

### Pandoc-Free Core Pipeline

The minimal prototype for `qmd → html` should use:

```
QMD Input
    ↓
pampa (QMD parser → Pandoc AST)
    ↓
Rust AST Transforms (ported from Lua filters)
    ↓
quarto-doctemplate (HTML writer)
    ↓
HTML Output + Dependencies
```

**Pandoc is invoked only when necessary:**
- Input formats pampa doesn't support (e.g., LaTeX, DOCX, RST)
- Output formats we haven't implemented writers for (e.g., PDF via LaTeX, DOCX)
- Features requiring Pandoc's Lua API that we haven't ported

This aligns with the existing `pico-quarto-render` prototype which already demonstrates this pipeline.

### Lua Filter Porting Strategy

The TypeScript version relies on ~211 Lua filter files (~31,600 LOC). The detailed analysis and design for porting the Lua filter infrastructure is in a separate document:

**See: [Lua Filter Infrastructure Porting](./2025-12-20-lua-filter-infrastructure-porting.md)** (Issue: k-thpl)

Key points:
- The custom node system (Callout, FloatRefTarget, etc.) can be implemented more directly in Rust
- Three design options: Native AST extension, Overlay system, or Hybrid approach
- Handler/Renderer registry pattern for format-conditional output
- Pandoc compatibility layer for JSON serialization when needed

## Current State Analysis

### What Already Works

| Component | Status | Location |
|-----------|--------|----------|
| CLI skeleton | Complete | `crates/quarto/src/main.rs` |
| Render command args | Defined | `Commands::Render` enum variant |
| **QMD parser** | **Mature** | `pampa` crate |
| YAML parsing | Complete | `quarto-yaml` crate |
| YAML validation | Phase 1 complete | `quarto-yaml-validation` crate |
| Config merging | Implemented | `quarto-config` crate |
| **Document templates** | **Working** | `quarto-doctemplate` crate |
| **Working HTML prototype** | **Proven** | `pico-quarto-render` crate |

### What Needs Building

1. **Project context management** - Find `_quarto.yml`, manage project state
2. **Dependency and resource management** - Track CSS, JS, images through pipeline
3. **AST transformation layer** - Port critical Lua filters to Rust
4. **Format resolution** - Merge metadata from multiple sources
5. **Engine abstraction** - Start with "markdown" engine (no execution)
6. **Third-party binary management** - dart-sass, esbuild (not Pandoc for MVP)

## Architecture Design

### Crate Organization (Open Question)

The user raised a valid concern about whether `quarto-render` should be a separate crate. The challenge is that rendering and project context are deeply intertwined.

**Option A: Single crate for render + project**
```
crates/quarto-project/
├── context.rs      # ProjectContext
├── types.rs        # ProjectType trait
├── render/         # Rendering pipeline
├── format/         # Format resolution
├── deps/           # Dependency management
└── engine/         # Execution engines
```

**Option B: Shared types in quarto-core, implementations in quarto crate**
```
crates/quarto-core/    # Shared traits and types
crates/quarto/src/
├── project/           # ProjectContext implementation
├── render/            # Render pipeline
└── commands/render.rs # CLI integration
```

**Recommendation**: Start with Option B (keep implementation in quarto crate) until we understand the abstraction boundaries better. Extract crates when clear patterns emerge.

### Core Abstractions

#### 0. Full Pipeline Architecture

The render pipeline has **explicit typed stages** with clear type boundaries:

```
┌─────────────────────────────────────────────────────────────────────┐
│                        RenderPipeline                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────┐                                               │
│  │  SourceConverter │  Optional: .ipynb/.py/%% → QMD text           │
│  │  (String → String)│  See: surface-syntax-converter-design.md     │
│  └────────┬─────────┘                                               │
│           ↓                                                          │
│  ┌──────────────────┐                                               │
│  │     Parser       │  pampa: QMD text → Pandoc AST                 │
│  │  (String → AST)  │                                               │
│  └────────┬─────────┘                                               │
│           ↓                                                          │
│  ┌──────────────────┐                                               │
│  │   Transforms     │  Vec<Box<dyn AstTransform>>                   │
│  │   (AST → AST)    │  Normalization, includes, engines,            │
│  │                  │  handlers, filters, crossrefs                 │
│  └────────┬─────────┘                                               │
│           ↓                                                          │
│  ┌──────────────────┐                                               │
│  │     Writers      │  Vec<Box<dyn Writer>>                         │
│  │  (AST → Output)  │  HTML, LaTeX, record intermediate, etc.       │
│  └──────────────────┘                                               │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

```rust
/// The complete render pipeline with explicit typed stages.
/// Built by project types based on format and configuration.
pub struct RenderPipeline {
    /// Optional surface syntax converter (.ipynb, percent scripts, R spin)
    /// Transforms non-QMD formats into QMD text before parsing.
    pub converter: Option<Box<dyn SourceConverter>>,

    /// Parser that turns QMD text into Pandoc AST.
    /// Default: pampa. Could be swapped for Pandoc JSON input, etc.
    pub parser: Box<dyn Parser>,

    /// Ordered sequence of AST→AST transforms.
    /// Built dynamically by project type based on format/config.
    pub transforms: Vec<Box<dyn AstTransform>>,

    /// Writers that produce output from the final AST.
    /// Multiple writers can run on the same AST (e.g., HTML + record markdown).
    pub writers: Vec<Box<dyn Writer>>,
}

/// Parser trait: text → AST
pub trait Parser: Send + Sync {
    fn name(&self) -> &str;

    /// Parse text content into Pandoc AST
    fn parse(&self, content: &str, ctx: &mut RenderContext) -> Result<PandocDocument>;
}

/// Writer trait: AST → output(s)
pub trait Writer: Send + Sync {
    fn name(&self) -> &str;

    /// Write the AST to output.
    /// May produce files, record artifacts, or both.
    fn write(&self, doc: &PandocDocument, ctx: &mut RenderContext) -> Result<WriteResult>;
}

pub struct WriteResult {
    /// Primary output file (if any)
    pub output_file: Option<PathBuf>,

    /// Additional files produced (supporting files, lib/, etc.)
    pub additional_files: Vec<PathBuf>,
}
```

**Pipeline Execution:**

```rust
impl RenderPipeline {
    pub fn execute(&self, input: PipelineInput, ctx: &mut RenderContext) -> Result<PipelineOutput> {
        // Stage 1: Surface syntax conversion (optional)
        let qmd_text = if let Some(converter) = &self.converter {
            let converted = converter.convert(&input.content, ctx)?;
            ctx.artifacts.store("source-map", converted.source_map)?;
            converted.qmd
        } else {
            input.content
        };

        // Stage 2: Parse to AST
        let mut doc = self.parser.parse(&qmd_text, ctx)?;

        // Stage 3: Run transforms in order
        for transform in &self.transforms {
            log::debug!("[{}] {}", transform.stage(), transform.name());
            transform.transform(&mut doc, ctx)?;
        }

        // Stage 4: Run writers (can be multiple)
        let mut outputs = Vec::new();
        for writer in &self.writers {
            log::debug!("[write] {}", writer.name());
            let result = writer.write(&doc, ctx)?;
            outputs.push(result);
        }

        Ok(PipelineOutput { outputs })
    }
}
```

#### 0.1 Artifact Store (Unified Key-Value Storage)

The `ArtifactStore` is a unified storage system for:
- **Intermediate documents** (markdown for book PDF compilation)
- **Supporting files** (images, data files generated by code execution)
- **Dependency files** (CSS, JS to be copied to output)
- **Source maps** (for error reporting back to original format)

This is a key-value store where values are byte buffers, enabling:
- Text files (markdown, HTML, JSON)
- Binary files (images, PDFs)
- Structured data (serialized for downstream use)

```rust
/// Unified artifact storage, shared between dependency system and intermediates.
pub struct ArtifactStore {
    /// Artifacts keyed by string identifier
    artifacts: HashMap<String, Artifact>,
}

/// An artifact stored during rendering.
/// Can be text, binary, or structured data.
pub struct Artifact {
    /// Raw content as bytes
    pub content: Vec<u8>,

    /// Content type hint (MIME type or custom identifier)
    /// Examples: "text/markdown", "image/png", "application/x-pandoc-ast"
    pub content_type: String,

    /// Optional file path if this artifact corresponds to a file
    pub path: Option<PathBuf>,

    /// Arbitrary metadata for downstream consumers
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ArtifactStore {
    pub fn new() -> Self {
        Self { artifacts: HashMap::new() }
    }

    /// Store an artifact by key
    pub fn store(&mut self, key: &str, artifact: Artifact) {
        self.artifacts.insert(key.to_string(), artifact);
    }

    /// Store text content
    pub fn store_text(&mut self, key: &str, content: &str, content_type: &str) {
        self.store(key, Artifact {
            content: content.as_bytes().to_vec(),
            content_type: content_type.to_string(),
            path: None,
            metadata: HashMap::new(),
        });
    }

    /// Store bytes
    pub fn store_bytes(&mut self, key: &str, content: Vec<u8>, content_type: &str) {
        self.store(key, Artifact {
            content,
            content_type: content_type.to_string(),
            path: None,
            metadata: HashMap::new(),
        });
    }

    /// Retrieve artifact by key
    pub fn get(&self, key: &str) -> Option<&Artifact> {
        self.artifacts.get(key)
    }

    /// Get all artifacts matching a prefix (e.g., "chapter:" for all chapters)
    pub fn get_by_prefix(&self, prefix: &str) -> Vec<(&str, &Artifact)> {
        self.artifacts.iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// List all keys
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.artifacts.keys().map(|s| s.as_str())
    }
}
```

**Usage Examples:**

```rust
// Writer records intermediate markdown for book PDF
impl Writer for MarkdownRecorder {
    fn write(&self, doc: &PandocDocument, ctx: &mut RenderContext) -> Result<WriteResult> {
        let markdown = render_to_markdown(doc)?;
        ctx.artifacts.store_text(
            &format!("intermediate:markdown:{}", self.chapter_id),
            &markdown,
            "text/markdown",
        );
        Ok(WriteResult { output_file: None, additional_files: vec![] })
    }
}

// Engine execution stores output images
impl AstTransform for JupyterEngine {
    fn transform(&self, doc: &mut PandocDocument, ctx: &mut RenderContext) -> Result<()> {
        // ... execute code ...
        for (i, image_bytes) in output_images.iter().enumerate() {
            ctx.artifacts.store_bytes(
                &format!("execution:image:{}:{}", cell_id, i),
                image_bytes.clone(),
                "image/png",
            );
        }
        Ok(())
    }
}

// Project-level finalization collects all chapter markdowns
impl BookProjectType {
    fn finalize(&self, ctx: &RenderContext) -> Result<()> {
        let chapters: Vec<_> = ctx.artifacts
            .get_by_prefix("intermediate:markdown:")
            .into_iter()
            .map(|(key, artifact)| {
                String::from_utf8_lossy(&artifact.content).to_string()
            })
            .collect();

        let merged = chapters.join("\n\n---\n\n");
        // ... render merged markdown to LaTeX/PDF ...
        Ok(())
    }
}
```

#### 0.2 Project-Level Orchestration

Project types (book, website, default) orchestrate multi-document rendering:

```rust
pub trait ProjectType: Send + Sync {
    fn name(&self) -> &str;

    /// Build the render pipeline for a single document.
    /// Called once per document, may vary based on format and document role.
    fn build_pipeline(&self, doc: &DocumentInfo, format: &Format) -> RenderPipeline;

    /// Called after each document is rendered.
    /// Opportunity to accumulate state for project-level finalization.
    fn on_document_rendered(
        &self,
        doc: &DocumentInfo,
        ctx: &RenderContext,
        result: &PipelineOutput,
    );

    /// Called after all documents are rendered.
    /// For books: merge chapters, resolve cross-refs, compile PDF.
    /// For websites: build search index, sitemap.
    fn finalize(&self, project_ctx: &mut ProjectRenderContext) -> Result<Vec<FinalOutput>>;
}
```

**Book Project Pattern** (based on quarto-cli analysis):

```rust
impl ProjectType for BookProjectType {
    fn build_pipeline(&self, doc: &DocumentInfo, format: &Format) -> RenderPipeline {
        let mut pipeline = RenderPipeline::new();

        // Standard transforms...
        pipeline.transforms.push(Box::new(MetadataNormalize::new()));
        // ... more transforms ...

        if format.is_multi_file() {
            // HTML: render each chapter to its own file
            pipeline.writers.push(Box::new(HtmlWriter::new()));
        } else {
            // PDF/ePub: record intermediate markdown, don't write final yet
            pipeline.writers.push(Box::new(MarkdownRecorder::new(doc.chapter_id())));
        }

        pipeline
    }

    fn finalize(&self, ctx: &mut ProjectRenderContext) -> Result<Vec<FinalOutput>> {
        let mut outputs = Vec::new();

        for format in &ctx.formats {
            if format.is_multi_file() {
                // HTML: post-process for cross-chapter crossrefs, bibliography
                self.resolve_crossrefs(ctx)?;
                self.process_bibliography(ctx)?;
                self.build_search_index(ctx)?;
            } else {
                // PDF: collect all intermediate markdowns, merge, render
                let chapters = ctx.artifacts.get_by_prefix("intermediate:markdown:");
                let merged = self.merge_chapters(chapters)?;
                let pdf = self.render_to_pdf(&merged, ctx)?;
                outputs.push(pdf);
            }
        }

        Ok(outputs)
    }
}
```

#### 1. ProjectContext

```rust
pub struct ProjectContext {
    /// Project root directory
    pub dir: PathBuf,

    /// Parsed project configuration (if _quarto.yml exists)
    pub config: Option<ProjectConfig>,

    /// Project type handler
    pub project_type: Box<dyn ProjectType>,

    /// Is this a single-file pseudo-project?
    pub is_single_file: bool,

    /// List of input files to render
    pub files: ProjectFiles,

    /// Execution engines used in this project
    pub engines: Vec<String>,

    /// Binary dependencies (dart-sass, esbuild, etc.)
    pub binaries: BinaryDependencies,
}
```

#### 2. Dependency and Resource Management (NEW)

Based on the quarto-cli analysis, we need three distinct concepts:

```rust
/// A format dependency that gets injected into output
/// (CSS, JS, meta tags, link tags)
pub struct FormatDependency {
    /// Unique identifier (e.g., "bootstrap", "quarto-html", "mermaid")
    pub name: String,

    /// Semantic version (for lib directory naming)
    pub version: Option<String>,

    /// External dependency (goes to quarto-contrib/)
    pub external: bool,

    /// JavaScript files to inject
    pub scripts: Vec<DependencyFile>,

    /// CSS files to inject
    pub stylesheets: Vec<DependencyFile>,

    /// Meta tags to inject
    pub meta: Vec<(String, String)>,

    /// Link tags to inject
    pub links: Vec<LinkTag>,

    /// Raw HTML to inject into <head>
    pub head_html: Option<String>,

    /// Resource files to copy (not injected)
    pub resources: Vec<DependencyFile>,
}

pub struct DependencyFile {
    /// File name in output
    pub name: String,
    /// Source path on disk
    pub path: PathBuf,
    /// HTML attributes (defer, async, etc.)
    pub attribs: HashMap<String, String>,
    /// Inject after body instead of in header
    pub after_body: bool,
}

/// Collected during rendering, processed at end
pub struct DependencyCollector {
    /// Format dependencies (CSS, JS to inject)
    dependencies: Vec<FormatDependency>,

    /// SASS bundles to compile
    sass_bundles: Vec<SassBundle>,

    /// Discovered resources (images, data files)
    resources: Vec<PathBuf>,

    /// Supporting files (excluded from resource discovery)
    supporting: Vec<PathBuf>,
}

impl DependencyCollector {
    /// Add a format dependency
    pub fn add_dependency(&mut self, dep: FormatDependency);

    /// Add a SASS bundle for compilation
    pub fn add_sass_bundle(&mut self, bundle: SassBundle);

    /// Mark a resource file as needed
    pub fn add_resource(&mut self, path: PathBuf);

    /// Mark a file as supporting (excluded from resources)
    pub fn add_supporting(&mut self, path: PathBuf);

    /// Process all dependencies: compile SASS, copy files, generate HTML
    pub fn finalize(
        &self,
        output_dir: &Path,
        binaries: &BinaryDependencies,
    ) -> Result<FinalizedDependencies>;
}
```

#### 3. SASS Compilation Layer

```rust
/// A SASS layer with ordered sections
pub struct SassLayer {
    pub uses: String,      // @use directives
    pub defaults: String,  // $variable defaults
    pub functions: String, // @function definitions
    pub mixins: String,    // @mixin definitions
    pub rules: String,     // CSS rules
}

/// A bundle of SASS to compile together
pub struct SassBundle {
    /// Which dependency to attach the compiled CSS to
    pub dependency: String,

    /// Unique key for caching
    pub key: String,

    /// User-provided layers
    pub user: Vec<SassLayer>,

    /// Built-in Quarto layers
    pub quarto: Option<SassLayer>,

    /// Framework layers (Bootstrap)
    pub framework: Option<SassLayer>,

    /// Dark mode variant layers
    pub dark: Option<DarkModeLayers>,

    /// Load paths for @use resolution
    pub load_paths: Vec<PathBuf>,
}

/// Compile SASS bundles to CSS using dart-sass
pub fn compile_sass_bundles(
    bundles: &[SassBundle],
    dart_sass: &Path,
    minified: bool,
) -> Result<Vec<CompiledCss>>;
```

#### 4. AST Transformation Layer

This replaces the Lua filter chain for the Rust-native pipeline. The detailed design is in the [Lua Filter Infrastructure Porting](./2025-12-20-lua-filter-infrastructure-porting.md) document.

**Key Design Decision**: The pipeline is a **flat, ordered vector** of transforms rather than hardcoded phases. This enables:
- Dynamic configuration by project types (website, book, default)
- Uniform treatment of engine execution, handlers, and Lua filters
- Future user-configurable pipeline customization

```rust
/// Context passed to all pipeline stages during rendering.
/// Contains mutable state that transforms and writers can read and write.
pub struct RenderContext<'a> {
    /// Collected dependencies (CSS, JS, resources)
    pub dependencies: DependencyCollector,

    /// Artifact store for intermediates, supporting files, source maps.
    /// Unified key-value storage shared across all pipeline stages.
    /// See section 0.1 for full API.
    pub artifacts: ArtifactStore,

    /// Project context (config, paths, engines)
    pub project: &'a ProjectContext,

    /// Target format being rendered
    pub format: &'a Format,

    /// Binary dependencies (dart-sass, pandoc, etc.)
    pub binaries: &'a BinaryDependencies,

    /// Information about the current document being rendered
    pub document: &'a DocumentInfo,
}

/// A transformation pass over the Pandoc AST.
///
/// All pipeline stages implement this trait uniformly:
/// - Normalization transforms
/// - Include shortcode processing
/// - Engine execution (Jupyter, Knitr)
/// - Diagram handlers (mermaid, graphviz)
/// - Lua filters (via Pandoc interop)
/// - Cross-reference resolution
/// - Format-specific rendering
pub trait AstTransform: Send + Sync {
    /// Human-readable name for logging and debugging
    fn name(&self) -> &str;

    /// Optional stage name for grouping in logs (e.g., "normalize", "execute", "handlers")
    fn stage(&self) -> &str { "transform" }

    /// Transform the document in place.
    ///
    /// Each transform decides internally whether to skip (e.g., if no relevant
    /// content exists). This avoids over-engineering a configuration language.
    fn transform(
        &self,
        doc: &mut PandocDocument,
        ctx: &mut RenderContext,
    ) -> Result<()>;
}

/// The transformation pipeline: an ordered sequence of transforms.
///
/// Pipeline construction is the responsibility of project types (website, book,
/// default). The executor simply iterates and runs each transform in order.
pub struct TransformPipeline {
    transforms: Vec<Box<dyn AstTransform>>,
}

impl TransformPipeline {
    pub fn new() -> Self {
        Self { transforms: Vec::new() }
    }

    /// Add a transform to the end of the pipeline
    pub fn push(&mut self, transform: Box<dyn AstTransform>) {
        self.transforms.push(transform);
    }

    /// Execute all transforms in order
    pub fn execute(
        &self,
        doc: &mut PandocDocument,
        ctx: &mut RenderContext,
    ) -> Result<()> {
        for transform in &self.transforms {
            log::debug!("[{}] Running: {}", transform.stage(), transform.name());
            transform.transform(doc, ctx)?;
        }
        Ok(())
    }
}
```

**Pipeline Construction Example** (by project type):

```rust
impl DefaultProjectType {
    pub fn build_pipeline(&self, format: &Format) -> TransformPipeline {
        let mut pipeline = TransformPipeline::new();

        // Normalization
        pipeline.push(Box::new(MetadataNormalize::new()));
        pipeline.push(Box::new(ShortcodeNormalize::new()));

        // Include processing
        pipeline.push(Box::new(IncludeShortcodes::new()));

        // Engine execution (each is just an AST transform)
        pipeline.push(Box::new(JupyterEngine::new()));
        pipeline.push(Box::new(KnitrEngine::new()));

        // Handlers (diagram rendering, etc.)
        pipeline.push(Box::new(MermaidHandler::new()));
        pipeline.push(Box::new(GraphvizHandler::new()));

        // User Lua filters (if configured)
        for filter_path in &self.config.filters {
            pipeline.push(Box::new(LuaFilter::new(filter_path)));
        }

        // Cross-references
        pipeline.push(Box::new(CrossRefResolve::new()));

        // Format-specific finalization
        if format.is_html() {
            pipeline.push(Box::new(HtmlFinalize::new()));
        }

        pipeline
    }
}
```

**Future: Async Execution**

The current design is synchronous for simplicity. When parallelization becomes a priority (multiple files, execution planning), the trait can be converted to async:

```rust
// Future evolution (not implemented now)
#[async_trait]
pub trait AstTransform: Send + Sync {
    async fn transform(&self, doc: &mut PandocDocument, ctx: &mut RenderContext) -> Result<()>;
}
```

This conversion is mechanical since we control all implementations. The `RenderContext` pattern helps by avoiding parameter list changes across the codebase.

#### 5. Custom Node System

The custom node system enables Quarto-specific node types (Callout, FloatRefTarget, etc.) that don't exist in Pandoc's AST.

**See [Lua Filter Infrastructure Porting](./2025-12-20-lua-filter-infrastructure-porting.md)** for the detailed design including:
- Three design options (Native AST, Overlay, Hybrid)
- Handler and Renderer trait definitions
- Slot-based storage for AST content
- Format-conditional rendering
- Pandoc JSON compatibility

#### 6. Format Resolution

```rust
pub struct Format {
    /// Format identifier (e.g., "html", "pdf", "docx")
    pub identifier: FormatIdentifier,

    /// User-visible metadata
    pub metadata: Metadata,

    /// Pandoc options (only used when calling Pandoc)
    pub pandoc: Option<PandocOptions>,

    /// Render options
    pub render: RenderOptions,

    /// Execute options
    pub execute: ExecuteOptions,

    /// Output extension
    pub output_extension: String,

    /// Does this format use native Rust pipeline?
    pub native_pipeline: bool,
}

impl Format {
    /// HTML uses native Rust pipeline
    pub fn html() -> Self {
        Self {
            identifier: FormatIdentifier::Html,
            native_pipeline: true,
            output_extension: "html".to_string(),
            ..Default::default()
        }
    }

    /// PDF requires Pandoc
    pub fn pdf() -> Self {
        Self {
            identifier: FormatIdentifier::Pdf,
            native_pipeline: false, // Requires Pandoc → LaTeX
            output_extension: "pdf".to_string(),
            ..Default::default()
        }
    }
}
```

#### 7. Binary Dependency Management

```rust
pub struct BinaryDependencies {
    /// dart-sass binary path (required for SASS theming)
    pub dart_sass: Option<PathBuf>,

    /// esbuild binary path (for JS bundling)
    pub esbuild: Option<PathBuf>,

    /// Pandoc binary path (only for non-native formats)
    pub pandoc: Option<PathBuf>,

    /// Typst binary path
    pub typst: Option<PathBuf>,
}

impl BinaryDependencies {
    pub fn discover() -> Result<Self> {
        Ok(Self {
            dart_sass: Self::find_optional("sass", "QUARTO_DART_SASS"),
            esbuild: Self::find_optional("esbuild", "QUARTO_ESBUILD"),
            pandoc: Self::find_optional("pandoc", "QUARTO_PANDOC"),
            typst: Self::find_optional("typst", "QUARTO_TYPST"),
        })
    }

    fn find_optional(name: &str, env_var: &str) -> Option<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var(env_var) {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }

        // 2. Try to find in PATH
        which::which(name).ok()
    }
}
```

### Rendering Flow

#### Single Document Flow

```
Input: chapter.qmd (or .ipynb, percent script, etc.)
         ↓
[1] ProjectContext::discover()
    - Find _quarto.yml (if exists)
    - Determine project type (book, website, default)
    - Resolve format from config + YAML front matter
         ↓
[2] ProjectType::build_pipeline(doc, format)
    - Build RenderPipeline with explicit typed stages:
      - converter: Option<SourceConverter>  (.ipynb → qmd)
      - parser: Parser                      (qmd → AST)
      - transforms: Vec<AstTransform>       (AST → AST)
      - writers: Vec<Writer>                (AST → outputs)
         ↓
[3] RenderContext::new()
    - Initialize DependencyCollector
    - Initialize ArtifactStore (key-value storage)
    - Bind project, format, binaries, document references
         ↓
[4] RenderPipeline::execute()
    ┌─────────────────────────────────────────────────────┐
    │ Stage 1: SourceConverter (optional)                 │
    │   - .ipynb → qmd text                               │
    │   - Store source map in ctx.artifacts               │
    ├─────────────────────────────────────────────────────┤
    │ Stage 2: Parser                                     │
    │   - pampa: qmd text → Pandoc AST                    │
    ├─────────────────────────────────────────────────────┤
    │ Stage 3: Transforms (ordered vector)                │
    │   for transform in transforms:                      │
    │     - transform.transform(&mut doc, &mut ctx)       │
    │     - May add to ctx.dependencies                   │
    │     - May store in ctx.artifacts                    │
    │   Includes: normalization, includes, engines,       │
    │   handlers, Lua filters, crossrefs                  │
    ├─────────────────────────────────────────────────────┤
    │ Stage 4: Writers (can be multiple)                  │
    │   for writer in writers:                            │
    │     - writer.write(&doc, &mut ctx)                  │
    │   Examples:                                         │
    │     - HtmlWriter → chapter.html                     │
    │     - MarkdownRecorder → store in ctx.artifacts     │
    │     - LaTeXWriter → chapter.tex                     │
    └─────────────────────────────────────────────────────┘
         ↓
[5] DependencyCollector::finalize()
    - Compile SASS bundles via dart-sass
    - Copy script/stylesheet files to lib/
    - Inject <script>, <link>, <meta> into HTML
    - Copy resource files
         ↓
[6] ProjectType::on_document_rendered()
    - Accumulate state for project-level finalization
         ↓
Output: chapter.html + lib/ directory
        + artifacts stored for project-level use
```

#### Multi-Document Flow (Book, Website)

```
Project with: chapter1.qmd, chapter2.qmd, chapter3.qmd
Format: HTML + PDF
         ↓
[1] ProjectContext::discover()
    - Find _quarto.yml with project.type: book
    - Build BookRenderItem[] (ordered chapter list)
         ↓
[2] For each document, for each format:
    ┌─────────────────────────────────────────────────────┐
    │ chapter1.qmd + HTML:                                │
    │   Pipeline with HtmlWriter → chapter1.html          │
    │                                                     │
    │ chapter1.qmd + PDF:                                 │
    │   Pipeline with MarkdownRecorder                    │
    │   → stores "intermediate:markdown:ch1" in artifacts │
    ├─────────────────────────────────────────────────────┤
    │ chapter2.qmd + HTML → chapter2.html                 │
    │ chapter2.qmd + PDF  → artifacts["intermediate:..."] │
    ├─────────────────────────────────────────────────────┤
    │ chapter3.qmd + HTML → chapter3.html                 │
    │ chapter3.qmd + PDF  → artifacts["intermediate:..."] │
    └─────────────────────────────────────────────────────┘
         ↓
[3] ProjectType::finalize()
    ┌─────────────────────────────────────────────────────┐
    │ HTML format (multi-file):                           │
    │   - resolve_crossrefs(): fix cross-chapter refs     │
    │   - process_bibliography(): run citeproc            │
    │   - build_search_index(): aggregate for search      │
    ├─────────────────────────────────────────────────────┤
    │ PDF format (single-file):                           │
    │   - Collect all "intermediate:markdown:*" artifacts │
    │   - merge_chapters(): combine into single doc       │
    │   - render_to_latex(): produce combined .tex        │
    │   - compile_pdf(): run latexmk → book.pdf           │
    └─────────────────────────────────────────────────────┘
         ↓
Output:
  HTML: chapter1.html, chapter2.html, chapter3.html, search.json
  PDF:  book.pdf (from merged intermediates)
```

## Implementation Phases

### Phase 1: Foundation (MVP) - REVISED

**Goal**: Render a single `.qmd` file to HTML **without calling Pandoc**

1. **Project context detection**
   - Implement `ProjectContext::discover(path)` that walks up looking for `_quarto.yml`
   - Parse `_quarto.yml` using `quarto-yaml`
   - Handle single-file mode (no project config)

2. **Dependency collector skeleton**
   - Implement `FormatDependency` and `DependencyCollector`
   - Basic file copying (no SASS compilation yet)

3. **Wire up existing infrastructure**
   - pampa for parsing
   - quarto-doctemplate for HTML output
   - Connect to render command

4. **Basic CLI integration**
   - Connect render command to pipeline
   - Error handling and progress output

**Deliverables**:
- `cargo run -- render simple.qmd` produces `simple.html`
- Uses pampa + quarto-doctemplate (no Pandoc)
- No code execution (markdown engine only)
- Basic HTML output (no styling yet)

### Phase 2: Dependency System

**Goal**: Support CSS, JS, and resource dependencies

1. **SASS compilation**
   - Implement `SassBundle` and `SassLayer`
   - Invoke dart-sass for compilation
   - Handle dark mode variants

2. **Dependency injection**
   - Copy files to `lib/` directory
   - Inject `<script>` and `<link>` tags into HTML
   - Handle external vs internal dependencies

3. **Resource discovery**
   - Track images and data files referenced in markdown
   - Copy to output directory

**Deliverables**:
- CSS styling works (Bootstrap, Quarto themes)
- JavaScript dependencies injected
- Images and resources copied correctly

### Phase 3: AST Transformation Layer

**Goal**: Port critical Lua filters to Rust

1. **Normalization transforms**
   - Pandoc AST → Quarto extended AST
   - Custom node parsing

2. **Core transforms**
   - Figure/table handling
   - Callout parsing and rendering
   - Code block processing

3. **Cross-reference system**
   - Float reference targets
   - Reference resolution
   - Index generation

**Deliverables**:
- Cross-references work
- Figures and tables render correctly
- Callouts styled properly

### Phase 4: Project Type System

**Goal**: Support project-level configuration

1. **Project type registry**
   - Implement `DefaultProjectType`
   - Type detection from `_quarto.yml project.type` field

2. **Multi-file rendering**
   - Discover input files from project directory
   - Respect `project.render` glob patterns

3. **Output directory handling**
   - Implement `--output-dir` flag
   - Project-relative output paths

**Deliverables**:
- `cargo run -- render` in a project directory renders all files
- Respects `_quarto.yml` project configuration

### Phase 5: Engine Infrastructure

**Goal**: Prepare for code execution

1. **Engine trait and registry**
   - Define `ExecutionEngine` trait
   - Implement `MarkdownEngine` (passthrough)
   - Engine selection algorithm

2. **Freeze/thaw support**
   - Check `_freeze/` for cached results
   - Skip execution if frozen

3. **Jupyter engine (placeholder)**
   - Define interface for future implementation

**Deliverables**:
- Engine selection works
- Freeze/thaw checks implemented

### Phase 6: Non-Native Formats (Pandoc Integration)

**Goal**: Support formats requiring Pandoc

1. **Pandoc invocation layer**
   - Emit Pandoc JSON from Rust AST
   - Call Pandoc with `--from json`
   - Lua filter integration

2. **PDF recipe**
   - LaTeX generation via Pandoc
   - latexmk integration
   - LaTeX postprocessor

3. **Other formats**
   - DOCX, EPUB via Pandoc

**Deliverables**:
- `cargo run -- render doc.qmd -t pdf` works
- Multiple output formats supported

## Key Design Decisions

### 1. Pandoc-Free Core Pipeline

**Decision**: Use pampa + quarto-doctemplate for QMD→HTML.

**Rationale**:
- Avoids external binary dependency for common case
- Enables full source location tracking
- Better performance (no IPC overhead)
- Pandoc only needed for formats we haven't implemented

### 2. Lua Filter Porting to Rust

**Decision**: Port critical filters (normalization, cross-refs) to Rust, keep format-specific Lua for Pandoc-based formats.

**Rationale**:
- Core functionality in Rust enables native pipeline
- ~31,600 LOC of Lua is too much to port all at once
- Format-specific Lua (LaTeX, DOCX) stays with Pandoc path

### 3. Dependency Collector Pattern

**Decision**: Use a collector that accumulates dependencies during AST transformation, then processes all at once.

**Rationale**:
- Matches quarto-cli's JSON-lines temp file pattern
- Allows transforms to declare dependencies without knowing output details
- Enables deferred SASS compilation (all bundles at once)

### 4. Flat Pipeline with Uniform Transforms

**Decision**: Use a flat `Vec<Box<dyn AstTransform>>` rather than hardcoded phase buckets.

**Rationale**:
- All pipeline stages (normalization, includes, engine execution, handlers, Lua filters, cross-refs) share the same interface
- Project types (website, book, default) control pipeline construction
- Engine execution is "just another transform" even if it shells out and reconciles
- Avoids over-engineering a phase configuration language—each transform decides internally whether to skip
- Future async conversion is mechanical since we control all implementations
- `RenderContext` bundles mutable state, reducing parameter churn when extending

### 5. Explicit Typed Pipeline Stages

**Decision**: Use a `RenderPipeline` struct with explicit typed stages: `SourceConverter`, `Parser`, `Vec<AstTransform>`, `Vec<Writer>`.

**Rationale**:
- Type changes are fundamental: text → AST → output files
- Explicit stages make the data flow clear and type-safe
- Multiple writers enable dual-output patterns (HTML + record intermediate)
- Surface syntax conversion is decoupled from AST transforms
- Parser is swappable (pampa, Pandoc JSON input, etc.)

### 6. Unified Artifact Store

**Decision**: Use a key-value `ArtifactStore` (bytes buffer) for intermediates, supporting files, source maps, and dependencies.

**Rationale**:
- Unified storage for all pipeline artifacts (text, binary, structured data)
- Shared between dependency system and intermediate document storage
- Enables book pattern: chapters record intermediate markdown, project-level finalize collects them
- Prefix-based retrieval (`"intermediate:markdown:*"`) for collecting related artifacts
- Content-type hints allow downstream consumers to interpret artifacts correctly

### 7. Project-Level Orchestration

**Decision**: Project types (book, website, default) implement `build_pipeline()`, `on_document_rendered()`, and `finalize()` hooks.

**Rationale**:
- Based on quarto-cli book implementation analysis
- Enables dual-path rendering: multi-file (HTML) vs single-file (PDF)
- `finalize()` handles cross-document operations: crossref resolution, bibliography, search index, chapter merging
- Clean separation between per-document pipeline and project-level coordination

### 8. Crate Boundary (Deferred)

**Decision**: Start with implementation in quarto crate, extract when patterns emerge.

**Rationale**:
- User concern about premature abstraction is valid
- ProjectContext and rendering are tightly coupled
- Will extract crates as clear boundaries become apparent

**2025-12-21 Update**: Confirmed. All new render pipeline code goes in `crates/quarto/src/` for now.

### 9. Unified Artifact System

**Decision**: Use a single `ArtifactStore` concept (not separate "DependencyCollector").

**Rationale**:
- "Dependency" is quarto-cli terminology; "Artifact" is clearer for Rust port
- The artifact store must support all quarto-cli dependency use cases:
  - CSS/JS files to inject
  - SASS bundles to compile
  - Resources to copy
  - Intermediate documents (for book PDF)
  - Supporting files from code execution
- Single unified key-value store is simpler than multiple systems

**2025-12-21 Update**: Confirmed. Refactor plan documents to use "ArtifactStore" consistently.

### 10. SASS Compilation (MVP)

**Decision**: Skip SASS compilation for MVP. Use pre-compiled CSS from quarto-cli.

**Rationale**:
- Pre-compiled CSS available in `resources/basic-quarto-website-compiled/_site/site_libs/bootstrap/`
- Reduces MVP scope significantly
- SASS compilation can be added in Phase 2

**2025-12-21 Update**: Confirmed. Link to static CSS files directly.

### 11. EJS/Navigation Templates (MVP)

**Decision**: Skip navbar/sidebar/footer rendering for MVP. No templating engine needed initially.

**Rationale**:
- Navigation adds significant complexity (EJS templates, config parsing)
- MVP focuses on single-document rendering
- Can output simple HTML without navigation structure
- Tera or QuickJS decision deferred to when website features are needed

**2025-12-21 Update**: Confirmed. MVP renders content only, no navigation chrome.

## Testing Strategy

### Unit Tests
- Project context detection (mock filesystem)
- Dependency collector behavior
- SASS bundle compilation
- AST transformation correctness

### Integration Tests
- End-to-end: `.qmd` → `.html` with dependencies
- Project rendering
- CLI flag handling

### Corpus Tests
- Run against Quarto documentation corpus
- Compare output with TypeScript version
- Track parity metrics

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| SASS compilation complexity | Medium | Medium | Use dart-sass directly, match TS layer ordering |
| AST transform correctness | Medium | High | Port tests from Lua, extensive corpus testing |
| Dependency injection edge cases | Medium | Medium | Match TS dependency file patterns exactly |
| Performance regression | Low | Medium | Benchmark against TypeScript version |

## Success Criteria

### Phase 1 (MVP)
- [ ] `cargo run -- render simple.qmd` produces valid HTML
- [ ] Uses pampa + quarto-doctemplate (no Pandoc)
- [ ] Basic error handling works

### Phase 2 (Dependencies)
- [ ] CSS styling works
- [ ] JavaScript dependencies injected
- [ ] Resources copied correctly

### Full Prototype
- [ ] Quarto docs corpus renders successfully
- [ ] Output quality matches TypeScript version
- [ ] Performance within 2x of TypeScript version (likely faster)

## Open Questions

1. **Crate boundary**: Should we start with everything in `quarto` crate and extract later, or define module boundaries upfront?

2. **SASS compilation**: Should we require dart-sass for the MVP, or start with pre-compiled CSS?

3. **Custom node system**: Should custom nodes be part of the pampa AST types, or a separate overlay?

4. **Engine priority**: After markdown engine, which execution engine first? (Jupyter seems most common)

---

## Implementation Summary (2025-12-21)

This section captures the final implementation plan after design review.

### Implementation Steps

| Step | Description | Dependencies |
|------|-------------|--------------|
| 1 | Add `CustomNode`, `Slot`, `Block::Custom`, `Inline::Custom` to `quarto-pandoc-types` | None |
| 2 | JSON serde for CustomNode with wrapper Div/Span round-trip (for Lua compatibility) | Step 1 |
| 3 | Create render infrastructure in `quarto` crate: `RenderContext`, `ProjectContext`, `ArtifactStore` | None |
| 4 | Wire up `quarto render` command skeleton | Step 3 |
| 5 | AST transform framework (`AstTransform` trait, `TransformPipeline`) | Steps 1, 3 |
| 6 | Essential transforms: custom node parsing, link resolution, figure handling, dependency collection | Steps 2, 5 |
| 7 | HTML writer with CustomNode support (Callout → HTML, etc.) | Steps 1, 6 |
| 8 | Template integration with dependency injection | Step 7 |
| 9 | Static CSS/JS resources (copy Bootstrap from pre-compiled assets) | Step 8 |
| 10 | End-to-end integration and testing | All above |

Steps 1-2 and 3-4 can proceed in parallel. Critical path: CustomNode types → transforms → HTML writer.

### HTML Testing Strategy

**Decision**: Use property-based HTML assertions instead of snapshot tests.

Snapshot tests are brittle when the output format is evolving. Instead, we test semantic properties of the HTML output using `scraper` (html5ever-based):

```rust
pub struct HtmlDoc { inner: Html }

impl HtmlDoc {
    pub fn parse(html: &str) -> Self;
    pub fn exists(&self, selector: &str) -> bool;
    pub fn count(&self, selector: &str) -> usize;
    pub fn text(&self, selector: &str) -> Option<String>;
    pub fn attr(&self, selector: &str, attr: &str) -> Option<String>;
    pub fn has_class(&self, selector: &str, class: &str) -> bool;
}
```

Example test:
```rust
#[test]
fn test_callout_structure() {
    let html = render_qmd("::: {.callout-warning}\nBe careful!\n:::");
    let doc = HtmlDoc::parse(&html);

    assert!(doc.exists("div.callout.callout-warning"));
    assert!(doc.text(".callout-body p").unwrap().contains("Be careful"));
}
```

Benefits over snapshots:
- Resilient to formatting/whitespace changes
- Tests document what we care about
- Meaningful failure messages
- Can add new attributes without breaking unrelated tests

### What's OUT of MVP Scope

- Code execution (Jupyter, Knitr)
- SASS compilation (use pre-compiled CSS from `resources/basic-quarto-website-compiled/`)
- Navigation (navbar, sidebar, footer)
- Website/book projects (multi-file)
- Cross-references
- Citations/bibliography
- Non-HTML formats (PDF, Word, etc.)
- Search index
- Lua filter execution

### Session Decisions (2025-12-21)

These decisions were confirmed during implementation planning:

1. **Custom variants**: Add `Block::Custom(CustomNode)` and `Inline::Custom(CustomNode)` as Quarto extensions to the Pandoc AST. Desugar to wrapper Divs/Spans for JSON serialization.

2. **Implementation location**: All render pipeline code in `crates/quarto/src/` for now; extract to `quarto-core` when patterns emerge.

3. **Unified artifact system**: Single `ArtifactStore` concept (not separate "DependencyCollector"). Must support all quarto-cli dependency use cases.

4. **SASS for MVP**: Skip SASS compilation. Use pre-compiled CSS from `resources/basic-quarto-website-compiled/_site/site_libs/bootstrap/`.

5. **EJS/Navigation for MVP**: Skip navbar/sidebar/footer. No templating engine needed initially.

6. **Thread safety**: Fixed `SourceInfo` to use `Arc` instead of `Rc`, making `Template` naturally `Send + Sync`. Removed unsafe `SendSyncTemplate` wrapper from `pico-quarto-render`.

7. **LinkedHashMap**: Use existing `hashlink::LinkedHashMap` for ordered maps (already a workspace dependency), not `indexmap`.

---

## References

- [Render Pipeline Analysis](../render-pipeline/single-document/README.md)
- [Lua Filter Infrastructure Porting](./2025-12-20-lua-filter-infrastructure-porting.md) - Custom node system design (Issue: k-thpl)
- [Project Context (TypeScript)](../../external-sources/quarto-cli/src/project/types.ts)
- [Dependency Types (TypeScript)](../../external-sources/quarto-cli/src/config/types.ts)
- [Dependency Injection (TypeScript)](../../external-sources/quarto-cli/src/command/render/pandoc-dependencies-html.ts)
- [SASS Compilation (TypeScript)](../../external-sources/quarto-cli/src/core/sass.ts)
- [Lua Filters](../../external-sources/quarto-cli/src/resources/filters/)
- [Lua AST Infrastructure](../../external-sources/quarto-cli/src/resources/filters/ast/)
- [Pandoc datadir](../../external-sources/quarto-cli/src/resources/pandoc/datadir/)
- [pico-quarto-render (Working Prototype)](../../crates/pico-quarto-render/)
- [Explicit Workflow Design](../explicit-workflow-design.md)
