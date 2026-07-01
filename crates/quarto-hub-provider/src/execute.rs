//! Execute-on-request loop for the code-execution provider (bd-sfet3264,
//! Phase 4a).
//!
//! Once joined to a hub, a [`Provider`] does two things on the index
//! `DocHandle`'s ephemeral channel:
//!
//!  1. **Broadcasts a capability beacon** every [`BEACON_INTERVAL`] so editors
//!     know an executor is online and which engines it can run.
//!  2. **Listens for `exec/request`** messages and, when the [`AuthzPolicy`]
//!     allows the requester, materializes the project, runs the engines
//!     natively (the same uncached `record_capture` path `q2 preview` uses),
//!     and writes the result back as a capture binary doc + `CaptureRef`
//!     sidecar entry — the transport the Phase 1 editor already consumes.
//!
//! The engine work runs on a blocking worker (`spawn_blocking` +
//! `pollster::block_on`), mirroring `quarto-preview`'s `re_execute.rs`, so a
//! long engine run never stalls the ephemeral-message reactor.
//!
//! ## Authorization (Phase 4a: mechanism-first, fail closed)
//!
//! [`AuthzPolicy`] is `AllowAll` (opened by `q2 provide-hub --allow-all`) or
//! `Deny` (the safe default: refuse every request). The real provider-only
//! gating — honoring a request only when `requesterActorId` equals the
//! provider's own per-project actor id — needs the hub to accept a Bearer on
//! `GET /auth/actor` and lands in Phase 5 as a third `ProviderOnly` variant
//! this enum is the seam for.

use std::collections::HashSet;
use std::path::Component;
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::write::GzEncoder;
use futures::StreamExt;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_hub::index::{CaptureRef, CaptureState, IndexDocument};
use quarto_hub::resource::create_binary_document;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;
use samod::Repo;
use std::io::Write as _;

use crate::ProviderError;
use crate::exec_channel::{BEACON_INTERVAL, ExecMessage, parse_exec_message};

/// MIME type stamped on capture binary docs. Must stay byte-identical to
/// `quarto_preview::capture_driver::CAPTURE_MIME_TYPE` and the literal the TS
/// consumers use (`ts-packages/quarto-sync-client`,
/// `hub-client/.../ReactPreview.capture.integration.test.tsx`). Duplicated here
/// rather than depending on the heavy `quarto-preview` crate (which pulls in an
/// axum server); the value is a self-describing label, not validated on read.
pub const CAPTURE_MIME_TYPE: &str = "application/x-engine-capture+gzip";

/// Who may trigger execution on this provider.
///
/// Phase 4a ships only the two poles; Phase 5 adds
/// `ProviderOnly { self_actor_id }` (gate on `requesterActorId == self`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzPolicy {
    /// Honor requests from anyone with access to the document
    /// (`q2 provide-hub --allow-all`).
    AllowAll,
    /// Refuse every request (the safe default when `--allow-all` is absent).
    Deny,
}

impl AuthzPolicy {
    /// Whether a request from `requester_actor_id` may run.
    pub fn allows(&self, _requester_actor_id: &str) -> bool {
        match self {
            AuthzPolicy::AllowAll => true,
            AuthzPolicy::Deny => false,
        }
    }
}

/// A joined provider ready to serve execution requests. Held behind an `Arc`
/// so the beacon loop and per-request workers can share it.
pub struct Provider {
    repo: Repo,
    index: IndexDocument,
    /// Beacon `actorId`. Phase 4a uses the samod peer id (stable for the
    /// process); Phase 5 swaps in the per-project actor id from `/auth/actor`.
    self_actor_id: String,
    /// Engines advertised in the beacon (available, non-markdown).
    engines: Vec<String>,
    authz: AuthzPolicy,
    /// Engine registry override for the capture run. `None` in production (the
    /// default registry); tests pass a passthrough engine. `Clone` is cheap
    /// (engines are `Arc`ed) so each run gets its own copy.
    registry: Option<EngineRegistry>,
    /// Paths currently executing, to collapse a duplicate request for a path
    /// already in flight (mirrors `re_execute.rs`'s `IN_FLIGHT`).
    in_flight: Mutex<HashSet<String>>,
}

impl Provider {
    /// Build a provider around a joined repo + index.
    pub fn new(
        repo: Repo,
        index: IndexDocument,
        self_actor_id: impl Into<String>,
        authz: AuthzPolicy,
        registry: Option<EngineRegistry>,
    ) -> Arc<Self> {
        let probe = registry.clone().unwrap_or_default();
        let engines = available_engines(&probe);
        Arc::new(Self {
            repo,
            index,
            self_actor_id: self_actor_id.into(),
            engines,
            authz,
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
                requester_actor_id,
            }) = parse_exec_message(&bytes)
            else {
                continue; // beacons from other providers, or non-exec traffic
            };

            if !self.authz.allows(&requester_actor_id) {
                tracing::info!(
                    path = %path,
                    request_id = %request_id,
                    requester = %requester_actor_id,
                    "refusing exec request (provider not opened with --allow-all)"
                );
                continue;
            }

            if !self.claim(&path) {
                tracing::debug!(path = %path, "exec request skipped: already in flight");
                continue;
            }

            let provider = Arc::clone(&self);
            let path_for_task = path.clone();
            tokio::task::spawn_blocking(move || {
                let result = pollster::block_on(provider.execute_document(&path_for_task));
                provider.release(&path_for_task);
                match result {
                    Ok(doc_id) => tracing::info!(
                        path = %path_for_task,
                        capture_doc_id = %doc_id,
                        "wrote capture for exec request"
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

    /// Materialize the project, run the engines for `rel_path`, and write the
    /// capture doc + sidecar. Returns the new capture document id. Public for
    /// the scripted integration test (drives one request without networking).
    pub async fn execute_document(&self, rel_path: &str) -> Result<String, String> {
        if !self.index.has_file(rel_path) {
            return Err(format!("path '{rel_path}' is not in the project index"));
        }
        if !is_safe_relative(rel_path) {
            return Err(format!("refusing to execute unsafe path '{rel_path}'"));
        }

        // Flip an *existing* capture to `running` for durable status. A
        // first-ever run has no CaptureRef to attach state to (the doc id is
        // required), so it goes straight to producing the capture — same as
        // re_execute.rs.
        if let Some(existing) = self.index.get_capture(rel_path) {
            let running = CaptureRef {
                capture_doc_id: existing.capture_doc_id,
                staleness: existing.staleness,
                state: Some(CaptureState::Running),
                last_error: None,
            };
            let _ = self.index.set_capture(rel_path, &running);
        }

        let result = self.run_and_store(rel_path).await;

        if let Err(msg) = &result {
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

    /// The happy-path body of [`execute_document`](Self::execute_document):
    /// materialize → discover → record (uncached) → write doc → set sidecar.
    async fn run_and_store(&self, rel_path: &str) -> Result<String, String> {
        let tmp =
            tempfile::tempdir().map_err(|e| format!("creating a materialization temp dir: {e}"))?;
        crate::materialize::materialize_project(&self.repo, &self.index, tmp.path())
            .await
            .map_err(|e| format!("materializing the project: {e}"))?;

        let abs_path = tmp.path().join(rel_path);
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&abs_path, runtime.as_ref())
            .map_err(|e| format!("project discovery failed: {e}"))?;

        // Decision #4: always a fresh run (uncached record_capture) — code may
        // have side effects and we don't prove it side-effect-free.
        let captures = record_capture(&abs_path, &project, runtime.clone(), self.registry.clone())
            .await
            .map_err(|e| format!("engine pipeline failed: {e}"))?;
        if captures.is_empty() {
            return Err("engine produced no capture (no code cells?)".to_string());
        }

        let new_doc_id = write_capture_doc(&self.repo, &captures)
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

        Ok(new_doc_id)
    }
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
/// shared [`CAPTURE_MIME_TYPE`] and JSON+gzip wire format).
async fn write_capture_doc(
    repo: &Repo,
    captures: &[EngineCapture],
) -> Result<String, ProviderError> {
    let json = serde_json::to_vec(captures)
        .map_err(|e| ProviderError::Protocol(format!("serialize captures: {e}")))?;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&json)
        .map_err(|e| ProviderError::Protocol(format!("gzip write: {e}")))?;
    let gzipped = enc
        .finish()
        .map_err(|e| ProviderError::Protocol(format!("gzip finish: {e}")))?;
    let doc = create_binary_document(&gzipped, CAPTURE_MIME_TYPE)
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
    fn allow_all_permits_any_requester() {
        assert!(AuthzPolicy::AllowAll.allows("anyone"));
        assert!(AuthzPolicy::AllowAll.allows(""));
    }

    #[test]
    fn deny_refuses_every_requester() {
        assert!(!AuthzPolicy::Deny.allows("anyone"));
        assert!(!AuthzPolicy::Deny.allows(""));
    }

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
