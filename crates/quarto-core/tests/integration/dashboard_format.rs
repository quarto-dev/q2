/*
 * tests/integration/dashboard_format.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `format: dashboard` identity (bd-dashboard-format-k21hd4vy, Phase 1).
 */

//! What a dashboard render looks like to the rest of the system before any
//! dashboard layout exists: it renders through the native HTML pipeline to
//! an `.html` file, and Lua sees it the way Quarto 1's filters do —
//! `FORMAT == "html"` (Q1 writes dashboards with pandoc's `html` writer),
//! with `quarto.doc.is_format("dashboard")` true through the base format.
//! Layout itself is asserted by the corpus harness (`dashboard_goldens`).

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn render(dir: &Path, format: &str) -> (std::path::PathBuf, String) {
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&dir.join("doc.qmd"), format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render failed: {e:?}"));
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    (result.output_path, html)
}

#[test]
fn dashboard_renders_to_html_through_the_native_pipeline() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat: dashboard\n---\n\n## Row\n\nDASH-BODY text.\n",
    );
    let (path, html) = render(temp.path(), "dashboard");
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("html"));
    assert!(html.contains("DASH-BODY"), "{html}");
}

/// A user filter in a dashboard render sees `FORMAT == "html"`, and
/// `is_format` recognizes both `html` and `dashboard` but not `revealjs`.
#[test]
fn dashboard_lua_sees_html_format_and_dashboard_base() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("probe.lua"),
        r#"
function Pandoc(doc)
  local s = "FORMAT=" .. FORMAT
    .. " html=" .. tostring(quarto.doc.is_format("html"))
    .. " dashboard=" .. tostring(quarto.doc.is_format("dashboard"))
    .. " revealjs=" .. tostring(quarto.doc.is_format("revealjs"))
  doc.blocks:insert(pandoc.Para(pandoc.Str(s)))
  return doc
end
"#,
    );
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat: dashboard\nfilters:\n  - probe.lua\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    assert!(
        html.contains("FORMAT=html html=true dashboard=true revealjs=false"),
        "{html}"
    );

    // The same filter under plain html: no dashboard base format.
    let (_, html) = render(temp.path(), "html");
    assert!(
        html.contains("FORMAT=html html=true dashboard=false revealjs=false"),
        "{html}"
    );
}

/// Options under `format: dashboard:` flatten into the document metadata
/// (`MetadataMergeStage` keys the flattening by base format), so the
/// dashboard transforms will see `orientation`, `scrolling`, `nav-buttons`.
#[test]
fn dashboard_format_scoped_options_reach_metadata() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("probe.lua"),
        r#"
function Pandoc(doc)
  doc.blocks:insert(pandoc.Para(pandoc.Str(
    "ORIENTATION=" .. pandoc.utils.stringify(doc.meta.orientation or "unset")
    .. " SCROLLING=" .. tostring(doc.meta.scrolling))))
  return doc
end
"#,
    );
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat:\n  dashboard:\n    orientation: columns\n    scrolling: true\nfilters:\n  - probe.lua\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    assert!(html.contains("ORIENTATION=columns SCROLLING=true"), "{html}");
}
