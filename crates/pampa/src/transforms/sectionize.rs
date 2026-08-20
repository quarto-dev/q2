/*
 * transforms/sectionize.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Sectionize transform: wrap headers in section Divs.
 */

//! Sectionize transform for wrapping headers in section Divs.
//!
//! This transform implements functionality analogous to Pandoc's `--section-divs` option.
//! It wraps each header and its following content in a `Div` block with:
//!
//! - The ID moved from the header to the section
//! - Classes duplicated on both section and header
//! - A `section` class added to identify it for the HTML writer
//! - A `levelN` class (e.g., `level2`) added to the section
//!
//! ## Example
//!
//! Input:
//! ```markdown
//! ## Section A {#sec-a .highlight}
//! Content A.
//! ### Subsection A.1
//! Sub content.
//! ## Section B
//! Content B.
//! ```
//!
//! Output structure (as Divs in the AST):
//! ```text
//! Div(#sec-a .section .level2 .highlight)
//!   Header(2, .highlight) "Section A"
//!   Para "Content A."
//!   Div(#subsection-a.1 .section .level3)
//!     Header(3) "Subsection A.1"
//!     Para "Sub content."
//! Div(#section-b .section .level2)
//!   Header(2) "Section B"
//!   Para "Content B."
//! ```
//!
//! The HTML writer recognizes Divs with the `section` class and emits
//! `<section>` tags instead of `<div>`.

use crate::pandoc::block::{Block, Div, Header};
use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_source_map::{By, SourceInfo};
use smallvec::smallvec;

/// Wrap headers in section Divs.
///
/// This function transforms a flat list of blocks into a nested structure
/// where each header and its following content are wrapped in a Div with
/// the `section` class.
///
/// # Arguments
///
/// * `blocks` - The blocks to transform
///
/// # Returns
///
/// A new vector of blocks with headers wrapped in section Divs.
///
/// # Behavior
///
/// - Content before the first header is preserved outside any section
/// - Headers at level N close all sections at level >= N
/// - The header's ID moves to the section Div
/// - The header's classes are duplicated on both section and header
/// - The header's attributes are duplicated on both section and header
/// - A `section` class is added to identify the Div for the HTML writer
/// - A `levelN` class is added to the section (e.g., `level2`)
pub fn sectionize_blocks(blocks: Vec<Block>) -> Vec<Block> {
    // Stack of (level, attr, content) for open sections
    // Each entry represents an open section that hasn't been closed yet
    let mut section_stack: Vec<(
        usize,
        (String, Vec<String>, LinkedHashMap<String, String>),
        Vec<Block>,
    )> = vec![];
    let mut output: Vec<Block> = vec![];

    for block in blocks {
        // Recurse into Divs first, so an inner heading is already a
        // section by the time the absorb check below looks at it.
        let block = sectionize_div(block);
        if let Block::Header(ref header) = block {
            let level = header.level;
            let (id, classes, attrs) = &header.attr;

            // Close all sections at level >= this header's level
            while let Some((stack_level, _, _)) = section_stack.last() {
                if *stack_level >= level {
                    let (_, section_attr, section_content) = section_stack.pop().unwrap();
                    let section_div = Block::Div(Div {
                        attr: section_attr,
                        content: section_content,
                        source_info: SourceInfo::Generated {
                            by: By::sectionize(),
                            from: smallvec![],
                        },
                        attr_source: AttrSourceInfo::empty(),
                    });
                    // Add closed section to parent, or output if no parent
                    if let Some((_, _, parent_content)) = section_stack.last_mut() {
                        parent_content.push(section_div);
                    } else {
                        output.push(section_div);
                    }
                } else {
                    break;
                }
            }

            // Create attributes for new section
            // - ID moves from header to section
            // - "section" class added first, then levelN, then user classes
            // - Other attributes are duplicated
            let mut section_classes = vec!["section".to_string(), format!("level{}", level)];
            section_classes.extend(classes.clone());
            let section_attr = (
                id.clone(), // ID moves to section
                section_classes,
                attrs.clone(), // Attributes duplicated
            );

            // Create header with ID removed but classes and attributes preserved
            let header_without_id = Block::Header(Header {
                level,
                attr: (String::new(), classes.clone(), attrs.clone()), // Empty ID, keep classes and attrs
                content: header.content.clone(),
                source_info: header.source_info.clone(),
                attr_source: header.attr_source.clone(),
            });

            // Push new section onto stack (with ID-less header as first content)
            section_stack.push((level, section_attr, vec![header_without_id]));
        } else {
            // Non-header block: add to innermost section, or output if none
            if let Some((_, _, content)) = section_stack.last_mut() {
                content.push(block);
            } else {
                output.push(block);
            }
        }
    }

    // Close all remaining open sections (innermost first)
    while let Some((_, section_attr, section_content)) = section_stack.pop() {
        let section_div = Block::Div(Div {
            attr: section_attr,
            content: section_content,
            source_info: SourceInfo::Generated {
                by: By::sectionize(),
                from: smallvec![],
            },
            attr_source: AttrSourceInfo::empty(),
        });
        if let Some((_, _, parent_content)) = section_stack.last_mut() {
            parent_content.push(section_div);
        } else {
            output.push(section_div);
        }
    }

    output
}

/// Sectionize the content of a non-section `Div`, then apply pandoc's
/// absorb rule.
///
/// `makeSections` recurses into Divs and replaces a Div by the section it
/// wraps when — and only when — the Div has an **empty id** and its
/// content is **exactly one section**; the Div's classes and key-values
/// are merged onto that section. Anything else keeps the Div and nests
/// the section(s) inside it.
///
/// The five cases are pinned by Quarto 1's render of
/// `claude-notes/plans/tabset-headings-in-toc-investigation/absorb-rule-probe/`:
/// a bare wrapper absorbs (`<section id="case-a" class="level3 wrap-a">`);
/// a Div with its own id does not; leading content before the heading
/// does not; two sibling headings do not; key-values are merged.
///
/// Blocks other than `Div` are returned untouched — in particular
/// `BlockQuote`, which pandoc does not descend into, so a heading quoted
/// there stays a bare `Header` and never reaches the table of contents.
fn sectionize_div(block: Block) -> Block {
    let Block::Div(div) = block else {
        return block;
    };
    if is_section(&div.attr.1) {
        return Block::Div(div);
    }

    let Div {
        attr,
        content,
        source_info,
        attr_source,
    } = div;
    let mut content = sectionize_blocks(content);

    let absorbable = attr.0.is_empty()
        && content.len() == 1
        && matches!(&content[0], Block::Div(inner) if is_section(&inner.attr.1));
    if !absorbable {
        return Block::Div(Div {
            attr,
            content,
            source_info,
            attr_source,
        });
    }

    let Some(Block::Div(mut section)) = content.pop() else {
        unreachable!("checked by `absorbable`")
    };
    // The section keeps its own id (the header's) and its `section` /
    // `levelN` markers; the wrapper contributes its classes and
    // key-values. `attr_source` for the merged-in pieces is dropped:
    // the section Div is `By::sectionize()`-generated anyway, and a
    // stale offset would be worse than none.
    for class in attr.1 {
        if !section.attr.1.contains(&class) {
            section.attr.1.push(class);
        }
    }
    for (k, v) in attr.2 {
        section.attr.2.entry(k).or_insert(v);
    }
    section.attr_source = AttrSourceInfo::empty();
    Block::Div(section)
}

/// A Div produced by [`sectionize_blocks`] carries the `section` class.
fn is_section(classes: &[String]) -> bool {
    classes.iter().any(|c| c == "section")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::block::Paragraph;
    use crate::pandoc::inline::{Inline, Str};

    // Helper functions for creating test data
    fn dummy_source_info() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn make_header(level: usize, id: &str, classes: Vec<&str>, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_header_with_attrs(
        level: usize,
        id: &str,
        classes: Vec<&str>,
        attrs: Vec<(&str, &str)>,
        text: &str,
    ) -> Block {
        let mut attr_map = LinkedHashMap::new();
        for (k, v) in attrs {
            attr_map.insert(k.to_string(), v.to_string());
        }
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                attr_map,
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })
    }

    // Helper to check if a block is a section Div with expected properties
    fn assert_is_section(
        block: &Block,
        expected_id: &str,
        expected_level: usize,
        expected_user_classes: &[&str],
    ) {
        let Block::Div(div) = block else {
            panic!("Expected Div, got {:?}", block);
        };
        let (id, classes, _) = &div.attr;

        assert_eq!(id, expected_id, "Section ID mismatch");
        assert!(
            classes.contains(&"section".to_string()),
            "Section should have 'section' class"
        );
        assert!(
            classes.contains(&format!("level{}", expected_level)),
            "Section should have 'level{}' class, got {:?}",
            expected_level,
            classes
        );
        for class in expected_user_classes {
            assert!(
                classes.contains(&class.to_string()),
                "Section should have '{}' class, got {:?}",
                class,
                classes
            );
        }
    }

    // Helper to get the header inside a section Div
    fn get_section_header(block: &Block) -> &Header {
        let Block::Div(div) = block else {
            panic!("Expected Div, got {:?}", block);
        };
        let Block::Header(header) = &div.content[0] else {
            panic!("Expected Header as first content, got {:?}", div.content[0]);
        };
        header
    }

    #[test]
    fn test_sectionize_empty_input() {
        let blocks: Vec<Block> = vec![];
        let result = sectionize_blocks(blocks);
        assert!(result.is_empty());
    }

    #[test]
    fn test_sectionize_flat_sections_same_level() {
        // h2, h2, h2 -> three sibling sections
        let blocks = vec![
            make_header(2, "sec-a", vec![], "Section A"),
            make_para("Content A."),
            make_header(2, "sec-b", vec![], "Section B"),
            make_para("Content B."),
            make_header(2, "sec-c", vec![], "Section C"),
            make_para("Content C."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 3, "Should have 3 sibling sections");
        assert_is_section(&result[0], "sec-a", 2, &[]);
        assert_is_section(&result[1], "sec-b", 2, &[]);
        assert_is_section(&result[2], "sec-c", 2, &[]);
    }

    #[test]
    fn test_sectionize_nested_sections() {
        // h2, h3, h3, h2 -> h3s nested inside first h2
        let blocks = vec![
            make_header(2, "sec-a", vec![], "Section A"),
            make_para("Content A."),
            make_header(3, "sub-a1", vec![], "Subsection A.1"),
            make_para("Sub content 1."),
            make_header(3, "sub-a2", vec![], "Subsection A.2"),
            make_para("Sub content 2."),
            make_header(2, "sec-b", vec![], "Section B"),
            make_para("Content B."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 2, "Should have 2 top-level sections");

        // First section should contain the two h3 subsections
        let Block::Div(sec_a) = &result[0] else {
            panic!("Expected Div");
        };
        assert_eq!(sec_a.attr.0, "sec-a");
        // Content: header, para, subsection1, subsection2
        assert_eq!(sec_a.content.len(), 4, "Section A should have 4 items");
        assert_is_section(&sec_a.content[2], "sub-a1", 3, &[]);
        assert_is_section(&sec_a.content[3], "sub-a2", 3, &[]);

        // Second section
        assert_is_section(&result[1], "sec-b", 2, &[]);
    }

    #[test]
    fn test_sectionize_deep_nesting() {
        // h1, h2, h3, h4 -> each level nested inside parent
        let blocks = vec![
            make_header(1, "level-1", vec![], "Level 1"),
            make_header(2, "level-2", vec![], "Level 2"),
            make_header(3, "level-3", vec![], "Level 3"),
            make_header(4, "level-4", vec![], "Level 4"),
            make_para("Deep content."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 1, "Should have 1 top-level section");

        // Navigate down the nesting
        let Block::Div(level1) = &result[0] else {
            panic!("Expected Div");
        };
        assert_eq!(level1.attr.0, "level-1");
        assert_eq!(level1.content.len(), 2); // header, level2-section

        let Block::Div(level2) = &level1.content[1] else {
            panic!("Expected nested Div");
        };
        assert_eq!(level2.attr.0, "level-2");

        let Block::Div(level3) = &level2.content[1] else {
            panic!("Expected nested Div");
        };
        assert_eq!(level3.attr.0, "level-3");

        let Block::Div(level4) = &level3.content[1] else {
            panic!("Expected nested Div");
        };
        assert_eq!(level4.attr.0, "level-4");
        // level4 content: header, para
        assert_eq!(level4.content.len(), 2);
    }

    #[test]
    fn test_sectionize_mixed_levels() {
        // h2, h4, h3 -> h4 inside h2, then h3 inside h2 (h4 closes, h3 opens)
        let blocks = vec![
            make_header(2, "level-2", vec![], "Level 2"),
            make_para("Content."),
            make_header(4, "level-4", vec![], "Level 4"),
            make_para("Deep content."),
            make_header(3, "level-3", vec![], "Level 3"),
            make_para("Back up."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 1, "Should have 1 top-level section");

        let Block::Div(level2) = &result[0] else {
            panic!("Expected Div");
        };
        assert_eq!(level2.attr.0, "level-2");
        // Content: header, para, level4-section, level3-section
        assert_eq!(level2.content.len(), 4);
        assert_is_section(&level2.content[2], "level-4", 4, &[]);
        assert_is_section(&level2.content[3], "level-3", 3, &[]);
    }

    #[test]
    fn test_sectionize_content_before_first_header() {
        // Content before any header is preserved outside sections
        let blocks = vec![
            make_para("Preamble content."),
            make_header(2, "first-section", vec![], "First Section"),
            make_para("Section content."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 2, "Should have preamble para + 1 section");
        assert!(matches!(&result[0], Block::Paragraph(_)));
        assert_is_section(&result[1], "first-section", 2, &[]);
    }

    #[test]
    fn test_sectionize_empty_section() {
        // Header with no following content creates a valid section
        let blocks = vec![
            make_header(2, "empty", vec![], "Empty Section"),
            make_header(2, "next", vec![], "Next Section"),
            make_para("Content."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 2, "Should have 2 sections");

        let Block::Div(empty_sec) = &result[0] else {
            panic!("Expected Div");
        };
        // Empty section contains only the header
        assert_eq!(empty_sec.content.len(), 1);
        assert!(matches!(&empty_sec.content[0], Block::Header(_)));

        assert_is_section(&result[1], "next", 2, &[]);
    }

    #[test]
    fn test_sectionize_id_moves_to_section() {
        // ID should move from header to section, header should have empty ID
        let blocks = vec![
            make_header(2, "my-id", vec![], "Section"),
            make_para("Content."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 1);

        let Block::Div(section) = &result[0] else {
            panic!("Expected Div");
        };
        assert_eq!(section.attr.0, "my-id", "Section should have the ID");

        let header = get_section_header(&result[0]);
        assert!(header.attr.0.is_empty(), "Header should have empty ID");
    }

    #[test]
    fn test_sectionize_classes_duplicated() {
        // Classes should be on both section AND header
        let blocks = vec![
            make_header(2, "sec", vec!["highlight", "special"], "Section"),
            make_para("Content."),
        ];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 1);
        assert_is_section(&result[0], "sec", 2, &["highlight", "special"]);

        let header = get_section_header(&result[0]);
        assert!(
            header.attr.1.contains(&"highlight".to_string()),
            "Header should keep 'highlight' class"
        );
        assert!(
            header.attr.1.contains(&"special".to_string()),
            "Header should keep 'special' class"
        );
        // Header should NOT have 'section' or 'levelN' classes
        assert!(
            !header.attr.1.contains(&"section".to_string()),
            "Header should not have 'section' class"
        );
        assert!(
            !header.attr.1.contains(&"level2".to_string()),
            "Header should not have 'level2' class"
        );
    }

    #[test]
    fn test_sectionize_attributes_duplicated() {
        // Key-value attributes should be on both section AND header
        let blocks = vec![make_header_with_attrs(
            2,
            "sec",
            vec!["myclass"],
            vec![("data-foo", "bar"), ("style", "color:red")],
            "Section",
        )];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 1);

        let Block::Div(section) = &result[0] else {
            panic!("Expected Div");
        };
        assert_eq!(section.attr.2.get("data-foo"), Some(&"bar".to_string()));
        assert_eq!(section.attr.2.get("style"), Some(&"color:red".to_string()));

        let header = get_section_header(&result[0]);
        assert_eq!(header.attr.2.get("data-foo"), Some(&"bar".to_string()));
        assert_eq!(header.attr.2.get("style"), Some(&"color:red".to_string()));
    }

    #[test]
    fn test_sectionize_no_headers() {
        // Document with no headers should pass through unchanged
        let blocks = vec![make_para("Just a paragraph."), make_para("Another one.")];

        let result = sectionize_blocks(blocks);

        assert_eq!(result.len(), 2);
        assert!(matches!(&result[0], Block::Paragraph(_)));
        assert!(matches!(&result[1], Block::Paragraph(_)));
    }

    #[test]
    fn test_sectionize_section_div_has_generated_provenance() {
        // Plan 6: every synthesized Section Div carries
        // Generated { by: sectionize(), from: [] }. The wrapped Header retains
        // its original source_info.
        let blocks = vec![
            make_header(2, "sec-a", vec![], "A"),
            make_para("body"),
            make_header(2, "sec-b", vec![], "B"),
        ];
        let result = sectionize_blocks(blocks);
        assert_eq!(result.len(), 2);

        // First section's outer Div is Generated.
        let Block::Div(div) = &result[0] else {
            panic!("Expected section Div");
        };
        match &div.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "sectionize");
                assert!(
                    from.is_empty(),
                    "sectionize synthesizers carry no source-side anchors"
                );
            }
            other => panic!("Expected Generated source_info, got {:?}", other),
        }

        // Second section (end-of-input path) — same shape.
        let Block::Div(div2) = &result[1] else {
            panic!("Expected section Div");
        };
        match &div2.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "sectionize");
                assert!(from.is_empty());
            }
            other => panic!("Expected Generated source_info, got {:?}", other),
        }

        // The wrapped Header inside the section retains its original
        // (dummy) source_info — only the Div is synthesized.
        let Block::Header(_) = &div.content[0] else {
            panic!("Expected Header inside section");
        };
    }

    // ── recursion into Divs, and pandoc's absorb rule ──────────────
    //
    // Ground truth for every case below was captured from Quarto 1's
    // render of `claude-notes/plans/tabset-headings-in-toc-investigation/
    // absorb-rule-probe/`. pandoc's `makeSections` recurses into Divs and
    // replaces a Div by the section it wraps when — and only when — the
    // Div has an empty id and its content is exactly one section.

    fn make_div(
        id: &str,
        classes: Vec<&str>,
        attrs: Vec<(&str, &str)>,
        content: Vec<Block>,
    ) -> Block {
        let mut attr_map = LinkedHashMap::new();
        for (k, v) in attrs {
            attr_map.insert(k.to_string(), v.to_string());
        }
        Block::Div(Div {
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                attr_map,
            ),
            content,
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Unwrap a `Block::Div`, or panic with a useful message.
    fn as_div<'a>(block: &'a Block, what: &str) -> &'a Div {
        match block {
            Block::Div(d) => d,
            other => panic!("expected {what} to be a Div, got {other:?}"),
        }
    }

    #[test]
    fn test_sectionize_recurses_into_divs() {
        // A heading inside a Div must become a section; before this it
        // stayed a bare Header and the HTML writer emitted <div><h3>.
        let blocks = vec![make_div(
            "",
            vec!["wrap"],
            vec![],
            vec![make_header(3, "inner", vec![], "Inner"), make_para("Body")],
        )];
        let result = sectionize_blocks(blocks);
        assert_eq!(result.len(), 1);
        let section = as_div(&result[0], "the absorbed wrapper");
        assert_eq!(section.attr.0, "inner");
        assert!(section.attr.1.contains(&"section".to_string()));
    }

    /// Case A: bare Div, one header-led run → absorbed, classes merged.
    #[test]
    fn test_sectionize_absorbs_bare_div_into_section() {
        let blocks = vec![make_div(
            "",
            vec!["wrap-a"],
            vec![],
            vec![
                make_header(3, "case-a", vec![], "Case A"),
                make_para("Body"),
            ],
        )];
        let result = sectionize_blocks(blocks);
        assert_eq!(result.len(), 1, "the Div is replaced, not nested");
        let section = as_div(&result[0], "section");
        assert_eq!(section.attr.0, "case-a", "the header's id wins");
        assert_eq!(
            section.attr.1,
            vec![
                "section".to_string(),
                "level3".to_string(),
                "wrap-a".to_string()
            ],
            "the Div's classes are merged after the section markers"
        );
    }

    /// Case B: a Div with its own id is *not* absorbed.
    #[test]
    fn test_sectionize_does_not_absorb_div_with_id() {
        let blocks = vec![make_div(
            "has-id",
            vec!["wrap-b"],
            vec![],
            vec![make_header(3, "case-b", vec![], "Case B")],
        )];
        let result = sectionize_blocks(blocks);
        let outer = as_div(&result[0], "the surviving wrapper");
        assert_eq!(outer.attr.0, "has-id");
        assert!(!outer.attr.1.contains(&"section".to_string()));
        assert_eq!(outer.content.len(), 1);
        let inner = as_div(&outer.content[0], "the nested section");
        assert_eq!(inner.attr.0, "case-b");
        assert!(inner.attr.1.contains(&"section".to_string()));
    }

    /// Case C: content before the first header blocks absorption.
    #[test]
    fn test_sectionize_does_not_absorb_div_with_leading_content() {
        let blocks = vec![make_div(
            "",
            vec!["wrap-c"],
            vec![],
            vec![
                make_para("Leading"),
                make_header(3, "case-c", vec![], "Case C"),
            ],
        )];
        let result = sectionize_blocks(blocks);
        let outer = as_div(&result[0], "the surviving wrapper");
        assert_eq!(outer.attr.1, vec!["wrap-c".to_string()]);
        assert_eq!(outer.content.len(), 2, "leading para + one section");
        assert!(matches!(outer.content[0], Block::Paragraph(_)));
        assert!(
            as_div(&outer.content[1], "nested section")
                .attr
                .1
                .contains(&"section".to_string())
        );
    }

    /// Case D: two sibling sections cannot collapse into one.
    #[test]
    fn test_sectionize_does_not_absorb_div_with_two_sections() {
        let blocks = vec![make_div(
            "",
            vec!["wrap-d"],
            vec![],
            vec![
                make_header(3, "d-one", vec![], "D one"),
                make_header(3, "d-two", vec![], "D two"),
            ],
        )];
        let result = sectionize_blocks(blocks);
        let outer = as_div(&result[0], "the surviving wrapper");
        assert_eq!(outer.attr.1, vec!["wrap-d".to_string()]);
        assert_eq!(outer.content.len(), 2, "two sibling sections");
        for (i, child) in outer.content.iter().enumerate() {
            assert!(
                as_div(child, "a nested section")
                    .attr
                    .1
                    .contains(&"section".to_string()),
                "child {i} is sectionized even though the Div is not absorbed"
            );
        }
    }

    /// Case E: key-value attributes are merged on absorption.
    #[test]
    fn test_sectionize_absorb_merges_key_values() {
        let blocks = vec![make_div(
            "",
            vec!["wrap-e"],
            vec![("data-thing", "x")],
            vec![make_header(3, "case-e", vec![], "Case E")],
        )];
        let result = sectionize_blocks(blocks);
        let section = as_div(&result[0], "section");
        assert!(
            section.attr.1.contains(&"section".to_string()),
            "the Div was absorbed, so what remains is the section itself"
        );
        assert_eq!(section.attr.0, "case-e");
        assert_eq!(
            section.attr.2.get("data-thing").map(String::as_str),
            Some("x")
        );
        assert!(section.attr.1.contains(&"wrap-e".to_string()));
    }

    /// An already-sectionized Div is left alone — sectionize must be
    /// idempotent, and a `section` Div is not a candidate for absorbing.
    #[test]
    fn test_sectionize_is_idempotent_over_divs() {
        let blocks = vec![make_div(
            "",
            vec!["wrap"],
            vec![],
            vec![make_header(3, "inner", vec![], "Inner")],
        )];
        let once = sectionize_blocks(blocks);
        let twice = sectionize_blocks(once.clone());
        assert_eq!(format!("{once:?}"), format!("{twice:?}"));
    }

    /// pandoc's `makeSections` does not descend into a `BlockQuote`, so a
    /// heading there stays a bare Header (and thus out of the TOC).
    #[test]
    fn test_sectionize_does_not_descend_into_blockquote() {
        let blocks = vec![Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
            content: vec![make_header(3, "quoted", vec![], "Quoted")],
            source_info: dummy_source_info(),
        })];
        let result = sectionize_blocks(blocks);
        let Block::BlockQuote(bq) = &result[0] else {
            panic!("blockquote survives")
        };
        assert!(
            matches!(bq.content[0], Block::Header(_)),
            "the heading stays a bare Header"
        );
    }

    #[test]
    fn test_sectionize_class_order() {
        // Classes should be: "section", "levelN", then user classes
        let blocks = vec![
            make_header(2, "sec", vec!["alpha", "beta"], "Section"),
            make_para("Content."),
        ];

        let result = sectionize_blocks(blocks);

        let Block::Div(section) = &result[0] else {
            panic!("Expected Div");
        };
        let classes = &section.attr.1;

        // Verify order: section, level2, alpha, beta
        assert_eq!(classes[0], "section");
        assert_eq!(classes[1], "level2");
        assert_eq!(classes[2], "alpha");
        assert_eq!(classes[3], "beta");
    }
}
