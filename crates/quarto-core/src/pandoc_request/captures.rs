//! Per-chapter capture inputs for a pandoc request.
//!
//! The host collects each chapter's recorded engine results (the gzipped
//! `EngineCapture[]` in the project's `captures` sidecar) and passes them
//! keyed by *sidecar key*: the path the editor uses (`currentFile.path`),
//! `/`-normalized and relative to the hub's VFS project root. Rust's own
//! chapter paths are `project.dir`-relative, so one helper
//! ([`sidecar_key`]) converts, and no other code compares the two spaces.

use std::path::Path;

use quarto_system_runtime::SystemRuntime;

use super::normalize_request_path_str;

/// What a capture document's payload holds, decided from the payload itself
/// (EXR-13). The TS layer fetches bytes without the MIME type, so the dispatch is
/// here, in Rust, on the decompressed JSON.
#[derive(Debug, Clone, Default)]
pub enum CapturePayload {
    /// No payload (absent or zero bytes).
    #[default]
    Empty,
    /// The legacy `EngineCapture[]` shape (or a stale single capture object).
    Legacy(Vec<quarto_trace::EngineCapture>),
    /// A coalesced engine execution record (an nbformat notebook).
    Record(std::sync::Arc<crate::record::Record>),
}

impl CapturePayload {
    /// The legacy captures this payload carries. A record yields none: it reaches the
    /// splice through [`CapturePayload::Record`] (EXR-14), never through the legacy path.
    pub fn into_captures(self) -> Vec<quarto_trace::EngineCapture> {
        match self {
            CapturePayload::Legacy(captures) => captures,
            CapturePayload::Empty | CapturePayload::Record(_) => Vec::new(),
        }
    }

    /// Whether there is nothing to splice (no payload, or an empty legacy list).
    pub fn is_empty(&self) -> bool {
        match self {
            CapturePayload::Empty => true,
            CapturePayload::Legacy(captures) => captures.is_empty(),
            CapturePayload::Record(_) => false,
        }
    }

    /// Names of the engines whose output this payload serves, so the engine stage can
    /// suppress its "(no execution)" warning for them: the captures' engine names, or the
    /// record's `quarto_engines` entries.
    pub fn served_engine_names(&self) -> std::collections::HashSet<String> {
        match self {
            CapturePayload::Empty => Default::default(),
            CapturePayload::Legacy(captures) => {
                captures.iter().map(|c| c.engine_name.clone()).collect()
            }
            CapturePayload::Record(record) => record
                .metadata
                .quarto_engines
                .iter()
                .map(|e| e.name.clone())
                .collect(),
        }
    }
}

impl From<Vec<quarto_trace::EngineCapture>> for CapturePayload {
    fn from(captures: Vec<quarto_trace::EngineCapture>) -> Self {
        if captures.is_empty() {
            CapturePayload::Empty
        } else {
            CapturePayload::Legacy(captures)
        }
    }
}

impl From<std::sync::Arc<crate::record::Record>> for CapturePayload {
    fn from(record: std::sync::Arc<crate::record::Record>) -> Self {
        CapturePayload::Record(record)
    }
}

/// Whether a legacy payload still presents. A legacy payload degrades to
/// [`CapturePayload::Empty`] (cells render as source) plus one notice when this is
/// `true`. It is off while legacy captures still present; flipping it is the whole
/// switch for EXR-17's "old captures degrade to no capture" window.
pub const DEGRADE_LEGACY_CAPTURES: bool = false;

/// Parse a capture document's gzipped payload, dispatching on its shape: a top-level
/// object with an `nbformat` key is a record; an array, or an object with
/// `engine_name`, is a legacy capture; anything else is an error naming both shapes.
///
/// `doc` names the document for the degrade notice only.
pub fn parse_capture_payload(bytes: Option<&[u8]>, doc: &str) -> Result<CapturePayload, String> {
    let (payload, notice) = parse_capture_payload_with(bytes, doc, DEGRADE_LEGACY_CAPTURES)?;
    if let Some(notice) = notice {
        tracing::warn!("{notice}");
    }
    Ok(payload)
}

/// [`parse_capture_payload`] with the degrade flag explicit; returns the notice
/// (at most one) instead of logging it.
fn parse_capture_payload_with(
    bytes: Option<&[u8]>,
    doc: &str,
    degrade_legacy: bool,
) -> Result<(CapturePayload, Option<String>), String> {
    use std::io::Read;

    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
        return Ok((CapturePayload::Empty, None));
    };
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .map_err(|e| format!("ungzip failed: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&json).map_err(|e| format!("JSON parse failed: {e}"))?;

    // `get` is `None` on anything but an object, so these are object-only tests.
    let is_record = value.get("nbformat").is_some();
    let is_capture = value.get("engine_name").is_some();
    if is_record && is_capture {
        return Err(
            "ambiguous capture payload: the object has both `nbformat` (a record) and \
             `engine_name` (a legacy capture)"
                .to_string(),
        );
    }
    if is_record {
        return crate::record::read_record_value(value)
            .map(|r| (CapturePayload::Record(std::sync::Arc::new(r)), None))
            .map_err(|e| format!("record parse failed: {e}"));
    }
    if value.is_array() || is_capture {
        let captures = legacy::parse(value)?;
        return Ok(legacy::arm(captures, doc, degrade_legacy));
    }
    Err(
        "unrecognized capture payload: expected a record (an object with `nbformat`) or a \
         legacy capture (an array, or an object with `engine_name`)"
            .to_string(),
    )
}

/// The `Legacy` arm: the only code that deserializes `EngineCapture` or applies its
/// degrade policy. Removing legacy captures (EXR-17) deletes this module and the
/// `Legacy` variant.
mod legacy {
    use super::CapturePayload;

    /// An array of captures, or a stale single capture object.
    pub(super) fn parse(
        value: serde_json::Value,
    ) -> Result<Vec<quarto_trace::EngineCapture>, String> {
        match serde_json::from_value::<Vec<quarto_trace::EngineCapture>>(value.clone()) {
            Ok(captures) => Ok(captures),
            Err(array_err) => match serde_json::from_value::<quarto_trace::EngineCapture>(value) {
                Ok(single) => Ok(vec![single]),
                Err(_) => Err(format!("JSON parse failed: {array_err}")),
            },
        }
    }

    pub(super) fn arm(
        captures: Vec<quarto_trace::EngineCapture>,
        doc: &str,
        degrade: bool,
    ) -> (CapturePayload, Option<String>) {
        if degrade {
            (
                CapturePayload::Empty,
                Some(format!(
                    "{doc}: the recorded results are in the legacy capture format, which is \
                     no longer presented; its code is shown as source"
                )),
            )
        } else {
            (CapturePayload::Legacy(captures), None)
        }
    }
}

/// The sidecar key of `path` given the VFS project root `root`.
///
/// Accepts the three forms callers have: a path that is already relative
/// (returned `/`-normalized), an absolute path under `root` (the root is
/// stripped), and an absolute path outside `root` (its leading `/` is
/// stripped; the existing wasm tests put a book at `/b` while the root is
/// `/project`). Pure, so it is tested without a runtime.
pub fn sidecar_key_from(root: &Path, path: &Path) -> String {
    let normalized = normalize_request_path_str(&path.to_string_lossy());
    let root = normalize_request_path_str(&root.to_string_lossy());
    if !normalized.starts_with('/') {
        return normalized;
    }
    let root_prefix = if root.ends_with('/') {
        root.clone()
    } else {
        format!("{root}/")
    };
    if let Some(rest) = normalized.strip_prefix(&root_prefix) {
        return rest.to_string();
    }
    normalized.trim_start_matches('/').to_string()
}

/// [`sidecar_key_from`] with the runtime's working directory as the root
/// (`WasmRuntime::cwd` is the hub's VFS project root).
pub fn sidecar_key(runtime: &dyn SystemRuntime, path: &Path) -> String {
    match runtime.cwd() {
        Ok(root) => sidecar_key_from(&root, path),
        Err(_) => sidecar_key_from(Path::new("/"), path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gz(json: &str) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(json.as_bytes()).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn sidecar_key_takes_all_three_forms() {
        let root = Path::new("/project");
        assert_eq!(sidecar_key_from(root, Path::new("ch1.qmd")), "ch1.qmd");
        assert_eq!(
            sidecar_key_from(root, Path::new("./sub/../ch1.qmd")),
            "ch1.qmd"
        );
        assert_eq!(
            sidecar_key_from(root, Path::new("/project/ch1.qmd")),
            "ch1.qmd"
        );
        assert_eq!(
            sidecar_key_from(root, Path::new("/project/sub/a.qmd")),
            "sub/a.qmd"
        );
        // Outside the root: the leading `/` is stripped.
        assert_eq!(sidecar_key_from(root, Path::new("/b/one.qmd")), "b/one.qmd");
        // A prefix that is not a path component of the root does not count.
        assert_eq!(
            sidecar_key_from(root, Path::new("/projects/x.qmd")),
            "projects/x.qmd"
        );
    }

    #[test]
    fn sidecar_key_handles_a_book_below_the_root() {
        // `_quarto.yml` in `book/`, the VFS root `/project`: the sidecar key
        // keeps the `book/` prefix that `project.dir`-relative paths lack.
        let root = Path::new("/project");
        assert_eq!(
            sidecar_key_from(root, Path::new("/project/book/one.qmd")),
            "book/one.qmd"
        );
    }

    const ONE: &str = r#"{"engine_name":"r","input_qmd":"","result":null,"files":[]}"#;

    fn record_json() -> String {
        use crate::engine::units::scan_units;
        use crate::record::{
            AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
            fill_ids, write_record,
        };
        let mut units = scan_units("# T\n\n```{python}\nprint(1)\n```\n");
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let def = HashDefinition::conservative("jupyter");
        let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
        b.push(
            0,
            UnitResult {
                outputs: vec![serde_json::json!({
                    "output_type": "stream", "name": "stdout", "text": ["1\n"]
                })],
                execution_count: Some(1),
                kernel: Some("python3".into()),
            },
        )
        .unwrap();
        let entry = b
            .finish(EngineEntryInput {
                owned_languages: vec!["python".into()],
                ..Default::default()
            })
            .unwrap();
        write_record(&coalesce(&[entry], &units).unwrap()).unwrap()
    }

    fn parse(bytes: Option<&[u8]>) -> Result<CapturePayload, String> {
        parse_capture_payload(bytes, "a.qmd")
    }

    /// `hub-client`'s WASM tests read this file as their record payload, so it must
    /// stay a record the reader accepts (and it carries the one python cell they use).
    #[test]
    fn the_checked_in_record_fixture_is_a_record() {
        let text = include_str!("../../tests/fixtures/records/basic.json");
        let Ok(CapturePayload::Record(r)) = parse(Some(&gz(text))) else {
            panic!("fixture must be a Record");
        };
        assert_eq!(r.cells.len(), 1);
        assert_eq!(r.cells[0].metadata.quarto_lang, "python");
    }

    #[test]
    fn payload_is_empty_for_none_and_empty() {
        assert!(matches!(parse(None), Ok(CapturePayload::Empty)));
        assert!(matches!(parse(Some(&[])), Ok(CapturePayload::Empty)));
    }

    #[test]
    fn legacy_capture_payload_reads_an_array_and_a_single_object() {
        let Ok(CapturePayload::Legacy(v)) = parse(Some(&gz(&format!("[{ONE}]")))) else {
            panic!("array must be Legacy");
        };
        assert_eq!(v.len(), 1);
        let Ok(CapturePayload::Legacy(v)) = parse(Some(&gz(ONE))) else {
            panic!("single object must be Legacy");
        };
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn payload_reads_a_notebook_object_as_a_record() {
        let Ok(CapturePayload::Record(r)) = parse(Some(&gz(&record_json()))) else {
            panic!("notebook must be a Record");
        };
        assert_eq!(r.cells.len(), 1);
    }

    #[test]
    fn payload_errors_are_distinct() {
        let corrupt = parse(Some(b"not gzip")).unwrap_err();
        assert!(corrupt.contains("ungzip"), "{corrupt}");
        let not_json = parse(Some(&gz("{oops"))).unwrap_err();
        assert!(not_json.contains("JSON parse failed"), "{not_json}");
        let neither = parse(Some(&gz(r#"{"hello":1}"#))).unwrap_err();
        assert!(
            neither.contains("nbformat") && neither.contains("engine_name"),
            "names both shapes: {neither}"
        );
        assert_eq!(parse(Some(&gz("42"))).unwrap_err(), neither);
        let bad_array = parse(Some(&gz("[1,2]"))).unwrap_err();
        assert!(bad_array.contains("JSON parse failed"), "{bad_array}");
        let bad_record = parse(Some(&gz(r#"{"nbformat":3}"#))).unwrap_err();
        assert!(bad_record.contains("record parse failed"), "{bad_record}");
        let all = [corrupt, not_json, neither, bad_array, bad_record];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn an_object_with_both_markers_is_ambiguous() {
        let mut v: serde_json::Value = serde_json::from_str(&record_json()).unwrap();
        v["engine_name"] = serde_json::json!("r");
        let err = parse(Some(&gz(&v.to_string()))).unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn legacy_capture_degrade_flag_off_presents_as_today() {
        let (payload, notice) =
            parse_capture_payload_with(Some(&gz(&format!("[{ONE}]"))), "a.qmd", false).unwrap();
        assert!(matches!(payload, CapturePayload::Legacy(ref v) if v.len() == 1));
        assert!(notice.is_none());
        assert!(!DEGRADE_LEGACY_CAPTURES, "default must be off");
    }

    #[test]
    fn legacy_capture_degrade_flag_on_gives_empty_and_one_notice() {
        let (payload, notice) =
            parse_capture_payload_with(Some(&gz(&format!("[{ONE}]"))), "ch1.qmd", true).unwrap();
        assert!(matches!(payload, CapturePayload::Empty));
        let notice = notice.expect("one notice");
        assert!(notice.contains("ch1.qmd"), "names the document: {notice}");
        // A record is never degraded.
        let (payload, notice) =
            parse_capture_payload_with(Some(&gz(&record_json())), "a.qmd", true).unwrap();
        assert!(matches!(payload, CapturePayload::Record(_)));
        assert!(notice.is_none());
    }
}
