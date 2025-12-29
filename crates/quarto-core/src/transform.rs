/*
 * transform.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * AST transformation pipeline infrastructure.
 */

//! AST transformation pipeline infrastructure.
//!
//! This module provides the core abstractions for AST transformations:
//!
//! - [`AstTransform`] - The trait implemented by all transformations
//! - [`TransformPipeline`] - Ordered collection of transforms to execute
//!
//! # Architecture
//!
//! Transforms are run in a flat, ordered sequence (insertion order).
//! Each transform can:
//! - Mutate the Pandoc AST (add/remove/modify blocks and inlines)
//! - Read from and write to the artifact store (for dependencies, metadata)
//! - Access format and project configuration
//!
//! # Example
//!
//! ```ignore
//! use quarto_core::transform::{AstTransform, TransformPipeline};
//!
//! struct MyTransform;
//!
//! impl AstTransform for MyTransform {
//!     fn name(&self) -> &str { "my-transform" }
//!
//!     fn transform(
//!         &self,
//!         ast: &mut quarto_pandoc_types::pandoc::Pandoc,
//!         ctx: &mut RenderContext,
//!     ) -> Result<()> {
//!         // Modify the AST...
//!         Ok(())
//!     }
//! }
//!
//! // Build pipeline
//! let mut pipeline = TransformPipeline::new();
//! pipeline.push(Box::new(MyTransform));
//!
//! // Execute transforms
//! pipeline.execute(&mut ast, &mut ctx)?;
//! ```

use crate::Result;
use crate::render::RenderContext;

/// Trait for AST transformations.
///
/// Transforms modify the Pandoc AST during the render pipeline.
/// They can also interact with the artifact store to record
/// dependencies or other metadata.
///
/// # Thread Safety
///
/// Transforms must be `Send + Sync` to support potential parallel
/// rendering of multiple documents.
pub trait AstTransform: Send + Sync {
    /// Human-readable name for this transform.
    ///
    /// Used for logging and debugging.
    fn name(&self) -> &str;

    /// Apply the transformation to the AST.
    ///
    /// # Arguments
    ///
    /// * `ast` - The Pandoc AST to transform
    /// * `ctx` - The render context (provides access to artifacts, format, project)
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails.
    fn transform(
        &self,
        ast: &mut quarto_pandoc_types::pandoc::Pandoc,
        ctx: &mut RenderContext,
    ) -> Result<()>;
}

/// A pipeline of AST transforms to execute in order.
///
/// Transforms run in insertion order.
pub struct TransformPipeline {
    transforms: Vec<Box<dyn AstTransform>>,
}

impl TransformPipeline {
    /// Create a new empty pipeline.
    pub fn new() -> Self {
        Self {
            transforms: Vec::new(),
        }
    }

    /// Add a transform to the pipeline.
    ///
    /// Transforms run in the order they are added.
    pub fn push(&mut self, transform: Box<dyn AstTransform>) {
        self.transforms.push(transform);
    }

    /// Add multiple transforms to the pipeline.
    pub fn extend(&mut self, transforms: impl IntoIterator<Item = Box<dyn AstTransform>>) {
        self.transforms.extend(transforms);
    }

    /// Get the number of transforms in the pipeline.
    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    /// Check if the pipeline is empty.
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }

    /// Execute all transforms in insertion order.
    ///
    /// # Arguments
    ///
    /// * `ast` - The Pandoc AST to transform
    /// * `ctx` - The render context
    ///
    /// # Errors
    ///
    /// Returns the first error encountered. Execution stops on error.
    pub fn execute(
        &self,
        ast: &mut quarto_pandoc_types::pandoc::Pandoc,
        ctx: &mut RenderContext,
    ) -> Result<()> {
        for transform in &self.transforms {
            tracing::debug!(transform = transform.name(), "Running transform");
            transform.transform(ast, ctx)?;
        }

        Ok(())
    }

    /// List the names of all transforms in execution order.
    ///
    /// Useful for debugging and logging.
    pub fn transform_names(&self) -> Vec<&str> {
        self.transforms.iter().map(|t| t.name()).collect()
    }
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::{BinaryDependencies, RenderContext};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_empty_ast() -> quarto_pandoc_types::pandoc::Pandoc {
        quarto_pandoc_types::pandoc::Pandoc::default()
    }

    /// A simple test transform that increments a counter.
    struct CountingTransform {
        name: &'static str,
        counter: Arc<AtomicUsize>,
        my_order: usize,
        order_tracker: Arc<std::sync::Mutex<Vec<usize>>>,
    }

    impl AstTransform for CountingTransform {
        fn name(&self) -> &str {
            self.name
        }

        fn transform(
            &self,
            _ast: &mut quarto_pandoc_types::pandoc::Pandoc,
            _ctx: &mut RenderContext,
        ) -> Result<()> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            self.order_tracker.lock().unwrap().push(self.my_order);
            Ok(())
        }
    }

    /// A transform that fails.
    struct FailingTransform;

    impl AstTransform for FailingTransform {
        fn name(&self) -> &str {
            "failing"
        }

        fn transform(
            &self,
            _ast: &mut quarto_pandoc_types::pandoc::Pandoc,
            _ctx: &mut RenderContext,
        ) -> Result<()> {
            Err(crate::error::QuartoError::other(
                "Transform failed intentionally",
            ))
        }
    }

    #[test]
    fn test_empty_pipeline() {
        let pipeline = TransformPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);
    }

    #[test]
    fn test_push_transforms() {
        let mut pipeline = TransformPipeline::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        pipeline.push(Box::new(CountingTransform {
            name: "first",
            counter: counter.clone(),
            my_order: 1,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(CountingTransform {
            name: "second",
            counter: counter.clone(),
            my_order: 2,
            order_tracker: order.clone(),
        }));

        assert_eq!(pipeline.len(), 2);
        assert!(!pipeline.is_empty());
    }

    #[test]
    fn test_execute_transforms() {
        let mut pipeline = TransformPipeline::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        pipeline.push(Box::new(CountingTransform {
            name: "first",
            counter: counter.clone(),
            my_order: 1,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(CountingTransform {
            name: "second",
            counter: counter.clone(),
            my_order: 2,
            order_tracker: order.clone(),
        }));

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = make_empty_ast();

        pipeline.execute(&mut ast, &mut ctx).unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn test_insertion_order() {
        let mut pipeline = TransformPipeline::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        // Add multiple transforms - they should run in insertion order
        pipeline.push(Box::new(CountingTransform {
            name: "first",
            counter: counter.clone(),
            my_order: 1,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(CountingTransform {
            name: "second",
            counter: counter.clone(),
            my_order: 2,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(CountingTransform {
            name: "third",
            counter: counter.clone(),
            my_order: 3,
            order_tracker: order.clone(),
        }));

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = make_empty_ast();

        pipeline.execute(&mut ast, &mut ctx).unwrap();

        // Should preserve insertion order
        assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_error_stops_execution() {
        let mut pipeline = TransformPipeline::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        pipeline.push(Box::new(CountingTransform {
            name: "before-fail",
            counter: counter.clone(),
            my_order: 1,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(FailingTransform));

        pipeline.push(Box::new(CountingTransform {
            name: "after-fail",
            counter: counter.clone(),
            my_order: 3,
            order_tracker: order.clone(),
        }));

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = make_empty_ast();

        let result = pipeline.execute(&mut ast, &mut ctx);

        assert!(result.is_err());
        // Only the first transform should have run
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(*order.lock().unwrap(), vec![1]);
    }

    #[test]
    fn test_transform_names() {
        let mut pipeline = TransformPipeline::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        pipeline.push(Box::new(CountingTransform {
            name: "alpha",
            counter: counter.clone(),
            my_order: 1,
            order_tracker: order.clone(),
        }));

        pipeline.push(Box::new(CountingTransform {
            name: "beta",
            counter: counter.clone(),
            my_order: 2,
            order_tracker: order.clone(),
        }));

        let names = pipeline.transform_names();
        assert_eq!(names, vec!["alpha", "beta"]);
    }
}
