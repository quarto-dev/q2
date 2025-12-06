# Quarto Markdown Pandoc - Project Overview

## Purpose
This Rust library and binary crate converts Markdown text to Pandoc's AST representation using a custom tree-sitter grammar for Markdown. It is part of the Quarto ecosystem for creating scientific and technical documents.

## Key Features
- **Two-Stage Parsing**: Uses a unique tree-sitter setup with separate grammars for block structure and inline structure within blocks
- **Pandoc AST Output**: Generates Pandoc's native AST format for compatibility with the Pandoc ecosystem
- **Multiple Output Formats**: Supports JSON and native Pandoc output formats
- **Command-Line Interface**: Provides a CLI tool for converting Quarto markdown documents

## Architecture
The project follows a modular architecture with clear separation of concerns:

### Core Components
- **Parser**: Uses `tree-sitter-qmd` for markdown parsing
- **Traversal System**: Custom traversal helpers in `traversals.rs` for navigating the tree-sitter data structure
- **Pandoc AST**: Data structures that mirror Pandoc's type system
- **Writers**: Output formatters for different target formats
- **Filters**: Processing pipeline for AST transformations

### Dependencies
- `tree-sitter` and `tree-sitter-qmd` for parsing
- `regex` for pattern matching
- `clap` for command-line interface
- `serde_json` for JSON serialization
- `glob` for file pattern matching

## Design Philosophy
The project emphasizes correctness and precision, particularly in source location tracking. It aims to maintain compatibility with Pandoc while providing enhanced features for Quarto-specific constructs like footnotes and shortcodes.

## Target Use Cases
- Converting Quarto markdown documents to Pandoc AST
- Integration with larger Quarto publishing pipeline
- Standalone markdown processing for scientific documents
- Development and testing of markdown parsing strategies