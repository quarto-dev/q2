//! JOL-2a: which metadata layers root at the document's own `FileId(0)`.
//!
//! The cell-less declared-engine rule treats a declaration as the document's
//! own only when its value roots at `DOCUMENT_OWN_FILE_ID`. This pins, against
//! a real discovered project and the real pipeline up to the document-profile
//! checkpoint, that front matter roots there and that every other layer
//! (`_quarto.yml`, `_metadata.yml`, a format extension default, a project
//! `format:` default) does not.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use quarto_core::engine::DOCUMENT_OWN_FILE_ID;
use quarto_core::format::Format;
use quarto_core::pipeline::build_html_pipeline_stages;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::stage::{LoadedSource, Pipeline, PipelineData, StageContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

const PROJECT: &str = "project:\n  type: default\n";
const PLAIN_PAGE: &str = "---\ntitle: T\n---\n\nHello `{python} 1+1`\n";

struct Observed {
    /// Root file ids of the merged `jupyter:` value, if the key is present.
    jupyter_ids: Option<BTreeSet<u64>>,
    /// Engine names in the stamped resolution.
    sequence: Vec<String>,
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

async fn observe(files: &[(&str, &str)], doc: &str, format: &str) -> Observed {
    let tmp = TempDir::new().unwrap();
    let root = quarto_system_runtime::canonicalize(tmp.path()).unwrap();
    for (name, body) in files {
        write(&root.join(name), body);
    }
    let doc_path = root.join(doc);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();

    let stages = build_html_pipeline_stages();
    let checkpoint = stages
        .iter()
        .position(|s| s.name() == "document-profile")
        .expect("pipeline has a document-profile stage");
    let pipeline = Pipeline::new(stages.into_iter().take(checkpoint + 1).collect()).unwrap();

    let content = std::fs::read(&doc_path).unwrap();
    let docinfo = DocumentInfo::from_path(&doc_path);
    let format = Format::from_format_string(format).unwrap();
    let mut ctx = StageContext::new(runtime.clone(), format, project, docinfo).unwrap();
    let input = PipelineData::LoadedSource(LoadedSource::new(doc_path, content));
    let out = pipeline.run(input, &mut ctx).await.expect("pipeline runs");
    let at = out.into_at_profile().unwrap();

    let jupyter_ids = at.ast.ast.meta.get("jupyter").map(|v| {
        let mut ids = std::collections::HashSet::new();
        v.source_info.collect_file_ids(&mut ids);
        ids.into_iter().map(|f| f.0 as u64).collect()
    });
    let sequence = at
        .profile
        .engine_resolution
        .map(|r| r.sequence)
        .unwrap_or_default();
    Observed {
        jupyter_ids,
        sequence,
    }
}

fn own_only() -> BTreeSet<u64> {
    BTreeSet::from([DOCUMENT_OWN_FILE_ID.0 as u64])
}

#[tokio::test]
async fn front_matter_roots_at_the_document_file_id() {
    let got = observe(
        &[
            ("_quarto.yml", PROJECT),
            ("a.qmd", "---\njupyter: python3\n---\n\nHi `{python} 1+1`\n"),
        ],
        "a.qmd",
        "html",
    )
    .await;
    assert_eq!(got.jupyter_ids, Some(own_only()));
    assert_eq!(got.sequence, vec!["jupyter".to_string()]);
}

#[tokio::test]
async fn project_file_does_not_root_at_the_document_file_id() {
    let got = observe(
        &[
            (
                "_quarto.yml",
                "project:\n  type: default\njupyter: python3\n",
            ),
            ("a.qmd", PLAIN_PAGE),
        ],
        "a.qmd",
        "html",
    )
    .await;
    assert_foreign(&got, "_quarto.yml");
}

#[tokio::test]
async fn directory_metadata_does_not_root_at_the_document_file_id() {
    let got = observe(
        &[
            ("_quarto.yml", PROJECT),
            ("sub/_metadata.yml", "jupyter: python3\n"),
            ("sub/a.qmd", PLAIN_PAGE),
        ],
        "sub/a.qmd",
        "html",
    )
    .await;
    assert_foreign(&got, "_metadata.yml");
}

#[tokio::test]
async fn format_extension_default_does_not_root_at_the_document_file_id() {
    let ext = "title: Acme\nversion: 1.0.0\ncontributes:\n  formats:\n    html:\n      jupyter: python3\n";
    let got = observe(
        &[
            ("_quarto.yml", PROJECT),
            ("_extensions/acme/_extension.yml", ext),
            ("a.qmd", PLAIN_PAGE),
        ],
        "a.qmd",
        "acme-html",
    )
    .await;
    assert_foreign(&got, "format extension");
}

#[tokio::test]
async fn project_format_default_does_not_root_at_the_document_file_id() {
    let got = observe(
        &[
            (
                "_quarto.yml",
                "project:\n  type: default\nformat:\n  html:\n    jupyter: python3\n",
            ),
            ("a.qmd", PLAIN_PAGE),
        ],
        "a.qmd",
        "html",
    )
    .await;
    assert_foreign(&got, "project format default");
}

/// The key arrives, none of its roots is the document's file, and so a plain
/// page declares no engine of its own.
fn assert_foreign(got: &Observed, layer: &str) {
    let ids = got
        .jupyter_ids
        .as_ref()
        .unwrap_or_else(|| panic!("{layer}: `jupyter:` should reach the merged metadata"));
    assert!(
        !ids.contains(&(DOCUMENT_OWN_FILE_ID.0 as u64)),
        "{layer} must not root at the document's file id; got {ids:?}"
    );
    assert!(
        got.sequence.is_empty(),
        "{layer}: no engine for a plain page; got {:?}",
        got.sequence
    );
}
