# Error Message System - Addendum for Error Codes

**Date**: 2025-10-28
**Context**: Additional steps needed when adding error messages with error codes

## What's Missing from the Main Document

The main document (`2025-10-28-error-message-system.md`) covers the core error message system well, but it was written BEFORE we integrated error codes. Here's what's missing:

## Adding Error Codes to New Error Messages

When you add a new error message (following the steps in the main document), you MUST also:

### Step 3.5: Add Error Code to JSON Spec

**IMPORTANT**: After writing your error spec JSON file, add a `"code"` field at the top:

```json
{
    "code": "Q-2-6",  // ADD THIS - use next sequential number
    "title": "Unclosed Strong Emphasis",
    "message": "I reached the end of the block before finding a closing '**' for the strong emphasis.",
    "captures": [...],
    "notes": [...]
}
```

**How to determine the next code:**
1. Check `crates/quarto-error-reporting/error_catalog.json`
2. Look for the highest Q-2-x code (e.g., Q-2-5)
3. Use the next sequential number (e.g., Q-2-6)

### Step 4.5: Add Entry to Error Catalog

After creating the error corpus files, update the error catalog:

**File**: `crates/quarto-error-reporting/error_catalog.json`

Add an entry for your new error code:

```json
{
  // ... existing entries ...
  "Q-2-5": {
    "subsystem": "markdown",
    "title": "Unclosed Emphasis",
    "message_template": "I reached the end of the block before finding a closing '_' for the emphasis.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-5",
    "since_version": "99.9.9"
  },
  "Q-2-6": {  // NEW ENTRY
    "subsystem": "markdown",
    "title": "Unclosed Strong Emphasis",
    "message_template": "I reached the end of the block before finding a closing '**' for the strong emphasis.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-6",
    "since_version": "99.9.9"
  }
}
```

**Important details:**
- `subsystem`: Use "markdown" for all parse errors
- `title`: Should match the title in your error corpus JSON
- `message_template`: Should match the message in your error corpus JSON
- `docs_url`: Follow the pattern (even though docs don't exist yet)
- `since_version`: Use "99.9.9" for now

### Updated Complete Workflow

Here's the complete workflow with error codes:

1. **Create error example**: `resources/error-corpus/006.qmd`
   ```markdown
   Unfinished **strong.
   ```

2. **Run error state reporter**:
   ```bash
   cd crates/quarto-markdown-pandoc
   cargo run -- --_internal-report-error-state -i resources/error-corpus/006.qmd
   ```

3. **Write error spec** with code: `resources/error-corpus/006.json`
   ```json
   {
       "code": "Q-2-6",
       "title": "Unclosed Strong Emphasis",
       "message": "I reached the end of the block before finding a closing '**' for the strong emphasis.",
       "captures": [
           {
               "label": "strong-start",
               "row": 0,
               "column": 11,
               "size": 2
           }
       ],
       "notes": [
           {
               "message": "This is the opening delimiter for the strong emphasis",
               "label": "strong-start",
               "noteType": "simple"
           }
       ]
   }
   ```

4. **Update error catalog**: `crates/quarto-error-reporting/error_catalog.json`
   ```json
   {
     // ... existing Q-2-5 ...
     "Q-2-6": {
       "subsystem": "markdown",
       "title": "Unclosed Strong Emphasis",
       "message_template": "I reached the end of the block before finding a closing '**' for the strong emphasis.",
       "docs_url": "https://quarto.org/docs/errors/Q-2-6",
       "since_version": "99.9.9"
     }
   }
   ```

5. **Build error table**:
   ```bash
   ./scripts/build_error_table.ts
   ```

6. **Build and test**:
   ```bash
   cargo build
   cargo run -- -i resources/error-corpus/006.qmd
   ```

7. **Accept snapshots**:
   ```bash
   cargo insta test --accept -p quarto-markdown-pandoc --test test_error_corpus
   ```

8. **Verify all tests pass**:
   ```bash
   cargo test -p quarto-markdown-pandoc
   ```

### Expected Output

With error codes, you should see:

```
Error: [Q-2-6] Unclosed Strong Emphasis
   ╭─[resources/error-corpus/006.qmd:1:19]
   │
 1 │ Unfinished **strong.
   │            ┬┬      ┬
   │            ╰───────── This is the opening delimiter for the strong emphasis
   │                   │
   │                   ╰── I reached the end of the block before finding a closing '**' for the strong emphasis.
───╯
```

JSON output:
```json
{
  "code": "Q-2-6",
  "title": "Unclosed Strong Emphasis",
  ...
}
```

## Error Code Numbering

We use **sequential numbering** starting from Q-2-1:

- Q-2-1: Unclosed Span
- Q-2-2: Mismatched Delimiter in Attribute Specifier
- Q-2-3: Key-value Pair Before Class Specifier in Attribute
- Q-2-4: Missing Space After Div Fence
- Q-2-5: Unclosed Emphasis
- Q-2-6: (next available)
- ...

**Do NOT skip numbers or try to organize by category.** Just use the next sequential number.

## Common Mistakes to Avoid

1. **Forgetting the code field**: The JSON spec MUST have `"code": "Q-2-X"` at the top
2. **Skipping catalog update**: You MUST add the entry to `error_catalog.json`
3. **Wrong subsystem**: Use "markdown" for parse errors (not "yaml" or "internal")
4. **Mismatched titles/messages**: Keep them consistent between error corpus and catalog
5. **Not accepting snapshots**: Run `cargo insta test --accept` after adding new errors
6. **Forgetting to rebuild**: Run `./scripts/build_error_table.ts` AND `cargo build`

## Quick Checklist

When adding a new error message:

- [ ] Create NNN.qmd with minimal example
- [ ] Run `--_internal-report-error-state` to get token info
- [ ] Create NNN.json with `"code": "Q-2-X"` (use next sequential number)
- [ ] Add Q-2-X entry to `error_catalog.json`
- [ ] Run `./scripts/build_error_table.ts`
- [ ] Run `cargo build`
- [ ] Test: `cargo run -- -i resources/error-corpus/NNN.qmd`
- [ ] Accept snapshots: `cargo insta test --accept -p quarto-markdown-pandoc --test test_error_corpus`
- [ ] Verify: `cargo test -p quarto-markdown-pandoc`

## Files to Modify

For each new error message, you will touch:

1. **NEW**: `resources/error-corpus/NNN.qmd` (the example)
2. **NEW**: `resources/error-corpus/NNN.json` (the spec WITH code)
3. **EDIT**: `crates/quarto-error-reporting/error_catalog.json` (add Q-2-X entry)
4. **AUTO-GENERATED**: `resources/error-corpus/_autogen-table.json` (via build script)
5. **AUTO-GENERATED**: Snapshot files (via cargo insta)

**Do NOT manually edit**:
- `_autogen-table.json` (generated by build script)
- Snapshot files (managed by cargo insta)
- Any Rust code (the system is already set up)

## References

- Main document: `claude-notes/analysis/2025-10-28-error-message-system.md`
- Error codes integration: `claude-notes/analysis/2025-10-28-error-codes-and-messages.md`
- This session transcript: Shows complete working example for unclosed emphasis
