//! VFS → temp-dir materialization test (bd-sfet3264, Phase 4a).
//!
//! Seeds a samod repo with a text file document and a binary file document,
//! wires them into an index, then materializes the project into a temp dir and
//! asserts the on-disk bytes match — including a nested path.

use automerge::{Automerge, ObjType, ROOT, transaction::Transactable};
use quarto_hub::index::IndexDocument;
use quarto_hub::resource::create_binary_document;
use quarto_hub_provider::materialize_project;
use samod::Repo;

/// Create a text file document with the given contents and return its id.
async fn create_text_doc(repo: &Repo, text: &str) -> String {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", ObjType::Text)?;
        tx.update_text(&obj, text)?;
        Ok(())
    })
    .unwrap();
    let handle = repo.create(doc).await.unwrap();
    handle.document_id().to_string()
}

#[tokio::test]
async fn materializes_text_and_binary_files_to_disk() {
    let repo = Repo::build_tokio().load().await;

    // A top-level text file and a nested binary asset.
    let qmd = "---\ntitle: Demo\n---\n\n```{r}\n1 + 1\n```\n";
    let text_id = create_text_doc(&repo, qmd).await;

    let png_bytes: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x01, 0x02];
    let binary_doc = create_binary_document(png_bytes, "image/png").unwrap();
    let binary_id = repo
        .create(binary_doc)
        .await
        .unwrap()
        .document_id()
        .to_string();

    let (index, _index_id) = IndexDocument::create(&repo).await.unwrap();
    index.add_file("report.qmd", &text_id).unwrap();
    index.add_file("assets/logo.png", &binary_id).unwrap();

    let dest = tempfile::tempdir().unwrap();
    let written = materialize_project(&repo, &index, dest.path())
        .await
        .expect("materialize");
    assert_eq!(written, 2, "both files should be written");

    let on_disk_qmd = std::fs::read_to_string(dest.path().join("report.qmd")).unwrap();
    assert_eq!(on_disk_qmd, qmd);

    let on_disk_png = std::fs::read(dest.path().join("assets/logo.png")).unwrap();
    assert_eq!(on_disk_png, png_bytes);
}
