# Error Message System Instructions

**Last Updated**: 2025-11-14
**Status**: Current and complete reference

This document consolidates everything you need to know about the error message system in `quarto-markdown-pandoc`.

## Table of Contents

1. [System Overview](#system-overview)
2. [File Structure](#file-structure)
3. [Adding New Error Messages](#adding-new-error-messages)
4. [Adding New Test Cases to Existing Errors](#adding-new-test-cases-to-existing-errors)
5. [Technical Details](#technical-details)
6. [Quick Reference](#quick-reference)

---

## System Overview

### Core Concept

The error message system is based on **Clinton Jeffery's TOPLAS 2003 paper** "Generating Syntax Errors from Examples". Instead of manually mapping parser states to error messages (which breaks on every grammar change), we:

1. Write **example error documents** that trigger specific parse errors
2. Run the parser to capture **(state, symbol)** pairs at error locations
3. Automatically build a **lookup table** mapping these pairs to rich diagnostic messages

At runtime, when the parser encounters an error, it looks up the (state, symbol) pair and produces a helpful message with highlighted source locations.

### Key Components

```
crates/quarto-markdown-pandoc/
├── resources/error-corpus/
│   ├── Q-*.json                    # Source: error specs with test cases
│   ├── case-files/                 # Generated: test .qmd files
│   └── _autogen-table.json         # Generated: (state, sym) → message lookup
├── scripts/
│   ├── build_error_table.ts        # Generates autogen table from Q-*.json
│   └── migrate_error_corpus.ts     # Migration tool (historical)
└── src/readers/
    ├── qmd_error_message_table.rs  # Runtime lookup (embeds autogen table)
    └── qmd_error_messages.rs       # Ariadne report rendering

crates/quarto-error-reporting/
└── error_catalog.json              # Registry of all error codes
```

---

## File Structure

### Consolidated Error Corpus Format (Current)

Each error code has **one JSON file** with multiple test cases:

**`resources/error-corpus/Q-2-10.json`**:
```json
{
  "code": "Q-2-10",
  "title": "Closed Quote Without Matching Open Quote",
  "message": "A space is causing a quote mark to be interpreted as a quotation close.",
  "notes": [
    {
      "message": "This is the opening quote. If you need an apostrophe, escape it with a backslash.",
      "label": "quote-start",
      "noteType": "simple"
    }
  ],
  "cases": [
    {
      "name": "simple",
      "description": "Apostrophe in plain text",
      "content": "a' b.",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 1,
          "size": 1
        }
      ],
      "prefixes": ["*", "[", "_"]
    },
    {
      "name": "in-emphasis",
      "description": "Apostrophe inside emphasis markup",
      "content": "*a' b.*",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 2,
          "size": 1
        }
      ]
    }
  ]
}
```

### Automatic Prefix Expansion

Cases can include an optional `prefixes` field to automatically generate variant test cases. This solves the exponential growth problem when testing errors in different parser contexts:

```json
{
  "name": "simple",
  "content": "*a",
  "captures": [...],
  "prefixes": ["[", "_", "^"]
}
```

The build script automatically generates:
- `Q-2-12-simple.qmd` - base case with content `*a`
- `Q-2-12-simple-1.qmd` - variant with content `[*a` (captures adjusted)
- `Q-2-12-simple-2.qmd` - variant with content `_*a` (captures adjusted)
- `Q-2-12-simple-3.qmd` - variant with content `^*a` (captures adjusted)

**Column adjustment**: All capture column values are automatically shifted by `prefix.length` for variant cases.

**Use case**: Different inline contexts (after `[`, `_`, `*`, etc.) produce different `lr_state` values for the same token. Prefixes allow comprehensive coverage without manually writing hundreds of test cases.

### Automatic Prefix and Suffix Expansion

Cases can also include an optional `prefixesAndSuffixes` field for testing errors within wrapping contexts (like emphasis, links, etc.):

```json
{
  "name": "simple",
  "content": "a' b.",
  "captures": [{"label": "quote-start", "row": 0, "column": 1, "size": 1}],
  "prefixesAndSuffixes": [
    ["**", "**\n"],
    ["*", "*\n"],
    ["[", "](url)\n"]
  ]
}
```

The build script automatically generates:
- `Q-2-10-simple.qmd` - base case with content `a' b.`
- `Q-2-10-simple-1.qmd` - variant with content `**a' b.**\n` (column: 1 + 2 = 3)
- `Q-2-10-simple-2.qmd` - variant with content `*a' b.*\n` (column: 1 + 1 = 2)
- `Q-2-10-simple-3.qmd` - variant with content `[a' b.](url)\n` (column: 1 + 1 = 2)

**Column adjustment**: All capture column values are automatically shifted by `prefix.length` for variant cases.

**Use case**: Testing the same error in different wrapping contexts (emphasis, strong, links, quotes, etc.) without duplicating the core error content.

**Note**: `prefixes` and `prefixesAndSuffixes` are mutually exclusive - use one or the other for a given case, not both.

### Duplicate Detection

The build script checks whether each prefix/variant generates a distinct `(lr_state, sym)` pair. If a prefix produces the same state as another variant, a warning is emitted but the build continues. This helps identify which prefixes are currently redundant while allowing them to remain for future grammar changes that might make them useful.

### Generated Files

When you run `./scripts/build_error_table.ts`, it:

1. **Clears** `resources/error-corpus/case-files/`
2. **Generates** `.qmd` files for each test case (base + prefix variants):
   - `case-files/Q-2-10-simple.qmd` (contains `a' b.`)
   - `case-files/Q-2-10-simple-1.qmd` (contains `*a' b.`)
   - `case-files/Q-2-10-simple-2.qmd` (contains `[a' b.`)
   - `case-files/Q-2-10-simple-3.qmd` (contains `_a' b.`)
   - `case-files/Q-2-10-in-emphasis.qmd` (contains `*a' b.*`)
3. **Runs parser** on each file with `--_internal-report-error-state`
4. **Captures** (state, sym, row, column) and consumed tokens
5. **Matches** captures to tokens by (row, column, size)
6. **Augments** captures with lrState and sym from tokens
7. **Writes** `_autogen-table.json` with all mappings
8. **Rebuilds** Rust code with new table

### Error Catalog

**`crates/quarto-error-reporting/error_catalog.json`**:
```json
{
  "Q-2-10": {
    "subsystem": "markdown",
    "title": "Closed Quote Without Matching Open Quote",
    "message_template": "A space is causing a quote mark to be interpreted as a quotation close.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-10",
    "since_version": "99.9.9"
  }
}
```

This is a **registry** of all error codes. Keep it in sync with error corpus.

---

## Adding New Error Messages

### Complete Workflow

Follow these steps in order:

#### 1. Determine the Next Error Code

```bash
cd crates/quarto-error-reporting
jq 'keys | map(select(startswith("Q-2-"))) | sort | last' error_catalog.json
```

Use the next sequential number (e.g., if last is `Q-2-26`, use `Q-2-27`).

#### 2. Create Minimal Error Example

Create a file with the **smallest possible** content that triggers the error:

**Good**: `Unfinished _emph.`
**Bad**: A 50-line document with multiple issues

#### 3. Capture Parser State

```bash
cd crates/quarto-markdown-pandoc
echo "Unfinished _emph." > /tmp/test.qmd
cargo run -- --_internal-report-error-state -i /tmp/test.qmd
```

**Example output**:
```json
{
  "errorStates": [
    {"state": 854, "sym": "_close_block", "row": 0, "column": 17}
  ],
  "tokens": [
    {"row": 0, "column": 11, "size": 1, "lrState": 125, "sym": "emphasis_delimiter"},
    {"row": 0, "column": 12, "size": 4, "lrState": 1434, "sym": "_word_no_digit"},
    {"row": 0, "column": 16, "size": 1, "lrState": 854, "sym": "."}
  ]
}
```

**Note the tokens** - you'll need their (row, column, size) for captures.

#### 4. Create Error Corpus JSON

Create `resources/error-corpus/Q-2-27.json`:

```json
{
  "code": "Q-2-27",
  "title": "Unclosed Emphasis",
  "message": "I reached the end of the block before finding a closing '_' for the emphasis.",
  "notes": [
    {
      "message": "This is the opening '_' mark",
      "label": "emphasis-start",
      "noteType": "simple"
    }
  ],
  "cases": [
    {
      "name": "simple",
      "description": "Simple unclosed emphasis",
      "content": "Unfinished _emph.",
      "captures": [
        {
          "label": "emphasis-start",
          "row": 0,
          "column": 11,
          "size": 1
        }
      ]
    }
  ]
}
```

**Critical**: Capture coordinates must **exactly match** a token from step 3.

#### 5. Update Error Catalog

Edit `crates/quarto-error-reporting/error_catalog.json`:

```json
{
  "Q-2-27": {
    "subsystem": "markdown",
    "title": "Unclosed Emphasis",
    "message_template": "I reached the end of the block before finding a closing '_' for the emphasis.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-27",
    "since_version": "99.9.9"
  }
}
```

**Important**: Keep title and message consistent with error corpus JSON.

#### 6. Build Error Table

```bash
cd crates/quarto-markdown-pandoc
./scripts/build_error_table.ts
```

This will:
- Generate `case-files/Q-2-27-simple.qmd`
- Run parser and match captures
- Update `_autogen-table.json`
- Touch Rust files to trigger rebuild

#### 7. Build and Test

```bash
cargo build
cargo run -- -i resources/error-corpus/case-files/Q-2-27-simple.qmd
```

**Expected output**:
```
Error: [Q-2-27] Unclosed Emphasis
   ╭─[Q-2-27-simple.qmd:1:17]
   │
 1 │ Unfinished _emph.
   │            ┬    ┬
   │            ╰────── This is the opening '_' mark
   │                 │
   │                 ╰── I reached the end of the block before finding a closing '_' for the emphasis.
───╯
```

#### 8. Update Test Snapshots

```bash
cargo insta test --accept -p quarto-markdown-pandoc --test test_error_corpus
```

#### 9. Verify All Tests Pass

```bash
cargo test -p quarto-markdown-pandoc
```

---

## Adding New Test Cases to Existing Errors

When you discover an error that **already has a code** but produces a generic "Parse error" in a new context:

### Example Scenario

You find that `*a" b*` (unclosed double quote inside emphasis) shows "Parse error" instead of "Q-2-11 Unclosed Double Quote".

### Steps

#### 1. Determine Error State

```bash
cd crates/quarto-markdown-pandoc
echo '*a" b*' > /tmp/test.qmd
cargo run -- --_internal-report-error-state -i /tmp/test.qmd
```

Note the (state, sym) pair and tokens.

#### 2. Edit Existing JSON File

Open `resources/error-corpus/Q-2-11.json` and add a new case:

```json
{
  "code": "Q-2-11",
  "title": "Unclosed Double Quote",
  "message": "I reached the end of the block before finding a closing '\"' for the quote.",
  "notes": [...],
  "cases": [
    {
      "name": "simple",
      "description": "Simple unclosed quote",
      "content": "\"Unclosed quote at end",
      "captures": [...]
    },
    {
      "name": "in-emphasis",
      "description": "Unclosed quote inside emphasis",
      "content": "*a\" b*",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 2,
          "size": 1
        }
      ]
    }
  ]
}
```

**Choose a descriptive case name**: `in-emphasis`, `in-link-text`, `in-heading`, etc.

#### 3. Rebuild Error Table

```bash
./scripts/build_error_table.ts
```

This will generate `case-files/Q-2-11-in-emphasis.qmd` and update the autogen table.

#### 4. Test

```bash
cargo build
echo '*a" b*' | cargo run -- --json-errors
```

Should now show `"code": "Q-2-11"` instead of generic parse error.

#### 5. Update Snapshots and Verify

```bash
cargo insta test --accept -p quarto-markdown-pandoc --test test_error_corpus
cargo test -p quarto-markdown-pandoc
```

---

## Technical Details

### How Capture Matching Works

The build script uses a **left join** on `(row, column, size)`:

1. You specify a capture in JSON: `{row: 0, column: 11, size: 1, label: "emphasis-start"}`
2. Script finds matching token: `{row: 0, column: 11, size: 1, lrState: 125, sym: "emphasis_delimiter"}`
3. Script augments capture: `{row: 0, column: 11, size: 1, label: "emphasis-start", lrState: 125, sym: "emphasis_delimiter"}`
4. At runtime, system uses `lrState` and `sym` to locate the capture in consumed tokens

**Critical**: If your capture coordinates don't match a consumed token, the build script will fail with an assertion error.

### Note Types

#### Simple Note
Points to a single location:
```json
{
  "message": "This is the opening bracket",
  "label": "span-start",
  "noteType": "simple"
}
```

Renders as a blue label at the captured location.

#### Label-Range Note
Points to a span between two locations:
```json
{
  "message": "This key-value pair cannot appear before the class specifier",
  "noteType": "label-range",
  "labelBegin": "key-value-start",
  "labelEnd": "key-value-end"
}
```

Requires two captures with matching labels. Renders as a blue label spanning from begin to end.

### Error Code Numbering

Use **sequential numbering**:
- Q-2-1, Q-2-2, Q-2-3, ..., Q-2-27, Q-2-28, ...
- **Do NOT** skip numbers or try to organize by category
- Just use the next available number

### Case Naming Conventions

Use **kebab-case** names that describe the context:
- `simple` - Most basic case
- `in-emphasis` - Inside emphasis markup
- `in-link-text` - Inside link text
- `in-heading` - Inside heading
- `in-strong-emphasis` - Inside strong emphasis
- `in-editorial-delete` - Inside editorial delete markup
- `in-inline-footnote` - Inside inline footnote

---

## Quick Reference

### Files to Create/Edit

For a **new error code Q-2-X**:
1. **CREATE**: `resources/error-corpus/Q-2-X.json`
2. **EDIT**: `crates/quarto-error-reporting/error_catalog.json`
3. **AUTO**: `resources/error-corpus/_autogen-table.json` (generated)
4. **AUTO**: `resources/error-corpus/case-files/Q-2-X-*.qmd` (generated)

For **adding a case** to existing error:
1. **EDIT**: `resources/error-corpus/Q-2-X.json` (add to cases array)
2. **AUTO**: `_autogen-table.json` and `case-files/` (regenerated)

### Commands

```bash
# Find next error code
cd crates/quarto-error-reporting
jq 'keys | map(select(startswith("Q-2-"))) | sort | last' error_catalog.json

# Capture parser state
cd crates/quarto-markdown-pandoc
cargo run -- --_internal-report-error-state -i /tmp/test.qmd

# Build error table
./scripts/build_error_table.ts

# Test error message
cargo run -- -i resources/error-corpus/case-files/Q-2-X-simple.qmd

# Update snapshots
cargo insta test --accept -p quarto-markdown-pandoc --test test_error_corpus

# Run all tests
cargo test -p quarto-markdown-pandoc
```

### Checklist

For **new error code**:
- [ ] Determine next error code (Q-2-X)
- [ ] Create minimal error example
- [ ] Run `--_internal-report-error-state` to get tokens
- [ ] Create `Q-2-X.json` with code, title, message, notes, and at least one case
- [ ] Add entry to `error_catalog.json`
- [ ] Run `./scripts/build_error_table.ts`
- [ ] Run `cargo build`
- [ ] Test the error message
- [ ] Accept snapshots with `cargo insta test --accept`
- [ ] Verify all tests pass

For **new test case**:
- [ ] Run `--_internal-report-error-state` on problematic input
- [ ] Edit existing `Q-2-X.json`, add new case to `cases` array
- [ ] Run `./scripts/build_error_table.ts`
- [ ] Run `cargo build`
- [ ] Test that the error message now appears
- [ ] Accept snapshots
- [ ] Verify tests pass

### Design Principles

1. **Minimal Examples**: Each case should demonstrate exactly one error in the smallest possible input
2. **Clear Messages**: Use first person ("I expected..."), standard terminology, actionable language
3. **Helpful Context**: Highlight where the problem began and what might fix it
4. **Consistency**: Match tone and style of existing error messages
5. **No Duplication**: Error metadata (code, title, message, notes) appears once; cases array contains variations

### Common Mistakes

1. **Forgetting error code**: JSON must have `"code": "Q-2-X"` at top level
2. **Skipping catalog**: Must add entry to `error_catalog.json`
3. **Wrong coordinates**: Capture (row, column, size) must exactly match a consumed token
4. **Not rebuilding**: Must run both `build_error_table.ts` AND `cargo build`
5. **Ignoring snapshots**: Must run `cargo insta test --accept` after changes
6. **Editing generated files**: Never manually edit `_autogen-table.json` or `case-files/`

---

## References

- **Paper**: Clinton Jeffery, "Generating Syntax Errors from Examples", TOPLAS 2003
- **Implementation**: `scripts/build_error_table.ts`
- **Runtime**: `src/readers/qmd_error_message_table.rs`, `src/readers/qmd_error_messages.rs`
- **Examples**: `resources/error-corpus/Q-*.json`
- **Previous docs**:
  - `claude-notes/analysis/2025-10-28-error-message-system.md`
  - `claude-notes/analysis/2025-10-28-error-message-system-addendum.md`
  - Consolidated into this file on 2025-11-14
