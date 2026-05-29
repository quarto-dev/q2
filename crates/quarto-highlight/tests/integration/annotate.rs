//! Tests for `quarto_highlight::annotate_pandoc` — the AST walker that
//! adds `data-hl-spans` attribute values to `CodeBlock` and inline `Code`
//! nodes whose first class resolves to a built-in or user grammar.
//!
//! Shape of the test: build a tiny Pandoc AST by hand, run the walker,
//! assert the attribute was written (or skipped, per the rules).

use quarto_highlight::{SPANS_ATTR_KEY, annotate_pandoc, encoding};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Attr, AttrSourceInfo, Block, CodeBlock, Inline, Inlines, Paragraph};
use quarto_pandoc_types::{Code, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::{FileId, SourceInfo};

fn attr_with_class(class: &str) -> Attr {
    use hashlink::LinkedHashMap;
    (String::new(), vec![class.to_string()], LinkedHashMap::new())
}

fn empty_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

fn empty_attr_source() -> AttrSourceInfo {
    AttrSourceInfo::empty()
}

fn make_code_block(class: &str, text: &str) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: attr_with_class(class),
        text: text.to_string(),
        source_info: empty_source_info(),
        attr_source: empty_attr_source(),
    })
}

fn empty_pandoc() -> Pandoc {
    Pandoc {
        meta: ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: empty_source_info(),
            merge_op: MergeOp::default(),
        },
        blocks: vec![],
    }
}

fn get_hl_attr(attr: &Attr) -> Option<&str> {
    attr.2.get(SPANS_ATTR_KEY).map(|s| s.as_str())
}

#[test]
fn annotates_known_codeblock_class() {
    let mut doc = empty_pandoc();
    doc.blocks
        .push(make_code_block("python", "def foo(): pass\n"));

    annotate_pandoc(&mut doc, None).expect("annotate must not error");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        panic!("expected CodeBlock");
    };
    let encoded = get_hl_attr(&cb.attr).expect("data-hl-spans should be written");
    let spans = encoding::decode(encoded).expect("attr should contain valid JSON");
    assert!(
        !spans.is_empty(),
        "spans should be non-empty for python source"
    );
    assert!(
        spans.iter().any(|s| s.capture == "keyword"),
        "expected at least one keyword span, got: {spans:?}"
    );
}

#[test]
fn leaves_unknown_class_untouched() {
    let mut doc = empty_pandoc();
    doc.blocks.push(make_code_block("klingon", "K'tah!"));

    annotate_pandoc(&mut doc, None).expect("annotate must not error");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        panic!("expected CodeBlock");
    };
    assert!(
        get_hl_attr(&cb.attr).is_none(),
        "no data-hl-spans should be written for unknown class"
    );
}

#[test]
fn leaves_existing_annotation_alone() {
    let mut doc = empty_pandoc();
    let mut block = make_code_block("python", "def foo(): pass\n");
    if let Block::CodeBlock(cb) = &mut block {
        cb.attr
            .2
            .insert(SPANS_ATTR_KEY.to_string(), "[]".to_string());
    }
    doc.blocks.push(block);

    annotate_pandoc(&mut doc, None).expect("annotate must not error");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    // Filter-authored annotation wins: we must not overwrite it even if
    // the grammar would have produced richer output.
    assert_eq!(get_hl_attr(&cb.attr), Some("[]"));
}

#[test]
fn annotates_inline_code() {
    use hashlink::LinkedHashMap;
    let inline = Inline::Code(Code {
        attr: (
            String::new(),
            vec!["python".to_string()],
            LinkedHashMap::new(),
        ),
        text: "print(42)".to_string(),
        source_info: empty_source_info(),
        attr_source: empty_attr_source(),
    });
    let mut doc = empty_pandoc();
    doc.blocks.push(Block::Paragraph(Paragraph {
        content: Inlines::from(vec![inline]),
        source_info: empty_source_info(),
    }));

    annotate_pandoc(&mut doc, None).expect("annotate must not error");

    let Block::Paragraph(p) = &doc.blocks[0] else {
        unreachable!()
    };
    let Inline::Code(c) = &p.content[0] else {
        panic!("expected inline Code");
    };
    let encoded = get_hl_attr(&c.attr).expect("data-hl-spans on inline Code");
    let spans = encoding::decode(encoded).unwrap();
    assert!(!spans.is_empty(), "inline code should produce spans");
}

#[test]
fn code_block_with_no_class_is_skipped() {
    use hashlink::LinkedHashMap;
    let mut doc = empty_pandoc();
    doc.blocks.push(Block::CodeBlock(CodeBlock {
        attr: (String::new(), vec![], LinkedHashMap::new()),
        text: "foo bar".to_string(),
        source_info: empty_source_info(),
        attr_source: empty_attr_source(),
    }));

    annotate_pandoc(&mut doc, None).expect("annotate must not error");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    assert!(get_hl_attr(&cb.attr).is_none());
}
