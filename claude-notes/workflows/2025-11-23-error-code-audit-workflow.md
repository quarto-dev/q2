# Error Code Audit Workflow

<!-- quarto-error-code-audit-ignore-file -->

Date: 2025-11-23
File: claude-notes/workflows/2025-11-23-error-code-audit-workflow.md

## Purpose

This workflow enables systematic auditing of Q-*-* error code usage across the codebase to ensure consistency with the central error catalog (`crates/quarto-error-reporting/error_catalog.json`). The goal is to identify:

1. **Missing catalog entries** - Error codes used in source but not in error_catalog.json
2. **Orphaned catalog entries** - Error codes in catalog but never referenced in code
3. **Inconsistent usage** - Error codes used with wrong subsystem or format

## Automated vs Manual

### Automated Script (RECOMMENDED)

Use `scripts/audit-error-codes.py` for automated auditing.

**Prerequisites:**
- Python 3.6+ (usually already installed)
- **ripgrep (`rg` command)** - REQUIRED for fast code searching
  - Install: https://github.com/BurntSushi/ripgrep#installation
  - Quick install:
    ```bash
    brew install ripgrep  # macOS
    apt install ripgrep   # Ubuntu/Debian
    dnf install ripgrep   # Fedora
    ```

**Verify prerequisites:**
```bash
./scripts/check-dependencies.sh
# Or manually:
python3 --version  # Should show 3.6+
rg --version       # Should show ripgrep version
```

**Features:**
- Loads catalog codes from error_catalog.json
- Searches entire codebase with ripgrep
- Automatically categorizes missing codes:
  - **Legitimate missing** - Used in production code (HIGH PRIORITY)
  - **Test/Example codes** - Only in tests/docs (LOW PRIORITY)
  - **Invalid format** - Typos, test sentinels (INVESTIGATE)
- Identifies orphaned catalog entries
- Multiple output formats (text, JSON, markdown)
- Exit code indicates if action needed

**Usage:**
```bash
# Quick check
./scripts/audit-error-codes.py

# Detailed JSON for tooling
./scripts/audit-error-codes.py --format json > audit.json

# Markdown report
./scripts/audit-error-codes.py --format markdown -o report.md
```

**When to use manual process:** Only for debugging, customization, or understanding internals.

## Background

### Error Code Structure

Error codes follow the format: `Q-<subsystem>-<number>`

- `Q-0-*` - Internal errors (bugs in Quarto itself)
- `Q-1-*` - YAML validation errors
- `Q-2-*` - Markdown parsing errors
- `Q-3-*` - Writer/output errors

### Where Error Codes Live

**Single Source of Truth:**
- `crates/quarto-error-reporting/error_catalog.json` - Central catalog with metadata

**Usage Locations:**
1. **Rust source code** - Multiple patterns:
   - `.with_code("Q-X-Y")` - DiagnosticMessageBuilder pattern
   - `code: Some("Q-X-Y".to_string())` - Direct field assignment
   - `error_code: Some("Q-X-Y".to_string())` - qmd-syntax-helper pattern
   - `ValidationErrorKind::error_code()` - Method returning static string
   - String literals in error constructors

2. **Error corpus** - Test cases:
   - `crates/quarto-markdown-pandoc/resources/error-corpus/Q-*.json`
   - One file per error code with test cases

3. **Tests** - Explicit references:
   - `tests/*.rs` - Rust test files
   - Snapshot files in `snapshots/error-corpus/`

4. **Documentation** - User-facing docs:
   - `docs/` - Documentation site
   - Plan files in `claude-notes/plans/`
   - Investigation notes

5. **Private crates**:
   - `private-crates/quarto-yaml-validation/src/error.rs` - YAML error codes
   - `private-crates/validate-yaml/` - Validation examples

## Workflow Overview

**RECOMMENDED**: Use the Python script `scripts/audit-error-codes.py` which automates phases 2-3 and 6.

```
┌─────────────────────────────────┐
│ 1. Run audit script             │
│    ./scripts/audit-error-codes.py
└────────────┬────────────────────┘
             ↓
      [Automated phases:]
      │ 2. Find all code references
      │ 3. Classify matches
      │ 6. Generate report
             ↓
┌─────────────────────────────────┐
│ 4. Review report & decide       │
│    (Human judgment required)     │
└────────────┬────────────────────┘
             ↓
┌─────────────────────────────────┐
│ 5. Fix issues                   │
│    (Add/remove catalog entries)  │
└─────────────────────────────────┘
```

### Quick Start

```bash
# Text report to terminal
./scripts/audit-error-codes.py

# JSON output for tooling
./scripts/audit-error-codes.py --format json > audit.json

# Markdown report to file
./scripts/audit-error-codes.py --format markdown -o audit-report.md
```

### Manual Process (Advanced)

The sections below describe the manual process for auditing. This is useful for:
- Understanding how the automation works
- Customizing the audit for specific needs
- Debugging issues with the automated script

## Phase 1: Extract Catalog Codes

**Objective**: Build a set of all error codes defined in error_catalog.json

### Steps

1. **Parse error_catalog.json**
   ```bash
   cd /Users/cscheid/repos/github/cscheid/kyoto

   # Extract all error codes from catalog
   jq -r 'keys[]' crates/quarto-error-reporting/error_catalog.json | sort > /tmp/catalog-codes.txt

   # Count total
   wc -l /tmp/catalog-codes.txt
   ```

2. **Validate format**
   ```bash
   # Ensure all codes match Q-\d+-\d+ pattern
   grep -vE '^Q-[0-9]+-[0-9]+$' /tmp/catalog-codes.txt
   # Should produce no output
   ```

3. **Group by subsystem**
   ```bash
   # Q-0-* (internal)
   grep '^Q-0-' /tmp/catalog-codes.txt > /tmp/catalog-Q-0.txt

   # Q-1-* (yaml)
   grep '^Q-1-' /tmp/catalog-codes.txt > /tmp/catalog-Q-1.txt

   # Q-2-* (markdown)
   grep '^Q-2-' /tmp/catalog-codes.txt > /tmp/catalog-Q-2.txt

   # Q-3-* (writer)
   grep '^Q-3-' /tmp/catalog-codes.txt > /tmp/catalog-Q-3.txt
   ```

**Output:**
- `/tmp/catalog-codes.txt` - All codes in catalog (sorted)
- `/tmp/catalog-Q-*.txt` - Codes grouped by subsystem

## Phase 2: Find All Code References

**Objective**: Search the entire codebase for Q-*-* patterns

### Search Strategy

We need multiple searches to catch all usage patterns:

1. **Primary pattern**: `Q-\d+-\d+`
2. **Context patterns**: Specific to how codes are used

### Steps

1. **Comprehensive regex search**
   ```bash
   cd /Users/cscheid/repos/github/cscheid/kyoto

   # Search all Rust files in both crates and private-crates
   rg 'Q-\d+-\d+' \
     --type rust \
     --type json \
     --type markdown \
     --glob '!target/' \
     --glob '!external-sources/' \
     --glob '!external-sites/' \
     --json > /tmp/error-code-matches.jsonl
   ```

2. **Extract unique codes from matches**
   ```bash
   # Parse JSONL and extract all Q-*-* codes
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r '.data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | \
     sort -u > /tmp/source-codes.txt

   # Count
   wc -l /tmp/source-codes.txt
   ```

3. **Categorize by location**
   ```bash
   # Codes in crates/
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r 'select(.data.path.text | startswith("crates/")) | .data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/source-codes-crates.txt

   # Codes in private-crates/
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r 'select(.data.path.text | startswith("private-crates/")) | .data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/source-codes-private.txt

   # Codes in error corpus
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r 'select(.data.path.text | contains("/error-corpus/")) | .data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/source-codes-corpus.txt

   # Codes in tests
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r 'select(.data.path.text | contains("/tests/")) | .data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/source-codes-tests.txt

   # Codes in docs
   grep '"type":"match"' /tmp/error-code-matches.jsonl | \
     jq -r 'select(.data.path.text | startswith("docs/")) | .data.lines.text' | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/source-codes-docs.txt
   ```

**Output:**
- `/tmp/error-code-matches.jsonl` - Raw ripgrep results
- `/tmp/source-codes.txt` - All unique codes found in source
- `/tmp/source-codes-*.txt` - Codes by location

## Phase 3: Compare and Classify

**Objective**: Find discrepancies between catalog and source

### Steps

1. **Find codes missing from catalog**
   ```bash
   # Codes used in source but not in catalog
   comm -13 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/missing-from-catalog.txt

   echo "Codes used in source but NOT in catalog:"
   cat /tmp/missing-from-catalog.txt
   wc -l /tmp/missing-from-catalog.txt
   ```

2. **Find orphaned codes in catalog**
   ```bash
   # Codes in catalog but not used anywhere
   comm -23 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/orphaned-in-catalog.txt

   echo "Codes in catalog but NOT used in source:"
   cat /tmp/orphaned-in-catalog.txt
   wc -l /tmp/orphaned-in-catalog.txt
   ```

3. **Find codes in both (consistent)**
   ```bash
   # Codes properly defined and used
   comm -12 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/consistent-codes.txt

   echo "Codes with consistent catalog entries:"
   wc -l /tmp/consistent-codes.txt
   ```

**Output:**
- `/tmp/missing-from-catalog.txt` - Need to add to catalog
- `/tmp/orphaned-in-catalog.txt` - Possibly unused/deprecated
- `/tmp/consistent-codes.txt` - Properly tracked codes

## Phase 4: Detailed Analysis

**Objective**: For each discrepancy, provide context for resolution

### 4A. Analyze Missing Codes

For each code in `/tmp/missing-from-catalog.txt`:

1. **Find all occurrences**
   ```bash
   # Example: Q-3-38
   code="Q-3-38"

   # Find all usages with context
   rg "$code" \
     --type rust \
     --type json \
     --glob '!target/' \
     -A 3 -B 3 > "/tmp/analysis-$code.txt"

   # Count occurrences
   rg "$code" --type rust --type json --glob '!target/' --count-matches
   ```

2. **Determine subsystem**
   ```bash
   # Extract subsystem number (first digit after Q-)
   subsystem=$(echo "$code" | grep -oE 'Q-([0-9]+)-' | grep -oE '[0-9]+')

   case $subsystem in
     0) echo "Subsystem: internal" ;;
     1) echo "Subsystem: yaml" ;;
     2) echo "Subsystem: markdown" ;;
     3) echo "Subsystem: writer" ;;
     *) echo "Subsystem: UNKNOWN" ;;
   esac
   ```

3. **Examine context**
   - Is this a legitimate error code that needs catalog entry?
   - Is this a typo or placeholder?
   - Is this code in comments/docs only?

4. **Recommend action**
   - Add to catalog with proper metadata
   - Fix typo in source
   - Remove if deprecated

### 4B. Analyze Orphaned Codes

For each code in `/tmp/orphaned-in-catalog.txt`:

1. **Check if recently removed**
   ```bash
   code="Q-2-99"

   # Search git history
   git log --all --oneline -S "$code" -- '*.rs' '*.json'

   # When was it removed?
   git log --all --oneline -- crates/quarto-error-reporting/error_catalog.json | \
     grep -A5 -B5 "$code"
   ```

2. **Check if renamed**
   ```bash
   # Look for similar codes that might be replacements
   # E.g., Q-2-99 might have become Q-2-100
   ```

3. **Recommend action**
   - Remove from catalog if truly unused
   - Keep if planned for future use (document in notes)
   - Investigate if recently removed (might be a regression)

## Phase 5: Validation Checks

**Objective**: Catch additional consistency issues

### Checks to Perform

1. **Subsystem consistency**
   ```bash
   # For each code, verify subsystem matches catalog metadata
   for code in $(cat /tmp/consistent-codes.txt); do
     subsystem=$(echo "$code" | grep -oE 'Q-([0-9]+)-' | sed 's/Q-//' | sed 's/-//')
     catalog_subsystem=$(jq -r ".\"$code\".subsystem" crates/quarto-error-reporting/error_catalog.json)

     # Map subsystem number to name
     case $subsystem in
       0) expected="internal" ;;
       1) expected="yaml" ;;
       2) expected="markdown" ;;
       3) expected="writer" ;;
     esac

     if [ "$catalog_subsystem" != "$expected" ]; then
       echo "MISMATCH: $code - catalog says '$catalog_subsystem', expected '$expected'"
     fi
   done
   ```

2. **Error corpus coverage**
   ```bash
   # Every Q-2-* markdown error should have error corpus file
   for code in $(grep '^Q-2-' /tmp/catalog-codes.txt); do
     corpus_file="crates/quarto-markdown-pandoc/resources/error-corpus/$code.json"
     if [ ! -f "$corpus_file" ]; then
       echo "MISSING CORPUS: $code (no $corpus_file)"
     fi
   done
   ```

3. **Duplicate detection**
   ```bash
   # Check for potential typos or duplicates
   # E.g., Q-1-10 vs Q-1-010
   for code in $(cat /tmp/source-codes.txt); do
     # Normalize: remove leading zeros
     normalized=$(echo "$code" | sed -E 's/Q-0*([0-9]+)-0*([0-9]+)/Q-\1-\2/')
     if [ "$code" != "$normalized" ]; then
       echo "FORMATTING: $code should be $normalized"
     fi
   done
   ```

4. **YAML validation codes**
   ```bash
   # All Q-1-* codes should have corresponding ValidationErrorKind
   yaml_codes=$(grep '^Q-1-' /tmp/catalog-codes.txt)

   # Extract codes from ValidationErrorKind::error_code() method
   grep -A1 'ValidationErrorKind::.*=>' private-crates/quarto-yaml-validation/src/error.rs | \
     grep -oE 'Q-[0-9]+-[0-9]+' | sort -u > /tmp/validation-error-kinds.txt

   # Compare
   echo "Q-1 codes in catalog:"
   wc -l /tmp/catalog-Q-1.txt
   echo "Q-1 codes in ValidationErrorKind:"
   wc -l /tmp/validation-error-kinds.txt

   comm -13 /tmp/validation-error-kinds.txt /tmp/catalog-Q-1.txt
   ```

## Phase 6: Generate Audit Report

**Objective**: Create a comprehensive report with actionable items

### Report Structure

```markdown
# Error Code Audit Report
Generated: YYYY-MM-DD

## Summary Statistics

- Total codes in catalog: X
- Total codes in source: Y
- Consistent codes: Z
- Missing from catalog: A
- Orphaned in catalog: B

## Issues Requiring Action

### 1. Missing from Catalog (PRIORITY HIGH)

These codes are used in source but have no catalog entry:

| Code | Occurrences | Files | Subsystem | Action Required |
|------|-------------|-------|-----------|-----------------|
| Q-3-38 | 3 | json.rs | writer | Add catalog entry |
| ... | ... | ... | ... | ... |

### 2. Orphaned in Catalog (PRIORITY MEDIUM)

These codes are in catalog but never used:

| Code | Subsystem | Last Seen | Action Required |
|------|-----------|-----------|-----------------|
| Q-2-99 | markdown | Never | Remove or document |
| ... | ... | ... | ... |

### 3. Consistency Issues (PRIORITY HIGH)

Subsystem mismatches and format issues:

| Code | Issue | Fix Required |
|------|-------|--------------|
| Q-1-10 | Subsystem mismatch | Update catalog |
| ... | ... | ... |

### 4. Missing Error Corpus (PRIORITY MEDIUM)

Q-2-* codes without test cases:

| Code | Status |
|------|--------|
| Q-2-35 | No corpus file |
| ... | ... |

## Detailed Findings

[For each issue, provide context from source code]

### Code: Q-3-38

**Status:** Missing from catalog
**Subsystem:** writer (Q-3)
**Occurrences:** 3

**Usage locations:**
1. `crates/quarto-markdown-pandoc/src/writers/json.rs:1295`
   ```rust
   code: Some("Q-3-38".to_string()),
   ```

**Recommended action:**
Add entry to error_catalog.json with:
- subsystem: "writer"
- title: [TBD - review context]
- message_template: [TBD - review context]
- docs_url: "https://quarto.org/docs/errors/Q-3-38"
- since_version: "99.9.9"

---

[Repeat for each issue]
```

### Generate the Report

```bash
# Create report file
report_file="claude-notes/investigations/$(date +%Y-%m-%d)-error-code-audit-report.md"

# Build report (pseudo-code)
cat > "$report_file" <<EOF
# Error Code Audit Report
Generated: $(date +%Y-%m-%d)

## Summary Statistics

- Total codes in catalog: $(wc -l < /tmp/catalog-codes.txt)
- Total codes in source: $(wc -l < /tmp/source-codes.txt)
- Consistent codes: $(wc -l < /tmp/consistent-codes.txt)
- Missing from catalog: $(wc -l < /tmp/missing-from-catalog.txt)
- Orphaned in catalog: $(wc -l < /tmp/orphaned-in-catalog.txt)

## Issues Requiring Action
[... continue building report ...]
EOF
```

## Phase 7: Fix Issues

**Objective**: Systematically resolve each discrepancy

### Priority Order

1. **Missing codes (HIGH)** - Add to catalog immediately
2. **Consistency issues (HIGH)** - Fix mismatches
3. **Missing corpus (MEDIUM)** - Add test cases
4. **Orphaned codes (MEDIUM)** - Remove or document

### Fix Process

For **missing codes**:

1. **Gather information**
   ```bash
   code="Q-3-38"
   rg "$code" -A5 -B5 --type rust
   ```

2. **Create catalog entry**
   ```bash
   # Edit error_catalog.json
   # Add entry with appropriate metadata
   ```

3. **Verify fix**
   ```bash
   jq ".\"$code\"" crates/quarto-error-reporting/error_catalog.json
   ```

4. **Commit**
   ```bash
   git add crates/quarto-error-reporting/error_catalog.json
   git commit -m "Add error catalog entry for $code"
   ```

For **orphaned codes**:

1. **Verify truly unused**
   ```bash
   code="Q-2-99"
   git log --all -S "$code" --oneline
   ```

2. **Document decision**
   - If deprecated: Remove from catalog
   - If future use: Add comment in catalog or notes
   - If regression: Investigate why usage was removed

3. **Execute**
   ```bash
   # If removing:
   jq "del(.\"$code\")" crates/quarto-error-reporting/error_catalog.json > tmp.json
   mv tmp.json crates/quarto-error-reporting/error_catalog.json

   git add crates/quarto-error-reporting/error_catalog.json
   git commit -m "Remove unused error code $code from catalog"
   ```

## Automation Scripts

### Quick Audit Script

```bash
#!/bin/bash
# quick-error-audit.sh
# Quick audit of error code consistency

set -e

cd /Users/cscheid/repos/github/cscheid/kyoto

echo "=== Error Code Audit ==="
echo

# Extract catalog codes
echo "Extracting catalog codes..."
jq -r 'keys[]' crates/quarto-error-reporting/error_catalog.json | sort > /tmp/catalog-codes.txt

# Search source codes
echo "Searching source code..."
rg 'Q-\d+-\d+' \
  --type rust --type json \
  --glob '!target/' --glob '!external-*/' \
  --no-filename --no-line-number --only-matching | \
  sort -u > /tmp/source-codes.txt

# Compare
echo
echo "Summary:"
echo "  Catalog codes: $(wc -l < /tmp/catalog-codes.txt)"
echo "  Source codes:  $(wc -l < /tmp/source-codes.txt)"

# Missing from catalog
comm -13 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/missing.txt
if [ -s /tmp/missing.txt ]; then
  echo
  echo "❌ MISSING FROM CATALOG ($(wc -l < /tmp/missing.txt)):"
  cat /tmp/missing.txt | sed 's/^/  - /'
fi

# Orphaned in catalog
comm -23 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/orphaned.txt
if [ -s /tmp/orphaned.txt ]; then
  echo
  echo "⚠️  ORPHANED IN CATALOG ($(wc -l < /tmp/orphaned.txt)):"
  cat /tmp/orphaned.txt | sed 's/^/  - /'
fi

# Consistent
comm -12 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/consistent.txt
echo
echo "✅ CONSISTENT ($(wc -l < /tmp/consistent.txt)):"
echo "  All good!"

echo
echo "Temporary files in /tmp/: catalog-codes.txt source-codes.txt missing.txt orphaned.txt consistent.txt"
```

Make executable and run:
```bash
chmod +x quick-error-audit.sh
./quick-error-audit.sh
```

## Integration with Beads

When issues are found, create beads issues:

```bash
# For missing catalog entries
br create "Add error catalog entry for Q-3-38" \
  -t task -p 1 \
  -d "Error code Q-3-38 is used in source but missing from error_catalog.json

      Usage locations:
      - crates/quarto-markdown-pandoc/src/writers/json.rs:1295

      Need to determine:
      - Appropriate title
      - Message template
      - Documentation

      See: claude-notes/investigations/YYYY-MM-DD-error-code-audit-report.md" \
  --json

# For orphaned codes
br create "Investigate orphaned error code Q-2-99" \
  -t task -p 2 \
  -d "Error code Q-2-99 exists in catalog but is not referenced in source.

      Determine if:
      - Code was removed and catalog should be updated
      - Code is planned for future use (document)
      - Code is used in external tools/tests

      See: claude-notes/investigations/YYYY-MM-DD-error-code-audit-report.md" \
  --json
```

## Maintenance Schedule

Run this audit:

1. **Before releases** - Ensure catalog is complete
2. **Monthly** - Catch drift early
3. **After major refactoring** - Verify no codes were lost
4. **When adding new subsystems** - Ensure proper cataloging

## Common Issues and Solutions

### Issue: Code in Comments Only

**Problem:** Error code appears in comments/docs but not in executable code

**Solution:** Decide if:
- Keep in catalog (referenced for documentation)
- Remove from catalog (outdated reference)
- Implement the error (if planned)

### Issue: Typos

**Problem:** Similar codes like Q-1-10 and Q-1-010

**Solution:**
1. Standardize format (no leading zeros)
2. Fix in source code
3. Update references
4. Consider pre-commit hook to enforce format

### Issue: Renamed Codes

**Problem:** Code changed from Q-2-5 to Q-2-50 but some refs still use old

**Solution:**
1. Add git alias for old code
2. Update all references
3. Consider migration guide in docs

### Issue: ValidationErrorKind Mismatch

**Problem:** Q-1-* code in catalog but no corresponding ValidationErrorKind

**Solution:**
1. Add variant to ValidationErrorKind enum
2. Implement error_code() method case
3. Add to validation logic
4. Add test coverage

## Notes

- **Prefer explicit over implicit**: Every error code used should have catalog entry
- **Document unknowns**: If you can't determine if code is used, document the uncertainty
- **Version tracking**: Note when codes are added/removed in catalog metadata
- **Breaking changes**: Removing codes from catalog is potentially breaking for users who reference docs
