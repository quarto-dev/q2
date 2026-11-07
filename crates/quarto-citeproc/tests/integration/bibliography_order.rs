//! Bibliography order for styles without a bibliography `<sort>`
//! (bd-mumobar8): entries follow citation number, as Pandoc's citeproc
//! lists them, not the order the references were added. References with no
//! citation number come last, in the order they were added.

use quarto_citeproc::{Citation, CitationItem, Processor, Reference};
use quarto_csl::parse_csl;

/// A numeric style with no bibliography `<sort>`.
const UNSORTED_NUMERIC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info><title>Unsorted</title><id>unsorted</id><updated>2026-01-01T00:00:00+00:00</updated></info>
  <citation><layout><text variable="citation-number" prefix="[" suffix="]"/></layout></citation>
  <bibliography><layout><text variable="title"/></layout></bibliography>
</style>
"#;

/// An author-title style with no bibliography `<sort>` that substitutes
/// repeated authors.
const SUBSTITUTING: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info><title>Substituting</title><id>substituting</id><updated>2026-01-01T00:00:00+00:00</updated></info>
  <citation><layout><text variable="title"/></layout></citation>
  <bibliography subsequent-author-substitute="---">
    <layout><group delimiter=". "><names variable="author"><name/></names><text variable="title"/></group></layout>
  </bibliography>
</style>
"#;

fn reference(id: &str) -> Reference {
    serde_json::from_str(&format!(
        r#"{{"id":"{id}","type":"book","title":"Title {id}",
            "author":[{{"family":"Smith","given":"Jane"}}]}}"#
    ))
    .unwrap()
}

fn cite(ids: &[&str]) -> Citation {
    Citation {
        items: ids
            .iter()
            .map(|id| CitationItem {
                id: id.to_string(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn processor(csl: &str, ids: &[&str]) -> Processor {
    let mut p = Processor::new(parse_csl(csl).unwrap());
    p.add_references(ids.iter().map(|id| reference(id)));
    p
}

fn output_ids(p: &mut Processor) -> Vec<String> {
    p.generate_bibliography_to_outputs()
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn unsorted_bibliography_follows_citation_order() {
    let mut p = processor(UNSORTED_NUMERIC, &["a", "b", "c"]);
    p.process_citations_with_disambiguation(&[cite(&["c"]), cite(&["a", "b"])])
        .unwrap();
    assert_eq!(output_ids(&mut p), ["c", "a", "b"]);
}

#[test]
fn unnumbered_references_follow_the_numbered_ones_in_insertion_order() {
    let mut p = processor(UNSORTED_NUMERIC, &["a", "b", "c", "d"]);
    p.process_citations_with_disambiguation(&[cite(&["c"])])
        .unwrap();
    assert_eq!(output_ids(&mut p), ["c", "a", "b", "d"]);
}

#[test]
fn string_bibliography_matches_the_output_bibliography() {
    let mut p = processor(UNSORTED_NUMERIC, &["a", "b", "c"]);
    p.process_citations_with_disambiguation(&[cite(&["b"]), cite(&["a"])])
        .unwrap();
    let ids: Vec<String> = p
        .generate_bibliography()
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, ["b", "a", "c"]);
}

#[test]
fn string_bibliography_applies_subsequent_author_substitute() {
    let mut p = processor(SUBSTITUTING, &["a", "b"]);
    p.process_citations_with_disambiguation(&[cite(&["a", "b"])])
        .unwrap();
    let entries = p.generate_bibliography().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries[0].1.starts_with("Jane Smith"),
        "first entry keeps the author: {:?}",
        entries[0].1
    );
    assert!(
        entries[1].1.starts_with("---"),
        "repeated author is substituted: {:?}",
        entries[1].1
    );
}
