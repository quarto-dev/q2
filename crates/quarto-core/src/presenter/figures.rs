/*
 * presenter/figures.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Figure sinks: where the presenter writes image outputs.
 */

//! Figure emission for the presenter (bd-5t6wvu7m).
//!
//! Image outputs (`image/png`, `image/jpeg`, `image/svg+xml`) are written to
//! `<doc_dir>/<stem>_files/figure-html/` — the same layout Q1's jupyter engine
//! (`mdImageOutput` + `figuresDir("html")`) and q2's knitr engine produce. Files are
//! named `cell-<cell#>-output-<out#>.<ext>` (Q1's shape). When anything was written, the
//! caller reports the `<stem>_files` directory in `ExecuteResult::supporting_files`
//! (dir-level, mirroring knitr), which is what `q2 render` copies to the output dir and
//! what the preview capture transport (bd-qbhp2cvv) embeds for browser replay.
//!
//! The presenter writes through a [`FigureSink`] so the same code can run natively
//! ([`FsFigureSink`]) and, later, against the WASM VFS. Decoding and numbering are shared
//! by every sink: only the final write differs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_system_runtime::SystemRuntime;

use super::output::extract_text_content;

/// Where the presenter writes image outputs.
pub trait FigureSink {
    /// Start figure numbering for the next cell. Called for every cell, visible or not,
    /// so file names stay keyed to a cell's position in the document.
    fn begin_cell(&mut self);

    /// Write one image output. Returns the doc-relative, forward-slash path to emit in
    /// markdown, or `None` when the payload is malformed or the write fails (the caller
    /// falls back — fail-soft, matching the engine's other output paths).
    fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String> {
        self.write_image_at(mime, value, None)
    }

    /// [`write_image`](Self::write_image) for an output whose producer named a file
    /// (`metadata.quarto_file_path`, epic decision 25). The file goes at `preferred` when that
    /// is a document-relative path inside the document directory and no other output of this
    /// document has claimed it; otherwise the sink uses its generated name and counts the
    /// fallback ([`path_fallbacks`](Self::path_fallbacks)).
    fn write_image_at(
        &mut self,
        mime: &str,
        value: &serde_json::Value,
        preferred: Option<&str>,
    ) -> Option<String>;

    /// How many outputs named a path that could not be honored.
    fn path_fallbacks(&self) -> usize {
        0
    }

    /// Image outputs that could not be used since the last call (malformed payload or a
    /// failed write), drained. The presenter reports each as a diagnostic; the output
    /// itself still falls back as described on `write_image`.
    fn take_failures(&mut self) -> Vec<FigureFailure>;

    /// Whether any figure file was successfully written.
    fn wrote_any(&self) -> bool;

    /// The `<stem>_files` directory (for supporting-files reporting).
    fn files_dir(&self) -> PathBuf;
}

/// An image output the presenter could not use (`Q-18-11`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FigureFailure {
    /// The output's mime type, e.g. `image/png`.
    pub mime: String,
    /// Why, in the decoder's or the filesystem's words.
    pub reason: String,
}

/// Decode an image output into `(file extension, bytes)`, or `None` when the mime is not
/// an image type we write or the payload is malformed.
pub fn decode_image(mime: &str, value: &serde_json::Value) -> Option<(&'static str, Vec<u8>)> {
    decode_image_checked(mime, value).ok()
}

/// [`decode_image`], with the reason a payload was rejected.
fn decode_image_checked(
    mime: &str,
    value: &serde_json::Value,
) -> Result<(&'static str, Vec<u8>), String> {
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/svg+xml" => "svg",
        _ => return Err(format!("`{mime}` is not an image type Quarto writes")),
    };
    let content = extract_text_content(value);
    // Q1 rule (`mdImageOutput`): SVG payloads that are literal `<svg` markup are written
    // as text; everything else is base64 (whitespace-stripped, padding optional).
    let bytes: Vec<u8> = if ext == "svg"
        && (content.trim_start().starts_with("<svg") || content.trim_start().starts_with("<?xml"))
    {
        content.into_bytes()
    } else {
        use base64::Engine as _;
        // Lenient on purpose: kernels disagree on wrapping (nbformat `\n`, but also `\r\n`
        // or spaces) and some (Ark) omit the trailing `=` padding. Strip all ASCII
        // whitespace and accept padded or unpadded input.
        const LENIENT: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        );
        let compact: String = content
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        match LENIENT.decode(compact) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(mime, error = %e, "jupyter figure: invalid base64; skipping");
                return Err(format!("the payload is not valid base64: {e}"));
            }
        }
    };
    Ok((ext, bytes))
}

/// Cell and output numbering plus the file layout, shared by every sink.
#[derive(Debug, Clone)]
pub struct FigureNaming {
    /// `<stem>_files` (single path component).
    files_dir_name: String,
    /// 1-based index of the cell currently being rendered.
    cell_index: usize,
    /// 1-based index of the next output within the current cell. Reset by `begin_cell`.
    output_index: usize,
    /// Failures not yet drained by `take_failures`.
    failures: Vec<FigureFailure>,
    /// Document-relative paths already handed out, so a producer path never overwrites
    /// another output's file.
    claimed: std::collections::HashSet<String>,
    /// Outputs whose producer path was refused.
    fallbacks: usize,
}

/// Whether `path` is a document-relative, forward-slash path that stays inside the
/// document directory.
fn is_inside_document(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.starts_with('/')
        && !path.contains(':')
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

impl FigureNaming {
    pub fn new(source_path: &Path) -> Self {
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        Self {
            files_dir_name: format!("{stem}_files"),
            cell_index: 0,
            output_index: 0,
            failures: Vec::new(),
            claimed: Default::default(),
            fallbacks: 0,
        }
    }

    /// Remember an image output that could not be used.
    pub fn record_failure(&mut self, mime: &str, reason: String) {
        self.failures.push(FigureFailure {
            mime: mime.to_string(),
            reason,
        });
    }

    /// Drain the recorded failures.
    pub fn take_failures(&mut self) -> Vec<FigureFailure> {
        std::mem::take(&mut self.failures)
    }

    /// Outputs whose producer path was refused.
    pub fn path_fallbacks(&self) -> usize {
        self.fallbacks
    }

    /// Claim the next output slot and return the document-relative path to write: the
    /// producer's `preferred` path when it is inside the document and unclaimed, else the
    /// generated one. The slot is claimed either way, so generated numbers do not depend on
    /// which outputs named a path.
    pub fn claim(&mut self, ext: &str, preferred: Option<&str>) -> String {
        let (_, generated) = self.next_file(ext);
        let rel = match preferred {
            Some(p) if is_inside_document(p) && !self.claimed.contains(p) => p.to_string(),
            Some(_) => {
                self.fallbacks += 1;
                generated
            }
            None => generated,
        };
        self.claimed.insert(rel.clone());
        rel
    }

    pub fn begin_cell(&mut self) {
        self.cell_index += 1;
        self.output_index = 0;
    }

    /// `<stem>_files`.
    pub fn files_dir_name(&self) -> &str {
        &self.files_dir_name
    }

    /// Claim the next output slot: `(file name, doc-relative path for markdown)`.
    pub fn next_file(&mut self, ext: &str) -> (String, String) {
        self.output_index += 1;
        let file_name = format!(
            "cell-{}-output-{}.{ext}",
            self.cell_index, self.output_index
        );
        let rel = format!("{}/figure-html/{file_name}", self.files_dir_name);
        (file_name, rel)
    }
}

/// Writes figure files to the real filesystem beside the source document.
pub struct FsFigureSink {
    /// Absolute directory of the source document.
    doc_dir: PathBuf,
    naming: FigureNaming,
    wrote_any: bool,
}

impl FsFigureSink {
    pub fn new(source_path: &Path) -> Self {
        let doc_dir = source_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self {
            doc_dir,
            naming: FigureNaming::new(source_path),
            wrote_any: false,
        }
    }
}

impl FigureSink for FsFigureSink {
    fn begin_cell(&mut self) {
        self.naming.begin_cell();
    }

    fn write_image_at(
        &mut self,
        mime: &str,
        value: &serde_json::Value,
        preferred: Option<&str>,
    ) -> Option<String> {
        let (ext, bytes) = match decode_image_checked(mime, value) {
            Ok(decoded) => decoded,
            Err(reason) => {
                self.naming.record_failure(mime, reason);
                return None;
            }
        };
        let rel = self.naming.claim(ext, preferred);
        let abs = self.doc_dir.join(&rel);
        if let Some(dir) = abs.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            tracing::warn!(dir = %dir.display(), error = %e, "jupyter figure: cannot create figures dir");
            self.naming
                .record_failure(mime, format!("cannot create `{}`: {e}", dir.display()));
            return None;
        }
        if let Err(e) = std::fs::write(&abs, &bytes) {
            tracing::warn!(file = %rel, error = %e, "jupyter figure: write failed");
            self.naming
                .record_failure(mime, format!("cannot write `{rel}`: {e}"));
            return None;
        }
        self.wrote_any = true;
        Some(rel)
    }

    fn path_fallbacks(&self) -> usize {
        self.naming.path_fallbacks()
    }

    fn take_failures(&mut self) -> Vec<FigureFailure> {
        self.naming.take_failures()
    }

    fn wrote_any(&self) -> bool {
        self.wrote_any
    }

    fn files_dir(&self) -> PathBuf {
        self.doc_dir.join(self.naming.files_dir_name())
    }
}

/// Writes figure files through a [`SystemRuntime`], so the presenter works on the WASM
/// VFS where `std::fs` compiles but fails at runtime.
pub struct RuntimeFigureSink {
    runtime: Arc<dyn SystemRuntime>,
    /// Absolute directory of the source document.
    doc_dir: PathBuf,
    naming: FigureNaming,
    wrote_any: bool,
}

impl RuntimeFigureSink {
    pub fn new(runtime: Arc<dyn SystemRuntime>, source_path: &Path) -> Self {
        let doc_dir = source_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self {
            runtime,
            doc_dir,
            naming: FigureNaming::new(source_path),
            wrote_any: false,
        }
    }
}

impl FigureSink for RuntimeFigureSink {
    fn begin_cell(&mut self) {
        self.naming.begin_cell();
    }

    fn write_image_at(
        &mut self,
        mime: &str,
        value: &serde_json::Value,
        preferred: Option<&str>,
    ) -> Option<String> {
        let (ext, bytes) = match decode_image_checked(mime, value) {
            Ok(decoded) => decoded,
            Err(reason) => {
                self.naming.record_failure(mime, reason);
                return None;
            }
        };
        let rel = self.naming.claim(ext, preferred);
        let abs = self.doc_dir.join(&rel);
        if let Some(dir) = abs.parent()
            && let Err(e) = self.runtime.dir_create(dir, true)
        {
            tracing::warn!(dir = %dir.display(), error = %e, "jupyter figure: cannot create figures dir");
            self.naming
                .record_failure(mime, format!("cannot create `{}`: {e}", dir.display()));
            return None;
        }
        if let Err(e) = self.runtime.file_write(&abs, &bytes) {
            tracing::warn!(file = %rel, error = %e, "jupyter figure: write failed");
            self.naming
                .record_failure(mime, format!("cannot write `{rel}`: {e}"));
            return None;
        }
        self.wrote_any = true;
        Some(rel)
    }

    fn path_fallbacks(&self) -> usize {
        self.naming.path_fallbacks()
    }

    fn take_failures(&mut self) -> Vec<FigureFailure> {
        self.naming.take_failures()
    }

    fn wrote_any(&self) -> bool {
        self.wrote_any
    }

    fn files_dir(&self) -> PathBuf {
        self.doc_dir.join(self.naming.files_dir_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// In-memory sink over the shared decode and numbering, optionally failing writes.
    struct MemorySink {
        naming: FigureNaming,
        files: Vec<(String, Vec<u8>)>,
        fail_writes: bool,
    }

    impl MemorySink {
        fn new(fail_writes: bool) -> Self {
            Self {
                naming: FigureNaming::new(Path::new("/doc/report.qmd")),
                files: Vec::new(),
                fail_writes,
            }
        }
    }

    impl FigureSink for MemorySink {
        fn begin_cell(&mut self) {
            self.naming.begin_cell();
        }
        fn write_image_at(
            &mut self,
            mime: &str,
            value: &serde_json::Value,
            preferred: Option<&str>,
        ) -> Option<String> {
            let (ext, bytes) = decode_image(mime, value)?;
            let rel = self.naming.claim(ext, preferred);
            if self.fail_writes {
                return None;
            }
            self.files.push((rel.clone(), bytes));
            Some(rel)
        }
        fn take_failures(&mut self) -> Vec<FigureFailure> {
            self.naming.take_failures()
        }
        fn wrote_any(&self) -> bool {
            !self.files.is_empty()
        }
        fn files_dir(&self) -> PathBuf {
            PathBuf::from("/doc/report_files")
        }
    }

    fn rels(sink: &MemorySink) -> Vec<&str> {
        sink.files.iter().map(|(r, _)| r.as_str()).collect()
    }

    #[test]
    fn numbering_runs_across_cells_and_outputs() {
        let mut sink = MemorySink::new(false);
        sink.begin_cell();
        sink.write_image("image/png", &json!(PNG));
        sink.write_image("image/jpeg", &json!(PNG));
        sink.begin_cell();
        sink.write_image("image/png", &json!(PNG));
        assert_eq!(
            rels(&sink),
            [
                "report_files/figure-html/cell-1-output-1.png",
                "report_files/figure-html/cell-1-output-2.jpeg",
                "report_files/figure-html/cell-2-output-1.png",
            ]
        );
    }

    #[test]
    fn hidden_cells_advance_numbering() {
        let mut sink = MemorySink::new(false);
        sink.begin_cell(); // a cell that wrote nothing
        sink.begin_cell();
        sink.write_image("image/png", &json!(PNG));
        assert_eq!(
            rels(&sink),
            ["report_files/figure-html/cell-2-output-1.png"]
        );
    }

    #[test]
    fn base64_newlines_are_stripped() {
        let split = format!("{}\n{}", &PNG[..20], &PNG[20..]);
        let (_, bytes) = decode_image("image/png", &json!(split)).unwrap();
        let (_, expected) = decode_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn unpadded_base64_decodes() {
        // Some kernels (Ark) emit base64 without the trailing `=` padding.
        let unpadded = PNG.trim_end_matches('=');
        assert_ne!(unpadded, PNG);
        let (_, bytes) = decode_image("image/png", &json!(unpadded)).unwrap();
        let (_, expected) = decode_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn base64_with_any_ascii_whitespace_decodes() {
        let messy = format!("{}\r\n {}\t{}\r\n", &PNG[..20], &PNG[20..40], &PNG[40..]);
        let (_, bytes) = decode_image("image/png", &json!(messy)).unwrap();
        let (_, expected) = decode_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn literal_svg_is_written_as_text() {
        let (ext, bytes) = decode_image("image/svg+xml", &json!(["<svg></svg>"])).unwrap();
        assert_eq!((ext, bytes.as_slice()), ("svg", b"<svg></svg>".as_slice()));
    }

    #[test]
    fn undecodable_payload_claims_no_output_slot() {
        let mut sink = MemorySink::new(false);
        sink.begin_cell();
        assert_eq!(sink.write_image("image/png", &json!("!!!")), None);
        assert_eq!(
            sink.write_image("image/png", &json!(PNG)).as_deref(),
            Some("report_files/figure-html/cell-1-output-1.png")
        );
    }

    #[test]
    fn svg_with_an_xml_prolog_is_written_as_text() {
        // R's svg() device writes `<?xml ...?>` before `<svg`.
        let svg = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"x\"></svg>";
        let (ext, bytes) = decode_image("image/svg+xml", &json!(svg)).unwrap();
        assert_eq!(ext, "svg");
        assert_eq!(bytes, svg.as_bytes());
    }

    #[test]
    fn non_image_mime_is_rejected() {
        assert_eq!(decode_image("text/plain", &json!("x")), None);
    }

    #[test]
    fn write_failure_returns_none() {
        let mut sink = MemorySink::new(true);
        sink.begin_cell();
        assert_eq!(sink.write_image("image/png", &json!(PNG)), None);
        assert!(!sink.wrote_any());
    }

    #[test]
    fn fs_sink_writes_under_the_document_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        let rel = sink.write_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(rel, "report_files/figure-html/cell-1-output-1.png");
        assert!(dir.path().join(&rel).is_file());
        assert!(sink.wrote_any());
        assert_eq!(sink.files_dir(), dir.path().join("report_files"));
    }

    // ── failures are recorded for the presenter to report (Q-18-11) ──

    #[test]
    fn undecodable_payload_is_recorded_once_and_drained() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        assert_eq!(sink.write_image("image/png", &json!("!!!")), None);
        let failures = sink.take_failures();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].mime, "image/png");
        assert!(failures[0].reason.contains("base64"), "{failures:?}");
        assert!(sink.take_failures().is_empty(), "drained");
    }

    #[test]
    fn unwritable_directory_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the `<stem>_files` directory must go.
        std::fs::write(dir.path().join("report_files"), "in the way").unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        assert_eq!(sink.write_image("image/png", &json!(PNG)), None);
        let failures = sink.take_failures();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].reason.contains("report_files"), "{failures:?}");
        assert!(!sink.wrote_any());
    }

    #[test]
    fn a_good_image_records_no_failure() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        assert!(sink.write_image("image/png", &json!(PNG)).is_some());
        assert!(sink.take_failures().is_empty());
    }

    // ── RuntimeFigureSink: every write goes through the SystemRuntime ──

    // ── The producer's own path (`metadata.quarto_file_path`, epic decision 25) ──

    #[test]
    fn fs_sink_writes_at_the_producers_path_inside_the_document_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        let rel = sink
            .write_image_at("image/png", &json!(PNG), Some("figs/unnamed-chunk-1-1.png"))
            .unwrap();
        assert_eq!(rel, "figs/unnamed-chunk-1-1.png");
        assert!(dir.path().join("figs/unnamed-chunk-1-1.png").is_file());
        assert_eq!(sink.path_fallbacks(), 0);
        // The generated name still counts the slot, so later outputs keep their numbers.
        let next = sink.write_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(next, "report_files/figure-html/cell-1-output-2.png");
    }

    #[test]
    fn a_colliding_path_falls_back_to_the_generated_name_and_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        let first = sink.write_image_at("image/png", &json!(PNG), Some("a.png"));
        let second = sink.write_image_at("image/png", &json!(PNG), Some("a.png"));
        assert_eq!(first.as_deref(), Some("a.png"));
        assert_eq!(
            second.as_deref(),
            Some("report_files/figure-html/cell-1-output-2.png")
        );
        assert_eq!(sink.path_fallbacks(), 1);
    }

    #[test]
    fn a_path_outside_the_document_directory_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = FsFigureSink::new(&dir.path().join("report.qmd"));
        sink.begin_cell();
        for (n, bad) in [
            "../x.png",
            "/abs/x.png",
            "a/../../x.png",
            "a\\b.png",
            "",
            "C:/x.png",
        ]
        .iter()
        .enumerate()
        {
            let rel = sink
                .write_image_at("image/png", &json!(PNG), Some(bad))
                .unwrap();
            assert_eq!(
                rel,
                format!("report_files/figure-html/cell-1-output-{}.png", n + 1),
                "{bad}"
            );
        }
        assert_eq!(sink.path_fallbacks(), 6);
        assert!(!dir.path().parent().unwrap().join("x.png").exists());
    }

    #[test]
    fn runtime_sink_honors_the_producers_path_through_the_runtime() {
        let runtime = Arc::new(RecordingRuntime::default());
        let mut sink = RuntimeFigureSink::new(runtime.clone(), Path::new("/doc/report.qmd"));
        sink.begin_cell();
        let rel = sink.write_image_at("image/png", &json!(PNG), Some("figs/a.png"));
        assert_eq!(rel.as_deref(), Some("figs/a.png"));
        let (_, png) = decode_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(
            *runtime.calls.lock().unwrap(),
            [
                Call::DirCreate(Path::new("/doc/figs").to_path_buf(), true),
                Call::FileWrite(Path::new("/doc/figs/a.png").to_path_buf(), png),
            ]
        );
    }

    use crate::presenter::recording_runtime::{Call, RecordingRuntime};

    #[test]
    fn runtime_sink_writes_every_figure_through_the_runtime() {
        let runtime = Arc::new(RecordingRuntime::default());
        // A directory that cannot exist: any `std::fs` write would be visible.
        let source = Path::new("/nonexistent-q2-presenter/report.qmd");
        let mut sink = RuntimeFigureSink::new(runtime.clone(), source);
        sink.begin_cell();
        let rel = sink.write_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(rel, "report_files/figure-html/cell-1-output-1.png");
        assert!(sink.wrote_any());
        assert_eq!(
            sink.files_dir(),
            Path::new("/nonexistent-q2-presenter/report_files")
        );

        let figures_dir = Path::new("/nonexistent-q2-presenter/report_files/figure-html");
        let (_, png) = decode_image("image/png", &json!(PNG)).unwrap();
        assert_eq!(
            *runtime.calls.lock().unwrap(),
            [
                Call::DirCreate(figures_dir.to_path_buf(), true),
                Call::FileWrite(figures_dir.join("cell-1-output-1.png"), png),
            ]
        );
        assert!(!Path::new("/nonexistent-q2-presenter").exists());
    }

    #[test]
    fn runtime_sink_matches_fs_sink_names() {
        let runtime = Arc::new(RecordingRuntime::default());
        let source = Path::new("/doc/report.qmd");
        let mut sink = RuntimeFigureSink::new(runtime, source);
        sink.begin_cell();
        sink.begin_cell();
        assert_eq!(
            sink.write_image("image/svg+xml", &json!(["<svg></svg>"]))
                .as_deref(),
            Some("report_files/figure-html/cell-2-output-1.svg")
        );
    }
}
