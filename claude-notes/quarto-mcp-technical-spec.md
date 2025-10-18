# Quarto MCP Server - Technical Specification

## Overview

This document provides technical implementation details for the Quarto MCP server, including API schemas, code examples, and integration patterns.

## MCP Protocol Basics

### Transport Layer

The server will support stdio transport (primary) with optional SSE/WebSocket:

```rust
use rmcp::prelude::*;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = QuartoMcpServer::new();

    // stdio transport for Claude Desktop, Cursor, etc.
    let transport = StdioTransport::new();

    server.serve(transport).await?;
    Ok(())
}
```

### Message Format

All communication uses JSON-RPC 2.0:

```json
// Client request
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "render_document",
    "arguments": {
      "path": "analysis.qmd",
      "format": "html"
    }
  }
}

// Server response
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Successfully rendered analysis.qmd to analysis.html"
      }
    ]
  }
}
```

## Resource Specifications

### 1. Project Configuration

**URI**: `project://config`

**Response Schema**:
```json
{
  "type": "object",
  "properties": {
    "project": {
      "type": "object",
      "properties": {
        "type": { "enum": ["default", "website", "book", "manuscript"] },
        "output-dir": { "type": "string" },
        "title": { "type": "string" }
      }
    },
    "format": {
      "type": "object",
      "additionalProperties": true
    },
    "execute": {
      "type": "object",
      "properties": {
        "freeze": { "type": "boolean" },
        "cache": { "type": "boolean" }
      }
    }
  }
}
```

**Example Response**:
```json
{
  "uri": "project://config",
  "mimeType": "application/json",
  "text": "{
    \"project\": {
      \"type\": \"website\",
      \"output-dir\": \"_site\"
    },
    \"website\": {
      \"title\": \"My Analysis Site\",
      \"navbar\": {
        \"left\": [{\"href\": \"index.qmd\", \"text\": \"Home\"}]
      }
    },
    \"format\": {
      \"html\": {
        \"theme\": \"cosmo\",
        \"code-fold\": true
      }
    }
  }"
}
```

### 2. Document Metadata

**URI**: `document://{path}/metadata`

**Response Schema**:
```json
{
  "type": "object",
  "properties": {
    "title": { "type": "string" },
    "author": { "type": ["string", "array"] },
    "date": { "type": "string" },
    "format": { "type": "object" },
    "execute": { "type": "object" },
    "bibliography": { "type": "string" },
    "citations": { "type": "array" }
  }
}
```

**Example Request**:
```json
{
  "method": "resources/read",
  "params": {
    "uri": "document://analysis.qmd/metadata"
  }
}
```

**Example Response**:
```json
{
  "uri": "document://analysis.qmd/metadata",
  "mimeType": "application/json",
  "text": "{
    \"title\": \"Statistical Analysis\",
    \"author\": \"Data Team\",
    \"format\": {
      \"html\": {
        \"code-fold\": true,
        \"toc\": true
      }
    },
    \"execute\": {
      \"echo\": true,
      \"warning\": false
    }
  }"
}
```

### 3. Code Cells

**URI**: `document://{path}/cells`

**Response Schema**:
```json
{
  "type": "object",
  "properties": {
    "cells": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": { "type": "string" },
          "language": { "type": "string" },
          "code": { "type": "string" },
          "options": { "type": "object" },
          "line_start": { "type": "number" },
          "line_end": { "type": "number" }
        }
      }
    }
  }
}
```

**Example Response**:
```json
{
  "uri": "document://analysis.qmd/cells",
  "mimeType": "application/json",
  "text": "{
    \"cells\": [
      {
        \"id\": \"cell-1\",
        \"language\": \"python\",
        \"code\": \"import pandas as pd\\ndf = pd.read_csv('data.csv')\",
        \"options\": {\"echo\": true, \"warning\": false},
        \"line_start\": 15,
        \"line_end\": 17
      },
      {
        \"id\": \"cell-2\",
        \"language\": \"python\",
        \"code\": \"df.describe()\",
        \"options\": {},
        \"line_start\": 25,
        \"line_end\": 26
      }
    ]
  }"
}
```

### 4. Project Files

**URI**: `project://files`

**Response Schema**:
```json
{
  "type": "object",
  "properties": {
    "files": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "path": { "type": "string" },
          "type": { "enum": ["qmd", "md", "ipynb", "rmd", "config", "data", "asset"] },
          "engine": { "type": "string" },
          "formats": { "type": "array" }
        }
      }
    }
  }
}
```

## Tool Specifications

### 1. Render Document

**Name**: `render_document`

**Description**: Render a Quarto document to specified format(s)

**Parameters**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the document to render"
    },
    "format": {
      "type": "string",
      "description": "Output format (html, pdf, docx, etc.). Defaults to all formats in YAML.",
      "optional": true
    },
    "execute": {
      "type": "boolean",
      "description": "Whether to execute code cells. Defaults to true.",
      "optional": true
    },
    "params": {
      "type": "object",
      "description": "Parameter overrides for parameterized documents",
      "optional": true
    }
  },
  "required": ["path"]
}
```

**Implementation Sketch**:
```rust
#[tool(tool_box)]
impl QuartoMcpServer {
    #[tool(description = "Render a Quarto document to specified format(s)")]
    async fn render_document(
        &self,
        #[tool(aggr)] params: RenderParams
    ) -> Result<CallToolResult, Error> {
        let path = PathBuf::from(&params.path);

        // Validate document exists
        if !path.exists() {
            return Err(Error::msg(format!("Document not found: {}", params.path)));
        }

        // Build render command
        let mut cmd = Command::new("quarto");
        cmd.arg("render").arg(&params.path);

        if let Some(format) = &params.format {
            cmd.arg("--to").arg(format);
        }

        if let Some(false) = params.execute {
            cmd.arg("--execute").arg("false");
        }

        // Execute render
        let output = cmd.output().await?;

        if output.status.success() {
            Ok(CallToolResult {
                content: vec![
                    ToolContent::Text(TextContent {
                        text: format!("Successfully rendered {} to {}",
                            params.path,
                            params.format.as_deref().unwrap_or("default format")
                        )
                    })
                ],
                is_error: false
            })
        } else {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            Ok(CallToolResult {
                content: vec![
                    ToolContent::Text(TextContent {
                        text: format!("Render failed:\n{}", error_msg)
                    })
                ],
                is_error: true
            })
        }
    }
}
```

### 2. Inspect Project

**Name**: `inspect_project`

**Description**: Inspect a Quarto project and return its configuration and structure

**Parameters**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the project directory. Defaults to current directory.",
      "optional": true
    }
  }
}
```

**Implementation**:
```rust
#[tool(description = "Inspect a Quarto project and return its configuration")]
async fn inspect_project(
    &self,
    #[tool(aggr)] params: InspectParams
) -> Result<CallToolResult, Error> {
    let path = params.path.unwrap_or_else(|| ".".to_string());

    // Run quarto inspect
    let output = Command::new("quarto")
        .arg("inspect")
        .arg(&path)
        .output()
        .await?;

    if output.status.success() {
        let json_str = String::from_utf8(output.stdout)?;

        Ok(CallToolResult {
            content: vec![
                ToolContent::Text(TextContent {
                    text: json_str
                })
            ],
            is_error: false
        })
    } else {
        Err(Error::msg("Failed to inspect project"))
    }
}
```

### 3. Execute Code Cell

**Name**: `execute_cell`

**Description**: Execute a specific code cell from a document

**Parameters**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the document"
    },
    "cell_id": {
      "type": "string",
      "description": "ID or index of the cell to execute"
    },
    "capture_output": {
      "type": "boolean",
      "description": "Whether to capture and return output. Defaults to true.",
      "optional": true
    }
  },
  "required": ["path", "cell_id"]
}
```

**Implementation Approach**:
1. Parse document to extract cell
2. Create temporary execution context
3. Execute via appropriate engine (Jupyter kernel, Knitr, etc.)
4. Capture and return output
5. Handle errors gracefully

### 4. Validate YAML

**Name**: `validate_yaml`

**Description**: Validate YAML frontmatter against Quarto schema

**Parameters**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the document to validate"
    },
    "strict": {
      "type": "boolean",
      "description": "Enable strict validation. Defaults to false.",
      "optional": true
    }
  },
  "required": ["path"]
}
```

**Response on Success**:
```json
{
  "content": [
    {
      "type": "text",
      "text": "YAML validation passed: No errors found"
    }
  ],
  "is_error": false
}
```

**Response on Error**:
```json
{
  "content": [
    {
      "type": "text",
      "text": "YAML validation failed:\n- Line 5: Unknown property 'titel' (did you mean 'title'?)\n- Line 12: Invalid value for 'toc'. Expected boolean, got string."
    }
  ],
  "is_error": true
}
```

### 5. List Formats

**Name**: `list_formats`

**Description**: List available output formats for a document

**Parameters**:
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to the document"
    }
  },
  "required": ["path"]
}
```

**Example Response**:
```json
{
  "content": [
    {
      "type": "text",
      "text": "Available formats for analysis.qmd:\n- html (default)\n- pdf\n- docx\n- revealjs"
    }
  ]
}
```

## Prompt Specifications

### 1. Debug Render Error

**Name**: `debug_render_error`

**Description**: Help diagnose and fix document rendering errors

**Template**:
```
You are helping a user debug a Quarto rendering error.

Document: {document_path}
Format: {format}
Error output:
{error_message}

Please analyze the error and provide:
1. A clear explanation of what went wrong
2. The likely root cause
3. Specific steps to fix the issue
4. Any related documentation links

Common issues to check:
- YAML syntax errors
- Missing dependencies (R packages, Python modules)
- Invalid crossreferences
- Code execution errors
- Format-specific requirements
```

### 2. Create Article

**Name**: `create_article`

**Description**: Interactive workflow for creating a new Quarto article

**Template**:
```
You are helping a user create a new Quarto article.

Please gather the following information:
1. Article title
2. Author name(s)
3. Computational engine (jupyter for Python, knitr for R, or none)
4. Output format (html, pdf, docx, or multiple)
5. Any special features (citations, cross-references, code folding, etc.)

Once you have this information, create a document with:
- Appropriate YAML frontmatter
- Section structure
- Example code cells (if computational)
- Placeholder content with guidance

After creation, offer to:
- Render the document
- Add citations/bibliography
- Configure additional formats
```

### 3. Fix YAML

**Name**: `fix_yaml`

**Description**: Interactive YAML error fixing workflow

**Template**:
```
You are helping a user fix YAML frontmatter errors in their Quarto document.

Document: {document_path}
Validation errors:
{validation_errors}

For each error:
1. Explain what's wrong in simple terms
2. Show the current (incorrect) YAML
3. Suggest the corrected YAML
4. Explain why the correction is needed

After suggesting fixes, offer to:
- Apply the corrections automatically
- Validate the corrected YAML
- Explain any format-specific requirements
```

## Integration Examples

### Claude Desktop Configuration

Add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "quarto": {
      "command": "quarto",
      "args": ["mcp"]
    }
  }
}
```

### Cursor Configuration

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "quarto": {
      "command": "quarto",
      "args": ["mcp"],
      "cwd": "${workspaceFolder}"
    }
  }
}
```

### VS Code with Continue

Add to `~/.continue/config.json`:

```json
{
  "experimental": {
    "modelContextProtocolServers": [
      {
        "name": "quarto",
        "command": "quarto",
        "args": ["mcp"]
      }
    ]
  }
}
```

## Usage Examples

### Example 1: Render and Fix Errors

```
User: "Render my analysis.qmd to HTML"

Claude (using MCP):
1. Calls render_document(path="analysis.qmd", format="html")
2. Receives error about missing Python package
3. Suggests: "The render failed because pandas is not installed.
   Would you like me to help you install it?"

User: "Yes, install it"

Claude:
1. Detects it's a Jupyter document
2. Suggests running: pip install pandas
3. Offers to re-render after installation
```

### Example 2: Create New Document

```
User: "Help me create a new data analysis document with Python"

Claude (using prompt template):
1. Uses create_article prompt
2. Asks for title, format preferences
3. Calls create_document(path="new-analysis.qmd", template="analysis", engine="jupyter")
4. Shows created document structure
5. Offers to render a preview
```

### Example 3: Project Inspection

```
User: "What's the structure of my Quarto website?"

Claude:
1. Calls inspect_project()
2. Reads project://config resource
3. Reads project://files resource
4. Responds: "Your Quarto website has:
   - 15 pages (12 .qmd, 3 .md)
   - Website configuration with navbar and sidebar
   - Output directory: _site
   - Formats: HTML with cosmo theme
   - 3 pages use Python (Jupyter), 2 use R (Knitr)"
```

### Example 4: Validate and Fix YAML

```
User: "Why isn't my document rendering?"

Claude:
1. Calls validate_yaml(path="document.qmd")
2. Finds errors in YAML frontmatter
3. Uses fix_yaml prompt to explain errors
4. Shows corrected YAML
5. Asks permission to apply fixes
6. Re-validates after fixes
```

## Error Handling

### Standard Error Response

All tools should return errors in this format:

```json
{
  "content": [
    {
      "type": "text",
      "text": "Error: {error_type}\n\nDetails: {error_details}\n\nSuggestion: {suggested_fix}"
    }
  ],
  "is_error": true
}
```

### Error Categories

1. **Validation Errors**: YAML syntax, schema violations
2. **Execution Errors**: Code cell failures, missing dependencies
3. **File Errors**: Not found, permission denied
4. **Rendering Errors**: Pandoc errors, format-specific issues
5. **Configuration Errors**: Invalid project setup

## Performance Considerations

### Caching Strategy

1. **Project Metadata**: Cache _quarto.yml parsing (invalidate on file change)
2. **Document Metadata**: Cache frontmatter parsing (invalidate on file change)
3. **Schema Validation**: Cache schemas (invalidate on Quarto version change)
4. **Inspection Results**: Cache for 30s (balance freshness vs performance)

### Async Operations

All I/O operations should be async:
- File reading (tokio::fs)
- Process execution (tokio::process::Command)
- Network requests (for publishing, etc.)

### Resource Limits

- Maximum file size for inspection: 10MB
- Maximum project size: 1000 files
- Execution timeout: 300s (configurable)
- Concurrent renders: 3 (configurable)

## Testing Strategy

### Unit Tests

Test each tool/resource independently:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_render_document() {
        let server = QuartoMcpServer::new();
        let params = RenderParams {
            path: "test/fixtures/simple.qmd".to_string(),
            format: Some("html".to_string()),
            execute: Some(false),
            params: None,
        };

        let result = server.render_document(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_yaml() {
        let server = QuartoMcpServer::new();
        let params = ValidateParams {
            path: "test/fixtures/invalid.qmd".to_string(),
            strict: Some(true),
        };

        let result = server.validate_yaml(params).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_error);
    }
}
```

### Integration Tests

Test full MCP protocol flow:

1. Start server with stdio transport
2. Send JSON-RPC requests
3. Validate responses
4. Test error conditions

### End-to-End Tests

Test with real MCP clients:
- Claude Desktop integration
- Cursor integration
- Custom test client

## Security Model

### Capabilities Declaration

Server declares capabilities in initialization:

```rust
fn get_info(&self) -> ServerInfo {
    ServerInfo {
        protocol_version: ProtocolVersion::V_2024_11_05,
        capabilities: ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .build(),
        server_info: Implementation {
            name: "quarto-mcp".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        instructions: Some(
            "Quarto MCP server for AI-assisted document authoring and rendering"
                .to_string()
        ),
    }
}
```

### Permission Model

1. **Read-only resources**: No confirmation needed
2. **Rendering**: Auto-approved (safe operation)
3. **File creation**: Confirm with user
4. **Code execution**: Confirm with user
5. **Publishing**: Always confirm with detailed summary

### Sandboxing

For code execution:
1. Run in isolated environment
2. Resource limits (CPU, memory, time)
3. Network access restrictions
4. File system access limited to project directory

## Future Enhancements

### Phase 2 Features (Post-MVP)

1. **Collaborative Features**
   - Multi-user project inspection
   - Change tracking via resources
   - Real-time preview updates

2. **Advanced Analytics**
   - Document complexity metrics
   - Citation analysis
   - Code quality metrics

3. **Integration Enhancements**
   - GitHub integration (issues, PRs)
   - Direct publishing to Quarto Pub
   - Extension marketplace integration

4. **Performance Optimizations**
   - Incremental rendering
   - Parallel document processing
   - Smart caching strategies

### Extension API

Allow community to add custom tools/resources:

```rust
pub trait McpExtension {
    fn name(&self) -> &str;
    fn tools(&self) -> Vec<Tool>;
    fn resources(&self) -> Vec<Resource>;
    fn prompts(&self) -> Vec<Prompt>;
}
```

## Conclusion

This technical specification provides a complete blueprint for implementing the Quarto MCP server. The design prioritizes:

1. **Standards compliance**: Full MCP protocol support
2. **User experience**: Rich, helpful tools and prompts
3. **Performance**: Async operations, caching, resource limits
4. **Security**: Permission model, sandboxing, validation
5. **Extensibility**: Clear extension points for future growth

Next step: 2-day Rust spike to validate core architecture and MCP library integration.
