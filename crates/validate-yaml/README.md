# validate-yaml

A command-line tool for validating YAML documents against schemas.

## Usage

```bash
validate-yaml --schema <schema-file> --input <input-file>
```

## Example

Given a schema file `schema.yaml`:

```yaml
object:
  properties:
    title:
      string:
        description: "Document title"
    author:
      string:
        description: "Document author"
  required:
    - title
    - author
```

And a document `document.yaml`:

```yaml
title: "My Document"
author: "John Doe"
```

Run validation:

```bash
validate-yaml --schema schema.yaml --input document.yaml
```

### Success Output

```
✓ Validation successful
  Input: document.yaml
  Schema: schema.yaml
```

### Failure Output

```
Error: YAML Validation Failed (Q-1-10)

Problem: Missing required property 'author'

  ✖ At document root
  ℹ Schema constraint: object
  ✖ In file `document.yaml` at line 2, column 6

  ? Add the `author` property to your YAML document?

See https://quarto.org/docs/errors/Q-1-10 for more information
```

## Features

- **YAML 1.2 Support**: Uses yaml-rust2 for consistent YAML 1.2 parsing
- **Structured Error Messages**: Tidyverse-style error reporting with:
  - Error codes (Q-1-xxx) for searchability
  - Clear problem statements
  - Contextual details with visual bullets (✖ error, ℹ info)
  - Actionable hints for fixing issues
  - Documentation links for each error code
- **Source Location Tracking**: Error messages include file, line, and column information
- **Schema Validation**: Supports Quarto's simplified JSON Schema subset including:
  - Basic types (boolean, number, string, null, any)
  - Enums
  - Objects with properties and required fields
  - Arrays
  - AnyOf and AllOf combinators
  - Schema references ($ref)

## Error Codes

Validation errors include searchable error codes:

- **Q-1-10**: Missing required property
- **Q-1-11**: Type mismatch (expected one type, got another)
- **Q-1-12**: Invalid enum value
- **Q-1-13**: Array length constraint violation
- **Q-1-14**: String pattern mismatch
- **Q-1-15**: Number range violation
- **Q-1-16**: Object property count violation
- **Q-1-17**: Unresolved schema reference
- **Q-1-18**: Unknown property in closed object
- **Q-1-19**: Array uniqueness violation
- **Q-1-99**: Generic validation error

Each error code links to detailed documentation at `https://quarto.org/docs/errors/Q-1-XX`.

## Exit Codes

- `0`: Validation successful
- `1`: Validation failed or error occurred

## Test Data

The `test-data/` directory contains example schemas and documents for testing:

- `simple-schema.yaml`: Example schema with basic types
- `valid-document.yaml`: Document that passes validation
- `invalid-document.yaml`: Document that fails validation (missing required property)
- `type-mismatch-document.yaml`: Document with type errors
