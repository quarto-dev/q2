# quarto-yaml

YAML parsing with source location tracking for the Quarto Rust port.

## Overview

This crate provides `YamlWithSourceInfo`, which wraps `yaml-rust2::Yaml` with source location information for every node in the YAML tree. This enables precise error reporting and source tracking through transformations.

## Design Philosophy

Uses the **owned data approach**: wraps owned `Yaml` values with a parallel children structure for source tracking. This follows rust-analyzer's precedent of using owned data for tree structures.

**Trade-offs:**
- Simple API with no lifetime parameters
- Compatible with config merging across different lifetimes
- Enables LSP caching (serializable)
- ~3x memory overhead (acceptable for config files <10KB)

## Features

- ✅ Parse YAML with complete source tracking
- ✅ Access raw `yaml-rust2::Yaml` for direct manipulation
- ✅ Source-tracked children for error reporting
- ✅ Type-safe access methods
- ⚠️ Basic alias support (converted to Null)
- ⚠️ Tags parsed but not exposed
- 🔴 Single document only (no multi-document streams yet)

## Usage

```rust
use quarto_yaml::{parse, parse_file};

// Parse from string
let yaml = parse(r#"
title: My Document
author: John Doe
tags:
  - rust
  - yaml
"#).unwrap();

// Parse with filename
let yaml = parse_file(content, "config.yaml").unwrap();

// Access raw Yaml
println!("Title: {:?}", yaml.yaml["title"]);

// Source-tracked access
if let Some(title) = yaml.get_hash_value("title") {
    println!("Title at {}:{}",
        title.source_info.line,
        title.source_info.col
    );
}

// Navigate arrays
if let Some(tags) = yaml.get_hash_value("tags") {
    for tag in tags.as_array().unwrap() {
        println!("{} at line {}",
            tag.yaml.as_str().unwrap(),
            tag.source_info.line
        );
    }
}
```

## API Overview

### Core Types

- **`YamlWithSourceInfo`** - Main wrapper with owned Yaml + source tracking
- **`SourceInfo`** - Source location (file, line, col, offset, length)
- **`YamlHashEntry`** - Hash entry with source spans for key, value, and entry

### Functions

- `parse(content: &str) -> Result<YamlWithSourceInfo>`
- `parse_file(content: &str, filename: &str) -> Result<YamlWithSourceInfo>`

### Methods on YamlWithSourceInfo

- `get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo>`
- `get_array_item(&self, index: usize) -> Option<&YamlWithSourceInfo>`
- `as_array(&self) -> Option<&[YamlWithSourceInfo]>`
- `as_hash(&self) -> Option<&[YamlHashEntry]>`
- `is_scalar()`, `is_array()`, `is_hash()` - Type checking
- `len()`, `is_empty()` - Child count

## Implementation Details

### Data Structure

```rust
pub struct YamlWithSourceInfo {
    pub yaml: Yaml,              // Direct access to raw Yaml
    pub source_info: SourceInfo, // This node's location
    children: Children,          // Source-tracked children (private)
}
```

### Parser

Uses yaml-rust2's `MarkedEventReceiver` API to build the tree:
- Event-based parsing (push parser)
- Stack-based tree construction
- Marker provides source positions

## Limitations

1. **Scalar lengths**: Currently approximate (uses value length)
2. **Aliases**: Converted to Null (anchor tracking not implemented)
3. **Tags**: Parsed but not exposed in API
4. **Multi-document**: Only first document parsed

## Future Work

See `claude-notes/implementation-plan.md` for roadmap:

**Phase 2**: Parser improvements (accurate spans, aliases, tags)
**Phase 3**: Public API enhancements (merging, validation)
**Phase 4**: Advanced features (multi-document, streaming)
**Phase 5**: Integration (unified SourceInfo, LSP support)

## Dependencies

- `yaml-rust2 = "0.9"` - YAML parsing with markers
- `serde = "1.0"` - For future serialization
- `thiserror = "1.0"` - Error types

## Testing

```bash
cd crates/quarto-yaml
cargo test
```

All 14 tests passing ✅

## Documentation

```bash
cargo doc --open
```

## License

MIT (same as Kyoto project)

## Notes

This crate is part of the Kyoto project - a Rust port of Quarto CLI. See the main project for context and architecture decisions.

For implementation notes, see `claude-notes/` directory.
