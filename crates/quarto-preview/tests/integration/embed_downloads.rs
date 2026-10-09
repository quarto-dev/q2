//! `POST /api/preview/render` for the embed's "Download as" formats against a real preview
//! server and real pandoc (Plan 9): odt, images in docx/epub/odt, capture outputs and plots,
//! the client's attribution payload, and the unchanged 400s. Skips when pandoc is absent.

use serde_json::json;

use super::render_routes_support::{
    PNG, Server, boot, capture_of, post, record_captures, require, zip_entry_text, zip_has_bytes,
};

const IMAGE_DOC: &str = "---\ntitle: T\n---\n\nHello\n\n![dot](img/dot.png)\n";

async fn image_server() -> Server {
    boot(&[("doc.qmd", IMAGE_DOC.as_bytes()), ("img/dot.png", PNG)]).await
}

#[tokio::test]
async fn odt_is_a_valid_odt_with_its_picture() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = image_server().await;
    let reply = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "odt", "content": IMAGE_DOC }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(
        reply.headers["content-type"],
        "application/vnd.oasis.opendocument.text"
    );
    assert!(reply.disposition().contains("filename=\"doc.odt\""));
    assert_eq!(
        zip_entry_text(&reply.body, "mimetype"),
        "application/vnd.oasis.opendocument.text"
    );
    assert!(
        zip_has_bytes(&reply.body, PNG),
        "the picture is inside the odt"
    );
}

#[tokio::test]
async fn docx_and_epub_contain_the_image_bytes() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = image_server().await;
    for format in ["docx", "epub"] {
        let reply = post(
            &server,
            "/api/preview/render",
            json!({ "path": "doc.qmd", "format": format, "content": IMAGE_DOC }),
        )
        .await;
        assert_eq!(reply.status, 200, "{format}: {}", reply.text());
        assert!(zip_has_bytes(&reply.body, PNG), "{format} lost its image");
    }
}

#[tokio::test]
async fn a_capture_output_and_plot_reach_a_docx() {
    if !require(&["pandoc"]) {
        return;
    }
    let doc = "---\ntitle: T\n---\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";
    let server = boot(&[("doc.qmd", doc.as_bytes())]).await;
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
    let reply = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "docx", "content": doc }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(zip_entry_text(&reply.body, "word/document.xml").contains("CAPTUREDOUTPUTMARKER"));
    assert!(
        zip_has_bytes(&reply.body, PNG),
        "the capture plot is embedded"
    );
    assert!(!server.root.join("doc_files").exists());
}

#[tokio::test]
async fn authors_are_stamped_from_the_payload() {
    if !require(&["pandoc"]) {
        return;
    }
    let doc = "---\ntitle: T\n---\n\nIt was [++ added] here.\n";
    let server = boot(&[("doc.qmd", doc.as_bytes())]).await;
    let payload = json!({
        "runs": [{ "start": 0, "end": doc.len(), "actor": "alice@example.com", "time": 1_700_000_000_000_i64 }],
        "identities": { "alice@example.com": { "name": "Alice Payload", "color": "#ff0000" } }
    })
    .to_string();
    let reply = post(
        &server,
        "/api/preview/render",
        json!({
            "path": "doc.qmd", "format": "docx", "content": doc, "attributionJson": payload
        }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    let xml = zip_entry_text(&reply.body, "word/document.xml");
    assert!(xml.contains("Alice Payload"), "{xml}");

    // Without a payload the author is not the payload's.
    let plain = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "docx", "content": doc }),
    )
    .await;
    assert_eq!(plain.status, 200);
    assert!(!zip_entry_text(&plain.body, "word/document.xml").contains("Alice Payload"));
}

#[tokio::test]
async fn the_unexecuted_count_is_in_the_stats_header() {
    if !require(&["pandoc"]) {
        return;
    }
    let doc = "---\ntitle: T\n---\n\n```{r}\n1\n```\n\n```{python}\n2\n```\n";
    let server = boot(&[("doc.qmd", doc.as_bytes())]).await;
    let reply = post(
        &server,
        "/api/preview/render",
        json!({ "path": "doc.qmd", "format": "docx", "content": doc }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(
        reply.header("x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":2}"#)
    );
}

#[tokio::test]
async fn the_existing_400s_are_unchanged() {
    let server = boot(&[("doc.qmd", b"hi\n".as_slice())]).await;
    for (path, format) in [
        ("doc.qmd", "html"),
        ("doc.qmd", "bogus"),
        ("missing.qmd", "docx"),
        ("../doc.qmd", "docx"),
    ] {
        let reply = post(
            &server,
            "/api/preview/render",
            json!({ "path": path, "format": format }),
        )
        .await;
        assert_eq!(reply.status, 400, "{path} {format}: {}", reply.text());
    }
}
