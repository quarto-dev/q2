//! The user docs' inline-expression examples, run as written (EXR-18).
//!
//! Each test pulls a `markdown` sample out of the docs page itself, so the text it renders
//! is the text a reader copies. A sample that changes in the page changes here; one that
//! moves or disappears fails with the heading and index it was looked up by.
//!
//! Samples that only *show* Quarto syntax (the `markdown` display blocks, error-code
//! illustrations) are deliberately not run. Samples that assume a value defined elsewhere
//! (`release_version`, `notice`) get a fixed setup cell *prepended*; the sample text is not
//! edited.
//!
//! Each test skips with a note when its engine is not installed.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// A fenced block of a docs page.
struct Fenced {
    /// The text of the nearest heading above the block (outside any fence).
    heading: String,
    /// The info string after the opening fence.
    info: String,
    /// The lines between the fences, each ending in `\n`.
    body: String,
}

/// Every fenced block of `page`, outermost fences only. A fence closes on a run of at least
/// as many backticks as opened it, which is how a longer outer fence holds a shorter inner one.
fn fenced_blocks(page: &str) -> Vec<Fenced> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut open: Option<(usize, String, String)> = None;
    for line in page.lines() {
        let ticks = line.chars().take_while(|c| *c == '`').count();
        match &mut open {
            None => {
                if ticks >= 3 {
                    open = Some((ticks, line[ticks..].trim().to_string(), String::new()));
                } else if let Some(h) = line.strip_prefix('#') {
                    heading = h.trim_start_matches('#').trim().to_string();
                }
            }
            Some((n, info, body)) => {
                if ticks >= *n && line[ticks..].trim().is_empty() {
                    out.push(Fenced {
                        heading: heading.clone(),
                        info: std::mem::take(info),
                        body: std::mem::take(body),
                    });
                    open = None;
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    out
}

fn docs_page(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/guides/authoring")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The `n`th `markdown` block under the heading `heading` on the computations page, which
/// must contain `anchor` (a guard that the lookup found the sample the test means).
fn sample(heading: &str, n: usize, anchor: &str) -> String {
    let page = docs_page("computations.qmd");
    let found: Vec<Fenced> = fenced_blocks(&page)
        .into_iter()
        .filter(|b| b.info == "markdown" && b.heading == heading)
        .collect();
    let block = found.get(n).unwrap_or_else(|| {
        panic!(
            "docs/guides/authoring/computations.qmd has {} markdown blocks under `{heading}`; wanted index {n}",
            found.len()
        )
    });
    assert!(
        block.body.contains(anchor),
        "markdown block {n} under `{heading}` no longer contains {anchor:?}:\n{}",
        block.body
    );
    block.body.clone()
}

fn engine_available(name: &str) -> bool {
    EngineRegistry::default()
        .get(name)
        .is_some_and(|e| e.is_available())
}

/// Run the real engine over `content` (the producer path `q2 preview` uses; see
/// capture_splice_engines.rs for why pollster) and return the post-engine markdown.
fn post_engine_markdown(content: &str, engine: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, content).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let captures =
        pollster::block_on(record_capture(&path, &project, runtime, None)).expect("record_capture");
    assert_eq!(captures.len(), 1, "expected one capture for {engine}");
    assert_eq!(captures[0].engine_name, engine);
    captures[0]
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// A knitr document: a setup chunk, then the docs sample unchanged.
fn with_r_setup(setup: &str, sample: &str) -> String {
    format!("```{{r}}\n{setup}\n```\n\n{sample}")
}

#[test]
fn extractor_handles_nested_fences_and_headings() {
    let page = "# Top\n\n## Sub\n\n````markdown\n```{r}\n1\n```\n````\n\n```\nplain\n```\n";
    let blocks = fenced_blocks(page);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].heading, "Sub");
    assert_eq!(blocks[0].info, "markdown");
    assert_eq!(blocks[0].body, "```{r}\n1\n```\n");
    assert_eq!(blocks[1].info, "");
}

#[test]
fn docs_knitr_brace_spelling() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr is not installed");
        return;
    }
    let s = sample("Inline expressions", 0, "`{r} release_version`");
    let md = post_engine_markdown(&with_r_setup("release_version <- \"alpha\"", &s), "knitr");
    assert!(md.contains("The current release is alpha."), "{md}");
}

#[test]
fn docs_knitr_classic_spelling() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr is not installed");
        return;
    }
    let s = sample("Inline expressions", 1, "`r release_version`");
    let md = post_engine_markdown(&with_r_setup("release_version <- \"alpha\"", &s), "knitr");
    assert!(md.contains("The current release is alpha."), "{md}");
}

#[test]
fn docs_knitr_i_opts_into_markdown() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr is not installed");
        return;
    }
    let s = sample(
        "The two spellings insert the value differently",
        0,
        "`{r} I(notice)`",
    );
    let md = post_engine_markdown(&with_r_setup("notice <- \"**important**\"", &s), "knitr");
    assert!(md.contains("The notice is **important**."), "{md}");
}

#[test]
fn docs_knitr_attribute_and_link_title() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr is not installed");
        return;
    }
    let s = sample("Where inline expressions work", 0, "data-version=");
    let md = post_engine_markdown(&with_r_setup("release_version <- \"alpha\"", &s), "knitr");
    assert!(md.contains("data-version=\"alpha\""), "{md}");
    assert!(md.contains("\"Version alpha\""), "{md}");
}

#[test]
fn docs_jupyter_span_sees_cell_state() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter is not installed");
        return;
    }
    let s = sample("Inline expressions with Jupyter", 0, "`{python} total`");
    let md = post_engine_markdown(&s, "jupyter");
    assert!(md.contains("The answer is 42."), "{md}");
    assert!(!md.contains("{python}"), "{md}");
}

#[test]
fn docs_jupyter_markdown_display_value_is_inserted_as_markdown() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter is not installed");
        return;
    }
    let s = sample("How a value is inserted", 0, "Markdown(\"**important**\")");
    let md = post_engine_markdown(&s, "jupyter");
    assert!(md.contains("The notice is **important**."), "{md}");
}

#[test]
fn docs_jupyter_spans_only_page() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter is not installed");
        return;
    }
    let s = sample(
        "Documents that contain only inline expressions",
        0,
        "jupyter: python3",
    );
    let md = post_engine_markdown(&s, "jupyter");
    let after = md
        .split("Today is ")
        .nth(1)
        .unwrap_or_else(|| panic!("no `Today is` in:\n{md}"));
    let year: String = after.chars().take(4).collect();
    assert!(
        year.chars().all(|c| c.is_ascii_digit()) && year.starts_with("20"),
        "span not evaluated to a year: {md}"
    );
}

#[test]
fn docs_jupyter_front_matter_span() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter is not installed");
        return;
    }
    let s = sample("Where inline expressions work", 1, "datetime");
    let doc = format!("{s}\nBody.\n");
    let md = post_engine_markdown(&doc, "jupyter");
    assert!(
        !md.contains("{python}"),
        "front matter span left as written:\n{md}"
    );
    assert!(md.contains("Report for 20"), "{md}");
}
