# YAML Validator Analysis

## Overview

Quarto's YAML validation system validates YAML content (frontmatter, project configs, cell options, brand configs) against JSON-Schema-like schemas, providing detailed error messages with source locations.

**Key Feature**: All errors include precise source locations (file, line, column) even when YAML is extracted from larger documents, thanks to MappedString integration.

## Architecture

```
YAML Intelligence (yaml-intelligence/)
    ↓
Annotated YAML Parser (annotated-yaml.ts)
    ↓
YAML Validator (yaml-validation/)
    ↓
YAML Schemas (yaml-schema/)
```

## Components

### 1. YAML Intelligence (`yaml-intelligence/`)

**Purpose**: IDE features - completions, hover, diagnostics

**Key files**:
- `yaml-intelligence.ts` - Main entry point for LSP features
- `annotated-yaml.ts` - Parse YAML into AnnotatedParse trees
- `parsing.ts` - Tree-sitter integration, cursor location
- `hover.ts` - Hover information for YAML keys/values
- `vs-code.ts` - VS Code-specific exports (used by LSP)

**Completions flow**:
1. Get cursor position in YAML
2. Parse YAML into AnnotatedParse tree
3. Navigate tree to cursor position
4. Find applicable schema for that location
5. Generate completions from schema
6. Return to LSP

**Example**:
```yaml
format:
  html:
    theme: <cursor>
```
- Navigate to `format.html.theme`
- Find schema for `theme` (enum of theme names)
- Return completion items: ["default", "cosmo", "cerulean", ...]

### 2. Annotated YAML Parser (`annotated-yaml.ts`)

**Purpose**: Parse YAML into AnnotatedParse trees with source location tracking

**Dual parser strategy**:

```typescript
readAnnotatedYamlFromMappedString(mappedSource: MappedString, lenient: bool)
```

- **Lenient mode** (`lenient=true`): Uses tree-sitter-yaml
  - Error recovery (can parse incomplete YAML)
  - Used for IDE features (completions on incomplete input)
  - Sometimes produces incorrect parses

- **Strict mode** (`lenient=false`): Uses js-yaml
  - JSON-Schema compliant
  - Used for validation (must be correct)
  - Fails on malformed YAML

**AnnotatedParse structure**:
```typescript
interface AnnotatedParse {
  start: number;              // Offset in source.value
  end: number;
  result: JSONValue;          // The parsed value
  kind: string;               // "object", "array", "string", etc.
  source: MappedString;       // The source text (with mapping!)
  components: AnnotatedParse[]; // Children (object props, array items)
}
```

**Example AnnotatedParse**:
```yaml
title: "My Document"
format: html
```

```
AnnotatedParse {
  start: 0, end: 33,
  kind: "object",
  result: { title: "My Document", format: "html" },
  components: [
    AnnotatedParse {
      start: 0, end: 20,
      kind: "string",
      result: "My Document",
      components: []
    },
    AnnotatedParse {
      start: 21, end: 33,
      kind: "string",
      result: "html",
      components: []
    }
  ]
}
```

### 3. YAML Validator (`yaml-validation/`)

**Purpose**: Validate AnnotatedParse against schemas, collect errors

**Key files**:
- `validator.ts` - Main validation logic
- `errors.ts` - Error creation and formatting
- `schema-navigation.ts` - Navigate schemas by path
- `schema-utils.ts` - Schema utilities (completions, walking)
- `schema.ts` - Schema state management
- `resolve.ts` - Resolve $ref references

**Validation flow**:
```typescript
// From validator.ts
class ValidationContext {
  validate(
    schema: Schema,
    source: MappedString,
    value: AnnotatedParse,
    pruneErrors: bool
  ): LocalizedError[]
}
```

1. Walk the AnnotatedParse tree
2. At each node, validate against corresponding schema
3. Collect errors with full context (instancePath, schemaPath)
4. Convert to LocalizedError with source locations
5. Return user-friendly errors

**LocalizedError structure**:
```typescript
interface LocalizedError {
  violatingObject: AnnotatedParse;  // What failed validation
  schema: Schema;                    // What schema it failed
  message: string;                   // Human-readable message
  instancePath: (string|number)[];   // Path in YAML (e.g., ["format", "html", "theme"])
  schemaPath: (string|number)[];     // Path in schema (for debugging)
  source: MappedString;              // Original source
  location: ErrorLocation;           // { start: {line, col}, end: {line, col} }
  niceError: TidyverseError;         // Formatted error for display
}
```

**Error handling pipeline** (from `errors.ts`):

Multiple handlers improve error messages:
- `ignoreExprViolations` - Filter out !expr tag errors in allowed contexts
- `expandEmptySpan` - Expand zero-width error spans for visibility
- `improveErrorHeadingForValueErrors` - Better messages for value errors
- `checkForTypeMismatch` - Detect type errors (string vs number, etc.)
- `checkForBadBoolean` - Catch "yes/no" vs true/false
- `checkForBadColon` - Detect missing space after colon
- `checkForBadEquals` - Detect `=` instead of `:`
- `identifyKeyErrors` - Identify disallowed keys
- `checkForNearbyCorrection` - Suggest typo corrections (edit distance)
- `checkForNearbyRequired` - Suggest required fields user might have meant
- `schemaDefinedErrors` - Use custom errorMessage from schema

**Example error transformation**:

Original schema error:
```
property "tema" not allowed
```

After handlers:
```
Unknown key "tema" in format.html

Did you mean "theme"?

Valid keys: theme, css, toc, number-sections, ...
```

### 4. YAML Schemas (`yaml-schema/`)

**Purpose**: Define the structure and validation rules for Quarto YAML

**Schema types** (from `types.ts`):
```typescript
type Schema =
  | FalseSchema     // Always fails (false)
  | TrueSchema      // Always passes (true)
  | BooleanSchema   // boolean type
  | NumberSchema    // number or integer
  | StringSchema    // string type
  | NullSchema      // null
  | EnumSchema      // enum (one of values)
  | AnySchema       // any type
  | AnyOfSchema     // one of (union)
  | AllOfSchema     // all of (intersection)
  | ArraySchema     // array
  | ObjectSchema    // object with properties
  | RefSchema       // $ref reference
```

**ObjectSchema** (most common):
```typescript
interface ObjectSchema {
  type: "object";
  properties?: Record<string, Schema>;
  patternProperties?: Record<string, Schema>;
  additionalProperties?: Schema | boolean;
  required?: string[];
  closed?: boolean;  // Quarto extension: disallow additional properties
  // ... annotations (description, documentation, completions, etc.)
}
```

**Schema annotations**:
```typescript
interface SchemaAnnotations {
  $id?: string;                      // Schema ID for references
  documentation?: SchemaDocumentation; // For HTML docs and hover
  description?: string;               // For error messages
  errorMessage?: string;              // Custom error message
  hidden?: boolean;                   // Hide from completions
  completions?: string[];             // Custom completion values
  exhaustiveCompletions?: boolean;    // Auto-trigger next completion
  tags?: Record<string, unknown>;     // Arbitrary metadata
}
```

**Key schema files**:
- `front-matter.ts` - Document frontmatter schema
- `project-config.ts` - _quarto.yml project config
- `chunk-metadata.ts` - Code cell options (#| key: value)
- `brand.ts` - _brand.yml brand configuration
- `format-schemas.ts` - Format-specific schemas (html, pdf, docx, etc.)
- `definitions.ts` - Shared schema definitions
- `from-yaml.ts` - Load schemas from YAML files

**Schema composition**:

Schemas use `$ref` and `allOf`/`anyOf` to compose:

```typescript
{
  type: "object",
  properties: {
    format: {
      anyOf: [
        { $ref: "#/definitions/html-format" },
        { $ref: "#/definitions/pdf-format" },
        { $ref: "#/definitions/docx-format" }
      ]
    }
  }
}
```

### 5. Completions System

**How completions work**:

1. **Schema walking** (`schema-utils.ts:schemaCompletions`):
   ```typescript
   schemaCompletions(
     schema: Schema,
     path: InstancePath,
     word: string,
     completionPosition: "key" | "value"
   ): Completion[]
   ```

2. **Completion generation**:
   - For object schemas: Generate key completions from `properties`
   - For enum schemas: Generate value completions from `enum`
   - For anyOf/allOf: Merge completions from all branches
   - Filter by user's partial input (`word`)

3. **Completion structure**:
   ```typescript
   interface Completion {
     display: string;           // What to show in UI
     type: "key" | "value";     // Completion type
     value: string;             // What to insert
     description: string;       // For hover/documentation
     suggest_on_accept: boolean; // Auto-trigger next completion
     schema?: Schema;           // Source schema
     documentation?: string;    // Markdown documentation
   }
   ```

4. **Exhaustive completions**:

   If `exhaustiveCompletions: true` on schema:
   - After accepting completion, auto-trigger next completion
   - Creates smooth UX for structured input (e.g., `format: html: theme: `)

## Integration Points

### With LSP

`vs-code.ts` exports functions for LSP (from YAML intelligence):

```typescript
// Used by quarto-lsp (TypeScript) and will be needed in Rust LSP
export async function getCompletions(context: EditorContext): Promise<CompletionResult>
export async function getLint(context: EditorContext): Promise<Array<LintItem>>
export async function getHover(context: EditorContext): Promise<HoverResult | null>
```

**EditorContext**:
```typescript
interface EditorContext {
  filepath: string;
  formats: string[];          // Active formats (html, pdf, etc.)
  code: string[];             // Lines of code
  position: { row: number; column: number };
  explicit: boolean;          // User explicitly requested (Ctrl+Space)
}
```

### With CLI

CLI uses validation for:
- Project config validation (_quarto.yml)
- Document frontmatter validation
- Extension validation (_extension.yml)
- Brand validation (_brand.yml)

Errors are formatted as TidyverseError and displayed to user.

## Key Insights for Rust Port

### 1. **Core Dependencies**

YAML system depends on:
- **MappedString** (analyzed separately) - Critical for error locations
- **Schema definitions** - Need to port all schema TypeScript to Rust
- **YAML parser** - Need Rust equivalent (serde_yaml + tree-sitter-yaml)
- **Error formatting** - TidyverseError format must be preserved

### 2. **Parser Strategy**

```rust
pub enum YamlParser {
    Strict,   // serde_yaml - for validation
    Lenient,  // tree-sitter - for IDE features
}

pub fn parse_yaml(
    source: MappedString,
    parser: YamlParser
) -> Result<AnnotatedParse, ParseError>
```

### 3. **AnnotatedParse in Rust**

```rust
pub struct AnnotatedParse {
    start: usize,
    end: usize,
    result: serde_json::Value,  // or custom JsonValue enum
    kind: String,
    source: Rc<MappedString>,
    components: Vec<AnnotatedParse>,
}
```

### 4. **Schema Representation**

**Option A**: Rust enums matching TypeScript
```rust
pub enum Schema {
    False,
    True,
    Boolean(BooleanSchema),
    Number(NumberSchema),
    String(StringSchema),
    // ... etc
}

pub struct ObjectSchema {
    properties: HashMap<String, Schema>,
    required: Vec<String>,
    // ... annotations
}
```

**Option B**: JSON Schema directly (via serde)
```rust
// Load schema from JSON, validate using existing crate
use jsonschema;

let schema = jsonschema::JSONSchema::compile(&schema_json)?;
let result = schema.validate(&instance);
```

**Recommendation**: **Option A** - more control over error messages and completions

### 5. **Schema Loading**

Current TypeScript:
- Schemas defined in TypeScript code
- Some loaded from YAML/JSON resources
- Schema definitions are ~2000 LOC

Rust approach:
- Define schemas in Rust (most control)
- Or: Define in JSON/YAML, deserialize to Rust structs
- Or: Hybrid - core schemas in Rust, format schemas from files

### 6. **Completion Generation**

```rust
pub fn schema_completions(
    schema: &Schema,
    path: &[PathComponent],
    word: &str,
    position: CompletionPosition,
) -> Vec<Completion>

pub enum PathComponent {
    Key(String),
    Index(usize),
}

pub enum CompletionPosition {
    Key,
    Value,
}
```

### 7. **Error Handler Pipeline**

```rust
pub trait ErrorHandler {
    fn handle(&self, error: &mut LocalizedError, source: &MappedString) -> bool;
}

pub struct ErrorPipeline {
    handlers: Vec<Box<dyn ErrorHandler>>,
}

impl ErrorPipeline {
    fn process(&self, errors: Vec<LocalizedError>, source: &MappedString) -> Vec<LocalizedError> {
        // Apply each handler in sequence
    }
}
```

### 8. **Critical Features**

Must preserve:
1. **Precise error locations** (via MappedString)
2. **Typo suggestions** (edit distance for key names)
3. **Context-aware completions** (based on formats, position)
4. **Custom error messages** (from schema annotations)
5. **Exhaustive completion** (auto-trigger)
6. **Schema references** ($ref resolution)

## Estimation

### Complexity: **High**

**Why**:
- Large surface area (~6,500 LOC TypeScript)
- Complex schema system with many edge cases
- Error handling must be excellent (user-facing)
- Integration with MappedString critical
- Completions need schema navigation logic
- Must handle malformed input gracefully (IDE features)

### LOC Estimate: ~4,000-5,000 lines Rust

**Breakdown**:
- AnnotatedParse + parsing: ~500 lines
- Validator core: ~800 lines
- Error handlers: ~600 lines
- Schema types: ~400 lines
- Schema definitions (frontmatter, project, etc.): ~1,500 lines
- Completion generation: ~400 lines
- Schema utilities (navigation, walking): ~500 lines
- Tests: ~300 lines

### Time Estimate: 4-6 weeks

**Phases**:
1. **Week 1**: AnnotatedParse + YAML parsing
   - Integrate serde_yaml and tree-sitter-yaml
   - Build AnnotatedParse from parse results
   - Connect with MappedString

2. **Week 2**: Schema types + basic validator
   - Define Schema enum and variants
   - Implement core validation logic
   - Basic error reporting

3. **Week 3**: Schema definitions
   - Port frontmatter schema
   - Port project config schema
   - Port format schemas

4. **Week 4**: Completions
   - Schema walking and navigation
   - Completion generation
   - Exhaustive completion logic

5. **Week 5**: Error handlers
   - Port all error improvement handlers
   - Typo suggestions
   - Context formatting

6. **Week 6**: Polish + testing
   - Integration tests
   - Error message QA
   - Performance testing

## Testing Strategy

### Unit Tests
- AnnotatedParse construction
- Schema validation (each schema type)
- Completion generation
- Error handler pipeline

### Integration Tests
- Full YAML validation (frontmatter, project config)
- Error messages match expectations
- Completions at various cursor positions
- Schema reference resolution

### Regression Tests
- Port existing YAML validation tests from quarto-cli
- Ensure error messages are as good or better

## Open Questions

1. **Schema definition format**?
   - TypeScript code (most control)
   - JSON Schema files (standard)
   - Hybrid approach?

2. **YAML parser**?
   - serde_yaml alone (strict only, no error recovery)
   - tree-sitter-yaml alone (error recovery, but sometimes wrong)
   - Both (like current, but complex)

3. **Performance**?
   - Validation happens on every keystroke in LSP
   - Need fast schema navigation
   - Caching strategies?

4. **Schema updates**?
   - Schemas will evolve with Quarto
   - Easier to update if in data files vs Rust code
   - But type safety is valuable

5. **Error message i18n**?
   - Current system is English only
   - Should Rust version support localization?
   - vscode-languageserver supports l10n

## Dependencies on Other Work

- **MappedString** must be implemented first
- **quarto-markdown** integration needed for tree-sitter
- **Error formatting** (TidyverseError) must be defined

## Notes

This is a critical system - YAML validation errors are very user-facing and must be excellent. The current TypeScript implementation has had years of refinement to provide helpful error messages. The Rust port must match or exceed this quality.
