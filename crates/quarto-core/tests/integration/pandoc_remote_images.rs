//! R6: remote images in the pandoc request (`PrefetchRemoteImagesStage`),
//! through `render_pandoc_request` over a directory with a runtime whose
//! network is scripted. The real browser fetch (`jsFetchUrlHardened`) and the
//! click-time snapshot are covered in `pandocRequest.wasm.test.ts`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, constants};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{
    CommandOutput, NativeRuntime, PathKind, PathMetadata, RuntimeError, RuntimeResult, SassOutput,
    SystemRuntime, TempDir, XdgDirKind,
};
use serde_json::Value;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nremote";

type Reply = Result<(Vec<u8>, String), String>;

/// `NativeRuntime` for everything but the network, which answers from a
/// table and records every URL it was asked for.
struct ScriptedNet {
    inner: NativeRuntime,
    replies: HashMap<String, Reply>,
    asked: Mutex<Vec<String>>,
}

impl ScriptedNet {
    fn new(replies: &[(&str, Reply)]) -> Arc<Self> {
        Arc::new(Self {
            inner: NativeRuntime::new(),
            replies: replies
                .iter()
                .map(|(u, r)| (u.to_string(), r.clone()))
                .collect(),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl SystemRuntime for ScriptedNet {
    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        self.asked.lock().unwrap().push(url.to_string());
        match self.replies.get(url) {
            Some(Ok(reply)) => Ok(reply.clone()),
            Some(Err(message)) => Err(RuntimeError::Network(message.clone())),
            None => Err(RuntimeError::Network(format!("unscripted {url}"))),
        }
    }
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.inner.file_read(path)
    }
    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.inner.file_write(path, contents)
    }
    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        self.inner.path_exists(path, kind)
    }
    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        self.inner.canonicalize(path)
    }
    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        self.inner.path_metadata(path)
    }
    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        self.inner.file_copy(src, dst)
    }
    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        self.inner.path_rename(old, new)
    }
    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        self.inner.file_remove(path)
    }
    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_create(path, recursive)
    }
    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_remove(path, recursive)
    }
    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        self.inner.dir_list(path)
    }
    fn cwd(&self) -> RuntimeResult<PathBuf> {
        self.inner.cwd()
    }
    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        self.inner.temp_dir(template)
    }
    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        self.inner.exec_pipe(command, args, stdin)
    }
    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        self.inner.exec_command(command, args, stdin)
    }
    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        self.inner.env_get(name)
    }
    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        self.inner.env_all()
    }
    fn os_name(&self) -> &'static str {
        self.inner.os_name()
    }
    fn arch(&self) -> &'static str {
        self.inner.arch()
    }
    fn cpu_time(&self) -> RuntimeResult<u64> {
        self.inner.cpu_time()
    }
    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        self.inner.xdg_dir(kind, subpath)
    }
    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stdout_write(data)
    }
    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stderr_write(data)
    }
    fn sass_available(&self) -> bool {
        self.inner.sass_available()
    }
    fn sass_compiler_name(&self) -> Option<&'static str> {
        self.inner.sass_compiler_name()
    }
    async fn compile_sass(
        &self,
        scss: &str,
        load_paths: &[PathBuf],
        minified: bool,
    ) -> RuntimeResult<SassOutput> {
        self.inner.compile_sass(scss, load_paths, minified).await
    }
}

/// A directory outside `/tmp` (wasm-mode requests reject mounts under it).
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r6-remote-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn render(root: &Path, qmd: &str, format: &str, net: &Arc<ScriptedNet>) -> PandocRequestOutcome {
    let doc = root.join("doc.qmd");
    std::fs::write(&doc, qmd).unwrap();
    std::fs::write(root.join("local.png"), b"local").unwrap();
    let project = ProjectContext::discover(&doc, net.as_ref()).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            attribution: None,
            scope: quarto_core::pandoc_request::render::BookScope::Auto,
            captures_by_path: Default::default(),
            capture_error: None,
            hooks: None,
            path: &doc,
            content: qmd.as_bytes(),
            format,
            project: &project,
            source_date_epoch: Some(1_700_000_000),
            captures: Vec::new(),
            typst_available_fonts: None,
            resolver: None,
        },
        Arc::clone(net) as Arc<dyn SystemRuntime>,
    ))
}

fn input_json(request: &PandocRequest) -> String {
    let f = request
        .files
        .iter()
        .find(|f| f.path.ends_with("/pandoc-input.json"))
        .expect("pandoc-input.json");
    String::from_utf8(f.bytes.clone()).unwrap()
}

/// Every `Image` target in the serialized AST, with its key-values.
fn images(json: &str) -> Vec<(String, Vec<(String, String)>)> {
    fn walk(v: &Value, out: &mut Vec<(String, Vec<(String, String)>)>) {
        match v {
            Value::Object(m) => {
                if m.get("t").and_then(Value::as_str) == Some("Image") {
                    let c = m["c"].as_array().unwrap();
                    let attrs = c[0][2]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|kv| {
                            (
                                kv[0].as_str().unwrap().to_string(),
                                kv[1].as_str().unwrap().to_string(),
                            )
                        })
                        .collect();
                    out.push((c[2][0].as_str().unwrap().to_string(), attrs));
                }
                m.values().for_each(|c| walk(c, out));
            }
            Value::Array(a) => a.iter().for_each(|c| walk(c, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&serde_json::from_str(json).unwrap(), &mut out);
    out
}

fn remote_refs(request: &PandocRequest) -> Vec<&quarto_core::pandoc_request::RequestFile> {
    request
        .resource_refs
        .iter()
        .filter(|f| f.path.contains("/_remote/"))
        .collect()
}

fn warnings(out: &PandocRequestOutcome) -> Vec<String> {
    out.diagnostics
        .iter()
        .filter(|d| d.title.starts_with("Remote image"))
        .map(|d| d.title.clone())
        .collect()
}

const DOC: &str = "---\ntitle: T\n---\n\n\
![alt one](https://img.example.com/a.png)\n\n\
Again ![alt again](https://img.example.com/a.png) and ![](https://img.example.com/b){width=2in}.\n\n\
::: {.callout-note}\nIn a callout: ![c](https://img.example.com/c.jpg)\n:::\n\n\
![](data:image/png;base64,AAAA) ![](local.png) ![](//cdn.example.com/p.png)\n";

fn script() -> Arc<ScriptedNet> {
    ScriptedNet::new(&[
        (
            "https://img.example.com/a.png",
            Ok((PNG.to_vec(), "image/png".into())),
        ),
        // MIME wins over the URL, which has no extension at all.
        (
            "https://img.example.com/b",
            Ok((b"gif".to_vec(), "image/gif".into())),
        ),
        (
            "https://img.example.com/c.jpg",
            Ok((b"jpg".to_vec(), "image/jpeg".into())),
        ),
    ])
}

#[test]
fn docx_remote_images_are_fetched_once_mounted_and_rewritten() {
    let (_guard, root) = scratch();
    let net = script();
    let out = render(&root, DOC, "docx", &net);
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");

    let mut asked = net.asked();
    asked.sort();
    assert_eq!(
        asked,
        [
            "https://img.example.com/a.png",
            "https://img.example.com/b",
            "https://img.example.com/c.jpg"
        ],
        "each distinct http(s) URL once; data:, relative and protocol-relative are not fetched"
    );

    let mounted = remote_refs(request);
    assert_eq!(
        mounted.len(),
        3,
        "{:?}",
        request
            .resource_refs
            .iter()
            .map(|f| &f.path)
            .collect::<Vec<_>>()
    );
    let remote_dir = format!(
        "{}/_remote/",
        quarto_core::pandoc_request::normalize_request_path(&root)
    );
    assert!(
        mounted.iter().all(|f| f.path.starts_with(&remote_dir)),
        "{:?}",
        mounted.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    let exts: Vec<&str> = {
        let mut e: Vec<&str> = mounted
            .iter()
            .map(|f| f.path.rsplit('.').next().unwrap())
            .collect();
        e.sort_unstable();
        e
    };
    assert_eq!(exts, ["gif", "jpg", "png"]);
    assert!(mounted.iter().any(|f| f.bytes == PNG));
    // The document's own local image is still mounted too.
    assert!(
        request
            .resource_refs
            .iter()
            .any(|f| f.path.ends_with("/local.png"))
    );

    let imgs = images(&input_json(request));
    let remote: Vec<_> = imgs
        .iter()
        .filter(|(src, _)| src.contains("/_remote/"))
        .collect();
    assert_eq!(remote.len(), 4, "a.png twice, b, c: {imgs:?}");
    for (src, attrs) in &remote {
        assert!(src.starts_with(&remote_dir), "absolute: {src}");
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "q2-remote-src" && v.starts_with("https://img.example.com/")),
            "{attrs:?}"
        );
    }
    // What pandoc is left to resolve is only data:, local and `//` targets.
    assert!(imgs.iter().any(|(s, _)| s.starts_with("data:")));
    assert!(imgs.iter().any(|(s, _)| s.starts_with("//cdn.example.com")));
    assert!(warnings(&out).is_empty());
}

#[test]
fn a_failed_fetch_leaves_the_url_for_docx_with_a_warning() {
    let (_guard, root) = scratch();
    let net = ScriptedNet::new(&[("https://gone.example.com/x.png", Err("HTTP 404".into()))]);
    let out = render(
        &root,
        "![alt text](https://gone.example.com/x.png)\n",
        "docx",
        &net,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");
    assert!(remote_refs(request).is_empty());
    let imgs = images(&input_json(request));
    assert_eq!(imgs[0].0, "https://gone.example.com/x.png");
    let w = warnings(&out);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-11-1"))
    );
    assert!(
        w[0].contains("gone.example.com") && w[0].contains("HTTP 404"),
        "{}",
        w[0]
    );
}

#[test]
fn http_and_oversized_and_non_image_responses_are_refused() {
    let (_guard, root) = scratch();
    let too_big = vec![0u8; constants().limits.image_bytes as usize + 1];
    let net = ScriptedNet::new(&[
        (
            "https://big.example.com/big.png",
            Ok((too_big, "image/png".into())),
        ),
        (
            "https://page.example.com/page",
            Ok((b"<html>".to_vec(), "text/html".into())),
        ),
    ]);
    let qmd = "![](http://plain.example.com/a.png) ![](https://big.example.com/big.png) ![](https://page.example.com/page)\n";
    let out = render(&root, qmd, "pptx", &net);
    let request = out.request.as_ref().expect("request");
    assert!(remote_refs(request).is_empty());
    assert_eq!(images(&input_json(request)).len(), 3);
    assert_eq!(warnings(&out).len(), 3, "{:?}", warnings(&out));
    assert!(
        net.asked().iter().all(|u| !u.starts_with("http://")),
        "http:// is never fetched"
    );
}

#[test]
fn typst_replaces_a_failed_image_with_its_alt_text() {
    let (_guard, root) = scratch();
    let net = ScriptedNet::new(&[("https://gone.example.com/x.png", Err("HTTP 404".into()))]);
    let out = render(
        &root,
        "Before ![the alt text](https://gone.example.com/x.png) after.\n",
        "typst",
        &net,
    );
    let request = out.request.as_ref().expect("request");
    let json = input_json(request);
    // (The URL still appears in `listing-item.image` metadata, which nothing fetches.)
    assert!(
        images(&json).is_empty(),
        "typst.lua would fetch an Image and exit 83"
    );
    assert!(
        json.contains("\"Span\"") && json.contains("\"alt\""),
        "the alt text stays"
    );
    assert_eq!(warnings(&out).len(), 1);
    let code = out
        .diagnostics
        .iter()
        .find(|d| d.title.starts_with("Remote image"))
        .unwrap();
    assert_eq!(code.code.as_deref(), Some("Q-20-9"));
}

#[test]
fn typst_pdf_request_carries_the_remote_image() {
    let (_guard, root) = scratch();
    let net = script();
    let out = render(
        &root,
        "![alt](https://img.example.com/a.png)\n",
        "typst-pdf",
        &net,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");
    let mounted = remote_refs(request);
    assert_eq!(mounted.len(), 1);
    assert_eq!(mounted[0].bytes, PNG);
    let imgs = images(&input_json(request));
    assert_eq!(imgs[0].0, mounted[0].path);
}

#[test]
fn a_document_without_remote_images_does_not_touch_the_network() {
    let (_guard, root) = scratch();
    let net = ScriptedNet::new(&[]);
    let out = render(&root, "![](local.png)\n", "docx", &net);
    assert!(out.request.is_some());
    assert!(net.asked().is_empty());
    assert!(!root.join("_remote").exists());
}
