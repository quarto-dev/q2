/*
 * pandoc.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub use crate::block::Blocks;
pub use crate::config_value::ConfigValue;

/*
 * A data structure that mimics Pandoc's `data Pandoc` type.
 * This is used to represent the parsed structure of a Quarto Markdown document.
 */

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pandoc {
    /// Document metadata (frontmatter).
    ///
    /// This is a ConfigValue (usually a Map) containing the document's
    /// metadata from YAML frontmatter. Use `meta.get("key")` to access
    /// individual fields.
    pub meta: ConfigValue,
    pub blocks: Blocks,
}
