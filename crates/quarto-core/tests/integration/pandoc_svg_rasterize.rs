//! Browser docx/pptx exports rasterize SVG images (`RasterizeSvgImagesStage`),
//! through `render_pandoc_request` over a directory with a runtime whose
//! rasterizer is scripted. The real `<img>`/`<canvas>` rasterizer and the
//! pixel size pandoc gives its PNG are covered in the Playwright spec
//! `pandoc-svg-rasterize.harness.spec.ts`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, constants, normalize_request_path};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{
    CommandOutput, NativeRuntime, PathKind, PathMetadata, RuntimeError, RuntimeResult, SassOutput,
    SystemRuntime, TempDir, XdgDirKind,
};
use serde_json::Value;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nraster";
const SVG_A: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 5'><rect/></svg>";
const SVG_B: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 10'><rect/></svg>";

type Reply = Box<dyn Fn(&[u8]) -> RuntimeResult<Vec<u8>> + Send + Sync>;

/// `NativeRuntime` for everything but the rasterizer, which answers from a
/// closure and records every SVG it was asked for.
struct ScriptedRaster {
    inner: NativeRuntime,
    can: bool,
    reply: Reply,
    calls: Mutex<Vec<(Vec<u8>, u32)>>,
}

impl ScriptedRaster {
    fn new(can: bool, reply: Reply) -> Arc<Self> {
        Arc::new(Self {
            inner: NativeRuntime::new(),
            can,
            reply,
            calls: Mutex::new(Vec::new()),
        })
    }

    /// A rasterizer that always answers `PNG`.
    fn ok() -> Arc<Self> {
        Self::new(true, Box::new(|_| Ok(PNG.to_vec())))
    }

    fn calls(&self) -> Vec<(Vec<u8>, u32)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SystemRuntime for ScriptedRaster {
    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        self.inner.fetch_url(url).await
    }
    fn can_rasterize_svg(&self) -> bool {
        self.can
    }
    async fn rasterize_svg(&self, svg: &[u8], max_side: u32) -> RuntimeResult<Vec<u8>> {
        self.calls.lock().unwrap().push((svg.to_vec(), max_side));
        (self.reply)(svg)
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
        .prefix("q2-svg-raster-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("a.svg"), SVG_A).unwrap();
    std::fs::write(root.join("B.SVG"), SVG_B).unwrap();
    std::fs::write(root.join("sp ace.svg"), SVG_A).unwrap();
    std::fs::write(root.join("local.png"), b"local").unwrap();
    (dir, root)
}

fn render(
    root: &Path,
    qmd: &str,
    format: &str,
    runtime: &Arc<ScriptedRaster>,
) -> PandocRequestOutcome {
    let doc = root.join("doc.qmd");
    std::fs::write(&doc, qmd).unwrap();
    let project = ProjectContext::discover(&doc, runtime.as_ref()).unwrap();
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
        Arc::clone(runtime) as Arc<dyn SystemRuntime>,
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

fn raster_refs(request: &PandocRequest) -> Vec<&quarto_core::pandoc_request::RequestFile> {
    request
        .resource_refs
        .iter()
        .filter(|f| f.path.contains("/_raster/"))
        .collect()
}

fn svg_warnings(out: &PandocRequestOutcome) -> Vec<String> {
    out.diagnostics
        .iter()
        .filter(|d| d.title.starts_with("SVG image"))
        .map(|d| d.title.clone())
        .collect()
}

const DOC: &str = "---\ntitle: T\n---\n\n\
![one](a.svg)\n\n\
Again ![again](a.svg) and ![upper](B.SVG){width=2in} and ![spaced](sp%20ace.svg).\n\n\
::: {.callout-note}\nIn a callout: ![c](a.svg)\n:::\n\n\
| h |\n|---|\n| ![t](B.SVG) |\n\n\
![query](a.svg?x=1) ![png](local.png)\n";

#[test]
fn docx_svgs_become_pngs_mounted_once_per_svg_and_rewritten() {
    let (_guard, root) = scratch();
    let rt = ScriptedRaster::ok();
    let out = render(&root, DOC, "docx", &rt);
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");

    // a.svg and `sp ace.svg` have the same bytes: three distinct files, two distinct SVGs.
    let calls = rt.calls();
    assert_eq!(
        calls.len(),
        2,
        "one call per distinct SVG, in document order"
    );
    assert_eq!(calls[0], (SVG_A.to_vec(), 2048));
    assert_eq!(calls[1].0, SVG_B.to_vec());

    // Same PNG bytes for both, so the mounted name (SVG-hash) is the only thing that differs.
    let mounted = raster_refs(request);
    assert_eq!(
        mounted.len(),
        2,
        "{:?}",
        request
            .resource_refs
            .iter()
            .map(|f| &f.path)
            .collect::<Vec<_>>()
    );
    let raster_dir = format!("{}/_raster/", normalize_request_path(&root));
    assert!(
        mounted
            .iter()
            .all(|f| f.path.starts_with(&raster_dir) && f.path.ends_with(".png"))
    );
    assert!(mounted.iter().all(|f| f.bytes == PNG));
    assert!(
        !request
            .resource_refs
            .iter()
            .any(|f| f.path.to_ascii_lowercase().ends_with(".svg")),
        "no SVG is mounted: every reference was rewritten"
    );

    let imgs = images(&input_json(request));
    let rewritten: Vec<_> = imgs
        .iter()
        .filter(|(s, _)| s.contains("/_raster/"))
        .collect();
    // a.svg x3 (incl. callout), `sp%20ace.svg`, B.SVG x2 (incl. table).
    assert_eq!(rewritten.len(), 6, "{imgs:?}");
    for (src, attrs) in &rewritten {
        assert!(
            src.starts_with(&raster_dir) && src.ends_with(".png"),
            "absolute .png: {src}"
        );
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "q2-raster-src" && v.to_ascii_lowercase().contains(".svg")),
            "{attrs:?}"
        );
    }
    // What is left: the query-string target (not a file) and the PNG.
    let left: Vec<&str> = imgs
        .iter()
        .filter(|(s, _)| !s.contains("/_raster/"))
        .map(|(s, _)| s.as_str())
        .collect();
    assert_eq!(left, ["a.svg?x=1", "local.png"]);
    assert!(svg_warnings(&out).is_empty());
}

#[test]
fn the_request_is_deterministic_for_a_deterministic_rasterizer() {
    let (_guard, root) = scratch();
    let a = render(&root, DOC, "docx", &ScriptedRaster::ok());
    let b = render(&root, DOC, "docx", &ScriptedRaster::ok());
    assert_eq!(a.request.unwrap().job_id, b.request.unwrap().job_id);
}

#[test]
fn only_docx_and_pptx_rasterize() {
    for format in ["pptx", "docx"] {
        let (_guard, root) = scratch();
        let rt = ScriptedRaster::ok();
        let out = render(&root, "![x](a.svg)\n", format, &rt);
        assert_eq!(rt.calls().len(), 1, "{format}");
        assert_eq!(
            raster_refs(out.request.as_ref().unwrap()).len(),
            1,
            "{format}"
        );
    }
    for format in ["typst", "epub"] {
        let (_guard, root) = scratch();
        let rt = ScriptedRaster::ok();
        let out = render(&root, "![x](a.svg)\n", format, &rt);
        assert!(rt.calls().is_empty(), "{format} keeps the SVG");
        let imgs = images(&input_json(out.request.as_ref().unwrap()));
        assert!(
            imgs.iter().all(|(s, _)| !s.contains("/_raster/")),
            "{format}"
        );
    }
}

#[test]
fn a_runtime_that_cannot_rasterize_leaves_the_svg_silently() {
    let (_guard, root) = scratch();
    let rt = ScriptedRaster::new(false, Box::new(|_| panic!("must not be called")));
    let out = render(&root, DOC, "docx", &rt);
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(rt.calls().is_empty());
    assert!(raster_refs(out.request.as_ref().unwrap()).is_empty());
    assert!(svg_warnings(&out).is_empty());
}

#[test]
fn not_supported_mid_render_is_silent_and_stops_asking() {
    let (_guard, root) = scratch();
    let rt = ScriptedRaster::new(
        true,
        Box::new(|_| Err(RuntimeError::NotSupported("no DOM".into()))),
    );
    let out = render(&root, DOC, "docx", &rt);
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(rt.calls().len(), 1, "unavailable once means unavailable");
    assert!(svg_warnings(&out).is_empty());
    assert!(raster_refs(out.request.as_ref().unwrap()).is_empty());
}

#[test]
fn a_failed_rasterization_keeps_that_svg_and_warns_naming_it() {
    let (_guard, root) = scratch();
    let rt = ScriptedRaster::new(
        true,
        Box::new(|svg| {
            if svg == SVG_B {
                Err(RuntimeError::Io(std::io::Error::other("tainted canvas")))
            } else {
                Ok(PNG.to_vec())
            }
        }),
    );
    let out = render(&root, "![ok](a.svg) ![bad](B.SVG)\n", "docx", &rt);
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().unwrap();
    let imgs = images(&input_json(request));
    assert!(imgs[0].0.contains("/_raster/"));
    assert_eq!(imgs[1].0, "B.SVG", "left for pandoc: alt text, as today");
    let w = svg_warnings(&out);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("B.SVG") && w[0].contains("tainted canvas"),
        "{}",
        w[0]
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-11-1"))
    );
}

#[test]
fn an_oversized_png_keeps_the_svg_instead_of_failing_the_download() {
    let (_guard, root) = scratch();
    let big = vec![0u8; constants().limits.image_bytes as usize + 1];
    let rt = ScriptedRaster::new(true, Box::new(move |_| Ok(big.clone())));
    let out = render(&root, "![big](a.svg)\n", "docx", &rt);
    // Mounting the PNG would push a Q-11-1 *error* and drop the whole request.
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");
    assert!(raster_refs(request).is_empty());
    assert_eq!(images(&input_json(request))[0].0, "a.svg");
    let w = svg_warnings(&out);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("a.svg") && w[0].contains("limit"), "{}", w[0]);
}

#[test]
fn a_missing_svg_is_left_to_pandoc() {
    let (_guard, root) = scratch();
    let rt = ScriptedRaster::ok();
    let out = render(&root, "![gone](nope.svg)\n", "docx", &rt);
    assert!(rt.calls().is_empty());
    assert_eq!(
        images(&input_json(out.request.as_ref().unwrap()))[0].0,
        "nope.svg"
    );
    assert!(svg_warnings(&out).is_empty());
}
