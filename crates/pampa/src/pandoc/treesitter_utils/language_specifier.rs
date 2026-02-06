/*
 * language_specifier.rs
 *
 * Functions for processing language_specifier nodes in the tree-sitter AST.
 * Handles the conversion of `{language #id .class key=value}` syntax to
 * proper Pandoc attributes.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::location::range_to_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a language_specifier node that may contain additional commonmark attributes.
///
/// The grammar allows:
/// - `{python}` - just a language
/// - `{python #id}` - language with id
/// - `{python .class}` - language with class
/// - `{python #id .class key=value}` - language with full attributes
///
/// This function extracts the language token and merges it with any commonmark attributes
/// found in the children, producing a single IntermediateAttr.
pub fn process_language_specifier(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // Check if there's a commonmark_specifier child
    let commonmark_attr = children
        .iter()
        .find(|(name, _)| name == "commonmark_specifier")
        .map(|(_, child)| child.clone());

    match commonmark_attr {
        None => {
            // No commonmark_specifier - return the full text as IntermediateBaseText
            // This preserves the existing behavior for simple {python} cases
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            let range = crate::pandoc::location::node_location(node);
            PandocNativeIntermediate::IntermediateBaseText(text, range)
        }
        Some(PandocNativeIntermediate::IntermediateAttr(attr, attr_source)) => {
            // We have a commonmark_specifier with attributes
            // Need to extract just the language portion from the node text

            // Find the commonmark_specifier child node to get its byte position
            let commonmark_child = find_named_child(node, "commonmark_specifier");

            let language = match commonmark_child {
                Some(cm_node) => {
                    // Extract text from language_specifier start to commonmark_specifier start
                    let lang_start = node.start_byte();
                    let lang_end = cm_node.start_byte();
                    let lang_text = std::str::from_utf8(&input_bytes[lang_start..lang_end])
                        .unwrap()
                        .trim()
                        .to_string();
                    lang_text
                }
                None => {
                    // Fallback: shouldn't happen, but handle gracefully
                    node.utf8_text(input_bytes).unwrap().to_string()
                }
            };

            // Build the combined attributes:
            // - id: from commonmark_specifier
            // - classes: [{language}, ...classes from commonmark_specifier]
            // - attributes: from commonmark_specifier

            let (cm_id, cm_classes, cm_attrs) = attr;

            // Create the language class wrapped in braces for roundtripping
            let language_class = format!("{{{}}}", language);

            // Build combined classes list
            let mut classes = vec![language_class];
            classes.extend(cm_classes);

            // Build the AttrSourceInfo
            // The language source is from the start of language_specifier to the start of commonmark_specifier
            let lang_source = match find_named_child(node, "commonmark_specifier") {
                Some(cm_node) => {
                    let lang_range = quarto_source_map::Range {
                        start: quarto_source_map::Location {
                            offset: node.start_byte(),
                            row: node.start_position().row,
                            column: node.start_position().column,
                        },
                        end: quarto_source_map::Location {
                            offset: cm_node.start_byte(),
                            row: cm_node.start_position().row,
                            column: cm_node.start_position().column,
                        },
                    };
                    Some(range_to_source_info_with_context(&lang_range, context))
                }
                None => None,
            };

            // Combine class sources: [language_source, ...commonmark_class_sources]
            let mut class_sources = vec![lang_source];
            class_sources.extend(attr_source.classes);

            let combined_attr_source = AttrSourceInfo {
                id: attr_source.id,
                classes: class_sources,
                attributes: attr_source.attributes,
            };

            PandocNativeIntermediate::IntermediateAttr(
                (cm_id, classes, cm_attrs),
                combined_attr_source,
            )
        }
        Some(other) => {
            // Unexpected intermediate type from commonmark_specifier
            // Fall back to returning the whole text
            eprintln!(
                "Warning: unexpected intermediate type in language_specifier: {:?}",
                other
            );
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            let range = crate::pandoc::location::node_location(node);
            PandocNativeIntermediate::IntermediateBaseText(text, range)
        }
    }
}

/// Find a named child of a tree-sitter node by name.
fn find_named_child<'a>(node: &'a tree_sitter::Node, name: &str) -> Option<tree_sitter::Node<'a>> {
    for i in 0..node.named_child_count() as u32 {
        if let Some(child) = node.named_child(i) {
            if child.kind() == name {
                return Some(child);
            }
        }
    }
    None
}

/// Process a language_specifier that contains a nested language_specifier (the `{{python}}` case).
/// This handles the recursive grammar rule: `seq('{', $.language_specifier, '}')`
pub fn process_nested_language_specifier(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    // Look for the inner language_specifier child
    for (name, child) in children {
        if name == "language_specifier" {
            return child;
        }
    }
    // Fallback - shouldn't happen
    use hashlink::LinkedHashMap;
    PandocNativeIntermediate::IntermediateAttr(
        (String::new(), vec![], LinkedHashMap::new()),
        AttrSourceInfo::empty(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_named_child_found() {
        // This would need a proper tree-sitter setup to test
        // For now, we just verify the function compiles
    }
}
