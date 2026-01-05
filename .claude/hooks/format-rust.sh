#!/bin/bash

# Post-tool-use hook that runs cargo fmt on edited Rust files
# This script receives JSON input via stdin with tool_input.file_path

# Read the file path from JSON input
file_path=$(jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Exit if no file path or not a Rust file
if [[ -z "$file_path" ]] || [[ "$file_path" != *.rs ]]; then
    exit 0
fi

# Exit if the file doesn't exist (e.g., was deleted)
if [[ ! -f "$file_path" ]]; then
    exit 0
fi

# Run cargo fmt on the file
cd "$CLAUDE_PROJECT_DIR" || exit 0
cargo fmt -- "$file_path" 2>/dev/null

exit 0
