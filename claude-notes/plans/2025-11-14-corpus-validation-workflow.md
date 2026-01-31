# Corpus Validation Workflow

Date: 2025-11-14
File: claude-notes/plans/2025-11-14-corpus-validation-workflow.md

## Purpose

This workflow enables systematic validation and correction of large document corpora against the qmd grammar. It distinguishes between:
1. **Document errors** (typos, disallowed syntax) - can be auto-fixed
2. **Grammar bugs** - require investigation and grammar fixes
3. **New error types** - require strategy development

**Key Principle**: Only trust the FIRST error in each parse. Error recovery may produce misleading subsequent errors.

## Strategy: Error Code-Based Classification

The `quarto-error-reporting` crate assigns specific error codes to known error types. This provides our classification mechanism:

- **Error HAS code** → Known document problem → Fix the document
- **Error has NO code** → Unknown problem → Requires review:
  - Could be a grammar/parser bug
  - Could be a new error type needing a code and fix strategy

## Workflow Overview

```
┌─────────────────────────────────────┐
│ 1. Setup: Clone repo, create branch │
└──────────────┬──────────────────────┘
               ↓
┌─────────────────────────────────────┐
│ 2. Scan all .qmd files              │
└──────────────┬──────────────────────┘
               ↓
┌─────────────────────────────────────┐
│ 3. For each file: Parse & classify  │
│    FIRST error only                 │
└──────────────┬──────────────────────┘
               ↓
      ┌────────┴─────────┐
      ↓                  ↓
┌──────────┐      ┌─────────────┐
│Has code? │      │ No code?    │
│→ Fix doc │      │→ Review     │
└────┬─────┘      └──────┬──────┘
     │                   │
     └─────→ Loop ←──────┘
            (re-parse until clean
             or needs review)
```

## Phase 1: Repository Setup

**Objective**: Create a clean working environment for corpus validation

### Steps:

1. **Clone the external repository** (if not already present)
   ```bash
   # In external-sites/ directory
   git clone https://github.com/org/repo repo-name
   cd repo-name
   ```

2. **Create tracking branch**
   ```bash
   git checkout -b quarto-markdown-syntax
   ```

3. **Record baseline**
   - Document the commit hash: `git rev-parse HEAD`
   - Note in beads issue or investigation notes

**Why separate branch?**
- Non-destructive: Original source preserved
- Trackable: All fixes in version control
- Reviewable: Can diff to see all changes
- Reversible: Can abandon if needed

## Phase 2: Corpus Scanning

**Objective**: Identify all files and create processing queue

### Steps:

1. **Find all .qmd files**
   ```bash
   find . -name "*.qmd" -type f > /tmp/corpus-files.txt
   wc -l /tmp/corpus-files.txt  # Count total
   ```

2. **Run initial parse on all files**
   ```bash
   # Create results directory
   mkdir -p .corpus-validation

   # Process each file, capturing first error only
   while read file; do
     echo "=== $file ===" >> .corpus-validation/errors.log
     cargo run --bin quarto-markdown-pandoc -- -i "$file" \
       --json-errors 2>&1 | head -1 >> .corpus-validation/errors.log
   done < /tmp/corpus-files.txt
   ```

3. **Classify files by status**
   ```bash
   # Files that parse cleanly
   # Files with coded errors (fixable)
   # Files with uncoded errors (need review)
   ```

**Output**:
- `.corpus-validation/errors.log` - All first errors
- Three lists: clean, coded-errors, uncoded-errors

## Phase 3: Error Processing Loop

**Objective**: Systematically fix or triage each file

### The Core Loop

For each file in the processing queue:

1. **Parse and capture FIRST error**
   ```bash
   cargo run --bin quarto-markdown-pandoc -- -i "$file" --json-errors
   ```

2. **Examine error structure**
   - Is there an error code field?
   - What is the error message?
   - Where is the error location?

3. **Decision Tree**

```
Does error have a code?
│
├─ YES → Is the fix strategy known?
│        │
│        ├─ YES → Apply fix, commit, re-parse (loop)
│        │
│        └─ NO → Stop, request review:
│                 "Error code XYZ exists but no fix strategy"
│
└─ NO → Stop, request review:
         Create minimal reproduction
         Report to user with context
```

### 3A. Coded Errors (Auto-fixable)

**When**: Error has a code field

**Process**:

1. **Identify error code and type**
   ```json
   {
     "code": "E0042",
     "message": "Invalid div fence: missing closing :::",
     "location": {...}
   }
   ```

2. **Apply known fix strategy**
   - Consult error code documentation
   - Apply mechanical fix to file
   - Examples:
     - `E0042`: Add closing `:::`
     - `E0103`: Fix malformed link syntax
     - `E0205`: Correct YAML indentation

3. **Commit the fix**
   ```bash
   git add "$file"
   git commit -m "Fix E0042: Add missing closing fence

   File: $file
   Error: [error message]"
   ```

4. **Re-parse and loop**
   - Run parser again
   - Get next FIRST error
   - Repeat until file is clean or uncoded error appears

**Important**:
- Only fix ONE error at a time (the first one)
- Always re-parse after each fix
- Each fix gets its own commit for traceability

### 3B. Uncoded Errors (Requires Review)

**When**: Error has no code field OR fix strategy unknown

**Process**:

1. **Report the error with context**
   - Show the file path
   - Show the error message
   - Show the surrounding lines of the file (context around error location)
   - Use the parser with `-i` flag to get human-readable output

2. **Request user review**
   - Stop processing this file
   - Present error with file context
   - Wait for user decision:
     - Grammar bug → Create beads issue, fix grammar
     - New error type → Add error code, document fix strategy
     - Document error → Fix the file directly
     - Expected failure → Document and skip

3. **Record decision**
   ```bash
   # Add file to appropriate list based on user decision
   echo "$file" >> .corpus-validation/needs-grammar-fix.txt
   # or
   echo "$file" >> .corpus-validation/needs-error-code.txt
   # or
   echo "$file" >> .corpus-validation/expected-failures.txt
   ```

## Phase 4: Progress Tracking

**Objective**: Maintain visibility into validation progress

### Tracking Mechanisms

1. **File status lists** (in `.corpus-validation/`)
   - `clean.txt` - Files that parse successfully
   - `coded-errors-fixed.txt` - Files with coded errors that were fixed
   - `needs-review.txt` - Files awaiting review
   - `needs-grammar-fix.txt` - Files blocked on grammar bugs
   - `expected-failures.txt` - Invalid files that won't be fixed

2. **Progress metrics**
   ```bash
   # Count by status
   wc -l .corpus-validation/*.txt

   # Success rate
   # (clean + fixed) / total
   ```

3. **Commit history**
   ```bash
   git log --oneline --grep="Fix E"
   # Shows all error fixes
   ```

4. **Beads issues**
   - Create issue for each grammar bug discovered
   - Create issue for each new error code needed
   - Link back to triggering file

### Example Tracking Session

```bash
# Start of session
Total files: 150
Clean: 50
Coded errors: 75
Uncoded errors: 25

# After fixing coded errors
Total files: 150
Clean: 120 (+70 fixed)
Coded errors: 5 (new errors revealed)
Uncoded errors: 25

# After reviewing uncoded
Grammar bugs identified: 3 (beads issues created)
New error codes needed: 2 (beads issues created)
Expected failures: 20 (documented)
```

## Phase 5: Batch Processing Strategy

**Objective**: Process multiple files efficiently while maintaining judgment

### Batch Processing Rules

1. **Process in sorted order** (alphabetical or by directory)
   - Reproducible
   - Easier to resume

2. **Batch same error codes together**
   - If 10 files have error `E0042`, fix all 10 in sequence
   - Single commit with all fixes
   - More efficient than one-by-one

3. **Stop on first uncoded error**
   - Don't accumulate review queue
   - Get user feedback before continuing
   - Prevents going down wrong path

4. **Checkpoint regularly**
   ```bash
   git push origin quarto-markdown-syntax
   # Every 20-30 fixes or end of session
   ```

### Parallel Processing (Optional)

If corpus is very large (>500 files):

1. **Split by directory**
   ```bash
   # Process docs/ separately from blog/
   ```

2. **Process clean files first**
   - Quick wins
   - Establishes baseline
   - Remaining files are "hard cases"

3. **Group by error code**
   - All E0042 together
   - All E0103 together
   - Efficient context switching

## Quick Reference

### Essential Commands

```bash
# Parse file and show first error
cargo run --bin quarto-markdown-pandoc -- -i <file> --json-errors 2>&1 | head -20

# Parse with detailed tree (for debugging)
cargo run --bin quarto-markdown-pandoc -- -i <file> -v 2>&1 | tail -100

# Check if file parses cleanly (exit code)
cargo run --bin quarto-markdown-pandoc -- -i <file> --json-errors 2>&1 && echo "CLEAN"

# Batch check multiple files
for f in $(cat files.txt); do
  echo "=== $f ==="
  cargo run --bin quarto-markdown-pandoc -- -i "$f" --json-errors 2>&1 | head -5
done
```

### Decision Shortcuts

```
Error has code starting with "E"?
  YES → Check fix strategy
    Known → Fix it
    Unknown → Review
  NO → Create minimal reproduction → Review
```

### When to Stop and Ask

- **Always stop when**:
  - Error has no code
  - Error has code but you don't know fix strategy
  - Minimal reproduction doesn't make sense
  - Fix strategy would be complex (>10 lines changed)
  - Same error code appears >50 times (might need tooling)

- **Can continue when**:
  - Error code recognized
  - Fix is mechanical (e.g., add closing fence, fix indent)
  - High confidence in fix correctness

## Integration with Beads

### Issue Types for Corpus Validation

1. **Grammar bugs** (discovered during validation)
   ```bash
   br create "Fix grammar: [construct] not parsing" \
     -t bug -p 1 \
     -d "Found in: <file>
         Minimal repro: <example>
         See: claude-notes/investigations/..." \
     --json
   ```

2. **New error codes needed**
   ```bash
   br create "Add error code for [construct]" \
     -t task -p 1 \
     -d "Pattern: <description>
         Example files: <list>
         Proposed fix: <strategy>" \
     --json
   ```

3. **Conversion rules for qmd-syntax-helper**
   ```bash
   br create "Add conversion rule: [pattern]" \
     -t feature -p 2 \
     -d "Error code: E0XXX
         Auto-fix: <strategy>
         Frequency: XX files affected" \
     --json
   ```

## Example Session

```bash
# Setup
cd external-sites/quarto-web
git checkout -b quarto-markdown-syntax
mkdir -p .corpus-validation

# Scan
find docs -name "*.qmd" > .corpus-validation/all-files.txt
echo "Total files: $(wc -l < .corpus-validation/all-files.txt)"

# Process first file
file=$(head -1 .corpus-validation/all-files.txt)
cargo run --bin quarto-markdown-pandoc -- -i "$file" --json-errors

# Result: Error E0042 - missing closing fence
# Fix: Add ::: at end
vim "$file"  # (or Edit tool)
git add "$file"
git commit -m "Fix E0042: Add missing closing fence"

# Re-parse
cargo run --bin quarto-markdown-pandoc -- -i "$file" --json-errors
# Result: CLEAN

# Record progress
echo "$file" >> .corpus-validation/clean.txt

# Next file...
```

## Notes

- **Preserve original formatting** when making fixes (don't reformat entire files)
- **Commit message consistency** helps tracking (always include error code)
- **Document unknowns** in `claude-notes/investigations/` for future reference
- **Error codes** are the single source of truth for "is this fixable?"
