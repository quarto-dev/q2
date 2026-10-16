//! `POST /api/preview/import` against a real preview server and the real pandoc (Plan 7):
//! each import fixture round-trips to its recording (JSON, media and status), and the route
//! refuses what it should. Skips when no usable pandoc is installed.

use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, run_with_on_ready};
use tokio::sync::oneshot;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../quarto-core/tests/fixtures/import-recordings"
);

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

struct Server {
    port: u16,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    _project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> Server {
    let project = tempfile::TempDir::with_prefix("q2-preview-import-route-").unwrap();
    std::fs::write(
        project.path().join("index.qmd"),
        "---\ntitle: T\n---\n\nHi.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-import-route-data-").unwrap();
    let port = pick_free_port();
    let config = PreviewConfig {
        render: Default::default(),
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: Default::default(),
    };
    let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
    let mut ready_tx = Some(ready_tx);
    let handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");
    Server {
        port,
        handle,
        _project: project,
        _data: data,
    }
}

fn pandoc_available() -> bool {
    match std::process::Command::new("pandoc")
        .arg("--version")
        .output()
    {
        Ok(o) if o.status.success() => true,
        _ => {
            eprintln!("skipping: no pandoc on PATH");
            false
        }
    }
}

struct Frame {
    header: serde_json::Value,
    output: Vec<u8>,
    media: Vec<(String, Vec<u8>)>,
}

fn parse_frame(body: &[u8]) -> Frame {
    let header_len = u32::from_be_bytes(body[..4].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&body[4..4 + header_len]).unwrap();
    let mut at = 4 + header_len;
    let output_len = header["outputLen"].as_u64().unwrap() as usize;
    let output = body[at..at + output_len].to_vec();
    at += output_len;
    let mut media = Vec::new();
    for entry in header["collected"].as_array().unwrap() {
        let len = entry["len"].as_u64().unwrap() as usize;
        media.push((
            entry["path"].as_str().unwrap().to_string(),
            body[at..at + len].to_vec(),
        ));
        at += len;
    }
    assert_eq!(at, body.len(), "no trailing bytes");
    Frame {
        header,
        output,
        media,
    }
}

fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            collect_files(&entry.path(), &path, out);
        } else {
            out.push((path, std::fs::read(entry.path()).unwrap()));
        }
    }
}

async fn post(port: u16, file_name: &str, body: Vec<u8>) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/api/preview/import"))
        .header("file_name", file_name)
        .body(body)
        .send()
        .await
        .expect("POST succeeds")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_fixture_round_trips_to_its_recording() {
    if !pandoc_available() {
        return;
    }
    let server = boot().await;
    let mut names: Vec<PathBuf> = std::fs::read_dir(FIXTURES)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort();
    let mut tested = 0;
    for dir in names {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with("source."));
        // Not every directory is a fixture (`sources/` holds the generators' inputs).
        let Some(source) = source else { continue };
        tested += 1;
        let response = post(
            server.port,
            &source,
            std::fs::read(dir.join(&source)).unwrap(),
        )
        .await;
        assert_eq!(response.status(), 200, "{name}");
        assert_eq!(
            response.headers()["content-type"],
            "application/octet-stream"
        );
        let frame = parse_frame(&response.bytes().await.unwrap());

        let status: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("status.json")).unwrap()).unwrap();
        assert_eq!(frame.header["status"], status["status"], "{name}");
        assert_eq!(frame.header["timedOut"], false, "{name}");
        let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap();
        assert_eq!(
            frame.header["stderr"].as_str().unwrap().trim_end(),
            stderr.trim_end(),
            "{name}: stderr differs from the recording"
        );
        if status["status"] == 0 {
            let recorded: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dir.join("pandoc.json")).unwrap()).unwrap();
            let got: serde_json::Value = serde_json::from_slice(&frame.output).unwrap();
            assert_eq!(got, recorded, "{name}: JSON differs from the recording");
            let mut expected = Vec::new();
            if dir.join("media").is_dir() {
                collect_files(
                    &dir.join("media"),
                    "/__q2_share__/import/media",
                    &mut expected,
                );
            }
            expected.sort();
            let paths =
                |v: &[(String, Vec<u8>)]| v.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
            assert_eq!(paths(&frame.media), paths(&expected), "{name}: media paths");
            assert!(frame.media == expected, "{name}: media bytes differ");
        } else {
            assert!(frame.output.is_empty() && frame.media.is_empty(), "{name}");
        }
    }
    assert!(tested >= 11, "the fixtures moved: only {tested} found");
    server.handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusals_are_400_and_413() {
    let server = boot().await;
    assert_eq!(
        post(server.port, "notes.md", b"hi".to_vec()).await.status(),
        400
    );
    let no_name = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/api/preview/import",
            server.port
        ))
        .body("hi")
        .send()
        .await
        .unwrap();
    assert_eq!(no_name.status(), 400);
    // Between the source cap and the route's body limit, and past the limit: both 413. The
    // default 2 MB body limit that other routes get would refuse a real 5 MB docx; this route
    // accepts up to the source cap.
    let over = vec![0u8; 26_214_401];
    assert_eq!(post(server.port, "big.docx", over).await.status(), 413);
    server.handle.abort();
}
