/*
 * engine/knitr/chunk_record_tests.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-11 Phase 11B: the R side records each chunk that carries a
//! `quarto-unit-id` option. These drive `execute` through `call_r` with real
//! R and knitr; a missing R is a skip, a signal about the machine.

use std::process::Command;

use super::{
    CallROptions, KNITR_RESOURCES, KnitrExecuteParams, KnitrExecuteResult, KnitrFormatConfig,
    call_r,
};

fn r_available(name: &str) -> bool {
    let ok = Command::new("Rscript")
        .args([
            "-e",
            "for (p in c('knitr', 'rmarkdown')) if (!requireNamespace(p, quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("SKIP: Rscript with knitr and rmarkdown not available: {name}");
    }
    ok
}

fn execute(markdown: &str) -> KnitrExecuteResult {
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
    };
    call_r("execute", &params, path, path, &CallROptions::quiet()).expect("R execution failed")
}

const THREE_CHUNKS: &str = "```{r}\n#| quarto-unit-id: u1\n1 + 1\n```\n\n\
```{r}\n#| quarto-unit-id: u2\n#| eval: false\nstop('never')\n```\n\n\
```{r}\n#| quarto-unit-id: u3\n#| include: false\n3 + 3\n```\n\n\
```{r}\n4 + 4\n```\n";

#[test]
fn chunk_records_one_per_chunk_with_an_id_in_order() {
    if !r_available("chunk_records_one_per_chunk_with_an_id_in_order") {
        return;
    }
    let result = execute(THREE_CHUNKS);
    let ids: Vec<&str> = result.chunks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["u1", "u2", "u3"], "the id-less chunk is not recorded");
    assert!(result.chunks[0].markdown.contains("[1] 2"));
    // eval: false keeps its code block, with no output
    assert!(
        result.chunks[1].markdown.contains("stop(&#39;never&#39;)")
            || result.chunks[1].markdown.contains("stop('never')")
    );
    // include: false leaves nothing in the document, so the record holds nothing
    assert_eq!(result.chunks[2].markdown, "");
    assert!(!result.markdown.contains("[1] 6"));
}

#[test]
fn chunk_records_markdown_is_what_lands_in_the_document() {
    if !r_available("chunk_records_markdown_is_what_lands_in_the_document") {
        return;
    }
    let result = execute(THREE_CHUNKS);
    for c in result.chunks.iter().filter(|c| !c.markdown.is_empty()) {
        assert!(
            result.markdown.contains(&c.markdown),
            "record for {} is not a substring of the document:\n{}",
            c.id,
            c.markdown
        );
    }
}

#[test]
fn chunk_records_digest_tracks_options_not_code() {
    if !r_available("chunk_records_digest_tracks_options_not_code") {
        return;
    }
    let chunk =
        |opts: &str, code: &str| format!("```{{r}}\n#| quarto-unit-id: d\n{opts}{code}\n```\n");
    let base = execute(&chunk("#| echo: false\n", "1 + 1")).chunks[0]
        .options_digest
        .clone();
    let again = execute(&chunk("#| echo: false\n", "1 + 1")).chunks[0]
        .options_digest
        .clone();
    let other_code = execute(&chunk("#| echo: false\n", "2 + 2")).chunks[0]
        .options_digest
        .clone();
    let other_option = execute(&chunk("#| echo: true\n", "1 + 1")).chunks[0]
        .options_digest
        .clone();
    assert_eq!(base, again, "digest is stable across runs");
    assert_eq!(base, other_code, "code is not part of the options digest");
    assert_ne!(base, other_option, "a changed option changes the digest");
}

#[test]
fn chunk_records_attribute_figure_files_to_their_chunk() {
    if !r_available("chunk_records_attribute_figure_files_to_their_chunk") {
        return;
    }
    let result = execute(
        "```{r}\n#| quarto-unit-id: f1\nplot(1)\n```\n\n\
```{r}\n#| quarto-unit-id: f2\n#| fig.show: hold\nplot(2); plot(3)\n```\n\n\
```{r}\n#| quarto-unit-id: f3\n1\n```\n",
    );
    let files: Vec<usize> = result.chunks.iter().map(|c| c.files.len()).collect();
    assert_eq!(files, [1, 2, 0], "files per chunk: {:?}", result.chunks);
    // no file is credited to two chunks
    let mut all: Vec<&String> = result.chunks.iter().flat_map(|c| &c.files).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 3);
}

/// Place each record into the document in order, between the prose that
/// surrounds the chunks, and return the rebuilt text. `segments` are the
/// source's prose pieces: one more than there are records.
fn place(segments: &[&str], records: &[crate::engine::knitr::types::KnitrChunkRecord]) -> String {
    assert_eq!(segments.len(), records.len() + 1);
    let mut out = String::from(segments[0]);
    for (rec, seg) in records.iter().zip(&segments[1..]) {
        out.push_str(&rec.markdown);
        out.push_str(seg);
    }
    out
}

#[test]
fn chunk_records_shadow_equality() {
    if !r_available("chunk_records_shadow_equality") {
        return;
    }
    // Fixtures echo `knitr_label_reinject.rs` (a labeled figure with a
    // caption), `knitr_display_fence.rs` (a plain cell next to a wide fence),
    // and an `include: false` chunk in the middle of prose.
    let cases: [(&str, [&str; 2], &str); 3] = [
        (
            "```{r}\n#| quarto-unit-id: a\n#| label: fig-cars\n#| echo: true\n#| fig-cap: \"Cars plot\"\nplot(cars)\n```\n",
            ["", ""],
            "labeled figure",
        ),
        (
            "Before.\n\n```{r}\n#| quarto-unit-id: a\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\nAfter.\n",
            ["Before.\n\n", "\n\nAfter.\n"],
            "plain cell between prose",
        ),
        (
            "Top.\n\n```{r}\n#| quarto-unit-id: a\n#| include: false\n1 + 1\n```\n\nBottom.\n",
            ["Top.\n\n", "\n\nBottom.\n"],
            "include: false",
        ),
    ];
    for (source, segments, what) in cases {
        let result = execute(source);
        assert_eq!(result.chunks.len(), 1, "{what}: one record");
        let placed = place(&segments, &result.chunks);
        assert_eq!(
            placed.trim_end(),
            result.markdown.trim_end(),
            "{what}: placing the record must reproduce result.markdown"
        );
    }
}

#[test]
fn chunk_records_shadow_equality_with_several_chunks() {
    if !r_available("chunk_records_shadow_equality_with_several_chunks") {
        return;
    }
    let result = execute(THREE_CHUNKS);
    let placed = place(&["", "\n", "\n", "\n"], &result.chunks);
    // The id-less fourth chunk is not a record, so the placed records are
    // the document up to it. knitr's blank-line spacing between chunks is
    // not part of a record, hence the whitespace-insensitive comparison.
    let flat = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat(&result.markdown).starts_with(&flat(&placed)),
        "placed records are a prefix of the document:\n--- placed\n{placed}\n--- markdown\n{}",
        result.markdown
    );
}

#[test]
fn chunk_records_leave_notebook_level_meta_unchanged() {
    if !r_available("chunk_records_leave_notebook_level_meta_unchanged") {
        return;
    }
    // An htmlDependency attached to a widget-like value: engine dependencies
    // and post-processing belong to the notebook, not to the chunk record.
    let chunk = |id_line: &str| {
        format!(
            "```{{r}}\n{id_line}htmltools::attachDependencies(\n  htmltools::div(\"hi\"),\n  htmltools::htmlDependency(\"q2dep\", \"1.0\", src = tempdir(), script = \"q2dep.js\")\n)\n```\n"
        )
    };
    let with_id = execute(&chunk("#| quarto-unit-id: w\n"));
    let without_id = execute(&chunk(""));
    assert_eq!(with_id.chunks.len(), 1);
    assert!(without_id.chunks.is_empty());
    assert_eq!(with_id.post_process, without_id.post_process);
    assert_eq!(with_id.preserve, without_id.preserve);
    assert_eq!(with_id.engine_dependencies, without_id.engine_dependencies);
    assert_eq!(with_id.markdown, without_id.markdown);
    assert!(
        !with_id.chunks[0].markdown.contains("<script")
            && !with_id.chunks[0].markdown.contains("<link"),
        "dependencies are not in the chunk record: {:?}",
        with_id.chunks[0].markdown
    );
}
