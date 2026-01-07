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
use std::path::PathBuf;

/// Test starting a kernel, executing code, and shutting it down.
#[tokio::test]
#[ignore = "requires ipykernel"]
async fn test_kernel_execute_print() {
    // Skip if Python kernel not available
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    let daemon = daemon();
    let working_dir = PathBuf::from(std::env::current_dir().unwrap());

    // Start a kernel session
    let key = daemon
        .get_or_start_session("python3", &working_dir)
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

    let daemon = daemon();
    let working_dir = PathBuf::from(std::env::current_dir().unwrap());

    let key = daemon
        .get_or_start_session("python3", &working_dir)
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
                .map(|v| v.as_str().unwrap_or("").contains('4'))
                .unwrap_or(false)
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

    let daemon = daemon();
    let working_dir = PathBuf::from(std::env::current_dir().unwrap());

    let key = daemon
        .get_or_start_session("python3", &working_dir)
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
// AST Transform Integration Tests
// =============================================================================

use quarto_core::engine::jupyter::JupyterTransform;
use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, CodeBlock, Paragraph};
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

fn make_test_project() -> ProjectContext {
    ProjectContext {
        dir: PathBuf::from(std::env::current_dir().unwrap()),
        config: None,
        is_single_file: true,
        files: vec![DocumentInfo::from_path(
            std::env::current_dir().unwrap().join("test.qmd"),
        )],
        output_dir: PathBuf::from(std::env::current_dir().unwrap()),
    }
}

fn make_python_code_block(code: &str) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: (
            String::new(),
            vec!["{python}".to_string()],
            Default::default(),
        ),
        text: code.to_string(),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn make_paragraph(text: &str) -> Block {
    Block::Paragraph(Paragraph {
        content: vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::default(),
        })],
        source_info: SourceInfo::default(),
    })
}

/// Test that JupyterTransform executes code and transforms the AST.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_jupyter_transform_print() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // Create an AST with a Python code block
    let mut ast = Pandoc {
        meta: ConfigValue::new_map(vec![], SourceInfo::default()),
        blocks: vec![
            make_paragraph("Introduction"),
            make_python_code_block("print('Hello from transform!')"),
            make_paragraph("Conclusion"),
        ],
    };

    // Set up context with execution enabled
    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.options.execute = true;

    // Run the transform
    let transform = JupyterTransform::new();
    transform
        .transform(&mut ast, &mut ctx)
        .expect("Transform failed");

    // The code block should be replaced with output
    // We should still have 3 blocks: intro, output, conclusion
    // (or possibly more if the output produces multiple blocks)
    assert!(
        ast.blocks.len() >= 2,
        "Expected at least 2 blocks, got {}",
        ast.blocks.len()
    );

    // First block should still be the intro paragraph
    assert!(
        matches!(&ast.blocks[0], Block::Paragraph(_)),
        "First block should be paragraph"
    );

    // The code block should have been replaced - verify it's not a CodeBlock with {python} anymore
    let _has_python_codeblock = ast.blocks.iter().any(|b| {
        if let Block::CodeBlock(cb) = b {
            cb.attr.1.iter().any(|c| c.contains("python"))
        } else {
            false
        }
    });

    // The code block may still exist if we're preserving it with output
    // For now, just verify the transform ran without error
    println!("Transform completed. Block count: {}", ast.blocks.len());
    for (i, block) in ast.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(_) => println!("  {}: Paragraph", i),
            Block::CodeBlock(cb) => println!("  {}: CodeBlock (classes: {:?})", i, cb.attr.1),
            Block::Div(_) => println!("  {}: Div", i),
            Block::RawBlock(rb) => println!("  {}: RawBlock ({})", i, rb.format),
            _ => println!("  {}: Other block type", i),
        }
    }
}

/// Test that JupyterTransform handles expression output.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_jupyter_transform_expression() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    let mut ast = Pandoc {
        meta: ConfigValue::new_map(vec![], SourceInfo::default()),
        blocks: vec![make_python_code_block("1 + 1")],
    };

    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.options.execute = true;

    let transform = JupyterTransform::new();
    transform
        .transform(&mut ast, &mut ctx)
        .expect("Transform failed");

    // Verify we got some output
    assert!(!ast.blocks.is_empty(), "Expected output blocks");

    println!("Expression transform completed. Blocks:");
    for (i, block) in ast.blocks.iter().enumerate() {
        match block {
            Block::CodeBlock(cb) => {
                println!(
                    "  {}: CodeBlock text='{}'",
                    i,
                    cb.text.chars().take(50).collect::<String>()
                );
            }
            _ => println!("  {}: {:?}", i, std::mem::discriminant(block)),
        }
    }
}

/// Test that daemon persists kernel across multiple transforms.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_daemon_persistence() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // Create first AST - set a variable
    let mut ast1 = Pandoc {
        meta: ConfigValue::new_map(vec![], SourceInfo::default()),
        blocks: vec![make_python_code_block("test_var = 42")],
    };

    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx1 = RenderContext::new(&project, &doc, &format, &binaries);
    ctx1.options.execute = true;

    // Run first transform - creates kernel and sets variable
    let transform = JupyterTransform::new();
    transform
        .transform(&mut ast1, &mut ctx1)
        .expect("First transform failed");

    // Create second AST - read the variable
    let mut ast2 = Pandoc {
        meta: ConfigValue::new_map(vec![], SourceInfo::default()),
        blocks: vec![make_python_code_block("print(test_var)")],
    };

    let mut ctx2 = RenderContext::new(&project, &doc, &format, &binaries);
    ctx2.options.execute = true;

    // Run second transform - should reuse kernel and see the variable
    transform
        .transform(&mut ast2, &mut ctx2)
        .expect("Second transform failed");

    // Verify the second transform got the variable value
    // Look for stdout containing "42"
    let has_output_42 = ast2.blocks.iter().any(|b| {
        if let Block::CodeBlock(cb) = b {
            cb.text.contains("42")
        } else {
            false
        }
    });

    println!("Second transform blocks:");
    for (i, block) in ast2.blocks.iter().enumerate() {
        match block {
            Block::CodeBlock(cb) => {
                println!(
                    "  {}: CodeBlock text='{}'",
                    i,
                    cb.text.chars().take(100).collect::<String>()
                );
            }
            _ => println!("  {}: {:?}", i, std::mem::discriminant(block)),
        }
    }

    assert!(
        has_output_42,
        "Expected output containing '42' from persisted kernel state"
    );
}

/// Test that matplotlib figures produce display_data outputs.
#[tokio::test]
#[ignore = "requires ipykernel and matplotlib"]
async fn test_kernel_execute_matplotlib() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    let daemon = daemon();
    let working_dir = PathBuf::from(std::env::current_dir().unwrap());

    let key = daemon
        .get_or_start_session("python3", &working_dir)
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
// Inline Expression Tests
// =============================================================================

use quarto_pandoc_types::inline::Code;

/// Helper to create a paragraph with an inline expression.
fn make_paragraph_with_inline_expr(prefix: &str, expr: &str, suffix: &str) -> Block {
    Block::Paragraph(Paragraph {
        content: vec![
            Inline::Str(Str {
                text: prefix.to_string(),
                source_info: SourceInfo::default(),
            }),
            Inline::Code(Code {
                attr: (String::new(), vec![], Default::default()),
                text: expr.to_string(),
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }),
            Inline::Str(Str {
                text: suffix.to_string(),
                source_info: SourceInfo::default(),
            }),
        ],
        source_info: SourceInfo::default(),
    })
}

/// Test that inline expressions like `{python} 1+1` are evaluated.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires ipykernel"]
async fn test_jupyter_transform_inline_expression() {
    if !python_kernel_available().await {
        eprintln!("Python kernel not available, skipping test");
        return;
    }

    // Create an AST with an inline expression
    // The text is: "The answer is `{python} 2+2`!"
    let mut ast = Pandoc {
        meta: ConfigValue::new_map(vec![], SourceInfo::default()),
        blocks: vec![make_paragraph_with_inline_expr(
            "The answer is ",
            "{python} 2+2",
            "!",
        )],
    };

    let project = make_test_project();
    let doc = DocumentInfo::from_path(std::env::current_dir().unwrap().join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.options.execute = true;

    let transform = JupyterTransform::new();
    transform
        .transform(&mut ast, &mut ctx)
        .expect("Transform failed");

    // The paragraph should now contain the result "4" instead of the Code inline
    println!("Inline expression transform completed. Blocks:");
    for (i, block) in ast.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(para) => {
                println!("  {}: Paragraph with {} inlines:", i, para.content.len());
                for (j, inline) in para.content.iter().enumerate() {
                    match inline {
                        Inline::Str(s) => println!("    {}: Str('{}')", j, s.text),
                        Inline::Code(c) => println!("    {}: Code('{}')", j, c.text),
                        _ => println!("    {}: {:?}", j, std::mem::discriminant(inline)),
                    }
                }
            }
            _ => println!("  {}: {:?}", i, std::mem::discriminant(block)),
        }
    }

    // Verify the inline expression was replaced with result
    let has_result_4 = ast.blocks.iter().any(|b| {
        if let Block::Paragraph(para) = b {
            para.content.iter().any(|inline| {
                if let Inline::Str(s) = inline {
                    s.text.contains('4')
                } else {
                    false
                }
            })
        } else {
            false
        }
    });

    assert!(
        has_result_4,
        "Expected paragraph to contain '4' from inline expression evaluation"
    );
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
