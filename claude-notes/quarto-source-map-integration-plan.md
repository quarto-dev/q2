# quarto-source-map Integration Plan

## Context

This document outlines the plan to migrate `quarto-markdown-pandoc` from using its own `pandoc::location` types to using `quarto-source-map` types throughout the codebase.

## Goals

1. Replace `pandoc::location::{Location, Range, SourceInfo}` with `quarto-source-map` equivalents
2. Integrate `quarto-yaml` for YAML metadata parsing with proper source tracking
3. Handle substring parsing for YAML frontmatter (offsets relative to .qmd file)
4. Convert tree-sitter location information to quarto-source-map format
5. Enable proper source location tracking for error messages

## Current State Analysis

### 1. pandoc::location Module

**Current types:**
- `Location { offset, row, column }`
- `Range { start: Location, end: Location }`
- `SourceInfo { filename_index: Option<usize>, range: Range }`
- Helper functions: `node_location()`, `node_source_info()`, `node_source_info_with_context()`

**Usage:** 104 call sites across 36 files

**Characteristics:**
- Simple, flat structure
- Filename stored as index into `ASTContext.filenames` vector
- No transformation tracking
- Direct conversion from tree-sitter Node positions

### 2. YAML Metadata Parsing

**Current approach:**
```rust
// In meta.rs:
let content = extract_between_delimiters(&block.text).unwrap();  // Extract substring
let mut parser = Parser::new_from_str(content);  // yaml-rust2 sees offset 0
let mut handler = YamlEventHandler::new();
parser.load(&mut handler, false);
```

**Problems:**
- yaml-rust2 Markers are relative to the extracted substring, not the full .qmd file
- Location information is currently ignored (`_mark` parameter)
- No way to map YAML errors back to .qmd file positions

### 3. Tree-sitter Integration

**Current approach:**
```rust
pub fn node_source_info_with_context(node: &tree_sitter::Node, context: &ASTContext) -> SourceInfo {
    let filename_index = if context.filenames.is_empty() { None } else { Some(0) };
    SourceInfo::new(filename_index, node_location(node))
}
```

**Tree-sitter provides:**
- `node.start_byte()`, `node.end_byte()` - byte offsets in source
- `node.start_position()`, `node.end_position()` - row/column positions
- These are already in the "original file" coordinate space

## Design

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│ quarto-markdown-pandoc                                      │
│                                                             │
│  ┌──────────────────┐         ┌──────────────────┐        │
│  │   Tree-sitter    │         │   YAML Parser    │        │
│  │   (markdown)     │         │  (frontmatter)   │        │
│  └────────┬─────────┘         └────────┬─────────┘        │
│           │                            │                   │
│           │ byte offsets               │ substring         │
│           │ row/column                 │ with offset       │
│           ▼                            ▼                   │
│  ┌────────────────────────────────────────────────┐       │
│  │      quarto-source-map::SourceInfo             │       │
│  │                                                 │       │
│  │  Tree-sitter:  SourceMapping::Original         │       │
│  │  YAML content: SourceMapping::Substring        │       │
│  └────────────────────────────────────────────────┘       │
│                          │                                 │
│                          ▼                                 │
│            ┌──────────────────────────────┐               │
│            │   SourceContext              │               │
│            │   (manages FileIds)          │               │
│            └──────────────────────────────┘               │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

#### 1. Tree-sitter Locations → SourceMapping::Original

Tree-sitter provides positions directly in the source file, so we use:

```rust
pub fn node_to_source_info(
    node: &tree_sitter::Node,
    file_id: FileId
) -> quarto_source_map::SourceInfo {
    SourceInfo::original(
        file_id,
        Range {
            start: Location {
                offset: node.start_byte(),
                row: node.start_position().row,
                column: node.start_position().column,
            },
            end: Location {
                offset: node.end_byte(),
                row: node.end_position().row,
                column: node.end_position().column,
            },
        },
    )
}
```

**This is straightforward** - tree-sitter already gives us what we need.

#### 2. YAML Frontmatter → SourceMapping::Substring

YAML frontmatter is extracted as a substring from the .qmd file:

```qmd
---
title: My Document
author: John Doe
---

# Content
```

The YAML parser sees:
```yaml
title: My Document
author: John Doe
```

When yaml-rust2 reports `title` at offset 0, we need to map it to the actual offset in the .qmd file (which is after the `---\n`).

**Solution:** Use `SourceMapping::Substring`

```rust
// 1. Create SourceInfo for the full .qmd file
let qmd_file_id = ctx.add_file("document.qmd".into(), Some(qmd_content.into()));
let qmd_source_info = SourceInfo::original(file_id, /* full range */);

// 2. Extract YAML substring (between --- markers)
let yaml_start_offset = /* position after first --- */;
let yaml_end_offset = /* position before second --- */;
let yaml_content = &qmd_content[yaml_start_offset..yaml_end_offset];

// 3. Create Substring source info for YAML
let yaml_source_info = SourceInfo::substring(
    qmd_source_info,
    yaml_start_offset,
    yaml_end_offset,
);

// 4. Pass to YAML parser (needs enhancement - see below)
let yaml_tree = quarto_yaml::parse_with_source_info(yaml_content, yaml_source_info)?;
```

When quarto-yaml reports a location at offset 10 in the YAML content, `SourceMapping::Substring` will automatically add `yaml_start_offset` to map back to the .qmd file.

#### 3. Enhance quarto-yaml for Substring Parsing

**Current API:**
```rust
pub fn parse(content: &str) -> Result<YamlWithSourceInfo>
pub fn parse_file(content: &str, filename: &str) -> Result<YamlWithSourceInfo>
```

**New API needed:**
```rust
/// Parse YAML with a parent SourceInfo (for substring parsing)
pub fn parse_with_parent(
    content: &str,
    parent: quarto_source_map::SourceInfo
) -> Result<YamlWithSourceInfo>
```

**Implementation:**
- Replace `quarto_yaml::SourceInfo` with `quarto_source_map::SourceInfo`
- In `YamlBuilder`, when creating source info from yaml-rust2 Markers:
  - If parent is provided and is a Substring, create new Substring mappings
  - If parent is Original, create Substring mapping to parent

**Example:**
```rust
// In YamlBuilder::make_source_info
fn make_source_info(&self, marker: &Marker, len: usize) -> quarto_source_map::SourceInfo {
    let local_range = Range {
        start: Location {
            offset: marker.index(),
            row: marker.line(),
            column: marker.col(),
        },
        end: Location {
            offset: marker.index() + len,
            row: /* calculate */,
            column: /* calculate */,
        },
    };

    if let Some(parent) = &self.parent_source_info {
        // Create Substring mapping to parent
        SourceInfo::substring(
            parent.clone(),
            marker.index(),
            marker.index() + len,
        )
    } else {
        // No parent - create Original mapping
        SourceInfo::original(self.file_id, local_range)
    }
}
```

#### 4. Replace ASTContext.filenames with SourceContext

**Current:** `ASTContext { filenames: Vec<String> }`
- Simple vector with indices
- No content storage
- No FileId type

**New:** Use `quarto_source_map::SourceContext`
- Proper FileId type
- Content storage for offset-to-row/column conversion
- Thread-safe with Arc<File> internally

**Migration:**
- Pass `&mut SourceContext` instead of `&ASTContext` to parsing functions
- Or embed SourceContext inside ASTContext if other fields need to stay

**Option A: Replace ASTContext entirely**
```rust
// Remove ASTContext, use SourceContext directly everywhere
fn process_node(node: &Node, ctx: &mut SourceContext) -> Result<...>
```

**Option B: Embed SourceContext in ASTContext**
```rust
pub struct ASTContext {
    pub source_context: SourceContext,
    pub example_list_counter: Cell<usize>,
    // other non-source-related fields
}
```

**Recommendation: Option B** - less invasive, preserves separation of concerns

### 5. Migration of pandoc::location Types

**Delete:**
- `pandoc::location::{Location, Range, SourceInfo}`
- Helper functions `node_location()`, `node_source_info()`, etc.

**Replace with:**
```rust
// New helpers in a compatibility module
pub fn node_to_source_info(node: &tree_sitter::Node, file_id: FileId) -> quarto_source_map::SourceInfo {
    // As shown in Decision #1 above
}

pub fn node_to_source_info_with_context(
    node: &tree_sitter::Node,
    ctx: &ASTContext
) -> quarto_source_map::SourceInfo {
    let file_id = ctx.source_context.primary_file_id()
        .expect("Context must have a primary file");
    node_to_source_info(node, file_id)
}
```

**Update all call sites** (104 across 36 files):
- Change imports from `crate::pandoc::location::*` to `quarto_source_map::*`
- Update struct field types
- Update function signatures

## Implementation Plan

### Phase 1: Enhance quarto-yaml (5 tasks)

**Goal:** Make quarto-yaml work with quarto-source-map and support substring parsing

1. **Replace quarto-yaml::SourceInfo with quarto-source-map::SourceInfo**
   - Update all structs, function signatures
   - Remove old SourceInfo type
   - Update all tests

2. **Add parent source tracking to YamlBuilder**
   - Add `parent_source_info: Option<SourceInfo>` field
   - Modify `make_source_info()` to create Substring mappings when parent exists

3. **Add new parse API: parse_with_parent**
   - Takes `parent: SourceInfo` parameter
   - Passes parent to YamlBuilder
   - Creates Substring mappings for all YAML nodes

4. **Update parse() and parse_file() to use new system**
   - Create Original SourceInfo and call parse_with_parent
   - Maintain backward compatibility

5. **Add tests for substring parsing**
   - Test that offsets map correctly through Substring
   - Test with SourceContext to verify mapping back to original

### Phase 2: Prepare quarto-markdown-pandoc (4 tasks)

**Goal:** Set up infrastructure without breaking existing code

6. **Add SourceContext to ASTContext**
   ```rust
   pub struct ASTContext {
       pub source_context: SourceContext,
       pub example_list_counter: Cell<usize>,
   }
   ```
   - Update constructors: `new()`, `with_filename()`, `anonymous()`
   - Add helper: `primary_file_id() -> Option<FileId>`

7. **Create compatibility module with conversion helpers**
   - File: `src/pandoc/source_map_compat.rs`
   - `node_to_source_info(node, file_id) -> quarto_source_map::SourceInfo`
   - `node_to_source_info_with_context(node, ctx) -> quarto_source_map::SourceInfo`
   - Temporary bridge from old to new

8. **Add new source_info_qsm fields alongside existing source_info**
   - Pick 2-3 representative structs (e.g., `HorizontalRule`, `Str`)
   - Add `source_info_qsm: Option<quarto_source_map::SourceInfo>`
   - Populate both old and new fields during transition
   - Verify tests still pass

9. **Update one complete parsing module as proof of concept**
   - Choose simple module (e.g., `thematic_break.rs`)
   - Use new `node_to_source_info_with_context`
   - Populate `source_info_qsm` field
   - Verify tests pass, location info preserved

### Phase 3: Migrate All Parsing (2 tasks)

**Goal:** Replace all pandoc::location usage with quarto-source-map

10. **Systematic replacement across all 36 files**
    - Script to update imports
    - Update all `node_source_info` → `node_to_source_info_with_context` calls
    - Update struct field types
    - May need multiple sub-tasks/PRs to keep changes reviewable

11. **Remove old pandoc::location module**
    - Delete `pandoc::location::{Location, Range, SourceInfo}`
    - Rename all `source_info_qsm` → `source_info`
    - Remove compatibility module
    - Update tests

### Phase 4: Integrate YAML with SourceMapping (3 tasks)

**Goal:** Use quarto-yaml with Substring mappings for frontmatter

12. **Update YAML metadata extraction to create Substring SourceInfo**
    - In `meta.rs::rawblock_to_meta()`
    - Calculate YAML offset within RawBlock
    - Create parent SourceInfo for the RawBlock
    - Create Substring SourceInfo for YAML content
    - Call `quarto_yaml::parse_with_parent()`

13. **Propagate SourceInfo through MetaValue**
    - Add source_info fields to MetaValue variants
    - Track locations for keys and values
    - Enable source tracking for metadata errors

14. **Update error reporting for metadata parsing**
    - Use DiagnosticCollector with location
    - Report YAML parse errors with correct file positions
    - Add tests verifying error locations

### Phase 5: Testing and Validation (3 tasks)

**Goal:** Ensure correctness and no regressions

15. **Add integration tests for source location tracking**
    - Test tree-sitter locations map correctly
    - Test YAML frontmatter locations map through Substring
    - Test nested YAML structures
    - Test error reporting with locations

16. **Add tests for offset edge cases**
    - Empty YAML frontmatter
    - YAML at start/end of file
    - Multiple transformation layers
    - Unicode handling (byte vs character offsets)

17. **Audit all DiagnosticCollector usage**
    - Ensure all `error()` and `warn()` calls that could benefit from location use `error_at()`/`warn_at()`
    - Verify locations are accurate in test output

### Phase 6: Cleanup and Documentation (2 tasks)

18. **Update documentation**
    - Document the SourceContext/ASTContext relationship
    - Document tree-sitter → quarto-source-map conversion
    - Document YAML substring handling
    - Update CLAUDE.md with source map patterns

19. **Remove temporary bridges**
    - Remove conversion helpers from pandoc::location (already deleted)
    - Check for any remaining `to_source_map_info()` calls
    - Consolidate into single pattern

## Risk Analysis

### High Risk

1. **Breaking change scale**: 104 call sites across 36 files
   - **Mitigation**: Phased approach, maintain both fields temporarily
   - **Mitigation**: Comprehensive test suite to catch regressions

2. **YAML offset calculation errors**: Off-by-one errors are easy
   - **Mitigation**: Extensive tests with known positions
   - **Mitigation**: Visual verification of error output

### Medium Risk

3. **Performance impact**: SourceMapping has more overhead than flat structure
   - **Mitigation**: SourceInfo is cheap to clone (uses Box<SourceInfo> for parents)
   - **Mitigation**: Benchmark if needed, optimize hot paths

4. **Tree-sitter row/column vs offset confusion**: Different coordinate systems
   - **Mitigation**: Use tree-sitter's positions directly, don't recalculate
   - **Mitigation**: Tests to verify row/column accuracy

### Low Risk

5. **quarto-yaml API changes**: This is internal code, not stable
   - API changes are acceptable for good reasons (like this migration)
   - No backward compatibility required

## Decisions Made

1. **✅ DECIDED: Efficient offset → row/column conversion**
   - Create `FileInformation` struct to encapsulate file analysis concerns
   - Store array of line break offsets (Vec<usize>)
   - Binary search for log-time, cache-friendly lookups
   - Don't need to store full content string for this purpose
   - **Design**:
     ```rust
     pub struct FileInformation {
         line_breaks: Vec<usize>,  // Byte offsets of each '\n'
         total_length: usize,
     }

     impl FileInformation {
         pub fn new(content: &str) -> Self { /* build line_breaks */ }
         pub fn offset_to_location(&self, offset: usize) -> Option<Location> { /* binary search */ }
     }

     pub struct SourceFile {
         pub path: String,
         pub file_info: Option<FileInformation>,  // Replaces content: Option<String>
         pub metadata: FileMetadata,
     }
     ```
   - **Benefits**:
     - Memory efficient (O(number of lines) instead of O(file size))
     - Fast lookups (O(log n) instead of O(n))
     - Clean encapsulation for future extensions (Unicode handling, syntax context, etc.)
   - **Action**: Implement as standalone task before main migration (see beads tasks)

2. **✅ DECIDED: Anonymous sources (e.g., `<metadata>`)**
   - Use FileId with sentinel path convention
   - Example: `FileId` with path `"<metadata>"`, `"<inline>"`, etc.
   - Consistent with existing `<metadata>` naming in codebase

3. **✅ DECIDED: ASTContext structure**
   - Use Option B: Embed SourceContext inside ASTContext
   - Less invasive than full replacement
   - Clear separation of concerns

4. **NEEDS CLARIFICATION: When to populate SourceContext with content?**
   - Question: When do we call `ctx.add_file(path, Some(content))` vs `ctx.add_file(path, None)`?
   - The `content: Option<String>` parameter in add_file is optional
   - Without content, we can't map offsets to row/column (but we can still track FileIds)
   - Options:
     a. Always pass content when available (at file read time)
     b. Pass None initially, populate lazily on first offset lookup
     c. Always pass content for real files, None for synthetic sources
   - **Waiting for clarification from user**

## Success Criteria

- ✅ All 152 tests pass
- ✅ No regressions in error message quality
- ✅ YAML frontmatter errors show correct file positions
- ✅ Tree-sitter locations preserved accurately
- ✅ All old pandoc::location types removed
- ✅ Documentation updated
- ✅ Can map any error back to original .qmd file position
- ✅ SourceMapping works correctly for Substring case

## Timeline Estimate

- Phase 1 (quarto-yaml): 2-3 days
- Phase 2 (infrastructure): 1-2 days
- Phase 3 (migration): 3-4 days
- Phase 4 (YAML integration): 2 days
- Phase 5 (testing): 2 days
- Phase 6 (cleanup): 1 day

**Total: 11-16 days** (approximately 2-3 weeks)

This is a conservative estimate for careful, test-driven development with thorough validation at each phase.
