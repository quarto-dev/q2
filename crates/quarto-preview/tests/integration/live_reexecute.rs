//! Phase 0 of `claude-notes/plans/2026-11-12-preview-with-q2-and-show-outputs.md`: the live
//! re-execute path of `q2 preview` under `-M`, the cache honoring the record sources, and the
//! `auto` policy re-running whenever a document is stale.
//!
//! Staleness is driven through `capture_driver::recompute_staleness`, never through the file
//! watcher: the watcher-driven staleness test is `#[ignore]`d on macOS (bd-9brz). The stub
//! engine builds a record (the passthrough engines build none), counts its runs, fails on an
//! input containing `FAIL`, and blocks on an input containing `BLOCK` until the test releases
//! the gate. Its record outputs hold the whole input text, so the `.quarto` copy shows which
//! version of the document last ran.
//!
//! Tests that boot the server (to set the `-M` overlay, a process-wide session setting) arm a
//! file watcher, so the module is in the serialized watcher group of `.config/nextest.toml`.

use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_core::pandoc_request::captures::CapturePayload;
use quarto_core::record::{
    EngineEntryInput, RecordBuilder, RecordSource, RecordSources, UnitResult,
};
use quarto_hub::HubContext;
use quarto_hub::context::HubConfig;
use quarto_hub::index::CaptureRef;
use quarto_hub::storage::StorageManager;
use quarto_preview::{
    EnginePolicy, PreviewConfig, RenderFlags, capture_driver, config::ReExecuteConfig,
    run_with_on_ready,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::json;
use tokio::sync::oneshot;

// ── The stub engine ───────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }
}

#[derive(Clone, Default)]
struct Probe {
    /// Engine runs that reached `execute`.
    runs: Arc<AtomicUsize>,
    gate: Arc<Gate>,
}

struct LiveEngine(Probe);

fn stub_error(e: impl std::fmt::Display) -> ExecutionError {
    ExecutionError::execution_failed("test-live", e.to_string())
}

impl ExecutionEngine for LiveEngine {
    fn name(&self) -> &str {
        "test-live"
    }
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == "test-live" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        if input.contains("BLOCK") {
            self.0.gate.wait();
        }
        if input.contains("FAIL") {
            return Err(stub_error("the stub engine was told to fail"));
        }
        let mut result = ExecuteResult::new(input);
        if ctx.want_record {
            let def = self.hash_definition();
            let mut builder = RecordBuilder::new("test-live", &def, &ctx.units, &ctx.author_texts)
                .map_err(stub_error)?;
            for (index, unit) in ctx.units.iter().enumerate() {
                if ctx.owned_languages.iter().any(|l| l == unit.lang()) {
                    builder
                        .push(
                            index,
                            UnitResult {
                                outputs: vec![json!({
                                    "output_type": "stream",
                                    "name": "stdout",
                                    "text": format!("RAN: {input}\n"),
                                })],
                                execution_count: Some(1),
                                kernel: None,
                            },
                        )
                        .map_err(stub_error)?;
                }
            }
            result.record = Some(
                builder
                    .finish(EngineEntryInput {
                        owned_languages: ctx.owned_languages.clone(),
                        document_metadata: ctx.document_metadata_json(),
                        ..Default::default()
                    })
                    .map_err(stub_error)?,
            );
        }
        Ok(result)
    }
}

fn registry(probe: &Probe) -> Arc<EngineRegistry> {
    let mut r = EngineRegistry::new();
    r.register(Arc::new(LiveEngine(probe.clone())));
    Arc::new(r)
}

// ── Fixtures ──────────────────────────────────────────────────────────────────────────────

/// A document whose own front matter opts into the `.quarto` copy.
fn recording_doc(marker: &str) -> String {
    format!(
        "---\nengine: test-live\nexecute:\n  record: true\n---\n\n```{{test-live}}\n{marker}\n```\n"
    )
}

/// A document with no `record:` of its own.
fn plain_doc(marker: &str) -> String {
    format!("---\nengine: test-live\n---\n\n```{{test-live}}\n{marker}\n```\n")
}

fn copy_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(".quarto")
        .join("execution-records")
        .join(format!("{stem}.ipynb"))
}

fn copy_text(dir: &Path, stem: &str) -> String {
    std::fs::read_to_string(copy_path(dir, stem)).unwrap_or_default()
}

fn plain_runtime() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

async fn wait_for(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !done() {
        assert!(Instant::now() < deadline, "timed out after {secs}s: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn runs(probe: &Probe) -> usize {
    probe.runs.load(Ordering::SeqCst)
}

fn capture(ctx: &HubContext, rel: &str) -> Option<CaptureRef> {
    ctx.index().get_capture(rel)
}

/// A hub context over `dir`, without a server. Its index lives in a fresh data directory (the
/// default `StorageManager::new` persists it under `dir/.quarto/hub`, so a second context over
/// the same `dir` would inherit the first one's sidecar).
async fn hub_ctx(dir: &Path) -> (Arc<HubContext>, tempfile::TempDir) {
    let data = tempfile::TempDir::with_prefix("q2-live-hub-").unwrap();
    let storage = StorageManager::new_with_data_dir(dir, data.path()).unwrap();
    let ctx = Arc::new(
        HubContext::new(storage, HubConfig::default())
            .await
            .unwrap(),
    );
    (ctx, data)
}

fn cfg(policy: EnginePolicy, cache: &Path, sources: Vec<RecordSource>) -> ReExecuteConfig {
    let mut c = ReExecuteConfig::new(policy, cache.to_path_buf()).with_gate(true);
    c.sources = RecordSources::new(sources);
    c
}

// ── Booting the server, for the tests that need the session's `-M` overlay ───────────────

struct Booted {
    ctx: Arc<HubContext>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
    _data: tempfile::TempDir,
}

impl Booted {
    async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
    }
}

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    listener.local_addr().expect("local_addr").port()
}

async fn boot(
    dir: &Path,
    single_file: Option<&str>,
    metadata: Option<serde_json::Value>,
    registry: Arc<EngineRegistry>,
    sources: Vec<RecordSource>,
) -> Booted {
    let data = tempfile::TempDir::with_prefix("q2-live-data-").unwrap();
    let config = PreviewConfig {
        upstream: None,
        render: RenderFlags {
            to: None,
            metadata,
            profile: Vec::new(),
        },
        host: "127.0.0.1".to_string(),
        port: pick_free_port(),
        project_root: Some(dir.to_path_buf()),
        single_file: single_file.map(PathBuf::from),
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: Some(registry),
        engine_policy: EnginePolicy::Manual,
        record_sources: RecordSources::new(sources),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: Default::default(),
    };
    let (tx, rx) = oneshot::channel::<Arc<HubContext>>();
    let mut tx = Some(tx);
    let server = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });
    let ctx = tokio::time::timeout(Duration::from_secs(20), rx)
        .await
        .expect("server reached on_ready within 20s")
        .expect("on_ready fired");
    Booted {
        ctx,
        server,
        _data: data,
    }
}

// ── `-M` reaches the engine run ──────────────────────────────────────────────────────────

fn overlay_record_on() -> serde_json::Value {
    json!({"execute": {"record": true, "record-sources": "engine"}})
}

/// `q2 preview doc.qmd -M 'execute:{record: true}' -M 'execute:{record-sources: engine}'` on a
/// document with no `record:` of its own: the startup capture writes the `.quarto` copy. Today
/// `-M` reaches the browser's render and the sources, never the engine run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minus_m_reaches_the_startup_engine_run() {
    let dir = tempfile::TempDir::with_prefix("q2-live-m-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("doc.qmd"), plain_doc("alpha")).unwrap();
    let probe = Probe::default();
    let booted = boot(
        &root,
        Some("doc.qmd"),
        Some(overlay_record_on()),
        registry(&probe),
        vec![RecordSource::Engine],
    )
    .await;

    wait_for("the startup capture", 30, || {
        capture(&booted.ctx, "doc.qmd").is_some()
    })
    .await;

    assert_eq!(runs(&probe), 1, "the startup capture ran the engine once");
    assert!(
        copy_text(&root, "doc").contains("RAN: "),
        "-M execute:{{record: true}} must reach the engine stage, which writes the copy; \
         {} is missing or empty",
        copy_path(&root, "doc").display()
    );
    booted.stop().await;
}

/// After that startup capture under `-M`, staleness (overlay-free, EXR-15 decision 5) finds the
/// document unchanged, and the stored record says which overlay it ran under. The overlay adds
/// an `execute:` key the stub's conservative hash definition counts, so an engine run whose
/// hashes still carried the overlay would read stale on any file event.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overlaid_run_stays_fresh_and_notes_its_overlay() {
    let dir = tempfile::TempDir::with_prefix("q2-live-ovl-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("doc.qmd"), plain_doc("alpha")).unwrap();
    let probe = Probe::default();
    let reg = registry(&probe);
    let overlay = json!({"execute": {
        "record": true, "record-sources": "engine", "daemon": 7
    }});
    let booted = boot(
        &root,
        Some("doc.qmd"),
        Some(overlay),
        reg.clone(),
        vec![RecordSource::Engine],
    )
    .await;
    wait_for("the startup capture", 30, || {
        capture(&booted.ctx, "doc.qmd").is_some()
    })
    .await;

    let entry = capture(&booted.ctx, "doc.qmd").unwrap();
    let payload = capture_driver::read_capture_from_doc(&booted.ctx, &entry.capture_doc_id)
        .await
        .expect("the capture doc reads back");
    let CapturePayload::Record(record) = payload else {
        panic!("the preview's writer gate is on by default: expected a record payload");
    };
    let noted = record
        .metadata
        .quarto_overlay
        .as_ref()
        .expect("a run under -M records the overlay it ran under in `quarto_overlay`");
    assert!(
        noted.contains_key("execute"),
        "the overlay's execution keys are noted: {noted:?}"
    );

    // A no-op file event: the document is unchanged, so it must stay fresh.
    let before = capture(&booted.ctx, "doc.qmd").unwrap().staleness;
    let c = cfg(
        EnginePolicy::Manual,
        &root.join("cache"),
        vec![RecordSource::Engine],
    );
    let flipped = capture_driver::recompute_staleness(
        booted.ctx.clone(),
        plain_runtime(),
        "doc.qmd",
        Some(reg),
        &c,
    )
    .await
    .unwrap();
    assert!(
        !flipped,
        "an unchanged document must not change its flag (was {before:?})"
    );
    assert_eq!(
        capture(&booted.ctx, "doc.qmd").unwrap().staleness,
        Some(false),
        "the flag stays fresh"
    );
    booted.stop().await;
}

/// `-M` outranks the document: `execute: {record: true}` over a front matter `record: false`,
/// and `record-sources: engine` over the document's own list, shared or not (under `--share`
/// the host list is only a ceiling, and `engine` is inside it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minus_m_outranks_the_documents_own_keys() {
    let dir = tempfile::TempDir::with_prefix("q2-live-prec-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    // A project, so both documents are captured at startup.
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    let own = "---\nengine: test-live\nexecute:\n  record: false\n  record-sources: q2\n---\n\n\
               ```{test-live}\nown\n```\n";
    std::fs::write(root.join("own.qmd"), own).unwrap();
    let wide = "---\nengine: test-live\nexecute:\n  record-sources: cache,q2,engine\n---\n\n\
                ```{test-live}\nwide\n```\n";
    std::fs::write(root.join("wide.qmd"), wide).unwrap();
    let probe = Probe::default();
    let booted = boot(
        &root,
        None,
        Some(overlay_record_on()),
        registry(&probe),
        vec![RecordSource::Cache, RecordSource::Q2, RecordSource::Engine],
    )
    .await;
    wait_for("the startup captures", 30, || {
        capture(&booted.ctx, "own.qmd").is_some() && capture(&booted.ctx, "wide.qmd").is_some()
    })
    .await;

    assert!(
        copy_text(&root, "own").contains("RAN: "),
        "-M record: true outranks the document's `record: false`"
    );
    assert_eq!(
        runs(&probe),
        2,
        "-M record-sources: engine outranks the documents' own lists (`q2`, `cache,q2,engine`)"
    );

    // Without `--share` the resolved list is -M's for both documents. Under `--share` the host
    // list (what the CLI derives from `-M`) is a ceiling a document may only narrow: `wide`
    // cannot widen it, and `own` (only `q2`, outside it) narrows it to nothing.
    let resolve = |share: bool, doc: &str| {
        let mut c = cfg(
            EnginePolicy::Manual,
            &root.join("cache"),
            vec![RecordSource::Engine],
        );
        c.share = share;
        c.document_sources(&root.join(doc), None)
            .unwrap()
            .sources
            .as_slice()
            .to_vec()
    };
    for doc in ["own.qmd", "wide.qmd"] {
        assert_eq!(
            resolve(false, doc),
            [RecordSource::Engine],
            "{doc}, no share"
        );
    }
    assert_eq!(resolve(true, "wide.qmd"), [RecordSource::Engine]);
    assert!(resolve(true, "own.qmd").is_empty());
    booted.stop().await;
}

// ── The cache honors the record sources ───────────────────────────────────────────────────

/// With `record-sources: engine`, an edit, a re-execute, a revert and a re-execute each run
/// the engine and rewrite the `.quarto` copy. Today the second re-execute hits the capture
/// cache (the reverted text's key is already there): no engine runs and the copy keeps the
/// undone version's outputs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cache_does_not_answer_a_re_execute_under_engine_only_sources() {
    let dir = tempfile::TempDir::with_prefix("q2-live-cache-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("doc.qmd");
    let only_engine = |marker: &str| {
        format!(
            "---\nengine: test-live\nexecute:\n  record: true\n  record-sources: engine\n---\n\n\
             ```{{test-live}}\n{marker}\n```\n"
        )
    };
    std::fs::write(&doc, only_engine("one")).unwrap();
    let probe = Probe::default();
    let reg = registry(&probe);
    let c = cfg(
        EnginePolicy::Auto,
        &root.join("cache"),
        vec![RecordSource::Engine],
    );
    let (ctx, _data) = hub_ctx(&root).await;

    let n =
        capture_driver::record_eager_captures(ctx.clone(), plain_runtime(), Some(reg.clone()), &c)
            .await
            .unwrap();
    assert_eq!(n, 1);
    assert_eq!(runs(&probe), 1);
    assert!(copy_text(&root, "doc").contains("one"));

    std::fs::write(&doc, only_engine("two")).unwrap();
    capture_driver::recompute_staleness(
        ctx.clone(),
        plain_runtime(),
        "doc.qmd",
        Some(reg.clone()),
        &c,
    )
    .await
    .unwrap();
    wait_for("the re-execute of `two`", 30, || {
        runs(&probe) == 2 && copy_text(&root, "doc").contains("two")
    })
    .await;

    std::fs::write(&doc, only_engine("one")).unwrap();
    capture_driver::recompute_staleness(ctx.clone(), plain_runtime(), "doc.qmd", Some(reg), &c)
        .await
        .unwrap();
    wait_for(
        "the re-execute of the reverted text to run the engine and rewrite the copy",
        20,
        || runs(&probe) == 3 && copy_text(&root, "doc").contains("one"),
    )
    .await;
}

// ── `auto` re-runs whenever the document is stale ─────────────────────────────────────────

/// A save with a typo fails the run (the flag stays stale); the save that fixes it re-runs
/// without a click. Today the second save finds no fresh-to-stale flip and returns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_re_runs_the_save_that_fixes_a_failed_run() {
    let dir = tempfile::TempDir::with_prefix("q2-live-fail-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("doc.qmd");
    std::fs::write(&doc, recording_doc("ok1")).unwrap();
    let probe = Probe::default();
    let reg = registry(&probe);
    let c = cfg(
        EnginePolicy::Auto,
        &root.join("cache"),
        vec![RecordSource::Engine],
    );
    let (ctx, _data) = hub_ctx(&root).await;
    capture_driver::record_eager_captures(ctx.clone(), plain_runtime(), Some(reg.clone()), &c)
        .await
        .unwrap();
    assert_eq!(runs(&probe), 1);

    std::fs::write(&doc, recording_doc("FAIL")).unwrap();
    capture_driver::recompute_staleness(
        ctx.clone(),
        plain_runtime(),
        "doc.qmd",
        Some(reg.clone()),
        &c,
    )
    .await
    .unwrap();
    wait_for("the failing run to finish", 30, || {
        runs(&probe) == 2
            && capture(&ctx, "doc.qmd")
                .is_some_and(|e| e.state == Some(quarto_hub::index::CaptureState::Error))
    })
    .await;

    std::fs::write(&doc, recording_doc("ok3")).unwrap();
    capture_driver::recompute_staleness(ctx.clone(), plain_runtime(), "doc.qmd", Some(reg), &c)
        .await
        .unwrap();
    wait_for("the fixing save to re-run on its own", 20, || {
        runs(&probe) == 3 && copy_text(&root, "doc").contains("ok3")
    })
    .await;
    wait_for("the flag to clear", 10, || {
        capture(&ctx, "doc.qmd").is_some_and(|e| e.staleness == Some(false))
    })
    .await;
}

/// A save during a run is not lost: the run finishes against the older text, and the newest
/// text runs after it. Today the run sets the flag fresh and the newest edit never runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_runs_the_edit_saved_during_a_run() {
    let dir = tempfile::TempDir::with_prefix("q2-live-block-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("doc.qmd");
    std::fs::write(&doc, recording_doc("ok1")).unwrap();
    let probe = Probe::default();
    let reg = registry(&probe);
    let c = cfg(
        EnginePolicy::Auto,
        &root.join("cache"),
        vec![RecordSource::Engine],
    );
    let (ctx, _data) = hub_ctx(&root).await;
    capture_driver::record_eager_captures(ctx.clone(), plain_runtime(), Some(reg.clone()), &c)
        .await
        .unwrap();

    std::fs::write(&doc, recording_doc("BLOCK")).unwrap();
    capture_driver::recompute_staleness(
        ctx.clone(),
        plain_runtime(),
        "doc.qmd",
        Some(reg.clone()),
        &c,
    )
    .await
    .unwrap();
    wait_for("the blocked run to start", 30, || runs(&probe) == 2).await;

    // The second save arrives while the first run is inside the engine.
    std::fs::write(&doc, recording_doc("ok3")).unwrap();
    capture_driver::recompute_staleness(ctx.clone(), plain_runtime(), "doc.qmd", Some(reg), &c)
        .await
        .unwrap();
    probe.gate.release();

    wait_for("the newest text to run after the first run", 30, || {
        runs(&probe) == 3 && copy_text(&root, "doc").contains("ok3")
    })
    .await;
    wait_for("the flag to clear", 10, || {
        capture(&ctx, "doc.qmd").is_some_and(|e| e.staleness == Some(false))
    })
    .await;
}

/// bd-twox482l: the default sources serve a stale `.quarto` copy at startup (no engine run),
/// so the flag starts stale. The next save must re-run under `auto`, not return early because
/// the flag is already `Some(true)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_re_runs_after_a_stale_copy_was_served_at_startup() {
    let dir = tempfile::TempDir::with_prefix("q2-live-twox-").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("doc.qmd");
    let probe = Probe::default();
    let reg = registry(&probe);

    // An earlier session wrote the copy.
    std::fs::write(&doc, recording_doc("one")).unwrap();
    {
        let c = cfg(
            EnginePolicy::Manual,
            &root.join("cache-1"),
            vec![RecordSource::Engine],
        );
        let (ctx, _data) = hub_ctx(&root).await;
        capture_driver::record_eager_captures(ctx, plain_runtime(), Some(reg.clone()), &c)
            .await
            .unwrap();
    }
    assert_eq!(runs(&probe), 1);
    assert!(copy_text(&root, "doc").contains("one"));

    // The document changes while the preview is stopped; the new session has the default
    // sources and an empty cache, so the `q2` source supplies the old copy.
    std::fs::write(&doc, recording_doc("two")).unwrap();
    let c = cfg(
        EnginePolicy::Auto,
        &root.join("cache-2"),
        vec![RecordSource::Cache, RecordSource::Q2, RecordSource::Engine],
    );
    let (ctx, _data) = hub_ctx(&root).await;
    capture_driver::record_eager_captures(ctx.clone(), plain_runtime(), Some(reg.clone()), &c)
        .await
        .unwrap();
    assert_eq!(
        runs(&probe),
        1,
        "startup served the copy without an engine run"
    );
    assert_eq!(
        capture(&ctx, "doc.qmd").unwrap().staleness,
        Some(true),
        "the served copy is stale"
    );

    // Any later save.
    std::fs::write(&doc, recording_doc("three")).unwrap();
    capture_driver::recompute_staleness(ctx.clone(), plain_runtime(), "doc.qmd", Some(reg), &c)
        .await
        .unwrap();
    wait_for("auto to re-run the stale document", 20, || {
        runs(&probe) == 2 && copy_text(&root, "doc").contains("three")
    })
    .await;
}
