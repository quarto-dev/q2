# Machine-Readable I/O Design for Quarto

**Date:** 2025-10-12
**Status:** Design Decision
**Decision:** Hybrid approach with global `--format` flag + per-command `--json` for backward compatibility

## Motivation

Quarto is frequently invoked programmatically by upstream tooling (IDEs, build systems, CI/CD pipelines). Currently, only some commands support machine-readable output (e.g., `quarto inspect --json`). This design extends structured I/O support across all commands consistently.

## Design Goals

1. **Universal Support**: Every command should be capable of producing machine-readable output
2. **Input & Output**: Support both structured input (config files) and output (JSON)
3. **Streaming**: Support progressive output for long-running operations
4. **Backward Compatible**: Maintain compatibility with existing `--json` flags in quarto-cli v1
5. **Rust Ecosystem Patterns**: Follow established patterns from cargo, ripgrep, rustc

## Architecture Decision: Option C - Hybrid Approach

### Global Format Flag

```rust
#[derive(Parser)]
#[command(name = "quarto")]
#[command(version = quarto_util::cli_version())]
#[command(about = "Quarto CLI")]
struct Cli {
    /// Output format for results and data
    #[arg(long, global = true, value_enum)]
    format: Option<OutputFormat>,

    /// Quiet mode - suppress human-readable progress messages
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Human-readable output with colors and formatting
    Human,
    /// Line-delimited JSON
    Json,
    /// YAML format (useful for config-heavy output)
    Yaml,
}

impl Default for OutputFormat {
    fn default() -> Self {
        // Auto-detect: JSON if stdout is piped, Human if terminal
        if std::io::stdout().is_terminal() {
            OutputFormat::Human
        } else {
            OutputFormat::Json
        }
    }
}
```

### Per-Command JSON Flag (Backward Compatibility)

```rust
#[derive(Subcommand)]
enum Commands {
    Render {
        // ... render options

        /// Output result as JSON (equivalent to --format json)
        #[arg(long)]
        json: bool,
    },

    Inspect {
        /// Output in JSON format (backward compat with v1)
        #[arg(long)]
        json: bool,
    },

    // Other commands...
}
```

**Priority:** Per-command `--json` flag overrides global `--format` for backward compatibility.

## Output Architecture

### 1. Separation of Concerns

**Principle:** Business logic returns structured data; presentation layer formats it.

```
Command Handler → Core Logic → Structured Result → Output Writer → Format
                                     ↓
                              (Serializable struct)
```

### 2. Output Trait

```rust
// quarto-util/src/output.rs

use serde::Serialize;
use std::io::{self, Write};

/// Trait for types that can be output in multiple formats
pub trait Outputable: Serialize {
    /// Convert to human-readable format for terminal display
    fn format_human(&self) -> String;

    /// Convert to machine-readable JSON (default implementation)
    fn format_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Convert to YAML format (default implementation)
    fn format_yaml(&self) -> serde_yaml::Result<String> {
        serde_yaml::to_string(self)
    }
}

/// Output writer that handles formatting and I/O separation
pub struct OutputWriter {
    format: OutputFormat,
    quiet: bool,
}

impl OutputWriter {
    pub fn new(format: OutputFormat, quiet: bool) -> Self {
        Self { format, quiet }
    }

    /// Write structured data in the appropriate format
    pub fn write<T: Outputable>(&self, data: &T) -> anyhow::Result<()> {
        match self.format {
            OutputFormat::Human if !self.quiet => {
                println!("{}", data.format_human());
            }
            OutputFormat::Json => {
                // Line-delimited JSON to stdout
                println!("{}", data.format_json()?);
            }
            OutputFormat::Yaml => {
                println!("{}", data.format_yaml()?);
            }
            _ => {} // quiet mode with no output
        }
        Ok(())
    }

    /// Write progress/status message (human-only, to stderr)
    pub fn write_progress(&self, msg: impl AsRef<str>) {
        if !self.quiet && self.format == OutputFormat::Human {
            eprintln!("{}", msg.as_ref());
        }
    }

    /// Write error message (to stderr, all formats)
    pub fn write_error(&self, msg: impl AsRef<str>) {
        eprintln!("{}", msg.as_ref());
    }

    /// Write streaming event (for long-running operations)
    pub fn write_event<T: Serialize>(&self, event: &StreamEvent<T>) -> anyhow::Result<()> {
        match self.format {
            OutputFormat::Json => {
                // Line-delimited JSON events
                println!("{}", serde_json::to_string(event)?);
            }
            OutputFormat::Human if !self.quiet => {
                // Human-readable progress
                match &event.type_ {
                    EventType::Progress => eprintln!("{}", event.message),
                    EventType::Result => {} // Handled by write()
                    EventType::Error => eprintln!("Error: {}", event.message),
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Streaming event for long-running operations
#[derive(Serialize)]
pub struct StreamEvent<T> {
    #[serde(rename = "type")]
    pub type_: EventType,
    pub timestamp: u64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventType {
    Progress,
    Result,
    Error,
}
```

### 3. Result Types

All command results should be serializable structs:

```rust
// quarto-core/src/render.rs

use serde::Serialize;
use quarto_util::output::Outputable;

/// Result of a render operation
#[derive(Serialize, Debug)]
pub struct RenderResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub format: String,
    pub execution_time_ms: u64,
    pub warnings: Vec<RenderWarning>,
    pub errors: Vec<RenderError>,
    pub success: bool,
}

#[derive(Serialize, Debug)]
pub struct RenderWarning {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
}

#[derive(Serialize, Debug)]
pub struct RenderError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub severity: ErrorSeverity,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Warning,
    Error,
    Fatal,
}

impl Outputable for RenderResult {
    fn format_human(&self) -> String {
        use colored::*;

        let status = if self.success {
            "✓".green()
        } else {
            "✗".red()
        };

        let mut output = format!(
            "{} Rendered {} to {} ({}ms)\n",
            status,
            self.input.display(),
            self.output.display(),
            self.execution_time_ms
        );

        if !self.warnings.is_empty() {
            output.push_str(&format!("\n{} warnings:\n", self.warnings.len()));
            for warning in &self.warnings {
                output.push_str(&format!("  ⚠ {}\n", warning.message.yellow()));
            }
        }

        if !self.errors.is_empty() {
            output.push_str(&format!("\n{} errors:\n", self.errors.len()));
            for error in &self.errors {
                output.push_str(&format!("  ✗ {}\n", error.message.red()));
            }
        }

        output
    }
}
```

### 4. Command Implementation Pattern

```rust
// quarto/src/commands/render.rs

use anyhow::Result;
use quarto_core::render::{render_document, RenderOptions, RenderResult};
use quarto_util::output::OutputWriter;

pub fn execute(ctx: &Context, args: RenderArgs) -> Result<()> {
    // Determine output format (per-command flag overrides global)
    let format = if args.json {
        OutputFormat::Json
    } else {
        ctx.format
    };

    let output_writer = OutputWriter::new(format, ctx.quiet);

    // Progress messages (human-only, to stderr)
    output_writer.write_progress("Rendering document...");

    // Business logic - returns structured data
    let options = RenderOptions {
        input: args.input,
        to: args.to,
        execute: args.execute,
        // ... other options
    };

    let result = render_document(options)?;

    // Write result in appropriate format (to stdout)
    output_writer.write(&result)?;

    // Exit code based on success
    if !result.success {
        std::process::exit(1);
    }

    Ok(())
}
```

## I/O Separation Pattern

Following cargo's pattern:

**stdout:** Structured data only (JSON, YAML)
- Command results
- Query responses
- Data exports

**stderr:** Human-readable messages
- Progress indicators
- Status messages
- Warnings (in human mode)
- Errors

This allows users to pipe stdout to other tools without interference:

```bash
# Capture JSON output, see progress on screen
quarto render doc.qmd --format json | jq '.output'

# Quiet mode - no progress, only JSON
quarto render doc.qmd --format json --quiet | process.py
```

## Streaming Output for Long Operations

For long-running commands (render, preview), emit line-delimited JSON events:

```rust
pub fn render_project(ctx: &Context, args: RenderArgs) -> Result<()> {
    let output = OutputWriter::new(ctx.format, ctx.quiet);

    for file in project.files() {
        // Emit progress event
        output.write_event(&StreamEvent {
            type_: EventType::Progress,
            timestamp: now(),
            message: format!("Rendering {}", file.display()),
            data: Some(json!({
                "file": file,
                "total": project.files().len(),
            })),
        })?;

        let result = render_file(file)?;

        // Emit result event
        output.write_event(&StreamEvent {
            type_: EventType::Result,
            timestamp: now(),
            message: format!("Completed {}", file.display()),
            data: Some(result),
        })?;
    }

    Ok(())
}
```

**JSON output (line-delimited):**
```json
{"type":"progress","timestamp":1699999999,"message":"Rendering doc1.qmd","data":{"file":"doc1.qmd","total":3}}
{"type":"result","timestamp":1699999999,"message":"Completed doc1.qmd","data":{"input":"doc1.qmd","output":"doc1.html","success":true}}
{"type":"progress","timestamp":1700000000,"message":"Rendering doc2.qmd","data":{"file":"doc2.qmd","total":3}}
...
```

**Human output:**
```
Rendering doc1.qmd...
  ✓ Completed (1.2s)
Rendering doc2.qmd...
  ✓ Completed (0.8s)
```

## Input: Configuration File Integration

Use `clap-serde-derive` for layered configuration:

```rust
use clap::Parser;
use serde::Deserialize;
use clap_serde_derive::ClapSerde;

#[derive(Parser)]
struct RenderArgs {
    /// Input file or project
    input: Option<PathBuf>,

    /// Flatten serde-compatible options
    #[command(flatten)]
    pub config: <RenderConfig as ClapSerde>::Opt,
}

#[derive(ClapSerde, Deserialize)]
struct RenderConfig {
    /// Output format
    #[arg(long)]
    to: Option<String>,

    /// Execute code
    #[arg(long)]
    execute: bool,

    /// Metadata values
    #[arg(long)]
    metadata: Vec<String>,
}

// Load configuration with priority:
// 1. Defaults
// 2. Project config (_quarto.yml)
// 3. User config (~/.quarto/config.yml)
// 4. Environment variables (QUARTO_*)
// 5. CLI arguments (highest priority)

pub fn load_config(args: &RenderArgs) -> Result<RenderConfig> {
    let defaults = RenderConfig::default();
    let project_config = load_project_config()?;
    let user_config = load_user_config()?;
    let env_config = load_env_config()?;

    // Merge with priority
    let config = defaults
        .merge(project_config)
        .merge(user_config)
        .merge(env_config)
        .merge(&args.config);

    Ok(config)
}
```

## Schema Versioning

For stability, version the JSON output schema:

```rust
#[derive(Serialize)]
pub struct RenderResult {
    /// Schema version for this output
    pub schema_version: &'static str,

    // ... rest of fields
}

impl RenderResult {
    pub fn new() -> Self {
        Self {
            schema_version: "2.0.0",
            // ...
        }
    }
}
```

Consumers can check `schema_version` to handle format changes.

## Auto-Detection vs Explicit

Default behavior uses auto-detection:

```rust
impl Default for OutputFormat {
    fn default() -> Self {
        use std::io::IsTerminal;

        if std::io::stdout().is_terminal() {
            OutputFormat::Human  // Terminal: pretty output
        } else {
            OutputFormat::Json   // Piped: machine-readable
        }
    }
}
```

Users can override:
```bash
# Force human output even when piped (for logging)
quarto render --format human | tee log.txt

# Force JSON even in terminal (for testing)
quarto render --format json
```

## Dependencies

Add to workspace:

```toml
[workspace.dependencies]
# Output formatting
colored = "2.1"                    # Terminal colors
serde_yaml = "0.9"                 # YAML support

# Config file integration
clap-serde-derive = "0.2"          # Merge clap + config files
config = "0.14"                    # Multi-source config
```

Add to `quarto-util/Cargo.toml`:

```toml
[dependencies]
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
colored = "2.1"
```

## Examples

### Example 1: Simple Render

**Command:**
```bash
quarto render doc.qmd --format json
```

**Output (stdout):**
```json
{"schema_version":"2.0.0","input":"doc.qmd","output":"doc.html","format":"html","execution_time_ms":1234,"warnings":[],"errors":[],"success":true}
```

### Example 2: Render with Warnings

**Command:**
```bash
quarto render doc.qmd
```

**Output (human):**
```
Rendering doc.qmd...
  Format: html

  ⚠ 2 warnings:
    ⚠ Undefined citation: @smith2020
    ⚠ Missing alt text for image

✓ Completed in 1.2s
Output: doc.html
```

**Same with JSON:**
```bash
quarto render doc.qmd --json
```

```json
{"schema_version":"2.0.0","input":"doc.qmd","output":"doc.html","format":"html","execution_time_ms":1234,"warnings":[{"message":"Undefined citation: @smith2020","file":"doc.qmd","line":42},{"message":"Missing alt text for image","file":"doc.qmd","line":89}],"errors":[],"success":true}
```

### Example 3: Inspect Command (Backward Compat)

```bash
# v1 style (still works)
quarto inspect doc.qmd --json

# v2 style (equivalent)
quarto inspect doc.qmd --format json
```

### Example 4: Streaming Progress

**Command:**
```bash
quarto render --format json
```

**Output (line-delimited JSON):**
```json
{"type":"progress","timestamp":1699999999,"message":"Scanning project...","data":{"files_found":10}}
{"type":"progress","timestamp":1699999999,"message":"Rendering index.qmd","data":{"file":"index.qmd","progress":"1/10"}}
{"type":"result","timestamp":1700000000,"message":"Completed index.qmd","data":{"input":"index.qmd","output":"index.html","success":true}}
{"type":"progress","timestamp":1700000001,"message":"Rendering about.qmd","data":{"file":"about.qmd","progress":"2/10"}}
...
```

### Example 5: Piping to Other Tools

```bash
# Extract output paths from render
quarto render --format json --quiet | jq -r '.output'

# Check for errors
quarto render --format json --quiet | jq 'select(.success == false)'

# Parse warnings
quarto render --format json | jq '.warnings[] | "\(.file):\(.line): \(.message)"'

# Convert to different format
quarto render --format yaml | yq '.output'
```

## Implementation Phases

### Phase 1: Foundation (Week 1)
- [ ] Add `OutputFormat` enum and global `--format` flag
- [ ] Implement `OutputWriter` in `quarto-util`
- [ ] Implement `Outputable` trait
- [ ] Add `colored` and `serde_yaml` dependencies

### Phase 2: Core Commands (Week 2-3)
- [ ] Implement `RenderResult` and `Outputable` for render
- [ ] Update `render` command to use `OutputWriter`
- [ ] Add JSON schema versioning
- [ ] Implement streaming events for project renders

### Phase 3: Additional Commands (Week 4)
- [ ] Extend to `inspect`, `check`, `list`, `tools` commands
- [ ] Add backward-compatible `--json` flags
- [ ] Implement auto-detection (terminal vs piped)

### Phase 4: Input Integration (Week 5)
- [ ] Add `clap-serde-derive` for config file integration
- [ ] Implement layered configuration loading
- [ ] Support environment variable overrides

### Phase 5: Polish & Documentation (Week 6)
- [ ] Document JSON schemas for all commands
- [ ] Add examples to `--help` output
- [ ] Write user documentation
- [ ] Add integration tests

## Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_output() {
        let result = RenderResult {
            input: PathBuf::from("test.qmd"),
            output: PathBuf::from("test.html"),
            format: "html".to_string(),
            execution_time_ms: 1000,
            warnings: vec![],
            errors: vec![],
            success: true,
        };

        let json = result.format_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["format"], "html");
    }

    #[test]
    fn test_human_output() {
        let result = RenderResult { /* ... */ };
        let human = result.format_human();

        assert!(human.contains("✓"));
        assert!(human.contains("test.html"));
    }

    #[test]
    fn test_streaming_events() {
        let writer = OutputWriter::new(OutputFormat::Json, false);

        let event = StreamEvent {
            type_: EventType::Progress,
            timestamp: 123456789,
            message: "Processing...".to_string(),
            data: Some(json!({"file": "test.qmd"})),
        };

        // Capture stdout
        let output = capture_stdout(|| {
            writer.write_event(&event).unwrap();
        });

        assert!(output.contains(r#""type":"progress"#));
        assert!(output.contains(r#""file":"test.qmd"#));
    }
}
```

## References

- [Command Line Applications in Rust - Machine Communication](https://rust-cli.github.io/book/in-depth/machine-communication.html)
- [Cargo's `--message-format json`](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
- [ripgrep's `--json` output](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md#json)
- [clap-serde crate](https://docs.rs/clap-serde/)
- [Line-delimited JSON](https://en.wikipedia.org/wiki/JSON_streaming#Line-delimited_JSON)

## Future Enhancements

1. **MessagePack Support**: Binary format for even faster parsing
2. **CBOR Support**: Compact binary format with schema support
3. **WebSocket Mode**: For interactive tools (IDE integration)
4. **gRPC API**: For advanced programmatic access
5. **Schema Registry**: Formal JSON schema definitions published separately
