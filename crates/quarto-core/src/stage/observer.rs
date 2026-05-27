/*
 * stage/observer.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline observer for tracing, progress reporting, and WASM callbacks.
 */

//! Observer abstraction for pipeline execution events.
//!
//! The [`PipelineObserver`] trait provides a unified abstraction for:
//! - OpenTelemetry tracing (native builds with `otel` feature)
//! - Progress bar updates (CLI)
//! - JavaScript callbacks (WASM builds)
//!
//! This abstraction allows the pipeline to emit events without
//! depending on a specific observability implementation.

use super::data::PipelineData;
use super::error::PipelineError;

/// Event severity level for pipeline events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    /// Very detailed information for debugging
    Trace,
    /// Debug-level information
    Debug,
    /// Informational messages
    Info,
    /// Warnings that don't prevent execution
    Warn,
}

impl EventLevel {
    /// Convert to a string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventLevel::Trace => "trace",
            EventLevel::Debug => "debug",
            EventLevel::Info => "info",
            EventLevel::Warn => "warn",
        }
    }
}

/// Observer for pipeline execution events.
///
/// Implementations of this trait receive notifications about pipeline
/// execution progress, allowing for tracing, progress reporting, and
/// other observability features.
///
/// All methods have empty default implementations, allowing observers
/// to implement only the events they care about.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to support potential parallel
/// execution of multiple pipelines.
pub trait PipelineObserver: Send + Sync {
    /// Called when a stage begins execution.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name of the stage
    /// * `index` - Zero-based index of the stage in the pipeline
    /// * `total` - Total number of stages in the pipeline
    fn on_stage_start(&self, _name: &str, _index: usize, _total: usize) {}

    /// Called when a stage completes successfully.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name of the stage
    /// * `index` - Zero-based index of the stage in the pipeline
    /// * `total` - Total number of stages in the pipeline
    fn on_stage_complete(&self, _name: &str, _index: usize, _total: usize) {}

    /// Called when a stage fails.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name of the stage
    /// * `index` - Zero-based index of the stage in the pipeline
    /// * `error` - The error that caused the failure
    fn on_stage_error(&self, _name: &str, _index: usize, _error: &PipelineError) {}

    /// Called for arbitrary events during execution.
    ///
    /// Stages can emit custom events for detailed tracing.
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable event message
    /// * `level` - Severity level of the event
    fn on_event(&self, _message: &str, _level: EventLevel) {}

    /// Called when the pipeline starts execution.
    ///
    /// # Arguments
    ///
    /// * `total_stages` - Total number of stages in the pipeline
    fn on_pipeline_start(&self, _total_stages: usize) {}

    /// Called when the pipeline completes successfully.
    fn on_pipeline_complete(&self) {}

    /// Called when the pipeline fails.
    ///
    /// # Arguments
    ///
    /// * `error` - The error that caused the failure
    fn on_pipeline_error(&self, _error: &PipelineError) {}

    /// Called before the first stage with the pipeline input data.
    ///
    /// This allows observers to capture the initial state of the pipeline.
    ///
    /// # Arguments
    ///
    /// * `data` - The input data about to be processed by the first stage
    fn on_pipeline_input(&self, _data: &PipelineData) {}

    /// Called after a stage completes successfully, with the output data.
    ///
    /// This allows observers to inspect or record the data flowing through
    /// the pipeline at each stage boundary.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name of the stage that produced this data
    /// * `index` - Zero-based index of the stage in the pipeline
    /// * `data` - The output data produced by the stage
    fn on_stage_data(&self, _name: &str, _index: usize, _data: &PipelineData) {}

    /// Called after an AST transform completes within `AstTransformsStage`.
    ///
    /// This provides finer-grained tracing of the individual transforms
    /// (callouts, TOC, sectionize, etc.) that run inside the AST transforms
    /// pipeline stage.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the transform (e.g., "callout", "toc-generate")
    /// * `index` - Zero-based index of the transform in the pipeline
    /// * `total` - Total number of transforms
    /// * `ast` - The AST after this transform has been applied
    /// * `ast_context` - The AST context, carrying filename attribution and
    ///   source-info pool for the `ast`. Required so trace observers can emit
    ///   correct source metadata instead of falling back to anonymous files.
    fn on_transform_data(
        &self,
        _name: &str,
        _index: usize,
        _total: usize,
        _ast: &quarto_pandoc_types::pandoc::Pandoc,
        _ast_context: &pampa::pandoc::ASTContext,
    ) {
    }

    /// Called after each engine runs within `EngineExecutionStage`
    /// (bd-5yff4), giving finer-grained tracing of a multi-engine
    /// sequence: one AST snapshot per engine, in execution order.
    ///
    /// Mirrors [`Self::on_transform_data`] — trace observers record an
    /// `engine:<name>` entry so a debugger can step through the sequence
    /// (e.g. `engine:knitr`, then `engine:mermaidjs`). The single-engine
    /// case emits exactly one such entry, just before the stage's own
    /// final snapshot.
    ///
    /// # Arguments
    ///
    /// * `engine_name` - Name of the engine that just ran (e.g. `"knitr"`).
    /// * `index` - Zero-based position of the engine in the executed
    ///   sequence (markdown no-ops are not counted).
    /// * `ast` - The reconciled AST after this engine's output was merged.
    /// * `ast_context` - The AST context (multi-slot after the first
    ///   engine), carrying filename attribution and the source-info pool.
    fn on_engine_data(
        &self,
        _engine_name: &str,
        _index: usize,
        _ast: &quarto_pandoc_types::pandoc::Pandoc,
        _ast_context: &pampa::pandoc::ASTContext,
    ) {
    }

    /// Called by transforms that want to publish auxiliary structured data
    /// alongside the AST trace.
    ///
    /// This is how front-end transforms surface data they *build* but don't
    /// encode back into the AST — the crossref index, collected citation keys,
    /// etc. It's deliberately open-ended: observers that know the `kind` can
    /// record it; those that don't can ignore it.
    ///
    /// Invariants:
    /// - `kind` is a stable, well-known tag (e.g. `"CrossrefIndex"`). Pick one
    ///   per data type you plan to emit and document it near the producer.
    /// - `data` is JSON; callers serialize their structured value upfront so
    ///   the observer stays object-safe and doesn't need generics.
    ///
    /// # Arguments
    ///
    /// * `stage` - Name of the transform or stage that produced this data
    ///   (e.g., `"crossref-index"`). Used to scope the trace entry.
    /// * `index` - Zero-based index of the producer in its enclosing pipeline,
    ///   or `0` if that's not meaningful.
    /// * `kind` - Stable tag identifying the kind of data. Observers may
    ///   dispatch on this.
    /// * `data` - Serialized JSON value of the data.
    fn on_auxiliary_data(
        &self,
        _stage: &str,
        _index: usize,
        _kind: &str,
        _data: &serde_json::Value,
    ) {
    }
}

/// No-op observer implementation.
///
/// This observer does nothing, providing minimal overhead when
/// observability is not needed. It's the default observer used
/// when no other is specified.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopObserver;

impl NoopObserver {
    /// Create a new no-op observer.
    pub fn new() -> Self {
        Self
    }
}

impl PipelineObserver for NoopObserver {
    // All methods use default empty implementations
}

/// Tracing observer that emits `tracing` events.
///
/// This observer integrates with the Rust `tracing` ecosystem,
/// emitting events at appropriate levels. It can be used with
/// any `tracing` subscriber, including OpenTelemetry exporters.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingObserver;

impl TracingObserver {
    /// Create a new tracing observer.
    pub fn new() -> Self {
        Self
    }
}

impl PipelineObserver for TracingObserver {
    fn on_stage_start(&self, name: &str, index: usize, total: usize) {
        tracing::info!(
            stage.name = name,
            stage.index = index,
            stage.total = total,
            "Starting stage"
        );
    }

    fn on_stage_complete(&self, name: &str, index: usize, total: usize) {
        tracing::info!(
            stage.name = name,
            stage.index = index,
            stage.total = total,
            "Completed stage"
        );
    }

    fn on_stage_error(&self, name: &str, index: usize, error: &PipelineError) {
        tracing::error!(
            stage.name = name,
            stage.index = index,
            error = %error,
            "Stage failed"
        );
    }

    fn on_event(&self, message: &str, level: EventLevel) {
        match level {
            EventLevel::Trace => tracing::trace!("{}", message),
            EventLevel::Debug => tracing::debug!("{}", message),
            EventLevel::Info => tracing::info!("{}", message),
            EventLevel::Warn => tracing::warn!("{}", message),
        }
    }

    fn on_pipeline_start(&self, total_stages: usize) {
        tracing::info!(total_stages = total_stages, "Starting pipeline");
    }

    fn on_pipeline_complete(&self) {
        tracing::info!("Pipeline completed successfully");
    }

    fn on_pipeline_error(&self, error: &PipelineError) {
        tracing::error!(error = %error, "Pipeline failed");
    }
}

/// Macro for emitting events through a stage context's observer.
///
/// This provides a convenient way to emit events with proper
/// formatting while maintaining the abstraction boundary.
///
/// # Examples
///
/// ```ignore
/// trace_event!(ctx, EventLevel::Debug, "Processing {} blocks", block_count);
/// trace_event!(ctx, EventLevel::Info, "Rendered document to {}", output_path);
/// ```
#[macro_export]
macro_rules! trace_event {
    ($ctx:expr, $level:expr, $($arg:tt)*) => {{
        $ctx.observer.on_event(&format!($($arg)*), $level);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test observer that counts events including data callbacks
    struct CountingObserver {
        starts: AtomicUsize,
        completes: AtomicUsize,
        errors: AtomicUsize,
        events: AtomicUsize,
        pipeline_inputs: AtomicUsize,
        stage_data_calls: AtomicUsize,
        /// Records (stage_name, data_kind) for each on_stage_data call
        stage_data_log: std::sync::Mutex<Vec<(String, super::super::data::PipelineDataKind)>>,
        /// Records (stage, kind, data) for each on_auxiliary_data call
        aux_log: std::sync::Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl CountingObserver {
        fn new() -> Self {
            Self {
                starts: AtomicUsize::new(0),
                completes: AtomicUsize::new(0),
                errors: AtomicUsize::new(0),
                events: AtomicUsize::new(0),
                pipeline_inputs: AtomicUsize::new(0),
                stage_data_calls: AtomicUsize::new(0),
                stage_data_log: std::sync::Mutex::new(Vec::new()),
                aux_log: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl PipelineObserver for CountingObserver {
        fn on_stage_start(&self, _name: &str, _index: usize, _total: usize) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }

        fn on_stage_complete(&self, _name: &str, _index: usize, _total: usize) {
            self.completes.fetch_add(1, Ordering::SeqCst);
        }

        fn on_stage_error(&self, _name: &str, _index: usize, _error: &PipelineError) {
            self.errors.fetch_add(1, Ordering::SeqCst);
        }

        fn on_event(&self, _message: &str, _level: EventLevel) {
            self.events.fetch_add(1, Ordering::SeqCst);
        }

        fn on_pipeline_input(&self, _data: &PipelineData) {
            self.pipeline_inputs.fetch_add(1, Ordering::SeqCst);
        }

        fn on_stage_data(&self, name: &str, _index: usize, data: &PipelineData) {
            self.stage_data_calls.fetch_add(1, Ordering::SeqCst);
            self.stage_data_log
                .lock()
                .unwrap()
                .push((name.to_string(), data.kind()));
        }

        fn on_auxiliary_data(
            &self,
            stage: &str,
            _index: usize,
            kind: &str,
            data: &serde_json::Value,
        ) {
            self.aux_log
                .lock()
                .unwrap()
                .push((stage.to_string(), kind.to_string(), data.clone()));
        }
    }

    #[test]
    fn test_noop_observer() {
        use super::super::data::{LoadedSource, PipelineDataKind};

        let observer = NoopObserver::new();
        // These should all be no-ops
        observer.on_stage_start("test", 0, 1);
        observer.on_stage_complete("test", 0, 1);
        observer.on_stage_error("test", 0, &PipelineError::Cancelled);
        observer.on_event("test message", EventLevel::Info);
        observer.on_pipeline_start(5);
        observer.on_pipeline_complete();
        observer.on_pipeline_error(&PipelineError::Cancelled);

        // New data-bearing methods should also be no-ops
        let data = PipelineData::LoadedSource(LoadedSource::new(
            std::path::PathBuf::from("test.qmd"),
            vec![],
        ));
        observer.on_pipeline_input(&data);
        observer.on_stage_data("test", 0, &data);
        assert_eq!(data.kind(), PipelineDataKind::LoadedSource);
    }

    #[test]
    fn test_counting_observer() {
        let observer = Arc::new(CountingObserver::new());

        observer.on_stage_start("stage1", 0, 2);
        observer.on_stage_start("stage2", 1, 2);
        observer.on_stage_complete("stage1", 0, 2);
        observer.on_stage_error("stage2", 1, &PipelineError::Cancelled);
        observer.on_event("message", EventLevel::Debug);

        assert_eq!(observer.starts.load(Ordering::SeqCst), 2);
        assert_eq!(observer.completes.load(Ordering::SeqCst), 1);
        assert_eq!(observer.errors.load(Ordering::SeqCst), 1);
        assert_eq!(observer.events.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_event_level_as_str() {
        assert_eq!(EventLevel::Trace.as_str(), "trace");
        assert_eq!(EventLevel::Debug.as_str(), "debug");
        assert_eq!(EventLevel::Info.as_str(), "info");
        assert_eq!(EventLevel::Warn.as_str(), "warn");
    }

    #[test]
    fn test_counting_observer_data_callbacks() {
        use super::super::data::{LoadedSource, PipelineDataKind};

        let observer = Arc::new(CountingObserver::new());

        let data = PipelineData::LoadedSource(LoadedSource::new(
            std::path::PathBuf::from("test.qmd"),
            vec![],
        ));

        observer.on_pipeline_input(&data);
        assert_eq!(observer.pipeline_inputs.load(Ordering::SeqCst), 1);

        observer.on_stage_data("parse", 0, &data);
        observer.on_stage_data("transform", 1, &data);
        assert_eq!(observer.stage_data_calls.load(Ordering::SeqCst), 2);

        let log = observer.stage_data_log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].0, "parse");
        assert_eq!(log[0].1, PipelineDataKind::LoadedSource);
        assert_eq!(log[1].0, "transform");
    }

    #[test]
    fn test_tracing_observer_creation() {
        // Just test that it can be created
        let _observer = TracingObserver::new();
    }

    #[test]
    fn test_on_auxiliary_data_default_is_noop() {
        // Guards the "open-ended, safely ignored by default" contract: adding
        // an aux kind shouldn't break observers that don't implement it.
        let observer = NoopObserver::new();
        observer.on_auxiliary_data("any-stage", 0, "AnyKind", &serde_json::json!({}));
    }

    #[test]
    fn test_on_auxiliary_data_routes_to_observer() {
        let observer = Arc::new(CountingObserver::new());
        observer.on_auxiliary_data(
            "crossref-index",
            0,
            "CrossrefIndex",
            &serde_json::json!({"entries": []}),
        );
        let log = observer.aux_log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, "crossref-index");
        assert_eq!(log[0].1, "CrossrefIndex");
        assert_eq!(log[0].2["entries"], serde_json::json!([]));
    }
}
