/*
 * engine/markdown.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Markdown engine - no code execution.
 */

//! Markdown engine (no code execution).
//!
//! The markdown engine is a no-op engine that passes content through unchanged.
//! It is used when:
//!
//! - The document has no executable code cells
//! - The document explicitly declares `engine: markdown`
//! - No other engine is detected or available
//!
//! This engine is always available, including in WASM builds.

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// Markdown engine - passes content through unchanged.
///
/// This is the default engine used when no computation is needed.
/// It simply returns the input markdown without modification.
///
/// # Characteristics
///
/// - Always available (no external dependencies)
/// - Does not support freeze/thaw (nothing to cache)
/// - No intermediate files produced
/// - Works in both native and WASM builds
#[derive(Debug, Clone, Default)]
pub struct MarkdownEngine;

impl MarkdownEngine {
    /// Create a new markdown engine instance.
    pub fn new() -> Self {
        Self
    }
}

impl ExecutionEngine for MarkdownEngine {
    fn name(&self) -> &str {
        "markdown"
    }

    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        // No execution - return input unchanged
        Ok(ExecuteResult::passthrough(input))
    }

    fn can_freeze(&self) -> bool {
        // Nothing to freeze - content is unchanged
        false
    }

    fn is_available(&self) -> bool {
        // Always available
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_context() -> ExecutionContext {
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
    }

    #[test]
    fn test_markdown_engine_name() {
        let engine = MarkdownEngine::new();
        assert_eq!(engine.name(), "markdown");
    }

    #[test]
    fn test_markdown_engine_always_available() {
        let engine = MarkdownEngine::new();
        assert!(engine.is_available());
    }

    #[test]
    fn test_markdown_engine_cannot_freeze() {
        let engine = MarkdownEngine::new();
        assert!(!engine.can_freeze());
    }

    #[test]
    fn test_markdown_engine_no_intermediate_files() {
        let engine = MarkdownEngine::new();
        let files = engine.intermediate_files(std::path::Path::new("/test.qmd"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_markdown_engine_passthrough() {
        let engine = MarkdownEngine::new();
        let ctx = make_test_context();

        let input = r#"---
title: Test Document
---

# Hello World

This is a paragraph.

```{python}
print("Hello")
```
"#;

        let result = engine.execute(input, &ctx).unwrap();

        // Output should be identical to input
        assert_eq!(result.markdown, input);
        assert!(result.supporting_files.is_empty());
        assert!(result.filters.is_empty());
        assert!(!result.needs_postprocess);
    }

    #[test]
    fn test_markdown_engine_empty_input() {
        let engine = MarkdownEngine::new();
        let ctx = make_test_context();

        let result = engine.execute("", &ctx).unwrap();
        assert_eq!(result.markdown, "");
    }

    #[test]
    fn test_markdown_engine_preserves_all_content() {
        let engine = MarkdownEngine::new();
        let ctx = make_test_context();

        // Test with various markdown constructs
        let input = r#"# Heading

- List item 1
- List item 2

| Col1 | Col2 |
|------|------|
| a    | b    |

> Blockquote

```r
x <- 1:10
```

[Link](https://example.com)
"#;

        let result = engine.execute(input, &ctx).unwrap();
        assert_eq!(result.markdown, input);
    }

    #[test]
    fn test_markdown_engine_default_impl() {
        let engine = MarkdownEngine::default();
        assert_eq!(engine.name(), "markdown");
    }

    #[test]
    fn test_markdown_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MarkdownEngine>();
    }
}
