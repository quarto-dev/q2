# Quick Fix: Add Missing Catalog Entries <!-- quarto-error-code-audit-ignore-file -->

Date: 2025-11-23
Priority: HIGH
Estimated time: 15-30 minutes

## Critical Missing Codes

These codes MUST be added to `crates/quarto-error-reporting/error_catalog.json`:

1. **Q-1-90** - YAML validation: SchemaFalse
2. **Q-1-91** - YAML validation: AllOfFailed
3. **Q-1-92** - YAML validation: AnyOfFailed
4. **Q-1-93** - YAML validation: OneOfFailed
5. **Q-3-38** - JSON writer: Serialization failed

## Step-by-Step Fix

### 1. Examine Current Usage

```bash
# Check how Q-1-90 is used
rg "Q-1-90" --type rust -A3 -B3

# Location: private-crates/quarto-yaml-validation/src/error.rs
# Context: ValidationErrorKind::SchemaFalse => "Q-1-90"
```

### 2. Prepare Catalog Entries

Open `crates/quarto-error-reporting/error_catalog.json` and add these entries.

Insert after the existing Q-1-* entries (after Q-1-20):

```json
  "Q-1-90": {
    "subsystem": "yaml",
    "title": "Schema False Validation",
    "message_template": "Validation failed: schema is defined as false (always fails).",
    "docs_url": "https://quarto.org/docs/errors/Q-1-90",
    "since_version": "99.9.9"
  },
  "Q-1-91": {
    "subsystem": "yaml",
    "title": "AllOf Validation Failed",
    "message_template": "AllOf validation failed: one or more schemas did not match.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-91",
    "since_version": "99.9.9"
  },
  "Q-1-92": {
    "subsystem": "yaml",
    "title": "AnyOf Validation Failed",
    "message_template": "AnyOf validation failed: none of the schemas matched.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-92",
    "since_version": "99.9.9"
  },
  "Q-1-93": {
    "subsystem": "yaml",
    "title": "OneOf Validation Failed",
    "message_template": "OneOf validation failed: expected exactly one schema to match.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-93",
    "since_version": "99.9.9"
  },
```

Insert after existing Q-3-* entries (after Q-3-55):

```json
  "Q-3-38": {
    "subsystem": "writer",
    "title": "JSON Serialization Failed",
    "message_template": "An error occurred while serializing the document to JSON format.",
    "docs_url": "https://quarto.org/docs/errors/Q-3-38",
    "since_version": "99.9.9"
  }
```

### 3. Verify JSON Format

```bash
# Check that JSON is still valid
jq . crates/quarto-error-reporting/error_catalog.json > /dev/null
echo "JSON valid: $?"  # Should print 0

# Count codes (should now be 72 instead of 67)
jq 'keys | length' crates/quarto-error-reporting/error_catalog.json
```

### 4. Test

```bash
# Run the audit again
./scripts/quick-error-audit.sh

# Should now show:
# - Missing from catalog: 47 (down from 52)
# - The 5 codes we added should no longer appear in /tmp/missing.txt
```

### 5. Verify Each Code

```bash
# Verify Q-1-90
jq '."Q-1-90"' crates/quarto-error-reporting/error_catalog.json

# Verify Q-1-91
jq '."Q-1-91"' crates/quarto-error-reporting/error_catalog.json

# Verify Q-1-92
jq '."Q-1-92"' crates/quarto-error-reporting/error_catalog.json

# Verify Q-1-93
jq '."Q-1-93"' crates/quarto-error-reporting/error_catalog.json

# Verify Q-3-38
jq '."Q-3-38"' crates/quarto-error-reporting/error_catalog.json
```

### 6. Commit

```bash
git add crates/quarto-error-reporting/error_catalog.json
git commit -m "Add missing error catalog entries for YAML validation and JSON writer

- Q-1-90: Schema false validation
- Q-1-91: AllOf validation failed
- Q-1-92: AnyOf validation failed
- Q-1-93: OneOf validation failed
- Q-3-38: JSON serialization failed

These codes are actively used in the codebase but were missing from the
central error catalog. Found via error code audit.

Refs: claude-notes/investigations/2025-11-23-error-code-audit-results.md"
```

## Complete Entries with Better Messages

If you want to write more detailed messages, here's the research:

### Q-1-90 (SchemaFalse)

```rust
// From: private-crates/quarto-yaml-validation/src/error.rs
ValidationErrorKind::SchemaFalse => "Schema 'false' always fails validation"
```

Better catalog entry:
```json
  "Q-1-90": {
    "subsystem": "yaml",
    "title": "Schema False Validation",
    "message_template": "Schema is defined as 'false', which always fails validation. This indicates the property or value is explicitly disallowed.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-90",
    "since_version": "99.9.9"
  }
```

### Q-1-91 (AllOfFailed)

```rust
ValidationErrorKind::AllOfFailed { failing_schemas } =>
    format!("AllOf validation failed for schemas: {:?}", failing_schemas)
```

Better catalog entry:
```json
  "Q-1-91": {
    "subsystem": "yaml",
    "title": "AllOf Validation Failed",
    "message_template": "AllOf constraint failed: the value must satisfy ALL of the specified schemas, but one or more schemas did not match.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-91",
    "since_version": "99.9.9"
  }
```

### Q-1-92 (AnyOfFailed)

```rust
ValidationErrorKind::AnyOfFailed { attempted_schemas } =>
    format!("AnyOf validation failed (tried {} schemas, none matched)", attempted_schemas)
```

Better catalog entry:
```json
  "Q-1-92": {
    "subsystem": "yaml",
    "title": "AnyOf Validation Failed",
    "message_template": "AnyOf constraint failed: the value must satisfy at least ONE of the specified schemas, but none matched.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-92",
    "since_version": "99.9.9"
  }
```

### Q-1-93 (OneOfFailed)

```rust
ValidationErrorKind::OneOfFailed { matching_schemas } => {
    if matching_schemas.is_empty() {
        "OneOf validation failed (no schemas matched)".to_string()
    } else {
        format!("OneOf validation failed (multiple schemas matched: {:?})", matching_schemas)
    }
}
```

Better catalog entry:
```json
  "Q-1-93": {
    "subsystem": "yaml",
    "title": "OneOf Validation Failed",
    "message_template": "OneOf constraint failed: the value must satisfy EXACTLY ONE of the specified schemas, but either none or multiple schemas matched.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-93",
    "since_version": "99.9.9"
  }
```

### Q-3-38 (JSON Serialization)

```rust
// From: crates/quarto-markdown-pandoc/src/writers/json.rs
vec![quarto_error_reporting::DiagnosticMessage {
    code: Some("Q-3-38".to_string()),
    title: "JSON serialization failed".to_string(),
    // ...
}]
```

Better catalog entry:
```json
  "Q-3-38": {
    "subsystem": "writer",
    "title": "JSON Serialization Failed",
    "message_template": "Failed to serialize the document to JSON format. This may indicate an internal error or unsupported document structure.",
    "docs_url": "https://quarto.org/docs/errors/Q-3-38",
    "since_version": "99.9.9"
  }
```

## After Adding Entries

### Update Documentation

These codes should eventually have documentation pages at:
- https://quarto.org/docs/errors/Q-1-90
- https://quarto.org/docs/errors/Q-1-91
- https://quarto.org/docs/errors/Q-1-92
- https://quarto.org/docs/errors/Q-1-93
- https://quarto.org/docs/errors/Q-3-38

(These can be created later as part of documentation work)

### Consider Error Corpus

For Q-1-90 through Q-1-93, consider adding test cases to demonstrate the errors in `private-crates/quarto-yaml-validation/tests/`.

## Verification Checklist

- [ ] All 5 entries added to error_catalog.json
- [ ] JSON format is valid (jq validates)
- [ ] Code count increased from 67 to 72
- [ ] Audit script shows 47 missing (down from 52)
- [ ] Each code has proper subsystem
- [ ] Each code has title, message_template, docs_url, since_version
- [ ] Git commit created with descriptive message

## Related Files

- Catalog: `crates/quarto-error-reporting/error_catalog.json`
- YAML validation: `private-crates/quarto-yaml-validation/src/error.rs`
- JSON writer: `crates/quarto-markdown-pandoc/src/writers/json.rs`
- Audit results: `claude-notes/investigations/2025-11-23-error-code-audit-results.md`
