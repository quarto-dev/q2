# Quarto 2

> **Experimental** - This project is under active development. It's not yet ready for production use, and will not be for a while.

This repository is a Rust implementation of the next version of [Quarto](https://quarto.org). The goal is to replace parts of the TypeScript/Deno runtime with a unified Rust implementation, enabling:

- Shared validation logic between CLI and Language Server Protocol (LSP)
- Improved performance, particularly for LSP operations
- Single-binary distribution

## Why Rust?

Posit has been investing in Rust for developer tooling:

- [Air](https://github.com/posit-dev/air/) - An R formatter and LSP written in Rust
- [Ark](https://github.com/posit-dev/ark/) - An R kernel for Jupyter written in Rust

Rust offers compelling advantages for Quarto's tooling:

- **Performance** - Native compilation provides significant speedups for parsing and validation, critical for responsive LSP experiences
- **WebAssembly** - Rust compiles to WASM, enabling browser-based tooling and editor integrations without separate runtime dependencies
- **Single binary** - No runtime installation required; simpler distribution and deployment
- **Memory safety** - Eliminates entire classes of bugs without garbage collection overhead

## Key Crates

### pampa

The most mature crate in this workspace. **pampa** is our Rust port of [Pandoc](https://pandoc.org), the universal document converter. While not a feature-for-feature reimplementation, pampa offers many of the same APIs and will feel familiar to Pandoc users.

Currently, pampa focuses on parsing Quarto Markdown (QMD) and producing Pandoc AST output with full source location tracking.

```bash
# Parse QMD to Pandoc JSON
cargo run -p pampa -- input.qmd -t json

# Parse with verbose tree-sitter output (for debugging)
cargo run -p pampa -- input.qmd -t json -v
```

**Features:**
- Tree-sitter based parsing (block + inline grammars)
- Multiple output formats: JSON, HTML, ANSI, Markdown, plaintext
- Lua filter support (Pandoc-compatible)
- Source location tracking through all transformations

### Supporting Infrastructure

The crates in this workspace share a focus on **precise source location tracking** and **uniform error reporting**:

| Crate | Purpose |
|-------|---------|
| `quarto-source-map` | Unified source location tracking with transformation history |
| `quarto-error-reporting` | Structured diagnostics with tidyverse-style formatting |
| `quarto-yaml` | YAML parsing with fine-grained source locations |
| `quarto-xml` | XML parsing with source tracking (for CSL files) |
| `quarto-pandoc-types` | Pandoc AST type definitions |
| `quarto-doctemplate` | Pandoc-compatible document template engine |

## Source Location Tracking

A core design principle: every semantic entity carries source location information through all transformations. This enables:

- Precise error messages pointing to exact locations in source files
- Provenance tracking through string extraction, concatenation, and filtering
- Serializable source info for LSP caching

```rust
// Source info tracks transformations
enum SourceInfo {
    Original { ... },           // Direct file position
    Substring { parent, ... },  // Extracted from parent
    Concat { pieces, ... },     // Multiple sources combined
    FilterProvenance { ... },   // Created by Lua filter
}
```

## Error Reporting

Errors use [ariadne](https://github.com/zesterer/ariadne) for precise, visually clear diagnostics:

```
$ echo '_hello world' | quarto-markdown-pandoc -t json

Error: [Q-2-5] Unclosed Underscore Emphasis
   ╭─[<stdin>:1:13]
   │
 1 │ _hello world
   │ ┬           ┬
   │ ╰────────────── This is the opening '_' mark.
   │             │
   │             ╰── I reached the end of the block before finding a closing '_' for the emphasis.
───╯
```

## Building

Requires Rust nightly (edition 2024).

```bash
# Build all crates
cargo build

# Run tests (uses nextest)
cargo nextest run

# Build WASM module
cd crates/wasm-qmd-parser && wasm-pack build
```

## Contributing

We welcome discussions about the project via GitHub issues.
However, the Quarto team will be working on this codebase internally before we're ready to accept outside contributions or make public binary releases/announcements.
Please feel free to open issues for questions, suggestions, or bug reports.

## Status

This is experimental software. All API should be considered unstable and may completely change.

## License

MIT - See [LICENSE](LICENSE) for details.
