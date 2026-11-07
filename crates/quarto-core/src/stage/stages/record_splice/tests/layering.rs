//! KNR-6 Phase 6A: setup-chunk layering through the splice.
//!
//! `knitr::opts_chunk$set(echo = FALSE)` in a setup chunk is R-side state the document does not
//! show. The record carries each chunk's *resolved* options, and the presenter layers the chunk's
//! current header over them (header wins). So:
//!
//! - a chunk with no header presents under the setup chunk's resolved `echo = FALSE`;
//! - a header edit (`#| echo: true`) shows at once from the stored record, the unit still fresh;
//! - a setup-chunk edit makes the setup unit stale but changes nothing for its neighbor, which
//!   keeps showing the old record until the next run.

use super::*;
use crate::engine::knitr::hash_rule::knitr_hash_definition;
use crate::engine::knitr::structured::{mark_structured, rehash_document};
use crate::presenter::knitr_inputs::{OUTPUT_KINDS_KEY, RESOLVED_OPTIONS_KEY};

const SETUP: &str = "```{r}\nknitr::opts_chunk$set(echo = FALSE)\n```\n\n";
const CHUNK: &str = "```{r}\nprint(1234)\n```\n";

/// A record of the setup-and-chunk document as R ran it: the chunk's resolved `echo` is false.
fn setup_record(source: &str) -> Record {
    let doc = parse(source);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    assert_eq!(units.len(), 2, "{qmd}");
    let def = knitr_hash_definition();
    let mut builder = RecordBuilder::new("knitr", &def, &units, &texts).unwrap();
    for (i, outputs) in [
        vec![],
        vec![json!({"output_type": "stream", "name": "stdout", "text": "[1] 1234"})],
    ]
    .into_iter()
    .enumerate()
    {
        builder
            .push(
                i,
                UnitResult {
                    outputs,
                    execution_count: None,
                    kernel: None,
                },
            )
            .unwrap();
    }
    let mut entry = builder
        .finish(EngineEntryInput {
            owned_languages: vec!["r".into()],
            ..Default::default()
        })
        .unwrap();
    for (i, cell) in entry.cells.iter_mut().enumerate() {
        mark_structured(&def, cell, "", "html").unwrap();
        let header_keys: Vec<String> = crate::record::partition_author_text("r", &texts.0[i].text)
            .unwrap()
            .0
            .into_keys()
            .collect();
        cell.metadata
            .extra
            .insert(crate::record::HEADER_KEYS_KEY.into(), json!(header_keys));
        // The setup chunk resolves nothing; the chunk after it resolves `echo = FALSE`.
        let resolved = if i == 0 {
            json!({})
        } else {
            json!({"echo": false})
        };
        cell.metadata
            .extra
            .insert(RESOLVED_OPTIONS_KEY.into(), resolved);
        cell.metadata.extra.insert(
            OUTPUT_KINDS_KEY.into(),
            json!(if i == 0 { json!([]) } else { json!([null]) }),
        );
    }
    entry.engine.known_options = Some(vec!["echo".to_string()]);
    rehash_document(&mut entry, &Default::default()).unwrap();
    coalesce(&[entry], &units).unwrap()
}

fn splice(source: &str, record: &Record) -> (String, SpliceReport) {
    let dir = tempfile::tempdir().unwrap();
    let mut c = ctx();
    let (doc, report) = splice_record(parse(source), record, &mut c, dir.path());
    (text_of(&doc), report)
}

fn recorded() -> (String, Record) {
    let source = format!("{SETUP}{CHUNK}");
    let record = setup_record(&source);
    (source, record)
}

#[test]
fn a_chunk_presents_under_the_setup_chunks_resolved_options() {
    let (source, record) = recorded();
    let (text, report) = splice(&source, &record);
    assert_eq!(report.units[1].1, UnitStaleness::Fresh, "{text}");
    assert!(
        !text.contains("print(1234)"),
        "echo = FALSE hides the source:\n{text}"
    );
    assert!(text.contains("[1] 1234"), "the output shows:\n{text}");
}

#[test]
fn a_header_edit_shows_without_re_running() {
    let (_, record) = recorded();
    let edited = format!("{SETUP}```{{r}}\n#| echo: true\nprint(1234)\n```\n");
    let (text, report) = splice(&edited, &record);
    assert_eq!(report.units[1].1, UnitStaleness::Fresh, "{text}");
    assert!(
        text.contains("print(1234)"),
        "the header wins over the setup:\n{text}"
    );
}

#[test]
fn a_setup_chunk_edit_does_not_show_until_the_next_run() {
    let (_, record) = recorded();
    let edited = format!("{}{CHUNK}", SETUP.replace("echo = FALSE", "echo = TRUE"));
    let (text, report) = splice(&edited, &record);
    assert_eq!(report.units[0].1, UnitStaleness::Stale, "{text}");
    assert_eq!(report.units[1].1, UnitStaleness::Fresh, "{text}");
    assert!(
        !text.contains("print(1234)"),
        "the neighbor keeps showing the old record:\n{text}"
    );
}
