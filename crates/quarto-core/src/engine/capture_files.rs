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
        paths.sort();
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
