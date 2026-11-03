//! Every example in the CommonMark spec converts without panicking.
//!
//! `pandoc.read(text, "commonmark")` (GH #885) hands this crate arbitrary
//! strings, so a construct the converter does not handle would crash the
//! render instead of producing a node. The spec's examples cover every
//! construct comrak produces with extensions off. Each is converted with
//! source tracking on, which also exercises the text tiling in `text.rs`.

use comrak::{Arena, Options, parse_document};
use comrak_to_pandoc::{SourceLocationContext, convert_document_with_source};
use quarto_source_map::FileId;

/// The markdown of each `example` block in `spec.txt`, with the spec's `→`
/// tab marker turned back into a tab.
fn spec_examples(spec: &str) -> Vec<String> {
    let fence = format!("{} example", "`".repeat(32));
    let mut examples = Vec::new();
    let mut lines = spec.lines();
    while let Some(line) = lines.next() {
        if line != fence {
            continue;
        }
        let mut markdown = String::new();
        for line in lines.by_ref() {
            if line == "." {
                break;
            }
            markdown.push_str(&line.replace('→', "\t"));
            markdown.push('\n');
        }
        examples.push(markdown);
    }
    examples
}

#[test]
fn every_commonmark_spec_example_converts() {
    let path = std::path::Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
        .join("../../external-sources/commonmark-spec/spec.txt");
    let spec = std::fs::read_to_string(&path).expect("spec.txt is checked in");
    let examples = spec_examples(&spec);
    assert!(examples.len() > 600, "found {} examples", examples.len());

    let failures: Vec<(usize, String)> = examples
        .iter()
        .enumerate()
        .filter_map(|(i, markdown)| {
            std::panic::catch_unwind(|| {
                let arena = Arena::new();
                let root = parse_document(&arena, markdown, &Options::default());
                let ctx = SourceLocationContext::new(markdown, FileId(0));
                convert_document_with_source(root, Some(&ctx));
            })
            .err()
            .map(|_| (i + 1, markdown.clone()))
        })
        .collect();

    assert!(
        failures.is_empty(),
        "{} spec examples panic (example number, markdown):\n{failures:#?}",
        failures.len()
    );
}
