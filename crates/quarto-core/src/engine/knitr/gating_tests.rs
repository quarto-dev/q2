/*
 * engine/knitr/gating_tests.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-3 slice 3: `KnitrEngine::execute` asks R for pieces only on a structured route, and a
//! fallback route says so once. These drive the engine itself with real R and knitr (a missing
//! R is a skip, a signal about the machine).

use std::path::PathBuf;
use std::process::Command;

use super::KnitrEngine;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::traits::ExecutionEngine;
use crate::engine::units::scan_units;
use crate::record::{AuthorTexts, QUARTO_MARKDOWN_MIME, fill_ids};

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

const DOC: &str = "```{r a}\nx <- 1:3\nx\n```\n\n```{r b}\n#| echo: false\n1 + 1\n```\n";

/// Run the engine on `DOC` for `format`, wanting a record.
fn run(format: &str, want_record: bool) -> (ExecuteResult, tempfile::TempDir) {
    run_doc(DOC, format, want_record)
}

fn run_doc(doc: &str, format: &str, want_record: bool) -> (ExecuteResult, tempfile::TempDir) {
    run_doc_with_files(doc, &[], format, want_record)
}

fn run_doc_with_files(
    doc: &str,
    files: &[(&str, &str)],
    format: &str,
    want_record: bool,
) -> (ExecuteResult, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let qmd = dir.path().join("doc.qmd");
    std::fs::write(&qmd, doc).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut units = scan_units(doc);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let ctx = ExecutionContext::new(
        dir.path().to_path_buf(),
        PathBuf::from(dir.path()),
        qmd,
        format,
    )
    .with_owned_languages(vec!["r".to_string(), "bash".to_string()])
    .with_units(units)
    .with_author_texts(texts)
    .with_want_record(want_record)
    .with_quiet(true);
    let result = KnitrEngine::new().execute(doc, &ctx).expect("execute");
    (result, dir)
}

fn is_structured(cell: &crate::record::RecordCell) -> bool {
    cell.metadata.extra.contains_key("quarto_resolved_options")
}

fn has_markdown_output(cell: &crate::record::RecordCell) -> bool {
    cell.outputs
        .iter()
        .any(|o| o.to_string().contains(QUARTO_MARKDOWN_MIME))
}

#[test]
fn a_structured_target_captures_structured_cells() {
    if !r_available("a_structured_target_captures_structured_cells") {
        return;
    }
    let (result, _dir) = run("html", true);
    let record = result.record.expect("a record");
    let code_cells: Vec<_> = record.cells.iter().collect();
    assert_eq!(code_cells.len(), 2);
    assert!(code_cells.iter().all(|c| is_structured(c)));
    assert!(code_cells.iter().all(|c| !has_markdown_output(c)));
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn a_fallback_target_writes_fallback_cells_and_says_so_once() {
    if !r_available("a_fallback_target_writes_fallback_cells_and_says_so_once") {
        return;
    }
    let (result, _dir) = run("pdf", true);
    let record = result.record.expect("a record");
    assert_eq!(record.cells.len(), 2);
    assert!(record.cells.iter().all(|c| !is_structured(c)));
    assert!(record.cells.iter().all(has_markdown_output));
    let notes: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| w.to_text(None).contains("LaTeX"))
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", result.warnings);
}

#[test]
fn no_record_wanted_means_no_capture_and_no_diagnostic() {
    if !r_available("no_record_wanted_means_no_capture_and_no_diagnostic") {
        return;
    }
    let (result, _dir) = run("pdf", false);
    assert!(result.record.is_none());
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

fn with_code<'a>(
    result: &'a ExecuteResult,
    code: &str,
) -> Vec<&'a quarto_error_reporting::DiagnosticMessage> {
    result
        .warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some(code))
        .collect()
}

#[test]
fn an_overridden_hook_makes_that_chunk_and_every_later_one_a_fallback_cell() {
    if !r_available("an_overridden_hook_makes_that_chunk_and_every_later_one_a_fallback_cell") {
        return;
    }
    let doc = "```{r before}\n1\n```\n\n\
               ```{r override}\n#| include: false\nknitr::knit_hooks$set(output = function(x, options) paste0('X', x))\n```\n\n\
               ```{r after}\n2\n```\n";
    let (result, _dir) = run_doc(doc, "html", true);
    let record = result.record.clone().expect("a record");
    let flags: Vec<bool> = record.cells.iter().map(is_structured).collect();
    assert_eq!(flags, [true, false, false]);
    let notes = with_code(&result, "Q-18-13");
    assert_eq!(notes.len(), 1, "{:?}", result.warnings);
    let text = notes[0].to_text(None);
    assert!(
        text.contains("output") && text.contains("override"),
        "{text}"
    );
}

#[test]
fn a_chunk_that_asked_for_the_cache_is_named_once() {
    if !r_available("a_chunk_that_asked_for_the_cache_is_named_once") {
        return;
    }
    let doc = "```{r cached}\n#| cache: true\n1\n```\n\n```{r also}\n#| cache: true\n2\n```\n";
    let (result, _dir) = run_doc(doc, "html", true);
    let notes = with_code(&result, "Q-18-12");
    assert_eq!(notes.len(), 1, "{:?}", result.warnings);
    let text = notes[0].to_text(None);
    assert!(text.contains("cached") && text.contains("also"), "{text}");
}

#[test]
fn a_child_chunk_is_a_fallback_cell_and_the_rest_stays_structured() {
    if !r_available("a_child_chunk_is_a_fallback_cell_and_the_rest_stays_structured") {
        return;
    }
    let doc =
        "```{r before}\n1\n```\n\n```{r kid, child='child.Rmd'}\n```\n\n```{r after}\n2\n```\n";
    let child = ("child.Rmd", "Child text.\n\n```{r inner}\n1 + 1\n```\n");
    let (result, _dir) = run_doc_with_files(doc, &[child], "html", true);
    let record = result.record.clone().expect("a record, not a dropped one");
    let flags: Vec<bool> = record.cells.iter().map(is_structured).collect();
    assert_eq!(flags, [true, false, true]);
    assert!(has_markdown_output(&record.cells[1]));
    let text = record.cells[1].outputs[0].to_string();
    assert!(text.contains("Child text."), "{text}");
    let notes = with_code(&result, "Q-18-14");
    assert_eq!(notes.len(), 1, "{:?}", result.warnings);
    assert!(notes[0].to_text(None).contains("kid"));
    assert!(
        result
            .warnings
            .iter()
            .all(|w| w.title != "Engine Execution Record Dropped"),
        "{:?}",
        result.warnings
    );
}

/// One case per row of KNR-3's chunk-kinds table: the unit kind of each chunk (true =
/// structured) and the diagnostic code the case yields (`None` = no coded note).
#[test]
fn the_fallback_table() {
    if !r_available("the_fallback_table") {
        return;
    }
    const OVERRIDE: &str = "```{r a}\n1\n```\n\n```{r o}\n#| include: false\nknitr::knit_hooks$set(source = function(x, options) x)\n```\n\n```{r b}\n2\n```\n";
    const CHILD: &[(&str, &str)] = &[("child.Rmd", "Child.\n")];
    let cases: Vec<(&str, &str, &str, &[(&str, &str)], Vec<bool>, Option<&str>)> = vec![
        ("plain R", "html", DOC, &[], vec![true, true], None),
        (
            "eval=FALSE",
            "html",
            "```{r a}\n#| eval: false\n1\n```\n",
            &[],
            vec![true],
            None,
        ),
        (
            "non-R engine",
            "html",
            "```{r a}\n1\n```\n\n```{bash b}\necho hi\n```\n",
            &[],
            vec![true, false],
            None,
        ),
        (
            "child=",
            "html",
            "```{r a}\n1\n```\n\n```{r k, child='child.Rmd'}\n```\n",
            CHILD,
            vec![true, false],
            Some("Q-18-14"),
        ),
        (
            "cache asked for",
            "html",
            "```{r a}\n#| cache: true\n1\n```\n",
            &[],
            vec![true],
            Some("Q-18-12"),
        ),
        (
            "hook override",
            "html",
            OVERRIDE,
            &[],
            vec![true, false, false],
            Some("Q-18-13"),
        ),
        ("latex target", "latex", DOC, &[], vec![false, false], None),
        (
            "uncovered target",
            "pptx",
            DOC,
            &[],
            vec![false, false],
            None,
        ),
    ];
    for (name, format, doc, files, flags, code) in cases {
        let (result, _dir) = run_doc_with_files(doc, files, format, true);
        let record = result
            .record
            .clone()
            .unwrap_or_else(|| panic!("{name}: no record: {:?}", result.warnings));
        let got: Vec<bool> = record.cells.iter().map(is_structured).collect();
        assert_eq!(got, flags, "{name}");
        // No unit has both structured outputs and stored markdown.
        for cell in &record.cells {
            assert!(
                !(is_structured(cell) && has_markdown_output(cell)),
                "{name}"
            );
        }
        let codes: Vec<&str> = result
            .warnings
            .iter()
            .filter_map(|w| w.code.as_deref())
            .collect();
        assert_eq!(
            codes,
            code.into_iter().collect::<Vec<_>>(),
            "{name}: {:?}",
            result.warnings
        );
    }
}

/// A document in a project subdirectory: knitr runs with the project directory as its working
/// directory, but writes figures under the document\'s own directory, so the adapter must find
/// them there (the end-to-end check of `read_file`\'s base directory, KNR-3 slice 2 note).
#[test]
fn a_figure_in_a_project_subdirectory_is_found_and_stays_structured() {
    if !r_available("a_figure_in_a_project_subdirectory_is_found_and_stays_structured") {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let doc = "```{r fig}\nplot(1:3)\n```\n";
    let qmd = sub.join("doc.qmd");
    std::fs::write(&qmd, doc).unwrap();
    let mut units = scan_units(doc);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let ctx = ExecutionContext::new(
        temp.path().to_path_buf(),
        root.path().to_path_buf(),
        qmd,
        "html",
    )
    .with_project_dir(Some(root.path().to_path_buf()))
    .with_owned_languages(vec!["r".to_string()])
    .with_units(units)
    .with_author_texts(texts)
    .with_want_record(true)
    .with_quiet(true);
    let result = KnitrEngine::new().execute(doc, &ctx).expect("execute");
    let record = result.record.expect("a record");
    assert!(
        is_structured(&record.cells[0]),
        "{:?}",
        record.cells[0].outputs
    );
}
