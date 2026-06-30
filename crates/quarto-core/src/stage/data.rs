/*
 * stage/data.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline data types that flow between stages.
 */

//! Data types for the render pipeline.
//!
//! All data types that flow through the pipeline are variants of [`PipelineData`].
//! Each variant represents a different stage in the document processing lifecycle:
//!
//! 1. [`LoadedSource`] - Raw file content with detected source type
//! 2. [`DocumentSource`] - Markdown content with metadata (after conversion)
//! 3. [`DocumentAst`] - Parsed Pandoc AST ready for transformation
//! 4. [`DocumentAtProfile`] - AST plus static snapshot at the profile checkpoint
//! 5. [`ExecutedDocument`] - Result after engine execution (Jupyter, Knitr)
//! 6. [`RenderedOutput`] - Rendered output (HTML, LaTeX) before relocation
//! 7. [`FinalOutput`] - Final output after relocation (for SSG slug support)

use std::path::PathBuf;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceContext;

use crate::document_profile::{DocumentProfile, IncludeEntry};
use crate::format::Format;

/// Type tag for pipeline data variants.
///
/// Used for runtime validation of stage composition without
/// matching on the full data enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineDataKind {
    /// Raw loaded source file
    LoadedSource,
    /// Markdown content with metadata
    DocumentSource,
    /// Parsed Pandoc AST
    DocumentAst,
    /// Pipeline paused at the profile checkpoint: AST plus extracted
    /// [`DocumentProfile`]. See `claude-notes/designs/document-profile-contract.md`.
    AtProfile,
    /// Result after engine execution
    ExecutedDocument,
    /// Rendered output before relocation
    RenderedOutput,
    /// Final output after relocation
    FinalOutput,
}

impl std::fmt::Display for PipelineDataKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineDataKind::LoadedSource => write!(f, "LoadedSource"),
            PipelineDataKind::DocumentSource => write!(f, "DocumentSource"),
            PipelineDataKind::DocumentAst => write!(f, "DocumentAst"),
            PipelineDataKind::AtProfile => write!(f, "AtProfile"),
            PipelineDataKind::ExecutedDocument => write!(f, "ExecutedDocument"),
            PipelineDataKind::RenderedOutput => write!(f, "RenderedOutput"),
            PipelineDataKind::FinalOutput => write!(f, "FinalOutput"),
        }
    }
}

/// All possible data types flowing through the pipeline.
///
/// This enum represents the different states a document passes through
/// during rendering. Each variant holds the relevant data for that stage.
#[derive(Debug)]
pub enum PipelineData {
    /// Loaded source with detected type (entry point after loading)
    LoadedSource(LoadedSource),

    /// Markdown content with metadata (after notebook conversion, etc.)
    DocumentSource(DocumentSource),

    /// Parsed AST ready for transformation
    DocumentAst(DocumentAst),

    /// Pipeline paused at the profile checkpoint. Carries the
    /// extracted [`DocumentProfile`] plus the `DocumentAst` from
    /// which subsequent stages resume. See the profile contract
    /// at `claude-notes/designs/document-profile-contract.md`.
    AtProfile(DocumentAtProfile),

    /// Result after engine execution (future: Jupyter, Knitr)
    ExecutedDocument(ExecutedDocument),

    /// Rendered output (HTML, LaTeX, etc.) before relocation
    RenderedOutput(RenderedOutput),

    /// Final output after relocation (for SSG slug support)
    FinalOutput(FinalOutput),
}

impl PipelineData {
    /// Get the kind of this data without matching on contents.
    pub fn kind(&self) -> PipelineDataKind {
        match self {
            Self::LoadedSource(_) => PipelineDataKind::LoadedSource,
            Self::DocumentSource(_) => PipelineDataKind::DocumentSource,
            Self::DocumentAst(_) => PipelineDataKind::DocumentAst,
            Self::AtProfile(_) => PipelineDataKind::AtProfile,
            Self::ExecutedDocument(_) => PipelineDataKind::ExecutedDocument,
            Self::RenderedOutput(_) => PipelineDataKind::RenderedOutput,
            Self::FinalOutput(_) => PipelineDataKind::FinalOutput,
        }
    }

    /// Try to extract LoadedSource from this data.
    pub fn into_loaded_source(self) -> Option<LoadedSource> {
        match self {
            Self::LoadedSource(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract DocumentSource from this data.
    pub fn into_document_source(self) -> Option<DocumentSource> {
        match self {
            Self::DocumentSource(s) => Some(s),
            _ => None,
        }
    }

    /// Try to extract DocumentAst from this data.
    pub fn into_document_ast(self) -> Option<DocumentAst> {
        match self {
            Self::DocumentAst(a) => Some(a),
            _ => None,
        }
    }

    /// Try to extract DocumentAtProfile from this data.
    pub fn into_at_profile(self) -> Option<DocumentAtProfile> {
        match self {
            Self::AtProfile(p) => Some(p),
            _ => None,
        }
    }

    /// Try to extract ExecutedDocument from this data.
    pub fn into_executed_document(self) -> Option<ExecutedDocument> {
        match self {
            Self::ExecutedDocument(e) => Some(e),
            _ => None,
        }
    }

    /// Try to extract RenderedOutput from this data.
    pub fn into_rendered_output(self) -> Option<RenderedOutput> {
        match self {
            Self::RenderedOutput(r) => Some(r),
            _ => None,
        }
    }

    /// Try to extract FinalOutput from this data.
    pub fn into_final_output(self) -> Option<FinalOutput> {
        match self {
            Self::FinalOutput(f) => Some(f),
            _ => None,
        }
    }
}

/// Source file type detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    /// Quarto Markdown (.qmd)
    Qmd,
    /// Plain Markdown (.md)
    Markdown,
    /// Jupyter Notebook (.ipynb)
    Ipynb,
    /// R Markdown (.Rmd)
    Rmd,
}

impl SourceType {
    /// Detect source type from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "qmd" => Some(Self::Qmd),
            "md" | "markdown" => Some(Self::Markdown),
            "ipynb" => Some(Self::Ipynb),
            "rmd" => Some(Self::Rmd),
            _ => None,
        }
    }

    /// Detect source type from file path.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }
}

/// Provenance of a file-to-QMD conversion performed by `EngineClaimsFileStage`.
///
/// v1 / C′ scope: carries only the converting engine's name, which is used to
/// build the synthetic intermediate-file label
/// `"<{original} (converted by {engine})>"` consumed by `ParseDocumentStage`.
///
/// A′ (deferred): faithful byte-range back-mapping would add `original_content`
/// and a `source_info` (converted→original map) here. Out of scope for this
/// plan; see plan1c §1060-1063.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionProvenance {
    /// Name of the engine that produced the converted QMD.
    pub engine: String,
}

/// Loaded file with detected source type.
///
/// This is the entry point for the pipeline - a file has been read
/// from disk (or VFS in WASM) and its type has been detected.
///
/// After `EngineClaimsFileStage` runs, `content` may have been replaced with
/// the QMD bytes produced by the claiming engine, and `conversion` records
/// which engine performed the conversion. `path` always stays as the user's
/// original input path.
#[derive(Debug)]
pub struct LoadedSource {
    /// Path to the source file (user's original path; never rewritten)
    pub path: PathBuf,
    /// File content — QMD bytes after engine conversion, raw bytes otherwise
    pub content: Vec<u8>,
    /// Detected (or post-conversion) source type
    pub source_type: SourceType,
    /// Set by `EngineClaimsFileStage` when an engine converts this file to QMD.
    /// `None` for `.qmd` / `.md` inputs (pass-through path).
    pub conversion: Option<ConversionProvenance>,
}

impl LoadedSource {
    /// Create a new LoadedSource with auto-detected type.
    pub fn new(path: PathBuf, content: Vec<u8>) -> Self {
        let source_type = SourceType::from_path(&path).unwrap_or(SourceType::Markdown);
        Self {
            path,
            content,
            source_type,
            conversion: None,
        }
    }

    /// Create a LoadedSource with explicit type.
    pub fn with_type(path: PathBuf, content: Vec<u8>, source_type: SourceType) -> Self {
        Self {
            path,
            content,
            source_type,
            conversion: None,
        }
    }

    /// Get content as a UTF-8 string (lossy conversion).
    pub fn content_string(&self) -> String {
        String::from_utf8_lossy(&self.content).into_owned()
    }
}

/// Pandoc includes for engine execution results.
///
/// These are additional content blocks that get injected into
/// the document at specific locations during Pandoc processing.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PandocIncludes {
    /// Content to include in document header
    pub header_includes: Vec<String>,
    /// Content to include before document body
    pub include_before: Vec<String>,
    /// Content to include after document body
    pub include_after: Vec<String>,
}

/// Markdown content with metadata (after any format conversion).
///
/// At this stage, the source has been converted to markdown
/// (e.g., .ipynb → .qmd) and the YAML front matter has been parsed.
#[derive(Debug)]
pub struct DocumentSource {
    /// Path to the original source file
    pub path: PathBuf,
    /// Markdown content (may have source mapping for error reporting)
    pub markdown: String,
    /// Parsed metadata from YAML front matter
    pub metadata: ConfigValue,
    /// Original source path before conversion (same as path if no conversion)
    pub original_source: PathBuf,
    /// Source context for error reporting
    pub source_context: SourceContext,
}

impl DocumentSource {
    /// Create a new DocumentSource from markdown content.
    pub fn new(path: PathBuf, markdown: String, metadata: ConfigValue) -> Self {
        let source_context = SourceContext::new();
        let original_source = path.clone();
        Self {
            path,
            markdown,
            metadata,
            original_source,
            source_context,
        }
    }

    /// Set the original source path (for converted documents).
    pub fn with_original_source(mut self, path: PathBuf) -> Self {
        self.original_source = path;
        self
    }

    /// Set the source context for error reporting.
    pub fn with_source_context(mut self, ctx: SourceContext) -> Self {
        self.source_context = ctx;
        self
    }
}

/// Parsed Pandoc AST.
///
/// The document has been parsed into a Pandoc AST and is ready
/// for transformation (callouts, cross-references, etc.).
///
/// Note: `ast_context` is mutable throughout the pipeline because
/// AST transforms may create new objects that need source info tracking.
///
/// `Clone` is implemented so the pipeline can take a snapshot at the
/// profile checkpoint (see [`DocumentAtProfile`]) and resume from it
/// later. All inner fields are already `Clone`.
#[derive(Debug, Clone, Default)]
pub struct DocumentAst {
    /// Path to the source file
    pub path: PathBuf,
    /// The Pandoc AST
    pub ast: Pandoc,
    /// AST context for source location tracking
    pub ast_context: pampa::pandoc::ASTContext,
    /// Source context for error reporting
    pub source_context: SourceContext,
    /// Warnings collected during parsing
    pub warnings: Vec<DiagnosticMessage>,
    /// Side-channel populated by `IncludeExpansionStage` for every
    /// file whose contents were spliced into this AST via
    /// `{{< include child.qmd >}}`. Each entry records the
    /// resolved path and a SHA-256 of the file's bytes at splice
    /// time. Drained by `DocumentProfileStage` into
    /// [`DocumentProfile::includes`] for Phase-8 cache-key
    /// invalidation (`bd-r82e`).
    ///
    /// Default: empty (no includes seen). Order: first-occurrence in
    /// expansion order; cycles already truncated by
    /// `IncludeExpansionStage`.
    pub recorded_includes: Vec<IncludeEntry>,
}

/// Pipeline state at the profile checkpoint.
///
/// Holds the extracted [`DocumentProfile`] plus the [`DocumentAst`]
/// from which subsequent stages resume. This variant exists purely to
/// expose the profile to project-level orchestration while keeping the
/// per-file pipeline compositional.
///
/// The Phase-0 pipeline wraps at [`DocumentProfileStage`] and unwraps
/// immediately afterward at [`UnwrapProfileStage`] so downstream stages
/// keep their existing `DocumentAst` signatures. Phase 1's two-pass
/// orchestration will consume the `AtProfile` variant directly to
/// build the project-wide [`ProjectIndex`].
///
/// [`DocumentProfileStage`]: crate::stage::stages::DocumentProfileStage
/// [`UnwrapProfileStage`]: crate::stage::stages::UnwrapProfileStage
#[derive(Debug, Clone)]
pub struct DocumentAtProfile {
    /// Static snapshot extracted at the checkpoint.
    pub profile: DocumentProfile,
    /// The AST the checkpoint was extracted from. Subsequent stages
    /// read this and continue rendering.
    pub ast: DocumentAst,
}

/// Result of engine execution (future: Jupyter, Knitr).
///
/// After executing code cells, this contains the resulting
/// markdown with cell outputs and any supporting files.
#[derive(Debug)]
pub struct ExecutedDocument {
    /// Path to the source file
    pub path: PathBuf,
    /// Markdown content with execution results
    pub markdown: String,
    /// Files created during execution (images, data files, etc.)
    pub supporting_files: Vec<PathBuf>,
    /// Pandoc filters to apply
    pub filters: Vec<String>,
    /// Content to inject at specific locations
    pub includes: PandocIncludes,
    /// Source context for error reporting
    pub source_context: SourceContext,
}

/// Rendered output before relocation/postprocessing.
///
/// The document has been rendered to its output format (HTML, LaTeX, etc.)
/// but has not yet been moved to its final location.
#[derive(Debug)]
pub struct RenderedOutput {
    /// Path to the input source file
    pub input_path: PathBuf,
    /// Path to the rendered output file
    pub output_path: PathBuf,
    /// Output format
    pub format: Format,
    /// Rendered content
    pub content: String,
    /// True for intermediate formats (e.g., LaTeX before PDF compilation)
    pub is_intermediate: bool,
    /// Supporting files (CSS, images, etc.)
    pub supporting_files: Vec<PathBuf>,
    /// Document metadata from the AST (for template rendering)
    pub metadata: ConfigValue,
    /// Source context for ariadne attribution of cross-file diagnostics.
    ///
    /// Carried forward from the document's `DocumentAst.source_context`
    /// by `RenderHtmlStage`. `ApplyTemplateStage` extends it with
    /// template + partial files via `compile_with_resolver_and_context`
    /// so doctemplate warnings (e.g. `Q-10-2 Undefined variable`)
    /// resolve to a real file slice in the renderer's stderr output.
    /// See bd-xdnk.
    pub source_context: SourceContext,
}

/// Final output after relocation.
///
/// The output file has been moved to its final location
/// (e.g., for SSG slug support in project builds).
#[derive(Debug)]
pub struct FinalOutput {
    /// Path to the input source file
    pub input_path: PathBuf,
    /// Final path to the output file
    pub output_path: PathBuf,
    /// Output format
    pub format: Format,
    /// Supporting files (relocated)
    pub supporting_files: Vec<PathBuf>,
    /// Warnings collected during rendering
    pub warnings: Vec<DiagnosticMessage>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    #[test]
    fn test_source_type_from_extension() {
        assert_eq!(SourceType::from_extension("qmd"), Some(SourceType::Qmd));
        assert_eq!(SourceType::from_extension("QMD"), Some(SourceType::Qmd));
        assert_eq!(SourceType::from_extension("md"), Some(SourceType::Markdown));
        assert_eq!(SourceType::from_extension("ipynb"), Some(SourceType::Ipynb));
        assert_eq!(SourceType::from_extension("Rmd"), Some(SourceType::Rmd));
        assert_eq!(SourceType::from_extension("txt"), None);
    }

    #[test]
    fn test_source_type_from_path() {
        assert_eq!(
            SourceType::from_path(std::path::Path::new("doc.qmd")),
            Some(SourceType::Qmd)
        );
        assert_eq!(
            SourceType::from_path(std::path::Path::new("/path/to/notebook.ipynb")),
            Some(SourceType::Ipynb)
        );
        assert_eq!(SourceType::from_path(std::path::Path::new("README")), None);
    }

    #[test]
    fn test_loaded_source_auto_detect() {
        let source = LoadedSource::new(PathBuf::from("test.qmd"), b"# Hello".to_vec());
        assert_eq!(source.source_type, SourceType::Qmd);
    }

    #[test]
    fn test_loaded_source_content_string() {
        let source = LoadedSource::new(PathBuf::from("test.md"), b"Hello, world!".to_vec());
        assert_eq!(source.content_string(), "Hello, world!");
    }

    #[test]
    fn test_pipeline_data_kind() {
        let loaded =
            PipelineData::LoadedSource(LoadedSource::new(PathBuf::from("test.qmd"), vec![]));
        assert_eq!(loaded.kind(), PipelineDataKind::LoadedSource);

        let doc_source = PipelineData::DocumentSource(DocumentSource::new(
            PathBuf::from("test.qmd"),
            String::new(),
            ConfigValue::null(SourceInfo::for_test()),
        ));
        assert_eq!(doc_source.kind(), PipelineDataKind::DocumentSource);
    }

    #[test]
    fn test_pipeline_data_into_methods() {
        let source = LoadedSource::new(PathBuf::from("test.qmd"), vec![]);
        let data = PipelineData::LoadedSource(source);

        // Correct conversion succeeds
        assert!(data.into_loaded_source().is_some());

        // Wrong conversion fails
        let source2 = LoadedSource::new(PathBuf::from("test.qmd"), vec![]);
        let data2 = PipelineData::LoadedSource(source2);
        assert!(data2.into_document_source().is_none());
    }

    #[test]
    fn test_pipeline_data_kind_display() {
        assert_eq!(PipelineDataKind::LoadedSource.to_string(), "LoadedSource");
        assert_eq!(
            PipelineDataKind::DocumentSource.to_string(),
            "DocumentSource"
        );
        assert_eq!(PipelineDataKind::DocumentAst.to_string(), "DocumentAst");
    }
}
