//! Analysis transforms that run at "LSP speed".
//!
//! Analysis transforms are AST transformations that can run quickly without
//! performing I/O, code execution, or other slow operations. They are suitable
//! for use in the LSP server for real-time feedback.
//!
//! # Available Transforms
//!
//! - [`MetaShortcodeTransform`] - Resolves `{{< meta key >}}` shortcodes

mod shortcode;

pub use shortcode::MetaShortcodeTransform;

use crate::AnalysisContext;
use quarto_pandoc_types::pandoc::Pandoc;

/// Error type for analysis transforms.
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    /// An error occurred during transformation.
    #[error("Transform error: {0}")]
    Transform(String),
}

/// Result type for analysis transforms.
pub type Result<T> = std::result::Result<T, TransformError>;

/// Trait for analysis transforms.
///
/// Analysis transforms modify the Pandoc AST based on document metadata
/// and structure. They should be fast and not perform I/O.
pub trait AnalysisTransform: Send + Sync {
    /// Name of the transform (for debugging/logging).
    fn name(&self) -> &str;

    /// Apply the transform to the AST.
    ///
    /// The transform may:
    /// - Modify the AST in place
    /// - Report diagnostics via `ctx.add_diagnostic()`
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails in an unrecoverable way.
    /// Recoverable issues (like missing metadata keys) should be reported
    /// as diagnostics rather than errors.
    fn transform(&self, pandoc: &mut Pandoc, ctx: &mut dyn AnalysisContext) -> Result<()>;
}

/// Run a sequence of analysis transforms on a document.
///
/// Transforms are applied in order. If any transform fails with an error,
/// the function returns early with that error.
///
/// # Example
///
/// ```rust,ignore
/// use quarto_analysis::transforms::{run_analysis_transforms, MetaShortcodeTransform};
///
/// let transforms: Vec<&dyn AnalysisTransform> = vec![&MetaShortcodeTransform];
/// run_analysis_transforms(&mut pandoc, &mut ctx, &transforms)?;
/// ```
pub fn run_analysis_transforms(
    pandoc: &mut Pandoc,
    ctx: &mut dyn AnalysisContext,
    transforms: &[&dyn AnalysisTransform],
) -> Result<()> {
    for transform in transforms {
        transform.transform(pandoc, ctx)?;
    }
    Ok(())
}
