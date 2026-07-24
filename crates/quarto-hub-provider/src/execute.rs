//! Execute-on-request loop for the code-execution provider (bd-sfet3264,
//! Phase 4a).
//!
//! Once joined to a hub, in `--watch` mode a [`Provider`] does two things on
//! the index `DocHandle`'s ephemeral channel:
//!
//!  1. **Broadcasts a capability beacon** every [`BEACON_INTERVAL`] so editors
//!     know an executor is online and which engines it can run.
//!  2. **Listens for `exec/request`** messages and, once the operator accepts
//!     at the [`ConsentGate`](crate::ConsentGate), materializes the project,
//!     runs the engines natively (the same uncached `record_capture` path
//!     `q2 preview` uses), and writes the result back as a capture binary doc +
//!     `CaptureRef` sidecar entry — the transport the Phase 1 editor consumes.
//!
//! The one-shot CLI path instead calls [`Provider::execute_once`] directly for
//! a single document, then [`Provider::flush_to_hub`] + [`Provider::stop`].
//!
//! The engine work runs on a blocking worker (`spawn_blocking` +
//! `pollster::block_on`), mirroring `quarto-preview`'s `re_execute.rs`, so a
//! long engine run never stalls the ephemeral-message reactor.
//!
//! ## Consent (bd-9lgiulr4)
//!
//! Before any engine runs, the provider materializes the project, synthesizes
//! the **resolved document** (the post-include, pre-engine QMD — exactly what
//! the engine receives) to a reviewable file, and consults a
//! [`ConsentGate`](crate::ConsentGate). Only on an affirmative decision does it
//! invoke the engine. This keeps a hijacked/spoofed CRDT document from silently
//! driving execution: the operator reviews the *actual bytes that will run*.
//! The review artifact is derived from the **same materialized snapshot** that
//! is then executed, so what is reviewed equals what runs.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{compute_input_qmd, record_capture};
use quarto_core::project::ProjectContext;
use quarto_hub::index::{CaptureRef, CaptureState, IndexDocument};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;
use samod::Repo;

use crate::ProviderError;
use crate::consent::ConsentGate;
use crate::exec_channel::{BEACON_INTERVAL, ExecMessage, parse_exec_message};

/// MIME type stamped on capture binary docs. Re-exported from
/// quarto-hub, the single source of truth (bd-eiku4ymo) — the former
/// duplication between here and `quarto-preview` is gone now that
/// quarto-hub (a dependency of both) owns the constant. The TS
/// consumers (`ts-packages/quarto-sync-client`,
/// `hub-client/.../ReactPreview.capture.integration.test.tsx`) hold
/// the same literal.
pub use quarto_hub::resource::CAPTURE_MIME_TYPE;

/// Result of a single execution attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOutcome {
    /// The engine ran and a capture was written; carries its document id.
    Executed(String),
    /// The operator declined at the consent gate; nothing was written.
    Rejected,
}

/// A joined provider ready to serve execution requests. Held behind an `Arc`
/// so the beacon loop and per-request workers can share it.
pub struct Provider {
    repo: Repo,
    index: IndexDocument,
    /// Beacon `actorId` (the samod peer id — stable for the process). Only used
    /// by the `--watch` beacon; unused in one-shot mode.
    self_actor_id: String,
    /// Engines advertised in the beacon (available, non-markdown).
    engines: Vec<String>,
    /// Operator consent, consulted before every engine run.
    consent: Arc<dyn ConsentGate>,
    /// Engine registry override for the capture run. `None` in production (the
    /// default registry); tests pass a passthrough engine. `Clone` is cheap
    /// (engines are `Arc`ed) so each run gets its own copy.
    registry: Option<EngineRegistry>,
    /// Paths currently executing, to collapse a duplicate request for a path
    /// already in flight (mirrors `re_execute.rs`'s `IN_FLIGHT`).
    in_flight: Mutex<HashSet<String>>,
}

impl Provider {
    /// Build a provider around a joined repo + index and a consent gate.
    pub fn new(
        repo: Repo,
        index: IndexDocument,
        self_actor_id: impl Into<String>,
        consent: Arc<dyn ConsentGate>,
        registry: Option<EngineRegistry>,
    ) -> Arc<Self> {
        let probe = registry.clone().unwrap_or_default();
        let engines = available_engines(&probe);
        Arc::new(Self {
            repo,
            index,
            self_actor_id: self_actor_id.into(),
            engines,
            consent,
            registry,
            in_flight: Mutex::new(HashSet::new()),
        })
    }

    /// Run until `shutdown` resolves: broadcast the beacon on a timer and serve
    /// requests from the ephemeral channel.
    pub async fn run<S>(self: Arc<Self>, shutdown: S)
    where
        S: std::future::Future<Output = ()> + Send,
    {
        let beacon = tokio::spawn({
            let provider = Arc::clone(&self);
            async move { provider.run_beacon_loop().await }
        });

        tokio::select! {
            () = Arc::clone(&self).run_request_loop() => {}
            () = shutdown => {}
        }

        beacon.abort();
    }

    /// Broadcast the capability beacon every [`BEACON_INTERVAL`], bumping the
    /// generation each tick. (Generation is reserved for the D5 `--force`
    /// takeover; unused by editors in Phase 4a.)
    async fn run_beacon_loop(self: Arc<Self>) {
        let mut generation = 0u64;
        let mut ticker = tokio::time::interval(BEACON_INTERVAL);
        loop {
            ticker.tick().await;
            let beacon =
                ExecMessage::beacon(self.self_actor_id.clone(), self.engines.clone(), generation);
            self.index.handle().broadcast(beacon.to_cbor());
            generation = generation.wrapping_add(1);
        }
    }

    /// Consume the index handle's ephemeral messages, dispatching each allowed
    /// `exec/request` to a blocking worker.
    async fn run_request_loop(self: Arc<Self>) {
        let mut stream = self.index.handle().ephemera();
        while let Some(bytes) = stream.next().await {
            let Some(ExecMessage::Request {
                path,
                request_id,
                requester_actor_id: _,
            }) = parse_exec_message(&bytes)
            else {
                continue; // beacons from other providers, or non-exec traffic
            };

            if !self.claim(&path) {
                tracing::debug!(path = %path, "exec request skipped: already in flight");
                continue;
            }

            let provider = Arc::clone(&self);
            let path_for_task = path.clone();
            let request_id = request_id.clone();
            tokio::task::spawn_blocking(move || {
                // The consent gate (inside execute_document) prompts the
                // operator before any engine runs; this blocking worker is the
                // right place for that blocking read.
                let result = pollster::block_on(provider.execute_document(&path_for_task));
                provider.release(&path_for_task);
                match result {
                    Ok(ExecOutcome::Executed(doc_id)) => tracing::info!(
                        path = %path_for_task,
                        capture_doc_id = %doc_id,
                        "wrote capture for exec request"
                    ),
                    Ok(ExecOutcome::Rejected) => tracing::info!(
                        path = %path_for_task,
                        request_id = %request_id,
                        "exec request rejected by operator"
                    ),
                    Err(e) => {
                        tracing::warn!(path = %path_for_task, error = %e, "exec request failed")
                    }
                }
            });
        }
    }

    /// Reserve `path` in the in-flight set; returns false if already claimed.
    fn claim(&self, path: &str) -> bool {
        self.in_flight
            .lock()
            .expect("in-flight mutex poisoned")
            .insert(path.to_string())
    }

    fn release(&self, path: &str) {
        self.in_flight
            .lock()
            .expect("in-flight mutex poisoned")
            .remove(path);
    }

    /// Materialize the project, obtain operator consent, then (on accept) run
    /// the engines for `rel_path` and write the capture doc + sidecar. Returns
    /// [`ExecOutcome`]. Public for the scripted integration test (drives one
    /// request without networking) and the one-shot CLI path.
    pub async fn execute_document(&self, rel_path: &str) -> Result<ExecOutcome, String> {
        if !self.index.has_file(rel_path) {
            return Err(format!("path '{rel_path}' is not in the project index"));
        }
        if !is_safe_relative(rel_path) {
            return Err(format!("refusing to execute unsafe path '{rel_path}'"));
        }

        let result = self.run_and_store(rel_path).await;

        // Only flip status once we know a run was actually attempted (consent
        // granted). A rejected request touches nothing.
        if let Ok(ExecOutcome::Executed(_)) = &result {
            // Success sidecar already written by run_and_store.
        } else if let Err(msg) = &result {
            // Surface the failure on an existing capture entry (best effort).
            if let Some(existing) = self.index.get_capture(rel_path) {
                let errored = CaptureRef {
                    capture_doc_id: existing.capture_doc_id,
                    staleness: existing.staleness,
                    state: Some(CaptureState::Error),
                    last_error: Some(msg.clone()),
                };
                let _ = self.index.set_capture(rel_path, &errored);
            }
        }

        result
    }

    /// The body of [`execute_document`](Self::execute_document): materialize →
    /// synthesize the review file → **consent** → (on accept) mark running →
    /// record (uncached) → write doc → set sidecar.
    ///
    /// Consent is obtained from the **same materialized snapshot** whose
    /// resolved form was written to the review file, so the reviewed bytes are
    /// the executed bytes (no re-pull between review and run).
    async fn run_and_store(&self, rel_path: &str) -> Result<ExecOutcome, String> {
        let tmp =
            tempfile::tempdir().map_err(|e| format!("creating a materialization temp dir: {e}"))?;
        crate::materialize::materialize_project(&self.repo, &self.index, tmp.path())
            .await
            .map_err(|e| format!("materializing the project: {e}"))?;

        let abs_path = tmp.path().join(rel_path);
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&abs_path, runtime.as_ref())
            .map_err(|e| format!("project discovery failed: {e}"))?;

        // Synthesize the resolved document (post-include, pre-engine QMD — the
        // bytes the engine receives) to a separate reviewable location.
        let review_dir =
            tempfile::tempdir().map_err(|e| format!("creating the review temp dir: {e}"))?;
        let review_file = write_review_file(
            &abs_path,
            &project,
            runtime.clone(),
            review_dir.path(),
            rel_path,
        )
        .await?;

        // Consent gate — BEFORE any engine invocation. On reject, write
        // nothing (Q6).
        if !self.consent.review(rel_path, &review_file) {
            return Ok(ExecOutcome::Rejected);
        }

        // Flip an *existing* capture to `running` for durable status. A
        // first-ever run has no CaptureRef to attach state to (the doc id is
        // required), so it goes straight to producing the capture — same as
        // re_execute.rs. (Done only after consent, so a rejected request never
        // perturbs the sidecar.)
        if let Some(existing) = self.index.get_capture(rel_path) {
            let running = CaptureRef {
                capture_doc_id: existing.capture_doc_id,
                staleness: existing.staleness,
                state: Some(CaptureState::Running),
                last_error: None,
            };
            let _ = self.index.set_capture(rel_path, &running);
        }

        // Decision #4: always a fresh run (uncached record_capture) — code may
        // have side effects and we don't prove it side-effect-free.
        let captures = record_capture(&abs_path, &project, runtime.clone(), self.registry.clone())
            .await
            .map_err(|e| format!("engine pipeline failed: {e}"))?;
        if captures.is_empty() {
            return Err("engine produced no capture (no code cells?)".to_string());
        }

        let new_doc_id = write_capture_doc(&self.repo, rel_path, &captures)
            .await
            .map_err(|e| format!("failed to store capture binary doc: {e}"))?;

        let updated = CaptureRef {
            capture_doc_id: new_doc_id.clone(),
            staleness: Some(false),
            state: Some(CaptureState::Idle),
            last_error: None,
        };
        self.index
            .set_capture(rel_path, &updated)
            .map_err(|e| format!("failed to update sidecar: {e}"))?;

        Ok(ExecOutcome::Executed(new_doc_id))
    }

    /// Execute one document once, off the reactor. The engine work and the
    /// consent prompt's blocking stdin read run on a blocking worker (mirroring
    /// the watch-mode request loop), so neither stalls the async runtime. Used
    /// by the one-shot CLI path.
    pub async fn execute_once(self: &Arc<Self>, rel_path: &str) -> Result<ExecOutcome, String> {
        let me = Arc::clone(self);
        let path = rel_path.to_string();
        tokio::task::spawn_blocking(move || pollster::block_on(me.execute_document(&path)))
            .await
            .map_err(|e| format!("execution task panicked: {e}"))?
    }

    /// Block until the hub peer has acknowledged both the index sidecar update
    /// and the capture binary doc — i.e. collaborators will actually receive
    /// the output — or a bounded timeout elapses.
    ///
    /// Uses the samod fork's [`DocHandle::they_have_our_changes`], which
    /// resolves once the peer's shared heads match ours (a real delivery
    /// confirmation, not a fixed sleep). The `timeout` is a safety net for a
    /// slow/dropped connection; on timeout we log and return so one-shot still
    /// exits (Q7: real wait preferred, bounded fallback accepted).
    pub async fn flush_to_hub(&self, capture_doc_id: &str, timeout: Duration) {
        let index_handle = self.index.handle();
        let (peers, _) = index_handle.peers();
        let Some(conn) = peers.keys().next().copied() else {
            tracing::warn!(
                "no hub connection at flush time; the capture may not have propagated to collaborators"
            );
            return;
        };

        let repo = self.repo.clone();
        let capture_doc_id = capture_doc_id.to_string();
        let confirm = async move {
            // Peer has our index sidecar update (the CaptureRef pointer).
            index_handle.they_have_our_changes(conn).await;
            // Peer has the capture binary doc itself.
            if let Ok(id) = samod::DocumentId::from_str(&capture_doc_id)
                && let Ok(Some(cap_handle)) = repo.find(id).await
            {
                cap_handle.they_have_our_changes(conn).await;
            }
        };

        if tokio::time::timeout(timeout, confirm).await.is_err() {
            tracing::warn!(
                "timed out waiting for the hub to acknowledge the capture; it may still be in flight"
            );
        }
    }

    /// Stop the underlying samod repo (drains storage tasks). Call after
    /// [`flush_to_hub`](Self::flush_to_hub) in one-shot mode.
    pub async fn stop(&self) {
        self.repo.stop().await;
    }
}

/// Synthesize the resolved document — the post-include, pre-engine QMD that
/// `EngineExecutionStage` hands the engine — to `<review_dir>/<name>.resolved.qmd`
/// and return its path. This is the artifact the operator reviews before
/// consenting.
async fn write_review_file(
    abs_path: &Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    review_dir: &Path,
    rel_path: &str,
) -> Result<PathBuf, String> {
    let bytes = compute_input_qmd(abs_path, project, runtime)
        .await
        .map_err(|e| format!("computing the resolved document for review: {e}"))?;

    let base = Path::new(rel_path).file_name().map_or_else(
        || std::ffi::OsString::from("document"),
        |n| n.to_os_string(),
    );
    let mut file_name = base;
    file_name.push(".resolved.qmd");
    let review_file = review_dir.join(file_name);

    std::fs::write(&review_file, &bytes)
        .map_err(|e| format!("writing the review file {}: {e}", review_file.display()))?;
    Ok(review_file)
}

/// Names of available execution engines, excluding the always-present markdown
/// engine (not a real "run" target). Advertised in the capability beacon.
fn available_engines(registry: &EngineRegistry) -> Vec<String> {
    let mut names: Vec<String> = registry
        .engine_names()
        .into_iter()
        .filter(|name| *name != "markdown")
        .filter(|name| registry.get(name).is_some_and(|e| e.is_available()))
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

/// Whether `rel_path` is a safe project-relative path (no `..`, not absolute).
fn is_safe_relative(rel_path: &str) -> bool {
    let path = std::path::Path::new(rel_path);
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        && path.components().any(|c| matches!(c, Component::Normal(_)))
}

/// Gzip the captures to JSON and store them as a capture binary automerge doc.
/// Mirror of `quarto-preview`'s `write_capture_doc` (kept in sync via the
/// shared [`CAPTURE_MIME_TYPE`] and the shared wire-format helper
/// `gzip_captures` — serialize + gzip + 10MB size warning, bd-qbhp2cvv).
async fn write_capture_doc(
    repo: &Repo,
    rel_path: &str,
    captures: &[EngineCapture],
) -> Result<String, ProviderError> {
    let gzipped = quarto_core::engine::capture_files::gzip_captures(captures)
        .map_err(|e| ProviderError::Protocol(format!("serialize/gzip captures: {e}")))?;
    let meta = quarto_hub::resource::CaptureDocMeta {
        source_path: rel_path.to_string(),
        engines: captures.iter().map(|c| c.engine_name.clone()).collect(),
    };
    let doc = quarto_hub::resource::create_capture_document(&gzipped, &meta)
        .map_err(|e| ProviderError::Protocol(format!("binary doc: {e}")))?;
    let handle = repo
        .create(doc)
        .await
        .map_err(|_| ProviderError::Repo("samod repo stopped".into()))?;
    Ok(handle.document_id().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_safe_relative_guards_traversal() {
        assert!(is_safe_relative("report.qmd"));
        assert!(is_safe_relative("chapters/intro.qmd"));
        assert!(is_safe_relative("./a.qmd"));
        assert!(!is_safe_relative("../escape.qmd"));
        assert!(!is_safe_relative("a/../../x.qmd"));
        assert!(!is_safe_relative("/etc/passwd"));
        assert!(!is_safe_relative(""));
        assert!(!is_safe_relative("."));
    }

    #[test]
    fn available_engines_excludes_markdown() {
        let registry = EngineRegistry::default();
        let engines = available_engines(&registry);
        assert!(
            !engines.iter().any(|e| e == "markdown"),
            "markdown must not be advertised: {engines:?}"
        );
    }
}
