//! Whole-book and chapter downloads from `POST /api/preview/render` against a real preview
//! server, real pandoc and real typst (Plan 10): typst source and PDF, epub, chapter-only,
//! a page outside the book, a page format on a chapter, the busy and abort contracts, and
//! that the user's `_book/` and render manifests are never touched. Skips when pandoc or
//! typst is absent.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::render_routes_support::{
    PNG, Reply, Server, boot, capture_of, post, preview_out_is_clean, record_captures, require,
    zip_has_bytes,
};

const INDEX: &str = "---\ntitle: Home\n---\n\nWelcome to the book.\n";
const CH1: &str = "# One\n\nChapter one text.\n\n![A pixel](pixel.png)\n";
const CH2: &str = "# Two\n\nChapter two text.\n";
const EXTRA: &str = "---\ntitle: Extra\n---\n\nA page outside the book.\n";
const CAPTURE_CH1: &str = "# One\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";
const QUARTO_YML: &str = "project:\n  type: book\n\nbook:\n  title: \"Small Book\"\n  author: \"A. Writer\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n";
const RENDER: &str = "/api/preview/render";

async fn book_server(ch1: &str) -> Server {
    boot(&[
        ("_quarto.yml", QUARTO_YML.as_bytes()),
        ("index.qmd", INDEX.as_bytes()),
        ("ch1.qmd", ch1.as_bytes()),
        ("ch2.qmd", CH2.as_bytes()),
        ("extra.qmd", EXTRA.as_bytes()),
        ("pixel.png", PNG),
    ])
    .await
}

async fn download(server: &Server, path: &str, format: &str, scope: Option<&str>) -> Reply {
    let mut body = json!({ "path": path, "format": format });
    if let Some(scope) = scope {
        body["metadata"] = json!({ "book-scope": scope });
    }
    post(server, RENDER, body).await
}

fn stats(reply: &Reply) -> serde_json::Value {
    serde_json::from_str(&reply.header("x-q2-stats").expect("X-Q2-Stats")).unwrap()
}

fn codes(reply: &Reply) -> String {
    reply.header("x-q2-diagnostics").unwrap_or_default()
}

/// A `_book/` and the render manifests with a marker each; `assert_untouched` checks them.
struct UserOutputs(Vec<(PathBuf, String)>);

impl UserOutputs {
    fn plant(root: &Path) -> Self {
        let files = [
            ("_book/keep.txt", "keep"),
            (".quarto/render-manifest.json", "{\"rendered\":[]}"),
            (".quarto/llms-manifest.json", "{\"llms\":[]}"),
        ];
        Self(
            files
                .iter()
                .map(|(rel, text)| {
                    let path = root.join(rel);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, text).unwrap();
                    (path, text.to_string())
                })
                .collect(),
        )
    }

    fn assert_untouched(&self, root: &Path) {
        for (path, text) in &self.0 {
            assert_eq!(&std::fs::read_to_string(path).unwrap(), text, "{path:?}");
        }
        assert_eq!(
            std::fs::read_dir(root.join("_book")).unwrap().count(),
            1,
            "_book/ gained files"
        );
    }
}

#[tokio::test]
async fn a_typst_book_downloads_as_source() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let planted = UserOutputs::plant(&server.root);
    let reply = download(&server, "ch1.qmd", "typst", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(
        reply.disposition().contains(".typ\""),
        "{}",
        reply.disposition()
    );
    assert_eq!(
        stats(&reply)["book"],
        json!({ "scope": "book", "chapters": 3 })
    );
    let typ = reply.text();
    for text in [
        "Welcome to the book",
        "Chapter one text",
        "Chapter two text",
    ] {
        assert!(typ.contains(text), "{text} missing");
    }
    planted.assert_untouched(&server.root);
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn a_typst_book_downloads_as_pdf_with_its_chapter_image() {
    if !require(&["pandoc", "typst"]) {
        return;
    }
    let server = book_server(CH1).await;
    let planted = UserOutputs::plant(&server.root);
    let reply = download(&server, "ch1.qmd", "typst-pdf", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(&reply.body[..4], b"%PDF");
    assert!(reply.disposition().contains(".pdf\""));
    assert!(!codes(&reply).contains("Q-21-3"), "{}", codes(&reply));
    planted.assert_untouched(&server.root);
    assert!(!server.root.join("_book/pixel.png").exists());
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn an_epub_book_holds_every_chapter_and_the_image() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let planted = UserOutputs::plant(&server.root);
    let reply = download(&server, "ch2.qmd", "epub", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(stats(&reply)["book"]["chapters"], 3);
    assert!(zip_has_bytes(&reply.body, PNG), "the chapter image");
    planted.assert_untouched(&server.root);
}

#[tokio::test]
async fn a_book_scope_consolidates_the_book_and_an_absent_or_chapter_scope_does_not() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let whole = download(&server, "ch1.qmd", "typst", Some("book")).await;
    assert_eq!(whole.status, 200, "{}", whole.text());
    assert!(whole.text().contains("Chapter two text"));

    // Nothing chosen: the page is the target, so it is the chapter.
    let nothing = download(&server, "ch1.qmd", "typst", None).await;
    assert_eq!(nothing.status, 200, "{}", nothing.text());
    assert!(!nothing.text().contains("Chapter two text"));
    assert_eq!(stats(&nothing)["book"]["scope"], "chapter");

    let chapter = download(&server, "ch1.qmd", "typst", Some("chapter")).await;
    assert_eq!(chapter.status, 200, "{}", chapter.text());
    let typ = chapter.text();
    assert!(typ.contains("Chapter one text"));
    assert!(!typ.contains("Chapter two text"));
    assert_eq!(stats(&chapter)["book"]["scope"], "chapter");
}

#[tokio::test]
async fn a_page_outside_the_book_renders_alone_with_a_notice() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let reply = download(&server, "extra.qmd", "typst", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    let typ = reply.text();
    assert!(typ.contains("A page outside the book"));
    assert!(!typ.contains("Chapter one text"));
    // An explicit book cannot be honoured: the warning, not the info.
    assert!(codes(&reply).contains("Q-5-48"), "{}", codes(&reply));
    assert!(!codes(&reply).contains("Q-5-41"), "{}", codes(&reply));
    assert_eq!(stats(&reply)["book"]["scope"], "none");
}

#[tokio::test]
async fn docx_on_a_chapter_stays_chapter_only_without_a_notice() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let reply = download(&server, "ch1.qmd", "docx", None).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(codes(&reply).is_empty(), "{}", codes(&reply));
    assert_eq!(stats(&reply)["book"]["scope"], "chapter");
}

#[tokio::test]
async fn a_chapters_recorded_plot_reaches_the_book() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CAPTURE_CH1).await;
    record_captures(
        &server,
        "ch1.qmd",
        &[capture_of(
            CAPTURE_CH1,
            "[1] 1",
            Some("ch1_files/figure-typst/cell-1.png"),
        )],
    )
    .await;
    // Asked from another chapter: the capture is resolved by ch1's own sidecar entry.
    let reply = download(&server, "ch2.qmd", "typst", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(
        reply.text().contains("cell-1.png"),
        "the plot is referenced"
    );
    assert_eq!(stats(&reply)["unexecuted_cells"], 0);
    assert!(
        !server.root.join("ch1_files").exists(),
        "no plot beside the source"
    );
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn an_unrecorded_chapter_counts_into_the_notice() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CAPTURE_CH1).await;
    let reply = download(&server, "ch1.qmd", "typst", Some("book")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(stats(&reply)["unexecuted_cells"], 1);
}

#[tokio::test]
async fn the_active_chapters_editor_text_is_what_is_rendered() {
    if !require(&["pandoc"]) {
        return;
    }
    let server = book_server(CH1).await;
    let reply = post(
        &server,
        RENDER,
        json!({
            "path": "ch1.qmd", "format": "typst", "metadata": { "book-scope": "book" },
            "content": "# One\n\nEDITED unsaved sentence.\n"
        }),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    let typ = reply.text();
    assert!(typ.contains("EDITED unsaved sentence"));
    assert!(!typ.contains("Chapter one text"));
    assert!(typ.contains("Chapter two text"));
    assert_eq!(
        std::fs::read_to_string(server.root.join("ch1.qmd")).unwrap(),
        CH1
    );
}

#[tokio::test]
async fn a_second_book_download_is_busy_and_the_guard_is_released() {
    if !require(&["pandoc", "typst"]) {
        return;
    }
    let server = book_server(CH1).await;
    let (a, b) = tokio::join!(
        download(&server, "ch1.qmd", "typst-pdf", Some("book")),
        download(&server, "ch2.qmd", "typst-pdf", Some("book")),
    );
    let statuses = [a.status, b.status];
    assert!(
        statuses.contains(&200) && statuses.contains(&409),
        "one wins, one is busy: {statuses:?}"
    );
    let busy = if a.status == 409 { &a } else { &b };
    assert_eq!(busy.text(), r#"{"busy":true}"#);
    let retry = download(&server, "ch1.qmd", "typst-pdf", Some("book")).await;
    assert_eq!(retry.status, 200, "{}", retry.text());
    assert!(preview_out_is_clean(&server.root));
}

#[tokio::test]
async fn an_aborted_request_releases_the_guard_and_a_retry_succeeds() {
    if !require(&["pandoc", "typst"]) {
        return;
    }
    let server = book_server(CH1).await;
    // Drop the client mid-render: the handler future is dropped and cancels the render.
    let aborted = tokio::time::timeout(
        Duration::from_millis(150),
        download(&server, "ch1.qmd", "typst-pdf", Some("book")),
    )
    .await;
    assert!(aborted.is_err(), "the render outlasted the client");
    // The cancelled render stops at its next boundary; a retry waits out a busy answer.
    let mut last = None;
    for _ in 0..60 {
        let reply = download(&server, "ch1.qmd", "typst-pdf", Some("book")).await;
        if reply.status != 409 {
            last = Some(reply);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let reply = last.expect("the guard was never released");
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(&reply.body[..4], b"%PDF");
}

/// The tracked fixture files of the orange-book smoke project. Only a chapter is
/// downloaded from it: a later chapter `embed`s `chapter1.qmd#fig-cars`, which needs that
/// cell's executed output (`chapter1_files/...`), and the route never executes, so the
/// whole book fails to compile under it (an embed of a recorded capture is not spliced).
fn orange_book_files() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Untracked render output that sits in the working tree locally.
            if name == "_book" || name.ends_with("_files") || name == ".quarto" {
                continue;
            }
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let rel = path.strip_prefix(base).unwrap();
                out.push((
                    rel.to_string_lossy().replace('\\', "/"),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let base =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../quarto/tests/smoke-all/typst/orange-book");
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    out
}

#[tokio::test]
async fn an_orange_book_chapter_downloads_as_a_pdf() {
    if !require(&["pandoc", "typst"]) {
        return;
    }
    let owned = orange_book_files();
    let files: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let server = boot(&files).await;
    let reply = download(&server, "chapter1.qmd", "typst-pdf", Some("chapter")).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(&reply.body[..4], b"%PDF");
    assert!(preview_out_is_clean(&server.root));
}
