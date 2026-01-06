/*
 * stage/traits.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * PipelineStage trait definition.
 */

//! Pipeline stage trait.
//!
//! The [`PipelineStage`] trait defines the interface for all pipeline stages.
//! Stages are the building blocks of the render pipeline, each responsible
//! for a specific transformation step.

use async_trait::async_trait;

use super::context::StageContext;
use super::data::{PipelineData, PipelineDataKind};
use super::error::PipelineError;

/// A single stage in the render pipeline.
///
/// Stages transform [`PipelineData`] from one form to another.
/// Each stage declares its expected input and output types, enabling
/// runtime validation of pipeline composition.
///
/// # Design Philosophy
///
/// - **Stages are unconditional**: They always run when included in a pipeline.
///   All conditional logic (engine selection, format branching) lives in the
///   `PipelinePlanner` which constructs the pipeline.
///
/// - **Stages can hold configuration**: For example, `AstTransforms` holds a
///   `TransformPipeline`. However, stages should not hold mutable state between
///   executions - all mutable state goes in `StageContext`.
///
/// - **Stages are async**: Even simple stages are async to enable consistent
///   pipeline execution. The overhead is negligible given Rust's zero-cost
///   abstractions.
///
/// # Thread Safety
///
/// Stages must be `Send + Sync` to support:
/// - Parallel rendering of multiple documents
/// - Storing stages in `Arc<Vec<Box<dyn PipelineStage>>>`
///
/// # Example
///
/// ```ignore
/// use async_trait::async_trait;
/// use quarto_core::stage::{
///     PipelineStage, PipelineData, PipelineDataKind, PipelineError, StageContext,
/// };
///
/// pub struct MyTransform;
///
/// #[async_trait]
/// impl PipelineStage for MyTransform {
///     fn name(&self) -> &str { "my-transform" }
///     fn input_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }
///     fn output_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }
///
///     async fn run(
///         &self,
///         input: PipelineData,
///         ctx: &mut StageContext,
///     ) -> Result<PipelineData, PipelineError> {
///         let PipelineData::DocumentAst(mut doc) = input else {
///             return Err(PipelineError::unexpected_input(
///                 self.name(),
///                 self.input_kind(),
///                 input.kind(),
///             ));
///         };
///
///         // Transform the AST...
///
///         Ok(PipelineData::DocumentAst(doc))
///     }
/// }
/// ```
#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Human-readable name for logging/debugging.
    ///
    /// This name appears in:
    /// - Log messages and traces
    /// - Error messages when the stage fails
    /// - Pipeline validation error messages
    fn name(&self) -> &str;

    /// What input type this stage expects.
    ///
    /// Used for runtime validation of pipeline composition.
    fn input_kind(&self) -> PipelineDataKind;

    /// What output type this stage produces.
    ///
    /// Used for runtime validation of pipeline composition.
    fn output_kind(&self) -> PipelineDataKind;

    /// Run the stage.
    ///
    /// Transforms the input data and returns the output.
    ///
    /// # Arguments
    ///
    /// * `input` - The input data for this stage
    /// * `ctx` - The stage context providing:
    ///   - Owned data (format, project, document info)
    ///   - Mutable artifact storage
    ///   - Observer for tracing/progress
    ///   - Cancellation token
    ///
    /// # Returns
    ///
    /// * `Ok(PipelineData)` - The transformed data
    /// * `Err(PipelineError)` - If the stage fails
    ///
    /// # Cancellation
    ///
    /// Long-running stages should periodically check `ctx.is_cancelled()`
    /// and return `Err(PipelineError::Cancelled)` if true.
    ///
    /// # Warnings
    ///
    /// Non-fatal issues should be added to `ctx.warnings` rather than
    /// returning an error.
    ///
    /// # Note on Naming
    ///
    /// We use "run" instead of "execute" to avoid confusion with
    /// Quarto's engine execution (Jupyter, Knitr).
    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple passthrough stage for testing
    struct PassthroughStage {
        name: &'static str,
        input: PipelineDataKind,
        output: PipelineDataKind,
    }

    #[async_trait]
    impl PipelineStage for PassthroughStage {
        fn name(&self) -> &str {
            self.name
        }

        fn input_kind(&self) -> PipelineDataKind {
            self.input
        }

        fn output_kind(&self) -> PipelineDataKind {
            self.output
        }

        async fn run(
            &self,
            input: PipelineData,
            _ctx: &mut StageContext,
        ) -> Result<PipelineData, PipelineError> {
            // Just pass through the data
            Ok(input)
        }
    }

    /// A stage that always fails
    struct FailingStage;

    #[async_trait]
    impl PipelineStage for FailingStage {
        fn name(&self) -> &str {
            "failing"
        }

        fn input_kind(&self) -> PipelineDataKind {
            PipelineDataKind::LoadedSource
        }

        fn output_kind(&self) -> PipelineDataKind {
            PipelineDataKind::LoadedSource
        }

        async fn run(
            &self,
            _input: PipelineData,
            _ctx: &mut StageContext,
        ) -> Result<PipelineData, PipelineError> {
            Err(PipelineError::stage_error("failing", "Intentional failure"))
        }
    }

    #[test]
    fn test_stage_metadata() {
        let stage = PassthroughStage {
            name: "test-stage",
            input: PipelineDataKind::LoadedSource,
            output: PipelineDataKind::DocumentSource,
        };

        assert_eq!(stage.name(), "test-stage");
        assert_eq!(stage.input_kind(), PipelineDataKind::LoadedSource);
        assert_eq!(stage.output_kind(), PipelineDataKind::DocumentSource);
    }

    // Note: Async tests would require tokio test runtime.
    // The actual async behavior is tested in the pipeline tests.
}
