//! One project (or one lone document) and what has been learned about it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::format_resolution::{ScopeRequest, resolve_document_format};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::session_runtime::{
    EditorLayer, ProjectSource, SessionRuntime, VersionKey, VfsBaseLayer,
};
use quarto_system_runtime::SystemRuntime;

use crate::ClientId;

/// Identifies a session within a [`crate::Workspace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionKind {
    /// A `_quarto.yml`-rooted project on disk.
    Project,
    /// A document with no project config above it. The session is rooted at
    /// the document's directory but never walks it: its base layer is empty
    /// and other files (includes) are read through to disk.
    SingleFile { rel: PathBuf },
    /// A document with no path (`untitled:`). Synthetic root, attached
    /// source: it never touches the disk.
    Untitled { rel: PathBuf },
}

/// What one analysis run of one document produced, minus anything that
/// holds the document's text (no AST, no `SourceContext`; decision D1).
#[derive(Debug, Clone)]
pub struct DocumentResult {
    /// The format the document was analysed for (what `q2 render` picks).
    pub format: String,
    pub symbols: Vec<quarto_lsp_core::Symbol>,
    pub folding_ranges: Vec<quarto_lsp_core::FoldingRange>,
    pub diagnostics: Vec<quarto_lsp_core::Diagnostic>,
    pub profile: Option<quarto_core::document_profile::DocumentProfile>,
    pub authored_meta: Option<quarto_pandoc_types::ConfigValue>,
    pub ast_context: Option<pampa::pandoc::ASTContext>,
}

/// A [`DocumentResult`] with what it depended on.
#[derive(Debug, Clone)]
pub(crate) struct Cached {
    /// Every project file the run looked up, with the version it saw.
    pub reads: BTreeMap<PathBuf, VersionKey>,
    /// `Some(client)` when the run read one of that client's buffers; such a
    /// result is only ever served to that client. `None`: shareable.
    pub owner: Option<ClientId>,
    /// The session's structure counter when the run started (files added or
    /// removed since then invalidate it: listings are not in the read log).
    pub structure: u64,
    pub result: Arc<DocumentResult>,
}

/// A cached `ProjectContext`, validated like a result.
#[derive(Debug, Clone)]
pub(crate) struct CachedProject {
    pub reads: BTreeMap<PathBuf, VersionKey>,
    pub owner: Option<ClientId>,
    pub structure: u64,
    pub context: ProjectContext,
}

pub(crate) struct Session {
    pub id: SessionId,
    pub kind: SessionKind,
    pub root: PathBuf,
    pub source: ProjectSource,
    pub base: Arc<VfsBaseLayer>,
    /// Bumped when a feeder reports a file added or removed.
    pub structure: u64,
    pub projects: Vec<CachedProject>,
    pub results: HashMap<PathBuf, Vec<Cached>>,
}

/// Everything an analysis run needs, owned, so it can run on another
/// thread as a [`crate::Job`].
#[derive(Clone)]
pub(crate) struct RunInputs {
    pub root: PathBuf,
    pub source: ProjectSource,
    pub base: Arc<VfsBaseLayer>,
    pub editor: Arc<EditorLayer>,
    pub inner: Arc<dyn SystemRuntime>,
    pub profiles: Option<Vec<String>>,
    pub kind: SessionKind,
}

impl RunInputs {
    pub fn runtime(&self) -> Arc<SessionRuntime> {
        Arc::new(SessionRuntime::new(
            &self.root,
            self.source,
            self.editor.clone(),
            self.base.clone(),
            self.inner.clone(),
        ))
    }

    /// The project context, as seen through `rt` (whose read log then
    /// includes the config files discovery read).
    pub fn discover(&self, rt: &SessionRuntime, doc: &Path) -> Result<ProjectContext, String> {
        match &self.kind {
            SessionKind::Untitled { .. } => Ok(lone_document_project(&self.root, doc)),
            SessionKind::Project => {
                ProjectContext::discover_with_profile(&self.root, rt, self.profiles.as_deref())
                    .map_err(|e| e.to_string())
            }
            SessionKind::SingleFile { .. } => {
                ProjectContext::discover_with_profile(doc, rt, self.profiles.as_deref())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

/// A single-file project for a document that exists nowhere on disk.
fn lone_document_project(root: &Path, doc: &Path) -> ProjectContext {
    ProjectContext {
        dir: root.to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(doc)],
        output_dir: root.to_path_buf(),
        ..Default::default()
    }
}

/// Analyse the document at `rel`. `project` is reused when the caller has a
/// current one; otherwise it is discovered through the same runtime, so its
/// reads land in the same log.
pub(crate) fn analyse(
    inputs: &RunInputs,
    rel: &Path,
    project: Option<&ProjectContext>,
) -> (BTreeMap<PathBuf, VersionKey>, DocumentResult) {
    let rt = inputs.runtime();
    let doc = inputs.root.join(rel);
    let text = rt.file_read_string(&doc).unwrap_or_default();

    let discovered;
    let project = match project {
        Some(p) => Ok(p),
        None => {
            discovered = inputs.discover(&rt, &doc);
            discovered.as_ref().map_err(Clone::clone)
        }
    };
    let result = match project {
        Ok(project) => {
            let format_key = resolve_document_format(
                project,
                &doc,
                Some(&text),
                &ScopeRequest::chapter(),
                None,
                rt.as_ref(),
            )
            .key_or("html")
            .to_string();
            let format = Format::from_format_string(&format_key)
                .unwrap_or_else(|_| Format::from_format_string("html").unwrap_or_default());
            let analysis = pollster::block_on(quarto_lsp_core::analyze_in_project(
                rt.clone(),
                project,
                &format,
                &DocumentInfo::from_path(&doc),
                &text,
            ));
            DocumentResult {
                format: format_key,
                symbols: analysis.analysis.symbols,
                folding_ranges: analysis.analysis.folding_ranges,
                diagnostics: analysis.analysis.diagnostics,
                profile: analysis.profile,
                authored_meta: analysis.authored_meta,
                ast_context: analysis.ast_context,
            }
        }
        Err(message) => DocumentResult {
            format: "html".to_string(),
            symbols: Vec::new(),
            folding_ranges: Vec::new(),
            diagnostics: vec![quarto_lsp_core::Diagnostic::new(
                Default::default(),
                quarto_lsp_core::DiagnosticSeverity::Error,
                format!("project discovery failed: {message}"),
            )],
            profile: None,
            authored_meta: None,
            ast_context: None,
        },
    };
    (rt.reads(), result)
}

/// The owner a result or project computed through `editor` gets: shareable
/// unless it read one of the client's buffers.
pub(crate) fn owner_of(
    reads: &BTreeMap<PathBuf, VersionKey>,
    client: ClientId,
) -> Option<ClientId> {
    reads
        .values()
        .any(|k| matches!(k, VersionKey::Buffer(_)))
        .then_some(client)
}

/// Whether every read in `reads` still matches over `rt`.
pub(crate) fn current(rt: &SessionRuntime, reads: &BTreeMap<PathBuf, VersionKey>) -> bool {
    reads.iter().all(|(rel, key)| rt.still_matches(rel, key))
}
