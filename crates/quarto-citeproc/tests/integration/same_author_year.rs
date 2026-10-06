//! Two distinct references sharing author + year, cited in one cluster, must
//! be disambiguated with year suffixes (bd-bkcgdee7).

use quarto_citeproc::{Citation, CitationItem, Processor, Reference};
use quarto_csl::parse_csl;

const CHICAGO: &str = include_str!("../../../pampa/resources/csl/chicago-author-date.csl");

fn reference(id: &str) -> Reference {
    serde_json::from_str(&format!(
        r#"{{"id":"{id}","type":"article-journal","title":"Coefficient alpha",
            "author":[{{"family":"Cronbach","given":"Lee J."}}],
            "issued":{{"date-parts":[[1951]]}}}}"#
    ))
    .unwrap()
}

fn item(id: &str) -> CitationItem {
    CitationItem {
        id: id.to_string(),
        ..Default::default()
    }
}

#[test]
fn same_author_year_in_one_cluster_gets_year_suffixes() {
    let mut p = Processor::new(parse_csl(CHICAGO).unwrap());
    p.add_references([reference("a"), reference("b")]);
    let cite = Citation {
        items: vec![item("a"), item("b")],
        ..Default::default()
    };
    let out = p.process_citations_with_disambiguation(&[cite]).unwrap();
    assert_eq!(out, ["(Cronbach 1951a, 1951b)"]);
}

/// `@a` (normal) and `@b` (author-in-text, as pandoc's bare `@id` syntax
/// produces) share author and year: the in-text cite must still count toward
/// disambiguation.
#[test]
fn author_in_text_cite_takes_part_in_disambiguation() {
    let mut p = Processor::new(parse_csl(CHICAGO).unwrap());
    p.add_references([reference("a"), reference("b")]);
    let cite = |id: &str, author_only: bool, n: i32| Citation {
        id: None,
        note_number: Some(n),
        items: vec![CitationItem {
            suppress_author: Some(false),
            author_only: Some(author_only),
            ..item(id)
        }],
    };
    let out = p
        .process_citations_with_disambiguation(&[cite("a", false, 1), cite("b", true, 2)])
        .unwrap();
    assert_eq!(out, ["(Cronbach 1951a)", "Cronbach (1951b)"]);
    let bib = p.generate_bibliography().unwrap();
    assert!(bib[0].1.contains("1951a"), "{bib:?}");
    assert!(bib[1].1.contains("1951b"), "{bib:?}");
}
