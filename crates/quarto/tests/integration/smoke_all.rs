/*
 * smoke_all.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for smoke-all document tests.
 *
 * This test harness automatically discovers all .qmd files in the smoke-all/
 * directory and runs their embedded test specifications via quarto-test.
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quarto_core::project::ProjectContext;
use quarto_system_runtime::NativeRuntime;
use quarto_test::{ProjectRenderCache, TestResult, run_test_file_with_project_cache};
use walkdir::WalkDir;

/// Run all smoke-all tests by discovering .qmd files in the smoke-all directory.
///
/// This test walks the smoke-all/ directory tree, finds all .qmd files,
/// and runs each one through the quarto-test infrastructure. All failures
/// are collected and reported together.
#[test]
fn smoke_all() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let smoke_all_dir = Path::new(manifest_dir).join("tests/smoke-all");

    if !smoke_all_dir.exists() {
        panic!("smoke-all directory not found: {}", smoke_all_dir.display());
    }

    // Discover all .qmd files
    let mut test_files: Vec<_> = WalkDir::new(&smoke_all_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "qmd"))
        .map(|e| e.path().to_path_buf())
        .collect();

    // Sort for deterministic ordering
    test_files.sort();

    // Optional filter: SMOKE_FILTER=kbd runs only fixtures whose path contains "kbd"
    if let Ok(filter) = std::env::var("SMOKE_FILTER") {
        test_files.retain(|p| {
            p.strip_prefix(&smoke_all_dir)
                .unwrap_or(p)
                .to_string_lossy()
                .contains(&filter)
        });
    }

    if test_files.is_empty() {
        panic!(
            "No .qmd files found in smoke-all directory: {}",
            smoke_all_dir.display()
        );
    }

    eprintln!("Running {} smoke-all tests...\n", test_files.len());

    let runtime = NativeRuntime::new();
    let mut projects: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    let mut discovery_failures: HashMap<PathBuf, String> = HashMap::new();
    for path in &test_files {
        // `ProjectContext::discover` also provides the canonical project root
        // for no-project fixtures (a single-file pseudo-project).
        match ProjectContext::discover(path, &runtime) {
            Ok(project) if !project.is_single_file => {
                projects.entry(project.dir).or_default().push(path.clone());
            }
            Ok(_) => {}
            Err(error) => {
                discovery_failures.insert(path.clone(), error.to_string());
            }
        }
    }

    let mut project_cache = ProjectRenderCache::default();
    let mut passed = 0;
    let mut skipped = 0;
    let mut failures: Vec<(String, String)> = Vec::new();

    for path in &test_files {
        // Get relative path for display
        let rel_path = path
            .strip_prefix(&smoke_all_dir)
            .unwrap_or(path)
            .display()
            .to_string();

        if let Some(error) = discovery_failures.get(path) {
            let detail = format!("    project discovery failed: {error}");
            eprintln!("  ✗ {}\n{}", rel_path, detail);
            failures.push((rel_path, detail));
            continue;
        }
        let project_root = projects
            .iter()
            .find_map(|(root, files)| files.contains(path).then_some(root.as_path()));
        match run_test_file_with_project_cache(path, project_root, &mut project_cache) {
            Ok(TestResult::Pass) => {
                eprintln!("  ✓ {}", rel_path);
                passed += 1;
            }
            Ok(TestResult::Skipped(reason)) => {
                eprintln!("  ⊘ {} (skipped: {})", rel_path, reason);
                skipped += 1;
            }
            Ok(TestResult::Fail(test_failures)) => {
                let messages: Vec<String> = test_failures
                    .iter()
                    .map(|f| format!("    {} [{}]: {}", f.format, f.assertion, f.message))
                    .collect();
                let detail = messages.join("\n");
                eprintln!("  ✗ {}\n{}", rel_path, detail);
                failures.push((rel_path, detail));
            }
            Err(e) => {
                let detail = format!("    error: {}", e);
                eprintln!("  ✗ {}\n{}", rel_path, detail);
                failures.push((rel_path, detail));
            }
        }
    }

    eprintln!();
    eprintln!(
        "Results: {} passed, {} skipped, {} failed",
        passed,
        skipped,
        failures.len()
    );

    if !failures.is_empty() {
        eprintln!("\nFailures:");
        for (path, detail) in &failures {
            eprintln!("\n  {}:\n{}", path, detail);
        }
        panic!(
            "{} smoke-all test(s) failed (see output above)",
            failures.len()
        );
    }
}
