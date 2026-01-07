# Plan: Jupyter Engine Implementation

**Issue**: k-kh5i
**Date**: 2026-01-07
**Status**: MVP Complete

## Overview

This plan describes the implementation of the Jupyter execution engine for Rust Quarto. The Jupyter engine executes Python, Julia, and other Jupyter-compatible code cells in Quarto documents.

### Key Architectural Insight

In Rust Quarto, the Jupyter engine is an **`AstTransform`** - it operates on the PandocAST, not on files. This is fundamentally different from TypeScript Quarto:

| Aspect | TypeScript Quarto | Rust Quarto |
|--------|-------------------|-------------|
| Input | QMD file → convert to ipynb | PandocAST with CodeBlocks |
| Orchestration | Python subprocess (jupyter.py, notebook.py) | Native Rust via runtimelib |
| Kernel protocol | Python's nbclient/jupyter_client | Rust's runtimelib (ZeroMQ) |
| Daemon | Separate Python process | In-process async/tokio |
| Surface syntax | Engine handles .ipynb conversion | Separate SourceConverter stage |

**No Python orchestration scripts needed** - We implement everything in native Rust using the runtimelib crate for Jupyter protocol communication.

### Goals (MVP Prototype)

The focus is **Jupyter kernel communication in Rust** - not a full-featured engine.

1. Execute code cells in PandocAST using Jupyter kernels via ZeroMQ
2. In-process daemon from the start (persistent kernel sessions)
3. Collect and convert outputs (text, images) to AST blocks
4. Basic inline code execution (`` `{python} expr` ``)

### Deferred (Not MVP)

- Chunk options (`echo`, `eval`, `output`, `warning`, `error`, `include`, etc.)
- Error recovery / continue-on-error
- Jupyter widgets (ipywidgets)
- Language-specific setup/cleanup (can add later)
- Freeze/thaw caching

### Non-Goals (This Plan)

- Surface syntax conversion (.ipynb → qmd) - handled by SourceConverter
- OJS (Observable JS) integration - separate effort
- Shiny server mode support - future enhancement

---

## Background

### TypeScript Quarto's Jupyter Architecture

TypeScript Quarto uses Python subprocess orchestration:

```
TypeScript (Deno)
    │
    └── spawn Python subprocess ──► jupyter.py
                                       │
                                       └── notebook.py (uses nbclient)
                                              │
                                              └── ZeroMQ ──► Jupyter Kernel
```

The Python scripts handle:
- `jupyter.py`: Entry point, daemon server management, command dispatch
- `notebook.py`: Notebook execution using `nbclient` and `jupyter_client`

### Rust Quarto's Approach: Pure Rust

We replace all Python orchestration with native Rust:

```
Rust Quarto (AstTransform)
    │
    └── JupyterDaemon (in-process, async)
           │
           └── runtimelib ──► ZeroMQ ──► Jupyter Kernel
```

**Why this works:**
- `runtimelib` provides native Rust ZeroMQ communication with Jupyter kernels
- `jupyter-protocol` defines all message types (execute_request, display_data, etc.)
- Setup/cleanup code is just strings executed in the kernel (no Python subprocess)

---

## Rust Jupyter Ecosystem

### Crates We'll Use

| Crate | Version | Purpose |
|-------|---------|---------|
| [`runtimelib`](https://docs.rs/runtimelib) | 0.30.2 | ZeroMQ kernel interaction |
| [`jupyter-protocol`](https://docs.rs/jupyter-protocol) | 0.11.0 | Message types, wire protocol |

Source code cloned to: `external-sources/runtimed/`

### runtimelib Code Review (2026-01-07)

**Project Health:**
- Actively maintained (commits from 2026-01-07)
- Used by Zed editor (production validation)
- BSD-3-Clause license
- ~400 lines in runtimelib, ~2500 in jupyter-protocol

**What runtimelib provides:**
- Kernelspec discovery (`list_kernelspecs()`, `read_kernelspec_jsons()`)
- ZeroMQ socket creation (`create_client_shell_connection()`, `create_client_iopub_connection()`, etc.)
- Jupyter runtime directories (`dirs::runtime_dir()`, `dirs::data_dirs()`)
- Port allocation (`peek_ports()`)
- HMAC message signing (via `ring` or `aws-lc-rs`)

**What runtimelib does NOT provide:**
- Kernel process spawning (we do this ourselves via `KernelspecDir::command()`)
- High-level execute API (we build `ExecuteRequest`, send, collect outputs manually)
- Kernel lifecycle management (start, interrupt, restart, shutdown - we implement)
- Output collection loop (we implement)

**Key dependencies:**
- `zeromq = "0.5.0-pre"` (Rust-native ZeroMQ, pre-release)
- `ring` or `aws-lc-rs` for HMAC (both well-maintained)

**Maintainability assessment:** We can maintain/develop this ourselves if needed. The codebase is small and well-structured.

### What We Must Build

Based on the `tokio-launch-kernel.rs` example, we need to implement:

1. **Kernel lifecycle manager** - spawn process, track PID, interrupt/restart/shutdown
2. **Connection file management** - allocate ports, write JSON, cleanup
3. **Output collection loop** - match on `Status`, `StreamContent`, `DisplayData`, `ExecuteResult`, `ErrorOutput`
4. **Session management** - kernel reuse for daemon mode
5. **Media extraction** - convert `jupyter_protocol::Media` to AST blocks

### Feature Flags

```toml
[dependencies]
runtimelib = { version = "0.30", features = ["tokio-runtime"] }
jupyter-protocol = "0.11"
```

---

## Design

### Integration with Render Pipeline

The Jupyter engine is an `AstTransform` in the render pipeline:

```
┌─────────────────────────────────────────────────────────────────────┐
│                        RenderPipeline                                │
├─────────────────────────────────────────────────────────────────────┤
│  [SourceConverter]  .ipynb → QMD text (SEPARATE, not Jupyter engine) │
│         ↓                                                            │
│  [Parser]           QMD text → PandocAST                             │
│         ↓                                                            │
│  [Transforms]       Vec<Box<dyn AstTransform>>                       │
│     ├── MetadataNormalize                                            │
│     ├── IncludeShortcodes                                            │
│     ├── **JupyterEngine** ← THIS PLAN                                │
│     ├── KnitrEngine                                                  │
│     ├── MermaidHandler                                               │
│     └── ...                                                          │
│         ↓                                                            │
│  [Writers]          AST → HTML/LaTeX/etc.                            │
└─────────────────────────────────────────────────────────────────────┘
```

### JupyterEngine as AstTransform

```rust
// crates/quarto-core/src/engine/jupyter/mod.rs

pub struct JupyterEngine {
    /// In-process daemon managing kernel sessions
    daemon: Arc<JupyterDaemon>,
}

impl AstTransform for JupyterEngine {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn stage(&self) -> &str {
        "execute"
    }

    fn transform(
        &self,
        doc: &mut PandocDocument,
        ctx: &mut RenderContext,
    ) -> Result<(), ExecutionError> {
        // 1. Check if document has Jupyter-executable code
        let cells = self.extract_code_cells(doc);
        let inline_exprs = self.extract_inline_expressions(doc);

        if cells.is_empty() && inline_exprs.is_empty() {
            return Ok(()); // Nothing to execute
        }

        // 2. Determine kernel from metadata or code block languages
        let kernelspec = self.resolve_kernelspec(doc, ctx)?;

        // 3. Execute code and replace in AST
        let kernel_key = KernelKey {
            kernelspec: kernelspec.name.clone(),
            working_dir: ctx.document.dir().to_path_buf(),
        };

        // Run async execution in blocking context
        tokio::runtime::Handle::current().block_on(async {
            self.execute_document(&kernel_key, doc, cells, inline_exprs, ctx).await
        })
    }
}

impl JupyterEngine {
    /// Extract executable code blocks from the document.
    fn extract_code_cells(&self, doc: &PandocDocument) -> Vec<CodeCellRef> {
        // Find all CodeBlock elements with executable languages
        // (python, julia, r, etc.)
    }

    /// Extract inline code expressions for evaluation.
    fn extract_inline_expressions(&self, doc: &PandocDocument) -> Vec<InlineExprRef> {
        // Find inline Code elements like `{python} 1 + 1`
    }

    /// Execute all code in the document.
    async fn execute_document(
        &self,
        kernel_key: &KernelKey,
        doc: &mut PandocDocument,
        cells: Vec<CodeCellRef>,
        inline_exprs: Vec<InlineExprRef>,
        ctx: &mut RenderContext,
    ) -> Result<(), ExecutionError> {
        // Get or start kernel
        let session = self.daemon.get_or_start_kernel(kernel_key).await?;

        // Execute setup cell (configure matplotlib, plotly, etc.)
        self.execute_setup_cell(&session, ctx).await?;

        // Execute code cells in order
        for cell in cells {
            let result = session.execute(&cell.source).await?;
            self.replace_code_block_with_output(doc, &cell, &result, ctx)?;
        }

        // Execute inline expressions
        if !inline_exprs.is_empty() {
            let results = session.evaluate_expressions(&inline_exprs).await?;
            self.replace_inline_expressions(doc, &inline_exprs, &results)?;
        }

        // Execute cleanup cell
        self.execute_cleanup_cell(&session, ctx).await?;

        Ok(())
    }
}
```

### JupyterDaemon (In-Process Kernel Manager)

The daemon manages kernel sessions across renders, providing the warm-kernel experience.

**Important:** runtimelib does NOT spawn kernel processes. We must do this ourselves following the pattern in `tokio-launch-kernel.rs`:

```rust
// crates/quarto-core/src/engine/jupyter/daemon.rs

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::process::Child;
use jupyter_protocol::ConnectionInfo;

/// Key for identifying kernel sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KernelKey {
    /// Kernelspec name (e.g., "python3", "julia-1.9")
    pub kernelspec: String,
    /// Working directory (kernels are per-directory for safety)
    pub working_dir: PathBuf,
}

/// A running kernel session.
pub struct KernelSession {
    /// The kernel process (we manage its lifecycle)
    process: Child,
    /// Connection info (ports, key)
    connection_info: ConnectionInfo,
    /// Path to connection file (for cleanup)
    connection_file: PathBuf,
    /// ZeroMQ shell socket
    shell_socket: runtimelib::ClientShellConnection,
    /// ZeroMQ iopub socket
    iopub_socket: runtimelib::ClientIoPubConnection,
    /// Session ID for message correlation
    session_id: String,
    /// Execution counter for this session
    execution_count: u32,
    /// Last activity time (for idle timeout)
    last_used: Instant,
}

/// In-process daemon managing Jupyter kernel sessions.
pub struct JupyterDaemon {
    /// Active kernel sessions
    sessions: RwLock<HashMap<KernelKey, KernelSession>>,
    /// Default idle timeout
    default_timeout: Duration,
}

impl JupyterDaemon {
    /// Start a new kernel for the given key.
    async fn start_kernel(&self, key: &KernelKey) -> Result<SessionHandle, DaemonError> {
        // 1. Find kernelspec
        let kernelspecs = runtimelib::list_kernelspecs().await;
        let spec = kernelspecs.iter()
            .find(|ks| ks.kernel_name == key.kernelspec)
            .ok_or_else(|| DaemonError::KernelNotFound(key.kernelspec.clone()))?;

        // 2. Allocate ports for ZeroMQ channels
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let ports = runtimelib::peek_ports(ip, 5).await?;

        // 3. Build connection info
        let connection_info = ConnectionInfo {
            transport: jupyter_protocol::connection_info::Transport::TCP,
            ip: ip.to_string(),
            stdin_port: ports[0],
            control_port: ports[1],
            hb_port: ports[2],
            shell_port: ports[3],
            iopub_port: ports[4],
            signature_scheme: "hmac-sha256".to_string(),
            key: uuid::Uuid::new_v4().to_string(),
            kernel_name: Some(key.kernelspec.clone()),
        };

        // 4. Write connection file
        let runtime_dir = runtimelib::dirs::runtime_dir();
        tokio::fs::create_dir_all(&runtime_dir).await?;
        let connection_file = runtime_dir.join(format!("kernel-{}.json", uuid::Uuid::new_v4()));
        tokio::fs::write(&connection_file, serde_json::to_string(&connection_info)?).await?;

        // 5. Spawn kernel process
        let process = spec.command(&connection_file, None, None)?
            .current_dir(&key.working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)  // Important: cleanup on daemon shutdown
            .spawn()?;

        // 6. Create ZeroMQ socket connections
        let session_id = uuid::Uuid::new_v4().to_string();
        let shell_socket = runtimelib::create_client_shell_connection(
            &connection_info, &session_id
        ).await?;
        let iopub_socket = runtimelib::create_client_iopub_connection(
            &connection_info, "", &session_id
        ).await?;

        // 7. Wait for kernel to be ready (poll for status message)
        // TODO: Implement kernel_info_request handshake

        let session = KernelSession {
            process,
            connection_info,
            connection_file,
            shell_socket,
            iopub_socket,
            session_id,
            execution_count: 0,
            last_used: Instant::now(),
        };

        // Store session
        let mut sessions = self.sessions.write().await;
        sessions.insert(key.clone(), session);

        Ok(SessionHandle::new(sessions.get_mut(key).unwrap()))
    }

    /// Shutdown a kernel session.
    async fn shutdown_kernel(&self, session: &mut KernelSession) -> Result<(), DaemonError> {
        // Send shutdown_request via control channel (if needed)
        // Then kill process
        session.process.start_kill()?;

        // Cleanup connection file
        let _ = tokio::fs::remove_file(&session.connection_file).await;

        Ok(())
    }
}
```

**Key insight:** The `kill_on_drop(true)` on the spawned process ensures kernels are cleaned up if the daemon crashes. This is safer than TypeScript's separate-process approach.

### Kernel Execution

```rust
// crates/quarto-core/src/engine/jupyter/execute.rs

use jupyter_protocol::{ExecuteRequest, JupyterMessage, JupyterMessageContent};

impl KernelSession {
    /// Execute code in the kernel.
    pub async fn execute(&mut self, code: &str) -> Result<ExecuteResult, ExecutionError> {
        self.execution_count += 1;
        self.last_used = Instant::now();

        // Build execute request
        let request = ExecuteRequest {
            code: code.to_string(),
            silent: false,
            store_history: true,
            user_expressions: Default::default(),
            allow_stdin: false,
            stop_on_error: true,
        };

        // Send request
        let msg_id = self.connection.send_execute_request(request).await?;

        // Collect outputs
        let mut outputs = Vec::new();
        let mut status = ExecuteStatus::Ok;

        loop {
            let msg = self.connection.recv_iopub().await?;

            match msg.content {
                JupyterMessageContent::Status(s) if s.execution_state == "idle" => {
                    break;
                }
                JupyterMessageContent::Stream(stream) => {
                    outputs.push(CellOutput::Stream {
                        name: stream.name,
                        text: stream.text,
                    });
                }
                JupyterMessageContent::DisplayData(data) => {
                    outputs.push(CellOutput::DisplayData {
                        data: data.data,
                        metadata: data.metadata,
                    });
                }
                JupyterMessageContent::ExecuteResult(result) => {
                    outputs.push(CellOutput::ExecuteResult {
                        execution_count: result.execution_count,
                        data: result.data,
                        metadata: result.metadata,
                    });
                }
                JupyterMessageContent::Error(err) => {
                    status = ExecuteStatus::Error {
                        ename: err.ename,
                        evalue: err.evalue,
                        traceback: err.traceback,
                    };
                }
                _ => {}
            }
        }

        // Wait for execute_reply
        let reply = self.connection.recv_shell_reply(&msg_id).await?;

        Ok(ExecuteResult {
            status,
            outputs,
            execution_count: self.execution_count,
        })
    }

    /// Evaluate expressions (for inline code).
    pub async fn evaluate_expressions(
        &mut self,
        expressions: &[InlineExprRef],
    ) -> Result<Vec<String>, ExecutionError> {
        // Use user_expressions feature of execute_request
        let user_expressions: HashMap<String, String> = expressions
            .iter()
            .enumerate()
            .map(|(i, expr)| (i.to_string(), expr.code.clone()))
            .collect();

        let request = ExecuteRequest {
            code: "".to_string(), // Empty code, just evaluate expressions
            silent: true,
            store_history: false,
            user_expressions,
            allow_stdin: false,
            stop_on_error: true,
        };

        let msg_id = self.connection.send_execute_request(request).await?;
        let reply = self.connection.recv_shell_reply(&msg_id).await?;

        // Extract results from user_expressions in reply
        let results = expressions
            .iter()
            .enumerate()
            .map(|(i, _)| {
                reply.user_expressions
                    .get(&i.to_string())
                    .and_then(|v| v.get("text/plain"))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        Ok(results)
    }
}
```

### Language-Specific Setup Code (Deferred)

Setup/cleanup code will be added later. For MVP, we execute user code directly without setup.

When implemented, setup code will be stored as Rust string constants (see `external-sources/quarto-dev/quarto-cli/src/resources/jupyter/lang/python/setup.py` for reference).

### Kernelspec Resolution

```rust
// crates/quarto-core/src/engine/jupyter/kernelspec.rs

use std::path::PathBuf;

/// Jupyter kernelspec information.
#[derive(Debug, Clone)]
pub struct JupyterKernelspec {
    pub name: String,
    pub display_name: String,
    pub language: String,
    pub path: PathBuf,
}

/// Resolve kernelspec from document metadata and code block languages.
pub fn resolve_kernelspec(
    doc: &PandocDocument,
    ctx: &RenderContext,
) -> Result<JupyterKernelspec, ExecutionError> {
    // 1. Check for explicit kernel in metadata
    //    jupyter: python3
    //    jupyter:
    //      kernel: python3
    if let Some(jupyter) = ctx.format.metadata.get("jupyter") {
        if let Some(kernel_name) = jupyter.as_str() {
            return find_kernelspec(kernel_name);
        }
        if let Some(kernel) = jupyter.get("kernel") {
            if let Some(kernel_name) = kernel.as_str() {
                return find_kernelspec(kernel_name);
            }
        }
    }

    // 2. Detect from primary language in code blocks
    let language = detect_primary_language(doc);
    find_kernelspec_for_language(&language)
}

/// Find kernelspec by name using runtimelib.
fn find_kernelspec(name: &str) -> Result<JupyterKernelspec, ExecutionError> {
    let specs = runtimelib::list_kernelspecs()?;

    specs.into_iter()
        .find(|ks| ks.name == name)
        .map(|ks| JupyterKernelspec {
            name: ks.name,
            display_name: ks.display_name,
            language: ks.language,
            path: ks.path,
        })
        .ok_or_else(|| ExecutionError::runtime_not_found("jupyter", name))
}

/// Find kernelspec for a language (e.g., "python" → "python3").
fn find_kernelspec_for_language(language: &str) -> Result<JupyterKernelspec, ExecutionError> {
    let specs = runtimelib::list_kernelspecs()?;

    // Find first kernel matching the language
    specs.into_iter()
        .find(|ks| ks.language.to_lowercase() == language.to_lowercase())
        .map(|ks| JupyterKernelspec {
            name: ks.name,
            display_name: ks.display_name,
            language: ks.language,
            path: ks.path,
        })
        .ok_or_else(|| ExecutionError::runtime_not_found("jupyter", language))
}

/// Detect the primary executable language from code blocks.
fn detect_primary_language(doc: &PandocDocument) -> String {
    let mut lang_counts: HashMap<String, usize> = HashMap::new();

    // Count code blocks by language
    for block in doc.walk_blocks() {
        if let Block::CodeBlock(attr, _) = block {
            for class in &attr.classes {
                let lang = class.to_lowercase();
                if is_jupyter_language(&lang) {
                    *lang_counts.entry(lang).or_insert(0) += 1;
                }
            }
        }
    }

    // Return most common language
    lang_counts.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(lang, _)| lang)
        .unwrap_or_else(|| "python".to_string())
}

/// Check if a language can be executed via Jupyter.
fn is_jupyter_language(lang: &str) -> bool {
    matches!(lang, "python" | "julia" | "r" | "scala" | "ruby" | "bash")
}
```

### Output Conversion

```rust
// crates/quarto-core/src/engine/jupyter/output.rs

use quarto_pandoc_types::{Block, Inline, Attr};
use std::path::PathBuf;

/// Cell output from kernel execution.
#[derive(Debug)]
pub enum CellOutput {
    Stream { name: String, text: String },
    DisplayData { data: MimeBundle, metadata: serde_json::Value },
    ExecuteResult { execution_count: u32, data: MimeBundle, metadata: serde_json::Value },
}

/// MIME bundle (mapping of mime type to content).
pub type MimeBundle = HashMap<String, serde_json::Value>;

/// Convert cell outputs to AST blocks.
pub fn outputs_to_blocks(
    outputs: &[CellOutput],
    ctx: &mut RenderContext,
) -> Result<Vec<Block>, ConversionError> {
    let mut blocks = Vec::new();

    for output in outputs {
        match output {
            CellOutput::Stream { name, text } => {
                // Stream output → CodeBlock with output class
                let attr = Attr {
                    classes: vec![format!("cell-output-{}", name)],
                    ..Default::default()
                };
                blocks.push(Block::CodeBlock(attr, text.clone()));
            }
            CellOutput::DisplayData { data, .. } |
            CellOutput::ExecuteResult { data, .. } => {
                // Rich output → best representation for format
                if let Some(block) = mime_to_block(data, ctx)? {
                    blocks.push(block);
                }
            }
        }
    }

    Ok(blocks)
}

/// Convert MIME bundle to best AST representation.
fn mime_to_block(
    data: &MimeBundle,
    ctx: &mut RenderContext,
) -> Result<Option<Block>, ConversionError> {
    // Priority order for HTML output
    let html_priority = [
        "text/html",
        "image/svg+xml",
        "image/png",
        "image/jpeg",
        "text/markdown",
        "text/latex",
        "text/plain",
    ];

    // Priority order for LaTeX/PDF output
    let latex_priority = [
        "text/latex",
        "application/pdf",
        "image/pdf",
        "image/png",
        "image/jpeg",
        "text/plain",
    ];

    let priority = if ctx.format.is_latex() {
        &latex_priority[..]
    } else {
        &html_priority[..]
    };

    for mime_type in priority {
        if let Some(content) = data.get(*mime_type) {
            return convert_mime_content(*mime_type, content, ctx);
        }
    }

    Ok(None)
}

/// Convert specific MIME content to AST.
fn convert_mime_content(
    mime_type: &str,
    content: &serde_json::Value,
    ctx: &mut RenderContext,
) -> Result<Option<Block>, ConversionError> {
    match mime_type {
        "text/plain" => {
            let text = content.as_str().unwrap_or("");
            let attr = Attr {
                classes: vec!["cell-output".to_string()],
                ..Default::default()
            };
            Ok(Some(Block::CodeBlock(attr, text.to_string())))
        }
        "text/html" => {
            let html = content.as_str().unwrap_or("");
            Ok(Some(Block::RawBlock("html".to_string(), html.to_string())))
        }
        "text/markdown" => {
            // Parse markdown content and inline it
            let md = content.as_str().unwrap_or("");
            let doc = pampa::parse_markdown(md)?;
            // Return first block or Para wrapping
            Ok(doc.blocks.into_iter().next())
        }
        "image/png" | "image/jpeg" | "image/svg+xml" => {
            // Save image and create reference
            let ext = match mime_type {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/svg+xml" => "svg",
                _ => "bin",
            };

            let data = if mime_type == "image/svg+xml" {
                content.as_str().unwrap_or("").as_bytes().to_vec()
            } else {
                // Base64 decode
                let b64 = content.as_str().unwrap_or("");
                base64::decode(b64)?
            };

            // Generate unique filename
            let filename = format!("figure-{}.{}", uuid::Uuid::new_v4(), ext);
            let path = ctx.temp_dir.join(&filename);

            // Store in artifacts
            ctx.artifacts.store_bytes(
                &format!("execution:image:{}", filename),
                data,
                mime_type,
            );

            // Create image element
            let attr = Attr {
                classes: vec!["cell-output-display".to_string()],
                ..Default::default()
            };
            let img = Inline::Image(
                attr,
                vec![],
                (path.to_string_lossy().to_string(), "".to_string()),
            );
            Ok(Some(Block::Para(vec![img])))
        }
        _ => Ok(None),
    }
}
```

---

## Module Structure

```
crates/quarto-core/src/engine/
├── mod.rs                      # Engine registry, ExecutionEngine trait
├── jupyter/
│   ├── mod.rs                  # JupyterEngine (AstTransform impl)
│   ├── daemon.rs               # JupyterDaemon (in-process kernel manager)
│   ├── session.rs              # KernelSession (single kernel connection)
│   ├── execute.rs              # Execution logic (send/recv messages)
│   ├── kernelspec.rs           # Kernel discovery and resolution
│   ├── setup.rs                # Language-specific setup code constants
│   ├── output.rs               # Output → AST conversion
│   └── error.rs                # Jupyter-specific errors
```

---

## Data Structures

### Execute Configuration

```rust
/// Configuration for code execution (from YAML metadata).
#[derive(Debug, Clone)]
pub struct ExecuteConfig {
    /// Evaluate code chunks (default: true)
    pub eval: bool,
    /// Show code in output
    pub echo: EchoOption,
    /// Show warnings
    pub warning: bool,
    /// Allow errors (continue on error)
    pub error: bool,
    /// Figure width (inches)
    pub fig_width: f64,
    /// Figure height (inches)
    pub fig_height: f64,
    /// Figure DPI
    pub fig_dpi: u32,
    /// Figure format (png, svg, pdf, retina)
    pub fig_format: String,
    /// IPython shell interactivity
    pub interactivity: Option<String>,
    /// Plotly connected mode
    pub plotly_connected: bool,
    /// Daemon timeout (0 = disable daemon)
    pub daemon_timeout: u64,
    /// Force daemon restart
    pub daemon_restart: bool,
}

impl Default for ExecuteConfig {
    fn default() -> Self {
        Self {
            eval: true,
            echo: EchoOption::True,
            warning: true,
            error: false,
            fig_width: 7.0,
            fig_height: 5.0,
            fig_dpi: 96,
            fig_format: "png".to_string(),
            interactivity: None,
            plotly_connected: false,
            daemon_timeout: 300, // 5 minutes
            daemon_restart: false,
        }
    }
}
```

### Execution Result

```rust
/// Result of executing a code cell.
#[derive(Debug)]
pub struct ExecuteResult {
    /// Execution status
    pub status: ExecuteStatus,
    /// Cell outputs
    pub outputs: Vec<CellOutput>,
    /// Execution count
    pub execution_count: u32,
}

#[derive(Debug)]
pub enum ExecuteStatus {
    Ok,
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}
```

---

## Implementation Phases (MVP Focus)

### Phase 1: Kernel Lifecycle
1. Add runtimelib and jupyter-protocol dependencies to quarto-core
2. Create `crates/quarto-core/src/engine/jupyter/` module structure
3. Implement kernelspec discovery via `runtimelib::list_kernelspecs()`
4. Implement `JupyterDaemon::start_kernel()` - spawn process, create sockets
5. Implement `JupyterDaemon::shutdown_kernel()` - cleanup
6. Test: start kernel, verify it's running, shut it down

### Phase 2: Execute Request/Reply
1. Implement `KernelSession::execute()`:
   - Build `ExecuteRequest` message
   - Send via shell socket
   - Collect outputs from iopub (loop until Status::Idle)
2. Handle message types: `StreamContent`, `DisplayData`, `ExecuteResult`, `ErrorOutput`
3. Test: execute `print("hello")`, verify output collected

### Phase 3: Output Conversion
1. Implement `jupyter_protocol::Media` → AST block conversion
2. Handle text/plain → CodeBlock
3. Handle image/png, image/jpeg → save file, create Image inline
4. Handle text/html → RawBlock
5. Test: execute matplotlib plot, verify image saved

### Phase 4: AST Integration
1. Implement `JupyterEngine` as `AstTransform`
2. Extract CodeBlocks with `python`/`julia` class
3. Execute cells in order
4. Replace CodeBlocks with output blocks
5. End-to-end test: parse QMD, execute, verify transformed AST

### Phase 5 (Stretch): Inline Execution
1. Extract inline code expressions (`` `{python} expr` ``)
2. Use `user_expressions` in execute request
3. Replace inline Code elements with results

### Future (Post-MVP)
- Chunk options (`echo`, `eval`, etc.)
- Language-specific setup/cleanup cells
- Module dependency tracking for daemon restart
- Error recovery / continue-on-error
- Kernel interrupt/timeout handling

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_kernelspec_resolution_explicit() {
        // Test that explicit kernel in metadata is respected
    }

    #[test]
    fn test_language_detection() {
        // Test detection of primary language from code blocks
    }

    #[test]
    fn test_setup_code_generation() {
        let config = ExecuteConfig {
            fig_width: 8.0,
            fig_format: "svg".to_string(),
            ..Default::default()
        };
        let code = build_setup_code("python", &config);
        assert!(code.contains("fig_width = 8.0"));
        assert!(code.contains("fig_format = 'svg'"));
    }

    #[test]
    fn test_output_conversion() {
        // Test MIME bundle → AST block conversion
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    #[tokio::test]
    #[ignore] // Requires Python + ipykernel
    async fn test_basic_execution() {
        let daemon = JupyterDaemon::new(Duration::from_secs(60));
        let key = KernelKey {
            kernelspec: "python3".to_string(),
            working_dir: PathBuf::from("."),
        };

        let session = daemon.get_or_start_kernel(&key).await.unwrap();
        let result = session.execute("1 + 1").await.unwrap();

        assert!(matches!(result.status, ExecuteStatus::Ok));
        assert!(!result.outputs.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_figure_output() {
        // Test that matplotlib figures produce image outputs
    }

    #[tokio::test]
    #[ignore]
    async fn test_daemon_persistence() {
        // Test that kernel persists across multiple executions
    }
}
```

---

## Dependencies

### Cargo.toml additions

```toml
[dependencies]
# Jupyter protocol
runtimelib = { version = "0.30", features = ["tokio-runtime"] }
jupyter-protocol = "0.11"

# Async runtime
tokio = { version = "1", features = ["sync", "time", "rt-multi-thread"] }

# Utilities
uuid = { version = "1", features = ["v4"] }
base64 = "0.21"
```

### Runtime Requirements

Users must have:
- A Jupyter kernel installed (e.g., `ipykernel` for Python, `IJulia` for Julia)
- The kernel registered in Jupyter's kernelspec system

---

## Success Criteria

### MVP Complete When:
- [x] Can discover and list Jupyter kernelspecs
- [x] Can start a Python kernel process
- [x] Can send execute_request and receive outputs
- [x] Can collect stdout (StreamContent) from kernel
- [x] Can collect display_data (images) from kernel
- [x] Can shutdown kernel cleanly (process killed, connection file removed)
- [x] JupyterEngine transforms AST: CodeBlock → output blocks
- [x] Integration test passes: `print("hello")` → output in AST
- [x] Integration test passes: matplotlib plot → image file + AST reference

### Stretch Goals:
- [x] Daemon persists kernel across multiple transforms
- [x] Inline expressions evaluated (via execute + result extraction, not user_expressions)
- [ ] Kernel timeout/interrupt handling

---

## Related Documents

- [Execution Engine Infrastructure](2026-01-06-execution-engine-infrastructure.md) - Core engine traits
- [Surface Syntax Converter Design](../surface-syntax-converter-design.md) - .ipynb conversion (separate from engine)
- [Quarto Render Prototype](2025-12-20-quarto-render-prototype.md) - Pipeline architecture

## External References

- [runtimelib docs](https://docs.rs/runtimelib)
- [jupyter-protocol docs](https://docs.rs/jupyter-protocol)
- [Jupyter Wire Protocol](https://jupyter-client.readthedocs.io/en/latest/messaging.html)
- [TypeScript jupyter.ts](external-sources/quarto-dev/quarto-cli/src/execute/jupyter/jupyter.ts) (reference only)
