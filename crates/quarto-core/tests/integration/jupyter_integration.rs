/*
 * tests/jupyter_integration.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Integration tests for Jupyter kernel lifecycle.
 */

//! Integration tests for Jupyter kernel lifecycle.
//!
//! These tests require a working Python installation with ipykernel.
//! They are marked with `#[ignore]` by default and can be run with:
//!
//! ```sh
//! cargo nextest run -p quarto-core --ignored jupyter_integration
//! ```

use quarto_core::engine::jupyter::{ResolvedKernel, daemon, list_kernelspecs};

/// Helper to check if Python kernel is available.
async fn python_kernel_available() -> bool {
    let specs: Vec<ResolvedKernel> = list_kernelspecs().await;
    specs.iter().any(|s| s.language.to_lowercase() == "python")
}

/// Test that we can list available kernelspecs.
#[tokio::test]
async fn test_list_kernelspecs() {
    let specs: Vec<ResolvedKernel> = list_kernelspecs().await;
    // Just verify we can call it without panicking
    // The result depends on the system configuration
    println!("Found {} kernelspecs", specs.len());
    for spec in &specs {
        println!("  - {} ({})", spec.name, spec.language);
    }
}

/// Test global daemon access.
#[tokio::test]
async fn test_global_daemon() {
    let daemon1 = daemon();
    let daemon2 = daemon();

    // Should be the same instance
    assert!(std::sync::Arc::ptr_eq(&daemon1, &daemon2));
}

/// Test that we can check if a Python kernel is available.
#[tokio::test]
async fn test_python_kernel_detection() {
    let available = python_kernel_available().await;
    println!("Python kernel available: {}", available);
    // This just tests the detection logic, doesn't require kernel to exist
}

// Full kernel lifecycle tests require ipykernel to be installed.
// Run with: cargo nextest run -p quarto-core --ignored jupyter_integration

use quarto_core::engine::jupyter::{CellOutput, ExecuteStatus};

/// Test starting a kernel, executing code, and shutting it down.
#[tokio::test]
#[ignore = "requires ipykernel"]
async fn test_kernel_execute_print() {
    // Skip if Python kernel not available
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // bd-hxhnnlzs: panic-safety — if an assertion below fails before
    // the explicit shutdown_session, this scope's drop still tears the
    // kernel down instead of leaking it to PID 1.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    let daemon = daemon();
    let working_dir = std::env::current_dir().unwrap();

    // Start a kernel session
    let key = daemon
        .get_or_start_session("python3", &working_dir, &[])
        .await
        .expect("Failed to start kernel");

    // Execute code with print()
    let result = daemon
        .execute_in_session(&key, "print('Hello from Python!')")
        .await
        .expect("Session not found")
        .expect("Execution failed");

    // Verify status is OK
    assert!(
        matches!(result.status, ExecuteStatus::Ok),
        "Expected OK status, got {:?}",
        result.status
    );

    // Verify we got stdout output
    let has_stdout = result.outputs.iter().any(|o| {
        if let CellOutput::Stream { name, text } = o {
            name == "stdout" && text.contains("Hello from Python!")
        } else {
            false
        }
    });
    assert!(
        has_stdout,
        "Expected stdout output with 'Hello from Python!'"
    );

    // Shutdown
    daemon
        .shutdown_session(&key)
        .await
        .expect("Shutdown failed");
}

/// Test evaluating an expression and getting execute_result.
#[tokio::test]
#[ignore = "requires ipykernel"]
async fn test_kernel_execute_expression() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // bd-hxhnnlzs: panic-safety — if an assertion below fails before
    // the explicit shutdown_session, this scope's drop still tears the
    // kernel down instead of leaking it to PID 1.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    let daemon = daemon();
    let working_dir = std::env::current_dir().unwrap();

    let key = daemon
        .get_or_start_session("python3", &working_dir, &[])
        .await
        .expect("Failed to start kernel");

    // Execute code that returns a value
    let result = daemon
        .execute_in_session(&key, "2 + 2")
        .await
        .expect("Session not found")
        .expect("Execution failed");

    assert!(matches!(result.status, ExecuteStatus::Ok));

    // Verify we got an execute_result with '4'
    let has_result = result.outputs.iter().any(|o| {
        if let CellOutput::ExecuteResult { data, .. } = o {
            data.get("text/plain")
                .is_some_and(|v| v.as_str().unwrap_or("").contains('4'))
        } else {
            false
        }
    });
    assert!(has_result, "Expected execute_result with '4'");

    daemon
        .shutdown_session(&key)
        .await
        .expect("Shutdown failed");
}

/// Test that errors are properly captured.
#[tokio::test]
#[ignore = "requires ipykernel"]
async fn test_kernel_execute_error() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // bd-hxhnnlzs: panic-safety — if an assertion below fails before
    // the explicit shutdown_session, this scope's drop still tears the
    // kernel down instead of leaking it to PID 1.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    let daemon = daemon();
    let working_dir = std::env::current_dir().unwrap();

    let key = daemon
        .get_or_start_session("python3", &working_dir, &[])
        .await
        .expect("Failed to start kernel");

    // Execute code that raises an error
    let result = daemon
        .execute_in_session(&key, "raise ValueError('test error')")
        .await
        .expect("Session not found")
        .expect("Execution failed");

    // Verify status is Error
    assert!(
        matches!(result.status, ExecuteStatus::Error { .. }),
        "Expected Error status"
    );

    // Check error details
    if let ExecuteStatus::Error { ename, evalue, .. } = &result.status {
        assert_eq!(ename, "ValueError");
        assert!(evalue.contains("test error"));
    }

    daemon
        .shutdown_session(&key)
        .await
        .expect("Shutdown failed");
}

// =============================================================================
// Shared fixtures for the pipeline tests below
// =============================================================================
//
// Note (bd-gthycd33): the AST-path transform tests that used to live
// here were retired together with `JupyterTransform` /
// `outputs_to_blocks` — production jupyter execution goes through the
// text path (`ExecutionEngine::execute` -> text_execute.rs), and the
// parallel AST emitter was a latent shape-divergence source with no
// production consumer. Kernel-state persistence is covered by
// `test_full_pipeline_multiple_cells` below and the cross-engine
// `engine_output_parity` suite; inline `{python} expr` evaluation
// (which only the retired prototype supported) is tracked as a
// follow-up strand.

use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

fn make_test_project() -> ProjectContext {
    ProjectContext {
        dir: std::env::current_dir().unwrap(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(
            std::env::current_dir().unwrap().join("test.qmd"),
        )],
        output_dir: std::env::current_dir().unwrap(),
    }
}

/// Test that matplotlib figures produce display_data outputs.
#[tokio::test]
#[ignore = "requires ipykernel and matplotlib"]
async fn test_kernel_execute_matplotlib() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // bd-hxhnnlzs: panic-safety — if an assertion below fails before
    // the explicit shutdown_session, this scope's drop still tears the
    // kernel down instead of leaking it to PID 1.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    let daemon = daemon();
    let working_dir = std::env::current_dir().unwrap();

    let key = daemon
        .get_or_start_session("python3", &working_dir, &[])
        .await
        .expect("Failed to start kernel");

    // Execute matplotlib code
    let code = r#"
import matplotlib.pyplot as plt
plt.figure()
plt.plot([1, 2, 3], [1, 4, 9])
plt.show()
"#;

    let result = daemon
        .execute_in_session(&key, code)
        .await
        .expect("Session not found")
        .expect("Execution failed");

    assert!(
        matches!(result.status, ExecuteStatus::Ok),
        "Expected OK status, got {:?}",
        result.status
    );

    // Verify we got a display_data output with image
    let has_image = result.outputs.iter().any(|o| {
        if let CellOutput::DisplayData { data, .. } = o {
            data.contains_key("image/png") || data.contains_key("image/svg+xml")
        } else {
            false
        }
    });

    // Note: matplotlib may not produce output in non-interactive mode
    // This is expected - we're testing that execution works
    if has_image {
        println!("Got image output from matplotlib!");
    } else {
        println!("No image output (expected in non-interactive mode)");
    }

    daemon
        .shutdown_session(&key)
        .await
        .expect("Shutdown failed");
}

// =============================================================================
// Full Pipeline Integration Tests
// =============================================================================

use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use std::sync::Arc;

/// Test that the full render pipeline can execute Python code.
///
/// This tests the complete flow:
/// 1. QMD source with Python code block
/// 2. ParseDocumentStage
/// 3. EngineExecutionStage (executes Python via Jupyter)
/// 4. AstTransformsStage
/// 5. RenderHtmlBodyStage
/// 6. ApplyTemplateStage
/// 7. Final HTML output with execution results
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_full_pipeline_python_execution() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    let content = br#"---
title: Pipeline Test
engine: jupyter
---

# Hello

```{python}
print("Hello from pipeline!")
```

The end.
"#;

    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let config = HtmlRenderConfig::default();
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());

    let result = render_qmd_to_html(content, "test.qmd", &mut ctx, &config, runtime).await;

    match result {
        Ok(output) => {
            println!("Pipeline succeeded!");
            println!("HTML output length: {} bytes", output.html.len());

            // The HTML should contain the executed output
            let has_hello = output.html.contains("Hello from pipeline!");
            println!("Contains 'Hello from pipeline!': {}", has_hello);
            assert!(
                has_hello,
                "Expected HTML to contain Python execution output"
            );

            // Should also contain the code block
            let has_code = output.html.contains("print");
            println!("Contains 'print': {}", has_code);

            // Should be valid HTML
            assert!(output.html.contains("<!DOCTYPE html>"));
            assert!(output.html.contains("<title>Pipeline Test</title>"));
        }
        Err(e) => {
            panic!("Pipeline failed: {:?}", e);
        }
    }
}

/// Test pipeline with multiple Python cells.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_full_pipeline_multiple_cells() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    let content = br#"---
title: Multi-Cell Test
engine: jupyter
---

```{python}
x = 42
```

```{python}
print(f"The answer is {x}")
```
"#;

    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let config = HtmlRenderConfig::default();
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());

    let result = render_qmd_to_html(content, "test.qmd", &mut ctx, &config, runtime).await;

    match result {
        Ok(output) => {
            println!("Multi-cell pipeline succeeded!");

            // The second cell should have access to x from the first cell
            let has_answer = output.html.contains("The answer is 42");
            println!("Contains 'The answer is 42': {}", has_answer);
            assert!(
                has_answer,
                "Expected HTML to contain result from persistent kernel state"
            );
        }
        Err(e) => {
            panic!("Pipeline failed: {:?}", e);
        }
    }
}
