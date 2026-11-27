/*
 * layout_panels.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Layout panels: a `layout-ncol` / `layout-nrow` / `layout` Div becomes
 * rows and cells in the HTML and reveal.js formats.
 */

//! End-to-end tests for `LayoutPanelTransform`.
//!
//! Each test renders a document through `render_to_file` and reads the HTML
//! written to disk. The expected markup is what Quarto 1.9.38 writes for the
//! same source (`layout/layout.lua` + `layout/html.lua`).

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Render `body` under `format` and return the HTML with whitespace removed.
fn render(format: &str, body: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    std::fs::write(&qmd_path, format!("---\nformat: {format}\n---\n\n{body}")).unwrap();
    std::fs::write(temp.path().join("x.png"), b"").unwrap();
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render failed: {e:?}"));
    let html = std::fs::read_to_string(Path::new(&result.output_path)).unwrap();
    html.chars().filter(|c| !c.is_whitespace()).collect()
}

const CELL_50: &str =
    "<divclass=\"quarto-layout-cell\"style=\"flex-basis:50.0%;justify-content:flex-start;\">";

const TWO_FIGURES: &str =
    "## Panel\n\n::: {layout-ncol=2}\n![Cap one](x.png)\n\n![Cap two](x.png)\n:::\n";

/// The checklist case: two captioned images side by side, in a deck and in
/// an HTML page. Before, the Div kept `data-layout-ncol` and the figures
/// stacked.
#[test]
fn ncol_makes_one_row_of_two_cells() {
    for format in ["revealjs", "html"] {
        let html = render(format, TWO_FIGURES);
        assert!(
            html.contains(&format!(
                "<divclass=\"quarto-layout-panel\"data-layout-ncol=\"2\">\
                 <divclass=\"quarto-layout-row\">{CELL_50}<divclass=\"quarto-figurequarto-figure-center\">"
            )),
            "{format}: panel, row and first cell; html:\n{html}"
        );
        assert_eq!(html.matches("class=\"quarto-layout-row\"").count(), 1);
        assert_eq!(html.matches(CELL_50).count(), 2, "{format}: two cells");
    }
}

/// `layout` gives rows of relative widths; a negative width is a spacer.
#[test]
fn layout_rows_widths_and_spacer() {
    let html = render(
        "revealjs",
        "## Panel\n\n::: {layout=\"[[40,-20,40],[100]]\"}\nONE\n\nTWO\n\nTHREE\n:::\n",
    );
    assert!(
        html.contains(
            "<divclass=\"quarto-layout-row\">\
             <divclass=\"quarto-layout-cell\"style=\"flex-basis:40.0%;justify-content:flex-start;\"><p>ONE</p></div>\
             <divclass=\"quarto-figure-spacerquarto-layout-cell\"style=\"flex-basis:20.0%;justify-content:flex-start;\">"
        ),
        "first row with its spacer; html:\n{html}"
    );
    assert!(
        html.contains(
            "<divclass=\"quarto-layout-row\">\
             <divclass=\"quarto-layout-cell\"style=\"flex-basis:100.0%;justify-content:flex-start;\"><p>THREE</p>"
        ),
        "second row; html:\n{html}"
    );
}

/// `layout-nrow` derives the columns, `layout-valign` becomes a row class
/// and leaves the panel, `layout-align` sets every cell.
#[test]
fn nrow_valign_and_align() {
    let html = render(
        "html",
        "::: {layout-nrow=2 layout-valign=\"bottom\" layout-align=\"right\"}\nA\n\nB\n\nC\n\nD\n:::\n",
    );
    assert_eq!(
        html.matches("<divclass=\"quarto-layout-rowquarto-layout-valign-bottom\">")
            .count(),
        2,
        "two rows; html:\n{html}"
    );
    assert_eq!(
        html.matches("style=\"flex-basis:50.0%;justify-content:flex-end;\"")
            .count(),
        4
    );
    assert!(!html.contains("data-layout-valign"));
}

/// A bare image or a table is centered in its cell; text starts at the left.
#[test]
fn bare_image_and_table_cells_are_centered() {
    let html = render(
        "html",
        "::: {layout-ncol=3}\n![](x.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nTEXT\n:::\n",
    );
    assert_eq!(
        html.matches("style=\"flex-basis:33.3%;justify-content:center;\"")
            .count(),
        2,
        "image and table cells; html:\n{html}"
    );
    assert_eq!(
        html.matches("style=\"flex-basis:33.3%;justify-content:flex-start;\"")
            .count(),
        1
    );
}

/// A child Div is the cell itself and keeps its classes.
#[test]
fn child_div_becomes_the_cell() {
    let html = render(
        "html",
        ":::: {layout-ncol=2}\n::: {.one}\nA\n:::\n\n::: {.two}\nB\n:::\n::::\n",
    );
    assert!(
        html.contains("<divclass=\"onequarto-layout-cell\"style=\"flex-basis:50.0%;")
            && html.contains("<divclass=\"twoquarto-layout-cell\"style=\"flex-basis:50.0%;"),
        "html:\n{html}"
    );
}

/// A figure div with layout attributes keeps its caption and gets its rows
/// inside the figure. Its outer Div becomes the panel, as in Quarto 1.
#[test]
fn figure_div_lays_out_its_content() {
    let html = render(
        "html",
        "::: {#fig-pair layout-ncol=2}\n![Left](x.png){#fig-l}\n\n![Right](x.png){#fig-r}\n\nTwo figures\n:::\n",
    );
    assert!(
        html.contains(
            "<divid=\"fig-pair\"class=\"quarto-layout-panel\"><figureclass=\"quarto-floatquarto-float-fig\">\
             <divaria-describedby=\"fig-pair-caption\"><divclass=\"quarto-layout-row\">"
        ),
        "panel around the figure; html:\n{html}"
    );
    assert_eq!(html.matches(CELL_50).count(), 2);
    assert!(html.contains("Twofigures</figcaption>"));
    assert!(!html.contains("data-layout-ncol"));
}

/// A code cell's source is not a cell: it moves in front of the panel, and
/// each output block gets a cell.
#[test]
fn code_cell_source_moves_in_front_of_the_panel() {
    let html = render(
        "html",
        "::: {.cell layout-ncol=\"2\"}\n```{.r .cell-code}\nplot(1)\n```\n\n\
         ::: {.cell-output-display}\n![](x.png)\n:::\n\n\
         ```{.r .cell-code}\nplot(2)\n```\n\n\
         ::: {.cell-output-display}\n![](x.png)\n:::\n:::\n",
    );
    let panel = html
        .find("<divclass=\"cellquarto-layout-panel\"data-layout-ncol=\"2\">")
        .unwrap_or_else(|| panic!("panel; html:\n{html}"));
    // The source is highlighted, so look for its `cell-code` wrapper.
    let code = html.find("cell-code").expect("the code is still there");
    assert!(code < panel, "the code comes before the panel");
    assert!(
        !html[panel..].contains("cell-code"),
        "no code is left inside the panel"
    );
    assert_eq!(
        html.matches("<divclass=\"quarto-layout-cell\"style=\"flex-basis:50.0%;justify-content:center;\"><p><img")
            .count(),
        2,
        "one cell per output; html:\n{html}"
    );
}

/// The row and cell rules ship in both stylesheets.
#[test]
fn layout_rules_ship_with_the_stylesheet() {
    for format in ["revealjs", "html"] {
        let temp = tempfile::TempDir::new().unwrap();
        let qmd_path = temp.path().join("doc.qmd");
        std::fs::write(
            &qmd_path,
            format!("---\nformat: {format}\n---\n\n{TWO_FIGURES}"),
        )
        .unwrap();
        std::fs::write(temp.path().join("x.png"), b"").unwrap();
        let options = RenderToFileOptions {
            quiet: true,
            ..Default::default()
        };
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let result = render_to_file(&qmd_path, format, &options, runtime).unwrap();
        let mut css = String::new();
        for entry in walk(temp.path()) {
            if entry.extension().is_some_and(|e| e == "css") {
                css.push_str(&std::fs::read_to_string(&entry).unwrap());
            }
        }
        drop(result);
        let css: String = css.chars().filter(|c| !c.is_whitespace()).collect();
        for rule in [
            ".quarto-layout-row{display:flex;flex-direction:row;align-items:flex-start",
            ".quarto-layout-cell{position:relative;margin-right:20px",
            ".quarto-layout-valign-bottom{align-items:flex-end",
        ] {
            assert!(css.contains(rule), "{format}: missing `{rule}`");
        }
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Formats other than HTML and reveal.js are left alone: a dashboard reads
/// its own layout from Divs.
#[test]
fn dashboard_is_untouched() {
    let html = render("dashboard", TWO_FIGURES);
    assert!(!html.contains("quarto-layout-panel"), "html:\n{html}");
}
