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
    fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String>;

    /// Whether any figure file was successfully written.
    fn wrote_any(&self) -> bool;

    /// The `<stem>_files` directory (for supporting-files reporting).
    fn files_dir(&self) -> PathBuf;
}

/// Decode an image output into `(file extension, bytes)`, or `None` when the mime is not
/// an image type we write or the payload is malformed.
pub fn decode_image(mime: &str, value: &serde_json::Value) -> Option<(&'static str, Vec<u8>)> {
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/svg+xml" => "svg",
        _ => return None,
    };
    let content = extract_text_content(value);
    // Q1 rule (`mdImageOutput`): SVG payloads that are literal `<svg` markup are written
    // as text; everything else is base64 (strip the nbformat multiline newlines first).
    let bytes: Vec<u8> = if ext == "svg" && content.trim_start().starts_with("<svg") {
        content.into_bytes()
    } else {
        use base64::Engine as _;
        match base64::engine::general_purpose::STANDARD.decode(content.replace('\n', "")) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(mime, error = %e, "jupyter figure: invalid base64; skipping");
                return None;
            }
        }
    };
    Some((ext, bytes))
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
        }
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

    fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String> {
        let (ext, bytes) = decode_image(mime, value)?;
        let (file_name, rel) = self.naming.next_file(ext);
        let abs = self
            .doc_dir
            .join(self.naming.files_dir_name())
            .join("figure-html");
        if let Err(e) = std::fs::create_dir_all(&abs) {
            tracing::warn!(dir = %abs.display(), error = %e, "jupyter figure: cannot create figures dir");
            return None;
        }
        if let Err(e) = std::fs::write(abs.join(&file_name), &bytes) {
            tracing::warn!(file = %file_name, error = %e, "jupyter figure: write failed");
            return None;
        }
        self.wrote_any = true;
        Some(rel)
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

    fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String> {
        let (ext, bytes) = decode_image(mime, value)?;
        let (file_name, rel) = self.naming.next_file(ext);
        let abs = self
            .doc_dir
            .join(self.naming.files_dir_name())
            .join("figure-html");
        if let Err(e) = self.runtime.dir_create(&abs, true) {
            tracing::warn!(dir = %abs.display(), error = %e, "jupyter figure: cannot create figures dir");
            return None;
        }
        if let Err(e) = self.runtime.file_write(&abs.join(&file_name), &bytes) {
            tracing::warn!(file = %file_name, error = %e, "jupyter figure: write failed");
            return None;
        }
        self.wrote_any = true;
        Some(rel)
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
        fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String> {
            let (ext, bytes) = decode_image(mime, value)?;
            let (_, rel) = self.naming.next_file(ext);
            if self.fail_writes {
                return None;
            }
            self.files.push((rel.clone(), bytes));
            Some(rel)
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

    // ── RuntimeFigureSink: every write goes through the SystemRuntime ──

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
