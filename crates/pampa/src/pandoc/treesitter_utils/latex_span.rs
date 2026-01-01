/*
 * latex_span.rs
 *
 * Functions for processing latex span nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, Math, MathType};
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_latex_span(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut is_inline_math = false;
    let mut is_display_math = false;
    let mut inlines: Vec<_> = children
        .into_iter()
        .filter(|(_, child)| {
            if matches!(
                child,
                PandocNativeIntermediate::IntermediateLatexInlineDelimiter(_)
            ) {
                is_inline_math = true;
                false // skip the delimiter
            } else if matches!(
                child,
                PandocNativeIntermediate::IntermediateLatexDisplayDelimiter(_)
            ) {
                is_display_math = true;
                false // skip the delimiter
            } else {
                true // keep other nodes
            }
        })
        .collect();
    assert!(
        inlines.len() == 1,
        "Expected exactly one inline in latex_span, got {}",
        inlines.len()
    );
    if is_inline_math && is_display_math {
        panic!("Unexpected both inline and display math in latex_span");
    }
    if !is_inline_math && !is_display_math {
        panic!("Expected either inline or display math in latex_span, got neither");
    }
    let math_type = if is_inline_math {
        MathType::InlineMath
    } else {
        MathType::DisplayMath
    };
    let (_, child) = inlines.remove(0);
    let PandocNativeIntermediate::IntermediateBaseText(text, _) = child else {
        panic!("Expected BaseText in latex_span, got {:?}", child)
    };
    PandocNativeIntermediate::IntermediateInline(Inline::Math(Math {
        math_type,
        text,
        source_info: node_source_info_with_context(node, context),
    }))
}
