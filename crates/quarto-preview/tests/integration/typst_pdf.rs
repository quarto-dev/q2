//! `POST /api/preview/pdf` and the typst keys of `POST /api/preview/render` against a real
//! preview server, real typst and real pandoc (Plan 9). Skips when either is absent.

use serde_json::json;

use super::render_routes_support::{
    Server, boot, capture_of, post, preview_out_is_clean, record_captures, require,
};

const DOC: &str = "---\ntitle: T\nformat: typst\n---\n\nHello typst.\n";

async fn server_with(files: &[(&str, &str)]) -> Server {
    let files: Vec<(&str, &[u8])> = files.iter().map(|(n, c)| (*n, c.as_bytes())).collect();
    boot(&files).await
}

#[tokio::test]
async fn the_pdf_route_returns_a_real_pdf_and_cleans_up() {
    if !require(&["typst", "pandoc"]) {
        return;
    }
    let server = server_with(&[("doc.qmd", DOC)]).await;
    let reply = post(
        &server,
        "/api/preview/pdf",
        json!({ "path": "doc.qmd", "content": DOC }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(&reply.body[..4], b"%PDF");
    assert_eq!(reply.headers["content-type"], "application/pdf");
    assert!(reply.headers.get("content-disposition").is_none());
    assert_eq!(
        reply.header("x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":0}"#)
    );
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn output_ext_does_not_change_what_pdf_and_typst_pdf_return() {
    if !require(&["typst", "pandoc"]) {
        return;
    }
    for (ext, source_name) in [("typ", "doc.typ"), ("typst", "doc.typst")] {
        let doc = format!(
            "---\ntitle: T\nformat:\n  typst:\n    output-ext: {ext}\n---\n\nHello typst.\n"
        );
        let server = server_with(&[("doc.qmd", &doc)]).await;

        let pdf = post(
            &server,
            "/api/preview/pdf",
            json!({ "path": "doc.qmd", "content": doc }),
        )
        .await;
        assert_eq!(pdf.status, 200, "{ext}: {}", pdf.text());
        assert_eq!(&pdf.body[..4], b"%PDF", "output-ext: {ext}");

        let keyed = post(
            &server,
            "/api/preview/render",
            json!({ "path": "doc.qmd", "format": "typst-pdf", "content": doc }),
        )
        .await;
        assert_eq!(keyed.status, 200, "{ext}: {}", keyed.text());
        assert_eq!(&keyed.body[..4], b"%PDF", "output-ext: {ext}");
        assert!(keyed.disposition().contains("filename=\"doc.pdf\""));

        let source = post(
            &server,
            "/api/preview/render",
            json!({ "path": "doc.qmd", "format": "typst", "content": doc }),
        )
        .await;
        assert_eq!(source.status, 200, "{ext}: {}", source.text());
        assert!(
            source
                .disposition()
                .contains(&format!("filename=\"{source_name}\"")),
            "output-ext: {ext}: {}",
            source.disposition()
        );
        assert!(
            source.text().contains("Hello typst."),
            "the typst source, not a PDF: {}",
            source.text()
        );
        assert!(preview_out_is_clean(&server.root));
    }
}

#[tokio::test]
async fn a_broken_document_is_422_with_diagnostics() {
    if !require(&["typst", "pandoc"]) {
        return;
    }
    let server = server_with(&[("doc.qmd", DOC)]).await;
    let reply = post(
        &server,
        "/api/preview/pdf",
        json!({ "path": "doc.qmd", "content": "---\ntitle: [unclosed\n---\n\nBody.\n" }),
    )
    .await;
    assert_eq!(reply.status, 422, "{}", reply.text());
    let body: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    assert!(
        body["error"].is_string() || !body["diagnostics"].as_array().unwrap().is_empty(),
        "{body}"
    );
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn a_recorded_capture_shows_its_output_and_nothing_lands_beside_the_source() {
    if !require(&["typst", "pandoc"]) {
        return;
    }
    let doc = "---\ntitle: T\nformat: typst\n---\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";
    let server = server_with(&[("doc.qmd", doc)]).await;
    record_captures(
        &server,
        "doc.qmd",
        &[capture_of(
            doc,
            "CAPTUREDOUTPUTMARKER",
            Some("doc_files/cell-1.png"),
        )],
    )
    .await;

    let source = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "typst", "content": doc }),
    )
    .await;
    assert_eq!(source.status, 200, "{}", source.text());
    assert!(
        source.text().contains("CAPTUREDOUTPUTMARKER"),
        "{}",
        source.text()
    );
    assert_eq!(
        source.header("x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":0}"#),
        "the cell is covered by the capture"
    );

    // The plot is found by typst (a PDF comes back) and the project root stays clean.
    let pdf = post(
        &server,
        "/api/preview/pdf",
        json!({ "path": "doc.qmd", "content": doc }),
    )
    .await;
    assert_eq!(pdf.status, 200, "{}", pdf.text());
    assert_eq!(&pdf.body[..4], b"%PDF");
    assert!(
        !server.root.join("doc_files").exists(),
        "plot written beside the source"
    );
    assert!(!server.root.join("doc.pdf").exists());
    assert!(!server.root.join("doc.typ").exists());
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn unchanged_cells_keep_their_outputs_after_the_prose_is_edited() {
    if !require(&["pandoc"]) {
        return;
    }
    let doc = "---\ntitle: T\nformat: typst\n---\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";
    let server = server_with(&[("doc.qmd", doc)]).await;
    record_captures(
        &server,
        "doc.qmd",
        &[capture_of(doc, "CAPTUREDOUTPUTMARKER", None)],
    )
    .await;
    let edited = doc.replace("Before.", "Before, edited.");
    let reply = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "typst", "content": edited }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(reply.text().contains("Before, edited."), "{}", reply.text());
    assert!(
        reply.text().contains("CAPTUREDOUTPUTMARKER"),
        "an unchanged cell keeps its output: {}",
        reply.text()
    );
}

#[tokio::test]
async fn a_cell_without_a_capture_is_not_executed_and_is_counted() {
    if !require(&["pandoc"]) {
        return;
    }
    let doc =
        "---\ntitle: T\nformat: typst\n---\n\n```{python}\nprint('SHOULD_NOT_RUN' + 'X')\n```\n";
    let server = server_with(&[("doc.qmd", doc)]).await;
    let reply = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "typst", "content": doc }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(
        reply.header("x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":1}"#)
    );
    assert!(
        !reply.text().contains("SHOULD_NOT_RUNX"),
        "the cell must not have run: {}",
        reply.text()
    );
}
