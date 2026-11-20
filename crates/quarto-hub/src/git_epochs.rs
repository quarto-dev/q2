//! Git-aware sync for the hub (GE-3, epic `2026-11-18-git-epochs-epic.md`).
//!
//! With [`GitTracking::Track`] the hub is a git-aware disk peer:
//!
//! - **The gate (G5).** `HubContext::sync_all` and `sync_file` ask the controller's synchronous `check()`
//!   first and skip everything while git is mid-operation. The same object is the [`SyncGate`] each sync
//!   pass consults again after reading a file and before it merges or writes.
//! - **The registry (G6, G17).** The hub creates the sidecar doc, writes the `repo` entry, and registers every
//!   participating file's doc under its blob at startup, only when the disk blob equals the HEAD blob.
//! - **Decisions (G2, G8, G9).** When the controller reports a decision, the hub classifies it. A commit moves
//!   registry entries forward and touches no index entry. A checkout runs the arrival rule per file, writes
//!   each sync checkpoint, and then changes the index once (flips, unlists, relists, new files).
//!
//! With [`GitTracking::Ignore`] (the default) none of this runs.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use automerge::{Automerge, ChangeHash, ROOT, ReadDoc, transaction::Transactable};
use quarto_git_epochs::arrival::{
    ArrivalAction, ArrivalInput, Attached, Kind, Policy, arrive, normalize_text,
};
use quarto_git_epochs::classify::{ChangedFile, ClassifyInput, Peer, Verdict, classify};
use quarto_git_epochs::controller::{
    Action, Controller, ControllerOptions, Decision, GateMode, GateReason,
};
use quarto_git_epochs::numbers::{
    CLIENT_UNAVAILABLE_TIMEOUT_MS, CONTROLLER_TICK_PENDING_MS, MTIME_SKIP_MS,
};
use quarto_git_epochs::probe::{ControllerProbe, GitProbe, ProbeOptions, SubprocessProbe};
use quarto_git_epochs::registry::doc as regdoc;
use quarto_git_epochs::registry::{self, Entry, RegistryOp, RegistryView};
use quarto_git_epochs::vocabulary::EventKind;
use quarto_util::to_forward_slashes;
use samod::{DocHandle, DocumentId, Repo};
use serde::Serialize;
use sha1::{Digest, Sha1};
use tracing::{debug, info, warn};

use crate::context::HubContext;
use crate::error::{Error, Result};
use crate::index::{IndexDocument, RepoIdentity};
use crate::resource::{
    self, compute_hash, create_binary_document, detect_mime_type, read_binary_content,
};
use crate::server::OnFileChangedCallback;
use crate::sync::{DiskWritePolicy, SyncGate};
use crate::sync_state::{SyncState, sha256_hash};

/// Whether the hub follows git (`--git track`, GE-4). Default: ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitTracking {
    #[default]
    Ignore,
    Track,
}

/// One entry of the status route's decision list (G33): what the hub did, when, to which files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionRecord {
    /// A shared event name (`flipped`, `joined`, `seeded`, `retired`, `moved-forward`, ...).
    pub kind: String,
    pub time_ms: u64,
    pub files: Vec<String>,
    /// The G9 step (`"1"`, `"2b"`, ...); `None` for a commit-type decision.
    pub step: Option<String>,
    /// Edits a retire step set aside.
    pub discarded: Option<u32>,
}

/// What `GET /api/preview/git-status` reports (GE-4 serves it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitStatus {
    pub tracking: bool,
    pub paused: bool,
    pub pending: bool,
    pub markers: Vec<String>,
    /// At most the last 20 decisions, oldest first.
    pub decisions: Vec<DecisionRecord>,
}

const DECISION_HISTORY: usize = 20;

/// Seams for tests (and for GE-4 and GE-13). Production code never sets them.
#[derive(Default)]
pub struct GitTestHooks {
    /// Runs after a sync reads a file (the argument is the relative path), before the gate is asked again.
    pub after_read: Mutex<Option<Box<dyn FnMut(&str) + Send>>>,
    /// Ordering log: `checkpoint:<path>` for each checkpoint, then `index`, then `registry`.
    pub events: Mutex<Vec<String>>,
    /// When it returns true for a path, attaching that file's canonical doc fails.
    pub fail_attach: Mutex<Option<Box<dyn Fn(&str) -> bool + Send>>>,
    /// When it returns an error kind for a path, reading that file fails with it.
    pub fail_read: Mutex<Option<Box<dyn Fn(&str) -> Option<std::io::ErrorKind> + Send>>>,
    /// Attach wait override in milliseconds; 0 means the default.
    pub attach_timeout_ms: AtomicU64,
    /// While set, `git_tick_at` does nothing: a decision stays pending for a test to inspect.
    pub hold_decisions: AtomicBool,
    /// Canonical-doc fetches in flight right now (arrival waits on all of them at once).
    pub attach_in_flight: AtomicUsize,
    /// The most canonical-doc fetches that were in flight together. A count, not a clock: it is 3 for three docs fetched
    /// concurrently and 1 if they were fetched one after another, whatever the load on the machine.
    pub attach_max_in_flight: AtomicUsize,
}

/// Counts one canonical-doc fetch as in flight until it is dropped.
struct InFlight<'a>(&'a GitTestHooks);

impl<'a> InFlight<'a> {
    fn new(hooks: &'a GitTestHooks) -> Self {
        let now = hooks.attach_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        hooks.attach_max_in_flight.fetch_max(now, Ordering::SeqCst);
        InFlight(hooks)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.attach_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The hub's git state. Lives in an `Arc` inside [`HubContext`].
pub struct GitEpochs {
    project_root: PathBuf,
    probe: Arc<SubprocessProbe>,
    controller: Mutex<Controller<SubprocessProbe>>,
    drive: tokio::sync::Mutex<()>,
    deciding: AtomicBool,
    last_closed_ms: AtomicU64,
    startup_pending: AtomicBool,
    sidecar: Mutex<Option<DocHandle>>,
    sidecar_heads: Mutex<Vec<ChangeHash>>,
    /// path -> blob at the last decision (or startup).
    last_tracked: Mutex<BTreeMap<String, String>>,
    generations: Mutex<HashMap<String, i64>>,
    decisions: Mutex<VecDeque<DecisionRecord>>,
    callback: Mutex<Option<OnFileChangedCallback>>,
    pub hooks: GitTestHooks,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Git's blob id of `bytes` (the hash `git hash-object` prints, computed without running git, so no clean
/// filter runs; G4).
pub fn blob_id(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    hex::encode(h.finalize())
}

/// True when the disk bytes are the HEAD blob, allowing for `core.autocrlf` (a CRLF working copy of an LF blob).
fn disk_is_blob(bytes: &[u8], blob: &str) -> bool {
    if blob_id(bytes) == blob {
        return true;
    }
    if bytes.windows(2).any(|w| w == b"\r\n") {
        let lf = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
        return blob_id(lf.as_bytes()) == blob;
    }
    false
}

fn kind_of(path: &str) -> Kind {
    match quarto_project_files::discovery::classify(Path::new(path)) {
        Some(class) => {
            if class.is_text() {
                Kind::Text
            } else {
                Kind::Binary
            }
        }
        None => {
            let binary = Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(resource::is_binary_extension);
            if binary { Kind::Binary } else { Kind::Text }
        }
    }
}

fn new_text_doc(text: &str) -> Result<Automerge> {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let text_obj = tx.put_object(ROOT, "text", automerge::ObjType::Text)?;
        tx.update_text(&text_obj, text)?;
        Ok(())
    })
    .map_err(|e| Error::Sync(format!("failed to initialize text document: {e:?}")))?;
    Ok(doc)
}

fn doc_text(h: &DocHandle) -> Option<String> {
    h.with_document(|d| {
        let (_, obj) = d.get(ROOT, "text").ok().flatten()?;
        d.text(&obj).ok()
    })
}

fn doc_bytes(h: &DocHandle) -> Option<Vec<u8>> {
    h.with_document(|d| read_binary_content(d))
}

fn sidecar_view(h: &DocHandle) -> RegistryView {
    h.with_document(|d| regdoc::view_of(&*d))
}

fn sidecar_apply(h: &DocHandle, ops: &[RegistryOp]) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }
    h.with_document(|d| {
        d.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, ops);
            Ok(())
        })
        .map(|_| ())
        .map_err(|e| Error::Sync(format!("failed to update the sidecar: {e:?}")))
    })
}

fn bare_doc_id(id: &str) -> &str {
    id.strip_prefix("automerge:").unwrap_or(id)
}

async fn find_doc(repo: &Repo, id: &str, wait: Duration) -> Option<DocHandle> {
    let doc_id = DocumentId::from_str(bare_doc_id(id)).ok()?;
    match tokio::time::timeout(wait, repo.find(doc_id)).await {
        Ok(Ok(Some(handle))) => Some(handle),
        _ => None,
    }
}

fn set_checkpoint(ss: &mut SyncState, h: &DocHandle, hash: &str) {
    let heads = h.with_document(|d| d.get_heads());
    ss.set_checkpoint(&h.document_id().to_string(), &heads, hash);
}

impl GitEpochs {
    /// Builds the controller for the project directory and records its baseline (HEAD, reflog).
    pub fn new(project_root: &Path) -> Arc<Self> {
        let opts = ProbeOptions::new(project_root);
        let mut controller = Controller::new(
            SubprocessProbe::new(opts.clone()),
            ControllerOptions::default(),
        );
        controller.init();
        Arc::new(GitEpochs {
            project_root: project_root.to_path_buf(),
            probe: Arc::new(SubprocessProbe::new(opts)),
            controller: Mutex::new(controller),
            drive: tokio::sync::Mutex::new(()),
            deciding: AtomicBool::new(false),
            last_closed_ms: AtomicU64::new(0),
            startup_pending: AtomicBool::new(false),
            sidecar: Mutex::new(None),
            sidecar_heads: Mutex::new(Vec::new()),
            last_tracked: Mutex::new(BTreeMap::new()),
            generations: Mutex::new(HashMap::new()),
            decisions: Mutex::new(VecDeque::new()),
            callback: Mutex::new(None),
            hooks: GitTestHooks::default(),
        })
    }

    /// False when git is missing, refused the repository, or the repository has no commits.
    pub fn is_active(&self) -> bool {
        self.controller.lock().unwrap().check(now_ms()).mode != GateMode::Off
    }

    pub fn set_callback(&self, cb: OnFileChangedCallback) {
        *self.callback.lock().unwrap() = Some(cb);
    }

    fn rel(&self, path: &Path) -> String {
        match path.strip_prefix(&self.project_root) {
            Ok(rel) => to_forward_slashes(rel),
            Err(_) => to_forward_slashes(path),
        }
    }

    /// Synchronous: markers and the reflog tail, no subprocess (G1, G5).
    pub fn gate_closed(&self) -> bool {
        let now = now_ms();
        let state = self.controller.lock().unwrap().check(now);
        let closed = state.mode == GateMode::Closed || self.deciding.load(Ordering::SeqCst);
        if closed {
            self.last_closed_ms.store(now, Ordering::SeqCst);
        }
        closed
    }

    pub fn status(&self) -> GitStatus {
        let state = self.controller.lock().unwrap().check(now_ms());
        GitStatus {
            tracking: state.mode != GateMode::Off,
            paused: !state.markers.is_empty(),
            pending: state.pending || self.deciding.load(Ordering::SeqCst),
            markers: state.markers,
            decisions: self.decisions.lock().unwrap().iter().cloned().collect(),
        }
    }

    fn event(&self, what: String) {
        self.hooks.events.lock().unwrap().push(what);
    }

    fn attach_timeout(&self) -> Duration {
        match self.hooks.attach_timeout_ms.load(Ordering::SeqCst) {
            0 => Duration::from_millis(CLIENT_UNAVAILABLE_TIMEOUT_MS),
            ms => Duration::from_millis(ms),
        }
    }

    /// The sidecar doc handle once it exists (tests and the status route read the registry through it).
    pub fn sidecar_doc(&self) -> Option<DocHandle> {
        self.sidecar()
    }

    fn sidecar(&self) -> Option<DocHandle> {
        self.sidecar.lock().unwrap().clone()
    }

    fn remember_sidecar_heads(&self, h: &DocHandle) {
        *self.sidecar_heads.lock().unwrap() = h.with_document(|d| d.get_heads());
    }

    fn record(&self, rec: DecisionRecord) {
        let mut q = self.decisions.lock().unwrap();
        q.push_back(rec);
        while q.len() > DECISION_HISTORY {
            q.pop_front();
        }
    }

    /// Called for a watcher event: a write the hub itself made (disk hash equal to the checkpoint hash) does
    /// not restart the quiet window (G1).
    pub fn note_activity(&self, rel: &str, own_write: bool, at_ms: u64) {
        if !own_write {
            self.controller.lock().unwrap().note_activity(rel, at_ms);
        }
    }

    // ---------------------------------------------------------------- startup

    /// Creates or finds the sidecar, writes the `repo` entry, and registers every participating file whose
    /// disk blob equals its HEAD blob (put-if-absent). Returns false when it could not run yet.
    pub(crate) async fn startup(&self, repo: &Repo, index: &IndexDocument) -> bool {
        if !self.is_active() {
            return true;
        }
        let tracked = match self.probe.ls_tree() {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "git epochs: cannot list tracked files; registry not populated");
                return true;
            }
        };
        if let (Ok(roots), Ok(prefix)) = (self.probe.root_commits(), self.probe.show_prefix()) {
            let identity = RepoIdentity { roots, prefix };
            if index.repo_identity().as_ref() != Some(&identity)
                && let Err(e) = index.set_repo_identity(&identity)
            {
                warn!(error = %e, "git epochs: failed to write the repo entry");
            }
        }
        let Some(sidecar) = self.ensure_sidecar(repo, index).await else {
            return true;
        };
        let files = index.get_all_files();
        let view = sidecar_view(&sidecar);
        let mut ops = Vec::new();
        let mut generations = self.generations.lock().unwrap();
        for tf in &tracked {
            let Some(doc) = files.get(&tf.path) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(self.project_root.join(&tf.path)) else {
                continue;
            };
            if !disk_is_blob(&bytes, &tf.blob) {
                continue; // dirty: stays unregistered until a commit-type decision
            }
            let generation = registry::next_generation(&view, &tf.path);
            let entry = Entry {
                path: tf.path.clone(),
                blob: tf.blob.clone(),
                doc: doc.clone(),
                generation,
            };
            let planned = registry::plan_register(&view, &entry);
            if !planned.is_empty() {
                generations.insert(doc.clone(), generation);
            }
            ops.extend(planned);
        }
        drop(generations);
        if let Err(e) = sidecar_apply(&sidecar, &ops) {
            warn!(error = %e, "git epochs: failed to register startup entries");
        }
        self.remember_sidecar_heads(&sidecar);
        *self.last_tracked.lock().unwrap() = tracked
            .iter()
            .map(|t| (t.path.clone(), t.blob.clone()))
            .collect();
        info!(
            registered = ops.len(),
            "git epochs: startup registration done"
        );
        true
    }

    async fn ensure_sidecar(&self, repo: &Repo, index: &IndexDocument) -> Option<DocHandle> {
        if let Some(h) = self.sidecar() {
            return Some(h);
        }
        let handle = match index.epochs_doc_id() {
            Some(id) => match find_doc(repo, &id, self.attach_timeout()).await {
                Some(h) => h,
                None => {
                    warn!(doc_id = %id, "git epochs: the sidecar doc is unavailable; ordinary sync only");
                    return None;
                }
            },
            None => {
                let doc = match Automerge::load(&regdoc::create_sidecar().save()) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(error = ?e, "git epochs: cannot build the sidecar doc");
                        return None;
                    }
                };
                let h = repo.create(doc).await.ok()?;
                if let Err(e) = index.set_epochs_doc_id(&h.document_id().to_string()) {
                    warn!(error = %e, "git epochs: cannot write the sidecar pointer");
                    return None;
                }
                h
            }
        };
        *self.sidecar.lock().unwrap() = Some(handle.clone());
        Some(handle)
    }

    // ---------------------------------------------------------------- driving

    /// One controller tick at `now_ms`; applies a decision when one comes. Returns the delay before the
    /// next tick in milliseconds.
    pub(crate) async fn tick(self: &Arc<Self>, ctx: &Arc<HubContext>, now_ms: u64) -> u64 {
        if self.hooks.hold_decisions.load(Ordering::SeqCst) {
            return CONTROLLER_TICK_PENDING_MS;
        }
        let _drive = self.drive.lock().await;
        let result = {
            let mut c = self.controller.lock().unwrap();
            let r = c.tick(now_ms);
            if !r.actions.is_empty() {
                self.deciding.store(true, Ordering::SeqCst);
            }
            r
        };
        for action in result.actions {
            let Action::Decision(d) = action;
            self.apply_decision(ctx, d).await;
            self.deciding.store(false, Ordering::SeqCst);
        }
        if self.startup_pending.load(Ordering::SeqCst) && !self.gate_closed() {
            ctx.git_run_startup().await;
        }
        if !self.gate_closed() {
            self.reevaluate(ctx).await;
        }
        result.next_tick_ms
    }

    pub(crate) fn mark_startup_pending(&self) {
        self.startup_pending.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_startup_pending(&self) -> bool {
        self.startup_pending.load(Ordering::SeqCst)
    }

    pub(crate) fn clear_startup_pending(&self) {
        self.startup_pending.store(false, Ordering::SeqCst);
    }

    // ---------------------------------------------------------------- decisions

    async fn apply_decision(self: &Arc<Self>, ctx: &Arc<HubContext>, d: Decision) {
        let tracked: BTreeMap<String, String> = d
            .tracked
            .iter()
            .map(|t| (t.path.clone(), t.blob.clone()))
            .collect();
        let prev = self.last_tracked.lock().unwrap().clone();
        // A decision with no reflog entry and no blob change is a lock or a marker without a rewrite
        // (`git add`, a fetch): nothing to classify, and disk edits are the author's.
        let quiet = match &d.reflog {
            Some(entries) => entries.is_empty(),
            None => tracked == prev,
        };
        if quiet && !d.jj {
            if prev.is_empty() {
                *self.last_tracked.lock().unwrap() = tracked;
            }
            return;
        }
        let input = SwitchInput {
            tracked,
            prev,
            held: d.held.iter().cloned().collect(),
            force: None,
            reflog: d.reflog.clone(),
            jj: d.jj,
        };
        self.switch(ctx, input).await;
    }

    /// G7: after a sidecar change, an attached doc that is no longer canonical goes through arrival, and an
    /// attached doc with no entry on any blob is registered again (the repair rule).
    async fn reevaluate(self: &Arc<Self>, ctx: &Arc<HubContext>) {
        let Some(sidecar) = self.sidecar() else {
            return;
        };
        let now_heads = sidecar.with_document(|d| d.get_heads());
        if *self.sidecar_heads.lock().unwrap() == now_heads {
            return;
        }
        let view = sidecar_view(&sidecar);
        let tracked = self.last_tracked.lock().unwrap().clone();
        let files = ctx.index().get_all_files();
        let mut force = Vec::new();
        let mut repair_ops = Vec::new();
        for (path, doc) in &files {
            let Some(blob) = tracked.get(path) else {
                continue;
            };
            let retired = view
                .retired
                .iter()
                .any(|e| &e.path == path && &e.doc == doc);
            match registry::canonical(&view, path, blob) {
                Some(canon) if &canon.doc != doc && !retired => force.push(path.clone()),
                Some(_) => {}
                None => {
                    let generation = self
                        .generations
                        .lock()
                        .unwrap()
                        .get(doc)
                        .copied()
                        .unwrap_or_else(|| registry::next_generation(&view, path));
                    repair_ops.extend(registry::plan_repair(&view, path, doc, generation, blob));
                }
            }
        }
        if let Err(e) = sidecar_apply(&sidecar, &repair_ops) {
            warn!(error = %e, "git epochs: repair failed");
        }
        self.remember_sidecar_heads(&sidecar);
        if !force.is_empty() {
            let input = SwitchInput {
                tracked: tracked.clone(),
                prev: tracked,
                held: BTreeSet::new(),
                force: Some(force),
                reflog: None,
                jj: false,
            };
            self.switch(ctx, input).await;
        }
    }

    async fn switch(self: &Arc<Self>, ctx: &Arc<HubContext>, input: SwitchInput) {
        let Some(sidecar) = self.sidecar() else {
            warn!("git epochs: no sidecar; the decision falls back to ordinary sync");
            *self.last_tracked.lock().unwrap() = input.tracked;
            return;
        };
        let Some(ss_mutex) = ctx.sync_state_mutex() else {
            return;
        };
        let repo = ctx.repo();
        let index = ctx.index();
        let policy = ctx.disk_write_policy();
        let SwitchInput {
            tracked,
            prev,
            held,
            force,
            reflog,
            jj,
        } = input;
        let reeval = force.is_some();
        let index_files = index.get_all_files();
        let view = sidecar_view(&sidecar);

        // Checkpoint hashes, read once and briefly.
        let snapshot: HashMap<String, String> = {
            let ss = ss_mutex.lock().await;
            ss.all_checkpoints()
                .iter()
                .map(|(k, v)| (k.clone(), v.last_sync_content_hash.clone()))
                .collect()
        };

        // Candidates: files whose blob changed, or whose disk differs from the last synced content.
        let mut cands: Vec<Cand> = Vec::new();
        let paths: Vec<String> = match &force {
            Some(f) => f.clone(),
            None => tracked.keys().cloned().collect(),
        };
        for path in paths {
            let Some(blob) = tracked.get(&path) else {
                continue;
            };
            let cur = index_files.get(&path).cloned();
            let kind = kind_of(&path);
            let abs = self.project_root.join(&path);
            let disk = match self.read_disk(&path, &abs) {
                Ok(b) => b,
                Err(e) => {
                    warn!(path = %path, error = %e, "git epochs: cannot read the file; ordinary sync for it");
                    continue;
                }
            };
            let disk_hash = hash_of(kind, &disk);
            let synced_hash = cur.as_ref().and_then(|d| snapshot.get(d)).cloned();
            let differs = held.contains(&path)
                || matches!((&cur, &synced_hash), (Some(_), Some(h)) if h != &disk_hash);
            let blob_changed = prev.get(&path) != Some(blob);
            if !reeval && !(blob_changed || differs) {
                continue;
            }
            let relistable = registry::canonical(&view, &path, blob).is_some();
            let discoverable =
                quarto_project_files::discovery::classify(Path::new(&path)).is_some();
            if cur.is_none() && !relistable && !discoverable {
                continue;
            }
            cands.push(Cand {
                path,
                blob: blob.clone(),
                cur,
                kind,
                disk,
                disk_hash,
                synced_hash,
                blob_bytes: Vec::new(),
                blob_found: false,
            });
        }

        // Files git removed: unlist, keep the doc in the registry (G13).
        let mut removes: Vec<String> = if reeval {
            vec![]
        } else {
            index_files
                .keys()
                .filter(|p| prev.contains_key(*p) && !tracked.contains_key(*p))
                .cloned()
                .collect()
        };
        removes.sort(); // the index map iterates in no fixed order

        // Blob content, one `cat-file --batch`.
        if !cands.is_empty() {
            let ids: Vec<String> = cands.iter().map(|c| c.blob.clone()).collect();
            let me = self.clone();
            let blobs = tokio::task::spawn_blocking(move || me.probe.cat_blobs(&ids))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            match blobs {
                Ok(map) => {
                    for c in &mut cands {
                        if let Some(b) = map.get(&c.blob) {
                            c.blob_bytes = b.clone();
                            c.blob_found = true;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "git epochs: cannot read blobs; ordinary sync");
                    cands.clear();
                }
            }
        }

        let verdict = if reeval {
            Verdict::Checkout
        } else {
            classify(&ClassifyInput {
                peer: Peer::Hub,
                reflog: reflog.clone(),
                changed: cands
                    .iter()
                    .map(|c| ChangedFile {
                        path: c.path.clone(),
                        disk: c.disk_hash.clone(),
                        synced: c.synced_hash.clone().unwrap_or_default(),
                        blob: hash_of(c.kind, &c.blob_bytes),
                    })
                    .collect(),
            })
        };

        let mut reg_ops: Vec<RegistryOp> = Vec::new();
        let mut puts: Vec<(String, String)> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        let mut records: BTreeMap<(String, Option<String>), (Vec<String>, Option<u32>)> =
            BTreeMap::new();

        if verdict == Verdict::Commit {
            // A commit moves the doc forward in the registry; no index entry changes (G8).
            for c in &cands {
                let Some(doc) = &c.cur else { continue };
                let old = prev.get(&c.path).cloned().unwrap_or_default();
                reg_ops.extend(registry::plan_move_forward(
                    &view, &c.path, &old, &c.blob, doc,
                ));
                records
                    .entry((EventKind::MovedForward.as_str().to_string(), None))
                    .or_default()
                    .0
                    .push(c.path.clone());
            }
        } else {
            // Which docs another path already lists, for the fork rule (G12).
            let collisions = registry::case_collisions(&view);
            // Fetch every canonical doc concurrently; the wait is one cap, not one per doc.
            let wait = self.attach_timeout();
            let want: BTreeSet<String> = cands
                .iter()
                .filter_map(|c| registry::canonical(&view, &c.path, &c.blob).map(|e| e.doc.clone()))
                .collect();
            let fetched: Vec<(String, Option<DocHandle>)> =
                futures::future::join_all(want.iter().map(|id| async move {
                    let _in_flight = InFlight::new(&self.hooks);
                    (id.clone(), find_doc(repo, id, wait).await)
                }))
                .await;
            let handles: HashMap<String, Option<DocHandle>> = fetched.into_iter().collect();

            let mut ss = ss_mutex.lock().await;
            for c in &cands {
                match self
                    .arrive_one(
                        repo,
                        &mut ss,
                        policy,
                        jj,
                        &view,
                        &collisions,
                        &index_files,
                        &handles,
                        c,
                    )
                    .await
                {
                    Ok(Some(done)) => {
                        reg_ops.extend(done.reg_ops);
                        if let Some(doc) = done.flip {
                            if index_files.get(&c.path) != Some(&doc) {
                                puts.push((c.path.clone(), doc));
                            }
                            stale.push(c.path.clone());
                        }
                        if let Some(event) = done.event {
                            let e = records
                                .entry((event.as_str().to_string(), Some(done.step.to_string())))
                                .or_default();
                            e.0.push(c.path.clone());
                            e.1 = e.1.or(done.discarded);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(path = %c.path, error = %e, "git epochs: switch failed for this file; ordinary sync");
                    }
                }
            }
            if let Err(e) = ss.save() {
                warn!(error = %e, "git epochs: failed to save sync state");
            }
        }

        // One index change per switch (G13).
        self.event("index".to_string());
        if let Err(e) = index.apply_switch(&puts, &removes, &stale) {
            warn!(error = %e, "git epochs: failed to apply the switch to the index");
        }
        // Registry changes follow the pointer.
        self.event("registry".to_string());
        if let Err(e) = sidecar_apply(&sidecar, &reg_ops) {
            warn!(error = %e, "git epochs: failed to update the registry");
        }
        self.remember_sidecar_heads(&sidecar);

        let cb = self.callback.lock().unwrap().clone();
        if let Some(cb) = cb {
            for (path, _) in &puts {
                cb(ctx.clone(), self.project_root.join(path));
            }
        }

        let now = now_ms();
        for ((kind, step), (files, discarded)) in records {
            self.record(DecisionRecord {
                kind,
                time_ms: now,
                files,
                step,
                discarded,
            });
        }
        if !removes.is_empty() {
            debug!(
                count = removes.len(),
                "git epochs: unlisted files git removed"
            );
        }
        if !reeval {
            *self.last_tracked.lock().unwrap() = tracked;
        }
    }

    fn read_disk(&self, rel: &str, abs: &Path) -> std::io::Result<Vec<u8>> {
        if let Some(f) = self.hooks.fail_read.lock().unwrap().as_ref()
            && let Some(kind) = f(rel)
        {
            return Err(std::io::Error::from(kind));
        }
        std::fs::read(abs)
    }

    /// The arrival rule for one file (G9). `Ok(None)` means nothing to do; `Err` falls back to ordinary sync.
    #[allow(clippy::too_many_arguments)]
    async fn arrive_one(
        &self,
        repo: &Repo,
        ss: &mut SyncState,
        policy: DiskWritePolicy,
        jj: bool,
        view: &RegistryView,
        collisions: &BTreeSet<String>,
        index_files: &HashMap<String, String>,
        handles: &HashMap<String, Option<DocHandle>>,
        c: &Cand,
    ) -> Result<Option<Done>> {
        let canon = registry::canonical(view, &c.path, &c.blob).cloned();
        if canon.is_some()
            && let Some(f) = self.hooks.fail_attach.lock().unwrap().as_ref()
            && f(&c.path)
        {
            return Err(Error::Sync("attaching the canonical doc failed".into()));
        }
        if !c.blob_found {
            return Err(Error::Sync("blob content unavailable".into()));
        }
        let canon_handle = canon
            .as_ref()
            .and_then(|e| handles.get(&e.doc).cloned().flatten());
        let unavailable = canon.is_some() && canon_handle.is_none();
        let attached = match (&c.cur, &canon) {
            (None, _) => Attached::None,
            (Some(cur), Some(e)) if cur == &e.doc => Attached::Canonical,
            (Some(_), _) => Attached::Other,
        };
        let retired_by_other = c.cur.as_ref().is_some_and(|cur| {
            view.retired
                .iter()
                .any(|e| e.path == c.path && &e.doc == cur)
        });
        let doc_content = canon_handle.as_ref().and_then(|h| match c.kind {
            Kind::Text => doc_text(h).map(|t| normalize_text(&t)),
            Kind::Binary => doc_bytes(h).map(|b| compute_hash(&b)),
        });
        let disk_cmp = match c.kind {
            Kind::Text => normalize_text(&String::from_utf8_lossy(&c.disk)),
            Kind::Binary => compute_hash(&c.disk),
        };
        let blob_cmp = match c.kind {
            Kind::Text => normalize_text(&String::from_utf8_lossy(&c.blob_bytes)),
            Kind::Binary => compute_hash(&c.blob_bytes),
        };
        let live_elsewhere = canon
            .as_ref()
            .is_some_and(|e| index_files.iter().any(|(p, d)| d == &e.doc && p != &c.path));
        let out = arrive(&ArrivalInput {
            peer: Peer::Hub,
            policy: Some(match policy {
                DiskWritePolicy::ReadOnly => Policy::ReadOnly,
                DiskWritePolicy::WriteBack => Policy::WriteBack,
            }),
            kind: c.kind,
            disk: disk_cmp.clone(),
            doc: doc_content.clone(),
            blob: blob_cmp.clone(),
            attached,
            retired_by_other,
            jj,
            canonical_unavailable: unavailable,
            live_under_other_path: live_elsewhere,
            fork_syncs: Some(true),
            case_collision: collisions.contains(&c.path),
        });
        let step = out.step.as_str();
        let disk_is_head = disk_cmp == blob_cmp;
        let generation = registry::next_generation(view, &c.path);
        let abs = self.project_root.join(&c.path);
        let mut done = Done {
            flip: None,
            reg_ops: Vec::new(),
            event: out.event,
            step,
            discarded: out.discarded,
        };

        match out.action {
            ArrivalAction::CreateFromDisk
            | ArrivalAction::WaitFiveSecondsThenCreate
            | ArrivalAction::CreateFromDiskUnregistered
            | ArrivalAction::RetireCreateFlip => {
                if out.action == ArrivalAction::RetireCreateFlip
                    && let Some(e) = &canon
                {
                    done.reg_ops
                        .extend(registry::plan_retire(view, &c.path, &c.blob, &e.doc));
                }
                let h = self.create_doc(repo, c).await?;
                let doc_id = h.document_id().to_string();
                self.checkpoint(ss, &h, c, &c.disk_hash);
                let register =
                    out.action != ArrivalAction::CreateFromDiskUnregistered && !jj && disk_is_head;
                if register {
                    self.generations
                        .lock()
                        .unwrap()
                        .insert(doc_id.clone(), generation);
                    done.reg_ops.push(RegistryOp::Put {
                        key: registry::registry_key(&c.path, &c.blob, &doc_id)
                            .map_err(|e| Error::Sync(e.to_string()))?,
                        generation,
                    });
                }
                done.flip = Some(doc_id);
            }
            ArrivalAction::AttachSilently => {
                let h = canon_handle.ok_or_else(|| Error::Sync("canonical doc missing".into()))?;
                self.checkpoint(ss, &h, c, &c.disk_hash);
                done.flip = Some(h.document_id().to_string());
            }
            ArrivalAction::ReBaseline => {
                let h = canon_handle.ok_or_else(|| Error::Sync("canonical doc missing".into()))?;
                self.checkpoint(ss, &h, c, &c.disk_hash);
            }
            ArrivalAction::JoinAndWarn | ArrivalAction::JoinWithNotice => {
                let h = canon_handle.ok_or_else(|| Error::Sync("canonical doc missing".into()))?;
                warn!(
                    path = %c.path,
                    "git epochs: the canonical doc holds live edits not in git; joined it and wrote its text to disk"
                );
                let hash = self.write_doc_to_disk(&h, c, &abs, policy)?;
                self.checkpoint(ss, &h, c, &hash);
                done.flip = Some(h.document_id().to_string());
            }
            ArrivalAction::ForkThenAttach | ArrivalAction::CreateFromContentNoHistory => {
                let source =
                    canon_handle.ok_or_else(|| Error::Sync("canonical doc missing".into()))?;
                let h = if out.action == ArrivalAction::ForkThenAttach {
                    let forked = source.with_document(|d| d.fork());
                    repo.create(forked)
                        .await
                        .map_err(|_| Error::Sync("repo is stopped".into()))?
                } else {
                    let doc = match c.kind {
                        Kind::Text => new_text_doc(&doc_text(&source).unwrap_or_default())?,
                        Kind::Binary => {
                            let bytes = doc_bytes(&source).unwrap_or_default();
                            create_binary_document(
                                &bytes,
                                &detect_mime_type(&bytes, Some(&c.path)),
                            )?
                        }
                    };
                    repo.create(doc)
                        .await
                        .map_err(|_| Error::Sync("repo is stopped".into()))?
                };
                let doc_id = h.document_id().to_string();
                let hash = self.write_doc_to_disk(&h, c, &abs, policy)?;
                self.checkpoint(ss, &h, c, &hash);
                self.generations
                    .lock()
                    .unwrap()
                    .insert(doc_id.clone(), generation);
                done.reg_ops.push(RegistryOp::Put {
                    key: registry::registry_key(&c.path, &c.blob, &doc_id)
                        .map_err(|e| Error::Sync(e.to_string()))?,
                    generation,
                });
                done.flip = Some(doc_id);
            }
            ArrivalAction::OrdinarySync | ArrivalAction::StayLive | ArrivalAction::DetachAndAsk => {
                done.event = None;
            }
        }
        Ok(Some(done))
    }

    fn checkpoint(&self, ss: &mut SyncState, h: &DocHandle, c: &Cand, hash: &str) {
        let _ = c.kind;
        self.event(format!("checkpoint:{}", c.path));
        set_checkpoint(ss, h, hash);
    }

    async fn create_doc(&self, repo: &Repo, c: &Cand) -> Result<DocHandle> {
        let doc = match c.kind {
            Kind::Text => new_text_doc(&String::from_utf8_lossy(&c.disk))?,
            Kind::Binary => {
                create_binary_document(&c.disk, &detect_mime_type(&c.disk, Some(&c.path)))?
            }
        };
        repo.create(doc)
            .await
            .map_err(|_| Error::Sync("repo is stopped".into()))
    }

    /// Under `WriteBack`, makes the disk equal the doc and returns the content hash the checkpoint records.
    /// Under `ReadOnly` the disk is never written, so the checkpoint records the disk hash.
    fn write_doc_to_disk(
        &self,
        h: &DocHandle,
        c: &Cand,
        abs: &Path,
        policy: DiskWritePolicy,
    ) -> Result<String> {
        let (bytes, hash) = match c.kind {
            Kind::Text => {
                let t = doc_text(h).unwrap_or_default();
                let hash = sha256_hash(&t);
                (t.into_bytes(), hash)
            }
            Kind::Binary => {
                let b = doc_bytes(h).unwrap_or_default();
                let hash = compute_hash(&b);
                (b, hash)
            }
        };
        if hash == c.disk_hash || normalize_equal(c, &bytes) {
            return Ok(c.disk_hash.clone());
        }
        if policy == DiskWritePolicy::ReadOnly {
            return Ok(c.disk_hash.clone());
        }
        std::fs::write(abs, &bytes)
            .map_err(|e| Error::Sync(format!("failed to write {}: {e}", abs.display())))?;
        Ok(hash)
    }
}

fn normalize_equal(c: &Cand, doc_bytes: &[u8]) -> bool {
    match c.kind {
        Kind::Text => {
            normalize_text(&String::from_utf8_lossy(doc_bytes))
                == normalize_text(&String::from_utf8_lossy(&c.disk))
        }
        Kind::Binary => false,
    }
}

fn hash_of(kind: Kind, bytes: &[u8]) -> String {
    match kind {
        Kind::Text => sha256_hash(&String::from_utf8_lossy(bytes)),
        Kind::Binary => compute_hash(bytes),
    }
}

struct SwitchInput {
    tracked: BTreeMap<String, String>,
    prev: BTreeMap<String, String>,
    held: BTreeSet<String>,
    /// Re-evaluation of these paths only (no removals, no classification).
    force: Option<Vec<String>>,
    reflog: Option<Vec<String>>,
    jj: bool,
}

struct Cand {
    path: String,
    blob: String,
    cur: Option<String>,
    kind: Kind,
    disk: Vec<u8>,
    disk_hash: String,
    synced_hash: Option<String>,
    blob_bytes: Vec<u8>,
    blob_found: bool,
}

struct Done {
    flip: Option<String>,
    reg_ops: Vec<RegistryOp>,
    event: Option<EventKind>,
    step: &'static str,
    discarded: Option<u32>,
}

impl SyncGate for GitEpochs {
    fn closed(&self) -> bool {
        self.gate_closed()
    }

    fn closed_after_read(&self, path: &Path) -> bool {
        let rel = self.rel(path);
        if let Some(f) = self.hooks.after_read.lock().unwrap().as_mut() {
            f(&rel);
        }
        self.gate_closed()
    }

    fn skip_file(&self, path: &Path) -> bool {
        // Git releases `index.lock` slightly before it writes the reflog, so for a moment after the gate
        // was closed a freshly rewritten file is left for the next tick (G1, Gate 0 item 7).
        let recently_closed =
            now_ms().saturating_sub(self.last_closed_ms.load(Ordering::SeqCst)) < 2_000;
        if !recently_closed {
            return false;
        }
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_millis(MTIME_SKIP_MS))
    }

    fn read_text(&self, path: &Path) -> std::io::Result<String> {
        self.read_disk(&self.rel(path), path).and_then(|b| {
            String::from_utf8(b)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.utf8_error()))
        })
    }

    fn read_bytes(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.read_disk(&self.rel(path), path)
    }
}

/// The reason the controller is off, for a startup log line.
pub fn off_reason_text(reason: GateReason) -> &'static str {
    match reason {
        GateReason::GitMissing => "git is missing or refused this directory",
        GateReason::Unborn => "the repository has no commits",
    }
}
