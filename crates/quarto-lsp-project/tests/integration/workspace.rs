//! `Workspace` driven with synthetic inputs, as a host would (C3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_lsp_project::{
    ClientEvent, ClientId, DocUri, FileChange, Input, Output, Workspace, WorkspaceConfig,
};
use quarto_system_runtime::NativeRuntime;

const C1: ClientId = 1;
const C2: ClientId = 2;

fn workspace() -> Workspace {
    Workspace::new(WorkspaceConfig::new(Arc::new(NativeRuntime::new())))
}

fn root() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    (tmp, root)
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn uri(path: &Path) -> DocUri {
    DocUri::from_file_path(path).unwrap()
}

fn open(ws: &mut Workspace, client: ClientId, uri: &DocUri, text: &str) -> Vec<Output> {
    ws.handle(Input::Client(
        client,
        ClientEvent::Opened {
            uri: uri.clone(),
            text: text.to_string(),
            version: 1,
        },
    ))
}

fn change(ws: &mut Workspace, client: ClientId, uri: &DocUri, text: &str, version: i32) {
    ws.handle(Input::Client(
        client,
        ClientEvent::Changed {
            uri: uri.clone(),
            text: text.to_string(),
            version,
        },
    ));
}

/// Run every spawned job inline and feed it back; return the other outputs.
fn drive(ws: &mut Workspace, outputs: Vec<Output>) -> Vec<Output> {
    let mut rest = Vec::new();
    for output in outputs {
        match output {
            Output::Spawn(job) => {
                let done = job.run();
                rest.extend(ws.handle(Input::JobDone(done)));
            }
            other => rest.push(other),
        }
    }
    rest
}

fn symbol_names(ws: &mut Workspace, client: ClientId, uri: &DocUri) -> Vec<String> {
    fn walk(symbols: &[quarto_lsp_core::Symbol], out: &mut Vec<String>) {
        for s in symbols {
            out.push(s.name.clone());
            walk(&s.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&ws.document_symbols(client, uri).unwrap(), &mut out);
    out
}

fn published(outputs: &[Output]) -> Vec<&DocUri> {
    outputs
        .iter()
        .filter_map(|o| match o {
            Output::Diagnostics { uri, .. } => Some(uri),
            _ => None,
        })
        .collect()
}

#[test]
fn a_document_outside_any_project_gets_a_single_file_session_and_no_walk() {
    let (_tmp, root) = root();
    // Neither the directory nor the file exists, as with the existing LSP
    // tests' `file:///test/valid.qmd`.
    let doc = root.join("missing").join("doc.qmd");
    let doc_uri = uri(&doc);
    let mut ws = workspace();
    let outputs = open(&mut ws, C1, &doc_uri, "# Alone\n");
    assert!(
        !outputs
            .iter()
            .any(|o| matches!(o, Output::Watch { .. } | Output::Spawn(_))),
        "a lone document is neither watched nor swept"
    );
    let flushed = ws.flush();
    assert_eq!(published(&flushed), [&doc_uri]);
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Alone"]);
    assert_eq!(ws.session_roots(), [root.join("missing")]);
    // Unchanged on the next flush: nothing re-published.
    assert!(published(&ws.flush()).is_empty());
    // A new version of the document is always re-published, even when its
    // diagnostics are the same.
    change(&mut ws, C1, &doc_uri, "# Alone, edited\n", 2);
    assert_eq!(published(&ws.flush()), [&doc_uri]);
}

#[test]
fn a_project_is_watched_swept_and_analysed_in_its_own_format() {
    let (_tmp, root) = root();
    write(
        &root,
        "_quarto.yml",
        "project:\n  type: default\nformat: typst\n",
    );
    write(&root, "index.qmd", "---\ntitle: Home\n---\n\n# Index\n");
    write(
        &root,
        "sub/doc.qmd",
        "---\ntitle: Doc\n---\n\n# Doc heading\n",
    );
    let doc = root.join("sub/doc.qmd");
    let doc_uri = uri(&doc);
    let mut ws = workspace();

    let outputs = open(
        &mut ws,
        C1,
        &doc_uri,
        "---\ntitle: Doc\n---\n\n# Doc heading\n",
    );
    let jobs = outputs
        .iter()
        .filter(|o| matches!(o, Output::Spawn(_)))
        .count();
    assert_eq!(jobs, 2, "one sweep job per document in the render list");
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, Output::Watch { root: r, .. } if r == &root))
    );
    drive(&mut ws, outputs);

    // The unopened page was profiled by the sweep.
    let index = ws.document(C1, &uri(&root.join("index.qmd"))).unwrap();
    assert_eq!(
        index.profile.as_ref().unwrap().title_text().as_deref(),
        Some("Home")
    );
    // bd-5d7frwlo: the project's format, not a hard-coded html.
    let result = ws.document(C1, &doc_uri).unwrap();
    assert_eq!(result.format, "typst");
    assert_eq!(
        result.profile.as_ref().unwrap().source_path,
        PathBuf::from("sub/doc.qmd")
    );
}

#[test]
fn an_unsaved_include_edit_reaches_the_document_that_includes_it() {
    let (_tmp, root) = root();
    write(&root, "_quarto.yml", "project:\n  type: default\n");
    write(&root, "_child.qmd", "## Child on disk\n");
    let doc_text = "# Top\n\n{{< include _child.qmd >}}\n";
    write(&root, "doc.qmd", doc_text);
    let doc_uri = uri(&root.join("doc.qmd"));
    let child_uri = uri(&root.join("_child.qmd"));
    let mut ws = workspace();
    let outputs = open(&mut ws, C1, &doc_uri, doc_text);
    drive(&mut ws, outputs);
    ws.flush();
    assert_eq!(
        symbol_names(&mut ws, C1, &doc_uri),
        ["Top", "Child on disk"]
    );

    open(&mut ws, C1, &child_uri, "## Child on disk\n");
    change(&mut ws, C1, &child_uri, "## Child edited\n", 2);
    ws.flush();
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Top", "Child edited"]);

    // Another client does not see client 1's unsaved buffer.
    assert_eq!(
        symbol_names(&mut ws, C2, &doc_uri),
        ["Top", "Child on disk"]
    );
}

#[test]
fn an_unsaved_quarto_yml_makes_a_project_and_is_not_analysed() {
    let (_tmp, root) = root();
    write(&root, "doc.qmd", "# Doc\n");
    let mut ws = workspace();
    let config_uri = uri(&root.join("_quarto.yml"));
    let mut outputs = open(&mut ws, C1, &config_uri, "project:\n  type: default\n");
    outputs.extend(open(&mut ws, C1, &uri(&root.join("doc.qmd")), "# Doc\n"));
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, Output::Watch { root: r, .. } if r == &root)),
        "the buffer's config roots a project at its directory"
    );
    drive(&mut ws, outputs);
    let flushed = ws.flush();
    assert_eq!(
        published(&flushed),
        [&uri(&root.join("doc.qmd"))],
        "the config buffer gets no diagnostics"
    );
    assert!(ws.document(C1, &config_uri).is_none());
    assert_eq!(
        symbol_names(&mut ws, C1, &uri(&root.join("doc.qmd"))),
        ["Doc"]
    );
}

#[test]
fn a_disk_change_reaches_open_documents_once_the_feeder_reports_it() {
    let (_tmp, root) = root();
    write(&root, "_quarto.yml", "project:\n  type: default\n");
    write(&root, "_child.qmd", "## Before\n");
    let doc_text = "# Top\n\n{{< include _child.qmd >}}\n";
    write(&root, "doc.qmd", doc_text);
    let doc_uri = uri(&root.join("doc.qmd"));
    let mut ws = workspace();
    let outputs = open(&mut ws, C1, &doc_uri, doc_text);
    drive(&mut ws, outputs);
    ws.flush();
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Top", "Before"]);

    write(&root, "_child.qmd", "## After\n");
    // The base is a snapshot: not seen until the feeder reports it.
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Top", "Before"]);
    ws.handle(Input::Files(vec![FileChange::Changed(
        root.join("_child.qmd"),
    )]));
    ws.flush();
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Top", "After"]);
}

#[test]
fn diagnostics_follow_edits_and_clear_on_close() {
    let (_tmp, root) = root();
    let doc_uri = uri(&root.join("doc.qmd"));
    let mut ws = workspace();
    open(&mut ws, C1, &doc_uri, "# Fine\n");
    ws.flush();
    change(&mut ws, C1, &doc_uri, "Text [>> half a comment\n", 2);
    let flushed = ws.flush();
    let errors = flushed
        .iter()
        .find_map(|o| match o {
            Output::Diagnostics { diagnostics, .. } => Some(diagnostics.len()),
            _ => None,
        })
        .expect("re-published after the edit");
    assert!(errors > 0);

    let closed = ws.handle(Input::Client(
        C1,
        ClientEvent::Closed {
            uri: doc_uri.clone(),
        },
    ));
    assert!(matches!(
        closed.as_slice(),
        [Output::Diagnostics { diagnostics, .. }] if diagnostics.is_empty()
    ));
    assert!(
        ws.session_roots().is_empty(),
        "the lone-document session is dropped"
    );
}

#[test]
fn an_untitled_document_is_analysed_without_touching_the_disk() {
    let mut ws = workspace();
    let doc_uri = DocUri::new("untitled:Untitled-1");
    open(&mut ws, C1, &doc_uri, "# Scratch\n\n## Notes\n");
    ws.flush();
    assert_eq!(symbol_names(&mut ws, C1, &doc_uri), ["Scratch", "Notes"]);
    assert!(ws.document(C2, &doc_uri).is_none(), "not open for client 2");
}

#[test]
fn jobs_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<quarto_lsp_project::Job>();
    assert_send::<quarto_lsp_project::JobOutput>();
}
