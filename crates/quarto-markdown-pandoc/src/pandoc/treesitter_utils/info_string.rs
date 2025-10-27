/*
 * info_string.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use std::collections::HashMap;

/// Process info_string to extract language as an attribute
pub fn process_info_string(
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                // Track source location for the language specifier
                let lang_source =
                    crate::pandoc::source_map_compat::range_to_source_info_with_context(
                        &range, context,
                    );

                let mut attr_source = AttrSourceInfo::empty();
                attr_source.classes.push(Some(lang_source));

                return PandocNativeIntermediate::IntermediateAttr(
                    ("".to_string(), vec![text], HashMap::new()),
                    attr_source,
                );
            }
            _ => {}
        }
    }
    panic!("Expected info_string to have a string, but found none");
}
