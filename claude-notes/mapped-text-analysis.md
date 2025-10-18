# Mapped Text Analysis

## Overview

Mapped-text is a fundamental data structure in Quarto that maintains source location tracking through text transformations. It's critical for error reporting, as it allows Quarto to report errors in terms of the original source file locations even after the text has been extracted, transformed, or processed.

**Total Code Size**: ~8,600 LOC across mapped-text, YAML intelligence, YAML validation, and YAML schema modules.

## Core Concept

### The Problem

Quarto frequently needs to:
1. Extract parts of source files (e.g., YAML frontmatter from a .qmd file)
2. Send those parts to parsers/validators (js-yaml, tree-sitter, pandoc)
3. Get error messages from those tools (with offsets into the extracted text)
4. Report errors with line/column numbers in the **original source file**

Example:
```
document.qmd (lines 1-100):
  Lines 1-10: YAML frontmatter
  Lines 11-100: Markdown content

Extract lines 1-10 → YAML string
Parse YAML → error at offset 45 in YAML string
Need to report: "Error in document.qmd at line 4, column 12"
```

### The Solution: MappedString

`MappedString` is a string plus a mapping function that tracks how offsets in the transformed string relate back to the original source.

## Type Definitions

```typescript
// Core types (from text-types.ts)

interface Range {
  start: number;
  end: number;
}

type StringMapResult = {
  index: number;
  originalString: MappedString;
} | undefined;

interface MappedString {
  readonly value: string;                    // The actual string content
  readonly fileName?: string;                // Source filename (for errors)
  readonly map: (index: number, closest?: boolean) => StringMapResult;
}

type EitherString = string | MappedString;
type StringChunk = string | MappedString | Range;
```

### Key Properties

1. **Composition**: MappedStrings compose - you can create a MappedString from pieces of other MappedStrings, and the mapping chains back to the original.

2. **The `map` function**:
   - Takes an index in the current string
   - Returns the index in the original string + a reference to that original
   - If `closest=true`, clamps to valid range rather than returning undefined
   - Walks back through the composition chain to the base string

## Core Operations

### 1. **asMappedString** - Create from plain string
```typescript
asMappedString(str: string, fileName?: string): MappedString
```
Creates a base MappedString where `map(i)` just returns `i` (identity mapping).

### 2. **mappedString** - Create from chunks
```typescript
mappedString(
  source: EitherString,
  pieces: StringChunk[],  // Ranges or new strings
  fileName?: string
): MappedString
```
Build a new MappedString by concatenating pieces of a source string.

**Example**:
```typescript
const source = asMappedString("---\ntitle: foo\n---\n");
// Extract just the YAML content (skip "---\n" at start)
const yaml = mappedString(source, [{ start: 4, end: 15 }]);
// yaml.value === "title: foo\n"
// yaml.map(0) points to index 4 in source
```

### 3. **mappedSubstring** - Extract a range
```typescript
mappedSubstring(source: EitherString, start: number, end?: number): MappedString
```
Like `String.substring()` but preserves mapping.

### 4. **mappedConcat** - Concatenate
```typescript
mappedConcat(strings: EitherString[]): MappedString
```
Joins multiple MappedStrings while maintaining all mappings.

### 5. **String-like operations**

All preserve mapping:
- `mappedTrim` / `mappedTrimStart` / `mappedTrimEnd`
- `mappedLines` - split into lines
- `mappedReplace` - replace text
- `mappedNormalizeNewlines` - convert `\r\n` to `\n`
- `skipRegexp` / `skipRegexpAll` - remove matches
- `breakOnDelimiter` - split on delimiter

### 6. **mappedIndexToLineCol** - Convert offset to line/column
```typescript
mappedIndexToLineCol(text: EitherString): (offset: number) => { line, column }
```
Takes an offset in the mapped string, maps it back to the original, then returns line/column in the original source.

### 7. **mappedDiff** - Recover mapping from external tools
```typescript
mappedDiff(source: MappedString, target: string): MappedString
```
Special function that uses a diff algorithm to recover MappedString information when text has been processed by external tools (like knitr) that don't preserve mapping.

## Usage in YAML System

### YAML Intelligence (`yaml-intelligence/`)

**Purpose**: Provides completions, hover, and validation for YAML in Quarto documents.

**MappedString Usage**:

1. **Context preparation**:
   ```typescript
   let code = asMappedString(context.code);
   // Trim "---" delimiters from frontmatter
   code = mappedString(code, [{ start: 3, end: code.value.length }]);
   ```

2. **Cell option extraction**:
   - Quarto code cells can have options in various formats: `#| key: value`, `%%| key: value`, etc.
   - Extracts these lines, normalizes comment syntax, builds YAML
   - All while tracking position in original document

3. **Tree-sitter integration**:
   ```typescript
   const treeSitterAnnotation = buildTreeSitterAnnotation(tree, mappedSource);
   ```
   Parses YAML with tree-sitter, annotates AST nodes with source positions via MappedString

### YAML Validation (`yaml-validation/`)

**Purpose**: Validates YAML against Quarto schemas, reports detailed errors.

**MappedString Usage**:

1. **AnnotatedParse**: The core data structure
   ```typescript
   interface AnnotatedParse {
     start: number;           // Offset in source
     end: number;
     result: JSONValue;       // Parsed value
     kind: string;
     source: MappedString;    // Original source with mapping!
     components: AnnotatedParse[];
   }
   ```

2. **Error creation**:
   ```typescript
   const locF = mappedIndexToLineCol(source);
   location = {
     start: locF(violatingObject.start),
     end: locF(violatingObject.end),
   };
   const mapResult = source.map(violatingObject.start);
   const fileName = mapResult?.originalString.fileName;
   ```
   Uses MappedString to convert error offsets to line/column and get filename.

3. **Source context** for error messages:
   ```typescript
   createSourceContext(violatingObject.source, {
     start: violatingObject.start,
     end: violatingObject.end
   })
   ```
   Extracts relevant lines from original source for pretty error display.

### YAML Schema (`yaml-schema/`)

**Purpose**: Defines Quarto's YAML schemas (frontmatter, project config, brand, etc.)

**MappedString Usage**: Indirect - schemas define validation rules, validator uses MappedString for error reporting.

## Key Insights for Rust Port

### 1. **Core Data Structure**

The Rust equivalent would be:

```rust
pub struct MappedString {
    value: String,
    file_name: Option<String>,
    // Could use Rc<dyn Fn> or an enum of mapping strategies
    map_fn: Box<dyn Fn(usize, bool) -> Option<StringMapResult>>,
}

pub struct StringMapResult {
    index: usize,
    original_string: Rc<MappedString>,  // Shared reference to original
}
```

**Challenge**: TypeScript closures are easy; Rust function composition is harder.

### 2. **Alternatives to Closures**

Instead of storing a closure, could use a more explicit representation:

```rust
pub enum MappingStrategy {
    Identity,                    // Base case
    Substring {                  // Single range
        parent: Rc<MappedString>,
        offset: usize,
    },
    Concat {                     // Multiple pieces
        pieces: Vec<MappedPiece>,
        offsets: Vec<usize>,
    },
}

pub struct MappedPiece {
    source: Rc<MappedString>,
    start: usize,
    end: usize,
}
```

This makes the mapping explicit and avoids complex closures.

### 3. **Critical Operations**

Must support:
- Creating from string (identity map)
- Extracting substrings (offset adjustm)
- Concatenating pieces
- Mapping offsets back to original
- Getting line/column from offset (via mapping)

### 4. **AnnotatedParse Integration**

YAML parser produces AnnotatedParse trees where every node has:
- Source text span (start/end offsets)
- Reference to MappedString (for mapping back)
- Parsed value
- Child nodes

```rust
pub struct AnnotatedParse {
    start: usize,
    end: usize,
    result: JsonValue,
    kind: String,
    source: Rc<MappedString>,
    components: Vec<AnnotatedParse>,
}
```

### 5. **YAML Parser Choice**

Current implementation uses:
- **tree-sitter-yaml** (lenient, error recovery, but sometimes incorrect)
- **js-yaml** (strict, compliant, but fails on errors)

Rust options:
- **serde_yaml**: Standard but strict
- **yaml-rust2**: Fork with better maintenance
- **tree-sitter-yaml**: Same as TS (has Rust bindings)

**Recommendation**: Use serde_yaml for strict parsing, potentially tree-sitter-yaml for error recovery mode.

### 6. **Testing**

Unit tests exist at `tests/unit/mapped-strings/mapped-text.test.ts`:
- Tests composition (substring of substring)
- Tests injection (mixing original ranges with new strings)
- Tests mapping correctness

These should be ported to Rust as property tests.

## Dependencies

**MappedString depends on**:
- `binary-search.ts` (glb function - greatest lower bound search)
- `text.ts` (line/column utilities, regex helpers)
- `ranged-text.ts` (range utilities)

**MappedString is used by**:
- YAML intelligence (completions, hover)
- YAML validation (error reporting)
- YAML parsing (AnnotatedParse)
- Error formatting (all Quarto errors use this)
- Code cell processing (knitr, jupyter)
- Diff-based recovery (mappedDiff for knitr output)

## Estimation

### Complexity: **Medium-High**

**Why**:
- Core concept is clean
- TypeScript uses closures heavily (harder in Rust)
- Needs careful lifetime management (Rc/Arc)
- Critical for correctness (errors must point to right place)

### LOC Estimate: ~1,000 lines Rust

**Breakdown**:
- Core MappedString: ~300 lines
- Operations (substring, concat, trim, etc.): ~400 lines
- Index/line/column utilities: ~200 lines
- Tests: ~100 lines (port existing tests)

### Time Estimate: 1-2 weeks

**Phases**:
1. Design Rust API (consider enum vs closure approach)
2. Implement core MappedString
3. Implement operations
4. Port unit tests
5. Integration with YAML parser

## Open Questions

1. **Closure vs Enum**: Should mapping be a closure or explicit enum?
   - **Closure**: More flexible, closer to TypeScript
   - **Enum**: More Rust-idiomatic, easier to debug, potentially faster

2. **Reference counting**: Rc or Arc?
   - **Rc**: If single-threaded
   - **Arc**: If LSP needs thread-safety

3. **Error handling**: How to handle invalid mappings?
   - Current TS code can return `undefined`
   - Rust could use `Option<StringMapResult>`

4. **Performance**: Is mapping overhead acceptable?
   - Every error needs to walk mapping chain
   - Could cache frequently-used mappings

5. **Integration**: How does this fit with quarto-markdown tree-sitter parser?
   - quarto-markdown produces tree-sitter nodes
   - Need to convert to AnnotatedParse with MappedString
