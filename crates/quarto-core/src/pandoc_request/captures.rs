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

/// Deserialize a gzipped JSON capture sequence (empty when absent or empty).
/// A stale single-object capture document is accepted as one capture.
pub fn parse_capture_gz(bytes: Option<&[u8]>) -> Result<Vec<quarto_trace::EngineCapture>, String> {
    use std::io::Read;

    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .map_err(|e| format!("ungzip failed: {e}"))?;
    match serde_json::from_slice::<Vec<quarto_trace::EngineCapture>>(&json) {
        Ok(captures) => Ok(captures),
        Err(array_err) => match serde_json::from_slice::<quarto_trace::EngineCapture>(&json) {
            Ok(single) => Ok(vec![single]),
            Err(_) => Err(format!("JSON parse failed: {array_err}")),
        },
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

    #[test]
    fn parse_capture_gz_is_empty_for_none_and_empty() {
        assert!(parse_capture_gz(None).unwrap().is_empty());
        assert!(parse_capture_gz(Some(&[])).unwrap().is_empty());
    }

    #[test]
    fn parse_capture_gz_reads_an_array_and_a_single_object() {
        let one = r#"{"engine_name":"r","input_qmd":"","result":null,"files":[]}"#;
        assert_eq!(
            parse_capture_gz(Some(&gz(&format!("[{one}]"))))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(parse_capture_gz(Some(&gz(one))).unwrap().len(), 1);
    }

    #[test]
    fn parse_capture_gz_reports_corrupt_input() {
        assert!(
            parse_capture_gz(Some(b"not gzip"))
                .unwrap_err()
                .contains("ungzip")
        );
        assert!(
            parse_capture_gz(Some(&gz("[1,2]")))
                .unwrap_err()
                .contains("JSON parse failed")
        );
    }
}
