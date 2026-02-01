# Shortcode Resolution Implementation Plan

**Beads Issue**: kyoto-yq1r
**Created**: 2026-02-01
**Status**: Implementation complete, all bugs fixed

---

## Overview

This plan describes the implementation of shortcode resolution in Rust Quarto. The goal is to:
1. Create the infrastructure for resolving shortcodes in the AST transform pipeline
2. Implement the `meta` shortcode as the first built-in handler
3. Design the system to be extensible for future shortcodes

## Scope

**In scope:**
- Built-in shortcode resolution transform
- `meta` shortcode handler (with dot notation)
- Inline context shortcode resolution
- Diagnostic integration for errors

**Out of scope (deferred):**
- Other built-in shortcodes (`var`, `env`, `pagebreak`, etc.)
- Block shortcodes / text context shortcodes
- User-defined shortcodes (extensions) - will be a separate transform
- Block shortcode validation in inline context

## Background

### Current State

1. **Parsing is complete**: `pampa` already parses `{{< name args... >}}` into `Inline::Shortcode`
2. **Types exist**: `Shortcode` and `ShortcodeArg` are defined in `quarto-pandoc-types`
3. **Shortcodes are skipped**: `MetadataNormalizeTransform` and `ResourceCollectorTransform` currently skip shortcodes
4. **Span conversion exists**: `pampa/src/pandoc/shortcode.rs` has `shortcode_to_span()` for Lua filter compatibility

### TS Quarto Implementation

From the analysis in `claude-notes/plans/2026-01-24-lua-filter-analysis.md`:

1. **Pipeline position**: Shortcode resolution runs in `quarto_pre_filters` via `pre-shortcodes-filter`
2. **Handler architecture**: Handlers are registered by name and called with `(args, kwargs, meta, raw_args, context)`
3. **`meta` shortcode**: Reads from document metadata, supports dot notation, returns type-converted inlines
4. **Error handling**: Missing keys render as `?meta:keyname` in bold

Key source files:
- `quarto-pre/shortcodes-handlers.lua` - Built-in handlers
- `customnodes/shortcodes.lua` - AST processing

## Design Decisions

### 1. Single Transform vs Two-Phase Pattern

**Decision**: Use a single `ShortcodeResolveTransform` rather than two phases.

**Rationale**: Unlike callouts (which need semantic representation for cross-cutting concerns), shortcodes are:
- Already their own inline type (`Inline::Shortcode`)
- Self-contained - resolution doesn't need intermediate representation
- Not referenced by other transforms

### 2. Handler Registry vs Match-Based Dispatch

**Decision**: Start with match-based dispatch in the transform, refactor to registry if needed.

**Rationale**:
- Simpler initial implementation
- Built-in shortcodes are a fixed set
- User-defined shortcodes are a future concern (will require registry)

### 3. Pipeline Position

**Decision**: Place `ShortcodeResolveTransform` early in the pipeline, after `CalloutResolveTransform` and before `MetadataNormalizeTransform`.

**Rationale**:
- Shortcodes may appear in callout content (must resolve after callouts are processed)
- Metadata normalization should see resolved content, not shortcode placeholders
- Follows TS Quarto's `quarto_pre_filters` positioning

### 4. Error Handling

**Decision**: Dual approach - visible inline content AND diagnostic messages.

**Visible Output**: Follow TS Quarto's approach - render errors as visible inline content (e.g., `?meta:keyname`). This ensures authors see feedback directly in the rendered document.

**Diagnostic Integration**: Additionally emit `DiagnosticMessage` warnings via the `StageContext.add_warning()` mechanism. These diagnostics:
- Include source location (`SourceInfo`) from the shortcode
- Are rendered as ariadne-formatted messages in `quarto render` CLI
- Are converted to JSON for Monaco editor diagnostics in hub-client

**Rationale**:
- Authors need feedback when shortcodes fail
- Silent failures are harder to debug
- Visible output matches existing TS Quarto behavior
- Diagnostics enable IDE integration (squiggly underlines in Monaco)
- Diagnostics enable beautiful CLI error messages with source context

## Architecture

### Core Types

```rust
// In quarto-core/src/transforms/shortcode_resolve.rs

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

/// Error information for shortcode resolution failures
pub struct ShortcodeError {
    /// Error message for visible output (e.g., "meta:title")
    pub key: String,
    /// Full diagnostic message with source location
    pub diagnostic: DiagnosticMessage,
}

/// Result of resolving a shortcode
pub enum ShortcodeResult {
    /// Resolved to inline content
    Inlines(Vec<Inline>),
    /// Error - renders visible content AND emits diagnostic
    Error(ShortcodeError),
    /// Shortcode should be preserved (e.g., escaped shortcodes)
    Preserve,
}

/// Context passed to shortcode handlers
pub struct ShortcodeContext<'a> {
    /// Document metadata
    pub metadata: &'a ConfigValue,
    /// Project context
    pub project: &'a ProjectContext,
    /// Render format
    pub format: &'a Format,
    /// Source info for the shortcode (for error reporting)
    pub source_info: &'a SourceInfo,
}

/// Trait for shortcode handlers
pub trait ShortcodeHandler: Send + Sync {
    /// The shortcode name (e.g., "meta", "var", "env")
    fn name(&self) -> &str;

    /// Resolve the shortcode to content
    fn resolve(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
    ) -> ShortcodeResult;
}
```

### Transform Implementation

```rust
pub struct ShortcodeResolveTransform {
    handlers: Vec<Box<dyn ShortcodeHandler>>,
}

impl ShortcodeResolveTransform {
    pub fn new() -> Self {
        Self {
            handlers: vec![
                Box::new(MetaShortcodeHandler),
                // Future: VarShortcodeHandler, EnvShortcodeHandler, etc.
            ],
        }
    }

    fn resolve_shortcode(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
    ) -> ShortcodeResult {
        // Handle escaped shortcodes
        if shortcode.is_escaped {
            return ShortcodeResult::Preserve;
        }

        // Find and call handler
        for handler in &self.handlers {
            if handler.name() == shortcode.name {
                return handler.resolve(shortcode, ctx);
            }
        }

        // Unknown shortcode - create error with diagnostic
        let diagnostic = DiagnosticMessageBuilder::warning("Unknown shortcode")
            .problem(format!("Shortcode `{}` is not recognized", shortcode.name))
            .add_hint("Check the shortcode name for typos")
            .with_location(ctx.source_info.clone())
            .build();
        ShortcodeResult::Error(ShortcodeError {
            key: shortcode.name.clone(),
            diagnostic,
        })
    }
}
```

### `meta` Shortcode Handler

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;

pub struct MetaShortcodeHandler;

impl ShortcodeHandler for MetaShortcodeHandler {
    fn name(&self) -> &str {
        "meta"
    }

    fn resolve(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
    ) -> ShortcodeResult {
        // Get the key from positional args
        let key = match shortcode.positional_args.first() {
            Some(ShortcodeArg::String(s)) => s.clone(),
            _ => {
                let diagnostic = DiagnosticMessageBuilder::warning("Missing shortcode argument")
                    .problem("The `meta` shortcode requires a metadata key")
                    .add_hint("Use `{{< meta key >}}` where `key` is a metadata field name")
                    .with_location(ctx.source_info.clone())
                    .build();
                return ShortcodeResult::Error(ShortcodeError {
                    key: "meta".to_string(),
                    diagnostic,
                });
            }
        };

        // Look up value in metadata (supports dot notation)
        match get_nested_metadata(ctx.metadata, &key) {
            Some(value) => ShortcodeResult::Inlines(config_value_to_inlines(value)),
            None => {
                let diagnostic = DiagnosticMessageBuilder::warning("Unknown metadata key")
                    .problem(format!("Metadata key `{}` not found in document", key))
                    .add_hint("Check that the key exists in your YAML frontmatter")
                    .with_location(ctx.source_info.clone())
                    .build();
                ShortcodeResult::Error(ShortcodeError {
                    key: format!("meta:{}", key),
                    diagnostic,
                })
            }
        }
    }
}

/// Navigate nested metadata using dot notation
fn get_nested_metadata<'a>(meta: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = meta;

    for part in parts {
        match current {
            ConfigValue::Map(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }

    Some(current)
}

/// Convert a ConfigValue to inline content
fn config_value_to_inlines(value: &ConfigValue) -> Vec<Inline> {
    match value {
        ConfigValue::String(s) => vec![Inline::Str(Str {
            text: s.clone(),
            source_info: SourceInfo::default(),
        })],
        ConfigValue::Number(n) => vec![Inline::Str(Str {
            text: n.to_string(),
            source_info: SourceInfo::default(),
        })],
        ConfigValue::Bool(b) => vec![Inline::Str(Str {
            text: b.to_string(),
            source_info: SourceInfo::default(),
        })],
        ConfigValue::Inlines(inlines) => inlines.clone(),
        ConfigValue::Blocks(blocks) => {
            // For blocks in inline context, flatten to plain text
            // This matches TS Quarto behavior
            flatten_blocks_to_inlines(blocks)
        }
        _ => vec![Inline::Str(Str {
            text: "?invalid meta type".to_string(),
            source_info: SourceInfo::default(),
        })],
    }
}
```

### AST Traversal

The transform traverses the entire AST, replacing shortcodes with their resolved content and collecting diagnostics:

```rust
impl AstTransform for ShortcodeResolveTransform {
    fn name(&self) -> &str {
        "shortcode-resolve"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Collect diagnostics during traversal
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        // Resolve shortcodes in all blocks
        resolve_blocks(
            &mut ast.blocks,
            self,
            &ast.meta,
            ctx.project,
            ctx.format,
            &mut diagnostics,
        );

        // Add any diagnostics to the render context
        for diagnostic in diagnostics {
            ctx.add_warning(diagnostic);
        }

        Ok(())
    }
}

/// Resolve shortcodes in inlines, collecting diagnostics
fn resolve_inlines(
    inlines: &mut Vec<Inline>,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    project: &ProjectContext,
    format: &Format,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let mut i = 0;
    while i < inlines.len() {
        if let Inline::Shortcode(shortcode) = &inlines[i] {
            let shortcode_ctx = ShortcodeContext {
                metadata,
                project,
                format,
                source_info: &shortcode.source_info, // From parsed shortcode
            };

            match transform.resolve_shortcode(shortcode, &shortcode_ctx) {
                ShortcodeResult::Inlines(replacement) => {
                    // Replace shortcode with resolved inlines
                    inlines.splice(i..=i, replacement);
                    // Don't increment i - new inlines may contain shortcodes
                }
                ShortcodeResult::Error(error) => {
                    // Emit diagnostic
                    diagnostics.push(error.diagnostic);
                    // Replace with visible error (TS Quarto style)
                    let error_inline = make_error_inline(&error.key);
                    inlines[i] = error_inline;
                    i += 1;
                }
                ShortcodeResult::Preserve => {
                    // Convert escaped shortcode to literal text
                    let literal = shortcode_to_literal(shortcode);
                    inlines[i] = literal;
                    i += 1;
                }
            }
        } else {
            // Recurse into inline containers
            recurse_inline(&mut inlines[i], transform, metadata, project, format, diagnostics);
            i += 1;
        }
    }
}

/// Create visible error inline: Strong("?key")
fn make_error_inline(key: &str) -> Inline {
    Inline::Strong(Strong {
        content: vec![Inline::Str(Str {
            text: format!("?{}", key),
            source_info: SourceInfo::default(),
        })],
        source_info: SourceInfo::default(),
    })
}
```

Key traversal considerations:
- Shortcodes can appear in any inline context (paragraphs, headers, links, etc.)
- Shortcodes can be nested in block structures (lists, quotes, divs)
- Diagnostics are collected during traversal and added to context at the end
- Error inlines are rendered in bold with `?` prefix (matching TS Quarto)

## Work Items

### Phase 0: Prerequisites (Added retroactively)

- [x] Add `source_info` field to `Shortcode` struct (quarto-pandoc-types)
- [x] Update all Shortcode instantiations in tests to include `source_info`
- [x] Add `warnings` field to `RenderContext` for diagnostic collection
- [x] Add `add_warning()` method to `RenderContext`
- [x] Update `AstTransformsStage` to transfer warnings to `StageContext`

### Phase 1: Infrastructure

- [x] Create `crates/quarto-core/src/transforms/shortcode_resolve.rs`
- [x] Define `ShortcodeError` struct with diagnostic integration
- [x] Define `ShortcodeResult` enum
- [x] Define `ShortcodeContext` struct (with source_info)
- [x] Define `ShortcodeHandler` trait
- [x] Implement `ShortcodeResolveTransform` struct
- [x] Add transform to `transforms/mod.rs` exports

### Phase 2: `meta` Handler

- [x] Implement `MetaShortcodeHandler`
- [x] Implement `get_nested_metadata()` for dot notation lookup
- [x] Implement `config_value_to_inlines()` for value conversion
- [x] Create diagnostics with `DiagnosticMessageBuilder` for errors
- [x] Handle error case with `?meta:keyname` visible output
- [x] Add unit tests for metadata lookup

### Phase 3: AST Traversal

- [x] Implement inline traversal (handle all inline containers)
- [x] Implement block traversal (handle all block containers)
- [x] Collect diagnostics during traversal
- [x] Add diagnostics to RenderContext via `add_warning()`
- [x] Handle escaped shortcodes (preserve as literal text)
- [x] Handle unknown shortcodes (render error + diagnostic)
- [x] Implement `make_error_inline()` for visible error output
- [x] Add integration tests with sample documents

### Phase 4: Pipeline Integration

- [x] Position transform correctly in pipeline order
- [x] Add transform to pipeline in `crates/quarto-core/src/pipeline.rs`
- [x] Update `MetadataNormalizeTransform` to handle resolved content (no changes needed - it already processes resolved inlines)
- [x] Verify hub-client WASM build works with new transform
- [x] Add end-to-end tests (added 5 tests in pipeline.rs)

### Phase 5: Documentation and Cleanup

- [x] Add module documentation (docstrings added to shortcode_resolve.rs)
- [x] Document handler trait for future implementers
- [x] Update CLAUDE.md if needed (no updates required)

### Additional Work Completed

- [x] Modified pampa postprocessor to NOT convert shortcodes to spans (shortcodes now remain as `Inline::Shortcode` in AST)
- [x] Updated native and JSON writers to convert shortcodes to spans at render time
- [x] Rewrote pampa shortcode tests to test `Inline::Shortcode` directly
- [x] Fixed spacing issue in native writer shortcode output

---

## Fixed Bug: Spacing Around Resolved Shortcodes

**Status**: Fixed (2026-02-01)

### Description

When shortcodes were resolved, spaces adjacent to the shortcode were being lost.

### Root Cause

The tree-sitter external scanner in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` (lines 2074-2082) unconditionally skips whitespace before matching tokens. For block-level parsing this is correct (indentation matters), but for inline tokens like shortcodes, this consumed whitespace that should have been emitted as `pandoc_space` nodes.

The scanner would:
1. Start at position 1 (the space before `{{<`)
2. Skip whitespace to position 2 (the `{`)
3. Match `{{<` as SHORTCODE_OPEN
4. The resulting token range would be 1-5, including the leading space

### Solution

Modified `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs` to detect and handle leading whitespace in the shortcode node, following the same pattern used for `inline_note_reference`:

1. Check if the node text starts with whitespace
2. If so, calculate separate source ranges for the space and the shortcode
3. Return `IntermediateInlines` with a `Space` inline followed by the `Shortcode` inline
4. Adjust the shortcode's source_info to exclude the leading whitespace

This is the same pattern already used in this codebase for handling `inline_note_reference` nodes that include leading whitespace.

### Files Modified

- `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs` - Added leading whitespace detection and Space emission
- `crates/pampa/src/pandoc/treesitter.rs` - Updated call to pass `input_bytes`

### Test

```bash
echo 'Text {{< meta title >}} more {{< meta author >}} end' | cargo run --bin pampa -- --to json
# Now shows Space nodes before shortcodes

echo '---
format: html
title: "My Title"
author: "Jane"
---

Text {{< meta title >}} more {{< meta author >}} end' > test.qmd
cargo run --bin quarto -- render --to html test.qmd
# Output: <p>Text My Title more Jane end</p>
```

## Testing Strategy

### Unit Tests

1. **Metadata lookup**: Test dot notation, missing keys, various value types
2. **Value conversion**: Test string, number, bool, inlines, blocks
3. **Error handling**: Test missing args, unknown shortcodes, invalid types
4. **Diagnostic creation**: Verify diagnostics have correct source locations

### Integration Tests

1. **Simple document**: Single `{{< meta title >}}` shortcode
2. **Nested metadata**: `{{< meta author.name >}}`
3. **Multiple shortcodes**: Multiple shortcodes in one document
4. **Shortcodes in various contexts**: In headers, lists, links, callouts
5. **Escaped shortcodes**: `{{{< meta title >}}}`
6. **Missing keys**: Verify both error output (`?meta:key`) AND diagnostic emission

### Diagnostic Tests

1. **Warning collection**: Verify warnings are added to RenderContext
2. **Source location**: Verify diagnostics point to shortcode location
3. **Multiple errors**: Verify multiple shortcode errors create multiple diagnostics

### End-to-End Tests

1. **Full render**: Complete QMD → HTML with shortcodes
2. **hub-client**: Verify WASM build works correctly
3. **CLI output**: Verify diagnostics render as ariadne-formatted messages

## Future Considerations

### Additional Built-in Shortcodes

After `meta`, these shortcodes should be straightforward to add:
- `var` - Read from `_variables.yml`
- `env` - Read from environment variables
- `pagebreak` - Format-specific page break
- `include` - Include file content (complex, may need separate planning)

### User-Defined Shortcodes (Extensions)

Not in scope for this plan. In TS Quarto, extensions can define shortcodes via Lua filters.

**Proposed architecture for Rust Quarto:**
- Create a separate `ExtensionShortcodeTransform` (distinct from built-in)
- This transform would run user-defined shortcode handlers from extensions
- The current `ShortcodeHandler` trait design is compatible with this approach
- The two transforms run in sequence: built-in first, then extension

This separation keeps the built-in transform simple and well-tested, while allowing future extension support without modifying the core transform.

### Block Shortcodes

Some shortcodes produce block content (e.g., `pagebreak`). This is deferred:
- Current implementation focuses on inline shortcodes only
- Block shortcodes would need `ShortcodeResult::Blocks` variant
- AST traversal would need to handle replacing inline with blocks (parent manipulation)
- Should also validate that block shortcodes fail gracefully in inline-only contexts

## References

- TS Quarto source: `external-sources/quarto-cli/src/resources/filters/quarto-pre/shortcodes-handlers.lua`
- Lua filter analysis: `claude-notes/plans/2026-01-24-lua-filter-analysis.md`
- Callout transform pattern: `crates/quarto-core/src/transforms/callout_resolve.rs`
