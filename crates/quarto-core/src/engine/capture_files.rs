/*
 * engine/capture_files.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Embed and materialize engine-generated supporting files in engine
 * captures (bd-qbhp2cvv).
 */

//! Supporting-file transport for engine captures (bd-qbhp2cvv).
//!
//! Engines like knitr and jupyter write figure files to disk during
//! execution (`<stem>_files/figure-html/…`) and report them in
//! [`ExecuteResult::supporting_files`](super::ExecuteResult) — as
//! *paths*. `q2 render` copies those paths into the output directory
//! (`copy_resources_to_output_dir`, native only), but preview replay
//! happens on a machine (or a browser VFS) where the paths don't
//! exist; the hub even runs engines in a temp dir that is deleted
//! right after recording. So the bytes must travel inside the
//! capture:
//!
//! - [`collect_capture_files`] runs at recording time, right where
//!   the `EngineCapture` aux event is emitted
//!   (`EngineExecutionStage`), while the files still exist. It reads
//!   every reported file (recursing into reported directories) and
//!   returns [`CaptureFile`]s keyed by doc-relative, forward-slash
//!   paths.
//! - [`materialize_capture_files`] runs at splice time
//!   ([`CaptureSpliceStage`](crate::stage::stages::CaptureSpliceStage)),
//!   writing the embedded bytes next to the live document via the
//!   context's [`SystemRuntime`] — which in WASM is the VFS that the
//!   preview's image resolvers (`assetWalker.ts`,
//!   `iframePostProcessor.ts`) read.
//!
//! Collection is gated by
//! [`PipelineObserver::wants_engine_capture_files`](crate::stage::PipelineObserver::wants_engine_capture_files)
//! so plain `q2 render` never pays the extra file I/O.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::CaptureFile;

/// Read the engine's reported supporting files and package them for
/// embedding in an [`quarto_trace::EngineCapture`].
///
/// `supporting_files` entries may be absolute or doc-dir-relative
/// (the [`ExecuteResult::supporting_files`](super::ExecuteResult)
/// contract); each may be a file or a directory (knitr reports the
/// whole `<stem>_files` directory). Directories are walked
/// recursively. Files are keyed by their path relative to `doc_dir`,
/// with forward-slash separators, so a capture recorded on any
/// platform replays on any other.
///
/// Fail-soft per entry: files that cannot be read, or that resolve
/// outside `doc_dir` (a relative image reference in the document
/// could never reach those anyway), are skipped with a warning.
/// Ordering is deterministic (directory listings are sorted) so
/// identical runs produce identical capture bytes for the
/// content-hash-keyed capture cache.
pub fn collect_capture_files(
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    supporting_files: &[PathBuf],
) -> Vec<CaptureFile> {
    let mut out = Vec::new();
    for entry in supporting_files {
        let resolved = if entry.is_absolute() {
            entry.clone()
        } else {
            doc_dir.join(entry)
        };
        collect_path(runtime, doc_dir, &resolved, &mut out);
    }
    out
}

fn collect_path(
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    path: &Path,
    out: &mut Vec<CaptureFile>,
) {
    if runtime.is_dir(path).unwrap_or(false) {
        let mut children = match runtime.dir_list(path) {
            Ok(children) => children,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "capture-files: cannot list supporting-file directory; skipping"
                );
                return;
            }
        };
        children.sort();
        for child in children {
            collect_path(runtime, doc_dir, &child, out);
        }
        return;
    }

    let Some(rel) = doc_relative_slash_path(doc_dir, path) else {
        tracing::warn!(
            path = %path.display(),
            doc_dir = %doc_dir.display(),
            "capture-files: supporting file is outside the document directory; skipping \
             (relative references in the document cannot reach it)"
        );
        return;
    };

    match runtime.file_read(path) {
        Ok(bytes) => out.push(CaptureFile {
            path: rel,
            contents_base64: BASE64.encode(&bytes),
        }),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "capture-files: cannot read supporting file; skipping"
            );
        }
    }
}

/// Compute `path` relative to `doc_dir` as a forward-slash string.
/// Returns `None` when `path` is not under `doc_dir`.
fn doc_relative_slash_path(doc_dir: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(doc_dir).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Write a capture's embedded files next to the live document so
/// relative references in the spliced output resolve.
///
/// In WASM, `runtime.file_write` targets the preview VFS — exactly
/// where the SPA/hub-client image resolvers look. Natively (tests),
/// it writes real files under the document's directory.
///
/// Fail-soft per file: a bad base64 payload or a write failure skips
/// that file with a warning; the splice proceeds (worst case is the
/// same broken image the user has today, never a broken preview).
/// Paths are validated to stay under `doc_dir` — a capture is remote
/// data, and a crafted `../…` entry must not escape the document
/// directory.
pub fn materialize_capture_files(
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    files: &[CaptureFile],
) {
    for file in files {
        if !is_safe_relative_path(&file.path) {
            tracing::warn!(
                path = %file.path,
                "capture-files: refusing to materialize non-relative or escaping path"
            );
            continue;
        }
        let target = doc_dir.join(&file.path);
        let bytes = match BASE64.decode(&file.contents_base64) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(
                    path = %file.path,
                    error = %e,
                    "capture-files: invalid base64 in capture; skipping"
                );
                continue;
            }
        };
        if let Some(parent) = target.parent()
            && let Err(e) = runtime.dir_create(parent, true)
        {
            tracing::warn!(
                path = %target.display(),
                error = %e,
                "capture-files: cannot create parent directory; skipping"
            );
            continue;
        }
        if let Err(e) = runtime.file_write(&target, &bytes) {
            tracing::warn!(
                path = %target.display(),
                error = %e,
                "capture-files: cannot write file; skipping"
            );
        }
    }
}

/// Warn threshold for a gzipped capture doc: 10 MB (decision recorded
/// in `claude-notes/plans/2026-07-23-preview-engine-supporting-files.md`).
/// Embedded figure bytes (Phase 2) can push multi-plot docs past the
/// point where syncing the capture binary doc hurts; no hard cap in
/// v1, but the operator gets a signal.
pub const CAPTURE_DOC_WARN_BYTES: usize = 10 * 1024 * 1024;

/// If a gzipped capture doc of `gzipped_len` bytes exceeds
/// [`CAPTURE_DOC_WARN_BYTES`], return the warning message to log.
/// Pure so the threshold behaviour is directly unit-testable; the
/// callers ([`gzip_captures`] and the capture-doc writers) feed it to
/// `tracing::warn!`.
pub fn capture_doc_size_warning(gzipped_len: usize) -> Option<String> {
    (gzipped_len > CAPTURE_DOC_WARN_BYTES).then(|| {
        format!(
            "capture doc is {:.1} MB gzipped (> {} MB): engine output embeds large \
             supporting files; syncing and storing this capture may be slow",
            gzipped_len as f64 / (1024.0 * 1024.0),
            CAPTURE_DOC_WARN_BYTES / (1024 * 1024),
        )
    })
}

/// Serialize + gzip an `EngineCapture` sequence — the shared wire
/// format of the capture binary doc (`application/x-engine-capture+gzip`).
/// Single implementation for the three capture writers
/// (`quarto-preview`'s eager driver and re-execute handler,
/// `quarto-hub-provider`'s exec server), all of which previously
/// duplicated the gzip block. Logs [`capture_doc_size_warning`] when
/// the result is oversized.
pub fn gzip_captures(captures: &[quarto_trace::EngineCapture]) -> std::io::Result<Vec<u8>> {
    gzip_json(&serde_json::to_vec(captures)?)
}

/// Gzip a coalesced execution record in its canonical writer bytes — the
/// payload of the record binary doc (`application/x-engine-record+gzip`,
/// EXR-13). Shares [`gzip_json`] (and so the size warning) with
/// [`gzip_captures`]; byte-for-byte deterministic for equal records.
pub fn gzip_record(record: &crate::record::Record) -> std::io::Result<Vec<u8>> {
    let text = crate::record::write_record(record)?;
    gzip_json(text.as_bytes())
}

/// The one gzip body behind [`gzip_captures`] and [`gzip_record`].
fn gzip_json(json: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Write as _;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(json)?;
    let gzipped = enc.finish()?;
    if let Some(msg) = capture_doc_size_warning(gzipped.len()) {
        tracing::warn!("{msg}");
    }
    Ok(gzipped)
}

/// Build-time default of the record-writer gate (EXR-13b). Off until release
/// N+1: readers ship a release before writers, because a store holding record
/// documents is stranded if the reader is reverted (epic decision 4).
pub const RECORD_WRITING_DEFAULT: bool = false;

/// Environment override of [`RECORD_WRITING_DEFAULT`]: `1`/`true`/`on`/`yes`
/// enables, `0`/`false`/`off`/`no` disables, anything else keeps the default.
pub const RECORD_WRITING_ENV: &str = "QUARTO_ENGINE_RECORD_WRITE";

/// The single writer gate consulted by all three capture writers.
pub fn record_writing_enabled() -> bool {
    record_writing_from(std::env::var(RECORD_WRITING_ENV).ok().as_deref())
}

/// Pure body of [`record_writing_enabled`] so the parsing is unit-testable
/// without touching the process environment.
pub fn record_writing_from(value: Option<&str>) -> bool {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("1" | "true" | "on" | "yes") => true,
        Some("0" | "false" | "off" | "no") => false,
        _ => RECORD_WRITING_DEFAULT,
    }
}

/// A capture payload ready to become a hub binary doc.
#[derive(Debug, Clone)]
pub struct EncodedCapturePayload {
    /// The gzipped bytes (`content` of the doc).
    pub gzipped: Vec<u8>,
    /// `true` for a record doc (`RECORD_MIME_TYPE`, `meta.kind: engine-record`),
    /// `false` for a legacy capture doc.
    pub is_record: bool,
    /// The `meta.engines` list: the record's `quarto_engines` names for a record,
    /// the captures' `engine_name`s for a legacy payload.
    pub engines: Vec<String>,
}

/// Choose and encode the payload a capture writer stores. A record is written
/// only when `record_gate` is on **and** the run produced one that describes
/// something (a preview cache hit has none, and an engine that builds no record
/// leaves an engineless one); otherwise the legacy capture array is written, byte-identical
/// to what the writers produced before EXR-13b.
pub fn encode_capture_payload(
    captures: &[quarto_trace::EngineCapture],
    record: Option<&crate::record::Record>,
    record_gate: bool,
) -> std::io::Result<EncodedCapturePayload> {
    match record.filter(|r| record_gate && r.describes_something()) {
        Some(record) => Ok(EncodedCapturePayload {
            gzipped: gzip_record(record)?,
            is_record: true,
            engines: record
                .metadata
                .quarto_engines
                .iter()
                .map(|e| e.name.clone())
                .collect(),
        }),
        None => Ok(EncodedCapturePayload {
            gzipped: gzip_captures(captures)?,
            is_record: false,
            engines: captures.iter().map(|c| c.engine_name.clone()).collect(),
        }),
    }
}

/// Whether a run has no form the gate-off writer can publish: a record that describes something,
/// no legacy captures, and the gate off. The legacy form would be an empty list and the page
/// would silently show nothing. This is the one statement of the rule: [`encode_for_publish`]
/// and the record-source walk both call it.
pub fn record_has_no_legacy_form(
    record_gate: bool,
    captures: &[quarto_trace::EngineCapture],
    record: Option<&crate::record::Record>,
) -> bool {
    !record_gate && captures.is_empty() && record.is_some_and(|r| r.describes_something())
}

/// Why a run could not be encoded for publishing.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The run is a record with no legacy form and the writer gate is off.
    #[error(
        "this record source needs the record writer on: set {RECORD_WRITING_ENV}=1 \
         (the record has no legacy capture form to publish)"
    )]
    RecordWriterOff,
}

/// [`encode_capture_payload`] for a publish seam: the same encoding, except that a run with no
/// legacy form under a gate that is off is refused ([`record_has_no_legacy_form`]) instead of
/// being written as an empty legacy list.
pub fn encode_for_publish(
    captures: &[quarto_trace::EngineCapture],
    record: Option<&crate::record::Record>,
    record_gate: bool,
) -> Result<EncodedCapturePayload, PublishError> {
    if record_has_no_legacy_form(record_gate, captures, record) {
        return Err(PublishError::RecordWriterOff);
    }
    Ok(encode_capture_payload(captures, record, record_gate)?)
}

/// A capture file path is safe when it is relative and contains no
/// `..` or root components — it must resolve strictly under the
/// document directory.
fn is_safe_relative_path(path: &str) -> bool {
    use std::path::Component;
    let p = Path::new(path);
    !path.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use crate::record::read_record;
    use quarto_trace::EngineCapture;
    use std::io::Read as _;

    fn record() -> crate::record::Record {
        read_record(include_str!("../../tests/fixtures/records/basic.json")).unwrap()
    }

    fn gunzip(bytes: &[u8]) -> String {
        let mut out = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn gzip_record_is_deterministic_and_round_trips() {
        let r = record();
        let a = gzip_record(&r).unwrap();
        assert_eq!(a, gzip_record(&r).unwrap());
        assert_eq!(read_record(&gunzip(&a)).unwrap(), r);
    }

    #[test]
    fn gzip_record_carries_the_canonical_writer_bytes() {
        let r = record();
        assert_eq!(
            gunzip(&gzip_record(&r).unwrap()),
            crate::record::write_record(&r).unwrap()
        );
    }

    #[test]
    fn gzip_helpers_share_one_gzip_and_warning_body() {
        // The warning decision is `capture_doc_size_warning`, called from the one
        // private `gzip_json` that both wrappers use.
        let src = include_str!("capture_files.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        assert_eq!(body.matches("GzEncoder::new").count(), 1, "one gzip body");
        assert_eq!(
            body.matches("capture_doc_size_warning(gzipped.len())")
                .count(),
            1,
            "one warning call"
        );
    }

    #[test]
    fn gate_defaults_off_and_parses_overrides() {
        assert!(!RECORD_WRITING_DEFAULT);
        assert!(!record_writing_from(None));
        assert!(!record_writing_from(Some("")));
        assert!(!record_writing_from(Some("garbage")));
        for on in ["1", "true", "TRUE", "on", "yes"] {
            assert!(record_writing_from(Some(on)), "{on}");
        }
        for off in ["0", "false", "off", "no"] {
            assert!(!record_writing_from(Some(off)), "{off}");
        }
    }

    fn capture(engine: &str) -> EngineCapture {
        serde_json::from_value(serde_json::json!({
            "engine_name": engine, "input_qmd": "x", "result": {}
        }))
        .unwrap()
    }

    #[test]
    fn gate_off_encodes_legacy_even_with_a_record() {
        let caps = vec![capture("jupyter")];
        let r = record();
        let enc = encode_capture_payload(&caps, Some(&r), false).unwrap();
        assert!(!enc.is_record);
        assert_eq!(enc.gzipped, gzip_captures(&caps).unwrap());
        assert_eq!(enc.engines, vec!["jupyter".to_string()]);
    }

    #[test]
    fn gate_on_encodes_the_record_with_its_engines() {
        let caps = vec![capture("jupyter")];
        let r = record();
        let enc = encode_capture_payload(&caps, Some(&r), true).unwrap();
        assert!(enc.is_record);
        assert_eq!(enc.gzipped, gzip_record(&r).unwrap());
        let want: Vec<String> = r
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(enc.engines, want);
    }

    #[test]
    fn gate_on_without_a_record_falls_back_to_legacy() {
        let caps = vec![capture("jupyter")];
        let enc = encode_capture_payload(&caps, None, true).unwrap();
        assert!(!enc.is_record);
    }

    /// An engine that builds no record (a passthrough or an extension engine without one)
    /// leaves the coalesced record with no engine entries. It describes nothing, so storing it
    /// would replace the legacy captures with an empty document and lose the outputs.
    #[test]
    fn gate_on_with_an_engineless_record_falls_back_to_legacy() {
        let mut empty = record();
        empty.metadata.quarto_engines.clear();
        empty.cells.clear();
        let caps = vec![capture("test-passthrough")];
        let enc = encode_capture_payload(&caps, Some(&empty), true).unwrap();
        assert!(!enc.is_record);
        assert_eq!(enc.engines, vec!["test-passthrough".to_string()]);
    }

    /// What RIU-3 imports: no engines, but cells. It describes something, so with the gate on it
    /// is stored as a record (EXR-15's rule is extended, not reversed).
    #[test]
    fn gate_on_with_an_engineless_record_with_cells_encodes_a_record() {
        let mut imported = record();
        imported.metadata.quarto_engines.clear();
        assert!(!imported.cells.is_empty());
        let enc = encode_capture_payload(&[], Some(&imported), true).unwrap();
        assert!(enc.is_record);
        assert_eq!(enc.gzipped, gzip_record(&imported).unwrap());
        assert!(enc.engines.is_empty());
    }

    fn imported() -> crate::record::Record {
        let mut r = record();
        r.metadata.quarto_engines.clear();
        r
    }

    fn nothing() -> crate::record::Record {
        let mut r = imported();
        r.cells.clear();
        r
    }

    /// The refusal predicate: a record that describes something, no captures, gate off.
    #[test]
    fn refusal_predicate_covers_every_combination() {
        let caps = [capture("jupyter")];
        let full = record();
        for (gate, captures, rec, want) in [
            (false, &[][..], Some(&full), true),
            (false, &[][..], Some(&imported()), true),
            (false, &caps[..], Some(&full), false),
            (true, &[][..], Some(&full), false),
            (true, &caps[..], Some(&full), false),
            (false, &[][..], Some(&nothing()), false),
            (false, &[][..], None, false),
            (false, &caps[..], None, false),
        ] {
            assert_eq!(
                record_has_no_legacy_form(gate, captures, rec),
                want,
                "gate={gate} captures={} record={:?}",
                captures.len(),
                rec.map(|r| r.describes_something())
            );
        }
    }

    #[test]
    fn publish_refuses_a_record_with_no_legacy_form_naming_the_variable() {
        let err = encode_for_publish(&[], Some(&imported()), false).unwrap_err();
        assert!(matches!(err, PublishError::RecordWriterOff));
        assert!(err.to_string().contains(RECORD_WRITING_ENV), "{err}");
    }

    #[test]
    fn publish_gate_off_with_captures_gives_the_legacy_form() {
        let caps = vec![capture("jupyter")];
        let enc = encode_for_publish(&caps, Some(&record()), false).unwrap();
        assert!(!enc.is_record);
        assert_eq!(enc.gzipped, gzip_captures(&caps).unwrap());
    }

    /// `encode_for_publish` and `encode_capture_payload` take the same decision for each
    /// combination, so the rule cannot drift between them.
    #[test]
    fn publish_and_encoder_agree_wherever_publish_does_not_refuse() {
        let caps = [capture("jupyter")];
        let full = record();
        let imp = imported();
        let none = nothing();
        for gate in [false, true] {
            for captures in [&[][..], &caps[..]] {
                for rec in [None, Some(&full), Some(&imp), Some(&none)] {
                    let refused = record_has_no_legacy_form(gate, captures, rec);
                    let published = encode_for_publish(captures, rec, gate);
                    assert_eq!(published.is_err(), refused);
                    if let Ok(p) = published {
                        let e = encode_capture_payload(captures, rec, gate).unwrap();
                        assert_eq!(p.is_record, e.is_record);
                        assert_eq!(p.gzipped, e.gzipped);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn collects_directory_recursively_with_doc_relative_paths() {
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().canonicalize().unwrap();
        write(&doc_dir.join("doc_files/figure-html/a.png"), b"AAA");
        write(&doc_dir.join("doc_files/figure-html/b.png"), b"BBB");
        write(&doc_dir.join("doc_files/data.csv"), b"1,2");

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let files = collect_capture_files(runtime.as_ref(), &doc_dir, &[doc_dir.join("doc_files")]);

        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec![
                "doc_files/data.csv",
                "doc_files/figure-html/a.png",
                "doc_files/figure-html/b.png",
            ]
        );
        let a = files
            .iter()
            .find(|f| f.path == "doc_files/figure-html/a.png")
            .unwrap();
        assert_eq!(BASE64.decode(&a.contents_base64).unwrap(), b"AAA");
    }

    #[test]
    fn collects_single_file_and_relative_entry() {
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().canonicalize().unwrap();
        write(&doc_dir.join("fig.png"), b"PNG");

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        // Doc-dir-relative entry (the ExecuteResult contract allows both).
        let files = collect_capture_files(runtime.as_ref(), &doc_dir, &[PathBuf::from("fig.png")]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "fig.png");
    }

    #[test]
    fn skips_files_outside_doc_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let doc_dir = root.join("proj");
        std::fs::create_dir_all(&doc_dir).unwrap();
        write(&root.join("outside.png"), b"X");

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let files = collect_capture_files(runtime.as_ref(), &doc_dir, &[root.join("outside.png")]);
        assert!(files.is_empty(), "outside-doc-dir file must be skipped");
    }

    #[test]
    fn missing_entry_is_skipped_not_fatal() {
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().canonicalize().unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let files =
            collect_capture_files(runtime.as_ref(), &doc_dir, &[doc_dir.join("never-created")]);
        assert!(files.is_empty());
    }

    #[test]
    fn materializes_files_under_doc_dir() {
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().canonicalize().unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let files = vec![CaptureFile {
            path: "doc_files/figure-html/fig.png".into(),
            contents_base64: BASE64.encode(b"PNGDATA"),
        }];
        materialize_capture_files(runtime.as_ref(), &doc_dir, &files);

        let on_disk = std::fs::read(doc_dir.join("doc_files/figure-html/fig.png")).unwrap();
        assert_eq!(on_disk, b"PNGDATA");
    }

    #[test]
    fn materialize_refuses_escaping_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let doc_dir = root.join("proj");
        std::fs::create_dir_all(&doc_dir).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        for evil in ["../evil.png", "/abs/evil.png", ""] {
            materialize_capture_files(
                runtime.as_ref(),
                &doc_dir,
                &[CaptureFile {
                    path: evil.into(),
                    contents_base64: BASE64.encode(b"X"),
                }],
            );
        }
        assert!(
            !root.join("evil.png").exists(),
            "escaping path must not be written"
        );
        assert!(!Path::new("/abs/evil.png").exists());
    }

    #[test]
    fn materialize_skips_invalid_base64() {
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().canonicalize().unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        materialize_capture_files(
            runtime.as_ref(),
            &doc_dir,
            &[CaptureFile {
                path: "fig.png".into(),
                contents_base64: "!!!not-base64!!!".into(),
            }],
        );
        assert!(!doc_dir.join("fig.png").exists());
    }

    #[test]
    fn size_warning_fires_only_above_threshold() {
        assert!(capture_doc_size_warning(0).is_none());
        assert!(capture_doc_size_warning(CAPTURE_DOC_WARN_BYTES).is_none());
        let msg = capture_doc_size_warning(CAPTURE_DOC_WARN_BYTES + 1)
            .expect("one byte over the threshold must warn");
        assert!(msg.contains("10 MB"), "message names the threshold: {msg}");
    }

    #[test]
    fn gzip_captures_roundtrips_through_gunzip() {
        use std::io::Read as _;
        let capture = quarto_trace::EngineCapture {
            engine_name: "knitr".into(),
            input_qmd: "```{r}\n1\n```\n".into(),
            result: serde_json::json!({"markdown": "out"}),
            record: None,
            files: vec![CaptureFile {
                path: "f.png".into(),
                contents_base64: BASE64.encode(b"PNG"),
            }],
        };
        let gzipped = gzip_captures(std::slice::from_ref(&capture)).unwrap();
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(&gzipped[..])
            .read_to_end(&mut json)
            .unwrap();
        let back: Vec<quarto_trace::EngineCapture> = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].files[0].path, "f.png");
    }

    #[test]
    fn round_trip_collect_then_materialize() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().canonicalize().unwrap().join("src");
        let dst_dir = tmp.path().canonicalize().unwrap().join("dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();
        write(&src_dir.join("d_files/figure-html/f.png"), b"\x89PNG\r\n");

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let files = collect_capture_files(runtime.as_ref(), &src_dir, &[src_dir.join("d_files")]);
        materialize_capture_files(runtime.as_ref(), &dst_dir, &files);

        let out = std::fs::read(dst_dir.join("d_files/figure-html/f.png")).unwrap();
        assert_eq!(out, b"\x89PNG\r\n");
    }
}
