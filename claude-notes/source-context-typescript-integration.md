# SourceContext JSON Serialization for TypeScript/WASM Integration

## Problem Statement

We need to enable quarto-cli (TypeScript/Deno) to use source location information from quarto-markdown-pandoc (Rust/WASM). Specifically:

1. **WASM module** reads .qmd files using Rust code
2. **Rust parser** creates `SourceContext` and `SourceInfo` with location tracking
3. **WASM output** is serialized to JSON
4. **TypeScript code** deserializes and needs to use this for YAML validation
5. **TypeScript validator** expects `MappedString` objects for error reporting

## Current Implementations

### Rust: quarto-source-map

**Data Structures:**
```rust
pub struct SourceInfo {
    pub range: Range,              // Current text range
    pub mapping: SourceMapping,    // How it maps to parent
}

pub enum SourceMapping {
    Original { file_id: FileId },
    Substring { parent: Box<SourceInfo>, offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Transformed { parent: Box<SourceInfo>, mapping: Vec<RangeMapping> },
}

pub struct SourceContext {
    files: Vec<SourceFile>,
}

pub struct SourceFile {
    pub path: String,
    pub file_info: Option<FileInformation>,
    pub metadata: FileMetadata,
}
```

**Key Characteristics:**
- Data-based representation (not functional)
- Already implements `Serialize`/`Deserialize`
- Stores mapping chain as nested data structures
- `FileInformation` has line break indices for efficient row/col lookup

### TypeScript: quarto-cli MappedString

**Type Definition:**
```typescript
interface MappedString {
    readonly value: string;
    readonly fileName?: string;
    readonly map: (index: number, closest?: boolean) => StringMapResult;
}

type StringMapResult = {
    index: number;
    originalString: MappedString;
} | undefined;
```

**Key Characteristics:**
- Function-based representation (uses closures)
- `map` function walks the chain at runtime
- Built through composition (`mappedSubstring`, `mappedConcat`)
- No explicit parent references - composition via closures
- Used extensively in YAML validation for error reporting

## Design Challenges

### Challenge 1: Data vs Function

- **Rust**: Stores mapping chain as serializable data
- **TypeScript**: Uses runtime functions (closures) to walk the chain
- **Issue**: Can't serialize JavaScript functions to JSON

### Challenge 2: Recursive Structures

- **Rust**: `SourceInfo` contains nested `Box<SourceInfo>` for Substring/Transformed
- **TypeScript**: Needs to reconstruct this as composed `map` functions
- **Issue**: JSON serialization of recursive structures needs careful handling

### Challenge 3: FileInformation

- **Rust**: `FileInformation` has efficient line break indices
- **TypeScript**: Already has `indexToLineCol` functions that work on strings
- **Issue**: Do we serialize `FileInformation` or reconstruct in TypeScript?

### Challenge 4: File Content

- **WASM**: May or may not have original file content available
- **TypeScript**: Needs content for line/col calculations
- **Issue**: Content could be large - serialize selectively?

## Use Case Analysis

### YAML Validation Flow

1. **Rust/WASM**: Parse .qmd → extract YAML frontmatter → parse YAML
2. **Rust/WASM**: Each YAML node has `YamlWithSourceInfo`
3. **Rust/WASM**: Serialize parsed YAML + SourceInfo to JSON
4. **TypeScript**: Deserialize JSON
5. **TypeScript**: Run YAML validation
6. **TypeScript**: Report errors with mapped locations back to .qmd file

### Required Operations

TypeScript needs to:
- Convert a YAML value's SourceInfo → MappedString
- Call `map(offset)` to resolve to original .qmd position
- Get fileName for error reporting
- Convert offset → line/col in original file

## Proposed Solutions

### Option A: Direct JSON Serialization + TypeScript Constructor

**Approach:**
1. Serialize `SourceInfo` to JSON as-is (already works)
2. Create TypeScript function: `sourceInfoToMappedString(sourceInfo: SerializedSourceInfo, context: SourceContext): MappedString`
3. This function recursively builds MappedString from the data structure

**Pros:**
- Minimal changes to Rust code
- Leverages existing Serialize implementation
- TypeScript has full control over MappedString construction

**Cons:**
- TypeScript needs to understand Rust's SourceMapping variants
- TypeScript code becomes coupled to Rust implementation

### Option B: Simplified JSON Format

**Approach:**
1. Create new `SourceInfoJson` type optimized for TypeScript consumption
2. Add method: `impl SourceInfo { pub fn to_json_representation(&self) -> SourceInfoJson }`
3. SourceInfoJson uses arrays of ranges instead of recursive structure
4. TypeScript constructs MappedString using `mappedString(source, pieces)`

**Pros:**
- Decouples TypeScript from Rust implementation details
- Easier for TypeScript to consume
- Could be more compact

**Cons:**
- Additional Rust code to maintain
- Need to define the conversion logic
- Potentially loses some structure information

### Option C: Flattened Mapping Table

**Approach:**
1. Pre-compute offset mappings for each SourceInfo
2. Serialize as flat lookup table: `{ [offset]: { fileId, originalOffset } }`
3. TypeScript constructs simple MappedString with table-based map function

**Pros:**
- Very simple for TypeScript
- Fast lookups (no recursive walking)
- Minimal TypeScript code

**Cons:**
- Could be large (one entry per offset?)
- Loses structural information
- May not work well for large documents

## Recommended Approach: Hybrid (Option A + Enhancements)

### Phase 1: Basic Integration
1. Keep existing JSON serialization of SourceInfo
2. Create TypeScript module: `source-map-bridge.ts`
3. Implement `sourceInfoToMappedString()` that:
   - Handles Original, Substring, Concat, Transformed variants
   - Recursively builds MappedString
   - Uses SourceContext to get file content when needed

### Phase 2: Optimization
1. Add optional "flattened" representation for frequently-accessed ranges
2. Cache MappedString construction results
3. Consider selective content serialization (only serialize file content if needed)

### Phase 3: Content Management
1. Decide: serialize file content in SourceFile, or keep separate?
2. For WASM use case: probably serialize content since it's already loaded
3. Add `SourceContext.without_content()` for cases where content shouldn't be serialized

## Implementation Tasks

### Task 1: TypeScript Bridge Module
Create `source-map-bridge.ts` in quarto-cli that:
- Defines TypeScript types matching Rust SourceInfo/SourceMapping
- Implements conversion functions
- Handles recursive structure reconstruction

### Task 2: Test SourceInfo JSON Serialization
Add tests in quarto-source-map that:
- Serialize various SourceInfo structures
- Verify JSON is correct
- Document expected format

### Task 3: Add Example Serialization
Create example showing:
- YAML parsing in Rust
- JSON serialization
- TypeScript deserialization
- MappedString construction
- Error reporting with mapped locations

### Task 4: Content Handling Strategy
Decide and implement:
- When to include file content in serialization
- How to handle large files
- Whether to use FileInformation or reconstruct in TS

### Task 5: Integration Test
End-to-end test:
- Parse .qmd in Rust WASM
- Serialize to JSON
- Deserialize in TypeScript
- Validate YAML
- Report error with correct .qmd location

## Open Questions

1. **File Content**: Should we always serialize file content in SourceFile, or have a flag?
2. **FileInformation**: Serialize line break indices, or let TypeScript compute them?
3. **Performance**: Is recursive MappedString construction acceptable, or do we need optimization?
4. **Caching**: Should TypeScript cache SourceInfo→MappedString conversions?
5. **API Design**: Should conversion be explicit, or transparent to existing code?

## Notes

- The TypeScript MappedString is well-designed for composition
- Rust SourceInfo maps well to MappedString conceptually
- Main challenge is data→function conversion
- This is not blocking other work but important for WASM integration
- Consider adding this to quarto-cli as a reusable library

## References

- Rust: `crates/quarto-source-map/src/`
- TypeScript: `external-sources/quarto-cli/src/core/lib/mapped-text.ts`
- TypeScript types: `external-sources/quarto-cli/src/core/lib/text-types.ts`
