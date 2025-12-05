# ValidationDiagnostic JSON Output Schema

<!-- quarto-error-code-audit-ignore-file -->

This document describes the JSON output format produced by `ValidationDiagnostic::to_json()` for machine-readable consumption of YAML validation errors.

## Overview

The JSON output provides structured validation error information with:
- **Machine-readable error types** with structured data
- **Source locations** with filenames and line/column positions
- **Path information** showing where in the YAML document the error occurred
- **Human-readable messages** for convenience

## Top-Level Structure

```json
{
  "error_kind": { ... },      // Structured error type with data
  "code": "Q-1-XX",           // Error code
  "message": "...",           // Human-readable message
  "hints": [ ... ],           // Array of hint strings (optional)
  "instance_path": [ ... ],   // Path in YAML document
  "schema_path": [ ... ],     // Path in schema
  "source_range": { ... }     // Source location (optional)
}
```

## Field Descriptions

### `error_kind` (Object)

Structured error information with discriminated union format:

```json
{
  "type": "ErrorTypeName",
  "data": { ... }
}
```

Common error types:

#### TypeMismatch

```json
{
  "type": "TypeMismatch",
  "data": {
    "expected": "number",
    "got": "string"
  }
}
```

#### MissingRequiredProperty

```json
{
  "type": "MissingRequiredProperty",
  "data": {
    "property": "author"
  }
}
```

#### InvalidEnumValue

```json
{
  "type": "InvalidEnumValue",
  "data": {
    "value": "foo",
    "allowed": ["html", "pdf", "docx"]
  }
}
```

#### NumberOutOfRange

```json
{
  "type": "NumberOutOfRange",
  "data": {
    "value": 150,
    "minimum": 0,
    "maximum": 100,
    "exclusive_minimum": null,
    "exclusive_maximum": null
  }
}
```

#### UnknownProperty

```json
{
  "type": "UnknownProperty",
  "data": {
    "property": "unkown_field"
  }
}
```

#### StringPatternMismatch

```json
{
  "type": "StringPatternMismatch",
  "data": {
    "value": "invalid-email",
    "pattern": "^[^@]+@[^@]+\\.[^@]+$"
  }
}
```

#### Other Error Types

- `SchemaFalse` - Schema explicitly rejects all values
- `StringTooShort` - String length below `minLength`
- `StringTooLong` - String length above `maxLength`
- `NumberNotMultipleOf` - Number not a multiple of specified value
- `ArrayTooShort` - Array length below `minItems`
- `ArrayTooLong` - Array length above `maxItems`
- `ArrayItemsNotUnique` - Array contains duplicate items
- `InvalidPropertyName` - Property name doesn't match pattern
- `AdditionalPropertiesNotAllowed` - Extra properties when `additionalProperties: false`
- `AnyOfNoMatch` - None of the `anyOf` schemas matched
- `AllOfNotAllMatch` - Not all `allOf` schemas matched
- `Other` - Catch-all for other validation failures

### `code` (String)

Error code in format `Q-1-XX`:
- `Q-1-10`: Missing required property
- `Q-1-11`: Type mismatch
- `Q-1-12`: Invalid enum value
- `Q-1-13`: String too short
- `Q-1-14`: String too long
- `Q-1-15`: String pattern mismatch
- `Q-1-16`: Number out of range
- `Q-1-17`: Number not multiple of
- `Q-1-18`: Unknown property
- `Q-1-19`: Array too short
- `Q-1-20`: Array too long
- `Q-1-21`: Array items not unique
- `Q-1-22`: Invalid property name
- `Q-1-23`: Additional properties not allowed
- `Q-1-24`: AnyOf no match
- `Q-1-25`: AllOf not all match
- `Q-1-30`: Schema false
- `Q-1-99`: Other validation error

### `message` (String)

Human-readable error message. Provided for convenience but consumers should use `error_kind` for programmatic error handling.

Example: `"Expected number, got string"`

### `hints` (Array of Strings, Optional)

Array of actionable hints for fixing the error. May be empty.

Example:
```json
[
  "Use a numeric value without quotes?",
  "Check the allowed value range in the schema?"
]
```

### `instance_path` (Array of PathSegment)

Path through the YAML document to the location of the error. Each segment is either a key or an index:

```json
[
  {"type": "Key", "value": "format"},
  {"type": "Key", "value": "html"},
  {"type": "Index", "value": 0}
]
```

- **Key**: Object property name
- **Index**: Array index (0-based)

Empty array `[]` means error is at document root.

### `schema_path` (Array of Strings)

Path through the schema to the constraint that failed. Useful for debugging schema issues.

Example:
```json
["object", "properties", "age", "number"]
```

### `source_range` (Object, Optional)

Source location information with filename and positions. May be absent if source tracking is unavailable.

```json
{
  "filename": "config.yaml",
  "start_offset": 18,
  "end_offset": 33,
  "start_line": 3,
  "start_column": 7,
  "end_line": 3,
  "end_column": 21
}
```

**Fields:**
- `filename` (String): Path to the source file
- `start_offset` (Number): Start byte offset in file (0-based)
- `end_offset` (Number): End byte offset in file (0-based)
- `start_line` (Number): Start line number (1-based)
- `start_column` (Number): Start column number (1-based)
- `end_line` (Number): End line number (1-based)
- `end_column` (Number): End column number (1-based)

**Note**: Line and column numbers are 1-indexed for human readability. Offsets are 0-indexed for programmatic use.

## Complete Examples

### Type Mismatch Error

```json
{
  "error_kind": {
    "type": "TypeMismatch",
    "data": {
      "expected": "number",
      "got": "string"
    }
  },
  "code": "Q-1-11",
  "message": "Expected number, got string",
  "hints": [
    "Use a numeric value without quotes?"
  ],
  "instance_path": [
    {"type": "Key", "value": "age"}
  ],
  "schema_path": [
    "object",
    "properties",
    "age",
    "number"
  ],
  "source_range": {
    "filename": "user.yaml",
    "start_offset": 5,
    "end_offset": 20,
    "start_line": 1,
    "start_column": 6,
    "end_line": 1,
    "end_column": 20
  }
}
```

### Missing Required Property Error

```json
{
  "error_kind": {
    "type": "MissingRequiredProperty",
    "data": {
      "property": "author"
    }
  },
  "code": "Q-1-10",
  "message": "Missing required property 'author'",
  "hints": [
    "Add the `author` property to your YAML document?"
  ],
  "instance_path": [],
  "schema_path": [
    "object"
  ],
  "source_range": {
    "filename": "document.yaml",
    "start_offset": 0,
    "end_offset": 109,
    "start_line": 1,
    "start_column": 1,
    "end_line": 5,
    "end_column": 1
  }
}
```

### Nested Path Error

```json
{
  "error_kind": {
    "type": "StringPatternMismatch",
    "data": {
      "value": "invalid-email",
      "pattern": "^[^@]+@[^@]+\\.[^@]+$"
    }
  },
  "code": "Q-1-15",
  "message": "String does not match pattern",
  "hints": [
    "Check that the string matches the expected format?"
  ],
  "instance_path": [
    {"type": "Key", "value": "user"},
    {"type": "Key", "value": "email"}
  ],
  "schema_path": [
    "object",
    "properties",
    "user",
    "object",
    "properties",
    "email",
    "string"
  ],
  "source_range": {
    "filename": "config.yaml",
    "start_offset": 35,
    "end_offset": 49,
    "start_line": 4,
    "start_column": 10,
    "end_line": 4,
    "end_column": 24
  }
}
```

## Usage in Downstream Tools

### TypeScript/JavaScript

```typescript
interface ValidationDiagnostic {
  error_kind: {
    type: string;
    data: Record<string, any>;
  };
  code: string;
  message: string;
  hints?: string[];
  instance_path: PathSegment[];
  schema_path: string[];
  source_range?: SourceRange;
}

type PathSegment =
  | { type: "Key"; value: string }
  | { type: "Index"; value: number };

interface SourceRange {
  filename: string;
  start_offset: number;
  end_offset: number;
  start_line: number;
  start_column: number;
  end_line: number;
  end_column: number;
}

// Example usage
function handleValidationError(diagnostic: ValidationDiagnostic) {
  switch (diagnostic.error_kind.type) {
    case "TypeMismatch":
      console.error(`Expected ${diagnostic.error_kind.data.expected}, got ${diagnostic.error_kind.data.got}`);
      break;
    case "MissingRequiredProperty":
      console.error(`Missing property: ${diagnostic.error_kind.data.property}`);
      break;
    // ... handle other types
  }
}
```

### Python

```python
from dataclasses import dataclass
from typing import Optional, Union, List, Dict, Any

@dataclass
class KeySegment:
    type: str  # "Key"
    value: str

@dataclass
class IndexSegment:
    type: str  # "Index"
    value: int

PathSegment = Union[KeySegment, IndexSegment]

@dataclass
class SourceRange:
    filename: str
    start_offset: int
    end_offset: int
    start_line: int
    start_column: int
    end_line: int
    end_column: int

@dataclass
class ValidationDiagnostic:
    error_kind: Dict[str, Any]
    code: str
    message: str
    hints: Optional[List[str]]
    instance_path: List[PathSegment]
    schema_path: List[str]
    source_range: Optional[SourceRange]

# Example usage
def handle_validation_error(diagnostic: dict):
    error_type = diagnostic["error_kind"]["type"]

    if error_type == "TypeMismatch":
        data = diagnostic["error_kind"]["data"]
        print(f"Expected {data['expected']}, got {data['got']}")
    elif error_type == "MissingRequiredProperty":
        prop = diagnostic["error_kind"]["data"]["property"]
        print(f"Missing property: {prop}")
```

### LSP Diagnostics

Convert to Language Server Protocol diagnostic format:

```typescript
import { Diagnostic, DiagnosticSeverity, Range, Position } from 'vscode-languageserver';

function toDiagnostic(vd: ValidationDiagnostic): Diagnostic {
  const range: Range = vd.source_range ? {
    start: {
      line: vd.source_range.start_line - 1,  // LSP uses 0-based
      character: vd.source_range.start_column - 1
    },
    end: {
      line: vd.source_range.end_line - 1,
      character: vd.source_range.end_column - 1
    }
  } : defaultRange;

  return {
    severity: DiagnosticSeverity.Error,
    range,
    message: vd.message,
    code: vd.code,
    source: 'quarto-yaml-validation',
    data: vd.error_kind  // Preserve structured data
  };
}
```

## Versioning

This schema follows semantic versioning. The current version is **1.0**.

Breaking changes to the JSON structure will increment the major version. Consumers should check for version compatibility.

## See Also

- [ValidationError](src/error.rs) - Internal error representation
- [ValidationDiagnostic](src/diagnostic.rs) - Wrapper type for JSON output
- [Error Codes](ERROR-CODES.md) - Complete list of error codes
