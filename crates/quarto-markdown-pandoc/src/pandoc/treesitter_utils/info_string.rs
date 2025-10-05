/*
 * info_string.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use std::collections::HashMap;

/// Process info_string to extract language as an attribute
pub fn process_info_string(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBaseText(text, _) => {
                return PandocNativeIntermediate::IntermediateAttr((
                    "".to_string(),
                    vec![text],
                    HashMap::new(),
                ));
            }
            _ => {}
        }
    }
    panic!("Expected info_string to have a string, but found none");
}
