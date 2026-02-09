/*
 * stage/pipeline.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline struct for executing stage sequences.
 */

//! Pipeline execution.
//!
//! The [`Pipeline`] struct represents a validated sequence of stages
//! that can be executed together. It handles:
//!
//! - Stage composition validation
//! - Sequential execution with observer notifications
//! - Cancellation support

use super::context::StageContext;
use super::data::{PipelineData, PipelineDataKind};
use super::error::{PipelineError, PipelineValidationError};
use super::traits::PipelineStage;

/// A validated sequence of pipeline stages.
///
/// The pipeline validates that stages compose correctly at construction
/// time, ensuring that each stage's output type matches the next stage's
/// input type.
///
/// # Example
///
/// ```ignore
/// use quarto_core::stage::{Pipeline, PipelineStage};
///
/// // Create stages
/// let stages: Vec<Box<dyn PipelineStage>> = vec![
///     Box::new(LoadSourceStage),
///     Box::new(ParseDocumentStage),
///     Box::new(TransformAstStage),
///     Box::new(RenderHtmlStage),
/// ];
///
/// // Create and validate pipeline
/// let pipeline = Pipeline::new(stages)?;
///
/// // Execute
/// let result = pipeline.run(input, &mut ctx).await?;
/// ```
pub struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
    expected_input: PipelineDataKind,
    expected_output: PipelineDataKind,
}

impl Pipeline {
    /// Create a new pipeline from stages.
    ///
    /// Validates that stages compose correctly:
    /// - Pipeline must have at least one stage
    /// - Each stage's output type must match the next stage's input type
    ///
    /// # Errors
    ///
    /// Returns [`PipelineValidationError`] if:
    /// - The stages vector is empty
    /// - Adjacent stages have incompatible types
    pub fn new(stages: Vec<Box<dyn PipelineStage>>) -> Result<Self, PipelineValidationError> {
        if stages.is_empty() {
            return Err(PipelineValidationError::Empty);
        }

        // Validate composition
        for window in stages.windows(2) {
            let output = window[0].output_kind();
            let input = window[1].input_kind();
            if output != input {
                return Err(PipelineValidationError::TypeMismatch {
                    stage_a: window[0].name().to_string(),
                    stage_b: window[1].name().to_string(),
                    output,
                    input,
                });
            }
        }

        let expected_input = stages.first().unwrap().input_kind();
        let expected_output = stages.last().unwrap().output_kind();

        Ok(Self {
            stages,
            expected_input,
            expected_output,
        })
    }

    /// What input type this pipeline expects.
    pub fn expected_input(&self) -> PipelineDataKind {
        self.expected_input
    }

    /// What output type this pipeline produces.
    pub fn expected_output(&self) -> PipelineDataKind {
        self.expected_output
    }

    /// Get the number of stages in the pipeline.
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Check if the pipeline is empty (should never be true after construction).
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Get stage names for debugging.
    pub fn stage_names(&self) -> Vec<&str> {
        self.stages.iter().map(|s| s.name()).collect()
    }

    /// Run the pipeline.
    ///
    /// Executes all stages in sequence, passing the output of each
    /// stage as input to the next.
    ///
    /// # Cancellation
    ///
    /// Checks for cancellation before each stage. If cancelled, returns
    /// `Err(PipelineError::Cancelled)`.
    ///
    /// # Observer Notifications
    ///
    /// Notifies the context's observer before and after each stage,
    /// and on error.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Input type doesn't match expected type
    /// - Any stage fails
    /// - Pipeline is cancelled
    pub async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        // Validate input type
        if input.kind() != self.expected_input {
            return Err(PipelineError::unexpected_input(
                "pipeline",
                self.expected_input,
                input.kind(),
            ));
        }

        let total = self.stages.len();
        ctx.observer.on_pipeline_start(total);

        let mut data = input;

        for (idx, stage) in self.stages.iter().enumerate() {
            // Check cancellation before each stage
            if ctx.cancellation.is_cancelled() {
                let err = PipelineError::Cancelled;
                ctx.observer.on_pipeline_error(&err);
                return Err(err);
            }

            ctx.observer.on_stage_start(stage.name(), idx, total);

            match stage.run(data, ctx).await {
                Ok(output) => {
                    ctx.observer.on_stage_complete(stage.name(), idx, total);
                    data = output;
                }
                Err(e) => {
                    ctx.observer.on_stage_error(stage.name(), idx, &e);
                    ctx.observer.on_pipeline_error(&e);
                    return Err(e);
                }
            }
        }

        ctx.observer.on_pipeline_complete();
        Ok(data)
    }
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("stages", &self.stage_names())
            .field("expected_input", &self.expected_input)
            .field("expected_output", &self.expected_output)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// A simple test stage that transforms between types
    struct TestStage {
        name: &'static str,
        input: PipelineDataKind,
        output: PipelineDataKind,
    }

    #[async_trait]
    impl PipelineStage for TestStage {
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
            Ok(input)
        }
    }

    /// A stage that always fails
    #[allow(dead_code)]
    struct FailingStage {
        name: &'static str,
    }

    #[async_trait]
    impl PipelineStage for FailingStage {
        fn name(&self) -> &str {
            self.name
        }

        fn input_kind(&self) -> PipelineDataKind {
            PipelineDataKind::LoadedSource
        }

        fn output_kind(&self) -> PipelineDataKind {
            PipelineDataKind::DocumentSource
        }

        async fn run(
            &self,
            _input: PipelineData,
            _ctx: &mut StageContext,
        ) -> Result<PipelineData, PipelineError> {
            Err(PipelineError::stage_error(self.name, "Test failure"))
        }
    }

    // === Validation Tests ===

    #[test]
    fn test_empty_pipeline() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![];
        let result = Pipeline::new(stages);
        assert!(matches!(result, Err(PipelineValidationError::Empty)));
    }

    #[test]
    fn test_single_stage_pipeline() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![Box::new(TestStage {
            name: "only-stage",
            input: PipelineDataKind::LoadedSource,
            output: PipelineDataKind::DocumentSource,
        })];

        let pipeline = Pipeline::new(stages).unwrap();

        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::DocumentSource);
        assert_eq!(pipeline.stage_names(), vec!["only-stage"]);
    }

    #[test]
    fn test_multi_stage_pipeline_valid() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(TestStage {
                name: "load",
                input: PipelineDataKind::LoadedSource,
                output: PipelineDataKind::DocumentSource,
            }),
            Box::new(TestStage {
                name: "parse",
                input: PipelineDataKind::DocumentSource,
                output: PipelineDataKind::DocumentAst,
            }),
            Box::new(TestStage {
                name: "render",
                input: PipelineDataKind::DocumentAst,
                output: PipelineDataKind::RenderedOutput,
            }),
        ];

        let pipeline = Pipeline::new(stages).unwrap();

        assert_eq!(pipeline.len(), 3);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::RenderedOutput);
        assert_eq!(pipeline.stage_names(), vec!["load", "parse", "render"]);
    }

    #[test]
    fn test_multi_stage_pipeline_type_mismatch() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(TestStage {
                name: "load",
                input: PipelineDataKind::LoadedSource,
                output: PipelineDataKind::DocumentSource,
            }),
            Box::new(TestStage {
                name: "render",
                // Wrong! Should expect DocumentSource, not DocumentAst
                input: PipelineDataKind::DocumentAst,
                output: PipelineDataKind::RenderedOutput,
            }),
        ];

        let result = Pipeline::new(stages);
        assert!(matches!(
            result,
            Err(PipelineValidationError::TypeMismatch { .. })
        ));

        if let Err(PipelineValidationError::TypeMismatch {
            stage_a,
            stage_b,
            output,
            input,
        }) = result
        {
            assert_eq!(stage_a, "load");
            assert_eq!(stage_b, "render");
            assert_eq!(output, PipelineDataKind::DocumentSource);
            assert_eq!(input, PipelineDataKind::DocumentAst);
        }
    }

    #[test]
    fn test_pipeline_debug() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![Box::new(TestStage {
            name: "test",
            input: PipelineDataKind::LoadedSource,
            output: PipelineDataKind::LoadedSource,
        })];

        let pipeline = Pipeline::new(stages).unwrap();
        let debug = format!("{:?}", pipeline);

        assert!(debug.contains("Pipeline"));
        assert!(debug.contains("test"));
        assert!(debug.contains("LoadedSource"));
    }

    // Note: Async run tests would require tokio test runtime.
    // Those are covered in integration tests.
}
