/*
 * engine/traits.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * ExecutionEngine trait definition.
 */

//! ExecutionEngine trait for code execution in Quarto documents.

use std::path::{Path, PathBuf};

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;

/// Execution engine for code cells in Quarto documents.
///
/// Engines transform markdown with executable code cells into markdown
/// with execution outputs. The transformation is text-in/text-out.
///
/// # Thread Safety
///
/// Engines must be `Send + Sync` for use in async pipeline contexts
/// and potential parallel rendering of multiple documents.
///
/// # Implementation Notes
///
/// - The `execute` method receives QMD text and returns QMD text with
///   code cell outputs expanded.
/// - Engines should preserve all non-code content unchanged.
/// - Supporting files (figures, data) should be written to the filesystem
///   and their paths included in the result.
/// - The markdown engine is a no-op that returns input unchanged.
///
/// # Example
///
/// ```ignore
/// use quarto_core::engine::{ExecutionEngine, ExecutionContext, ExecuteResult};
///
/// struct MyEngine;
///
/// impl ExecutionEngine for MyEngine {
///     fn name(&self) -> &str {
///         "my-engine"
///     }
///
///     fn execute(
///         &self,
///         input: &str,
///         ctx: &ExecutionContext,
///     ) -> Result<ExecuteResult, ExecutionError> {
///         // Process code cells and return result
///         Ok(ExecuteResult::new(processed_markdown))
///     }
/// }
/// ```
pub trait ExecutionEngine: Send + Sync {
    /// Human-readable name for this engine.
    ///
    /// This is used for:
    /// - Matching against `engine:` declarations in document metadata
    /// - Log messages and diagnostics
    /// - Registry lookup
    ///
    /// Standard names: "markdown", "knitr", "jupyter"
    fn name(&self) -> &str;

    /// Execute code cells in the input content.
    ///
    /// # Arguments
    ///
    /// * `input` - QMD text with executable code cells
    /// * `ctx` - Execution context with paths, config, and options
    ///
    /// # Returns
    ///
    /// `ExecuteResult` containing:
    /// - Transformed markdown with execution outputs
    /// - Paths to any supporting files created
    /// - Filters to apply during rendering
    /// - Content to inject into the document
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError` if:
    /// - The engine runtime is not available
    /// - Code execution fails
    /// - IO operations fail
    fn execute(&self, input: &str, ctx: &ExecutionContext)
    -> Result<ExecuteResult, ExecutionError>;

    /// Whether this engine supports freeze/thaw caching.
    ///
    /// If true, execution results can be cached in the `_freeze/`
    /// directory and reused on subsequent renders when source
    /// code hasn't changed.
    ///
    /// Default: `false`
    fn can_freeze(&self) -> bool {
        false
    }

    /// Get intermediate files produced by this engine.
    ///
    /// These files may need to be cleaned up after rendering completes.
    /// For example, knitr produces `{input}_files/` directories.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input document
    ///
    /// # Returns
    ///
    /// Paths to intermediate files/directories that may exist.
    fn intermediate_files(&self, _input_path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Check if this engine is available in the current environment.
    ///
    /// This checks whether the required runtime (R, Python, etc.)
    /// is installed and accessible.
    ///
    /// Default: `true` (assume available)
    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal test engine for verification
    struct TestEngine {
        name: &'static str,
        available: bool,
    }

    impl ExecutionEngine for TestEngine {
        fn name(&self) -> &str {
            self.name
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            Ok(ExecuteResult::passthrough(input))
        }

        fn is_available(&self) -> bool {
            self.available
        }
    }

    #[test]
    fn test_engine_trait_name() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        assert_eq!(engine.name(), "test");
    }

    #[test]
    fn test_engine_trait_default_can_freeze() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        assert!(!engine.can_freeze());
    }

    #[test]
    fn test_engine_trait_default_intermediate_files() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        let files = engine.intermediate_files(Path::new("/test.qmd"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_engine_trait_is_available() {
        let available = TestEngine {
            name: "test",
            available: true,
        };
        let unavailable = TestEngine {
            name: "test",
            available: false,
        };

        assert!(available.is_available());
        assert!(!unavailable.is_available());
    }

    #[test]
    fn test_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TestEngine>();
    }
}
