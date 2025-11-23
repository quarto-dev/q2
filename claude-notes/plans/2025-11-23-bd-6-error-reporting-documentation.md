# bd-6: Documentation and Examples for quarto-error-reporting

**Date**: 2025-11-23
**Issue**: bd-6
**Priority**: 3 (Task)
**Status**: In Progress

## Context

The `quarto-error-reporting` crate is **internal infrastructure** for the Quarto Rust port. Its purpose is to provide consistent, high-quality error messages across all Quarto subsystems following tidyverse best practices.

**Current State**:
- ✅ All core features implemented (bd-1, bd-2, bd-3, bd-4 complete)
- ✅ 35 tests passing
- ✅ 70 error codes in catalog
- ✅ Heavily used across codebase (45+ usages in quarto-markdown-pandoc)
- ❌ Examples directory is empty
- ❌ 1 documentation warning
- ❌ Missing integration tests
- ❌ No contributor guide for error code system

## Goal

**Make quarto-error-reporting maintainable and usable by Quarto contributors** working on the Rust port. The documentation should help authors of Quarto functionality understand:
1. When and how to use this crate
2. How to add new error codes
3. How to write tidyverse-compliant error messages
4. Integration patterns for different Quarto subsystems

## Tasks

### Phase 1: Study and Planning ✓

1. ✓ Review existing source code
2. ✓ Review previous notes on error reporting design
3. ✓ Identify integration patterns in the codebase
4. ✓ Understand real-world usage

### Phase 2: Core Documentation

#### 2.1 Fix Documentation Warnings
- [ ] Fix unresolved link to `to_text_with_options`
- [ ] Run `cargo doc` and ensure clean build
- [ ] Verify all doc examples compile with `cargo test --doc`

#### 2.2 README Enhancement
- [ ] Add "Architecture Overview" section
  - Where this fits in Quarto
  - Relationship to other crates (quarto-source-map, ariadne)
  - The three output formats (text, JSON, ariadne)
- [ ] Add "Quick Start for Contributors" section
  - Simple example of creating an error
  - Link to relevant examples/
- [ ] Add "Error Code System" section
  - Subsystem numbering (0-9)
  - How to pick the next error number
  - Link to error_catalog.json
- [ ] Add "Tidyverse Guidelines" section
  - Four-part structure
  - Writing problem statements ("must" or "can't")
  - Writing hints (ends with "?")
  - Max 5 details
- [ ] Add "Integration Patterns" section
  - Parse errors (with SourceInfo)
  - Validation errors (structured details)
  - Writer errors (accumulated in context)
- [ ] Add "Adding New Error Codes" guide
  - Edit error_catalog.json
  - Use in code with `.with_code()`
  - Test it

#### 2.3 Module Documentation
- [ ] Enhance `src/lib.rs` module-level docs
  - Architecture diagram
  - When to use each type (DiagnosticMessage vs Builder)
  - Common patterns
- [ ] Enhance `src/diagnostic.rs` docs
  - Explain the four-part structure
  - Document the rendering pipeline
- [ ] Enhance `src/builder.rs` docs
  - Explain how builder encodes tidyverse guidelines
  - Show progression from simple to complex
- [ ] Enhance `src/catalog.rs` docs
  - Document subsystem numbering
  - Explain catalog structure

### Phase 3: Examples

Create runnable examples in `examples/` directory:

#### 3.1 Basic Patterns
- [ ] `basic_error.rs` - Simplest possible error
- [ ] `builder_api.rs` - Using DiagnosticMessageBuilder
- [ ] `with_error_code.rs` - Using error catalog

#### 3.2 Quarto Integration Patterns
- [ ] `parse_error_pattern.rs` - How qmd.rs reports parse errors
  - Use SourceInfo for location
  - Integration with ariadne
  - Example from actual parse error
- [ ] `yaml_validation_pattern.rs` - YAML validation errors
  - Structured details
  - Multiple related errors
  - Example from schema validation
- [ ] `writer_error_pattern.rs` - How writers handle errors
  - Accumulating errors in context
  - Continuing vs. failing
  - Example from ANSI writer
- [ ] `diagnostic_collector.rs` - Using DiagnosticCollector
  - Common pattern across codebase
  - When to use error() vs. error_at()
  - Checking has_errors()

#### 3.3 Advanced Usage
- [ ] `custom_rendering.rs` - TextRenderOptions
  - Disabling hyperlinks for tests
  - Custom formatting
- [ ] `migration_helpers.rs` - Using generic_error! macro
  - File/line tracking
  - When to use vs. builder

### Phase 4: Testing

#### 4.1 Integration Tests
- [ ] Test the macros (generic_error!, generic_warning!)
- [ ] Test full ariadne rendering with real SourceContext
- [ ] Test TextRenderOptions with hyperlinks disabled
- [ ] Test DiagnosticCollector workflow

#### 4.2 Catalog Validation Tests
- [ ] All error codes follow Q-{subsystem}-{number} format
- [ ] Subsystem numbers are in valid range (0-9)
- [ ] All catalog entries have required fields
- [ ] All docs URLs are well-formed
- [ ] No duplicate error codes

#### 4.3 Tidyverse Compliance Tests
- [ ] Problem statements use "must" or "can't" (advisory)
- [ ] Hints end with "?" (advisory)
- [ ] Details don't exceed 5 (advisory via build_with_validation)

### Phase 5: Contributing Guide

- [ ] Create `CONTRIBUTING.md` for error-reporting
  - How to add a new error code
  - Error code numbering conventions
  - Tidyverse style guide
  - Testing requirements

## Success Criteria

A Quarto contributor should be able to:
- [ ] Understand where error-reporting fits in architecture (30 seconds)
- [ ] Find the example matching their use case (2 minutes)
- [ ] Add a new error code correctly (10 minutes)
- [ ] Write tidyverse-compliant error messages without external docs

## Research Notes

### Subsystem Numbers (from error_catalog.json)
- 0: Internal/System Errors
- 1: YAML and Configuration
- 2: Markdown and Parsing
- 3: Engines and Execution
- 4: Rendering and Formats
- 5: Projects and Structure
- 6: Extensions and Plugins
- 7: CLI and Tools
- 8: Publishing and Deployment
- 9+: Reserved for future use

### Current Usage Patterns (from codebase analysis)

1. **Parse Error Pattern** (qmd_error_messages.rs):
   - Converts tree-sitter parse states to DiagnosticMessage
   - Uses SourceInfo with proper file/offset tracking
   - Renders with ariadne for source context

2. **Collector Pattern** (diagnostic_collector.rs):
   - Accumulates multiple errors/warnings
   - Provides helper methods: error(), warn(), error_at(), warn_at()
   - Checks has_errors() before proceeding
   - Renders to text or JSON

3. **Writer Error Pattern** (ansi.rs):
   - AnsiWriterContext accumulates errors during traversal
   - Returns Result with Vec<DiagnosticMessage> on failure
   - Allows continuing on non-fatal errors

4. **Generic Migration Pattern** (used widely):
   - Uses generic_error! and generic_warning! macros
   - Provides file/line tracking for migration phase
   - All use Q-0-99 error code

### Key Design Principles

From claude-notes/error-reporting-design-research.md:
1. Structured, machine-readable errors (JSON)
2. Human-friendly terminal output (ANSI + ariadne)
3. Tidyverse four-part structure
4. TypeScript-style error codes (Q-{subsystem}-{number})
5. Semantic markup with Pandoc spans

## Dependencies

- All prerequisite phases complete (bd-1, bd-2, bd-3, bd-4)
- No blockers

## Estimated Effort

- Phase 1: Complete
- Phase 2: 3-4 hours (documentation)
- Phase 3: 4-5 hours (examples)
- Phase 4: 2-3 hours (tests)
- Phase 5: 1 hour (contributing guide)

**Total: 10-13 hours**
