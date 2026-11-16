/*
 * tests/integration/knitr_end_to_end_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * KNR-7: whole knitr documents through the real binary, checked on the final HTML.
 */

//! LOCAL EVIDENCE ONLY: CI has no R, so each test skips there (the pattern of
//! `plain_render_recorded_route_cli.rs`, whose `r_available` this copies).
//!
//! These cover the KNR-7 end-to-end items that the corpus shadow gate (which compares
//! markdown) cannot: the full pipeline output of a document that uses every feature, and a
//! document that mixes structured chunks with a fallback chunk.

use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn r_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "for (p in c('knitr', 'rmarkdown', 'jsonlite', 'htmltools')) if (!requireNamespace(p, quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Render `doc` as `doc.qmd` in a fresh directory and return the directory and the HTML.
fn render_to_html(doc: &str) -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("doc.qmd"), doc).unwrap();
    let out = Command::new(Q2_BIN)
        .current_dir(dir.path())
        .args(["render", "doc.qmd"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "q2 render failed:\n{stderr}");
    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    (dir, html)
}

/// A figure, a table, an htmlwidget-style dependency, an `error: true` chunk and inline
/// expressions in one document: every one of them reaches the final page.
const FULL_DOC: &str = r#"---
title: Everything
engine: knitr
---

```{r}
#| label: fig-cars
#| fig-cap: Cars
plot(cars)
```

```{r}
knitr::kable(head(cars, 2))
```

```{r}
htmltools::tagList(htmltools::tags$div(id = "widget-marker", "WIDGET-TEXT"))
```

```{r}
#| error: true
stop("kaboom-on-purpose")
```

The answer is `{r} 40 + 2` and also `r 1 + 1`.
"#;

#[test]
fn a_document_with_every_feature_renders_each_one_into_the_final_page() {
    if !r_available() {
        eprintln!("SKIP: R with knitr, rmarkdown, jsonlite and htmltools is not available");
        return;
    }
    let (dir, html) = render_to_html(FULL_DOC);
    assert!(html.contains("<img"), "the figure is on the page");
    assert!(html.contains("fig-cars"), "the figure keeps its label");
    assert!(html.contains("<table"), "the table is on the page");
    assert!(
        html.contains("WIDGET-TEXT"),
        "the htmltools output is on the page"
    );
    assert!(
        html.contains("kaboom-on-purpose"),
        "an `error: true` chunk shows its error and the render still succeeds"
    );
    assert!(
        html.contains("The answer is 42 and also 2"),
        "both inline spellings"
    );
    let files = dir.path().join("doc_files");
    assert!(files.exists(), "the figure file is written beside the page");
}

/// Structured chunks and a fallback chunk (a cached chunk keeps knitr's markdown) in one
/// document: all three outputs reach the page, in document order.
const MIXED_DOC: &str = r#"---
title: Mixed
engine: knitr
---

```{r}
cat("FIRST-STRUCTURED\n")
```

```{r}
#| cache: true
cat("MIDDLE-FALLBACK\n")
```

```{r}
cat("LAST-STRUCTURED\n")
```
"#;

#[test]
fn structured_and_fallback_chunks_render_together_in_document_order() {
    if !r_available() {
        eprintln!("SKIP: R with knitr, rmarkdown, jsonlite and htmltools is not available");
        return;
    }
    let (_dir, html) = render_to_html(MIXED_DOC);
    let at = |needle: &str| {
        html.rfind(needle)
            .unwrap_or_else(|| panic!("{needle} is on the page:\n{html}"))
    };
    let (first, middle, last) = (
        at("FIRST-STRUCTURED"),
        at("MIDDLE-FALLBACK"),
        at("LAST-STRUCTURED"),
    );
    assert!(
        first < middle && middle < last,
        "document order: {first} {middle} {last}"
    );
}
