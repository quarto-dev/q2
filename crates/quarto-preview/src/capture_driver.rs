//! Engine capture driver for the q2 preview server (Phase C.1).
//!
//! Once a `HubContext` is constructed and its initial filesystem sync
//! has settled, this module walks the tracked `.qmd` files and — for
//! any file with code cells but no recorded capture yet — runs the
//! engine via [`quarto_core::engine::preview_record::record_capture`]
//! and stores the result.
//!
//! Storage shape:
//!   - The serialized [`EngineCapture`] is gzipped JSON and stored as a
//!     samod *binary* document (`{ content, mimeType, hash }` schema
//!     from `quarto_hub::resource::create_binary_document`). The MIME
//!     type `application/x-engine-capture+gzip` flags it for the
//!     browser-side reader (Phase C.4).
//!   - The binary doc's document ID is written to the IndexDocument's
//!     V2 capture sidecar via `IndexDocument::set_capture`. The SPA
//!     observes the sidecar through the sync client's
//!     `onCapturesChange` callback (landed in Phase C.3).
//!
//! Cap (per plan §C.1 Risk #2): documents are processed *sequentially*
//! so a project with five engine-bearing docs doesn't burst five
//! concurrent engine invocations on startup.

use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{
    CaptureRun, SourceInputsOptions, compute_input_qmd, compute_source_inputs,
    compute_source_inputs_with,
};
use quarto_core::pandoc_request::captures::{CapturePayload, parse_capture_payload};
use quarto_core::project::ProjectContext;
use quarto_core::record::{Record, record_staleness, source_staleness};
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::HubContext;
use quarto_hub::index::CaptureRef;
use quarto_system_runtime::SystemRuntime;

use crate::config::{EnginePolicy, ReExecuteConfig};
use crate::diagnostics;

/// MIME type marker written into the capture binary doc so the
/// browser-side reader can verify it's looking at the right payload
/// shape (gzipped EngineCapture JSON) instead of an unrelated resource.
/// Re-exported from quarto-hub, the single source of truth
/// (bd-eiku4ymo).
pub use quarto_hub::resource::CAPTURE_MIME_TYPE;

/// Walk the project's `.qmd` files and record engine captures for any
/// file that has code cells but no existing sidecar entry yet.
///
/// Returns the number of captures successfully written. Soft-fails on
/// per-file errors (logs and continues) — one bad document shouldn't
/// keep us from recording captures for the rest. Returns `Ok(0)` for
/// standalone-mode contexts (no project files).
///
/// Tests pass `engine_registry: Some(...)` to substitute a passthrough
/// engine for jupyter/knitr without a real runtime; production callers
/// pass `None` and use the default registry.
pub async fn record_eager_captures(
    ctx: Arc<HubContext>,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<Arc<EngineRegistry>>,
    cfg: &ReExecuteConfig,
) -> Result<usize, RecordError> {
    // C.6: `preview.engine: off` forbids running the engine, but it no longer returns here:
    // the record sources are still consulted, so a cached or `.quarto` record shows and only
    // the engine is skipped (the walk reports it unavailable; RIU epic, decision 5).
    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        // Standalone mode — nothing to record.
        return Ok(0);
    };
    // Walk the project's `.qmd` files only — engines run against qmd
    // input. Iterating the index map (which also covers `_quarto.yml`,
    // images, etc.) would feed non-qmd bytes into parse-document; that
    // panics on the first binary file (bd-i6jy4 / bd-6qbto).
    let Some(project_files) = ctx.project_files() else {
        return Ok(0);
    };

    // Snapshot the list. The index map will be mutated (sidecar
    // writes) as we go; iterating a snapshot keeps the loop simple.
    let qmd_paths: Vec<String> = project_files
        .qmd_files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let mut recorded = 0_usize;
    for rel_path in qmd_paths {
        // Sidecar is keyed by the same path as the index — skip files
        // that already have a capture so re-runs are idempotent.
        if ctx.index().has_capture(&rel_path) {
            // A persisted `--data-dir` carries the ref (and its staleness
            // flag) over from the previous session. The file may have
            // changed while the server was stopped, and no file-change
            // event will fire for that, so check it now (RIU-0 S1, D1).
            // Under `auto` this also re-executes a changed document.
            if let Err(e) = recompute_staleness(
                ctx.clone(),
                runtime.clone(),
                &rel_path,
                engine_registry.clone(),
                cfg,
            )
            .await
            {
                tracing::warn!(rel_path = %rel_path, error = %e, "startup staleness recompute failed");
            }
            tracing::debug!(rel_path = %rel_path, "capture already recorded, skipping");
            continue;
        }

        // Resolve to an absolute path inside the project tree. The
        // IndexDocument stores forward-slash relative paths regardless
        // of platform; PathBuf::push handles platform-native joining.
        let abs_path = project_root.join(&rel_path);

        match record_one(
            &abs_path,
            &rel_path,
            &ctx,
            &runtime,
            engine_registry.clone(),
            cfg,
        )
        .await
        {
            Ok(true) => recorded += 1,
            Ok(false) => {} // no capture needed; not an error
            Err(e) => {
                // bd-b9kzg: also surface the failure to the SPA via
                // the per-page diagnostic sink. `emit` calls
                // `tracing::warn!` itself, so this is a clean
                // replacement (no double-logging). The sink is only
                // populated when the OnceLock is set — i.e. when
                // run_with_on_ready bootstrapped it. Unit tests
                // that call `record_eager_captures` directly without
                // booting a server get a silent no-op.
                if let Some(sink) = diagnostics::current_sink() {
                    sink.emit(
                        &rel_path,
                        DiagnosticMessageBuilder::warning("Engine capture failed")
                            .with_code("Q-PREVIEW-CAP-1")
                            .problem(format!("{e}"))
                            .build(),
                    );
                } else {
                    tracing::warn!(
                        rel_path = %rel_path,
                        error = %e,
                        "failed to record engine capture; continuing",
                    );
                }
            }
        }
    }

    if recorded > 0 {
        tracing::info!(count = recorded, "recorded engine captures");
    }
    Ok(recorded)
}

/// Drive a single file's capture record. Returns `Ok(true)` if a
/// capture was written, `Ok(false)` if no capture was needed (prose-only
/// or default markdown engine), and `Err` for failures the caller
/// should log.
async fn record_one(
    abs_path: &std::path::Path,
    rel_path: &str,
    ctx: &Arc<HubContext>,
    runtime: &Arc<dyn SystemRuntime>,
    engine_registry: Option<Arc<EngineRegistry>>,
    cfg: &ReExecuteConfig,
) -> Result<bool, RecordError> {
    // Project discovery walks up for `_quarto.yml`; for single-file
    // projects this produces an is_single_file context.
    let project = ProjectContext::discover(abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    // RIU-2: walk the ordered record sources (cache, the `.quarto` copy, the engine) and publish
    // whichever supplies first. The pre-engine pipeline runs once, here; the cache key and the
    // staleness check both derive from its result.
    // Only the Positron import needs the language ownership map (its gate); ask for it only then.
    // The list is per document: its `execute: record-sources`, bounded by the host list under
    // `--share` (OPR-5). An unreadable key is the page's one diagnostic.
    let resolved = cfg
        .document_sources(abs_path, None)
        .map_err(RecordError::RecordFailed)?;
    if let Some(sink) = crate::diagnostics::current_sink() {
        for note in &resolved.notes {
            let builder = if note.warning {
                quarto_error_reporting::DiagnosticMessageBuilder::warning(note.text.clone())
            } else {
                quarto_error_reporting::DiagnosticMessageBuilder::info(note.text.clone())
            };
            sink.emit(rel_path, builder.build());
        }
    }
    let sources = resolved.sources;
    let wants_import = sources.iter().any(|s| {
        matches!(
            s,
            quarto_core::record::RecordSource::Positron | quarto_core::record::RecordSource::Dir(_)
        )
    });
    let inputs = compute_source_inputs_with(
        abs_path,
        &project,
        runtime.clone(),
        SourceInputsOptions {
            ownership: wants_import,
            registry: None,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;
    let lookup = crate::record_lookup::PreviewLookup {
        abs_path,
        project: &project,
        runtime: runtime.clone(),
        registry: engine_registry,
        cache_dir: &cfg.cache_dir,
    };
    let walk_ctx = quarto_core::record::WalkContext {
        import: wants_import.then_some(quarto_core::record::ImportContext {
            abs_path,
            source: Some(rel_path),
            positron_dir: None,
        }),
        list: &sources,
        engine_enabled: cfg.policy != EnginePolicy::Off,
        record_gate: cfg.record_gate,
    };
    let resolution = quarto_core::record::resolve_records(&walk_ctx, &inputs, &lookup).await;

    let (source, run) = match resolution.chosen {
        Some(quarto_core::record::Chosen::Run { source, run }) => (source, run),
        // The preview's engine step always returns a run to publish.
        Some(quarto_core::record::Chosen::EnginePublished) => return Ok(false),
        None => {
            // A refusal that nothing else overrode stands, naming the variable; an engine
            // error is the document's error. Otherwise nothing supplied (a prose-only
            // document, or sources that hold nothing): not an error.
            // Say on the page why each source passed (RIU-4); silent for prose-only documents,
            // and when a source failed (that failure is the page's one diagnostic).
            if resolution.first_failure().is_none()
                && let Some(diag) = quarto_core::engine::diagnostics::no_record_source_diagnostic(
                    &resolution.outcomes,
                    !inputs.sugared_blocks.is_empty(),
                )
                && let Some(sink) = crate::diagnostics::current_sink()
            {
                sink.emit(rel_path, diag);
            }
            if let Some(why) = resolution.refusal() {
                return Err(RecordError::Refused(why.to_string()));
            }
            if let Some(failure) = resolution.first_failure() {
                return Err(RecordError::RecordFailed(failure.to_string()));
            }
            return Ok(false);
        }
    };

    // A refused source that a later source replaced is reported on the page.
    if let Some(refused) = resolution.outcomes.iter().find_map(|o| match &o.kind {
        quarto_core::record::OutcomeKind::Refused(why) => Some((o.source.label(), why.clone())),
        _ => None,
    }) && let Some(sink) = crate::diagnostics::current_sink()
    {
        sink.emit(
            rel_path,
            quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                "The {} record was not used",
                refused.0
            ))
            .problem(refused.1)
            .build(),
        );
    }

    let staleness = source_staleness(&source, &run, &inputs);
    let capture_doc_id = write_capture_doc_gated(ctx, rel_path, &run, cfg.record_gate).await?;

    let capture_ref = CaptureRef {
        capture_doc_id,
        staleness: Some(staleness),
        state: None,
        last_error: None,
    };
    ctx.index()
        .set_capture(rel_path, &capture_ref)
        .map_err(|e| RecordError::SidecarFailed(format!("{}", e)))?;

    // An imported record has no captures, so name the record's engines then.
    let engines = if run.captures.is_empty() {
        run.record
            .iter()
            .flat_map(|r| r.metadata.quarto_engines.iter().map(|e| e.name.as_str()))
            .collect::<Vec<_>>()
    } else {
        run.captures
            .iter()
            .map(|c| c.engine_name.as_str())
            .collect::<Vec<_>>()
    }
    .join(", ");
    tracing::info!(
        rel_path = %rel_path,
        engines = %engines,
        "recorded engine capture(s)",
    );
    Ok(true)
}

/// What the sidecar's capture doc held, as far as staleness is concerned.
enum Recorded {
    /// The first engine's `input_qmd`, the legacy byte-compare key (bd-5yff4).
    Legacy(String),
    Record(Arc<Record>),
}

/// Recompute whether the active sidecar entry for `rel_path` is
/// stale (Phase C.2). Compares the file's current canonical QMD
/// against the recorded capture's `input_qmd` byte-for-byte (per
/// plan §Q-C3 v1 whole-QMD policy). On mismatch, flips
/// `CaptureRef.staleness` to `Some(true)`; on match, normalizes to
/// `Some(false)`.
///
/// No-op when:
///   - the path isn't in the project file index
///   - no sidecar entry exists for the path
///   - the recorded capture binary doc is missing or unparseable
///     (logged but not propagated — we don't want a corrupt cache
///     to spam the watcher with errors).
///
/// Returns `Ok(true)` when staleness flipped, `Ok(false)` when no
/// change was needed, and `Err` for fatal failures the caller
/// should log.
pub async fn recompute_staleness(
    ctx: Arc<HubContext>,
    runtime: Arc<dyn SystemRuntime>,
    rel_path: &str,
    engine_registry: Option<Arc<EngineRegistry>>,
    cfg: &ReExecuteConfig,
) -> Result<bool, RecordError> {
    // C.6: `preview.engine: off` skips the watcher staleness hook
    // entirely (no captures exist anyway, but defensive).
    if cfg.policy == EnginePolicy::Off {
        return Ok(false);
    }
    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        return Ok(false);
    };
    let Some(existing) = ctx.index().get_capture(rel_path) else {
        // No capture recorded for this file — nothing to invalidate.
        return Ok(false);
    };

    // Load and decode the existing capture sequence so we can compare
    // its input_qmd to what the file would produce now. bd-5yff4: the
    // staleness key is the *first* engine's input_qmd — the document's
    // canonical pre-engine QMD. Each later engine's input is a
    // deterministic function of the prior engine's output, so an
    // unchanged first input means the whole recorded sequence is still
    // valid.
    let recorded = match read_capture_from_doc(&ctx, &existing.capture_doc_id).await {
        Ok(CapturePayload::Legacy(captures)) => match captures.into_iter().next() {
            Some(first) => Recorded::Legacy(first.input_qmd),
            None => {
                tracing::warn!(
                    rel_path = %rel_path,
                    "recorded capture doc was empty; skipping staleness check",
                );
                return Ok(false);
            }
        },
        Ok(CapturePayload::Empty) => {
            tracing::warn!(
                rel_path = %rel_path,
                "recorded capture doc was empty; skipping staleness check",
            );
            return Ok(false);
        }
        Ok(CapturePayload::Record(record)) => Recorded::Record(record),
        Err(e) => {
            tracing::warn!(
                rel_path = %rel_path,
                error = %e,
                "could not read recorded capture for staleness check",
            );
            return Ok(false);
        }
    };

    let abs_path = project_root.join(rel_path);
    let project = ProjectContext::discover(&abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    let is_stale = match recorded {
        // Legacy payloads keep the byte compare: any prose edit flips it.
        Recorded::Legacy(recorded_input_qmd) => {
            let current_input_qmd = compute_input_qmd(&abs_path, &project, runtime)
                .await
                .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;
            current_input_qmd != recorded_input_qmd.as_bytes()
        }
        // Records compare the per-engine document hashes, recomputed from the record's
        // stored declarations on the masked, overlay-free pre-engine document (EXR-15,
        // decision 5). No engine is loaded.
        Recorded::Record(record) => {
            let source = compute_source_inputs(&abs_path, &project, runtime)
                .await
                .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;
            let input_qmd = String::from_utf8_lossy(&source.input_qmd);
            match record_staleness(
                &record,
                &input_qmd,
                &source.sugared_blocks,
                &source.metadata,
            ) {
                Ok(stale) => stale,
                Err(e) => {
                    tracing::warn!(
                        rel_path = %rel_path,
                        error = %e,
                        "could not apply the record's hash declarations; skipping staleness check",
                    );
                    return Ok(false);
                }
            }
        }
    };
    let target_value = Some(is_stale);
    if existing.staleness == target_value {
        return Ok(false);
    }

    let updated = CaptureRef {
        capture_doc_id: existing.capture_doc_id.clone(),
        staleness: target_value,
        state: existing.state,
        last_error: existing.last_error.clone(),
    };
    ctx.index()
        .set_capture(rel_path, &updated)
        .map_err(|e| RecordError::SidecarFailed(format!("{}", e)))?;

    tracing::debug!(
        rel_path = %rel_path,
        staleness = is_stale,
        "updated sidecar staleness",
    );

    // C.6 Auto policy: when staleness just flipped to true, kick off a
    // re-execute synchronously so the SPA sees a fresh capture without
    // requiring the user to click the overlay. We reuse the same path
    // the HTTP /api/preview/re-execute handler takes, including its
    // in-flight guard (so a debounced flurry of file changes is
    // collapsed to one engine run at a time per doc).
    if cfg.policy == EnginePolicy::Auto && is_stale {
        crate::re_execute::trigger_auto_re_execute(
            ctx.clone(),
            rel_path.to_string(),
            engine_registry,
            cfg.clone(),
        );
    }

    Ok(true)
}

/// Encode and store a capture run as a samod binary doc: a record doc when the
/// writer gate (`record_writing_enabled`, on by default for the preview) is on, the
/// run has a record, and every engine that ran is in it; else the legacy
/// `EngineCapture` array (bd-5yff4: one per engine), with `Q-18-15` when an engine
/// that builds no record is why. The gate decides only this hub payload: the local
/// cache writes its record file whatever the gate says. Returns the new
/// doc's stringified DocumentId for the sidecar `captureDocId` field. Shared by
/// the eager driver and the re-execute handler (EXR-13b).
///
/// `rel_path` is the project-relative source path, stamped (with the
/// engine names) into the doc's uncompressed `meta` audit map
/// (bd-eiku4ymo) so `hub admin scan` can read provenance without
/// gunzipping.
pub async fn write_capture_doc(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    run: &CaptureRun,
) -> Result<String, RecordError> {
    write_capture_doc_gated(
        ctx,
        rel_path,
        run,
        quarto_core::engine::capture_files::record_writing_enabled(
            quarto_core::engine::capture_files::WriterSurface::Preview,
        ),
    )
    .await
}

/// [`write_capture_doc`] with the gate passed explicitly (test seam).
pub async fn write_capture_doc_gated(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    run: &CaptureRun,
    record_gate: bool,
) -> Result<String, RecordError> {
    use quarto_core::engine::capture_files::{PublishError, encode_for_publish};
    let encoded = encode_for_publish(&run.captures, run.record.as_ref(), record_gate).map_err(
        |e| match e {
            PublishError::Io(e) => RecordError::Gzip(e.to_string()),
            refused @ PublishError::RecordWriterOff => RecordError::Refused(refused.to_string()),
        },
    )?;

    report_engines_without_record(rel_path, run, &encoded.engines_without_record);

    let meta = quarto_hub::resource::CaptureDocMeta {
        source_path: rel_path.to_string(),
        engines: encoded.engines,
    };
    quarto_hub::resource::publish_payload_doc(
        ctx.repo(),
        encoded.is_record,
        &encoded.gzipped,
        &meta,
    )
    .await
    .map_err(|e| match e {
        quarto_hub::resource::PublishDocError::Build(e) => {
            RecordError::CreateBinaryDoc(e.to_string())
        }
        quarto_hub::resource::PublishDocError::RepoStopped => RecordError::RepoStopped,
    })
}

/// Tell the page about a document stored in the legacy format because an engine in it
/// builds no execution record (`Q-18-15`, RIU-1). Called once per capture run, from the
/// one writer both the eager driver and re-execute go through, so a re-run replaces the
/// earlier diagnostic instead of stacking a second one. With nothing to report it still
/// drops a stale one: the edit that removed the record-less engine clears the warning.
fn report_engines_without_record(rel_path: &str, run: &CaptureRun, without: &[String]) {
    const CODE: &str = "Q-18-15";
    let Some(sink) = diagnostics::current_sink() else {
        return;
    };
    let mut kept = sink.get_for_page(rel_path);
    let before = kept.len();
    kept.retain(|d| d.code.as_deref() != Some(CODE));
    if kept.len() != before {
        sink.replace_page(rel_path, kept);
    }
    if without.is_empty() {
        return;
    }
    let with: Vec<String> = run
        .record
        .as_ref()
        .map(|r| {
            r.metadata
                .quarto_engines
                .iter()
                .map(|e| e.name.clone())
                .collect()
        })
        .unwrap_or_default();
    sink.emit(
        rel_path,
        quarto_core::engine::diagnostics::engines_without_record_diagnostic(&with, without),
    );
}

/// Errors the driver can surface. Each variant is a short message
/// suitable for `tracing::warn!` — the calling loop catches the
/// failure, logs, and moves on to the next file. We deliberately
/// don't propagate `PipelineError` shapes here because the calling
/// surface (file-watcher hook, on-ready callback) treats all
/// per-file failures uniformly.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("project discovery failed: {0}")]
    DiscoverFailed(String),
    #[error("engine capture pipeline failed: {0}")]
    RecordFailed(String),
    #[error("serializing/gzipping capture failed: {0}")]
    Gzip(String),
    #[error("creating capture binary doc failed: {0}")]
    CreateBinaryDoc(String),
    #[error("samod repo is stopped")]
    RepoStopped,
    /// The run is a record with no legacy form and the writer gate is off (RIU-2).
    #[error("{0}")]
    Refused(String),
    #[error("writing sidecar entry failed: {0}")]
    SidecarFailed(String),
}

/// Resolve the capture payload out of a binary doc by its document ID: a
/// legacy `EngineCapture[]` (bd-5yff4: one per engine) or an engine execution
/// record (EXR-13). The on-the-wire format (gzipped JSON inside a
/// `quarto_hub::resource::create_binary_document` envelope) is shared with
/// Phase C.4's WASM/SPA reader, so keeping the Rust reader public lets test
/// code and any future server-side inspector use the same path.
///
/// This fetches `content` and calls
/// [`parse_capture_payload`](quarto_core::pandoc_request::captures::parse_capture_payload),
/// the one reader that decides the shape (it never looks at the MIME).
pub async fn read_capture_from_doc(
    ctx: &Arc<HubContext>,
    capture_doc_id: &str,
) -> Result<CapturePayload, String> {
    use std::str::FromStr;

    use automerge::ROOT;
    use automerge::ReadDoc;
    use samod::DocumentId;

    let id = DocumentId::from_str(capture_doc_id).map_err(|e| format!("invalid doc id: {}", e))?;
    let handle = ctx
        .repo()
        .find(id)
        .await
        .map_err(|_stopped| "repo stopped".to_string())?
        .ok_or_else(|| "capture doc not found".to_string())?;

    let gzipped = handle.with_document(|doc| {
        doc.get(ROOT, "content")
            .ok()
            .flatten()
            .and_then(|(v, _)| v.into_bytes().ok())
            .ok_or_else(|| "binary doc missing 'content' field".to_string())
    })?;

    parse_capture_payload(Some(&gzipped), capture_doc_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EXR-13b writer gate (shared by the eager driver and the re-execute
    /// handler): gate off writes a legacy capture doc even when the run carries a
    /// record; gate on writes a record doc; gate on with no record (a cache hit)
    /// falls back to legacy.
    async fn written_mime_and_kind(gate: bool, with_record: bool) -> (String, String) {
        use automerge::ReadDoc;
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let record = with_record.then(|| {
            quarto_core::record::read_record(include_str!(
                "../../quarto-core/tests/fixtures/records/basic.json"
            ))
            .unwrap()
        });
        // An engine run always has captures; a run with a record and none has no legacy form.
        let run = CaptureRun {
            captures: vec![
                serde_json::from_value::<quarto_trace::EngineCapture>(serde_json::json!({
                    "engine_name": "jupyter", "input_qmd": "x", "result": {}
                }))
                .unwrap(),
            ],
            record,
        };
        let id = write_capture_doc_gated(&ctx, "doc.qmd", &run, gate)
            .await
            .unwrap();
        let handle = ctx
            .repo()
            .find(id.parse().unwrap())
            .await
            .unwrap()
            .expect("doc exists");
        handle.with_document(|doc| {
            let mime = doc
                .get(automerge::ROOT, "mimeType")
                .unwrap()
                .and_then(|(v, _)| v.into_string().ok())
                .unwrap_or_default();
            let kind = quarto_hub::resource::read_capture_meta(doc)
                .and_then(|m| m.kind)
                .unwrap_or_default();
            (mime, kind)
        })
    }

    /// A run whose record is the `basic` fixture's and whose captures come from the
    /// given engines (RIU-1 record-less-engine rule).
    fn run_with_captures(engines: &[&str]) -> CaptureRun {
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let captures = engines
            .iter()
            .map(|e| {
                serde_json::from_value(serde_json::json!({
                    "engine_name": e, "input_qmd": "x", "result": {}
                }))
                .unwrap()
            })
            .collect();
        CaptureRun {
            captures,
            record: Some(record),
        }
    }

    async fn fresh_ctx() -> (TempDir, Arc<HubContext>) {
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        (temp, ctx)
    }

    /// RIU-1: a document with an engine outside the record is stored whole in the legacy
    /// format, and the page gets exactly one `Q-18-15` warning per capture run, even when
    /// the run is written twice (a re-execute replaces it).
    #[tokio::test]
    async fn recordless_engine_stores_legacy_and_warns_once_per_page() {
        let (_t, ctx) = fresh_ctx().await;
        let sink = Arc::new(diagnostics::DiagnosticSink::new());
        diagnostics::set_sink(sink.clone());
        let record_engine = run_with_captures(&[])
            .record
            .unwrap()
            .metadata
            .quarto_engines[0]
            .name
            .clone();
        let run = run_with_captures(&[&record_engine, "echo"]);

        for _ in 0..2 {
            let id = write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
                .await
                .unwrap();
            let payload = read_capture_from_doc(&ctx, &id).await.unwrap();
            assert_eq!(
                payload.into_captures().len(),
                2,
                "both engines' captures are stored in the legacy array"
            );
        }

        let diags = sink.get_for_page("doc.qmd");
        assert_eq!(diags.len(), 1, "one warning, not stacked: {diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-18-15"));
        let text = diags[0].to_text(None);
        assert!(text.contains("\"echo\""), "{text}");
        assert!(text.contains(&record_engine), "{text}");
    }

    /// RIU-1: with every capture's engine in the record there is nothing to say.
    #[tokio::test]
    async fn no_warning_when_every_engine_is_in_the_record() {
        let (_t, ctx) = fresh_ctx().await;
        let sink = Arc::new(diagnostics::DiagnosticSink::new());
        diagnostics::set_sink(sink.clone());
        let record_engine = run_with_captures(&[])
            .record
            .unwrap()
            .metadata
            .quarto_engines[0]
            .name
            .clone();
        let run = run_with_captures(&[&record_engine]);

        write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
            .await
            .unwrap();

        assert!(sink.get_for_page("doc.qmd").is_empty());
    }

    #[tokio::test]
    async fn writer_gate_off_writes_a_legacy_capture_doc() {
        let (mime, kind) = written_mime_and_kind(false, true).await;
        assert_eq!(mime, quarto_hub::resource::CAPTURE_MIME_TYPE);
        assert_eq!(kind, "engine-capture");
    }

    #[tokio::test]
    async fn writer_gate_on_writes_a_record_doc() {
        let (mime, kind) = written_mime_and_kind(true, true).await;
        assert_eq!(mime, quarto_hub::resource::RECORD_MIME_TYPE);
        assert_eq!(kind, "engine-record");
    }

    /// EXR-15: the record doc's `meta.engines` is the record's engines list, in order, and the
    /// payload parses back to an equal record.
    #[tokio::test]
    async fn record_doc_meta_and_payload_match_the_record() {
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let expected: Vec<String> = record
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let run = CaptureRun {
            captures: Vec::new(),
            record: Some(record.clone()),
        };
        let id = write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
            .await
            .unwrap();
        let handle = ctx
            .repo()
            .find(id.parse().unwrap())
            .await
            .unwrap()
            .expect("doc exists");
        let engines = handle.with_document(|doc| {
            quarto_hub::resource::read_capture_meta(doc)
                .map(|m| m.engines)
                .unwrap_or_default()
        });
        assert_eq!(engines, expected);
        let CapturePayload::Record(back) = read_capture_from_doc(&ctx, &id).await.unwrap() else {
            panic!("expected a record payload");
        };
        assert_eq!(*back, record);
    }

    /// RIU-2: a record that describes something, with no captures (an imported one), has no
    /// legacy form. With the gate off the publish seam refuses and names the variable, rather
    /// than writing an empty legacy list that shows an empty page.
    #[tokio::test]
    async fn gate_off_refuses_a_record_with_no_legacy_form() {
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let run = CaptureRun {
            captures: Vec::new(),
            record: Some(record),
        };
        let err = write_capture_doc_gated(&ctx, "doc.qmd", &run, false)
            .await
            .unwrap_err();
        assert!(matches!(err, RecordError::Refused(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains(quarto_core::engine::capture_files::RECORD_WRITING_ENV),
            "{err}"
        );
        // With the gate on the same run publishes a record doc.
        write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn writer_gate_on_without_a_record_writes_legacy() {
        let (mime, _) = written_mime_and_kind(true, false).await;
        assert_eq!(mime, quarto_hub::resource::CAPTURE_MIME_TYPE);
    }

    use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError};
    use quarto_hub::context::HubConfig;
    use quarto_hub::storage::StorageManager;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    /// Passthrough engine reporting as a non-markdown name so it
    /// bypasses EngineExecutionStage's `name() == "markdown"`
    /// short-circuit and triggers capture emission.
    struct PassthroughTestEngine;

    impl ExecutionEngine for PassthroughTestEngine {
        fn name(&self) -> &str {
            "test-passthrough"
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            let mut out = String::from(input);
            out.push_str("\n<!-- test-passthrough -->\n");
            Ok(ExecuteResult::passthrough(&out))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-passthrough" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn make_registry() -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(PassthroughTestEngine));
        Arc::new(reg)
    }

    // ──────────────────────────────────────────────────────────────
    // R5 seam tests (P2-14 / P2-15): the preview capture path runs a
    // real *extension* engine (the Task-13 echo TS engine) because
    // record_capture uses the DISCOVERED project.registry (Task 8),
    // not built-ins. Node-gated — the echo engine spawns a Node
    // engine-host subprocess (see crates/quarto-core tests/integration
    // /echo_engine_e2e.rs).
    // ──────────────────────────────────────────────────────────────

    use quarto_core::engine::ts_process::node_available_for_tests;

    /// Recursively copy `src` into `dst` (dst is created).
    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else {
                std::fs::copy(&from, &to).unwrap();
            }
        }
    }

    /// Absolute path to the committed echo-engine fixture extension
    /// (lives under the sibling `quarto-core` crate's test fixtures).
    fn echo_engine_fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quarto-core/tests/fixtures/extensions/echo-engine")
    }

    /// Build a HubContext over a temp project that has the echo
    /// extension installed under `_extensions/echo-engine/` and a single
    /// `.qmd` at `rel` with `content`. `ProjectContext::discover` (run
    /// inside `record_one` / `recompute_staleness`) walks up to
    /// `_extensions/` and builds the echo engine into `project.registry`.
    async fn build_ctx_with_echo_extension(
        rel: &str,
        content: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let project = TempDir::with_prefix("r5-echo-preview-").unwrap();
        let echo_dst = project.path().join("_extensions").join("echo-engine");
        copy_dir(&echo_engine_fixture_dir(), &echo_dst);
        // The committed echo-engine bundle is deleted (plan1c3: hermetic fixtures
        // are regenerated at test time). These tests execute the engine, so
        // regenerate the real bundle in-place via the same public build lib the
        // quarto-core suites use (the embedded rollup driver, run under Node). Mirrors
        // `engine_fixture_build::build_bundle` — the `../../resources/...` depth is
        // identical because quarto-preview and quarto-core are both one level under
        // `crates/`.
        quarto_core::extension::build::build_ts_extension(
            quarto_core::extension::build::BuildOptions {
                ext_dir: Some(echo_dst.clone()),
                config: None,
                workspace: false,
                workspace_root: Some(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .canonicalize()
                        .expect("workspace root"),
                ),
            },
        )
        .expect("regenerate echo-engine bundle for preview capture test");
        let doc_path = project.path().join(rel);
        if let Some(parent) = doc_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&doc_path, content).unwrap();

        let storage = StorageManager::new(project.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        (project, ctx, runtime)
    }

    /// P2-14 — the EAGER preview capture path runs the echo *extension*
    /// engine server-side. Production passes `engine_registry: None`, so
    /// `record_capture` uses the discovered `project.registry` (which,
    /// post-Task-7b, contains the echo engine built from the installed
    /// `_extensions/echo-engine/`). The recorded capture's result
    /// markdown must contain the engine's `ECHO_EXECUTED` sentinel.
    ///
    /// Fail-on-revert (recorded in the Task-15 report): overwrite
    /// `ctx.registry` with a built-ins `EngineRegistry::new()` right
    /// after `StageContext::new()` in `record_capture` ⇒ echo absent ⇒
    /// `ECHO_EXECUTED` missing ⇒ RED. This binds "the eager preview
    /// capture uses the project registry (with extension engines)".
    #[tokio::test]
    async fn p2_14_eager_capture_runs_extension_engine() {
        if !node_available_for_tests() {
            eprintln!("SKIP: node not found — p2_14_eager_capture_runs_extension_engine");
            return;
        }
        let (_tmp, ctx, runtime) = build_ctx_with_echo_extension(
            "doc.qmd",
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nHello from preview!\n```\n",
        )
        .await;

        // engine_registry = None ⇒ the production path: the discovered
        // project.registry (with the echo extension engine) is used.
        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "echo doc should produce exactly one capture");

        let entry = ctx.index().get_capture("doc.qmd").expect("sidecar entry");
        let captures = read_capture_from_doc(&ctx, &entry.capture_doc_id)
            .await
            .expect("capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].engine_name, "echo",
            "the extension engine (echo), not built-in markdown, must own the capture"
        );
        let markdown = captures[0]
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("capture result.markdown");
        assert!(
            markdown.contains("ECHO_EXECUTED"),
            "eager preview capture must run the echo extension engine server-side \
             (ECHO_EXECUTED sentinel); got: {markdown}"
        );
    }

    /// P2-15 — the echo extension engine survives an ON-EDIT
    /// re-execution (capture sites 2/3: `recompute_staleness` Auto →
    /// `trigger_auto_re_execute` → `perform_re_execute`). After the
    /// eager capture, a changed cell body forces a cache miss and a
    /// genuinely fresh capture; the new capture must STILL contain
    /// `ECHO_EXECUTED`, and — the vacuity guard — must reflect the
    /// edited body (`SECOND`) with a new `captureDocId`, proving a REAL
    /// re-execute happened rather than a re-assertion of the P2-14
    /// eager capture.
    ///
    /// RED (fail-on-revert): if sites 2/3 fell back to a built-ins
    /// registry, the echo engine would vanish on edit ⇒ the
    /// post-re-execute `ECHO_EXECUTED` assertion goes RED.
    #[tokio::test]
    async fn p2_15_on_edit_re_execution_keeps_extension_engine() {
        if !node_available_for_tests() {
            eprintln!("SKIP: node not found — p2_15_on_edit_re_execution_keeps_extension_engine");
            return;
        }
        crate::re_execute::reset_in_flight_for_tests();

        // One shared cache dir across eager + re-execute so the cache is
        // real: the edited body must MISS it (proving a genuine re-run),
        // not silently reuse the eager capture.
        let cache_dir = cache_dir_for_test();

        let (tmp, ctx, runtime) = build_ctx_with_echo_extension(
            "doc.qmd",
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nFIRST\n```\n",
        )
        .await;

        // Eager capture (production path, None registry) seeds the sidecar.
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir.clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "eager echo capture must seed the sidecar");
        let initial_doc_id = ctx
            .index()
            .get_capture("doc.qmd")
            .unwrap()
            .capture_doc_id
            .clone();

        // Edit the cell body ⇒ different canonical QMD ⇒ cache miss ⇒
        // real re-execute.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nSECOND\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Auto, cache_dir.clone()),
        )
        .await
        .unwrap();
        assert!(flipped, "editing the cell must flip staleness");

        // Auto spawned a blocking re-execute worker; poll for the fresh
        // capture (state: idle, new captureDocId).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let new_entry = loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(quarto_hub::index::CaptureState::Idle)
                && entry.capture_doc_id != initial_doc_id
            {
                break entry;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "Auto re-execute did not produce a new capture within 30s; entry = {entry:?}"
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert_eq!(
            new_entry.staleness,
            Some(false),
            "Auto re-execute must clear staleness"
        );

        let captures = read_capture_from_doc(&ctx, &new_entry.capture_doc_id)
            .await
            .expect("re-executed capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].engine_name, "echo");
        // Vacuity guard: the re-executed capture reflects the EDITED body
        // — a genuine cache-miss re-run, not the eager capture replayed.
        assert!(
            captures[0].input_qmd.contains("SECOND"),
            "re-executed capture must reflect the edited cell body (SECOND); got: {}",
            captures[0].input_qmd
        );
        let markdown = captures[0]
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("capture result.markdown");
        assert!(
            markdown.contains("ECHO_EXECUTED"),
            "the echo extension engine must STILL run after an on-edit re-execute \
             (sites 2/3 use project.registry); got: {markdown}"
        );
    }

    /// Each test invocation gets a process-unique cache directory so
    /// cross-test pollution can't accidentally turn a cache miss into
    /// a hit (or vice versa). The OS cleans up on process exit; we
    /// don't bother with TempDir RAII because the driver tests don't
    /// depend on the dir being gone before they finish.
    fn cache_dir_for_test() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let mut dir = std::env::temp_dir();
        dir.push(format!("q2-c7-driver-test-{pid}-{id}"));
        dir
    }

    async fn build_ctx_with_files(
        files: &[(&str, &str)],
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        build_ctx_with_text_and_binary_files(files, &[]).await
    }

    async fn build_ctx_with_text_and_binary_files(
        text_files: &[(&str, &str)],
        binary_files: &[(&str, &[u8])],
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let project = TempDir::with_prefix("c1-driver-test-").unwrap();
        for (rel, content) in text_files {
            let path = project.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        for (rel, bytes) in binary_files {
            let path = project.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, bytes).unwrap();
        }
        let storage = StorageManager::new(project.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        (project, ctx, runtime)
    }

    #[tokio::test]
    async fn standalone_mode_records_nothing() {
        let temp = TempDir::with_prefix("c1-standalone-").unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let count = record_eager_captures(
            ctx,
            runtime,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    /// RIU-2: a cache entry holding only a record (what an import stores) is published through
    /// `record_one`, with no engine and no registry. Before, the `captures.is_empty()` guard
    /// dropped it and the page showed nothing.
    #[tokio::test]
    async fn a_record_only_cache_entry_is_published() {
        let doc = "---\ntitle: T\n---\n\n```{python}\n1 + 1\n```\n";
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", doc)]).await;
        let abs = tmp.path().join("doc.qmd");
        let cache_dir = cache_dir_for_test();
        let project = ProjectContext::discover(&abs, runtime.as_ref()).unwrap();
        let input_qmd =
            quarto_core::engine::preview_record::compute_input_qmd(&abs, &project, runtime.clone())
                .await
                .unwrap();
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        crate::cache::store_run(
            &cache_dir,
            &crate::cache::compute_key(&input_qmd),
            &CaptureRun {
                captures: Vec::new(),
                record: Some(record),
            },
        );

        let wrote = record_one(
            &abs,
            "doc.qmd",
            &ctx,
            &runtime,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir.clone()).with_gate(true),
        )
        .await
        .unwrap();
        assert!(wrote, "a record-only run is published");
        assert!(ctx.index().has_capture("doc.qmd"));

        // With the gate off the same run has no legacy form: refused, nothing published.
        ctx.index().remove_capture("doc.qmd").ok();
        let err = record_one(
            &abs,
            "doc.qmd",
            &ctx,
            &runtime,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir.clone()),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RecordError::Refused(_)), "{err:?}");
    }

    // ── RIU-2: record_one walks the source list ──────────────────────────

    const WALK_DOC: &str = "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n";

    struct CountingPassthrough(Arc<std::sync::atomic::AtomicUsize>);
    impl ExecutionEngine for CountingPassthrough {
        fn name(&self) -> &str {
            "test-passthrough"
        }
        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ExecuteResult::passthrough(input))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-passthrough" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn counting_registry() -> (Arc<EngineRegistry>, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(CountingPassthrough(calls.clone())));
        (Arc::new(reg), calls)
    }

    fn basic_record() -> quarto_core::record::Record {
        quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap()
    }

    /// Put `record` in the preview cache under the key of `abs`'s current content.
    async fn seed_cache(
        abs: &std::path::Path,
        runtime: &Arc<dyn SystemRuntime>,
        cache_dir: &std::path::Path,
        record: quarto_core::record::Record,
    ) {
        let project = ProjectContext::discover(abs, runtime.as_ref()).unwrap();
        let input_qmd =
            quarto_core::engine::preview_record::compute_input_qmd(abs, &project, runtime.clone())
                .await
                .unwrap();
        crate::cache::store_run(
            cache_dir,
            &crate::cache::compute_key(&input_qmd),
            &CaptureRun {
                captures: Vec::new(),
                record: Some(record),
            },
        );
    }

    fn cfg_for(
        cache_dir: &std::path::Path,
        policy: EnginePolicy,
        sources: Vec<quarto_core::record::RecordSource>,
        gate: bool,
    ) -> ReExecuteConfig {
        let mut c = ReExecuteConfig::new(policy, cache_dir.to_path_buf()).with_gate(gate);
        c.sources = quarto_core::record::RecordSources::new(sources);
        c
    }

    /// The `q2` source: a file at `.quarto/execution-records/<doc>.ipynb` supplies the record
    /// with the cache empty, no engine runs, and the staleness comes from the source (the
    /// record's hashes do not match this document, so the page is flagged stale).
    #[tokio::test]
    async fn the_q2_source_supplies_a_record_with_no_engine_run() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let project = ProjectContext::discover(&abs, runtime.as_ref()).unwrap();
        let file = quarto_core::record::execution_record_path(&project.dir, &abs);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            quarto_core::record::write_record(&basic_record()).unwrap(),
        )
        .unwrap();
        let (registry, calls) = counting_registry();
        let cfg = cfg_for(
            &cache_dir_for_test(),
            EnginePolicy::Manual,
            vec![RecordSource::Cache, RecordSource::Q2, RecordSource::Engine],
            true,
        );

        let wrote = record_one(&abs, "doc.qmd", &ctx, &runtime, Some(registry), &cfg)
            .await
            .unwrap();
        assert!(wrote);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let entry = ctx.index().get_capture("doc.qmd").unwrap();
        assert_eq!(
            entry.staleness,
            Some(true),
            "staleness comes from the supplying source, not a constant"
        );
    }

    /// Write a `q2` record of `doc` (built by the recording stub engine) for `abs`.
    async fn write_q2_record_of(
        abs: &std::path::Path,
        runtime: &Arc<dyn SystemRuntime>,
    ) -> ProjectContext {
        let project = ProjectContext::discover(abs, runtime.as_ref()).unwrap();
        let run = quarto_core::engine::preview_record::record_capture_run(
            abs,
            &project,
            runtime.clone(),
            Some(recording_registry(false)),
        )
        .await
        .unwrap();
        let file = quarto_core::record::execution_record_path(&project.dir, abs);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            quarto_core::record::write_record(&run.record.unwrap()).unwrap(),
        )
        .unwrap();
        project
    }

    /// RIU-4: a `q2` record written for the current document is published fresh; one written for
    /// an older document (a code cell since edited) is published stale.
    #[tokio::test]
    async fn a_q2_record_is_fresh_for_the_current_document_and_stale_for_an_older_one() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", RECORD_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        write_q2_record_of(&abs, &runtime).await;
        let cfg = cfg_for(
            &cache_dir_for_test(),
            EnginePolicy::Manual,
            vec![RecordSource::Q2],
            true,
        );

        assert!(
            record_one(&abs, "doc.qmd", &ctx, &runtime, None, &cfg)
                .await
                .unwrap()
        );
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "the record matches the document"
        );

        std::fs::write(&abs, RECORD_DOC.replace("\nx\n", "\ny\n")).unwrap();
        assert!(
            record_one(&abs, "doc.qmd", &ctx, &runtime, None, &cfg)
                .await
                .unwrap()
        );
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "the record is of an older document"
        );
    }

    /// RIU-4: sources are read at boot and not again. After the `q2` record is published, a file
    /// change recomputes staleness from the capture doc even when the source file is gone or
    /// replaced with garbage, and a fresh source reading would have failed.
    #[tokio::test]
    async fn a_file_change_does_not_consult_the_source_list() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", RECORD_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let project = write_q2_record_of(&abs, &runtime).await;
        let cfg = cfg_for(
            &cache_dir_for_test(),
            EnginePolicy::Manual,
            vec![RecordSource::Q2],
            true,
        );
        record_one(&abs, "doc.qmd", &ctx, &runtime, None, &cfg)
            .await
            .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false)
        );

        // The source goes bad, and the document changes.
        let file = quarto_core::record::execution_record_path(&project.dir, &abs);
        std::fs::write(&file, "not json").unwrap();
        std::fs::write(&abs, RECORD_DOC.replace("\nx\n", "\ny\n")).unwrap();

        recompute_staleness(ctx.clone(), runtime.clone(), "doc.qmd", None, &cfg)
            .await
            .unwrap();
        let entry = ctx.index().get_capture("doc.qmd").unwrap();
        assert_eq!(
            entry.staleness,
            Some(true),
            "judged from the published capture"
        );
        assert!(
            entry.last_error.is_none(),
            "the broken source was never read: {:?}",
            entry.last_error
        );
    }

    /// An unreadable `.quarto` file is skipped and the next source is tried.
    #[tokio::test]
    async fn an_unreadable_q2_file_is_skipped_and_the_next_source_is_tried() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let project = ProjectContext::discover(&abs, runtime.as_ref()).unwrap();
        let file = quarto_core::record::execution_record_path(&project.dir, &abs);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "not json").unwrap();
        let (registry, calls) = counting_registry();
        let cfg = cfg_for(
            &cache_dir_for_test(),
            EnginePolicy::Manual,
            vec![RecordSource::Q2, RecordSource::Engine],
            false,
        );
        let wrote = record_one(&abs, "doc.qmd", &ctx, &runtime, Some(registry), &cfg)
            .await
            .unwrap();
        assert!(wrote, "the engine supplied after the bad file");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A record the gate-off writer refuses falls through to the next source.
    #[tokio::test]
    async fn a_refused_cache_record_falls_through_to_the_engine() {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let cache_dir = cache_dir_for_test();
        seed_cache(&abs, &runtime, &cache_dir, basic_record()).await;
        let (registry, calls) = counting_registry();
        let cfg = cfg_for(
            &cache_dir,
            EnginePolicy::Manual,
            quarto_core::record::RecordSources::preview_default()
                .iter()
                .cloned()
                .collect(),
            false,
        );
        let wrote = record_one(&abs, "doc.qmd", &ctx, &runtime, Some(registry), &cfg)
            .await
            .unwrap();
        assert!(wrote);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// With the refused record the last source that could supply, the error stands.
    #[tokio::test]
    async fn a_refusal_stands_when_nothing_else_supplies() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let cache_dir = cache_dir_for_test();
        seed_cache(&abs, &runtime, &cache_dir, basic_record()).await;
        let cfg = cfg_for(
            &cache_dir,
            EnginePolicy::Manual,
            vec![RecordSource::Cache],
            false,
        );
        let err = record_one(&abs, "doc.qmd", &ctx, &runtime, None, &cfg)
            .await
            .unwrap_err();
        assert!(matches!(err, RecordError::Refused(_)), "{err:?}");
    }

    /// `preview.engine: off` still consults the sources: a cached record shows and the
    /// engine is never run (the lookup sits outside the old `off` early return).
    #[tokio::test]
    async fn policy_off_shows_a_cached_record_and_never_runs_the_engine() {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let cache_dir = cache_dir_for_test();
        seed_cache(&abs, &runtime, &cache_dir, basic_record()).await;
        let (registry, calls) = counting_registry();
        let cfg = ReExecuteConfig::new(EnginePolicy::Off, cache_dir).with_gate(true);
        let count = record_eager_captures(ctx.clone(), runtime, Some(registry), &cfg)
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(ctx.index().has_capture("doc.qmd"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// RIU-4: `execute: record-sources: cache,positron,engine` under `preview.engine: off`: the engine stays
    /// listed but never runs, and the cached record still shows.
    #[tokio::test]
    async fn off_with_engine_listed_shows_the_cached_record_and_never_runs_the_engine() {
        use quarto_core::record::RecordSource;
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let cache_dir = cache_dir_for_test();
        seed_cache(&abs, &runtime, &cache_dir, basic_record()).await;
        let (registry, calls) = counting_registry();
        let cfg = cfg_for(
            &cache_dir,
            EnginePolicy::Off,
            vec![
                RecordSource::Cache,
                RecordSource::Positron,
                RecordSource::Engine,
            ],
            true,
        );
        assert!(
            record_one(&abs, "doc.qmd", &ctx, &runtime, Some(registry), &cfg)
                .await
                .unwrap()
        );
        assert!(ctx.index().has_capture("doc.qmd"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// `off` with nothing cached shows nothing and never runs the engine.
    #[tokio::test]
    async fn policy_off_with_no_record_runs_nothing() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", WALK_DOC)]).await;
        let (registry, calls) = counting_registry();
        let cfg = ReExecuteConfig::new(EnginePolicy::Off, cache_dir_for_test());
        let count = record_eager_captures(ctx.clone(), runtime, Some(registry), &cfg)
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(!ctx.index().has_capture("doc.qmd"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // ── RIU-2: reuse across a restart (RIU-0 S1 B3, S2b) ─────────────────

    /// One session over a persisted store: seed, then drop everything so `hub.lock` is free.
    async fn session(
        project: &std::path::Path,
        data: &std::path::Path,
        cfg: &ReExecuteConfig,
    ) -> (Arc<HubContext>, usize) {
        let storage = StorageManager::new_with_data_dir(project, data).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let (registry, calls) = crate::test_support::counting_recording_registry();
        record_eager_captures(ctx.clone(), runtime, Some(registry), cfg)
            .await
            .unwrap();
        (ctx, calls.load(std::sync::atomic::Ordering::SeqCst))
    }

    fn restart_cfg(data: &std::path::Path) -> ReExecuteConfig {
        ReExecuteConfig::new(EnginePolicy::Manual, data.join("captures")).with_gate(true)
    }

    /// With a persisted store a prose edit while stopped reuses the record through the
    /// `CaptureRef`: no engine run and no stale flag.
    #[tokio::test]
    async fn a_prose_edit_while_stopped_reuses_the_record_through_the_ref() {
        let project = TempDir::with_prefix("riu2-restart-project-").unwrap();
        let data = TempDir::with_prefix("riu2-restart-data-").unwrap();
        let doc = project.path().join("doc.qmd");
        std::fs::write(&doc, crate::test_support::RECORDING_DOC).unwrap();
        let cfg = restart_cfg(data.path());

        let (ctx, first_calls) = session(project.path(), data.path(), &cfg).await;
        assert_eq!(first_calls, 1, "first open runs the engine once");
        drop(ctx);

        std::fs::write(
            &doc,
            format!(
                "{}\nA prose edit while stopped.\n",
                crate::test_support::RECORDING_DOC
            ),
        )
        .unwrap();
        let (ctx, calls) = session(project.path(), data.path(), &cfg).await;
        assert_eq!(calls, 0, "the ref carries reuse across the restart");
        let entry = ctx.index().get_capture("doc.qmd").expect("ref persisted");
        assert_eq!(entry.staleness, Some(false), "a prose edit is not stale");
    }

    /// With no ref (a cache copied into a fresh data dir) a prose edit misses the
    /// content-keyed cache and runs the engine once. This documents RIU-0 S1 B3 so a later
    /// change to it is deliberate.
    #[tokio::test]
    async fn without_a_ref_a_prose_edit_misses_the_content_keyed_cache() {
        let project = TempDir::with_prefix("riu2-noref-project-").unwrap();
        let data1 = TempDir::with_prefix("riu2-noref-data1-").unwrap();
        let data2 = TempDir::with_prefix("riu2-noref-data2-").unwrap();
        let doc = project.path().join("doc.qmd");
        std::fs::write(&doc, crate::test_support::RECORDING_DOC).unwrap();

        let (ctx, first_calls) =
            session(project.path(), data1.path(), &restart_cfg(data1.path())).await;
        assert_eq!(first_calls, 1);
        drop(ctx);

        // Copy the cache into a fresh data dir: the cache survives, the ref does not.
        std::fs::create_dir_all(data2.path().join("captures")).unwrap();
        for entry in std::fs::read_dir(data1.path().join("captures")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(
                entry.path(),
                data2.path().join("captures").join(entry.file_name()),
            )
            .unwrap();
        }

        // Unedited: a hit, no engine.
        let (ctx, calls) = session(project.path(), data2.path(), &restart_cfg(data2.path())).await;
        assert_eq!(calls, 0, "the same content hits the copied cache");
        drop(ctx);

        // Edited: a different key, a miss, one engine run.
        let data3 = TempDir::with_prefix("riu2-noref-data3-").unwrap();
        std::fs::create_dir_all(data3.path().join("captures")).unwrap();
        for entry in std::fs::read_dir(data1.path().join("captures")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(
                entry.path(),
                data3.path().join("captures").join(entry.file_name()),
            )
            .unwrap();
        }
        std::fs::write(
            &doc,
            format!("{}\nA prose edit.\n", crate::test_support::RECORDING_DOC),
        )
        .unwrap();
        let (_ctx, calls) = session(project.path(), data3.path(), &restart_cfg(data3.path())).await;
        assert_eq!(calls, 1, "a prose edit with no ref re-runs the engine once");
    }

    #[tokio::test]
    async fn prose_only_doc_records_no_capture() {
        let (_tmp, ctx, runtime) =
            build_ctx_with_files(&[("doc.qmd", "---\ntitle: Prose\n---\n\nNo cells.\n")]).await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
        assert!(!ctx.index().has_capture("doc.qmd"));
    }

    /// Regression test for bd-i6jy4: the eager-capture driver must
    /// only iterate `.qmd` files. Before the fix it iterated every
    /// entry in the index — including binaries — and fed their bytes
    /// to the parse-document stage, which panicked on the first
    /// non-UTF-8 byte (sibling bd-6qbto).
    ///
    /// The test puts *valid-qmd-with-engine-cells* content into a
    /// `.png` file so that, on the unfixed driver, the passthrough
    /// engine would actually run and a capture would be recorded for
    /// `logo.png`. The fix makes discovery's binary classification
    /// authoritative: `.png` lives in `binary_files`, never reaches
    /// `record_one`, and `has_capture("logo.png")` stays false.
    #[tokio::test]
    async fn binary_files_are_not_fed_to_parse_pipeline() {
        let engine_cell_bytes =
            b"---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n";

        let (_tmp, ctx, runtime) = build_ctx_with_text_and_binary_files(
            &[("doc.qmd", "---\ntitle: Prose\n---\n\nNo cells.\n")],
            &[("logo.png", engine_cell_bytes.as_slice())],
        )
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();

        assert_eq!(
            count, 0,
            "no captures expected: doc.qmd is prose-only, logo.png is a binary that must be skipped",
        );
        assert!(
            !ctx.index().has_capture("logo.png"),
            "binary files must never have engine captures recorded — bd-i6jy4",
        );
        assert!(
            !ctx.index().has_capture("doc.qmd"),
            "prose-only qmd records no capture (sanity)",
        );
    }

    #[tokio::test]
    async fn doc_with_passthrough_engine_records_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert!(ctx.index().has_capture("doc.qmd"));

        let entry = ctx.index().get_capture("doc.qmd").expect("sidecar entry");
        assert!(
            !entry.capture_doc_id.is_empty(),
            "captureDocId should be set"
        );
        assert_eq!(entry.staleness, Some(false));
    }

    /// bd-6d2wj4zp Phase 5 (S5): `.md` inputs never execute engines —
    /// even with an explicit engine spec and code cells. Now that the
    /// hub syncs `.md` into `ProjectFiles::qmd_files` (D10), the eager
    /// capture walk must skip them, or preview would execute engines
    /// the render path refuses to run (Q-2-40 warns and skips there).
    #[tokio::test]
    async fn md_doc_with_engine_spec_records_no_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "notes.md",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        // Non-vacuity: the .md must actually be in the synced source set
        // (D10) — otherwise this test would pass for the wrong reason.
        assert!(
            ctx.project_files()
                .expect("project mode")
                .qmd_files
                .contains(&std::path::PathBuf::from("notes.md")),
            "notes.md should be discovered into the hub source set"
        );

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 0, ".md must be skipped by the capture walk (S5)");
        assert!(!ctx.index().has_capture("notes.md"));
    }

    #[tokio::test]
    async fn capture_binary_doc_round_trips_through_samod() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 1);

        let entry = ctx.index().get_capture("doc.qmd").unwrap();
        let captures = read_capture_from_doc(&ctx, &entry.capture_doc_id)
            .await
            .expect("capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        let capture = &captures[0];

        assert_eq!(capture.engine_name, "test-passthrough");
        assert!(
            capture.input_qmd.contains("SENTINEL"),
            "input_qmd should preserve the cell body; got: {}",
            capture.input_qmd
        );
        let markdown = capture
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("result.markdown");
        assert!(markdown.contains("<!-- test-passthrough -->"));
    }

    /// PC3 — a failing engine on one doc must not stop the eager loop
    /// from recording the NEXT doc's capture, and must emit
    /// `Q-PREVIEW-CAP-1` to the diagnostic sink for the failing doc.
    ///
    /// Mock boundary: only the failing engine is a test double (mirrors
    /// `FailingTestEngine` in `diagnostics_capture_failure.rs`); the
    /// driver and samod context are real, pattern-matched off
    /// `capture_binary_doc_round_trips_through_samod` above. `qmd_files`
    /// is sorted (`discovery.rs:154`), so the `a-`/`b-` filename prefixes
    /// guarantee the failing doc is processed BEFORE the working one —
    /// otherwise a loop-return-on-Err regression could slip through
    /// undetected if the working doc happened to run first.
    ///
    /// Fail-on-revert binding (see task-p3-report.md for the transcript):
    /// (a) commenting out the `sink.emit(...)` call turns the
    /// diagnostics-count assertion RED (diagnostic absent);
    /// (b) replacing the loop's `Err(e) => { ... }` arm with
    /// `Err(e) => return Err(RecordError::RecordFailed(format!("{e}")))`
    /// turns the doc-B assertions RED (loop stops at the first failure,
    /// B's capture is never recorded).
    struct FailingTestEngine;
    impl ExecutionEngine for FailingTestEngine {
        fn name(&self) -> &str {
            "test-failing"
        }
        fn execute(
            &self,
            _input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            Err(ExecutionError::Other(
                "synthetic PC3 failing engine failure".to_string(),
            ))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-failing" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn make_registry_with_failing_and_passthrough() -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(FailingTestEngine));
        reg.register(Arc::new(PassthroughTestEngine));
        Arc::new(reg)
    }

    #[tokio::test]
    async fn pc3_failing_engine_does_not_block_next_doc_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[
            (
                "a-failing.qmd",
                "---\nengine: test-failing\n---\n\n```{test-failing}\nboom\n```\n",
            ),
            (
                "b-echo.qmd",
                "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
            ),
        ])
        .await;

        // Fresh sink for this test process (nextest runs each test in
        // its own process, so this is the first writer — see
        // diagnostics.rs's `set_sink` doc comment).
        let sink = Arc::new(diagnostics::DiagnosticSink::new());
        diagnostics::set_sink(sink.clone());

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry_with_failing_and_passthrough()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            count, 1,
            "only b-echo.qmd should have recorded a capture (a-failing.qmd's engine fails)"
        );

        // Positive assertion (mandatory — binds continue-on-error):
        // doc B's capture WAS recorded despite doc A's failure.
        assert!(
            ctx.index().has_capture("b-echo.qmd"),
            "doc B's capture must be recorded even though doc A's engine failed"
        );
        let b_entry = ctx.index().get_capture("b-echo.qmd").unwrap();
        let b_captures = read_capture_from_doc(&ctx, &b_entry.capture_doc_id)
            .await
            .expect("doc B's capture doc round-trips")
            .into_captures();
        assert_eq!(b_captures.len(), 1);
        assert_eq!(b_captures[0].engine_name, "test-passthrough");

        // Doc A: no capture recorded, and Q-PREVIEW-CAP-1 reached the
        // diagnostic sink.
        assert!(
            !ctx.index().has_capture("a-failing.qmd"),
            "doc A's capture must NOT be recorded — its engine failed"
        );
        let a_diags = sink.get_for_page("a-failing.qmd");
        assert_eq!(
            a_diags.len(),
            1,
            "exactly one diagnostic expected for a-failing.qmd; got {a_diags:?}"
        );
        assert_eq!(a_diags[0].code.as_deref(), Some("Q-PREVIEW-CAP-1"));
        let problem = a_diags[0]
            .problem
            .as_ref()
            .expect("diagnostic should carry the engine's error message");
        assert!(
            problem
                .as_str()
                .contains("synthetic PC3 failing engine failure"),
            "problem field should surface the engine's error; got: {}",
            problem.as_str()
        );
    }

    #[tokio::test]
    async fn re_running_driver_is_idempotent() {
        // The first run records a capture; the second run sees the
        // sidecar entry and skips the file, returning 0.
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )])
        .await;

        let first = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        let second = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0, "second run should be a no-op");
    }

    // ──────────────────────────────────────────────────────────────
    // Phase C.2: recompute_staleness
    // ──────────────────────────────────────────────────────────────

    /// Helper: write fixture content to the file backing rel_path,
    /// then return paths + ctx. The path argument is relative to the
    /// project root managed by the HubContext.
    async fn build_ctx_record_then_return_root(
        rel: &str,
        content: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[(rel, content)]).await;
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "fixture must produce a capture");
        (tmp, ctx, runtime)
    }

    /// RIU-0 S1 defect D1: a persisted `--data-dir` keeps the capture ref
    /// (and its staleness flag) across a restart. The eager driver used to
    /// skip a path that already had a ref without re-checking it, so code
    /// edited while the server was stopped showed as fresh (no Re-execute
    /// banner) until the next file-change event.
    #[tokio::test]
    async fn restart_marks_existing_capture_stale_when_code_changed_offline() {
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false)
        );

        // Edit the cell while "the server is stopped": no file-change
        // event reaches `recompute_staleness`.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        // "Restart": the eager driver runs again over the persisted index.
        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "an existing ref is not re-recorded");
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "startup must recompute staleness against the current file"
        );
    }

    /// The other direction of D1: a stale flag persisted from the previous
    /// session clears on restart when the file again matches the capture.
    #[tokio::test]
    async fn restart_clears_persisted_stale_flag_when_content_matches() {
        let original = "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n";
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root("doc.qmd", original).await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();
        assert!(
            recompute_staleness(
                ctx.clone(),
                runtime.clone(),
                "doc.qmd",
                None,
                &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone())
            )
            .await
            .unwrap()
        );
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true)
        );

        // Reverted while the server was stopped, then restarted.
        std::fs::write(tmp.path().join("doc.qmd"), original).unwrap();
        record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false)
        );
    }

    #[tokio::test]
    async fn recompute_staleness_marks_stale_when_cell_body_changes() {
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        // Sanity: initial recompute against unchanged content is a no-op.
        assert!(
            !recompute_staleness(
                ctx.clone(),
                runtime.clone(),
                "doc.qmd",
                None,
                &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone())
            )
            .await
            .unwrap(),
            "unchanged content should not flip staleness"
        );
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false)
        );

        // Edit the cell body on disk — same engine, different cell.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(flipped, "recompute should report a flip");
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "sidecar should now say staleness: true"
        );
    }

    /// Pre-existing watcher scope limit, **not a regression** (EXR-14 scope note): the
    /// preview watcher recomputes staleness only for the path that changed
    /// (`on_file_changed` in `lib.rs`), so an edit to an *included* file never recomputes
    /// the includer. Recomputing the includer itself does see the edit; only the trigger
    /// is missing. Fixing the watcher is outside the execution-records epic.
    #[tokio::test]
    async fn editing_an_included_file_does_not_recompute_the_includer_known_watcher_limit() {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[
            (
                "doc.qmd",
                "---\nengine: test-passthrough\n---\n\n{{< include _inc.qmd >}}\n",
            ),
            ("_inc.qmd", "```{test-passthrough}\nfirst\n```\n"),
        ])
        .await;
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(count >= 1, "the includer has a capture");

        std::fs::write(
            tmp.path().join("_inc.qmd"),
            "```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        // What the watcher does for the changed path: recompute that path only.
        recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            "_inc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "the includer is not recomputed by an edit to its include"
        );

        // The includer itself would see the edit if something recomputed it.
        assert!(
            recompute_staleness(
                ctx.clone(),
                runtime,
                "doc.qmd",
                None,
                &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone())
            )
            .await
            .unwrap(),
            "recomputing the includer does see the include's edit"
        );
    }

    #[tokio::test]
    async fn recompute_staleness_clears_when_content_reverts() {
        // After staleness is set, restoring the original content
        // must flip staleness back to false (so users can recover by
        // undoing). The recompute is idempotent.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;

        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();
        let _ = recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true)
        );

        // Revert.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .unwrap();
        let _ = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "reverting content should clear staleness"
        );
    }

    #[tokio::test]
    async fn recompute_staleness_also_flips_for_prose_only_edits_v1_limitation() {
        // Plan §Q-C3: v1 whole-QMD byte-equality means prose-only
        // edits also flip the staleness flag. Documented limitation;
        // test pins the v1 behaviour.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\nA paragraph.\n\n```{test-passthrough}\nx\n```\n",
        )
        .await;

        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\nA different paragraph.\n\n```{test-passthrough}\nx\n```\n",
        )
        .unwrap();
        recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "prose-only edit ALSO flips staleness in v1 (known limitation per §Q-C3)"
        );
    }

    /// An engine that records the units it owns, as the Jupyter engine does (EXR-4's
    /// `RecordBuilder` over the stage's units and author texts). The passthrough engine
    /// above builds no record.
    /// `header_keys`: behave like the knitr adapter (KNR-3 slice 8): `echo` and `warning` are
    /// presentation options outside the hash, and each cell records the keys of its `#|` header.
    struct RecordingTestEngine {
        header_keys: bool,
    }

    impl ExecutionEngine for RecordingTestEngine {
        fn name(&self) -> &str {
            "test-recording"
        }

        fn hash_definition(&self) -> quarto_core::record::HashDefinition {
            let mut def = quarto_core::record::HashDefinition::conservative(self.name());
            if self.header_keys {
                def.presentation_cell_options = vec!["echo".into(), "warning".into()];
            }
            def
        }

        fn execute(
            &self,
            input: &str,
            ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            use quarto_core::record::{
                EngineEntryInput, HEADER_KEYS_KEY, RecordBuilder, UnitResult, partition_author_text,
            };
            let mut result = ExecuteResult::passthrough(input);
            if ctx.want_record {
                let fail = |e: &dyn std::fmt::Display| {
                    ExecutionError::execution_failed("test-recording", e.to_string())
                };
                let def = self.hash_definition();
                let mut builder =
                    RecordBuilder::new(self.name(), &def, &ctx.units, &ctx.author_texts)
                        .map_err(|e| fail(&e))?;
                for (i, unit) in ctx.units.iter().enumerate() {
                    if ctx.owned_languages.iter().any(|l| l == unit.lang()) {
                        builder
                            .push(i, UnitResult::default())
                            .map_err(|e| fail(&e))?;
                    }
                }
                let mut record = builder
                    .finish(EngineEntryInput {
                        owned_languages: ctx.owned_languages.clone(),
                        document_metadata: ctx.document_metadata_json(),
                        ..Default::default()
                    })
                    .map_err(|e| fail(&e))?;
                if self.header_keys {
                    let owned: Vec<usize> = (0..ctx.units.len())
                        .filter(|i| {
                            ctx.owned_languages
                                .iter()
                                .any(|l| l == ctx.units[*i].lang())
                        })
                        .collect();
                    for (cell, i) in record.cells.iter_mut().zip(owned) {
                        let (options, _) =
                            partition_author_text(ctx.units[i].lang(), &ctx.author_texts.0[i].text)
                                .map_err(|e| fail(&e))?;
                        cell.metadata.extra.insert(
                            HEADER_KEYS_KEY.into(),
                            serde_json::json!(options.keys().collect::<Vec<_>>()),
                        );
                    }
                }
                result.record = Some(record);
            }
            Ok(result)
        }

        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-recording" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn recording_registry(header_keys: bool) -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(RecordingTestEngine { header_keys }));
        Arc::new(reg)
    }

    // ──────────────────────────────────────────────────────────────
    // EXR-15: record-level staleness (decision 5)
    // ──────────────────────────────────────────────────────────────

    /// Like `build_ctx_record_then_return_root`, but the sidecar points at a *record*
    /// doc (writer gate on), built from a real pre-engine run.
    async fn build_ctx_record_doc_then_return_root(
        files: &[(&str, &str)],
        rel: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        build_ctx_record_doc_with(files, rel, false).await
    }

    async fn build_ctx_record_doc_with(
        files: &[(&str, &str)],
        rel: &str,
        header_keys: bool,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let (tmp, ctx, runtime) = build_ctx_with_files(files).await;
        let abs = tmp.path().join(rel);
        let project = ProjectContext::discover(&abs, runtime.as_ref()).unwrap();
        let run = quarto_core::engine::preview_record::record_capture_run(
            &abs,
            &project,
            runtime.clone(),
            Some(recording_registry(header_keys)),
        )
        .await
        .unwrap();
        assert!(run.record.is_some(), "the engine stage emits a record");
        let id = write_capture_doc_gated(&ctx, rel, &run, true)
            .await
            .unwrap();
        ctx.index()
            .set_capture(
                rel,
                &CaptureRef {
                    capture_doc_id: id,
                    staleness: Some(false),
                    state: None,
                    last_error: None,
                },
            )
            .unwrap();
        (tmp, ctx, runtime)
    }

    async fn staleness_after_edit(
        ctx: &Arc<HubContext>,
        runtime: &Arc<dyn SystemRuntime>,
        rel: &str,
    ) -> Option<bool> {
        recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            rel,
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        ctx.index().get_capture(rel).unwrap().staleness
    }

    const RECORD_DOC: &str =
        "---\nengine: test-recording\n---\n\nA paragraph.\n\n```{test-recording}\nx\n```\n";

    #[tokio::test]
    async fn record_prose_only_edit_stays_fresh() {
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            RECORD_DOC.replace("A paragraph.", "A different paragraph."),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false),
            "a prose edit does not stale a record"
        );
    }

    #[tokio::test]
    async fn record_code_edit_flips_staleness() {
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            RECORD_DOC.replace("\nx\n", "\ny\n"),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn record_unchanged_document_stays_fresh() {
        let (_tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false)
        );
    }

    const HEADED_DOC: &str = "---\nengine: test-recording\n---\n\nA paragraph.\n\n```{test-recording}\n#| echo: true\nx\n```\n";

    /// KNR-3 slice 8: deleting a header key the cell recorded flips the sidecar staleness, though
    /// the hashes (which leave `echo` out) still say fresh. Adding a key does not.
    #[tokio::test]
    async fn a_deleted_header_key_flips_staleness() {
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_with(&[("doc.qmd", HEADED_DOC)], "doc.qmd", true).await;
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false)
        );
        std::fs::write(
            tmp.path().join("doc.qmd"),
            HEADED_DOC.replace("#| echo: true\n", "#| echo: true\n#| warning: false\n"),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false),
            "an added key is not a deletion"
        );
        std::fs::write(
            tmp.path().join("doc.qmd"),
            HEADED_DOC.replace("#| echo: true\n", ""),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(true)
        );
    }

    /// Under `Auto` the same deletion re-runs (a new capture, staleness cleared); under `Manual`
    /// it only sets the flag.
    #[tokio::test]
    async fn auto_policy_re_executes_after_a_header_key_is_deleted() {
        crate::re_execute::reset_in_flight_for_tests();
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_with(&[("doc.qmd", HEADED_DOC)], "doc.qmd", true).await;
        let initial = ctx
            .index()
            .get_capture("doc.qmd")
            .unwrap()
            .capture_doc_id
            .clone();
        std::fs::write(
            tmp.path().join("doc.qmd"),
            HEADED_DOC.replace("#| echo: true\n", ""),
        )
        .unwrap();
        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            Some(recording_registry(true)),
            &ReExecuteConfig::new(EnginePolicy::Auto, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(flipped, "Auto reports the flip");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(quarto_hub::index::CaptureState::Idle)
                && entry.capture_doc_id != initial
            {
                assert_eq!(entry.staleness, Some(false));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no re-run within 10s; entry = {entry:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// The function, not the watcher: recomputing the includer after an edit inside an
    /// included file that holds code sets `staleness`, because the hash comes from the
    /// expanded pre-engine units, not from file bytes. (The watcher never makes this call;
    /// see the known-limit test above.)
    #[tokio::test]
    async fn record_function_sees_a_code_edit_inside_an_include() {
        let (tmp, ctx, runtime) = build_ctx_record_doc_then_return_root(
            &[
                (
                    "doc.qmd",
                    "---\nengine: test-recording\n---\n\n{{< include _inc.qmd >}}\n",
                ),
                ("_inc.qmd", "```{test-recording}\nfirst\n```\n"),
            ],
            "doc.qmd",
        )
        .await;
        std::fs::write(
            tmp.path().join("_inc.qmd"),
            "```{test-recording}\nsecond\n```\n",
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn recompute_staleness_noop_when_no_capture() {
        // A path without a sidecar entry (e.g. a fresh doc that
        // never had a capture recorded) is a no-op — recompute
        // should return Ok(false) and not error.
        let (_tmp, ctx, runtime) =
            build_ctx_with_files(&[("doc.qmd", "---\ntitle: Prose\n---\n\nhi\n")]).await;

        let result = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(!result);
        assert!(ctx.index().get_capture("doc.qmd").is_none());
    }

    #[tokio::test]
    async fn recompute_staleness_noop_in_standalone_mode() {
        // Standalone hub (no project root) → nothing to recompute.
        let temp = TempDir::with_prefix("c2-staleness-standalone-").unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let result = recompute_staleness(
            ctx,
            runtime,
            "anything.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(!result);
    }

    // ──────────────────────────────────────────────────────────────
    // Phase C.6: preview.engine policy (Off / Auto)
    // ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn off_policy_skips_eager_capture_for_doc_with_code_cells() {
        // §C.6 acceptance: "off skips eager + suppresses code-cell exec."
        // A doc with a code cell that would otherwise produce a capture
        // (covered by `doc_with_passthrough_engine_records_capture`)
        // must produce zero captures under `Off`.
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Off, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "Off policy must skip eager capture");
        assert!(
            !ctx.index().has_capture("doc.qmd"),
            "Off policy must leave the sidecar untouched"
        );
    }

    #[tokio::test]
    async fn off_policy_makes_recompute_staleness_a_noop_even_with_capture() {
        // §C.6 acceptance: under Off, the file-watcher staleness hook
        // does nothing — even if a stale-looking capture exists from a
        // prior session.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        // Edit the cell body — under Manual this would flip staleness.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            None,
            &ReExecuteConfig::new(EnginePolicy::Off, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(!flipped, "Off policy must not flip staleness");
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "Off policy must leave the existing staleness flag alone"
        );
    }

    #[tokio::test]
    async fn auto_policy_marks_stale_and_triggers_re_execute() {
        // §C.6 acceptance: "auto re-executes on every code-cell change
        // without the overlay." We can't easily black-box assert "the
        // SPA never showed the overlay" in a unit test, but we can
        // assert that recompute_staleness under Auto:
        //   (a) DOES detect the staleness flip (the SPA's invariant);
        //   (b) The capture is eventually replaced (state: idle, new
        //       captureDocId) without an HTTP call.
        crate::re_execute::reset_in_flight_for_tests();
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        let initial_doc_id = ctx
            .index()
            .get_capture("doc.qmd")
            .unwrap()
            .capture_doc_id
            .clone();

        // Edit the cell body so the next recompute sees a mismatch.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            Some(make_registry()),
            &ReExecuteConfig::new(EnginePolicy::Auto, cache_dir_for_test().clone()),
        )
        .await
        .unwrap();
        assert!(flipped, "Auto must still report the staleness flip");

        // Auto kicked off a spawn_blocking worker; poll for the new
        // capture (state: idle, fresh captureDocId, staleness: false).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(quarto_hub::index::CaptureState::Idle)
                && entry.capture_doc_id != initial_doc_id
            {
                assert_eq!(
                    entry.staleness,
                    Some(false),
                    "Auto re-execute must clear the staleness flag"
                );
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "Auto re-execute did not produce a new capture within 10s; entry = {:?}",
                    entry
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    // ── RIU-3a: an imported Positron record through record_one and recompute_staleness ─────

    use quarto_core::record::{RecordSource, RecordSources, content_hash};

    /// A Positron-format cache directory for the document at `abs`, one cell per code string.
    fn positron_dir_for(abs: &std::path::Path, codes: &[&str]) -> TempDir {
        let dir = TempDir::with_prefix("riu3a-positron-").unwrap();
        let cells: Vec<serde_json::Value> = codes
            .iter()
            .map(|c| {
                serde_json::json!({
                    "cell_type": "code", "execution_count": null,
                    "metadata": {"quarto_content_hash": content_hash(c)},
                    "outputs": [{"output_type": "stream", "name": "stdout", "text": [format!("out of {c}\n")]}],
                    "source": []
                })
            })
            .collect();
        let uri = quarto_core::positron_cache::vscode_file_uri(
            &abs.to_string_lossy(),
            quarto_core::positron_cache::HostOs::current(),
        );
        std::fs::write(
            dir.path().join("cache.ipynb"),
            serde_json::json!({
                "nbformat": 4, "nbformat_minor": 5,
                "metadata": {"quarto_source": uri, "quarto_cache_version": 1},
                "cells": cells
            })
            .to_string(),
        )
        .unwrap();
        dir
    }

    fn import_cfg(dir: &TempDir) -> ReExecuteConfig {
        let mut cfg =
            ReExecuteConfig::new(EnginePolicy::Manual, cache_dir_for_test()).with_gate(true);
        cfg.sources = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
        cfg
    }

    const IMPORT_DOC: &str =
        "---\ntitle: T\n---\n\n```{python}\nx = 1\n```\n\n```{python}\ny = 2\n```\n";

    async fn publish_imported(
        doc: &str,
        cached: &[&str],
    ) -> (
        TempDir,
        TempDir,
        Arc<HubContext>,
        Arc<dyn SystemRuntime>,
        ReExecuteConfig,
    ) {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", doc)]).await;
        let abs = tmp.path().join("doc.qmd");
        let dir = positron_dir_for(&abs, cached);
        let cfg = import_cfg(&dir);
        let wrote = record_one(&abs, "doc.qmd", &ctx, &runtime, None, &cfg)
            .await
            .unwrap();
        assert!(
            wrote,
            "the imported record is published with no engine and no registry"
        );
        (tmp, dir, ctx, runtime, cfg)
    }

    fn staleness(ctx: &Arc<HubContext>) -> Option<bool> {
        ctx.index().get_capture("doc.qmd").unwrap().staleness
    }

    async fn recompute(
        ctx: &Arc<HubContext>,
        runtime: &Arc<dyn SystemRuntime>,
        cfg: &ReExecuteConfig,
    ) -> bool {
        recompute_staleness(ctx.clone(), runtime.clone(), "doc.qmd", None, cfg)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_unedited_imported_page_is_published_fresh_and_stays_quiet() {
        let (_tmp, _dir, ctx, runtime, cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2"]).await;
        assert_eq!(staleness(&ctx), Some(false));
        assert!(
            !recompute(&ctx, &runtime, &cfg).await,
            "same answer as the publish"
        );
        assert_eq!(staleness(&ctx), Some(false));
    }

    #[tokio::test]
    async fn a_page_with_units_the_cache_does_not_cover_stays_quiet() {
        let (_tmp, _dir, ctx, runtime, cfg) = publish_imported(IMPORT_DOC, &["x = 1"]).await;
        assert_eq!(staleness(&ctx), Some(false));
        assert!(!recompute(&ctx, &runtime, &cfg).await);
    }

    #[tokio::test]
    async fn a_chunk_the_gate_refuses_does_not_flag_the_page() {
        let doc = "---\ntitle: T\n---\n\n```{python}\nx = 1\n```\n\n```{ojs}\nz = 3\n```\n";
        let (_tmp, _dir, ctx, runtime, cfg) = publish_imported(doc, &["x = 1", "z = 3"]).await;
        assert_eq!(
            staleness(&ctx),
            Some(false),
            "the refused chunk's cache cell is claimed"
        );
        assert!(!recompute(&ctx, &runtime, &cfg).await);
    }

    #[tokio::test]
    async fn a_cache_older_than_the_document_is_flagged_at_boot() {
        // The cache has an output for code the document no longer has.
        let (_tmp, _dir, ctx, _runtime, _cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2", "old = 0"]).await;
        assert_eq!(staleness(&ctx), Some(true));
    }

    #[tokio::test]
    async fn editing_a_cell_flips_the_page_and_reverting_clears_it() {
        let (tmp, _dir, ctx, runtime, cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2"]).await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            IMPORT_DOC.replace("y = 2", "y = 3"),
        )
        .unwrap();
        assert!(recompute(&ctx, &runtime, &cfg).await, "an edit flips it");
        assert_eq!(staleness(&ctx), Some(true));
        std::fs::write(tmp.path().join("doc.qmd"), IMPORT_DOC).unwrap();
        assert!(
            recompute(&ctx, &runtime, &cfg).await,
            "reverting flips it back"
        );
        assert_eq!(staleness(&ctx), Some(false));
    }

    #[tokio::test]
    async fn deleting_a_cell_flags_the_page() {
        let (tmp, _dir, ctx, runtime, cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2"]).await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\ntitle: T\n---\n\n```{python}\nx = 1\n```\n",
        )
        .unwrap();
        assert!(recompute(&ctx, &runtime, &cfg).await);
        assert_eq!(staleness(&ctx), Some(true));
    }

    #[tokio::test]
    async fn a_prose_edit_does_not_flag_an_imported_page() {
        let (tmp, _dir, ctx, runtime, cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2"]).await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            IMPORT_DOC.replace("title: T", "title: Other"),
        )
        .unwrap();
        assert!(!recompute(&ctx, &runtime, &cfg).await);
        assert_eq!(staleness(&ctx), Some(false));
    }

    /// The guard picks on the marker **and** the engine list: a hand-edited copy carrying both
    /// takes the normal `record_is_stale` path.
    #[test]
    fn a_marker_with_engines_takes_the_normal_staleness_path() {
        let doc = "```{python}\nx = 1\n```\n";
        let (units, authors) = {
            let mut u = quarto_core::engine::units::scan_units(doc);
            let a = quarto_core::record::AuthorTexts::from_scanned(&u);
            quarto_core::record::fill_ids(&mut u, &a).unwrap();
            (u, a)
        };
        let file = serde_json::json!({"nbformat": 4, "nbformat_minor": 5,
            "metadata": {"quarto_cache_version": 1},
            "cells": [{"cell_type": "code", "metadata": {"quarto_content_hash": content_hash("x = 1")},
                       "outputs": [], "source": []}]});
        let import =
            quarto_core::record::import_positron_cache(file.to_string().as_bytes()).unwrap();
        let mut record =
            quarto_core::record::record_from_positron(&import, &units, &authors, None, &|_| {
                "jupyter".into()
            });
        let meta = Default::default();
        assert!(
            !record_staleness(&record, doc, &[], &meta).unwrap(),
            "imported: matches"
        );

        // The same record with an engine entry whose document hash cannot match.
        record.metadata.quarto_engines.push(
            serde_json::from_value(serde_json::json!({
                "name": "jupyter", "can_freeze": true, "document_hash": "not-the-hash",
                "owned_languages": ["python"],
                "hash_definition": quarto_core::record::HashDefinition::conservative("jupyter")
            }))
            .unwrap(),
        );
        assert!(
            record_staleness(&record, doc, &[], &meta).unwrap(),
            "with an engine it is compared by `record_is_stale`, whose hash differs"
        );
    }

    /// A `jupyter` engine that claims `python` and fails if it is ever run: the stand-in for a
    /// machine with no usable Jupyter runtime. It counts how often it was asked.
    struct UnstartableJupyter(Arc<std::sync::atomic::AtomicUsize>);
    impl ExecutionEngine for UnstartableJupyter {
        fn name(&self) -> &str {
            "jupyter"
        }
        fn execute(
            &self,
            _input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ExecutionError::Other("no Jupyter runtime".into()))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "python" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    /// RIU-6 Phase 0 observation 4: with a source ahead of `engine` that supplies, the engine is
    /// never asked, so a machine with no Jupyter runtime gets no `Q-PREVIEW-CAP-1` (which the
    /// eager driver emits exactly when `record_one` fails). The control: when the source has
    /// nothing, the engine runs, fails, and `record_one` reports it.
    #[tokio::test]
    async fn a_covering_positron_source_ahead_of_engine_keeps_cap_1_from_appearing() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::new(UnstartableJupyter(calls.clone())));
        let reg = Arc::new(reg);

        let (tmp, ctx, runtime) = build_ctx_with_files(&[("doc.qmd", IMPORT_DOC)]).await;
        let abs = tmp.path().join("doc.qmd");
        let covering = positron_dir_for(&abs, &["x = 1", "y = 2"]);
        let mut cfg = import_cfg(&covering);
        cfg.sources = RecordSources::new(vec![
            RecordSource::Dir(covering.path().to_path_buf()),
            RecordSource::Engine,
        ]);
        let wrote = record_one(&abs, "doc.qmd", &ctx, &runtime, Some(reg.clone()), &cfg)
            .await
            .expect("the cache supplies, so the failing engine is never reached");
        assert!(wrote);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        // Control: a directory with nothing for this document falls through to the engine.
        ctx.index().remove_capture("doc.qmd").ok();
        let empty = TempDir::with_prefix("riu3a-empty-").unwrap();
        cfg.sources = RecordSources::new(vec![
            RecordSource::Dir(empty.path().to_path_buf()),
            RecordSource::Engine,
        ]);
        let err = record_one(&abs, "doc.qmd", &ctx, &runtime, Some(reg), &cfg)
            .await
            .unwrap_err();
        assert!(matches!(err, RecordError::RecordFailed(_)), "{err:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// RIU-3a: Re-execute on a page that has only an imported record. With `engine` absent from
    /// the list the request is refused (the guard's message, a 403), nothing runs and the
    /// sidecar is untouched; with `engine` listed the same request runs the engine.
    #[tokio::test]
    async fn re_execute_on_an_imported_page_is_refused_without_engine_and_runs_it_when_listed() {
        use axum::http::StatusCode;
        crate::re_execute::reset_in_flight_for_tests();
        let (tmp, dir, ctx, _runtime, mut cfg) =
            publish_imported(IMPORT_DOC, &["x = 1", "y = 2"]).await;
        // Edit a cell, so the imported record is stale and Re-execute is what the viewer offers.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            IMPORT_DOC.replace("x = 1", "x = 11"),
        )
        .unwrap();
        let before = ctx.index().get_capture("doc.qmd").unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::new(UnstartableJupyter(calls.clone())));
        let reg = Arc::new(reg);
        let request = || crate::re_execute::ReExecuteRequest {
            path: "doc.qmd".to_string(),
        };

        // No `engine` in the list (the Dir source alone): refused.
        let response = crate::re_execute::re_execute_with_registry_and_cache(
            ctx.clone(),
            request(),
            Some(reg.clone()),
            &cfg,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let msg = String::from_utf8_lossy(&body).into_owned();
        assert!(msg.contains("`execute: record-sources`"), "{msg}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let now = ctx.index().get_capture("doc.qmd").unwrap();
        assert_eq!(
            now.capture_doc_id, before.capture_doc_id,
            "sidecar untouched"
        );

        // With `engine` listed the engine runs.
        cfg.sources = RecordSources::new(vec![
            RecordSource::Dir(dir.path().to_path_buf()),
            RecordSource::Engine,
        ]);
        let response = crate::re_execute::re_execute_with_registry_and_cache(
            ctx.clone(),
            request(),
            Some(reg),
            &cfg,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while calls.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "the engine never ran");
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Let the worker finish before the runtime goes away.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
