/*
 * language_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process language_attribute to format it with braces
pub fn process_language_attribute(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                return PandocNativeIntermediate::IntermediateBaseText(
                    "{".to_string() + &text + "}",
                    range,
                );
            }
            _ => {}
        }
    }
    panic!("Expected language_attribute to have a language, but found none");
}
