/*
 * quarto-test/src/lib.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Testing infrastructure for Quarto documents with embedded assertions.
 */

//! # quarto-test
//!
//! Testing infrastructure for Quarto documents with embedded assertions.
//!
//! This crate provides the ability to run tests defined in QMD files via
//! `_quarto.tests` YAML metadata, similar to Quarto 1's smoke-all testing.
//!
//! ## Example
//!
//! ```yaml
//! ---
//! title: Test Document
//! format: html
//! _quarto:
//!   tests:
//!     html:
//!       ensureFileRegexMatches:
//!         - ["<!DOCTYPE html>", "<title>Test Document</title>"]
//!         - ["ERROR"]  # patterns that must NOT match
//! ---
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! use quarto_test::{run_test_file, TestResult};
//!
//! let result = run_test_file(Path::new("test.qmd"))?;
//! match result {
//!     TestResult::Pass => println!("All tests passed"),
//!     TestResult::Fail(failures) => eprintln!("Failures: {:?}", failures),
//!     TestResult::Skipped(reason) => println!("Skipped: {}", reason),
//! }
//! ```

mod assertions;
mod runner;
mod spec;

pub use assertions::{Assertion, VerifyContext};
pub use runner::{
    ProjectRenderCache, TestResult, TestSummary, run_test_file, run_test_file_with_project_cache,
    run_test_files,
};
pub use spec::{RunConfig, TestSpec};
