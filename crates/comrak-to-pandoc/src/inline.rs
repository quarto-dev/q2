/*
 * inline.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert comrak inline nodes to Pandoc inlines.
 */

use crate::text::tokenize_text;
use crate::{empty_attr, empty_source_info};
use comrak::arena_tree::Node;
use comrak::nodes::{Ast, NodeCode, NodeLink, NodeValue};
use hashlink::LinkedHashMap;
use quarto_pandoc_types::{
    AttrSourceInfo, Code, Emph, Image, Inline, Inlines, LineBreak, Link, SoftBreak, Strong,
    TargetSourceInfo,
};
use std::cell::RefCell;

/// Convert a comrak node's inline children to Pandoc inlines.
pub fn convert_children_to_inlines<'a>(node: &'a Node<'a, RefCell<Ast>>) -> Inlines {
    node.children()
        .flat_map(|child| convert_inline(child))
        .collect()
}

/// Convert a comrak inline node to Pandoc inlines.
///
/// Returns a Vec because some nodes (like Text) expand to multiple inlines.
pub fn convert_inline<'a>(node: &'a Node<'a, RefCell<Ast>>) -> Inlines {
    let ast = node.data.borrow();

    match &ast.value {
        NodeValue::Text(text) => tokenize_text(text),

        NodeValue::SoftBreak => {
            vec![Inline::SoftBreak(SoftBreak {
                source_info: empty_source_info(),
            })]
        }

        NodeValue::LineBreak => {
            vec![Inline::LineBreak(LineBreak {
                source_info: empty_source_info(),
            })]
        }

        NodeValue::Code(code) => {
            vec![convert_code(code)]
        }

        NodeValue::Emph => {
            let children = convert_children_to_inlines(node);
            vec![Inline::Emph(Emph {
                content: children,
                source_info: empty_source_info(),
            })]
        }

        NodeValue::Strong => {
            let children = convert_children_to_inlines(node);
            vec![Inline::Strong(Strong {
                content: children,
                source_info: empty_source_info(),
            })]
        }

        NodeValue::Link(link) => {
            let children = convert_children_to_inlines(node);
            vec![convert_link(link, children)]
        }

        NodeValue::Image(link) => {
            let children = convert_children_to_inlines(node);
            vec![convert_image(link, children)]
        }

        NodeValue::Escaped => {
            // Escaped characters just become the character itself
            // The actual character is in the children as Text
            convert_children_to_inlines(node)
        }

        // Unsupported inline types - panic as they're outside CommonMark subset
        NodeValue::HtmlInline(_) => {
            panic!("HtmlInline not supported in CommonMark subset")
        }
        NodeValue::Strikethrough => {
            panic!("Strikethrough (GFM extension) not supported in CommonMark subset")
        }
        NodeValue::Superscript => {
            panic!("Superscript not supported in CommonMark subset")
        }
        NodeValue::Subscript => {
            panic!("Subscript not supported in CommonMark subset")
        }
        NodeValue::FootnoteReference(_) => {
            panic!("FootnoteReference not supported in CommonMark subset")
        }
        NodeValue::Math(_) => {
            panic!("Math not supported in CommonMark subset")
        }
        NodeValue::WikiLink(_) => {
            panic!("WikiLink not supported in CommonMark subset")
        }
        NodeValue::Underline => {
            panic!("Underline not supported in CommonMark subset")
        }
        NodeValue::SpoileredText => {
            panic!("SpoileredText not supported in CommonMark subset")
        }
        NodeValue::EscapedTag(_) => {
            panic!("EscapedTag not supported in CommonMark subset")
        }
        NodeValue::Highlight => {
            panic!("Highlight not supported in CommonMark subset")
        }
        NodeValue::Raw(_) => {
            panic!("Raw not supported in CommonMark subset")
        }

        // Block nodes shouldn't appear in inline context
        _ => {
            panic!(
                "Unexpected node type in inline context: {:?}",
                std::mem::discriminant(&ast.value)
            )
        }
    }
}

fn convert_code(code: &NodeCode) -> Inline {
    Inline::Code(Code {
        attr: empty_attr(),
        text: code.literal.clone(),
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn convert_link(link: &NodeLink, children: Inlines) -> Inline {
    // Detect autolinks: content is just Str(url) matching the URL
    let is_autolink = match children.as_slice() {
        [Inline::Str(s)] => s.text == link.url,
        _ => false,
    };

    let attr = if is_autolink {
        // pampa adds "uri" class to autolinks
        (String::new(), vec!["uri".to_string()], LinkedHashMap::new())
    } else {
        empty_attr()
    };

    Inline::Link(Link {
        attr,
        content: children,
        target: (link.url.clone(), link.title.clone()),
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    })
}

fn convert_image(link: &NodeLink, children: Inlines) -> Inline {
    // For images, children become alt text
    Inline::Image(Image {
        attr: empty_attr(),
        content: children,
        target: (link.url.clone(), link.title.clone()),
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena, Options};

    fn get_first_para_inlines(markdown: &str) -> Inlines {
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &Options::default());
        // Get first child (should be paragraph)
        let para = root.first_child().expect("Expected a block");
        convert_children_to_inlines(para)
    }

    #[test]
    fn test_simple_text() {
        let inlines = get_first_para_inlines("hello world\n");
        assert_eq!(inlines.len(), 3);
        match &inlines[0] {
            Inline::Str(s) => assert_eq!(s.text, "hello"),
            _ => panic!("Expected Str"),
        }
        assert!(matches!(inlines[1], Inline::Space(_)));
        match &inlines[2] {
            Inline::Str(s) => assert_eq!(s.text, "world"),
            _ => panic!("Expected Str"),
        }
    }

    #[test]
    fn test_emphasis() {
        let inlines = get_first_para_inlines("*hello*\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Emph(e) => {
                assert_eq!(e.content.len(), 1);
                match &e.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "hello"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Emph"),
        }
    }

    #[test]
    fn test_strong() {
        let inlines = get_first_para_inlines("**hello**\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Strong(s) => {
                assert_eq!(s.content.len(), 1);
            }
            _ => panic!("Expected Strong"),
        }
    }

    #[test]
    fn test_inline_code() {
        let inlines = get_first_para_inlines("`code`\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Code(c) => assert_eq!(c.text, "code"),
            _ => panic!("Expected Code"),
        }
    }

    #[test]
    fn test_link() {
        let inlines = get_first_para_inlines("[text](http://example.com)\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(l) => {
                assert_eq!(l.target.0, "http://example.com");
                assert_eq!(l.target.1, "");
                // Not an autolink, so no uri class
                assert!(l.attr.1.is_empty());
            }
            _ => panic!("Expected Link"),
        }
    }

    #[test]
    fn test_autolink() {
        let inlines = get_first_para_inlines("<http://example.com>\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(l) => {
                assert_eq!(l.target.0, "http://example.com");
                // Autolink should have uri class
                assert_eq!(l.attr.1, vec!["uri".to_string()]);
            }
            _ => panic!("Expected Link"),
        }
    }

    #[test]
    fn test_image() {
        let inlines = get_first_para_inlines("![alt](image.png)\n");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Image(i) => {
                assert_eq!(i.target.0, "image.png");
                // Alt text should be in content
                assert_eq!(i.content.len(), 1);
            }
            _ => panic!("Expected Image"),
        }
    }

    #[test]
    fn test_soft_break() {
        let inlines = get_first_para_inlines("hello\nworld\n");
        // Should be: Str("hello"), SoftBreak, Str("world")
        assert_eq!(inlines.len(), 3);
        assert!(matches!(inlines[1], Inline::SoftBreak(_)));
    }

    #[test]
    fn test_hard_break() {
        let inlines = get_first_para_inlines("hello\\\nworld\n");
        // Should be: Str("hello"), LineBreak, Str("world")
        assert_eq!(inlines.len(), 3);
        assert!(matches!(inlines[1], Inline::LineBreak(_)));
    }
}
