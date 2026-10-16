//! `POST /api/preview/import` — document import (docx/odt/rtf/epub/pptx to qmd) for the
//! embedded hub (Plan 7, P7-D1).
//!
//! The embed carries no pandoc.wasm, so the hub client's import flow runs its one pandoc step
//! here: the Rust wasm still owns the request, the format table, the transforms and the qmd;
//! only "run the reader over these bytes" comes to the server.
//!
//! This is an allow-listed route, not a "run this `PandocRequest`" route: pandoc's
//! `--filter`/`--lua-filter` would give every `--share` guest code execution on the host. The
//! server picks the format from the file name's extension, builds the argv itself, and appends
//! `--sandbox` and a heap cap.
//!
//! Request: body = the source bytes; header `file_name` = a name whose extension selects the
//! format (the client sends `source.<ext>`, so it is always ASCII).
//!
//! Responses:
//! - `200`: `application/octet-stream`, framed `[u32 BE header_len][JSON header][output][media…]`.
//!   The header is `{status, stderr, stdout, timedOut, aborted, outputLen, collected:[{path,len}],
//!   diagnostics}`. `status` is pandoc's raw exit status (`null` when it was killed); `output` is
//!   `out.json` (empty on failure); `collected` lists the extracted media in the order their
//!   bytes follow. Every occurrence of the per-request directory is rewritten to the wasm's
//!   virtual import directory, so `finish_import` and the client's manifest builder see what
//!   they would from pandoc.wasm (P7-D5).
//! - `400`: the extension is not an import format (`Q-24-1`), or `file_name` is missing (plain text).
//! - `413`: the body is over the source cap (`Q-24-2`); past the route's body limit, axum's own
//!   refusal with no body.
//! - `422`: pandoc is missing (`Q-20-1`) or too old (`Q-20-2`).
//!
//! The `400`/`413`/`422` refusals that Rust words itself carry `{ "diagnostics": [...] }`.
//! - `500`: the server could not run pandoc, or a host path survived the rewrite.
//!
//! Auth is loopback-only, inherited from the server, as for `render_download`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use quarto_core::child_runner::{ChildError, ChildLimits, run_child};
use quarto_core::import::formats::{ImportFormat, MAX_SOURCE_BYTES, format_for_file_name};
use quarto_core::import::request::{import_argv_in, import_dir, prepare_import};
use quarto_core::resolve_and_gate_pandoc;
use quarto_core::stage::{Cancellation, PipelineError};
use quarto_error_reporting::{DiagnosticMessage, JsonDiagnostic, diagnostic_to_json};
use quarto_source_map::SourceContext;
use quarto_system_runtime::NativeRuntime;
use serde::Serialize;

/// The request body cap: the source cap plus slack for the multipart-free framing of a proxy.
pub const BODY_LIMIT: usize = MAX_SOURCE_BYTES as usize + 64 * 1024;

/// Wall-clock budget for one pandoc run. RSS can exceed the heap cap by about a quarter, so
/// this is the backstop that a memory cap alone is not.
const WALL_TIMEOUT: Duration = Duration::from_secs(120);

/// `+RTS -M<n>M`: pandoc exits 251 (`Heap exhausted`) at the cap.
const HEAP_CAP_MB: u32 = 512;

/// Each collected file (`ts-packages/pandoc-host/src/limits.ts`: `collected_file_bytes`).
const COLLECTED_FILE_BYTES: u64 = 26_214_400;
/// All collected files together (`collected_total_bytes`).
const COLLECTED_TOTAL_BYTES: u64 = 314_572_800;

/// How the pandoc child is run; every field is a seam for tests.
pub(crate) struct RunConfig {
    pub pandoc: PathBuf,
    pub timeout: Duration,
    pub heap_cap_mb: u32,
}

/// A host-side diagnostic, in the shape `@quarto/pandoc-host` calls a `HostDiagnostic`.
#[derive(Debug, Serialize)]
pub(crate) struct HostDiagnostic {
    origin: &'static str,
    kind: &'static str,
    code: &'static str,
    message: String,
    path: String,
    size: u64,
}

/// What a finished (or killed) pandoc run left.
pub(crate) struct ImportRun {
    /// The raw exit status; `None` when pandoc was killed (timeout, cancel, a signal).
    pub status: Option<i32>,
    pub timed_out: bool,
    pub aborted: bool,
    pub stdout: String,
    pub stderr: String,
    /// `out.json`, with host paths rewritten; empty unless pandoc succeeded.
    pub output: Vec<u8>,
    /// Extracted media as `(virtual path, bytes)`, sorted by path.
    pub collected: Vec<(String, Vec<u8>)>,
    pub diagnostics: Vec<HostDiagnostic>,
}

#[derive(Debug)]
pub(crate) enum ImportFailure {
    /// Pandoc is missing or too old.
    Gate(Vec<DiagnosticMessage>),
    /// Anything else the server cannot recover from.
    Internal(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameHeader<'a> {
    status: Option<i32>,
    stderr: &'a str,
    stdout: &'a str,
    timed_out: bool,
    aborted: bool,
    output_len: usize,
    collected: Vec<CollectedEntry<'a>>,
    diagnostics: &'a [HostDiagnostic],
}

#[derive(Serialize)]
struct CollectedEntry<'a> {
    path: &'a str,
    len: usize,
}

/// Cancels the run when the handler future is dropped (the client went away).
struct CancelOnDrop(Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Axum handler for `POST /api/preview/import`.
pub async fn import_handler(headers: HeaderMap, body: Bytes) -> Response {
    let Some(file_name) = headers.get("file_name").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing file_name header").into_response();
    };
    // Validate-only: Q-24-1 (extension) before Q-24-2 (size).
    let checked = prepare_import(file_name, body.len() as u64, "");
    if !checked.success {
        let code = checked.diagnostics.first().and_then(|d| d.code.as_deref());
        let status = if code == Some("Q-24-2") {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        };
        return (status, diagnostics_json(&checked.diagnostics)).into_response();
    }
    // `prepare_import` succeeded, so the extension names a format.
    let Some(format) = format_for_file_name(file_name) else {
        return (StatusCode::BAD_REQUEST, "unsupported file type").into_response();
    };

    let cancel = Cancellation::new();
    let _guard = CancelOnDrop(cancel.clone());
    let source = body.to_vec();
    let outcome = tokio::task::spawn_blocking(move || {
        let pandoc = || resolve_and_gate_pandoc("import", &NativeRuntime::new());
        import_blocking(&pandoc, format, &source, cancel)
    })
    .await;
    match outcome {
        Ok(Ok(run)) => frame_response(&run),
        Ok(Err(ImportFailure::Gate(diagnostics))) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            diagnostics_json(&diagnostics),
        )
            .into_response(),
        Ok(Err(ImportFailure::Internal(message))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("import task failed: {e}"),
        )
            .into_response(),
    }
}

/// `{ "diagnostics": [...] }`: the body of every refusal that Rust words itself.
fn diagnostics_json(diagnostics: &[DiagnosticMessage]) -> Json<serde_json::Value> {
    let ctx = SourceContext::new();
    let json: Vec<JsonDiagnostic> = diagnostics
        .iter()
        .map(|d| diagnostic_to_json(d, &ctx))
        .collect();
    Json(serde_json::json!({ "diagnostics": json }))
}

/// Resolve pandoc through `resolve` (the version gate), then run the import with the default
/// limits. `resolve` is a parameter so a test can stand in for a missing pandoc without
/// touching the process environment.
pub(crate) fn import_blocking(
    resolve: &dyn Fn() -> Result<PathBuf, PipelineError>,
    format: &ImportFormat,
    source: &[u8],
    cancel: Cancellation,
) -> Result<ImportRun, ImportFailure> {
    let pandoc = resolve().map_err(|e| match e {
        PipelineError::StageError { diagnostics, .. } => ImportFailure::Gate(diagnostics),
        other => ImportFailure::Internal(other.to_string()),
    })?;
    let config = RunConfig {
        pandoc,
        timeout: WALL_TIMEOUT,
        heap_cap_mb: HEAP_CAP_MB,
    };
    run_import(&config, format, source, cancel)
}

/// Run pandoc over `source` in a fresh directory that is removed afterwards.
pub(crate) fn run_import(
    config: &RunConfig,
    format: &ImportFormat,
    source: &[u8],
    cancel: Cancellation,
) -> Result<ImportRun, ImportFailure> {
    let internal =
        |what: &str, e: &dyn std::fmt::Display| ImportFailure::Internal(format!("{what}: {e}"));
    let tmp = tempfile::TempDir::with_prefix("q2-import-")
        .map_err(|e| internal("could not create a temporary directory", &e))?;
    // macOS `/var` is `/private/var`: pandoc may print either, so the argv carries the
    // canonical form and the rewrite looks for exactly that.
    let real = tmp
        .path()
        .canonicalize()
        .map_err(|e| internal("could not resolve the temporary directory", &e))?;
    let real_dir = strip_verbatim_prefix(&real);

    let ext = format.extensions[0].trim_start_matches('.');
    let source_path = real.join(format!("source.{ext}"));
    std::fs::write(&source_path, source).map_err(|e| internal("could not write the source", &e))?;
    let source_arg = strip_verbatim_prefix(&source_path);

    let argv = import_argv_in(format, &source_arg, &real_dir);
    let mut cmd = std::process::Command::new(&config.pandoc);
    cmd.args(&argv[1..])
        .arg("--sandbox")
        .arg("+RTS")
        .arg(format!("-M{}M", config.heap_cap_mb))
        .arg("-RTS")
        .current_dir(&real);

    let limits = ChildLimits {
        deadline: Some(config.timeout),
        cancellation: Some(cancel),
    };
    let (status, timed_out, aborted, stdout, stderr) = match run_child(cmd, limits) {
        Ok(out) => (out.status.code(), false, false, out.stdout, out.stderr),
        Err(ChildError::TimedOut) => (None, true, false, Vec::new(), Vec::new()),
        Err(ChildError::Cancelled) => (None, false, true, Vec::new(), Vec::new()),
        Err(ChildError::Io(e)) => {
            return Err(ImportFailure::Internal(format!(
                "could not run pandoc ({}): {e}",
                config.pandoc.display()
            )));
        }
    };

    let remap = Remap::new(&real_dir);
    let stdout = remap.text(&String::from_utf8_lossy(&stdout))?;
    let stderr = remap.text(&String::from_utf8_lossy(&stderr))?;
    let mut run = ImportRun {
        status,
        timed_out,
        aborted,
        stdout,
        stderr,
        output: Vec::new(),
        collected: Vec::new(),
        diagnostics: Vec::new(),
    };
    if status != Some(0) {
        return Ok(run);
    }

    let out_json = std::fs::read_to_string(real.join("out.json"))
        .map_err(|e| internal("pandoc succeeded but wrote no out.json", &e))?;
    run.output = remap.json(&out_json)?.into_bytes();
    let (collected, diagnostics) = collect_media(&real.join("media"), &remap)?;
    run.collected = collected;
    run.diagnostics = diagnostics;
    Ok(run)
}

/// `\\?\C:\…` (what `canonicalize` yields on Windows) to `C:\…`; other paths unchanged.
/// UNVERIFIED on Windows: pandoc is believed to print the plain form.
fn strip_verbatim_prefix(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Replaces the per-request directory with the wasm's virtual import directory, and fails
/// closed when any trace of it survives.
struct Remap {
    /// Each spelling of the real directory to look for: as given, and with `\` as `/`.
    needles: Vec<String>,
    virtual_dir: String,
}

impl Remap {
    fn new(real_dir: &str) -> Self {
        let mut needles = vec![real_dir.to_string()];
        let slashed = real_dir.replace('\\', "/");
        if slashed != real_dir {
            needles.push(slashed);
        }
        Self {
            needles,
            virtual_dir: import_dir(),
        }
    }

    /// One string, rewritten. After a rewrite the string is a virtual path (or contains one),
    /// so Windows separators that followed the directory become `/`.
    fn string(&self, s: &str) -> String {
        let mut out = s.to_string();
        let mut changed = false;
        for needle in &self.needles {
            if out.contains(needle.as_str()) {
                out = out.replace(needle.as_str(), &self.virtual_dir);
                changed = true;
            }
        }
        if changed && out.starts_with(&self.virtual_dir) && out.contains('\\') {
            out = out.replace('\\', "/");
        }
        out
    }

    /// Whether any spelling of the real directory is still present in `s`.
    fn leaks(&self, s: &str) -> bool {
        self.needles.iter().any(|n| s.contains(n.as_str()))
    }

    fn checked(&self, s: String, what: &str) -> Result<String, ImportFailure> {
        if self.leaks(&s) {
            return Err(ImportFailure::Internal(format!(
                "a host path survived the rewrite in {what}"
            )));
        }
        Ok(s)
    }

    fn text(&self, s: &str) -> Result<String, ImportFailure> {
        self.checked(self.string(s), "pandoc's output")
    }

    /// Rewrites the string values of a JSON document (not the raw text, where a Windows path
    /// holds `\\`), and re-checks the serialized result.
    fn json(&self, text: &str) -> Result<String, ImportFailure> {
        fn walk(v: &mut serde_json::Value, f: &dyn Fn(&str) -> String) {
            match v {
                serde_json::Value::String(s) => *s = f(s),
                serde_json::Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f)),
                serde_json::Value::Object(o) => o.iter_mut().for_each(|(_, x)| walk(x, f)),
                _ => {}
            }
        }
        let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
            ImportFailure::Internal(format!("pandoc's JSON could not be read: {e}"))
        })?;
        walk(&mut value, &|s| self.string(s));
        let rewritten = serde_json::to_string(&value).map_err(|e| {
            ImportFailure::Internal(format!("pandoc's JSON could not be written: {e}"))
        })?;
        // Object keys are not rewritten; a leak there would also trip this check.
        self.checked(rewritten, "pandoc's JSON")
    }
}

/// Every regular file under `media_dir`, as `(virtual path, bytes)` sorted by path, with the
/// per-file and total caps applied in path order exactly as the browser host does
/// (`collectFiles` in `ts-packages/pandoc-host/src/shared.ts`): a file over the per-file cap,
/// or one that would take the running total over the total cap, is dropped with a
/// `collect-limit` warning and later files are still checked. Sizes come from `stat`; a file
/// is read only once it is known to fit. Symlinks and other non-regular entries are skipped.
fn collect_media(
    media_dir: &Path,
    remap: &Remap,
) -> Result<(Vec<(String, Vec<u8>)>, Vec<HostDiagnostic>), ImportFailure> {
    let mut found: Vec<(String, PathBuf, u64)> = Vec::new();
    if media_dir.is_dir() {
        walk_media(media_dir, &remap.virtual_dir_media(), &mut found)?;
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let mut collected = Vec::new();
    let mut diagnostics = Vec::new();
    let mut total: u64 = 0;
    for (virtual_path, real_path, size) in found {
        let why = if size > COLLECTED_FILE_BYTES {
            Some(format!(
                "is {size} bytes; the per-file limit is {COLLECTED_FILE_BYTES}"
            ))
        } else if total + size > COLLECTED_TOTAL_BYTES {
            Some(format!(
                "would take the collected total to {} bytes; the limit is {COLLECTED_TOTAL_BYTES}",
                total + size
            ))
        } else {
            None
        };
        if let Some(why) = why {
            diagnostics.push(HostDiagnostic {
                origin: "host",
                kind: "warning",
                code: "collect-limit",
                message: format!("collected file `{virtual_path}` {why} and was dropped"),
                path: virtual_path,
                size,
            });
            continue;
        }
        let bytes = std::fs::read(&real_path).map_err(|e| {
            ImportFailure::Internal(format!("could not read {}: {e}", real_path.display()))
        })?;
        total += bytes.len() as u64;
        collected.push((virtual_path, bytes));
    }
    Ok((collected, diagnostics))
}

impl Remap {
    fn virtual_dir_media(&self) -> String {
        format!("{}/media", self.virtual_dir)
    }
}

fn walk_media(
    dir: &Path,
    virtual_prefix: &str,
    out: &mut Vec<(String, PathBuf, u64)>,
) -> Result<(), ImportFailure> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| ImportFailure::Internal(format!("could not list {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| ImportFailure::Internal(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // `symlink_metadata` does not follow links, so a link out of the directory is skipped.
        let meta = std::fs::symlink_metadata(entry.path())
            .map_err(|e| ImportFailure::Internal(e.to_string()))?;
        let virtual_path = format!("{virtual_prefix}/{name}");
        if meta.is_dir() {
            walk_media(&entry.path(), &virtual_path, out)?;
        } else if meta.is_file() {
            out.push((virtual_path, entry.path(), meta.len()));
        }
    }
    Ok(())
}

/// `[u32 BE header_len][JSON header][output][media…]`.
fn frame_response(run: &ImportRun) -> Response {
    let header = FrameHeader {
        status: run.status,
        stderr: &run.stderr,
        stdout: &run.stdout,
        timed_out: run.timed_out,
        aborted: run.aborted,
        output_len: run.output.len(),
        collected: run
            .collected
            .iter()
            .map(|(path, bytes)| CollectedEntry {
                path,
                len: bytes.len(),
            })
            .collect(),
        diagnostics: &run.diagnostics,
    };
    let header_json = match serde_json::to_vec(&header) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not encode the response: {e}"),
            )
                .into_response();
        }
    };
    let media_len: usize = run.collected.iter().map(|(_, b)| b.len()).sum();
    let mut body = Vec::with_capacity(4 + header_json.len() + run.output.len() + media_len);
    body.extend_from_slice(&(header_json.len() as u32).to_be_bytes());
    body.extend_from_slice(&header_json);
    body.extend_from_slice(&run.output);
    for (_, bytes) in &run.collected {
        body.extend_from_slice(bytes);
    }
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_core::import::formats::IMPORT_FORMATS;
    use std::io::{Read, Write};

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../quarto-core/tests/fixtures/import-recordings"
    );

    fn fixture(name: &str, file: &str) -> Vec<u8> {
        std::fs::read(Path::new(FIXTURES).join(name).join(file))
            .unwrap_or_else(|e| panic!("{name}/{file}: {e}"))
    }

    fn format(id: &str) -> &'static ImportFormat {
        IMPORT_FORMATS.iter().find(|f| f.id == id).unwrap()
    }

    /// The real pandoc through the version gate, or `None` (the test is skipped).
    fn real_pandoc() -> Option<PathBuf> {
        match resolve_and_gate_pandoc("import", &NativeRuntime::new()) {
            Ok(p) => Some(p),
            Err(_) => {
                eprintln!("skipping: no usable pandoc on PATH");
                None
            }
        }
    }

    fn real_config(pandoc: PathBuf) -> RunConfig {
        RunConfig {
            pandoc,
            timeout: WALL_TIMEOUT,
            heap_cap_mb: HEAP_CAP_MB,
        }
    }

    // ---- the rewrite, without a process ------------------------------------------------------

    #[test]
    fn remap_rewrites_strings_and_fails_closed_on_a_surviving_path() {
        let remap = Remap::new("/tmp/q2-import-abc");
        assert_eq!(
            remap.string("see /tmp/q2-import-abc/media/a.png here"),
            "see /__q2_share__/import/media/a.png here"
        );
        let json = remap
            .json(r#"{"c":["/tmp/q2-import-abc/media/a.png"]}"#)
            .unwrap();
        assert!(json.contains("/__q2_share__/import/media/a.png"), "{json}");
        // An object key is not rewritten, so the real path survives and the run is refused.
        let err = remap
            .json(r#"{"/tmp/q2-import-abc/media/a.png":1}"#)
            .unwrap_err();
        assert!(matches!(err, ImportFailure::Internal(m) if m.contains("survived")));
    }

    #[test]
    fn remap_handles_windows_style_directories() {
        // Both the verbatim-stripped form and the slash form are looked for, and the
        // separators after the directory become `/`.
        let remap = Remap::new(r"C:\Users\x\AppData\Local\Temp\q2-import-abc");
        assert_eq!(
            remap.string(r"C:\Users\x\AppData\Local\Temp\q2-import-abc\media\a.png"),
            "/__q2_share__/import/media/a.png"
        );
        assert_eq!(
            remap.string("C:/Users/x/AppData/Local/Temp/q2-import-abc/media/a.png"),
            "/__q2_share__/import/media/a.png"
        );
        assert!(
            remap
                .json(r#"{"c":"C:\\Users\\x\\AppData\\Local\\Temp\\q2-import-abc\\media\\a.png"}"#)
                .unwrap()
                .contains("/__q2_share__/import/media/a.png")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\C:\a\b")),
            r"C:\a\b".to_string()
        );
    }

    #[test]
    fn the_collection_limits_match_the_browser_host() {
        // ts-packages/pandoc-host/src/limits.ts (DEFAULT_LIMITS).
        let limits = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ts-packages/pandoc-host/src/limits.ts"
        ))
        .unwrap();
        assert!(limits.contains(&format!("collected_file_bytes: {COLLECTED_FILE_BYTES}")));
        assert!(limits.contains(&format!("collected_total_bytes: {COLLECTED_TOTAL_BYTES}")));
    }

    #[test]
    fn collection_sorts_by_path_applies_both_caps_and_skips_symlinks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let media = tmp.path().join("media");
        std::fs::create_dir_all(media.join("sub")).unwrap();
        std::fs::write(media.join("b.png"), b"bb").unwrap();
        std::fs::write(media.join("a.png"), b"a").unwrap();
        std::fs::write(media.join("sub/c.png"), b"ccc").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", media.join("link.png")).unwrap();
        let remap = Remap::new("/does/not/matter");
        let (collected, diagnostics) = collect_media(&media, &remap).unwrap();
        let paths: Vec<&str> = collected.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/__q2_share__/import/media/a.png",
                "/__q2_share__/import/media/b.png",
                "/__q2_share__/import/media/sub/c.png",
            ]
        );
        assert!(diagnostics.is_empty());
    }

    // ---- the real pandoc -----------------------------------------------------------------------

    #[test]
    fn native_media_paths_are_remapped_to_virtual_prefix() {
        let Some(pandoc) = real_pandoc() else { return };
        let run = run_import(
            &real_config(pandoc),
            format("docx"),
            &fixture("images-docx", "source.docx"),
            Cancellation::new(),
        )
        .map_err(|e| format!("{e:?}"))
        .unwrap();
        assert_eq!(run.status, Some(0), "{}", run.stderr);
        let json = String::from_utf8(run.output.clone()).unwrap();
        assert!(!json.contains("q2-import-"), "a host path leaked: {json}");
        assert!(json.contains("/__q2_share__/import/media/"), "{json}");
        assert!(!run.collected.is_empty());
        for (path, bytes) in &run.collected {
            assert!(path.starts_with("/__q2_share__/import/media/"), "{path}");
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn every_fixture_reads_to_the_recorded_json_and_status() {
        let Some(pandoc) = real_pandoc() else { return };
        for entry in std::fs::read_dir(FIXTURES).unwrap() {
            let entry = entry.unwrap();
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let source = std::fs::read_dir(Path::new(FIXTURES).join(&name))
                .unwrap()
                .filter_map(|e| e.ok())
                .find(|e| e.file_name().to_string_lossy().starts_with("source."))
                .map(|e| e.file_name().to_string_lossy().into_owned());
            let Some(source) = source else { continue };
            let fmt = format_for_file_name(&source).unwrap();
            let run = run_import(
                &real_config(pandoc.clone()),
                fmt,
                &fixture(&name, &source),
                Cancellation::new(),
            )
            .map_err(|e| format!("{e:?}"))
            .unwrap();
            let status: serde_json::Value =
                serde_json::from_slice(&fixture(&name, "status.json")).unwrap();
            assert_eq!(
                i64::from(run.status.unwrap()),
                status["status"].as_i64().unwrap(),
                "{name}"
            );
            if run.status == Some(0) {
                let recorded: serde_json::Value =
                    serde_json::from_slice(&fixture(&name, "pandoc.json")).unwrap();
                let got: serde_json::Value = serde_json::from_slice(&run.output).unwrap();
                assert_eq!(got, recorded, "{name}: JSON differs from the recording");
                // The recording keeps the extracted media under `media/`.
                let recorded_media = Path::new(FIXTURES).join(&name).join("media");
                let mut expected = Vec::new();
                if recorded_media.is_dir() {
                    let mut found = Vec::new();
                    walk_media(&recorded_media, "/__q2_share__/import/media", &mut found)
                        .map_err(|e| format!("{e:?}"))
                        .unwrap();
                    for (path, real, _) in found {
                        expected.push((path, std::fs::read(real).unwrap()));
                    }
                    expected.sort();
                }
                let paths =
                    |v: &[(String, Vec<u8>)]| v.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
                assert_eq!(
                    paths(&run.collected),
                    paths(&expected),
                    "{name}: media paths"
                );
                assert!(run.collected == expected, "{name}: media bytes differ");

                // `finish_import` accepts a native run exactly as it does the recording
                // (the media stored as they are; the EMF/WMF flagged as unconverted, the way
                // the recorded `expected.qmd` was made).
                use sha2::{Digest, Sha256};
                let manifest: Vec<serde_json::Value> = run
                    .collected
                    .iter()
                    .map(|(path, bytes)| {
                        let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
                        let mut entry = serde_json::json!({
                            "pandoc_path": path,
                            "status": "stored",
                            "sha256": Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect::<String>(),
                            "ext": ext,
                        });
                        if matches!(ext, "emf" | "wmf") {
                            entry["conversion_failed"] = serde_json::json!(true);
                        }
                        entry
                    })
                    .collect();
                let finished = quarto_core::import::finish_import(
                    std::str::from_utf8(&run.output).unwrap(),
                    &run.stderr,
                    &format!("{name}.qmd"),
                    &serde_json::to_string(&manifest).unwrap(),
                    Some(fmt.id),
                );
                assert!(finished.success, "{name}: {:?}", finished.diagnostics);
                assert!(
                    !finished
                        .diagnostics
                        .iter()
                        .any(|d| d.code.as_deref() == Some("Q-24-12")),
                    "{name}: {:?}",
                    finished.diagnostics
                );
                let expected_qmd = Path::new(FIXTURES).join(&name).join("expected.qmd");
                if expected_qmd.exists() {
                    assert_eq!(
                        finished.qmd.as_deref(),
                        Some(std::fs::read_to_string(expected_qmd).unwrap().as_str()),
                        "{name}: the qmd from a native run differs from expected.qmd"
                    );
                }
            } else {
                assert!(run.output.is_empty(), "{name}");
            }
        }
    }

    /// An epub whose content references the host (a file with a marker) and the network from
    /// HTML, CSS and SVG. `--sandbox` must keep all of them out of the result.
    fn crafted_epub(port: u16, secret: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut add = |name: &str, body: &str| {
            zip.start_file(name, stored).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        add("mimetype", "application/epub+zip");
        add(
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        );
        add(
            "content.opf",
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>T</dc:title><dc:language>en</dc:language></metadata><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/><item id="s" href="s.css" media-type="text/css"/><item id="f" href="font.woff" media-type="font/woff"/><item id="v" href="v.svg" media-type="image/svg+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#,
        );
        // References from HTML (images, an object, media, an iframe, a stylesheet), from CSS
        // (a font face, an import) and from SVG (an image), each to a host file or the network.
        add(
            "c.xhtml",
            &format!(
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>T</title><link rel="stylesheet" href="s.css"/></head><body><p>hi</p><img src="file://{secret}"/><img src="//127.0.0.1:{port}/x.png"/><img src="http://127.0.0.1:{port}/y.png"/><img src="v.svg"/><object data="file://{secret}"></object><audio src="http://127.0.0.1:{port}/a.mp3"></audio><iframe src="file://{secret}"></iframe></body></html>"#
            ),
        );
        add(
            "s.css",
            &format!(
                "@import url(//127.0.0.1:{port}/z.css);@font-face{{font-family:F;src:url(file://{secret})}}@font-face{{font-family:G;src:url(font.woff)}}body{{background:url(http://127.0.0.1:{port}/bg.png)}}"
            ),
        );
        add("font.woff", "not a font");
        add(
            "v.svg",
            &format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image href="file://{secret}"/><image xlink:href="http://127.0.0.1:{port}/s.png"/></svg>"#
            ),
        );
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_crafted_epub_yields_no_host_data_and_makes_no_connection() {
        let Some(pandoc) = real_pandoc() else { return };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = tempfile::TempDir::new().unwrap();
        let secret_file = dir.path().join("secret.txt");
        std::fs::write(&secret_file, "SECRET-MARKER-7f3a").unwrap();
        let run = run_import(
            &real_config(pandoc),
            format("epub"),
            &crafted_epub(port, &secret_file.to_string_lossy()),
            Cancellation::new(),
        )
        .map_err(|e| format!("{e:?}"))
        .unwrap();
        assert!(
            listener.accept().is_err(),
            "pandoc connected to the loopback listener"
        );
        let marker = b"SECRET-MARKER-7f3a";
        let holds = |bytes: &[u8]| bytes.windows(marker.len()).any(|w| w == marker);
        for (path, bytes) in &run.collected {
            assert!(!holds(bytes), "{path} holds the host file");
        }
        assert!(
            !holds(&run.output) && !holds(run.stderr.as_bytes()) && !holds(run.stdout.as_bytes())
        );
        // Whatever happened to the references, the run itself is bounded and deterministic.
        assert!(run.status.is_some(), "{}", run.stderr);
    }

    /// A docx whose `document.xml` is `megabytes` MB of paragraphs: a few KB on disk.
    fn expanding_docx(megabytes: usize) -> Vec<u8> {
        let src = fixture("basic-docx", "source.docx");
        let mut input = zip::ZipArchive::new(std::io::Cursor::new(src)).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for i in 0..input.len() {
            let mut file = input.by_index(i).unwrap();
            if file.is_dir() {
                continue;
            }
            let name = file.name().unwrap().to_string();
            out.start_file(&name, options).unwrap();
            if name == "word/document.xml" {
                out.write_all(
                    br#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
                )
                .unwrap();
                let para = b"<w:p><w:r><w:t>aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</w:t></w:r></w:p>";
                for _ in 0..(megabytes * 1024 * 1024 / para.len()) {
                    out.write_all(para).unwrap();
                }
                out.write_all(b"</w:body></w:document>").unwrap();
            } else {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).unwrap();
                out.write_all(&buf).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn an_expanding_docx_hits_the_heap_cap() {
        let Some(pandoc) = real_pandoc() else { return };
        let bomb = expanding_docx(48);
        assert!(
            bomb.len() < 2 * 1024 * 1024,
            "the bomb must stay small: {}",
            bomb.len()
        );
        let config = RunConfig {
            heap_cap_mb: 32,
            ..real_config(pandoc)
        };
        let run = run_import(&config, format("docx"), &bomb, Cancellation::new())
            .map_err(|e| format!("{e:?}"))
            .unwrap();
        assert_eq!(run.status, Some(251), "stderr: {}", run.stderr);
        assert!(run.stderr.contains("Heap exhausted"), "{}", run.stderr);
        assert!(run.output.is_empty() && run.collected.is_empty());
    }

    // ---- a fake pandoc ---------------------------------------------------------------------------

    /// A script standing in for pandoc: it ignores its arguments and sleeps.
    #[cfg(unix)]
    fn sleeping_pandoc(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-pandoc");
        std::fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(not(unix))]
    fn sleeping_pandoc(dir: &Path) -> PathBuf {
        let path = dir.join("fake-pandoc.cmd");
        std::fs::write(&path, "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n").unwrap();
        path
    }

    #[test]
    fn a_wall_clock_timeout_kills_pandoc_and_is_reported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = RunConfig {
            pandoc: sleeping_pandoc(tmp.path()),
            timeout: Duration::from_millis(300),
            heap_cap_mb: HEAP_CAP_MB,
        };
        let started = std::time::Instant::now();
        let run = run_import(&config, format("docx"), b"x", Cancellation::new())
            .map_err(|e| format!("{e:?}"))
            .unwrap();
        assert!(run.timed_out && !run.aborted && run.status.is_none());
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn cancelling_kills_pandoc_and_is_reported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = RunConfig {
            pandoc: sleeping_pandoc(tmp.path()),
            timeout: Duration::from_secs(60),
            heap_cap_mb: HEAP_CAP_MB,
        };
        let cancel = Cancellation::new();
        cancel.cancel();
        let run = run_import(&config, format("docx"), b"x", cancel)
            .map_err(|e| format!("{e:?}"))
            .unwrap();
        assert!(run.aborted && !run.timed_out && run.status.is_none());
    }

    /// A script that writes `out.json` and a stderr line, both naming the request directory,
    /// then exits 0; `$LEAK_KEY` selects an object-key leak instead of a string value.
    #[cfg(unix)]
    fn leaking_pandoc(dir: &Path, key_leak: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(if key_leak { "leak-key" } else { "leak-value" });
        let json = if key_leak {
            r#"{"'"$d"'/media/a.png":1}"#
        } else {
            r#"{"c":"'"$d"'/media/a.png"}"#
        };
        let script = format!(
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift;; esac; shift; done\n\
             d=$(dirname \"$out\")\necho \"warning: $d/media/a.png\" >&2\necho \"note $d\"\n\
             echo '{json}' > \"$out\"\n"
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn the_remap_covers_stderr_stdout_and_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = RunConfig {
            pandoc: leaking_pandoc(tmp.path(), false),
            timeout: Duration::from_secs(20),
            heap_cap_mb: HEAP_CAP_MB,
        };
        let run = run_import(&config, format("docx"), b"x", Cancellation::new())
            .map_err(|e| format!("{e:?}"))
            .unwrap();
        assert_eq!(run.status, Some(0));
        assert_eq!(
            run.stderr.trim(),
            "warning: /__q2_share__/import/media/a.png"
        );
        assert_eq!(run.stdout.trim(), "note /__q2_share__/import");
        assert!(
            String::from_utf8(run.output)
                .unwrap()
                .contains("/__q2_share__/import/media/a.png")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_surviving_real_path_is_an_error_not_a_response() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = RunConfig {
            pandoc: leaking_pandoc(tmp.path(), true),
            timeout: Duration::from_secs(20),
            heap_cap_mb: HEAP_CAP_MB,
        };
        let err = run_import(&config, format("docx"), b"x", Cancellation::new())
            .err()
            .expect("the run must fail closed");
        assert!(matches!(err, ImportFailure::Internal(m) if m.contains("survived")));
    }

    #[test]
    fn a_pandoc_that_cannot_start_is_an_internal_error() {
        let config = RunConfig {
            pandoc: PathBuf::from("/definitely/not/pandoc"),
            timeout: Duration::from_secs(5),
            heap_cap_mb: HEAP_CAP_MB,
        };
        let err = run_import(&config, format("docx"), b"x", Cancellation::new())
            .err()
            .expect("spawn must fail");
        assert!(matches!(err, ImportFailure::Internal(m) if m.contains("could not run pandoc")));
    }

    // ---- the gate and the handler ---------------------------------------------------------------

    #[test]
    fn missing_pandoc_is_q_20_1_and_too_old_is_q_20_2() {
        use quarto_core::pandoc_filters::version::gate;
        for (found, code) in [(None, "Q-20-1"), (Some("pandoc 1.0"), "Q-20-2")] {
            let resolve = move || -> Result<PathBuf, PipelineError> {
                Err(PipelineError::stage_error_with_diagnostics(
                    "import",
                    vec![gate(found).unwrap_err()],
                ))
            };
            let err = import_blocking(&resolve, format("docx"), b"x", Cancellation::new())
                .err()
                .expect("the gate refuses");
            match err {
                ImportFailure::Gate(d) => assert_eq!(d[0].code.as_deref(), Some(code)),
                other => panic!("{other:?}"),
            }
        }
    }

    async fn call(app: axum::Router, file_name: Option<&str>, body: Vec<u8>) -> Response {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/preview/import");
        if let Some(n) = file_name {
            req = req.header("file_name", n);
        }
        app.oneshot(req.body(axum::body::Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    fn app() -> axum::Router {
        axum::Router::new().route(
            "/api/preview/import",
            axum::routing::post(import_handler)
                .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT)),
        )
    }

    #[tokio::test]
    async fn an_unsupported_extension_or_missing_name_is_400() {
        for name in [Some("notes.md"), Some("noextension"), None] {
            let response = call(app(), name, b"hello".to_vec()).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name:?}");
        }
        // Rust words the extension refusal (Q-24-1) itself.
        let response = call(app(), Some("notes.md"), b"hello".to_vec()).await;
        let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(body["diagnostics"][0]["code"], "Q-24-1");
    }

    #[tokio::test]
    async fn a_source_over_the_cap_is_413() {
        // Over the source cap but under the route's body limit: Rust's Q-24-2 refuses it.
        let over_cap = vec![0u8; MAX_SOURCE_BYTES as usize + 1];
        let response = call(app(), Some("big.docx"), over_cap).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert_eq!(body["diagnostics"][0]["code"], "Q-24-2");
        // Over the body limit: the layer refuses it before the handler runs.
        let over_limit = vec![0u8; BODY_LIMIT + 1];
        let response = call(app(), Some("big.docx"), over_limit).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn frames_are_length_prefixed_header_output_then_media() {
        let run = ImportRun {
            status: Some(0),
            timed_out: false,
            aborted: false,
            stdout: String::new(),
            stderr: "warn".into(),
            output: b"{}".to_vec(),
            collected: vec![
                ("/m/a.png".into(), vec![1, 2, 3]),
                ("/m/b.png".into(), vec![4]),
            ],
            diagnostics: Vec::new(),
        };
        let response = frame_response(&run);
        assert_eq!(response.status(), StatusCode::OK);
        let body = futures_body(response);
        let header_len = u32::from_be_bytes(body[..4].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&body[4..4 + header_len]).unwrap();
        assert_eq!(header["status"], 0);
        assert_eq!(header["outputLen"], 2);
        assert_eq!(header["collected"][1]["len"], 1);
        assert_eq!(&body[4 + header_len..], b"{}\x01\x02\x03\x04");
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    fn futures_body(response: Response) -> Vec<u8> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        })
    }
}
