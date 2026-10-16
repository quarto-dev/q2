//! Shared harness for the Plan 9 route tests (`typst_pdf`, `embed_downloads`): a real
//! preview server over a scratch project, plus capture and zip helpers. The tests that use
//! it need native `typst` and/or `pandoc` and skip with a clear reason when absent.

use std::io::Read;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use quarto_hub::HubContext;
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_preview::{PreviewConfig, run_with_on_ready};
use quarto_trace::{CaptureFile, EngineCapture};
use tokio::sync::oneshot;

/// A 1x1 PNG.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

pub fn have(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `true` when `bin` is installed; otherwise says why the test is skipping.
pub fn require(bins: &[&str]) -> bool {
    for bin in bins {
        if !have(bin) {
            eprintln!("SKIP: `{bin}` is not on PATH");
            return false;
        }
    }
    true
}

pub struct Server {
    pub port: u16,
    pub ctx: Arc<HubContext>,
    /// The canonical project directory.
    pub root: PathBuf,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    _project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Boot a preview server over a project holding `files` (relative path, bytes).
pub async fn boot(files: &[(&str, &[u8])]) -> Server {
    let project = tempfile::TempDir::with_prefix("q2-preview-routes-").unwrap();
    let root = project.path().canonicalize().unwrap();
    for (name, bytes) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let data = tempfile::TempDir::with_prefix("q2-preview-routes-data-").unwrap();
    let port = pick_free_port();
    let config = PreviewConfig {
        render: Default::default(),
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(root.clone()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: quarto_preview::config::EnginePolicy::Off,
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
    let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");
    Server {
        port,
        ctx,
        root,
        handle,
        _project: project,
        _data: data,
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// A percent-encoded header, decoded.
    pub fn header(&self, name: &str) -> Option<String> {
        let value = self.headers.get(name)?.to_str().ok()?;
        Some(
            percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .into_owned(),
        )
    }

    pub fn disposition(&self) -> String {
        self.headers
            .get("content-disposition")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    }
}

/// POST `body` as JSON to `route` (`/api/preview/pdf`, `/api/preview/render`).
pub async fn post(server: &Server, route: &str, body: serde_json::Value) -> Reply {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}{route}", server.port))
        .json(&body)
        .send()
        .await
        .expect("request");
    Reply {
        status: response.status().as_u16(),
        headers: response.headers().clone(),
        body: response.bytes().await.expect("body").to_vec(),
    }
}

/// An `EngineCapture` for `doc` (which holds one ```` ```{r} ```` cell): the recorded
/// markdown is the document with that cell replaced by an output wrapper holding
/// `output`, optionally with a plot at `plot` (relative to the document). The prose around
/// the cell stays, as the splice walks both documents in lockstep.
pub fn capture_of(doc: &str, output: &str, plot: Option<&str>) -> EngineCapture {
    let image = plot.map(|p| format!("![]({p})\n")).unwrap_or_default();
    let wrapper = format!(
        "::: {{.cell}}\n::: {{.cell-output .cell-output-stdout}}\n{output}\n:::\n\
         ::: {{.cell-output-display}}\n{image}:::\n:::\n"
    );
    let start = doc.find("```{r}").expect("a {r} cell");
    let end = start + doc[start..].find("\n```\n").expect("closing fence") + "\n```\n".len();
    let markdown = format!("{}{wrapper}{}", &doc[..start], &doc[end..]);
    EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: doc.to_string(),
        result: serde_json::json!({ "markdown": markdown }),
        files: plot
            .map(|p| CaptureFile {
                path: p.to_string(),
                contents_base64: base64::engine::general_purpose::STANDARD.encode(PNG),
            })
            .into_iter()
            .collect(),
    }
}

/// Record `captures` in the hub's sidecar for `path`, as the eager driver would.
pub async fn record_captures(server: &Server, path: &str, captures: &[EngineCapture]) {
    let id = quarto_preview::capture_driver::write_capture_doc(&server.ctx, path, captures)
        .await
        .expect("write the capture doc");
    server
        .ctx
        .index()
        .set_capture(
            path,
            &CaptureRef {
                capture_doc_id: id,
                staleness: None,
                state: Some(CaptureState::Idle),
                last_error: None,
            },
        )
        .expect("set the sidecar entry");
}

pub fn zip_archive(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("a zip")
}

pub fn zip_entry_text(bytes: &[u8], name: &str) -> String {
    let mut text = String::new();
    zip_archive(bytes)
        .by_name(name)
        .unwrap_or_else(|_| panic!("zip entry {name}"))
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// Whether any entry of the zip is byte-for-byte `needle`.
pub fn zip_has_bytes(bytes: &[u8], needle: &[u8]) -> bool {
    let mut zip = zip_archive(bytes);
    (0..zip.len()).any(|i| {
        let mut buf = Vec::new();
        zip.by_index(i).unwrap().read_to_end(&mut buf).unwrap();
        buf == needle
    })
}

/// No request directory is left under the project's `.quarto/preview-out`.
pub fn preview_out_is_clean(root: &Path) -> bool {
    let dir = root.join(".quarto/preview-out");
    !dir.exists()
        || std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .all(|e| e.file_name() == ".gitignore")
}
