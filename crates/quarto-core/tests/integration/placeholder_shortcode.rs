//! The built-in `placeholder` shortcode (bd-gebcr1sa): SVG wherever the
//! output takes it, PNG for LaTeX and docx, rasterized locally with the
//! `typst` binary (quarto-cli bc2d42e00) instead of the retired
//! svg2png.deno.dev service. Without typst, a PNG placeholder fails loudly.
//!
//! The tests that rasterize need `typst` (`QUARTO_TYPST` or `PATH`; CI
//! provisions it) and skip without it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use pampa::lua::LuaOptions;
use pampa::lua::shortcode::{
    LuaShortcodeEngine, LuaShortcodeResult, ShortcodeArgs, ShortcodeCallContext,
};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn script() -> PathBuf {
    Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
        .join("../../resources/extensions/quarto/placeholder/placeholder.lua")
}

fn typst() -> Option<PathBuf> {
    quarto_core::render::find_typst(&NativeRuntime::new())
}

/// Call `{{< placeholder 120 60 [format=…] >}}` in text context for
/// `target_format`, with `typst` as `quarto.paths.typst()`.
async fn placeholder(
    target_format: &str,
    format_kwarg: Option<&str>,
    typst: Option<PathBuf>,
) -> LuaShortcodeResult {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = LuaOptions {
        typst,
        ..Default::default()
    };
    let mut engine = LuaShortcodeEngine::new_with(target_format, runtime, &options).unwrap();
    engine.load_script(&script()).await.unwrap();
    let args = ShortcodeArgs {
        positional: vec!["120".to_string(), "60".to_string()],
        keyword: format_kwarg
            .map(|f| vec![("format".to_string(), f.to_string())])
            .unwrap_or_default(),
        metadata: ConfigValue::new_map(vec![], SourceInfo::generated(By::programmatic_config())),
    };
    engine
        .call("placeholder", &args, ShortcodeCallContext::Text)
        .await
        .expect("placeholder handler is registered")
}

fn text(result: LuaShortcodeResult) -> String {
    match result {
        LuaShortcodeResult::Text(t) => t,
        other => panic!("expected a data URI, got {other:?}"),
    }
}

fn error(result: LuaShortcodeResult) -> String {
    match result {
        LuaShortcodeResult::Error(e) => e,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// HTML and Typst take SVG natively: no typst binary is consulted.
#[tokio::test]
async fn defaults_to_svg_where_the_output_takes_it() {
    for target in ["html", "revealjs", "typst", "gfm"] {
        let uri = text(placeholder(target, None, None).await);
        assert!(
            uri.starts_with("data:image/svg+xml;base64,"),
            "{target}: {uri}"
        );
    }
}

/// LaTeX and docx default to PNG, which without typst fails loudly
/// instead of falling back to SVG or text.
#[tokio::test]
async fn latex_and_docx_default_to_png_and_fail_loudly_without_typst() {
    for target in ["latex", "pdf", "beamer", "docx"] {
        let err = error(placeholder(target, None, None).await);
        assert!(err.contains("needs the typst binary"), "{target}: {err}");
    }
}

/// `format=svg` still overrides the PNG default.
#[tokio::test]
async fn explicit_svg_overrides_the_png_default() {
    let uri = text(placeholder("latex", Some("svg"), None).await);
    assert!(uri.starts_with("data:image/svg+xml;base64,"), "{uri}");
}

/// A typst that cannot run is a `fatal`, naming the failure.
#[tokio::test]
async fn a_broken_typst_fails_loudly() {
    let missing = Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
        .join("no-such-typst-binary");
    let err = error(placeholder("html", Some("png"), Some(missing)).await);
    assert!(
        err.contains("failed to rasterize SVG to PNG via typst"),
        "{err}"
    );
}

#[tokio::test]
async fn png_is_rasterized_locally_with_typst() {
    let Some(typst) = typst() else {
        eprintln!("skipping: no typst binary");
        return;
    };
    let uri = text(placeholder("html", Some("png"), Some(typst)).await);
    let b64 = uri
        .strip_prefix("data:image/png;base64,")
        .unwrap_or_else(|| panic!("expected a PNG data URI, got {uri}"));
    let png = BASE64_STANDARD.decode(b64).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

/// Upstream's contract test (`placeholder-typst-text.test.ts`): the
/// rasterization passes `--ignore-system-fonts`, so the dimension label
/// only shows if typst draws it with its embedded fonts. A labelled SVG
/// must give a materially larger PNG than a blank one.
#[test]
fn typst_renders_the_label_with_embedded_fonts() {
    let Some(typst) = typst() else {
        eprintln!("skipping: no typst binary");
        return;
    };
    let dir = tempfile::TempDir::new().unwrap();
    let render = |label: &str, out: &str| -> Vec<u8> {
        let svg = format!(
            "<svg width=\"120\" height=\"60\" xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 120 60\"><rect width=\"120\" height=\"60\" fill=\"#ddd\"/><text x=\"50%\" y=\"50%\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#000\" text-anchor=\"middle\">{label}</text></svg>"
        );
        std::fs::write(dir.path().join("in.svg"), svg).unwrap();
        std::fs::write(
            dir.path().join("in.typ"),
            "#set page(width: auto, height: auto, margin: 0pt)\n#image(\"in.svg\")\n",
        )
        .unwrap();
        let png = dir.path().join(out);
        let status = std::process::Command::new(&typst)
            .arg("compile")
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "png", "--ppi", "96", "--ignore-system-fonts"])
            .arg(dir.path().join("in.typ"))
            .arg(&png)
            .status()
            .unwrap();
        assert!(status.success(), "typst compile failed");
        std::fs::read(png).unwrap()
    };
    let with_text = render("120 x 60", "label.png");
    let blank = render(" ", "blank.png");
    assert!(
        with_text.len() > blank.len() + 100,
        "label did not render: {}B vs blank {}B",
        with_text.len(),
        blank.len()
    );
}
