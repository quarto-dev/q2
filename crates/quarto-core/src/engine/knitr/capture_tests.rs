/*
 * engine/knitr/capture_tests.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-1 Phase 1B: the R-side capture (`resources/rmd/capture.R`). These drive
//! `execute` through `call_r` with real R and knitr, capture on and off; a
//! missing R is a skip, a signal about the machine.

use std::process::Command;

use super::types::{KnitrCapturedChunk, KnitrPiece};
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
    execute_with_files(markdown, &[], capture)
}

/// [`execute`] with extra files (child documents) written beside the document first.
fn execute_with_files(
    markdown: &str,
    files: &[(&str, &str)],
    capture: bool,
) -> (KnitrExecuteResult, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let result = execute_in(dir.path(), markdown, files, capture);
    (result, dir)
}

/// [`execute_with_files`] in a directory the caller owns, so a second run sees the first run\'s files.
fn execute_in(
    path: &std::path::Path,
    markdown: &str,
    files: &[(&str, &str)],
    capture: bool,
) -> KnitrExecuteResult {
    execute_with_format(
        path,
        markdown,
        files,
        capture,
        KnitrFormatConfig::with_defaults("html"),
    )
}

/// [`execute_in`] with the document\'s format (its `execute:` scope) chosen by the caller.
fn execute_with_format(
    path: &std::path::Path,
    markdown: &str,
    files: &[(&str, &str)],
    capture: bool,
    format: KnitrFormatConfig,
) -> KnitrExecuteResult {
    let qmd = path.join("doc.qmd");
    std::fs::write(&qmd, markdown).unwrap();
    for (name, text) in files {
        std::fs::write(path.join(name), text).unwrap();
    }
    let params = KnitrExecuteParams {
        input: qmd,
        markdown: markdown.to_string(),
        format,
        temp_dir: path.to_path_buf(),
        lib_dir: None,
        dependencies: true,
        cwd: path.to_path_buf(),
        params: None,
        resource_dir: KNITR_RESOURCES.path().unwrap().to_path_buf(),
        handled_languages: vec![],
        capture,
        nonce: None,
    };
    call_r("execute", &params, path, path, &CallROptions::quiet()).expect("R execution failed")
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
fn capture_reports_knitrs_known_option_names_once() {
    if !r_available("capture_reports_knitrs_known_option_names_once") {
        return;
    }
    let (result, _dir) = execute(MIXED, true);
    let known = result
        .known_options
        .as_ref()
        .expect("a captured run reports the option names knitr knows");
    // The names hooks.R does not forward as attributes: knitr\'s own, then Quarto\'s three groups
    // (`quarto_knitr_opts`, `quarto_opts`, `other_opts`).
    for name in [
        "echo",
        "fig.width",
        "fig.cap",
        "code-fold",
        "layout-ncol",
        "class.source",
        "attr.output",
        "fenced.echo",
        "quarto-unit-id",
        // set by execute.R on the format\'s `opts_chunk`, which knitr applies before any chunk
        "ft.shadow",
    ] {
        assert!(known.iter().any(|k| k == name), "{name} in {known:?}");
    }
    assert!(!known.iter().any(|k| k == "foo.unknown"), "{known:?}");
    // Capture off: nothing to report.
    let (result, _dir) = execute(MIXED, false);
    assert!(result.known_options.is_none());
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

/// knitr >= 1.51 re-signals every chunk error through `rlang::entrace()` when rlang is
/// installed, which rewraps it as `rlang_error` (bd-54odm3rz). The error piece names
/// the condition the author raised, so the record does not depend on the knitr version.
#[test]
fn capture_error_names_the_condition_the_author_raised() {
    if !r_available("capture_error_names_the_condition_the_author_raised") {
        return;
    }
    let (result, _dir) = execute(
        "```{r e}\n#| error: true\n\
         f <- function() {\n\
           cnd <- structure(class = c('myError', 'error', 'condition'),\n\
             list(message = 'custom', call = sys.call()))\n\
           stop(cnd)\n\
         }\n\
         f()\n\
         g <- function() stop('inner')\n\
         g()\n\
         h <- function() rlang::abort('aborted', class = 'myAbort')\n\
         h()\n```\n",
        true,
    );
    let errors: Vec<(&str, &str, Option<&str>)> = result.pieces[0]
        .pieces
        .iter()
        .filter_map(|p| match p {
            KnitrPiece::Error {
                ename,
                evalue,
                call,
                ..
            } => Some((ename.as_str(), evalue.as_str(), call.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(
        errors,
        [
            ("myError", "custom", Some("f()")),
            ("simpleError", "inner", Some("g()")),
            // an rlang error is not rewrapped; its own class is kept
            ("myAbort", "aborted", Some("h()")),
        ]
    );
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
fn capture_records_resolved_options() {
    if !r_available("capture_records_resolved_options") {
        return;
    }
    let (result, dir) = execute(
        "```{r k}\n#| fig-width: 5\n#| fig.cap: !expr paste('a', 'b')\nplot(1)\n```\n",
        true,
    );
    let o = &result.pieces[0].options;
    assert_eq!(
        o["fig.width"], 5,
        "Quarto's spelling is normalized to knitr's"
    );
    assert_eq!(o["fig.cap"], "a b", "the resolved caption, not the call");
    assert!(o.get("code").is_none(), "document text stays out: {o}");
    assert!(o.get("fig.path").is_none());
    assert!(!dir.path().join("doc_cache").exists(), "no cache directory");
}

/// KNR-3 slice 8: why a deleted header key re-runs instead of restoring a recorded default.
/// q2\'s `opts_hooks` derive presentation options from other keys, so deleting one key changes
/// options that are not header keys, and `opts_chunk$get` would give the wrong values back.
#[test]
fn deleting_a_header_key_changes_the_options_derived_from_it() {
    if !r_available("deleting_a_header_key_changes_the_options_derived_from_it") {
        return;
    }
    let options = |markdown: &str| execute(markdown, true).0.pieces[0].options.clone();

    let with = options("```{r fig-x}\n#| echo: fenced\nplot(1)\n```\n");
    let without = options("```{r fig-x}\nplot(1)\n```\n");
    // (`fig.show` is `hold` either way: the `fig-` label sets it on its own.)
    for derived in ["fenced.echo", "results"] {
        assert_ne!(
            with[derived], without[derived],
            "`{derived}` is derived from `echo: fenced`: {with} vs {without}"
        );
    }

    let with = options("```{r k}\n#| collapse: true\n1\n```\n");
    let without = options("```{r k}\n1\n```\n");
    assert_ne!(
        with["comment"], without["comment"],
        "`comment` is derived from `collapse`: {with} vs {without}"
    );
}

#[test]
fn capture_flags_an_overridden_hook_from_that_chunk_on() {
    if !r_available("capture_flags_an_overridden_hook_from_that_chunk_on") {
        return;
    }
    let (result, _dir) = execute(
        "```{r before}\n1\n```\n\n\
         ```{r override}\n#| include: false\nknitr::knit_hooks$set(output = function(x, options) paste0('X', x))\n```\n\n\
         ```{r after}\n2\n```\n",
        true,
    );
    let flags: Vec<Option<&str>> = result
        .pieces
        .iter()
        .map(|c| c.hook_override.as_deref())
        .collect();
    assert_eq!(flags, [None, Some("output"), Some("output")]);
}

#[test]
fn capture_without_an_override_flags_nothing() {
    if !r_available("capture_without_an_override_flags_nothing") {
        return;
    }
    let (result, _dir) = execute(MIXED, true);
    assert!(result.pieces.iter().all(|c| c.hook_override.is_none()));
}

/// A chunk with a unit id (`u<n>`) and the given option lines, as the engine writes them.
fn id_chunk(label: &str, id: &str, opts: &str, code: &str) -> String {
    format!("```{{r {label}}}\n#| quarto-unit-id: \"{id}\"\n{opts}{code}\n```\n\n")
}

#[test]
fn capture_names_the_units_that_asked_for_the_cache() {
    if !r_available("capture_names_the_units_that_asked_for_the_cache") {
        return;
    }
    // level 1: a level-3 chunk is recorded since OPR-7e and is not an unrecorded cached unit
    let doc = id_chunk("cached", "u1", "#| cache: 1\n", "1") + &id_chunk("plain", "u2", "", "2");
    let (result, _dir) = execute(&doc, true);
    assert_eq!(result.cached_units, ["u1"]);
    // the cached chunk is not recorded; the plain one is
    let ids: Vec<&str> = result
        .pieces
        .iter()
        .filter_map(|c| c.options.get("quarto-unit-id")?.as_str())
        .collect();
    assert_eq!(ids, ["u2"]);
    let (off, _dir) = execute(&doc, false);
    assert!(
        off.cached_units.is_empty(),
        "nothing is reported without capture"
    );
}

/// `execute: cache: false` still forces the cache off for every chunk: the capture wraps that
/// hook rather than replacing it.
#[test]
fn a_document_that_turns_the_cache_off_has_no_cached_units() {
    if !r_available("a_document_that_turns_the_cache_off_has_no_cached_units") {
        return;
    }
    let doc = id_chunk("c", "u1", "#| cache: true\n", "1");
    let dir = tempfile::tempdir().unwrap();
    let qmd = dir.path().join("doc.qmd");
    std::fs::write(&qmd, &doc).unwrap();
    let mut format = KnitrFormatConfig::with_defaults("html");
    format.execute.cache = Some(serde_json::json!(false));
    let params = KnitrExecuteParams {
        input: qmd,
        markdown: doc.clone(),
        format,
        temp_dir: dir.path().to_path_buf(),
        lib_dir: None,
        dependencies: true,
        cwd: dir.path().to_path_buf(),
        params: None,
        resource_dir: KNITR_RESOURCES.path().unwrap().to_path_buf(),
        handled_languages: vec![],
        capture: true,
        nonce: None,
    };
    let path = dir.path();
    let result: KnitrExecuteResult =
        call_r("execute", &params, path, path, &CallROptions::quiet()).unwrap();
    assert!(result.cached_units.is_empty(), "{:?}", result.cached_units);
    assert_eq!(result.pieces.len(), 1, "the chunk is recorded");
}

const CHILD_DOC: &str = "Before.\n\n\
    ```{r child_chunk, child='child.Rmd'}\n#| quarto-unit-id: \"u1\"\n```\n\n\
    ```{r after}\n#| quarto-unit-id: \"u2\"\n1\n```\n";
const CHILD_FILE: (&str, &str) = ("child.Rmd", "Child text.\n\n```{r inner}\n1 + 1\n```\n");

#[test]
fn a_child_chunk_gets_a_chunk_record_holding_the_childs_markdown() {
    if !r_available("a_child_chunk_gets_a_chunk_record_holding_the_childs_markdown") {
        return;
    }
    let (result, _dir) = execute_with_files(CHILD_DOC, &[CHILD_FILE], true);
    let ids: Vec<&str> = result.chunks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["u1", "u2"], "the child\'s inner chunk has no id");
    let child = &result.chunks[0];
    assert!(child.markdown.contains("Child text."), "{}", child.markdown);
    assert!(child.markdown.contains("1 + 1"), "{}", child.markdown);
    assert!(
        result.markdown.contains(&child.markdown),
        "the record holds exactly what landed in the document: {:?}",
        child.markdown
    );
}

#[test]
fn a_child_chunk_is_a_captured_fallback_piece_in_document_order() {
    if !r_available("a_child_chunk_is_a_captured_fallback_piece_in_document_order") {
        return;
    }
    let (result, _dir) = execute_with_files(CHILD_DOC, &[CHILD_FILE], true);
    let with_id: Vec<(&str, &KnitrCapturedChunk)> = result
        .pieces
        .iter()
        .filter_map(|c| Some((c.options.get("quarto-unit-id")?.as_str()?, c)))
        .collect();
    let ids: Vec<&str> = with_id.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["u1", "u2"]);
    match with_id[0].1.pieces.as_slice() {
        [KnitrPiece::Other { classes }] => assert_eq!(classes, &["child"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_child_chunk_does_not_change_the_markdown() {
    if !r_available("a_child_chunk_does_not_change_the_markdown") {
        return;
    }
    let (off, _a) = execute_with_files(CHILD_DOC, &[CHILD_FILE], false);
    let (on, _b) = execute_with_files(CHILD_DOC, &[CHILD_FILE], true);
    assert_eq!(off.markdown, on.markdown);
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

/// OPR-7b (reverses the KNR-6 rule that this test used to pin): a chunk that asks for the cache
/// keeps it. A cache written by an ordinary run is replayed by a recorded run, so the chunk does
/// not run again, produces no pieces, and is reported in `cached_units` so Rust leaves it out of
/// the expected unit count. The chunk\'s markdown (here, its output) stays in the document.
#[test]
fn a_cache_written_without_capture_is_replayed_by_a_recorded_run() {
    if !r_available("a_cache_written_without_capture_is_replayed_by_a_recorded_run") {
        return;
    }
    // Each run leaves a file behind and prints how many files it found, so a cache hit (no run)
    // shows as the old count and no new file.
    let doc = id_chunk(
        "k",
        "u1",
        "#| cache: true\n",
        "n <- length(list.files('.', pattern = '^ran_'))\nfile.create(paste0('ran_', n))\nn",
    );
    let dir = tempfile::tempdir().unwrap();
    let plain = execute_in(dir.path(), &doc, &[], false);
    assert!(plain.pieces.is_empty());
    assert!(
        dir.path().join("doc_cache").exists(),
        "the ordinary run wrote a cache"
    );
    assert!(dir.path().join("ran_0").exists());

    let recorded = execute_in(dir.path(), &doc, &[], true);
    assert!(
        !dir.path().join("ran_1").exists(),
        "the chunk hit the cache instead of running again"
    );
    assert!(recorded.pieces.is_empty(), "a cached chunk is not recorded");
    assert_eq!(recorded.cached_units, ["u1"]);
    assert_eq!(recorded.markdown, plain.markdown);
}

/// OPR-0c follow-up 3, pinned for levels 1 and 2 (OPR-7e records level 3 only): on a cache hit
/// knitr runs `opts_hooks$cache` for every chunk and `evaluate` for none of the cached ones. The
/// recorder adds nothing for a level-1 or level-2 chunk, and a hit gives the same document as
/// the miss.
#[test]
fn a_level_1_or_2_cache_hit_records_nothing_for_the_cached_chunk() {
    if !r_available("a_level_1_or_2_cache_hit_records_nothing_for_the_cached_chunk") {
        return;
    }
    let doc = id_chunk("l1", "u1", "#| cache: 1\n", "1")
        + &id_chunk("l2", "u2", "#| cache: 2\n", "2")
        + &id_chunk("live", "u4", "", "4");
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), &doc, &[], true);
    let hit = execute_in(dir.path(), &doc, &[], true);
    for (name, run) in [("miss", &miss), ("hit", &hit)] {
        assert_eq!(run.cached_units, ["u1", "u2"], "{name}");
        assert_eq!(
            unit_ids(run),
            ["u4"],
            "{name}: only the uncached chunk is recorded"
        );
        assert_eq!(
            run.pieces[0].ordinal, 1,
            "{name}: ordinals skip cached chunks"
        );
    }
    assert_eq!(miss.markdown, hit.markdown);
}

fn unit_ids(result: &KnitrExecuteResult) -> Vec<&str> {
    result
        .pieces
        .iter()
        .filter_map(|c| c.options.get("quarto-unit-id")?.as_str())
        .collect()
}

/// Every `*.q2rec.json` under `dir` (the sidecars OPR-7e writes beside knitr\'s cache entries).
fn sidecars(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.to_string_lossy().ends_with(".q2rec.json") {
                found.push(p);
            }
        }
    }
    found.sort();
    found
}

/// A record\'s comparable shape: everything but the run-local ordinal.
fn shape(result: &KnitrExecuteResult, id: &str) -> (String, String) {
    let piece = result
        .pieces
        .iter()
        .find(|c| c.options.get("quarto-unit-id").and_then(|v| v.as_str()) == Some(id))
        .unwrap_or_else(|| panic!("no record for {id}"));
    let chunk = result.chunks.iter().find(|c| c.id == id).unwrap();
    (
        format!(
            "{} {} {:?} {:?} {:?}",
            piece.label, piece.code, piece.options, piece.pieces, piece.files
        ),
        format!("{chunk:?}"),
    )
}

/// OPR-7e: a level-3 hit is recorded like the miss (the sidecar written beside the entry carries
/// the record), the ids come back in document order, and the document is unchanged.
#[test]
fn a_level_3_cache_hit_is_recorded_like_the_miss() {
    if !r_available("a_level_3_cache_hit_is_recorded_like_the_miss") {
        return;
    }
    let doc = id_chunk("a", "u1", "", "1")
        + &id_chunk("c", "u2", "#| cache: true\n", "x <- 2\nx")
        + &id_chunk("b", "u3", "", "3");
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), &doc, &[], true);
    assert!(!sidecars(dir.path()).is_empty(), "the miss wrote a sidecar");
    let hit = execute_in(dir.path(), &doc, &[], true);
    for (name, run) in [("miss", &miss), ("hit", &hit)] {
        assert!(
            run.cached_units.is_empty(),
            "{name}: {:?}",
            run.cached_units
        );
        assert_eq!(unit_ids(run), ["u1", "u2", "u3"], "{name}: document order");
        let ordinals: Vec<u32> = run.pieces.iter().map(|c| c.ordinal).collect();
        assert_eq!(ordinals, [1, 2, 3], "{name}");
    }
    assert_eq!(shape(&miss, "u2"), shape(&hit, "u2"));
    assert_eq!(miss.markdown, hit.markdown);
}

/// The same inside a blockquote and a list item: the chunk record keeps knitr\'s indent.
#[test]
fn a_level_3_cache_hit_in_a_blockquote_and_a_list_item_is_recorded() {
    if !r_available("a_level_3_cache_hit_in_a_blockquote_and_a_list_item_is_recorded") {
        return;
    }
    let doc = "> ```{r q}\n> #| quarto-unit-id: \"u1\"\n> #| cache: true\n> 1\n> ```\n\n\
               - item\n\n  ```{r l}\n  #| quarto-unit-id: \"u2\"\n  #| cache: true\n  2\n  ```\n";
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), doc, &[], true);
    let hit = execute_in(dir.path(), doc, &[], true);
    assert_eq!(unit_ids(&hit), ["u1", "u2"]);
    assert_eq!(shape(&miss, "u1"), shape(&hit, "u1"));
    assert_eq!(shape(&miss, "u2"), shape(&hit, "u2"));
    assert_eq!(miss.markdown, hit.markdown);
}

/// A hit with capture off (the sidecar is neither read nor written) gives an uncached render.
#[test]
fn a_level_3_cache_hit_with_capture_off_equals_an_uncached_render() {
    if !r_available("a_level_3_cache_hit_with_capture_off_equals_an_uncached_render") {
        return;
    }
    let cached = id_chunk("c", "u1", "#| cache: true\n", "1 + 1");
    let uncached = id_chunk("c", "u1", "", "1 + 1");
    let dir = tempfile::tempdir().unwrap();
    let _ = execute_in(dir.path(), &cached, &[], false);
    let hit = execute_in(dir.path(), &cached, &[], false);
    let (plain, _d) = execute(&uncached, false);
    assert_eq!(hit.markdown, plain.markdown);
    assert!(
        sidecars(dir.path()).is_empty(),
        "capture off writes no sidecar"
    );
}

/// knitr returns before the cache lookup for `include=FALSE`: recorded on the miss, absent on the
/// hit (and reported as an unrecorded cached unit, so Rust does not expect it).
#[test]
fn an_include_false_level_3_chunk_is_recorded_on_a_miss_and_absent_on_a_hit() {
    if !r_available("an_include_false_level_3_chunk_is_recorded_on_a_miss_and_absent_on_a_hit") {
        return;
    }
    let doc = id_chunk("h", "u1", "#| cache: true\n#| include: false\n", "y <- 3")
        + &id_chunk("v", "u2", "", "2");
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), &doc, &[], true);
    let hit = execute_in(dir.path(), &doc, &[], true);
    assert_eq!(unit_ids(&miss), ["u1", "u2"]);
    assert!(miss.cached_units.is_empty(), "{:?}", miss.cached_units);
    assert_eq!(unit_ids(&hit), ["u2"]);
    assert_eq!(hit.cached_units, ["u1"]);
}

/// `include` is not in knitr\'s cache key, and a document-level `execute: include` is not part of
/// the chunk text either: flipping it from false to true hits the old entry. The stored record
/// was made for `include: false`, so it must not come back as a hidden cell.
#[test]
fn a_level_3_hit_whose_include_changed_is_absent_not_a_hidden_cell() {
    if !r_available("a_level_3_hit_whose_include_changed_is_absent_not_a_hidden_cell") {
        return;
    }
    let doc = id_chunk("h", "u1", "#| cache: true\n", "1");
    let with_include = |include: bool| {
        let mut format = KnitrFormatConfig::with_defaults("html");
        format.execute.include = Some(include);
        format
    };
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_with_format(dir.path(), &doc, &[], true, with_include(false));
    assert_eq!(unit_ids(&miss), ["u1"], "recorded on the miss");
    let hit = execute_with_format(dir.path(), &doc, &[], true, with_include(true));
    assert!(unit_ids(&hit).is_empty(), "{:?}", unit_ids(&hit));
    assert_eq!(hit.cached_units, ["u1"]);
}

/// A sidecar that fails a check (corrupt JSON, another version, a digest that does not match the
/// stored markdown) is absent and quiet: the chunk keeps knitr\'s markdown and the run succeeds.
#[test]
fn a_sidecar_that_fails_a_check_degrades_the_chunk_to_absent() {
    if !r_available("a_sidecar_that_fails_a_check_degrades_the_chunk_to_absent") {
        return;
    }
    let doc = id_chunk("c", "u1", "#| cache: true\n", "1 + 1") + &id_chunk("l", "u2", "", "2");
    type Tamper = fn(&mut serde_json::Value) -> Option<String>;
    let tampers: [(&str, Tamper); 4] = [
        ("corrupt json", |_| Some("{ not json".to_string())),
        ("wrong version", |v| {
            v["version"] = serde_json::json!(999);
            None
        }),
        ("wrong digest", |v| {
            v["digest"] = serde_json::json!("0000");
            None
        }),
        ("wrong id", |v| {
            v["record"]["options"]["quarto-unit-id"] = serde_json::json!("u9");
            v["chunk"]["id"] = serde_json::json!("u9");
            None
        }),
    ];
    for (what, tamper) in tampers {
        let dir = tempfile::tempdir().unwrap();
        let miss = execute_in(dir.path(), &doc, &[], true);
        let files = sidecars(dir.path());
        assert_eq!(files.len(), 1, "{what}");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&files[0]).unwrap()).unwrap();
        let text = tamper(&mut v).unwrap_or_else(|| v.to_string());
        std::fs::write(&files[0], text).unwrap();
        let hit = execute_in(dir.path(), &doc, &[], true);
        assert_eq!(unit_ids(&hit), ["u2"], "{what}");
        assert_eq!(hit.cached_units, ["u1"], "{what}");
        assert_eq!(hit.markdown, miss.markdown, "{what}");
    }
}

// ---- OPR-7e follow-up (S5): the cache options and placements the 7e tests did not reach, each a
// miss then a hit in fresh R processes (every `call_r` is its own `Rscript`).

/// What a hit did to each unit the miss recorded: `Some(true)` recorded and equal to the miss,
/// `Some(false)` recorded and different (a bug), `None` absent. Also checks the two invariants that
/// hold for every case: the document is identical, and an absent unit is named in `cached_units`.
fn hit_vs_miss(
    miss: &KnitrExecuteResult,
    hit: &KnitrExecuteResult,
    what: &str,
) -> Vec<(String, Option<bool>)> {
    assert_eq!(miss.markdown, hit.markdown, "{what}: the document differs");
    let hit_ids = unit_ids(hit);
    let mut out = Vec::new();
    for id in unit_ids(miss) {
        if hit_ids.contains(&id) {
            out.push((id.to_string(), Some(shape(miss, id) == shape(hit, id))));
        } else {
            assert!(
                hit.cached_units.iter().any(|u| u == id),
                "{what}: {id} is neither recorded nor named in cached_units: {:?}",
                hit.cached_units
            );
            out.push((id.to_string(), None));
        }
    }
    for (id, same) in &out {
        assert_ne!(*same, Some(false), "{what}: the record of {id} differs");
    }
    out
}

fn miss_then_hit(
    doc: &str,
    files: &[(&str, &str)],
) -> (KnitrExecuteResult, KnitrExecuteResult, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), doc, files, true);
    let hit = execute_in(dir.path(), doc, files, true);
    (miss, hit, dir)
}

#[test]
fn s5_dependson_hit_is_recorded_like_the_miss() {
    if !r_available("s5_dependson_hit_is_recorded_like_the_miss") {
        return;
    }
    let doc = id_chunk("a", "u1", "#| cache: true\n", "x <- 1\nx")
        + &id_chunk(
            "b",
            "u2",
            "#| cache: true\n#| dependson: a\n",
            "y <- x + 1\ny",
        );
    let (miss, hit, _d) = miss_then_hit(&doc, &[]);
    let r = hit_vs_miss(&miss, &hit, "dependson");
    assert_eq!(r, [("u1".into(), Some(true)), ("u2".into(), Some(true))]);
}

#[test]
fn s5_autodep_hit_is_recorded_like_the_miss() {
    if !r_available("s5_autodep_hit_is_recorded_like_the_miss") {
        return;
    }
    let doc = id_chunk("a", "u1", "#| cache: true\n#| autodep: true\n", "x <- 1\nx")
        + &id_chunk(
            "b",
            "u2",
            "#| cache: true\n#| autodep: true\n",
            "y <- x + 1\ny",
        );
    let (miss, hit, _d) = miss_then_hit(&doc, &[]);
    let r = hit_vs_miss(&miss, &hit, "autodep");
    assert_eq!(r, [("u1".into(), Some(true)), ("u2".into(), Some(true))]);
}

#[test]
fn s5_cache_lazy_false_hit_is_recorded_like_the_miss() {
    if !r_available("s5_cache_lazy_false_hit_is_recorded_like_the_miss") {
        return;
    }
    let doc = id_chunk(
        "a",
        "u1",
        "#| cache: true\n#| cache.lazy: false\n",
        "x <- 1\nx",
    ) + &id_chunk("b", "u2", "", "x + 1");
    let (miss, hit, _d) = miss_then_hit(&doc, &[]);
    let r = hit_vs_miss(&miss, &hit, "cache.lazy");
    assert_eq!(r, [("u1".into(), Some(true)), ("u2".into(), Some(true))]);
}

/// `cache.rebuild: true` makes knitr re-run the chunk on every render, so it is a live (miss)
/// record on the second run too, and it rewrites its sidecar.
#[test]
fn s5_cache_rebuild_runs_again_and_is_recorded_live() {
    if !r_available("s5_cache_rebuild_runs_again_and_is_recorded_live") {
        return;
    }
    let doc = id_chunk(
        "a",
        "u1",
        "#| cache: true\n#| cache.rebuild: true\n",
        "n <- length(list.files('.', pattern = '^ran_'))\nfile.create(paste0('ran_', n))\nn",
    );
    let dir = tempfile::tempdir().unwrap();
    let _miss = execute_in(dir.path(), &doc, &[], true);
    let again = execute_in(dir.path(), &doc, &[], true);
    assert!(dir.path().join("ran_1").exists(), "the chunk ran again");
    assert_eq!(unit_ids(&again), ["u1"]);
    assert!(again.cached_units.is_empty());
}

#[test]
fn s5_cached_chunk_in_a_callout_is_recorded_like_the_miss() {
    if !r_available("s5_cached_chunk_in_a_callout_is_recorded_like_the_miss") {
        return;
    }
    let doc = format!(
        "::: {{.callout-note}}\n\n{}:::\n\n",
        id_chunk("c", "u1", "#| cache: true\n", "1 + 1")
    );
    let (miss, hit, _d) = miss_then_hit(&doc, &[]);
    let r = hit_vs_miss(&miss, &hit, "callout");
    assert_eq!(r, [("u1".into(), Some(true))]);
}

/// A cached chunk whose `child=` document holds a cached chunk. The child chunk record holds the
/// child\'s markdown; its inner chunk has no id, so it is never a unit of its own.
#[test]
fn s5_child_chunk_hit_is_recorded_like_the_miss_or_absent() {
    if !r_available("s5_child_chunk_hit_is_recorded_like_the_miss_or_absent") {
        return;
    }
    let doc = "```{r child_chunk, child='child.Rmd'}\n#| quarto-unit-id: \"u1\"\n#| cache: true\n```\n\n\
               ```{r after}\n#| quarto-unit-id: \"u2\"\n2\n```\n";
    let child = (
        "child.Rmd",
        "Child text.\n\n```{r inner}\n#| cache: true\n1 + 1\n```\n",
    );
    let (miss, hit, _d) = miss_then_hit(doc, &[child]);
    let r = hit_vs_miss(&miss, &hit, "child");
    assert_eq!(r[1], ("u2".into(), Some(true)));
}

/// `<doc>_files` (figures) deleted while the cache is kept.
#[test]
fn s5_files_dir_deleted_with_the_cache_kept() {
    if !r_available("s5_files_dir_deleted_with_the_cache_kept") {
        return;
    }
    let doc = id_chunk("p", "u1", "#| cache: true\n", "plot(1)") + &id_chunk("q", "u2", "", "2");
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_in(dir.path(), &doc, &[], true);
    let files_dir: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.to_string_lossy().ends_with("_files"))
        .collect();
    assert_eq!(files_dir.len(), 1, "{files_dir:?}");
    for d in &files_dir {
        std::fs::remove_dir_all(d).unwrap();
    }
    let hit = execute_in(dir.path(), &doc, &[], true);
    let r = hit_vs_miss(&miss, &hit, "_files deleted");
    // knitr does not regenerate a figure on a hit, so the file stays missing, exactly as in an
    // unrecorded run (the render then warns Q-5-6 and goes on). The record is still the miss\'s.
    assert!(!dir.path().join("doc_files/figure-html/p-1.png").exists());
    assert_eq!(r, [("u1".into(), Some(true)), ("u2".into(), Some(true))]);
}

/// [`execute_in`] for a capture run that carries a nonce, as the engine\'s own path does: R puts
/// `<!-- q2:NONCE:ID -->` in the document where a recorded unit goes.
fn execute_with_nonce(path: &std::path::Path, markdown: &str, nonce: &str) -> KnitrExecuteResult {
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
        capture: true,
        nonce: Some(nonce.to_string()),
    };
    call_r("execute", &params, path, path, &CallROptions::quiet()).expect("R execution failed")
}

/// OPR-7e follow-up: on a level-3 hit with capture on, the skeleton marker in the document
/// carries THIS run\'s nonce, not the nonce the entry was written under, and keeps knitr\'s indent
/// (a blockquote prefix). A stale nonce would make the skeleton reader treat the unit as missing.
#[test]
fn a_level_3_hit_puts_this_runs_nonce_in_the_skeleton_marker() {
    if !r_available("a_level_3_hit_puts_this_runs_nonce_in_the_skeleton_marker") {
        return;
    }
    let doc = id_chunk("c", "u1", "#| cache: true\n", "1 + 1")
        + "> ```{r q}\n> #| quarto-unit-id: \"u2\"\n> #| cache: true\n> 2\n> ```\n";
    let dir = tempfile::tempdir().unwrap();
    let miss = execute_with_nonce(dir.path(), &doc, "nonceA");
    for id in ["u1", "u2"] {
        let marker = format!("<!-- q2:nonceA:{id} -->");
        assert!(miss.markdown.contains(&marker), "miss: {}", miss.markdown);
    }
    let hit = execute_with_nonce(dir.path(), &doc, "nonceB");
    assert_eq!(unit_ids(&hit), ["u1", "u2"], "both are recorded on the hit");
    assert!(
        !hit.markdown.contains("nonceA"),
        "the old nonce leaked into the hit: {}",
        hit.markdown
    );
    assert!(
        hit.markdown.contains("<!-- q2:nonceB:u1 -->"),
        "{}",
        hit.markdown
    );
    assert!(
        hit.markdown.contains("> <!-- q2:nonceB:u2 -->"),
        "the blockquote prefix is kept: {}",
        hit.markdown
    );
}
