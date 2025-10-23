---
description: Use rust-analyzer LSP for Rust code analysis and refactoring
---

# Rust Analyzer Skill

Use this skill when working with Rust code to:
- Find symbol definitions and references
- Rename symbols safely across the codebase
- Get type information and hover documentation
- Find implementations of traits
- Apply code actions and quick fixes
- Expand macros
- Get diagnostics for files

This is MUCH more reliable than sed scripts for Rust code modifications.

## When to Use This Skill

- Renaming functions, variables, types, or modules
- Finding all usages of a symbol
- Understanding type information
- Finding trait implementations
- Applying automated refactorings
- Getting accurate diagnostics

## Available Operations

### 1. Rename Symbol
Safely rename a symbol across the entire codebase with proper scope awareness.

```bash
# Start rust-analyzer in the background
cd /path/to/project
rust-analyzer lsp-server &
RA_PID=$!

# Use rust-analyzer CLI for rename (if available)
# Note: rust-analyzer primarily works via LSP protocol
# You'll need to use LSP client tools like 'rust-analyzer rename'
```

### 2. Find References
Find all references to a symbol.

```bash
# Use grep as fallback, but rust-analyzer via LSP is more accurate
rg --type rust "symbol_name"
```

### 3. Go to Definition
Find where a symbol is defined.

### 4. Get Hover Information
Get type information and documentation for a symbol.

### 5. Code Actions
Apply quick fixes and refactorings suggested by rust-analyzer.

## LSP Communication Pattern

rust-analyzer communicates via JSON-RPC over stdin/stdout. Here's how to interact with it:

### Starting the LSP Server

```bash
# Start rust-analyzer in LSP mode
rust-analyzer lsp-server
```

### LSP Request Format

All LSP requests follow this JSON-RPC format:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "methodName",
  "params": {}
}
```

### Common LSP Methods

1. **Initialize** - Must be called first
   - Method: `initialize`
   - Params: `{ "rootUri": "file:///path/to/project", "capabilities": {} }`

2. **Goto Definition**
   - Method: `textDocument/definition`
   - Params: `{ "textDocument": { "uri": "file://..." }, "position": { "line": 0, "character": 0 } }`

3. **Find References**
   - Method: `textDocument/references`
   - Params: `{ "textDocument": { "uri": "file://..." }, "position": { "line": 0, "character": 0 }, "context": { "includeDeclaration": true } }`

4. **Rename**
   - Method: `textDocument/rename`
   - Params: `{ "textDocument": { "uri": "file://..." }, "position": { "line": 0, "character": 0 }, "newName": "new_name" }`

5. **Hover**
   - Method: `textDocument/hover`
   - Params: `{ "textDocument": { "uri": "file://..." }, "position": { "line": 0, "character": 0 } }`

6. **Code Action**
   - Method: `textDocument/codeAction`
   - Params: `{ "textDocument": { "uri": "file://..." }, "range": { "start": {...}, "end": {...} }, "context": { "diagnostics": [] } }`

## Helper Function for LSP Communication

```python
#!/usr/bin/env python3
import json
import subprocess
import sys

def send_lsp_request(method, params, project_root):
    """Send an LSP request to rust-analyzer and return the response."""

    # Start rust-analyzer
    proc = subprocess.Popen(
        ['rust-analyzer', 'lsp-server'],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=project_root
    )

    # Initialize
    initialize_request = {
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "rootUri": f"file://{project_root}",
            "capabilities": {
                "textDocument": {
                    "rename": {"prepareSupport": True},
                    "references": {},
                    "definition": {},
                    "hover": {"contentFormat": ["markdown", "plaintext"]}
                }
            }
        }
    }

    send_message(proc, initialize_request)
    response = read_response(proc)

    # Send initialized notification
    initialized_notification = {
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }
    send_message(proc, initialized_notification)

    # Send actual request
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    }

    send_message(proc, request)
    result = read_response(proc)

    # Shutdown
    shutdown_request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": None
    }
    send_message(proc, shutdown_request)
    read_response(proc)

    proc.stdin.close()
    proc.wait()

    return result

def send_message(proc, message):
    """Send a JSON-RPC message."""
    content = json.dumps(message)
    header = f"Content-Length: {len(content)}\r\n\r\n"
    proc.stdin.write(header + content)
    proc.stdin.flush()

def read_response(proc):
    """Read a JSON-RPC response."""
    # Read headers
    headers = {}
    while True:
        line = proc.stdout.readline()
        if line == '\r\n':
            break
        if ':' in line:
            key, value = line.split(':', 1)
            headers[key.strip()] = value.strip()

    # Read content
    content_length = int(headers.get('Content-Length', 0))
    content = proc.stdout.read(content_length)
    return json.loads(content)

if __name__ == "__main__":
    # Example usage
    if len(sys.argv) < 2:
        print("Usage: lsp_helper.py <command> [args...]")
        sys.exit(1)

    command = sys.argv[1]
    # Add command handling here
```

## Practical Bash Helper

For simpler use cases, here's a bash-based approach:

```bash
#!/bin/bash
# Helper script for common rust-analyzer operations

RA_HELPER_DIR=$(mktemp -d)
trap "rm -rf $RA_HELPER_DIR" EXIT

# Function to send LSP request
send_lsp_request() {
    local method="$1"
    local params="$2"
    local project_root="${3:-.}"

    # Use a Python one-liner or similar
    python3 -c "
import json, subprocess, sys

proc = subprocess.Popen(
    ['rust-analyzer', 'lsp-server'],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    cwd='$project_root'
)

# Initialize and send request
# (implementation details from above)
"
}

# Example: Find references
find_references() {
    local file="$1"
    local line="$2"
    local char="$3"

    # For now, use ripgrep as a simpler alternative
    # Full LSP implementation would be more accurate
    rg --type rust --line-number --column "pattern"
}
```

## Simpler Approach: Use cargo and ripgrep

For many common tasks, these built-in tools work well:

```bash
# Find all references to a symbol
rg --type rust "\\bsymbol_name\\b"

# Find definitions (functions, structs, etc.)
rg --type rust "^\\s*(pub\\s+)?(fn|struct|enum|trait|impl)\\s+symbol_name"

# Find usages in a specific crate
rg --type rust "symbol_name" crates/specific-crate/

# Get type information via cargo check
cargo check --message-format=json 2>&1 | jq -r 'select(.reason == "compiler-message") | .message.rendered'

# Expand macros
cargo expand --lib path::to::module

# Run clippy for suggestions
cargo clippy --message-format=json 2>&1 | jq -r 'select(.reason == "compiler-message") | .message.rendered'
```

## Best Practices

1. **For Renaming**: Use cargo-based search first to understand scope, then use Edit tool with careful string matching
2. **For Finding References**: Use ripgrep with word boundaries `\b`
3. **For Type Information**: Use cargo check or cargo clippy
4. **For Macro Expansion**: Use cargo-expand
5. **For Refactoring**: Break into small steps, test after each change

## Instructions for Claude

When you invoke this skill:

1. Determine what Rust operation is needed
2. Choose the appropriate tool:
   - Simple renames in single files → Use Edit tool directly
   - Finding all usages → Use ripgrep with appropriate patterns
   - Type information → Use cargo check
   - Complex refactoring → Break into smaller steps
3. Always verify changes with `cargo check` or `cargo test`
4. For complex LSP operations, consider asking the user if they have an LSP client configured

Remember: For this monorepo, prefer simple, reliable tools over complex LSP interactions unless specifically needed.
