//! Integration tests for the Quarto LSP server.
//!
//! These tests spawn the LSP server as a subprocess and communicate
//! with it over stdio using JSON-RPC.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

/// Create a JSON-RPC request with the given method and params.
fn make_request(id: i32, method: &str, params: serde_json::Value) -> String {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    });
    let content = serde_json::to_string(&request).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", content.len(), content)
}

/// Create a JSON-RPC notification (no id) with the given method and params.
fn make_notification(method: &str, params: serde_json::Value) -> String {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    let content = serde_json::to_string(&request).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", content.len(), content)
}

/// Read a single LSP message from the reader.
fn read_message(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    // Read Content-Length header
    let mut header_line = String::new();
    reader
        .read_line(&mut header_line)
        .expect("Failed to read response header");

    let content_length: usize = header_line
        .trim()
        .strip_prefix("Content-Length: ")
        .expect("Missing Content-Length header")
        .parse()
        .expect("Invalid Content-Length");

    // Read empty line
    let mut empty_line = String::new();
    reader
        .read_line(&mut empty_line)
        .expect("Failed to read empty line");

    // Read content
    let mut content = vec![0u8; content_length];
    reader
        .read_exact(&mut content)
        .expect("Failed to read response content");
    let content_str = String::from_utf8(content).expect("Invalid UTF-8 in response");

    serde_json::from_str(&content_str).expect("Failed to parse response JSON")
}

/// Test harness for LSP integration tests.
struct LspTestHarness {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_request_id: i32,
}

impl LspTestHarness {
    /// Create a new test harness by spawning the LSP server.
    fn new() -> Self {
        let binary_path = Self::build_and_get_binary_path();

        let mut child = Command::new(&binary_path)
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn quarto lsp");

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");
        let reader = BufReader::new(stdout);

        Self {
            child,
            stdin,
            reader,
            next_request_id: 1,
        }
    }

    /// Build the quarto binary and return its path.
    fn build_and_get_binary_path() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        // Run cargo build first to ensure the binary exists
        let status = Command::new("cargo")
            .args(["build", "-p", "quarto"])
            .current_dir(workspace_root)
            .status()
            .expect("Failed to build quarto");
        assert!(status.success(), "Failed to build quarto binary");

        let binary_path = workspace_root.join("target").join("debug").join("quarto");
        assert!(
            binary_path.exists(),
            "quarto binary not found at {:?}",
            binary_path
        );

        binary_path
    }

    /// Send a request and return the response.
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_request_id;
        self.next_request_id += 1;

        let request = make_request(id, method, params);
        self.stdin
            .write_all(request.as_bytes())
            .expect("Failed to write request");
        self.stdin.flush().expect("Failed to flush stdin");

        // Read responses until we find one with our id
        loop {
            let response = read_message(&mut self.reader);
            if response.get("id").and_then(|i| i.as_i64()) == Some(id as i64) {
                return response;
            }
            // Continue reading (may be notifications)
        }
    }

    /// Send a notification (no response expected).
    fn notify(&mut self, method: &str, params: serde_json::Value) {
        let notification = make_notification(method, params);
        self.stdin
            .write_all(notification.as_bytes())
            .expect("Failed to write notification");
        self.stdin.flush().expect("Failed to flush stdin");
    }

    /// Initialize the LSP server.
    fn initialize(&mut self) -> serde_json::Value {
        let params = serde_json::json!({
            "processId": std::process::id(),
            "capabilities": {},
            "rootUri": null
        });
        let response = self.request("initialize", params);

        // Send initialized notification
        self.notify("initialized", serde_json::json!({}));

        response
    }

    /// Open a text document.
    fn open_document(&mut self, uri: &str, content: &str, version: i32) {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "quarto",
                "version": version,
                "text": content
            }
        });
        self.notify("textDocument/didOpen", params);
    }

    /// Change a text document (full sync).
    fn change_document(&mut self, uri: &str, content: &str, version: i32) {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "version": version
            },
            "contentChanges": [
                { "text": content }
            ]
        });
        self.notify("textDocument/didChange", params);
    }

    /// Close a text document.
    fn close_document(&mut self, uri: &str) {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri
            }
        });
        self.notify("textDocument/didClose", params);
    }

    /// Wait for a publishDiagnostics notification for the given URI.
    fn wait_for_diagnostics(
        &mut self,
        expected_uri: &str,
        timeout: Duration,
    ) -> Vec<serde_json::Value> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            let msg = read_message(&mut self.reader);
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
            {
                if let Some(params) = msg.get("params") {
                    if params.get("uri").and_then(|u| u.as_str()) == Some(expected_uri) {
                        return params
                            .get("diagnostics")
                            .and_then(|d| d.as_array())
                            .cloned()
                            .unwrap_or_default();
                    }
                }
            }
        }
        panic!("Timed out waiting for diagnostics for {}", expected_uri);
    }

    /// Request document symbols.
    fn document_symbols(&mut self, uri: &str) -> serde_json::Value {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri
            }
        });
        self.request("textDocument/documentSymbol", params)
    }
}

impl Drop for LspTestHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

// =============================================================================
// Initialization Tests
// =============================================================================

#[test]
fn test_initialize() {
    let mut harness = LspTestHarness::new();
    let response = harness.initialize();

    // Verify response
    assert!(
        response.get("result").is_some(),
        "Missing result in response"
    );

    let result = &response["result"];
    assert!(
        result.get("capabilities").is_some(),
        "Missing capabilities in result"
    );
    assert!(
        result.get("serverInfo").is_some(),
        "Missing serverInfo in result"
    );
    assert_eq!(result["serverInfo"]["name"], "quarto-lsp");

    // Verify text document sync capability
    let caps = &result["capabilities"];
    assert!(
        caps.get("textDocumentSync").is_some(),
        "Missing textDocumentSync capability"
    );

    // Verify document symbol capability
    assert!(
        caps.get("documentSymbolProvider").is_some(),
        "Missing documentSymbolProvider capability"
    );
}

// =============================================================================
// Diagnostic Tests
// =============================================================================

#[test]
fn test_diagnostics_valid_document() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/valid.qmd";
    let content = r#"---
title: "Test Document"
---

# Introduction

This is a valid QMD document with no errors.

## Section One

Some content here.
"#;

    harness.open_document(uri, content, 1);

    let diagnostics = harness.wait_for_diagnostics(uri, Duration::from_secs(5));

    // Valid document should have no error-level diagnostics
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.get("severity").and_then(|s| s.as_i64()) == Some(1)) // 1 = Error
        .collect();

    assert!(
        errors.is_empty(),
        "Valid document should have no errors, got: {:?}",
        errors
    );
}

#[test]
fn test_diagnostics_on_document_change() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/changing.qmd";

    // Open with valid content
    let valid_content = "# Hello\n\nWorld";
    harness.open_document(uri, valid_content, 1);

    let diagnostics = harness.wait_for_diagnostics(uri, Duration::from_secs(5));
    let error_count = diagnostics
        .iter()
        .filter(|d| d.get("severity").and_then(|s| s.as_i64()) == Some(1))
        .count();
    assert_eq!(
        error_count, 0,
        "Initial valid document should have no errors"
    );

    // Change to still-valid content
    let updated_content = "# Updated\n\nNew content here.";
    harness.change_document(uri, updated_content, 2);

    let diagnostics = harness.wait_for_diagnostics(uri, Duration::from_secs(5));
    let error_count = diagnostics
        .iter()
        .filter(|d| d.get("severity").and_then(|s| s.as_i64()) == Some(1))
        .count();
    assert_eq!(
        error_count, 0,
        "Updated valid document should still have no errors"
    );
}

#[test]
fn test_diagnostics_cleared_on_close() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/closing.qmd";
    let content = "# Test\n\nContent";

    harness.open_document(uri, content, 1);

    // Wait for initial diagnostics
    let _ = harness.wait_for_diagnostics(uri, Duration::from_secs(5));

    // Close the document
    harness.close_document(uri);

    // Should get an empty diagnostics notification
    let diagnostics = harness.wait_for_diagnostics(uri, Duration::from_secs(5));
    assert!(
        diagnostics.is_empty(),
        "Diagnostics should be cleared on document close"
    );
}

#[test]
fn test_diagnostics_with_yaml_frontmatter() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/frontmatter.qmd";
    let content = r#"---
title: "My Document"
author: "Test Author"
date: "2024-01-20"
format: html
---

# Content

This document has valid YAML frontmatter.
"#;

    harness.open_document(uri, content, 1);

    let diagnostics = harness.wait_for_diagnostics(uri, Duration::from_secs(5));

    // Valid frontmatter should not produce errors
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.get("severity").and_then(|s| s.as_i64()) == Some(1))
        .collect();

    assert!(
        errors.is_empty(),
        "Valid YAML frontmatter should have no errors, got: {:?}",
        errors
    );
}

// =============================================================================
// Document Symbol Tests
// =============================================================================

#[test]
fn test_document_symbols_headers() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/symbols.qmd";
    let content = r#"# Section One

Content.

## Subsection A

More content.

# Section Two

Final content.
"#;

    harness.open_document(uri, content, 1);

    // Wait for diagnostics to ensure document is processed
    let _ = harness.wait_for_diagnostics(uri, Duration::from_secs(5));

    // Request document symbols
    let response = harness.document_symbols(uri);

    assert!(
        response.get("result").is_some(),
        "Missing result in document symbols response"
    );

    let symbols = response["result"]
        .as_array()
        .expect("Expected array of symbols");

    // Should have at least 2 top-level sections
    assert!(
        symbols.len() >= 2,
        "Expected at least 2 top-level symbols, got {}",
        symbols.len()
    );

    // First symbol should be "Section One"
    assert_eq!(
        symbols[0]["name"].as_str(),
        Some("Section One"),
        "First symbol should be 'Section One'"
    );

    // Second symbol should be "Section Two"
    assert_eq!(
        symbols[1]["name"].as_str(),
        Some("Section Two"),
        "Second symbol should be 'Section Two'"
    );
}

#[test]
fn test_document_symbols_with_code_cells() {
    let mut harness = LspTestHarness::new();
    harness.initialize();

    let uri = "file:///test/code_cells.qmd";
    let content = r#"# Analysis

```{python}
#| label: setup
import pandas as pd
```

Some text.

```{r}
#| label: plot-data
plot(x, y)
```
"#;

    harness.open_document(uri, content, 1);

    // Wait for diagnostics
    let _ = harness.wait_for_diagnostics(uri, Duration::from_secs(5));

    // Request document symbols
    let response = harness.document_symbols(uri);

    assert!(
        response.get("result").is_some(),
        "Missing result in document symbols response"
    );

    let symbols = response["result"]
        .as_array()
        .expect("Expected array of symbols");

    // Should have at least the header
    assert!(
        !symbols.is_empty(),
        "Expected at least one symbol for the header"
    );
}
