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

    /// Test observer that counts events
    struct CountingObserver {
        starts: AtomicUsize,
        completes: AtomicUsize,
        errors: AtomicUsize,
        events: AtomicUsize,
    }

    impl CountingObserver {
        fn new() -> Self {
            Self {
                starts: AtomicUsize::new(0),
                completes: AtomicUsize::new(0),
                errors: AtomicUsize::new(0),
                events: AtomicUsize::new(0),
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
    }

    #[test]
    fn test_noop_observer() {
        let observer = NoopObserver::new();
        // These should all be no-ops
        observer.on_stage_start("test", 0, 1);
        observer.on_stage_complete("test", 0, 1);
        observer.on_stage_error("test", 0, &PipelineError::Cancelled);
        observer.on_event("test message", EventLevel::Info);
        observer.on_pipeline_start(5);
        observer.on_pipeline_complete();
        observer.on_pipeline_error(&PipelineError::Cancelled);
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
    fn test_tracing_observer_creation() {
        // Just test that it can be created
        let _observer = TracingObserver::new();
    }
}
