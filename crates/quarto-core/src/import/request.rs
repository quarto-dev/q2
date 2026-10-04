//! `prepare_import`: validate a source file and build the pandoc request that reads it
//! (epic interface 2).
//!
//! The request runs `pandoc -f <fmt> -t json` inside pandoc.wasm. The source's bytes never
//! enter the Rust wasm (I9): the request names the file in `host_inputs` and TS supplies the
//! bytes at run time. Nothing here touches the clock or the filesystem (I20).

use std::collections::BTreeMap;

use quarto_error_reporting::DiagnosticMessage;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::formats::{IMPORT_FORMATS, ImportFormat, MAX_SOURCE_BYTES, format_for_file_name};
use super::report;
use crate::pandoc_request::{
    PandocRequest, REQUEST_SCHEMA_VERSION, RequestKind, RequestPost, constants, types::HostInput,
};

/// Directory (under the share root) holding the source, the output and the extracted media.
pub const IMPORT_DIR_NAME: &str = "import";
/// The `SOURCE_DATE_EPOCH` every import request carries, the same constant the writer
/// recordings use; it keeps the request (and so any reader that consults it) deterministic.
pub const IMPORT_SOURCE_DATE_EPOCH: &str = "1700000000";

/// The empty share tree, as the host reads it (`ShareTree` in `@quarto/pandoc-host`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmptyShareTree {
    pub share_tree_version: String,
    pub files: Vec<()>,
}

/// What `prepare_import` returns.
#[derive(Debug, Default)]
pub struct PrepareImportOutcome {
    pub success: bool,
    pub diagnostics: Vec<DiagnosticMessage>,
    /// The pandoc reader name.
    pub format: Option<String>,
    pub request: Option<PandocRequest>,
    pub share_tree: Option<EmptyShareTree>,
    pub source_path: Option<String>,
}

/// Hex SHA-256 over zero bytes: the version of the empty share tree (no filters run).
pub fn empty_share_tree_version() -> String {
    hex::encode(Sha256::digest([]))
}

/// `/__q2_share__/import`.
pub fn import_dir() -> String {
    format!("{}/{IMPORT_DIR_NAME}", constants().share_root)
}

/// The directory `--extract-media` writes into, and the host collects from.
pub fn extract_dir() -> String {
    format!("{}/media", import_dir())
}

fn is_hex_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The argv of a reader run over `source_path` (interface 2).
pub fn import_argv(format: &ImportFormat, source_path: &str) -> Vec<String> {
    let dir = import_dir();
    let mut argv = vec!["pandoc".to_string(), "-f".into(), format.id.into()];
    if format.track_changes {
        argv.push("--track-changes=all".into());
    }
    argv.extend([
        "-t".to_string(),
        "json".into(),
        format!("--extract-media={dir}/media"),
        "-o".into(),
        format!("{dir}/out.json"),
        source_path.to_string(),
    ]);
    argv
}

/// Validate `file_name` and `size` (Q-24-1, then Q-24-2). With an empty `sha256_hex` that is
/// all: success or failure, no request. Otherwise, build the request; `size` must then be the
/// length of the bytes actually read.
pub fn prepare_import(file_name: &str, size: u64, sha256_hex: &str) -> PrepareImportOutcome {
    let Some(format) = format_for_file_name(file_name) else {
        let accepted: Vec<&str> = IMPORT_FORMATS
            .iter()
            .flat_map(|f| f.extensions.iter().copied())
            .collect();
        return PrepareImportOutcome {
            diagnostics: vec![report::unsupported_file_type(file_name, &accepted)],
            ..Default::default()
        };
    };
    if size > MAX_SOURCE_BYTES {
        return PrepareImportOutcome {
            diagnostics: vec![report::source_too_large(file_name, size, MAX_SOURCE_BYTES)],
            format: Some(format.id.to_string()),
            ..Default::default()
        };
    }
    if sha256_hex.is_empty() {
        return PrepareImportOutcome {
            success: true,
            format: Some(format.id.to_string()),
            ..Default::default()
        };
    }
    if !is_hex_sha256(sha256_hex) {
        return PrepareImportOutcome {
            diagnostics: vec![report::internal_error(
                "the source's SHA-256 is not 64 hex digits",
            )],
            format: Some(format.id.to_string()),
            ..Default::default()
        };
    }

    let dir = import_dir();
    let ext = format.extensions[0].trim_start_matches('.');
    let source_path = format!("{dir}/source.{ext}");
    let output_path = format!("{dir}/out.json");
    let share_root = constants().share_root.clone();
    let share_tree_version = empty_share_tree_version();
    let mut env = BTreeMap::new();
    env.insert(
        "SOURCE_DATE_EPOCH".to_string(),
        IMPORT_SOURCE_DATE_EPOCH.to_string(),
    );

    let mut request = PandocRequest {
        schema_version: REQUEST_SCHEMA_VERSION,
        kind: RequestKind::Pandoc,
        job_id: String::new(),
        writer: "json".to_string(),
        argv: import_argv(format, &source_path),
        env,
        files: Vec::new(),
        dirs: Vec::new(),
        resource_refs: Vec::new(),
        share_tree_path: format!("{share_root}/pandoc-share"),
        share_root,
        doc_dir: dir.clone(),
        project_root: dir,
        output_path,
        stage_name: "import".to_string(),
        json_path: source_path.clone(),
        post: RequestPost::None,
        expected_pandoc_wasm_sha256: constants().wasm_sha256.clone(),
        share_tree_version: share_tree_version.clone(),
        typst_available_fonts: None,
        host_inputs: vec![HostInput {
            path: source_path.clone(),
            sha256: sha256_hex.to_ascii_lowercase(),
            size,
        }],
        collect_dirs: vec![extract_dir()],
    };
    request.job_id = request.compute_job_id();

    PrepareImportOutcome {
        success: true,
        diagnostics: Vec::new(),
        format: Some(format.id.to_string()),
        request: Some(request),
        share_tree: Some(EmptyShareTree {
            share_tree_version,
            files: Vec::new(),
        }),
        source_path: Some(source_path),
    }
}
