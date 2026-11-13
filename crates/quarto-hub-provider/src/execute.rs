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

use async_trait::async_trait;
use futures::StreamExt;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{
    CaptureRun, SourceInputs, SourceInputsOptions, compute_input_qmd, compute_source_inputs,
    compute_source_inputs_with, record_capture_run,
};
use quarto_core::overlay_runtime::RenderOverlay;
use quarto_core::project::ProjectContext;
use quarto_core::record::{
    Chosen, EngineError, EngineResult, ImportContext, ImportedOnly, LookupError, RecordLookup,
    RecordSource, RecordSources, WalkContext, execution_record_path, imported_record_is_stale,
    overlay_free_run, read_record, resolve_records,
};
use quarto_hub::index::{CaptureRef, CaptureState, IndexDocument};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
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
    /// default registry); tests pass a passthrough engine. The registry is
    /// itself `Arc`-wrapped (ts-engine-extensions Arc migration) so cloning the
    /// handle for each run is cheap and shares one registry instance.
    registry: Option<Arc<EngineRegistry>>,
    /// Paths currently executing, to collapse a duplicate request for a path
    /// already in flight (mirrors `re_execute.rs`'s `IN_FLIGHT`).
    in_flight: Mutex<HashSet<String>>,
    /// `-M` for `provide-hub` (F8-D9): runtime metadata every execution
    /// sees, so a `-M` that changes how code runs (`execute:eval:false`,
    /// `jupyter:python3`) reaches the engine. `None` adds nothing.
    overlay: Option<Arc<RenderOverlay>>,
    /// `execute: record-sources` (RIU-4). Exactly `engine` (the default) keeps the pre-RIU path: no walk.
    record_sources: RecordSources,
    /// The writer gate, read once at construction (`QUARTO_ENGINE_RECORD_WRITE`).
    record_gate: bool,
}

impl Provider {
    /// Build a provider around a joined repo + index and a consent gate.
    pub fn new(
        repo: Repo,
        index: IndexDocument,
        self_actor_id: impl Into<String>,
        consent: Arc<dyn ConsentGate>,
        registry: Option<Arc<EngineRegistry>>,
    ) -> Arc<Self> {
        Self::new_with_overlay(repo, index, self_actor_id, consent, registry, None)
    }

    /// [`Provider::new`] with a `-M` overlay (F8-D9) that every execution's
    /// runtime carries.
    pub fn new_with_overlay(
        repo: Repo,
        index: IndexDocument,
        self_actor_id: impl Into<String>,
        consent: Arc<dyn ConsentGate>,
        registry: Option<Arc<EngineRegistry>>,
        overlay: Option<Arc<RenderOverlay>>,
    ) -> Arc<Self> {
        Self::new_with_records(
            repo,
            index,
            self_actor_id,
            consent,
            registry,
            overlay,
            RecordSources::provider_default(),
            quarto_core::engine::capture_files::record_writing_enabled(
                quarto_core::engine::capture_files::WriterSurface::Provider,
            ),
        )
    }

    /// [`Provider::new_with_overlay`] with the `execute: record-sources` list and the writer gate (RIU-4).
    /// With the list exactly `engine` this is the pre-RIU provider. With any other list,
    /// `execute_document` walks it, and `engine` absent means this provider never runs code.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_records(
        repo: Repo,
        index: IndexDocument,
        self_actor_id: impl Into<String>,
        consent: Arc<dyn ConsentGate>,
        registry: Option<Arc<EngineRegistry>>,
        overlay: Option<Arc<RenderOverlay>>,
        record_sources: RecordSources,
        record_gate: bool,
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
            overlay,
            record_sources,
            record_gate,
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

        // The host list is a ceiling: the document's own `execute: record-sources` may only
        // reorder or narrow it (OPR-5 Part B). Read it from the materialized project before
        // anything runs, so a guest can narrow even the default list. The one extra
        // materialization per request is the price of reading the guest's `_quarto.yml`,
        // `_metadata.yml` and front matter the way the render will.
        let resolved = self.resolve_document_sources(rel_path).await;

        // The default list (`engine` alone) is the pre-RIU path with no walk and no source
        // inputs. Any other list walks its sources; the engine is reached only through the walk.
        let result = match resolved {
            Err(msg) => Err(msg),
            Ok((list, notes)) => {
                let result = if list == RecordSources::provider_default() {
                    self.run_and_store(rel_path).await
                } else {
                    self.execute_with_sources(rel_path, &list).await
                };
                match result {
                    // A warning on a successful run has no client route yet (strand
                    // bd-ia3idnvk); the operator log carries it.
                    Err(msg) if !notes.is_empty() => Err(format!("{msg} ({})", notes.join("; "))),
                    other => other,
                }
            }
        };

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

        // Canonicalize: engines (knitr) report supporting files by their real
        // path, and on macOS the temp dir is under the `/var` -> `/private/var`
        // symlink. Without this, `collect_capture_files` sees those files as
        // outside the document directory and drops the figures (bd-qbhp2cvv).
        let root = tmp
            .path()
            .canonicalize()
            .map_err(|e| format!("canonicalizing the materialization temp dir: {e}"))?;
        let abs_path = root.join(rel_path);
        // The plain runtime is the source's view (what the viewer's staleness
        // check sees); the overlay-wrapped one is what executes (F8-D9).
        let plain: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let runtime: Arc<dyn SystemRuntime> = match &self.overlay {
            Some(overlay) => overlay.apply(plain.clone()),
            None => plain.clone(),
        };
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
        let mut run =
            record_capture_run(&abs_path, &project, runtime.clone(), self.registry.clone())
                .await
                .map_err(|e| format!("engine pipeline failed: {e}"))?;
        if !run.has_content() {
            return Err("engine produced no capture (no code cells?)".to_string());
        }
        if let Some(overlay) = &self.overlay {
            // F8-D9 / 8.3c: the staleness key (the first capture's `input_qmd`)
            // describes the *source*, not this provider's execution settings.
            // Recorded under a `-M`, it would embed the overlay's metadata and
            // differ from the viewer's overlay-free recompute, marking every
            // capture stale. Store the overlay-free bytes; execution and the
            // outputs above still saw the overlay. The execution record's
            // document hashes are recomputed the same way (EXR-5).
            let source_project = ProjectContext::discover(&abs_path, plain.as_ref())
                .map_err(|e| format!("project discovery failed: {e}"))?;
            let source = compute_source_inputs(&abs_path, &source_project, plain)
                .await
                .map_err(|e| format!("computing the source inputs failed: {e}"))?;
            run = overlay_free_run(run, &source, overlay)?;
        }

        let (new_doc_id, without_record) = write_capture_doc(
            &self.repo,
            rel_path,
            &run,
            quarto_core::engine::capture_files::record_writing_enabled(
                quarto_core::engine::capture_files::WriterSurface::Provider,
            ),
        )
        .await
        .map_err(|e| format!("failed to store capture binary doc: {e}"))?;
        if !without_record.is_empty() {
            // The decision is made when the payload is written, so the note comes after
            // the run, once per document (Q-18-15, RIU-1).
            eprintln!(
                "{}",
                quarto_core::engine::diagnostics::engines_without_record_line(
                    rel_path,
                    &without_record
                )
            );
        }

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

    /// Publish a record a non-engine source supplies (`positron` or a directory of Positron
    /// caches), through the shared source walk and the RIU-2 publish seam (RIU-3a Phase D).
    ///
    /// Consent gates **engine** code only, and only `run_and_store` consults it; this path never
    /// calls an engine (the walk is given [`ImportedOnly`], which refuses one), so it needs no
    /// consent and works with `AlwaysReject`. `record_gate` is the writer gate, an argument so a
    /// test needs no environment: with it off the seam refuses (an imported record has no legacy
    /// form) and the error names `QUARTO_ENGINE_RECORD_WRITE`. Nothing is written then.
    ///
    /// The document is materialized to a temp path, so the locator's URI-hash name can never
    /// match here: only a directory source or the content-hash tier finds a cache.
    ///
    /// Not here (RIU-4): the call from `execute_document` and the start-up validation.
    pub async fn publish_record_source(
        &self,
        rel_path: &str,
        source: &RecordSource,
        record_gate: bool,
    ) -> Result<ExecOutcome, String> {
        if !matches!(source, RecordSource::Positron | RecordSource::Dir(_)) {
            return Err(format!(
                "`{}` is not an imported record source",
                source.label()
            ));
        }
        if !self.index.has_file(rel_path) {
            return Err(format!("path '{rel_path}' is not in the project index"));
        }
        if !is_safe_relative(rel_path) {
            return Err(format!("refusing to publish unsafe path '{rel_path}'"));
        }

        let list = RecordSources::new(vec![source.clone()]);
        match self
            .walk_and_publish(rel_path, &list, record_gate, &|_, _| Box::new(ImportedOnly))
            .await?
        {
            Some(done) => Ok(done),
            None => Err("internal error: an imported-only walk reported an engine run".into()),
        }
    }

    /// `execute_document` for a non-default list: walk the sources, publishing what one supplies.
    /// `engine` is reachable only through [`ProviderLookup::run_engine`], which goes through the
    /// consent-gated `run_and_store`.
    async fn execute_with_sources(
        &self,
        rel_path: &str,
        list: &RecordSources,
    ) -> Result<ExecOutcome, String> {
        let declined = std::cell::Cell::new(false);
        let engine_outcome = std::cell::RefCell::new(None);
        let make = |project_dir: &Path, abs_path: &Path| -> Box<dyn RecordLookup + '_> {
            Box::new(ProviderLookup {
                provider: self,
                rel_path,
                project_dir: project_dir.to_path_buf(),
                abs_path: abs_path.to_path_buf(),
                declined: &declined,
                engine_outcome: &engine_outcome,
            })
        };
        let result = self
            .walk_and_publish(rel_path, list, self.record_gate, &make)
            .await;
        if declined.get() {
            return Ok(ExecOutcome::Rejected);
        }
        match result? {
            Some(done) => Ok(done),
            None => engine_outcome.into_inner().ok_or_else(|| {
                "internal error: the engine published but reported no outcome".into()
            }),
        }
    }

    /// Materialize the project, compute the source inputs once, walk `list` with the lookup
    /// `make` builds, and publish the chosen run through the shared seam. A walk that chose the
    /// engine returns the outcome `run_and_store` already wrote (reported by the lookup).
    async fn walk_and_publish<'l>(
        &self,
        rel_path: &str,
        list: &RecordSources,
        record_gate: bool,
        make: &dyn Fn(&Path, &Path) -> Box<dyn RecordLookup + 'l>,
    ) -> Result<Option<ExecOutcome>, String> {
        let tmp =
            tempfile::tempdir().map_err(|e| format!("creating a materialization temp dir: {e}"))?;
        crate::materialize::materialize_project(&self.repo, &self.index, tmp.path())
            .await
            .map_err(|e| format!("materializing the project: {e}"))?;
        let root = tmp
            .path()
            .canonicalize()
            .map_err(|e| format!("canonicalizing the materialization temp dir: {e}"))?;
        let abs_path = root.join(rel_path);
        let plain: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&abs_path, plain.as_ref())
            .map_err(|e| format!("project discovery failed: {e}"))?;
        let inputs = compute_source_inputs_with(
            &abs_path,
            &project,
            plain,
            SourceInputsOptions {
                ownership: true,
                registry: self.registry.clone(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| format!("computing the source inputs failed: {e}"))?;

        let walk = WalkContext {
            list,
            engine_enabled: list.contains_engine(),
            record_gate,
            import: Some(ImportContext {
                abs_path: &abs_path,
                source: Some(rel_path),
                positron_dir: None,
            }),
        };
        let resolution =
            resolve_records(&walk, &inputs, make(&project.dir, &abs_path).as_ref()).await;
        let run = match resolution.chosen {
            Some(Chosen::Run { run, .. }) => run,
            // The lookup already published the engine's run and holds its outcome.
            Some(Chosen::EnginePublished) => return Ok(None),
            None => {
                if let Some(failure) = resolution.first_failure() {
                    return Err(failure.to_string());
                }
                if let Some(why) = resolution.refusal() {
                    return Err(why.to_string());
                }
                let why = resolution
                    .outcomes
                    .iter()
                    .map(|o| format!("{}: {:?}", o.source.label(), o.kind))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(format!("no record to publish from {why}"));
            }
        };

        let stale = run.record.as_ref().is_some_and(|r| {
            imported_record_is_stale(
                r,
                &String::from_utf8_lossy(&inputs.input_qmd),
                &inputs.sugared_blocks,
            )
        });
        let (doc_id, _) = write_capture_doc(&self.repo, rel_path, &run, record_gate)
            .await
            .map_err(|e| format!("failed to store capture binary doc: {e}"))?;
        self.index
            .set_capture(
                rel_path,
                &CaptureRef {
                    capture_doc_id: doc_id.clone(),
                    staleness: Some(stale),
                    state: Some(CaptureState::Idle),
                    last_error: None,
                },
            )
            .map_err(|e| format!("failed to update sidecar: {e}"))?;
        Ok(Some(ExecOutcome::Executed(doc_id)))
    }

    /// The record sources for one document: the host list (the ceiling) narrowed or reordered
    /// by the document's own `execute: record-sources`, read from a fresh materialization of
    /// the project. Out-of-ceiling entries are ignored with a warning (logged here, and
    /// returned so a failed run can carry them in `last_error`). An empty result is an error
    /// that names the reason.
    async fn resolve_document_sources(
        &self,
        rel_path: &str,
    ) -> Result<(RecordSources, Vec<String>), String> {
        use quarto_core::record::{
            CommandSources, Trust, describe_sources, resolve_record_sources,
        };
        let tmp =
            tempfile::tempdir().map_err(|e| format!("creating a materialization temp dir: {e}"))?;
        crate::materialize::materialize_project(&self.repo, &self.index, tmp.path())
            .await
            .map_err(|e| format!("materializing the project: {e}"))?;
        let root = tmp
            .path()
            .canonicalize()
            .map_err(|e| format!("canonicalizing the materialization temp dir: {e}"))?;
        let abs_path = root.join(rel_path);
        let plain: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&abs_path, plain.as_ref())
            .map_err(|e| format!("project discovery failed: {e}"))?;
        // The document is read by the layers *below* the operator's `-M`: the runtime layer is
        // the host's, and the ceiling already carries its value.
        let resolved = resolve_record_sources(
            &project,
            &abs_path,
            None,
            plain.as_ref(),
            &CommandSources::PROVIDE_HUB,
            Trust::Ceiling {
                allowed: &self.record_sources,
            },
        )
        .map_err(|e| e.to_string())?;
        let notes: Vec<String> = resolved.notes.iter().map(|n| n.text.clone()).collect();
        for note in &resolved.notes {
            tracing::warn!(path = %rel_path, "{}", note.text);
            eprintln!("warning: {rel_path}: {}", note.text);
        }
        if resolved.sources.as_slice().is_empty() {
            return Err(format!(
                "the document's `execute: record-sources` leaves no source this provider \
                 allows (allowed: {})",
                describe_sources(&self.record_sources)
            ));
        }
        Ok((resolved.sources, notes))
    }

    /// Whether `engine` is in this provider's `execute: record-sources` list.
    fn engine_listed(&self) -> bool {
        self.record_sources.contains_engine()
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
    // nested-cell-masking (spec § "The engine capture — asymmetric, by
    // necessity" / `compute_input_qmd`'s three-consumer table): this is the
    // one security-facing consumer — the artifact a human operator reads
    // before consenting to execution — so, unlike the staleness compare and
    // cache key, it must show the author's own `{r}`, not the internal
    // mask marker. `compute_input_qmd` returns masked bytes; unmask them
    // here before writing.
    let text = String::from_utf8_lossy(&bytes);
    let unmasked = quarto_core::engine::nested_cell_mask::unmask(&text);

    let base = Path::new(rel_path).file_name().map_or_else(
        || std::ffi::OsString::from("document"),
        |n| n.to_os_string(),
    );
    let mut file_name = base;
    file_name.push(".resolved.qmd");
    let review_file = review_dir.join(file_name);

    std::fs::write(&review_file, &unmasked)
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

/// Encode the run (a record doc when the writer gate is on, which for `provide-hub` is
/// off by default until the hosted hub reads records, and the run has a record, else a
/// legacy capture doc) and store it as a binary automerge doc.
/// Mirror of `quarto-preview`'s `write_capture_doc`; both go through
/// `encode_for_publish` (serialize + gzip + 10MB size warning, bd-qbhp2cvv)
/// and `create_engine_payload_document`, behind the one gate
/// `record_writing_enabled` (EXR-13b; the caller passes the gate). Returns the new doc's
/// id and the engines that build no record, when that is why the legacy form was chosen.
async fn write_capture_doc(
    repo: &Repo,
    rel_path: &str,
    run: &CaptureRun,
    record_gate: bool,
) -> Result<(String, Vec<String>), ProviderError> {
    use quarto_core::engine::capture_files::{PublishError, encode_for_publish};
    let encoded = encode_for_publish(&run.captures, run.record.as_ref(), record_gate).map_err(
        |e| match e {
            PublishError::Io(e) => ProviderError::Protocol(format!("serialize/gzip captures: {e}")),
            refused @ PublishError::RecordWriterOff => ProviderError::Protocol(refused.to_string()),
        },
    )?;
    let meta = quarto_hub::resource::CaptureDocMeta {
        source_path: rel_path.to_string(),
        engines: encoded.engines,
    };
    quarto_hub::resource::publish_payload_doc(repo, encoded.is_record, &encoded.gzipped, &meta)
        .await
        .map_err(|e| match e {
            quarto_hub::resource::PublishDocError::Build(e) => {
                ProviderError::Protocol(format!("binary doc: {e}"))
            }
            quarto_hub::resource::PublishDocError::RepoStopped => {
                ProviderError::Repo("samod repo stopped".into())
            }
        })
        .map(|doc_id| (doc_id, encoded.engines_without_record))
}

/// The provider's [`RecordLookup`] (RIU-4): what `q2` and `engine` mean when `provide-hub`
/// is the one asking. `cache` has nothing (a start-up error for this command); `q2` reads the
/// `.quarto` copy from the materialized project (nearly always empty); `engine` is
/// `run_and_store`, consent gate included, which publishes as it runs.
struct ProviderLookup<'a> {
    provider: &'a Provider,
    rel_path: &'a str,
    /// The materialized project and document, for the `q2` copy's path.
    project_dir: PathBuf,
    abs_path: PathBuf,
    /// Set when the operator declined at the consent gate.
    declined: &'a std::cell::Cell<bool>,
    /// What `run_and_store` returned when it published.
    engine_outcome: &'a std::cell::RefCell<Option<ExecOutcome>>,
}

#[async_trait(?Send)]
impl RecordLookup for ProviderLookup<'_> {
    async fn lookup(
        &self,
        source: &RecordSource,
        _inputs: &SourceInputs,
    ) -> Result<Option<CaptureRun>, LookupError> {
        match source {
            RecordSource::Q2 => {
                let path = execution_record_path(&self.project_dir, &self.abs_path);
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => {
                        return Err(LookupError::Unreadable(format!(
                            "reading {}: {e}",
                            path.display()
                        )));
                    }
                };
                let record = read_record(&text).map_err(|e| {
                    LookupError::Unreadable(format!("{} is not a record: {e}", path.display()))
                })?;
                Ok(Some(CaptureRun {
                    captures: Vec::new(),
                    record: Some(record),
                }))
            }
            _ => Ok(None),
        }
    }

    async fn run_engine(&self, _inputs: &SourceInputs) -> Result<EngineResult, EngineError> {
        if !self.provider.engine_listed() {
            return Err(EngineError::Failed(
                "`engine` is not in `execute: record-sources`, so this provider does not run code"
                    .into(),
            ));
        }
        match self.provider.run_and_store(self.rel_path).await {
            Ok(ExecOutcome::Rejected) => {
                self.declined.set(true);
                Err(EngineError::Failed("the operator declined".into()))
            }
            Ok(done) => {
                *self.engine_outcome.borrow_mut() = Some(done);
                Ok(EngineResult::Published)
            }
            Err(e) => Err(EngineError::Failed(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RIU-1: with the gate on, a capture from an engine outside the record keeps the
    /// document legacy and is reported to the caller, which prints the one-line note.
    #[tokio::test]
    async fn recordless_engine_is_reported_by_the_writer() {
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let in_record = record.metadata.quarto_engines[0].name.clone();
        let capture = |e: &str| -> quarto_trace::EngineCapture {
            serde_json::from_value(serde_json::json!({
                "engine_name": e, "input_qmd": "x", "result": {}
            }))
            .unwrap()
        };
        let run = CaptureRun {
            captures: vec![capture(&in_record), capture("echo")],
            record: Some(record),
        };
        let repo = Repo::build_tokio().load().await;
        let (_, without) = write_capture_doc(&repo, "doc.qmd", &run, true)
            .await
            .unwrap();
        assert_eq!(without, vec!["echo".to_string()]);
    }

    /// EXR-13b writer gate: gate off writes a legacy capture doc even when the run
    /// has a record; gate on writes a record doc.
    async fn written_kind(gate: bool) -> (String, String) {
        use automerge::ReadDoc;
        use quarto_core::record::read_record;
        let record = read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let run = CaptureRun {
            captures: vec![
                serde_json::from_value::<quarto_trace::EngineCapture>(serde_json::json!({
                    "engine_name": "jupyter", "input_qmd": "x", "result": {}
                }))
                .unwrap(),
            ],
            record: Some(record),
        };
        let repo = Repo::build_tokio().load().await;
        let (id, _) = write_capture_doc(&repo, "doc.qmd", &run, gate)
            .await
            .unwrap();
        let handle = repo
            .find(id.parse().unwrap())
            .await
            .unwrap()
            .expect("doc exists");
        handle.with_document(|doc| {
            let get = |k: &str| {
                doc.get(automerge::ROOT, k)
                    .unwrap()
                    .and_then(|(v, _)| v.into_string().ok())
                    .unwrap_or_default()
            };
            let kind = quarto_hub::resource::read_capture_meta(doc)
                .and_then(|m| m.kind)
                .unwrap_or_default();
            (get("mimeType"), kind)
        })
    }

    #[tokio::test]
    async fn writer_gate_off_writes_a_legacy_capture_doc() {
        let (mime, kind) = written_kind(false).await;
        assert_eq!(mime, quarto_hub::resource::CAPTURE_MIME_TYPE);
        assert_eq!(kind, "engine-capture");
    }

    /// RIU-2: a record with no captures has no legacy form, so the gate-off publish refuses
    /// and names the variable.
    #[tokio::test]
    async fn gate_off_refuses_a_record_with_no_legacy_form() {
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let run = CaptureRun {
            captures: Vec::new(),
            record: Some(record),
        };
        let repo = Repo::build_tokio().load().await;
        let err = write_capture_doc(&repo, "doc.qmd", &run, false)
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Protocol(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains(quarto_core::engine::capture_files::RECORD_WRITING_ENV),
            "{err}"
        );
    }

    #[tokio::test]
    async fn writer_gate_on_writes_a_record_doc() {
        let (mime, kind) = written_kind(true).await;
        assert_eq!(mime, quarto_hub::resource::RECORD_MIME_TYPE);
        assert_eq!(kind, "engine-record");
    }

    /// EXR-15: the record doc's `meta.engines` is the record's engines list, in order, and the
    /// payload parses back to an equal record.
    #[tokio::test]
    async fn record_doc_meta_and_payload_match_the_record() {
        use automerge::ReadDoc;
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let expected_engines: Vec<String> = record
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let run = CaptureRun {
            captures: Vec::new(),
            record: Some(record.clone()),
        };
        let repo = Repo::build_tokio().load().await;
        let (id, _) = write_capture_doc(&repo, "doc.qmd", &run, true)
            .await
            .unwrap();
        let handle = repo.find(id.parse().unwrap()).await.unwrap().unwrap();
        let (engines, gzipped) = handle.with_document(|doc| {
            let engines = quarto_hub::resource::read_capture_meta(doc)
                .map(|m| m.engines)
                .unwrap_or_default();
            let content = doc
                .get(automerge::ROOT, "content")
                .unwrap()
                .and_then(|(v, _)| v.into_bytes().ok())
                .unwrap();
            (engines, content)
        });
        assert_eq!(engines, expected_engines);
        let parsed =
            quarto_core::pandoc_request::captures::parse_capture_payload(Some(&gzipped), &id)
                .unwrap();
        let quarto_core::pandoc_request::captures::CapturePayload::Record(back) = parsed else {
            panic!("expected a record payload");
        };
        assert_eq!(*back, record);
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

    /// T21 (hunk H11): `write_review_file` is the artifact a human operator
    /// reads before consenting to execution (see this module's doc comment,
    /// "Consent"). The operator must see the `{r}` they actually wrote, not
    /// `q2-nested-executable` — the one *security-facing* consumer of
    /// `compute_input_qmd`, deliberately unmasked while the staleness compare
    /// (`quarto-preview/src/capture_driver.rs`) and cache key
    /// (`quarto-preview/src/cache.rs`) deliberately stay masked. Do not
    /// "fix" this test by masking the review file to match those two.
    ///
    /// The discriminating assertion is `assert_ne!` against
    /// `compute_input_qmd`'s own (masked, per H10) bytes: today, with neither
    /// H10 nor H11 implemented, `compute_input_qmd` and `write_review_file`
    /// produce byte-identical unmasked output, so the two are *equal* and the
    /// test is genuinely red. It only goes green once `write_review_file`
    /// unmasks what `compute_input_qmd` masked, making the two outputs
    /// differ for a doc with a display block.
    #[tokio::test]
    async fn write_review_file_shows_the_authors_r_not_the_mask() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let qmd_path = dir.path().join("doc.qmd");
        std::fs::write(
            &qmd_path,
            "---\ntitle: Review\n---\n\n```markdown\n```{r}\n1 + 1\n```\n```\n",
        )
        .expect("write fixture");

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&qmd_path, runtime.as_ref())
            .expect("discover single-file project");

        let review_dir = dir.path().join("review");
        std::fs::create_dir_all(&review_dir).expect("create review dir");

        let review_file =
            write_review_file(&qmd_path, &project, runtime.clone(), &review_dir, "doc.qmd")
                .await
                .expect("write_review_file");

        let contents = std::fs::read_to_string(&review_file).expect("read review file");

        let masked = compute_input_qmd(&qmd_path, &project, runtime)
            .await
            .expect("compute_input_qmd");
        let masked_str = String::from_utf8(masked).expect("UTF-8");

        assert_ne!(
            contents, masked_str,
            "write_review_file must unmask what compute_input_qmd masks — the \
             review file and the (masked) engine-input bytes must not be \
             identical for a doc with a display block; got:\n{contents}"
        );
        assert!(
            contents.contains("{r}"),
            "review file must show the author's `{{r}}` opener verbatim so the \
             operator reviews the actual bytes that will run; got:\n{contents}"
        );
        assert!(
            !contents.contains("q2-nested-executable"),
            "review file must NOT contain the internal masking marker — an \
             operator can't consent to code they can't recognize; got:\n{contents}"
        );
    }
}
