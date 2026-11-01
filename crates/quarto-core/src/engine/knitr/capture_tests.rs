/*
 * engine/knitr/capture_tests.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-1 Phase 1B: the R-side capture (`resources/rmd/capture.R`). These drive
//! `execute` through `call_r` with real R and knitr, capture on and off; a
//! missing R is a skip, a signal about the machine.

use std::process::Command;

use super::types::KnitrPiece;
use super::{
    CallROptions, KNITR_RESOURCES, KnitrExecuteParams, KnitrExecuteResult, KnitrFormatConfig,
    call_r,
};

fn r_available(name: &str) -> bool {
    let ok = Command::new("Rscript")
        .args([
            "-e",
            "for (p in c('knitr', 'rmarkdown', 'jsonlite')) if (!requireNamespace(p, quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("SKIP: Rscript with knitr, rmarkdown and jsonlite not available: {name}");
    }
    ok
}

/// Run `execute` on `markdown`; the temp dir is returned so figure files stay on disk.
fn execute(markdown: &str, capture: bool) -> (KnitrExecuteResult, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    let qmd = path.join("doc.qmd");
    std::fs::write(&qmd, markdown).unwrap();
    let params = KnitrExecuteParams {
        input: qmd,
        markdown: markdown.to_string(),
        format: KnitrFormatConfig::with_defaults("html"),
        temp_dir: path.to_path_buf(),
        lib_dir: None,
        dependencies: true,
        cwd: path.to_path_buf(),
        params: None,
        resource_dir: KNITR_RESOURCES.path().unwrap().to_path_buf(),
        handled_languages: vec![],
        capture,
    };
    let result =
        call_r("execute", &params, path, path, &CallROptions::quiet()).expect("R execution failed");
    (result, dir)
}

const MIXED: &str = "```{r a}\nx <- 1:3\nx\nmessage('hi')\nwarning('careful')\n```\n\n\
```{r b}\n#| error: true\nstop('boom')\n```\n\n\
```{r c}\nplot(1)\n```\n\n\
```{r d}\n#| eval: false\nnever()\n```\n";

#[test]
fn capture_off_returns_no_pieces() {
    if !r_available("capture_off_returns_no_pieces") {
        return;
    }
    let (result, _dir) = execute(MIXED, false);
    assert!(result.pieces.is_empty());
}

#[test]
fn capture_records_one_chunk_per_chunk_in_order() {
    if !r_available("capture_records_one_chunk_per_chunk_in_order") {
        return;
    }
    let (result, _dir) = execute(MIXED, true);
    let seen: Vec<(u32, &str)> = result
        .pieces
        .iter()
        .map(|c| (c.ordinal, c.label.as_str()))
        .collect();
    assert_eq!(seen, [(1, "a"), (2, "b"), (3, "c"), (4, "d")]);
}

#[test]
fn capture_classifies_pieces() {
    if !r_available("capture_classifies_pieces") {
        return;
    }
    let (result, _dir) = execute(MIXED, true);
    let a = &result.pieces[0];
    assert_eq!(a.code, "x <- 1:3\nx\nmessage('hi')\nwarning('careful')");
    let kinds: Vec<&str> = a.pieces.iter().map(kind).collect();
    assert_eq!(
        kinds,
        [
            "source", "source", "output", "source", "message", "source", "warning"
        ]
    );
    match &a.pieces[2] {
        KnitrPiece::Output { text } => assert_eq!(text, "[1] 1 2 3\n"),
        other => panic!("{other:?}"),
    }
    match &a.pieces[6] {
        // as knitr formats it: no call for a top-level warning, wrapped at `width`
        KnitrPiece::Warning { text, .. } => assert_eq!(text, "Warning: careful"),
        other => panic!("{other:?}"),
    }
    let b = &result.pieces[1];
    match b.pieces.last().unwrap() {
        KnitrPiece::Error {
            ename,
            evalue,
            text,
            ..
        } => {
            assert_eq!(ename, "simpleError");
            assert_eq!(evalue, "boom");
            assert!(text.starts_with("Error"), "{text}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn capture_plot_piece_names_a_file_that_exists() {
    if !r_available("capture_plot_piece_names_a_file_that_exists") {
        return;
    }
    let (result, dir) = execute(MIXED, true);
    let c = &result.pieces[2];
    let KnitrPiece::Plot { files } = c.pieces.last().unwrap() else {
        panic!("{:?}", c.pieces);
    };
    assert_eq!(files.len(), 1);
    assert!(dir.path().join(&files[0]).is_file(), "{files:?}");
    assert_eq!(c.files, *files);
}

#[test]
fn capture_several_plots_each_name_their_own_file() {
    if !r_available("capture_several_plots_each_name_their_own_file") {
        return;
    }
    let (result, dir) = execute(
        "```{r p}\n#| fig.keep: all\n#| dev: [png, pdf]\nplot(1:5)\nlines(1:5)\nplot(5:1)\n```\n",
        true,
    );
    let named: Vec<&String> = result.pieces[0]
        .pieces
        .iter()
        .filter_map(|p| match p {
            KnitrPiece::Plot { files } => Some(&files[0]),
            _ => None,
        })
        .collect();
    assert_eq!(named.len(), 3);
    let mut distinct = named.clone();
    distinct.dedup();
    assert_eq!(distinct, named, "one distinct file per plot: {named:?}");
    for f in &named {
        // the first device's file, not the pdf
        assert!(f.ends_with(".png"), "{f}");
        assert!(dir.path().join(f).is_file());
    }
}

#[test]
fn capture_include_graphics_is_an_image_paths_piece() {
    if !r_available("capture_include_graphics_is_an_image_paths_piece") {
        return;
    }
    let (result, _dir) = execute(
        "```{r}\npng('made.png', width = 200, height = 200)\nplot(1:3)\ninvisible(dev.off())\n```\n\n```{r incl}\nknitr::include_graphics('made.png')\n```\n",
        true,
    );
    let incl = &result.pieces[1];
    match incl.pieces.last().unwrap() {
        KnitrPiece::ImagePaths { paths, widths } => {
            assert_eq!(paths, &["made.png"]);
            // knitr's `raster_dpi_width`: the PNG\'s pixel width at its own resolution
            assert_eq!(widths, &[Some(200.0)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(incl.files, ["made.png"]);
}

#[test]
fn capture_include_graphics_with_out_width_has_no_computed_width() {
    if !r_available("capture_include_graphics_with_out_width_has_no_computed_width") {
        return;
    }
    let (result, _dir) = execute(
        "```{r}\npng('made.png', width = 200, height = 200)\nplot(1:3)\ninvisible(dev.off())\n```\n\n```{r incl, out.width='50%'}\nknitr::include_graphics('made.png')\n```\n",
        true,
    );
    match result.pieces[1].pieces.last().unwrap() {
        KnitrPiece::ImagePaths { widths, .. } => assert_eq!(widths, &[None]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn capture_eval_false_chunk_is_source_only() {
    if !r_available("capture_eval_false_chunk_is_source_only") {
        return;
    }
    let (result, _dir) = execute(MIXED, true);
    let d = &result.pieces[3];
    assert_eq!(d.pieces.len(), 1);
    match &d.pieces[0] {
        KnitrPiece::Source { text } => assert_eq!(text, "never()\n"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn capture_other_engine_is_a_fallback_piece() {
    if !r_available("capture_other_engine_is_a_fallback_piece") {
        return;
    }
    let (result, _dir) = execute("```{bash}\necho hi\n```\n", true);
    match &result.pieces[0].pieces[0] {
        KnitrPiece::Other { classes } => assert_eq!(classes, &["bash"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn capture_records_resolved_options_and_forces_cache_off() {
    if !r_available("capture_records_resolved_options_and_forces_cache_off") {
        return;
    }
    let (result, dir) = execute(
        "```{r k}\n#| cache: true\n#| fig-width: 5\n#| fig.cap: !expr paste('a', 'b')\nplot(1)\n```\n",
        true,
    );
    let o = &result.pieces[0].options;
    assert_eq!(o["cache"], 0, "knitr's cache is off while recording");
    assert_eq!(
        o["fig.width"], 5,
        "Quarto's spelling is normalized to knitr's"
    );
    assert_eq!(o["fig.cap"], "a b", "the resolved caption, not the call");
    assert!(o.get("code").is_none(), "document text stays out: {o}");
    assert!(o.get("fig.path").is_none());
    assert!(!dir.path().join("doc_cache").exists(), "no cache directory");
}

#[test]
fn capture_does_not_change_the_markdown() {
    if !r_available("capture_does_not_change_the_markdown") {
        return;
    }
    let (off, _a) = execute(MIXED, false);
    let (on, _b) = execute(MIXED, true);
    assert_eq!(off.markdown, on.markdown);
}

fn kind(p: &KnitrPiece) -> &'static str {
    match p {
        KnitrPiece::Source { .. } => "source",
        KnitrPiece::Output { .. } => "output",
        KnitrPiece::Message { .. } => "message",
        KnitrPiece::Warning { .. } => "warning",
        KnitrPiece::Error { .. } => "error",
        KnitrPiece::Plot { .. } => "plot",
        KnitrPiece::ImagePaths { .. } => "image_paths",
        KnitrPiece::Asis { .. } => "asis",
        KnitrPiece::Other { .. } => "other",
    }
}
