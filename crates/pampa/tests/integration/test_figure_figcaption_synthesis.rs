//! Figcaption synthesis from `data-qf-*` kvs (bd-hcp8m3ve).
//!
//! The crossref renderer (quarto-core) emits floats as
//! `Div > Figure(attr: quarto-float classes + data-qf-* kvs)`. Pandoc's
//! `Caption` carries no attr, so the HTML writer synthesizes the
//! `<figcaption>` id and classes from the Figure's kvs and strips the
//! kvs from the emitted `<figure>` tag. Contract:
//! `claude-notes/designs/float-layout-class-taxonomy.md`.

use hashlink::LinkedHashMap;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc};
use pampa::writers;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Figure, Paragraph, Plain};
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::inline::Str;
use quarto_source_map::SourceInfo;

fn si() -> SourceInfo {
    SourceInfo::for_test()
}

fn str_inline(s: &str) -> Inline {
    Inline::Str(Str {
        text: s.to_string(),
        source_info: si(),
    })
}

fn caption_para(text: &str) -> Block {
    Block::Paragraph(Paragraph {
        content: vec![str_inline(text)],
        source_info: si(),
    })
}

fn figure_with_kvs(kvs: &[(&str, &str)], caption: Option<&str>) -> Block {
    let mut map: LinkedHashMap<String, String> = LinkedHashMap::new();
    for (k, v) in kvs {
        map.insert(k.to_string(), v.to_string());
    }
    Block::Figure(Figure {
        attr: (
            String::new(),
            vec!["quarto-float".to_string(), "quarto-float-fig".to_string()],
            map,
        ),
        caption: Caption {
            short: None,
            long: caption.map(|c| vec![caption_para(c)]),
            source_info: si(),
        },
        content: vec![Block::Plain(Plain {
            content: vec![str_inline("content")],
            source_info: si(),
        })],
        source_info: si(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn to_html(block: Block) -> String {
    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![block],
    };
    let ctx = ASTContext::default();
    let mut buf = Vec::new();
    writers::html::write(&pandoc, &ctx, &mut buf).expect("html write");
    String::from_utf8(buf).expect("utf8")
}

#[test]
fn figcaption_gets_id_and_classes_from_kvs() {
    let html = to_html(figure_with_kvs(
        &[
            ("data-qf-ref-type", "fig"),
            ("data-qf-caption-location", "bottom"),
            ("data-qf-caption-id", "fig-1-caption"),
        ],
        Some("Figure 1: Cap"),
    ));
    assert!(
        html.contains(
            "<figcaption id=\"fig-1-caption\" class=\"quarto-float-caption-bottom quarto-float-caption quarto-float-fig\">"
        ),
        "figcaption id/classes missing in: {html}"
    );
    // kvs are consumed, never emitted on the <figure>.
    assert!(
        !html.contains("data-qf-"),
        "data-qf-* kvs leaked into HTML: {html}"
    );
    // The figure keeps its own classes.
    assert!(
        html.contains("<figure class=\"quarto-float quarto-float-fig\""),
        "figure classes missing in: {html}"
    );
}

#[test]
fn caption_location_top_places_figcaption_before_content() {
    let html = to_html(figure_with_kvs(
        &[
            ("data-qf-ref-type", "fig"),
            ("data-qf-caption-location", "top"),
            ("data-qf-caption-id", "fig-1-caption"),
        ],
        Some("Figure 1: Cap"),
    ));
    let cap = html.find("<figcaption").expect("figcaption present");
    let content = html.find("content").expect("content present");
    assert!(
        cap < content,
        "caption-location=top must place figcaption before content: {html}"
    );
    assert!(
        html.contains("quarto-float-caption-top"),
        "location class missing: {html}"
    );
}

#[test]
fn uncaptioned_kv_adds_uncaptioned_class() {
    let html = to_html(figure_with_kvs(
        &[
            ("data-qf-ref-type", "fig"),
            ("data-qf-caption-location", "bottom"),
            ("data-qf-caption-id", "fig-1-caption"),
            ("data-qf-uncaptioned", "1"),
        ],
        Some("Figure 1"),
    ));
    assert!(
        html.contains("quarto-uncaptioned"),
        "quarto-uncaptioned class missing: {html}"
    );
}

#[test]
fn plain_figure_without_kvs_is_unchanged() {
    let html = to_html(Block::Figure(Figure {
        attr: ("fig-x".to_string(), Vec::new(), LinkedHashMap::new()),
        caption: Caption {
            short: None,
            long: Some(vec![caption_para("A caption")]),
            source_info: si(),
        },
        content: vec![Block::Plain(Plain {
            content: vec![str_inline("content")],
            source_info: si(),
        })],
        source_info: si(),
        attr_source: AttrSourceInfo::empty(),
    }));
    assert!(
        html.contains("<figure id=\"fig-x\">"),
        "plain figure: {html}"
    );
    assert!(
        html.contains("<figcaption>"),
        "plain figcaption untouched: {html}"
    );
}
