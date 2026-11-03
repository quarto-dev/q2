//! KNR-3 slice 6: the option sweep.
//!
//! Every knitr `opts_chunk` default and every Quarto cell option is varied over one structured
//! knitr record, in the current document's `#|` header, and classified as the splice reports it:
//!
//! - [`Outcome::Hashed`]: the unit reads as stale, so the next run re-executes it;
//! - [`Outcome::Presents`]: the unit stays fresh and the presented markdown changes;
//! - [`Outcome::Inert`]: neither. The table says why each one is allowed to be.
//!
//! An option that is neither hashed nor presented would be a silent divergence: editing it
//! would do nothing until something else forced a re-run. The test fails on any option whose
//! outcome differs from the table, so a new option or a changed rule has to be classified.

use super::*;
use crate::engine::knitr::hash_rule::knitr_hash_definition;
use crate::engine::knitr::structured::{mark_structured, rehash_document};
use crate::presenter::knitr_inputs::{OUTPUT_KINDS_KEY, RESOLVED_OPTIONS_KEY};

/// A 1x1 transparent PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Hashed,
    Presents,
    /// Neither, and that is correct; `INERT_REASONS` says why.
    Inert,
    /// Neither, and that is a bug: live knitr output changes (the corpus shows each one), the
    /// structured reader ignores the option. Fixing one flips its row to `Presents`.
    Gap,
}
use Outcome::*;

/// What `capture_known_options` ships: knitr\'s `opts_chunk` names, then the Quarto and internal
/// groups of `capture.R` (a few internal names that no author writes are left out).
const KNOWN: &[&str] = &[
    "eval",
    "echo",
    "results",
    "tidy",
    "tidy.opts",
    "collapse",
    "prompt",
    "comment",
    "highlight",
    "size",
    "background",
    "strip.white",
    "cache",
    "cache.path",
    "cache.vars",
    "cache.lazy",
    "dependson",
    "autodep",
    "cache.rebuild",
    "fig.keep",
    "fig.show",
    "fig.align",
    "fig.path",
    "dev",
    "dev.args",
    "dpi",
    "fig.ext",
    "fig.width",
    "fig.height",
    "fig.env",
    "fig.cap",
    "fig.scap",
    "fig.lp",
    "fig.subcap",
    "fig.pos",
    "out.width",
    "out.height",
    "out.extra",
    "fig.retina",
    "external",
    "sanitize",
    "interval",
    "aniopts",
    "warning",
    "error",
    "message",
    "render",
    "ref.label",
    "child",
    "engine",
    "split",
    "include",
    "purl",
    "fig.link",
    "fig.alt",
    "fig.num",
    "label",
    "lst-cap",
    "lst-label",
    "classes",
    "panel",
    "column",
    "tbl-column",
    "tbl-cap-location",
    "cap-location",
    "code-fold",
    "code-summary",
    "code-overflow",
    "code-line-numbers",
    "layout",
    "layout-nrow",
    "layout-ncol",
    "layout-align",
    "layout-valign",
    "output",
    "html-table-processing",
    "fig-column",
    "fig.column",
    "fig-cap-location",
    "fig.cap-location",
    "fig-format",
    "fig.format",
    "fig-dpi",
    "fig.dpi",
    "fenced.echo",
    "chunk.echo",
    "indent",
    "class.source",
    "class.output",
    "class.message",
    "class.warning",
    "class.error",
    "attr.source",
    "attr.output",
    "attr.message",
    "attr.warning",
    "attr.error",
];

const BASE_HEADER: &[(&str, &str)] = &[("echo", "true")];

fn rich_record(source: &str) -> Record {
    let outputs = vec![
        json!({"output_type": "stream", "name": "stdout", "text": "[1] 1"}),
        json!({"output_type": "stream", "name": "stderr", "text": "a message"}),
        json!({"output_type": "stream", "name": "stderr", "text": "a warning"}),
        json!({"output_type": "display_data", "data": {"image/png": PNG, "text/plain": ["<fig>"]}, "metadata": {}}),
    ];
    let doc = parse(source);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let def = knitr_hash_definition();
    let mut builder = RecordBuilder::new("knitr", &def, &units, &texts).unwrap();
    builder
        .push(
            0,
            UnitResult {
                outputs,
                execution_count: None,
                kernel: None,
            },
        )
        .unwrap();
    let mut entry = builder
        .finish(EngineEntryInput {
            owned_languages: vec!["r".into()],
            ..Default::default()
        })
        .unwrap();
    // What the knitr adapter records (slice 8): the keys of the `#|` header as written.
    let header_keys: Vec<String> = crate::record::partition_author_text("r", &texts.0[0].text)
        .unwrap()
        .0
        .into_keys()
        .collect();
    for cell in &mut entry.cells {
        mark_structured(&def, cell, "", "html").unwrap();
        cell.metadata
            .extra
            .insert(crate::record::HEADER_KEYS_KEY.into(), json!(header_keys));
        cell.metadata
            .extra
            .insert(RESOLVED_OPTIONS_KEY.into(), json!({}));
        cell.metadata.extra.insert(
            OUTPUT_KINDS_KEY.into(),
            json!([null, "message", "warning", null]),
        );
    }
    entry.engine.known_options = Some(KNOWN.iter().map(|s| s.to_string()).collect());
    rehash_document(&mut entry, &Default::default()).unwrap();
    coalesce(&[entry], &units).unwrap()
}

fn source_with(option: Option<(&str, &str)>) -> String {
    let mut header: Vec<(&str, &str)> = BASE_HEADER.to_vec();
    if let Some((name, value)) = option {
        header.retain(|(n, _)| *n != name);
        header.push((name, value));
    }
    let lines: String = header
        .iter()
        .map(|(n, v)| format!("#| {n}: {v}\n"))
        .collect();
    format!("```{{r}}\n{lines}1\n```\n")
}

/// Splice with figures written under a temporary directory, so the figure presents as an image.
fn splice_html(source: &str, record: &Record) -> (String, SpliceReport) {
    let dir = tempfile::tempdir().unwrap();
    let mut c = ctx();
    let (doc, report) = splice_record(parse(source), record, &mut c, dir.path());
    (text_of(&doc), report)
}

fn classify(name: &str, value: &str) -> Outcome {
    let record = rich_record(&source_with(None));
    let (base, base_report) = splice_html(&source_with(None), &record);
    assert_eq!(base_report.units[0].1, UnitStaleness::Fresh, "{base}");
    let (text, report) = splice_html(&source_with(Some((name, value))), &record);
    if report.units[0].1 == UnitStaleness::Stale {
        Hashed
    } else if text != base {
        Presents
    } else {
        Inert
    }
}

/// (option, a YAML value that differs from the default, the outcome it must have).
const SWEEP: &[(&str, &str, Outcome)] = &[
    ("eval", "false", Hashed),
    ("echo", "false", Presents),
    ("results", "hide", Presents),
    ("tidy", "true", Hashed),
    ("tidy.opts", "\"list(width.cutoff = 40)\"", Hashed),
    ("collapse", "true", Presents),
    ("prompt", "true", Presents),
    ("comment", "\"'#>'\"", Presents),
    ("highlight", "false", Inert),
    ("size", "small", Inert),
    ("background", "\"'#fff'\"", Inert),
    ("strip.white", "false", Inert),
    ("cache", "true", Hashed),
    ("cache.path", "\"'c/'\"", Hashed),
    ("cache.vars", "\"'x'\"", Hashed),
    ("cache.lazy", "false", Hashed),
    ("dependson", "\"'a'\"", Hashed),
    ("autodep", "true", Hashed),
    ("cache.rebuild", "true", Hashed),
    ("fig.keep", "none", Hashed),
    ("fig.show", "hide", Presents),
    ("fig.align", "center", Gap),
    ("fig.path", "\"'f/'\"", Hashed),
    ("dev", "svg", Hashed),
    ("dev.args", "\"list(bg = 'red')\"", Hashed),
    ("dpi", "150", Hashed),
    ("fig.ext", "\"'svg'\"", Hashed),
    ("fig.width", "3", Hashed),
    ("fig.height", "3", Hashed),
    ("fig.env", "\"'figure*'\"", Gap),
    ("fig.cap", "\"'A caption'\"", Presents),
    ("fig.scap", "\"'short'\"", Gap),
    ("fig.lp", "\"'x:'\"", Inert),
    ("fig.subcap", "\"'sub'\"", Presents),
    ("fig.pos", "\"'h'\"", Gap),
    ("out.width", "\"'50%'\"", Gap),
    ("out.height", "\"'50%'\"", Gap),
    ("out.extra", "\"'style=\\\"x\\\"'\"", Gap),
    ("fig.retina", "2", Hashed),
    ("external", "false", Hashed),
    ("sanitize", "true", Hashed),
    ("interval", "2", Hashed),
    ("aniopts", "\"'loop'\"", Hashed),
    ("warning", "false", Hashed),
    ("error", "false", Hashed),
    ("message", "false", Hashed),
    ("render", "\"'x'\"", Hashed),
    ("split", "true", Hashed),
    ("include", "false", Hashed),
    ("purl", "false", Hashed),
    ("fenced.echo", "true", Presents),
    ("chunk.echo", "false", Inert),
    ("fig.alt", "\"'alt text'\"", Presents),
    ("class.source", "foo", Presents),
    ("class.output", "foo", Presents),
    ("class.message", "foo", Presents),
    ("class.warning", "foo", Presents),
    ("class.error", "foo", Inert),
    ("attr.source", "\"'.n'\"", Presents),
    ("attr.output", "\"'.n'\"", Presents),
    ("output", "false", Hashed),
    ("classes", "foo", Gap),
    ("panel", "tabset", Presents),
    ("column", "margin", Presents),
    ("fig-column", "margin", Gap),
    ("tbl-column", "margin", Gap),
    ("cap-location", "top", Gap),
    ("fig-cap-location", "top", Gap),
    ("tbl-cap-location", "top", Gap),
    ("code-fold", "true", Presents),
    ("code-summary", "\"'Show'\"", Presents),
    ("code-overflow", "wrap", Gap),
    ("code-line-numbers", "true", Presents),
    ("lst-cap", "\"'Listing'\"", Presents),
    ("lst-label", "lst-a", Presents),
    ("layout", "\"[[1]]\"", Presents),
    ("layout-nrow", "2", Presents),
    ("layout-ncol", "2", Presents),
    ("layout-align", "center", Presents),
    ("layout-valign", "top", Presents),
    ("keep-hidden", "true", Presents),
    ("html-table-processing", "none", Inert),
    ("fig-width", "3", Hashed),
    ("fig-height", "3", Hashed),
    ("fig-align", "center", Gap),
    ("fig-alt", "\"'a'\"", Presents),
    ("fig-cap", "\"'cap'\"", Presents),
    ("label", "abc", Hashed),
    ("fig-format", "svg", Hashed),
    ("fig-dpi", "150", Hashed),
    ("dpi", "150", Hashed),
    ("indent", "\"'  '\"", Gap),
    ("attr.message", "\"'.n'\"", Presents),
    ("attr.warning", "\"'.n'\"", Presents),
    ("attr.error", "\"'.n'\"", Inert),
    ("fig.link", "\"'http://x'\"", Gap),
];

const INERT_REASONS: &[(&str, &str)] = &[
    (
        "attr.error",
        "the fixture has no error output; attr.message and attr.warning share the path and present",
    ),
    (
        "background",
        "latex-only; live html output is unchanged (corpus options/background)",
    ),
    (
        "highlight",
        "live html output is unchanged (corpus options/highlight)",
    ),
    (
        "size",
        "latex-only; live html output is unchanged (corpus options/size)",
    ),
    (
        "strip.white",
        "applied by knitr when it runs; the corpus body has no blank lines, so the corpus cannot tell (see the slice 6 notes)",
    ),
    (
        "fig.lp",
        "latex-only; live html output is unchanged (corpus options/fig.lp)",
    ),
    (
        "chunk.echo",
        "live html output is unchanged (corpus options/chunk.echo)",
    ),
    (
        "html-table-processing",
        "live html output is unchanged (corpus quarto-cell-options/html-table-processing)",
    ),
    (
        "class.error",
        "the fixture has no error output; corpus options/class.error covers it",
    ),
];

#[test]
fn every_option_is_hashed_or_presented() {
    let mut wrong = Vec::new();
    for (name, value, want) in SWEEP {
        let got = classify(name, value);
        // A gap is inert today: it flips to `Presents` when the reader learns the option.
        let got = if *want == Gap && got == Inert {
            Gap
        } else {
            got
        };
        if got != *want {
            wrong.push(format!("{name}: {got:?}, table says {want:?}"));
        }
        if *want == Inert {
            assert!(
                INERT_REASONS.iter().any(|(n, _)| n == name),
                "{name} is inert with no reason in INERT_REASONS"
            );
        }
    }
    assert!(wrong.is_empty(), "outcomes changed:\n{}", wrong.join("\n"));
}

/// Every option the hash declaration treats as presentation-only appears in the sweep, so
/// adding one to the list forces a decision about how it is presented.
#[test]
fn every_presentation_only_option_is_in_the_sweep() {
    let swept: Vec<&str> = SWEEP.iter().map(|(n, _, _)| *n).collect();
    let missing: Vec<&str> = crate::engine::knitr::hash_rule::STRUCTURED_PRESENTATION_OPTIONS
        .iter()
        .copied()
        .filter(|o| !swept.contains(o) && !swept.contains(&o.replace('.', "-").as_str()))
        .collect();
    assert!(missing.is_empty(), "not swept: {missing:?}");
}

/// KNR-3 slice 8: deleting any swept option from a header that had it makes the unit stale, so
/// the presenter is never asked to show a value the author removed. A fresh run re-derives the
/// options that depend on it (`results` and `fig.show` under `echo: fenced`, `comment` under
/// `collapse`), which restoring a recorded default could not.
#[test]
fn deleting_any_swept_option_from_the_header_makes_the_unit_stale() {
    let mut survived = Vec::new();
    for (name, value, _) in SWEEP {
        let record = rich_record(&source_with(Some((name, value))));
        // The base header with that one option removed (for `echo`, an empty header).
        let lines: String = BASE_HEADER
            .iter()
            .filter(|(n, _)| n != name)
            .map(|(n, v)| format!("#| {n}: {v}\n"))
            .collect();
        let deleted = format!("```{{r}}\n{lines}1\n```\n");
        let (_, report) = splice_html(&deleted, &record);
        if report.units[0].1 != UnitStaleness::Stale {
            survived.push(*name);
        }
    }
    assert!(
        survived.is_empty(),
        "deletion left these fresh: {survived:?}"
    );
}
