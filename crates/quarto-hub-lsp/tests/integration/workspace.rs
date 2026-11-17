//! `HubWorkspace` against an in-process hub: following projects, reading
//! their comments, and change hints.

use std::sync::Arc;
use std::time::Duration;

use quarto_hub_lsp::workspace::{ChangeHint, HubWorkspace, ProjectComments, WorkspaceError};
use quarto_hub_provider::HeaderDialer;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::support::{create_project, create_text_doc, eventually, set_text, spawn_hub};

const WAIT: Duration = Duration::from_secs(10);

async fn connect(ws_url: url::Url) -> (HubWorkspace, UnboundedReceiver<ChangeHint>) {
    let dialer = HeaderDialer::new(ws_url, Vec::new()).unwrap();
    HubWorkspace::connect(Arc::new(dialer), WAIT)
        .await
        .expect("connect to the hub")
}

/// `(path, comment texts)` per file, the shape most assertions compare.
fn summary(project: &ProjectComments) -> Vec<(String, Vec<String>)> {
    project
        .files
        .iter()
        .map(|f| {
            (
                f.path.clone(),
                f.comments.iter().map(|c| c.text.clone()).collect(),
            )
        })
        .collect()
}

fn owned(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    rows.iter()
        .map(|(path, texts)| {
            (
                path.to_string(),
                texts.iter().map(|t| t.to_string()).collect(),
            )
        })
        .collect()
}

#[tokio::test]
async fn following_a_project_reports_the_comments_of_each_qmd_file_sorted_by_path() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(
        &hub,
        &[
            ("docs/b.qmd", "[>> two ]\n\n[>> three ]\n"),
            ("a.qmd", "Prose [>> one ] end.\n"),
            ("notes.md", "[>> not a qmd file ]\n"),
        ],
    )
    .await;
    let (workspace, _hints) = connect(ws_url).await;

    workspace.follow(&index.document_id()).await.unwrap();
    let project = workspace
        .project_comments(&index.document_id())
        .await
        .unwrap();

    assert_eq!(project.project, index.document_id());
    assert_eq!(
        summary(&project),
        owned(&[("a.qmd", &["one"]), ("docs/b.qmd", &["two", "three"])])
    );
    assert!(project.files.iter().all(|f| f.error.is_none()));
}

#[tokio::test]
async fn a_file_reports_its_document_id_and_comment_positions() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("index.qmd", "Intro.\n\n[>> test! ]\n")]).await;
    let (workspace, _hints) = connect(ws_url).await;

    workspace.follow(&index.document_id()).await.unwrap();
    let project = workspace
        .project_comments(&index.document_id())
        .await
        .unwrap();

    let file = &project.files[0];
    assert_eq!(file.document, ids[0]);
    assert_eq!(file.comments[0].file.as_deref(), Some("index.qmd"));
    assert_eq!(file.comments[0].start_line, Some(3));
}

#[tokio::test]
async fn an_edit_on_the_hub_is_reflected_in_the_next_query() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("a.qmd", "No comments yet.\n")]).await;
    let (workspace, _hints) = connect(ws_url).await;
    workspace.follow(&index.document_id()).await.unwrap();

    set_text(&hub, &ids[0], "Now [>> added ] one.\n").await;

    let project_id = index.document_id();
    eventually(WAIT, "the added comment", || async {
        let project = workspace.project_comments(&project_id).await.ok()?;
        (summary(&project) == owned(&[("a.qmd", &["added"])])).then_some(())
    })
    .await;
}

#[tokio::test]
async fn a_file_added_to_the_index_is_followed() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "[>> one ]\n")]).await;
    let (workspace, _hints) = connect(ws_url).await;
    workspace.follow(&index.document_id()).await.unwrap();

    let new_id = create_text_doc(&hub, "[>> fresh ]\n").await;
    index.add_file("b.qmd", &new_id).unwrap();

    let project_id = index.document_id();
    eventually(WAIT, "the new file's comment", || async {
        let project = workspace.project_comments(&project_id).await.ok()?;
        (summary(&project) == owned(&[("a.qmd", &["one"]), ("b.qmd", &["fresh"])])).then_some(())
    })
    .await;
}

#[tokio::test]
async fn a_file_that_does_not_parse_reports_its_failure_instead_of_no_comments() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "Prose [>> half typed\n")]).await;
    let (workspace, _hints) = connect(ws_url).await;

    workspace.follow(&index.document_id()).await.unwrap();
    let project = workspace
        .project_comments(&index.document_id())
        .await
        .unwrap();

    let file = &project.files[0];
    assert!(file.comments.is_empty());
    let error = file.error.as_ref().expect("a parse failure");
    assert!(error.messages[0].starts_with("Q-2-21: "), "{error:?}");
}

#[tokio::test]
async fn a_hub_side_edit_produces_a_change_hint_naming_the_document() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("a.qmd", "Text.\n")]).await;
    let (workspace, mut hints) = connect(ws_url).await;
    workspace.follow(&index.document_id()).await.unwrap();

    set_text(&hub, &ids[0], "Text [>> new ].\n").await;

    let hint = tokio::time::timeout(WAIT, async {
        loop {
            let hint = hints.recv().await.expect("the hint channel stays open");
            if hint.document == ids[0] {
                return hint;
            }
        }
    })
    .await
    .expect("a change hint for the edited file");
    assert_eq!(hint.project, index.document_id());
}

#[tokio::test]
async fn unfollowing_a_project_stops_its_hints_and_its_queries() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("a.qmd", "Text.\n")]).await;
    let (workspace, mut hints) = connect(ws_url).await;
    let project_id = index.document_id();
    workspace.follow(&project_id).await.unwrap();

    assert!(workspace.unfollow(&project_id));
    while hints.try_recv().is_ok() {}
    set_text(&hub, &ids[0], "Text [>> unseen ].\n").await;

    let late = tokio::time::timeout(Duration::from_millis(500), hints.recv()).await;
    assert!(
        late.is_err(),
        "no hint may arrive after unfollowing: {late:?}"
    );
    assert!(matches!(
        workspace.project_comments(&project_id).await,
        Err(WorkspaceError::UnknownProject(_))
    ));
    assert!(
        !workspace.unfollow(&project_id),
        "a second unfollow is a no-op"
    );
}

#[tokio::test]
async fn following_a_document_the_hub_does_not_have_fails() {
    let (_hub, ws_url) = spawn_hub().await;
    let (workspace, _hints) = connect(ws_url).await;

    // A well-formed id that only an unconnected repo holds.
    let elsewhere = samod::Repo::build_tokio().load().await;
    let missing = create_text_doc(&elsewhere, "x").await;
    assert!(matches!(
        workspace.follow(&missing).await,
        Err(WorkspaceError::IndexNotFound(_))
    ));
}
