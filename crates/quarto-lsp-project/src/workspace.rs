//! The [`Workspace`]: every session, every client's open documents, and the
//! sans-IO state machine a host drives.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use quarto_core::project::ProjectContext;
use quarto_core::session_runtime::{EditorLayer, ProjectSource, VfsBaseLayer};
use quarto_system_runtime::SystemRuntime;

use crate::session::{
    Cached, CachedProject, DocumentResult, RunInputs, Session, SessionId, SessionKind, analyse,
    current, owner_of,
};
use crate::uri::DocUri;

/// Identifies a client connection (an editor, a hub host). v1 hosts have
/// exactly one; nothing assumes it.
pub type ClientId = u32;

/// Loads a disk session's base layer: the host's one piece of bulk I/O.
pub type BaseLoader = Arc<dyn Fn(&Path) -> std::io::Result<VfsBaseLayer> + Send + Sync>;

/// How a [`Workspace`] is set up.
pub struct WorkspaceConfig {
    /// Everything that is not a project file goes here (and, in disk mode,
    /// project files no layer holds).
    pub inner: Arc<dyn SystemRuntime>,
    /// Active project profiles (`initializationOptions.profile`); `None`
    /// falls back to `QUARTO_PROFILE` and the config defaults.
    pub profiles: Option<Vec<String>>,
    /// Profile every document of a newly opened project in the background
    /// (decision R5). On by default.
    pub sweep: bool,
    pub load_base: BaseLoader,
}

impl WorkspaceConfig {
    pub fn new(inner: Arc<dyn SystemRuntime>) -> Self {
        Self {
            inner,
            profiles: None,
            sweep: true,
            load_base: default_base_loader(),
        }
    }
}

fn default_base_loader() -> BaseLoader {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Arc::new(|root: &Path| VfsBaseLayer::load_from_disk(root))
    }
    #[cfg(target_arch = "wasm32")]
    {
        Arc::new(|_root: &Path| Ok(VfsBaseLayer::new()))
    }
}

/// Something a client did.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    Opened {
        uri: DocUri,
        text: String,
        version: i32,
    },
    /// Full-text sync: `text` is the whole new buffer.
    Changed {
        uri: DocUri,
        text: String,
        version: i32,
    },
    Closed {
        uri: DocUri,
    },
}

/// A file a feeder saw change, by absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// Created or modified.
    Changed(PathBuf),
    Removed(PathBuf),
}

/// What a host feeds the [`Workspace`].
pub enum Input {
    Client(ClientId, ClientEvent),
    Files(Vec<FileChange>),
    JobDone(JobOutput),
}

/// What the [`Workspace`] asks its host to do.
pub enum Output {
    /// Push these diagnostics for `uri` to `client` (replacing earlier ones).
    Diagnostics {
        client: ClientId,
        uri: DocUri,
        version: Option<i32>,
        diagnostics: Vec<quarto_lsp_core::Diagnostic>,
    },
    /// Report file changes under `root` as [`Input::Files`].
    Watch { session: SessionId, root: PathBuf },
    /// Run `job.run()` anywhere (any thread) and feed the output back as
    /// [`Input::JobDone`].
    Spawn(Job),
}

/// A background analysis: a `Send` bundle of owned inputs.
pub struct Job {
    session: SessionId,
    rel: PathBuf,
    structure: u64,
    inputs: RunInputs,
    project: ProjectContext,
}

impl Job {
    pub fn run(self) -> JobOutput {
        let (reads, result) = analyse(&self.inputs, &self.rel, Some(&self.project));
        JobOutput {
            session: self.session,
            rel: self.rel,
            structure: self.structure,
            reads,
            result,
        }
    }
}

/// The result of a [`Job`].
pub struct JobOutput {
    session: SessionId,
    rel: PathBuf,
    structure: u64,
    reads: BTreeMap<PathBuf, quarto_core::session_runtime::VersionKey>,
    result: DocumentResult,
}

struct OpenDoc {
    session: SessionId,
    rel: PathBuf,
    version: i32,
    text: Arc<[u8]>,
    /// Something it may depend on changed; re-validate on the next flush
    /// (which skips buffers that are not documents).
    stale: bool,
    published: Option<Vec<quarto_lsp_core::Diagnostic>>,
}

#[derive(Default)]
struct Client {
    docs: BTreeMap<DocUri, OpenDoc>,
}

/// Every session and every client's open documents. See the crate docs.
pub struct Workspace {
    config: WorkspaceConfig,
    sessions: BTreeMap<SessionId, Session>,
    next_session: u32,
    clients: HashMap<ClientId, Client>,
}

impl Workspace {
    pub fn new(config: WorkspaceConfig) -> Self {
        Self {
            config,
            sessions: BTreeMap::new(),
            next_session: 0,
            clients: HashMap::new(),
        }
    }

    // ── Inputs ──────────────────────────────────────────────────────────

    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Client(client, event) => self.client_event(client, event),
            Input::Files(changes) => {
                self.files_changed(&changes);
                Vec::new()
            }
            Input::JobDone(output) => {
                self.job_done(output);
                Vec::new()
            }
        }
    }

    fn client_event(&mut self, client: ClientId, event: ClientEvent) -> Vec<Output> {
        let mut out = Vec::new();
        match event {
            ClientEvent::Opened { uri, text, version } => {
                let Some((session, rel)) = self.resolve(client, &uri, Some(&text), &mut out) else {
                    return out;
                };
                self.clients.entry(client).or_default().docs.insert(
                    uri,
                    OpenDoc {
                        session,
                        rel,
                        version,
                        text: Arc::from(text.into_bytes()),
                        stale: true,
                        published: None,
                    },
                );
                self.mark_stale(session);
            }
            ClientEvent::Changed { uri, text, version } => {
                let Some(doc) = self
                    .clients
                    .get_mut(&client)
                    .and_then(|c| c.docs.get_mut(&uri))
                else {
                    return out;
                };
                doc.text = Arc::from(text.into_bytes());
                doc.version = version;
                let session = doc.session;
                self.mark_stale(session);
            }
            ClientEvent::Closed { uri } => {
                let Some(doc) = self
                    .clients
                    .get_mut(&client)
                    .and_then(|c| c.docs.remove(&uri))
                else {
                    return out;
                };
                out.push(Output::Diagnostics {
                    client,
                    uri,
                    version: None,
                    diagnostics: Vec::new(),
                });
                self.mark_stale(doc.session);
                self.drop_if_unused(doc.session);
            }
        }
        out
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn files_changed(&mut self, changes: &[FileChange]) {
        let inner = self.config.inner.clone();
        let mut touched = Vec::new();
        for change in changes {
            let (FileChange::Changed(path) | FileChange::Removed(path)) = change;
            let path = normalize(path);
            for session in self.sessions.values_mut() {
                let Ok(rel) = path.strip_prefix(&session.root) else {
                    continue;
                };
                touched.push(session.id);
                if session.kind != SessionKind::Project {
                    // Single-file sessions hold nothing in their base: disk
                    // reads are validated by re-hashing.
                    continue;
                }
                let rel = rel.to_path_buf();
                let had =
                    quarto_core::session_runtime::BaseLayer::entry(session.base.as_ref(), &rel)
                        .is_some();
                let base = Arc::make_mut(&mut session.base);
                match change {
                    FileChange::Removed(_) => {
                        if base.remove(&rel) {
                            session.structure += 1;
                        }
                    }
                    FileChange::Changed(_) => match quarto_project_files::classify(&rel) {
                        Some(class) if class.is_text() => {
                            if let Ok(bytes) = inner.file_read(&path) {
                                base.insert_file(rel.clone(), bytes);
                                if !had {
                                    session.structure += 1;
                                }
                            }
                        }
                        Some(_) => {
                            if let Ok(meta) = inner.path_metadata(&path) {
                                base.insert_on_demand(
                                    rel.clone(),
                                    quarto_core::session_runtime::OnDemandFile {
                                        source: path.clone(),
                                        size: meta.size,
                                        modified: meta.modified,
                                    },
                                );
                                if !had {
                                    session.structure += 1;
                                }
                            }
                        }
                        None => {}
                    },
                }
            }
        }
        for session in touched {
            self.mark_stale(session);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn files_changed(&mut self, _changes: &[FileChange]) {}

    fn job_done(&mut self, output: JobOutput) {
        let Some(session) = self.sessions.get_mut(&output.session) else {
            return;
        };
        if output.structure != session.structure {
            return;
        }
        let entries = session.results.entry(output.rel).or_default();
        entries.retain(|c| c.owner.is_some());
        entries.push(Cached {
            reads: output.reads,
            owner: None,
            structure: output.structure,
            result: Arc::new(output.result),
        });
    }

    /// Re-validate every stale open document and publish the diagnostics of
    /// the ones whose results changed. Hosts call this when their input
    /// queue is empty, so a burst of edits is analysed once.
    pub fn flush(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        let stale: Vec<(ClientId, DocUri)> = self
            .clients
            .iter()
            .flat_map(|(client, c)| {
                c.docs
                    .iter()
                    .filter(|(uri, d)| d.stale && is_document(uri, &d.rel))
                    .map(|(uri, _)| (*client, uri.clone()))
            })
            .collect();
        for (client, uri) in stale {
            let Some((session, rel, version)) = self
                .clients
                .get(&client)
                .and_then(|c| c.docs.get(&uri))
                .map(|d| (d.session, d.rel.clone(), d.version))
            else {
                continue;
            };
            let result = self.result_for(client, session, &rel);
            let Some(doc) = self
                .clients
                .get_mut(&client)
                .and_then(|c| c.docs.get_mut(&uri))
            else {
                continue;
            };
            doc.stale = false;
            if doc.published.as_ref() != Some(&result.diagnostics) {
                doc.published = Some(result.diagnostics.clone());
                out.push(Output::Diagnostics {
                    client,
                    uri,
                    version: Some(version),
                    diagnostics: result.diagnostics.clone(),
                });
            }
        }
        out
    }

    // ── Queries ─────────────────────────────────────────────────────────

    /// The current analysis of `uri` as `client` sees it (its own unsaved
    /// buffers included). A document that is not open is read through the
    /// session's layers; `None` for a document with no path that is not
    /// open.
    ///
    /// Only documents are analysed (`.qmd`, `.md`, and pathless buffers):
    /// an open `_quarto.yml` or `_metadata.yml` is a buffer that shadows the
    /// disk, never a document, so it gets `None` here and no diagnostics.
    pub fn document(&mut self, client: ClientId, uri: &DocUri) -> Option<Arc<DocumentResult>> {
        let (session, rel) = match self.clients.get(&client).and_then(|c| c.docs.get(uri)) {
            Some(doc) => (doc.session, doc.rel.clone()),
            None => {
                let mut ignored = Vec::new();
                self.resolve(client, uri, None, &mut ignored)?
            }
        };
        if !is_document(uri, &rel) {
            return None;
        }
        Some(self.result_for(client, session, &rel))
    }

    /// Outline symbols for `uri`.
    pub fn document_symbols(
        &mut self,
        client: ClientId,
        uri: &DocUri,
    ) -> Option<Vec<quarto_lsp_core::Symbol>> {
        self.document(client, uri).map(|r| r.symbols.clone())
    }

    // ── Sessions ────────────────────────────────────────────────────────

    fn editor_for(&self, client: ClientId, session: SessionId) -> EditorLayer {
        let mut editor = EditorLayer::new();
        if let Some(c) = self.clients.get(&client) {
            for doc in c.docs.values().filter(|d| d.session == session) {
                editor.set(doc.rel.clone(), doc.text.clone(), doc.version);
            }
        }
        editor
    }

    fn run_inputs(&self, client: Option<ClientId>, session: &Session) -> RunInputs {
        RunInputs {
            root: session.root.clone(),
            source: session.source,
            base: session.base.clone(),
            editor: Arc::new(
                client
                    .map(|c| self.editor_for(c, session.id))
                    .unwrap_or_default(),
            ),
            inner: self.config.inner.clone(),
            profiles: self.config.profiles.clone(),
            kind: session.kind.clone(),
        }
    }

    fn result_for(&mut self, client: ClientId, sid: SessionId, rel: &Path) -> Arc<DocumentResult> {
        let Some(session) = self.sessions.get(&sid) else {
            return Arc::new(empty_result());
        };
        let inputs = self.run_inputs(Some(client), session);
        let rt = inputs.runtime();
        let structure = session.structure;
        if let Some(hit) = session.results.get(rel).and_then(|entries| {
            entries.iter().find(|c| {
                (c.owner.is_none() || c.owner == Some(client))
                    && c.structure == structure
                    && current(&rt, &c.reads)
            })
        }) {
            return hit.result.clone();
        }

        let project = session.projects.iter().find(|p| {
            (p.owner.is_none() || p.owner == Some(client))
                && p.structure == structure
                && current(&rt, &p.reads)
        });
        let (project, project_reads) = match project {
            Some(p) => (p.context.clone(), p.reads.clone()),
            None => {
                let rt = inputs.runtime();
                let doc = inputs.root.join(rel);
                match inputs.discover(&rt, &doc) {
                    Ok(context) => {
                        let reads = rt.reads();
                        let owner = owner_of(&reads, client);
                        let session = self.sessions.get_mut(&sid).expect("session");
                        session.projects.retain(|p| p.owner != owner);
                        session.projects.push(CachedProject {
                            reads: reads.clone(),
                            owner,
                            structure,
                            context: context.clone(),
                        });
                        (context, reads)
                    }
                    Err(_) => {
                        // `analyse` reports the failure as a diagnostic.
                        let (reads, result) = analyse(&inputs, rel, None);
                        return self.store(sid, client, rel, structure, reads, result);
                    }
                }
            }
        };
        let (mut reads, result) = analyse(&inputs, rel, Some(&project));
        for (path, key) in project_reads {
            reads.entry(path).or_insert(key);
        }
        self.store(sid, client, rel, structure, reads, result)
    }

    fn store(
        &mut self,
        sid: SessionId,
        client: ClientId,
        rel: &Path,
        structure: u64,
        reads: BTreeMap<PathBuf, quarto_core::session_runtime::VersionKey>,
        result: DocumentResult,
    ) -> Arc<DocumentResult> {
        let result = Arc::new(result);
        let owner = owner_of(&reads, client);
        if let Some(session) = self.sessions.get_mut(&sid) {
            let entries = session.results.entry(rel.to_path_buf()).or_default();
            entries.retain(|c| c.owner != owner);
            entries.push(Cached {
                reads,
                owner,
                structure,
                result: result.clone(),
            });
        }
        result
    }

    /// Find or create the session for `uri`, returning it and the document's
    /// project-relative path. `text` is the buffer being opened, if any: it
    /// takes part in finding the project root like the client's other
    /// buffers.
    fn resolve(
        &mut self,
        client: ClientId,
        uri: &DocUri,
        text: Option<&str>,
        out: &mut Vec<Output>,
    ) -> Option<(SessionId, PathBuf)> {
        let Some(path) = uri.to_file_path() else {
            text?; // a pathless document exists only while open
            return Some(self.untitled_session(uri));
        };
        let inner = self.config.inner.clone();
        let path = canonical(inner.as_ref(), &path);
        let dir = path.parent()?.to_path_buf();

        // The client's buffers over the disk, so an unsaved `_quarto.yml`
        // makes a project.
        let mut buffers = HashMap::new();
        if let Some(c) = self.clients.get(&client) {
            for (doc_uri, doc) in &c.docs {
                if let Some(p) = doc_uri.to_file_path() {
                    buffers.insert(canonical(inner.as_ref(), &p), doc.text.to_vec());
                }
            }
        }
        if let Some(text) = text {
            buffers.insert(path.clone(), text.as_bytes().to_vec());
        }
        let overlay = quarto_core::overlay_runtime::OverlayRuntime::new(inner.clone(), buffers);
        let found = quarto_core::project::discovery::find_project_root(&dir, &overlay)
            .ok()
            .flatten();

        match found {
            Some((root, _config)) => {
                let root = canonical(inner.as_ref(), &root);
                let rel = path.strip_prefix(&root).ok()?.to_path_buf();
                let existing = self
                    .sessions
                    .values()
                    .find(|s| s.kind == SessionKind::Project && s.root == root)
                    .map(|s| s.id);
                let sid = match existing {
                    Some(sid) => sid,
                    None => self.create_project_session(root, out),
                };
                Some((sid, rel))
            }
            None => {
                let rel = PathBuf::from(path.file_name()?);
                let existing = self
                    .sessions
                    .values()
                    .find(|s| {
                        s.root == dir && s.kind == SessionKind::SingleFile { rel: rel.clone() }
                    })
                    .map(|s| s.id);
                let sid = existing.unwrap_or_else(|| {
                    self.insert_session(
                        SessionKind::SingleFile { rel: rel.clone() },
                        dir,
                        ProjectSource::Disk,
                        VfsBaseLayer::new(),
                    )
                });
                Some((sid, rel))
            }
        }
    }

    fn untitled_session(&mut self, uri: &DocUri) -> (SessionId, PathBuf) {
        let mut name = uri.display_name();
        if Path::new(&name).extension().is_none() {
            name.push_str(".qmd");
        }
        let rel = PathBuf::from(name);
        let id = self.next_session;
        let root = PathBuf::from(format!("/__quarto_untitled__/{id}"));
        let sid = self.insert_session(
            SessionKind::Untitled { rel: rel.clone() },
            root,
            ProjectSource::Attached,
            VfsBaseLayer::new(),
        );
        (sid, rel)
    }

    fn create_project_session(&mut self, root: PathBuf, out: &mut Vec<Output>) -> SessionId {
        let base = match (self.config.load_base)(&root) {
            Ok(base) => base,
            Err(e) => {
                tracing::warn!(root = %root.display(), error = %e, "could not load the project; analysing from disk");
                VfsBaseLayer::new()
            }
        };
        let sid = self.insert_session(
            SessionKind::Project,
            root.clone(),
            ProjectSource::Disk,
            base,
        );
        out.push(Output::Watch { session: sid, root });
        if self.config.sweep {
            out.extend(self.sweep(sid));
        }
        sid
    }

    /// Background jobs profiling every document of the project, as the disk
    /// has it (no buffers), so project-level answers are ready (R5).
    fn sweep(&mut self, sid: SessionId) -> Vec<Output> {
        let Some(session) = self.sessions.get(&sid) else {
            return Vec::new();
        };
        let inputs = self.run_inputs(None, session);
        let rt = inputs.runtime();
        let Ok(project) = inputs.discover(&rt, &inputs.root) else {
            return Vec::new();
        };
        let structure = session.structure;
        let jobs: Vec<Output> = project
            .files
            .iter()
            .filter_map(|f| f.input.strip_prefix(&inputs.root).ok())
            .map(|rel| {
                Output::Spawn(Job {
                    session: sid,
                    rel: rel.to_path_buf(),
                    structure,
                    inputs: inputs.clone(),
                    project: project.clone(),
                })
            })
            .collect();
        let reads = rt.reads();
        if let Some(session) = self.sessions.get_mut(&sid) {
            session.projects.push(CachedProject {
                reads,
                owner: None,
                structure,
                context: project,
            });
        }
        jobs
    }

    fn insert_session(
        &mut self,
        kind: SessionKind,
        root: PathBuf,
        source: ProjectSource,
        base: VfsBaseLayer,
    ) -> SessionId {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        self.sessions.insert(
            id,
            Session {
                id,
                kind,
                root,
                source,
                base: Arc::new(base),
                structure: 0,
                projects: Vec::new(),
                results: HashMap::new(),
            },
        );
        id
    }

    fn mark_stale(&mut self, session: SessionId) {
        for client in self.clients.values_mut() {
            for doc in client.docs.values_mut().filter(|d| d.session == session) {
                doc.stale = true;
            }
        }
    }

    fn drop_if_unused(&mut self, sid: SessionId) {
        let keep = match self.sessions.get(&sid) {
            Some(s) => s.kind == SessionKind::Project,
            None => return,
        };
        let open = self
            .clients
            .values()
            .any(|c| c.docs.values().any(|d| d.session == sid));
        if !keep && !open {
            self.sessions.remove(&sid);
        }
    }

    /// The root of every session, for tests and diagnostics.
    pub fn session_roots(&self) -> Vec<PathBuf> {
        self.sessions.values().map(|s| s.root.clone()).collect()
    }
}

/// Whether a buffer is a document the workspace analyses. Non-QMD inputs
/// that need source conversion are out of scope for v1 (decision D6).
fn is_document(uri: &DocUri, rel: &Path) -> bool {
    if uri.to_file_path().is_none() {
        return true;
    }
    rel.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("qmd") || e.eq_ignore_ascii_case("md"))
}

fn empty_result() -> DocumentResult {
    DocumentResult {
        format: "html".to_string(),
        symbols: Vec::new(),
        folding_ranges: Vec::new(),
        diagnostics: Vec::new(),
        profile: None,
        authored_meta: None,
        ast_context: None,
    }
}

/// Lexically normalize: drop `.`, fold `..`.
fn normalize(path: &Path) -> PathBuf {
    quarto_core::overlay_runtime::normalize_overlay_path(path)
}

/// The canonical form of a path that may not exist: canonicalize the
/// longest existing ancestor and re-attach the rest. Session roots and
/// document paths go through this so `/var` vs `/private/var` (macOS) or a
/// symlinked checkout compare equal.
fn canonical(rt: &dyn SystemRuntime, path: &Path) -> PathBuf {
    let path = normalize(path);
    let mut existing = path.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = rt.canonicalize(&existing) {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return path,
        }
        if existing
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
        {
            return path;
        }
    }
}
