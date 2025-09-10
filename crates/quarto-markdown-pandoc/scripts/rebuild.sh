#!/bin/bash
./scripts/build_error_table.ts > resources/error-corpus/_autogen-table.json
touch src/readers/qmd_error_message_table.rs
cargo build