# Error Code Audit: Summary and Next Steps

<!-- quarto-error-code-audit-ignore-file -->

Date: 2025-11-23
Status: Workflow Complete, Ready for Iteration

## What Was Created

### 1. Python Audit Script ⭐
**File:** `scripts/audit-error-codes.py`

A comprehensive, automated tool that replaces the manual phases 2-3 of the workflow:

**Capabilities:**
- ✅ Loads error catalog from JSON
- ✅ Searches entire codebase with ripgrep
- ✅ Automatically categorizes missing codes:
  - Legitimate (production code) - HIGH PRIORITY
  - Test/Example (docs/tests) - LOW PRIORITY
  - Invalid format (typos/sentinels) - INVESTIGATE
- ✅ Identifies orphaned catalog entries
- ✅ Subsystem breakdown
- ✅ Multiple output formats: text, JSON, markdown
- ✅ Exit codes for CI/CD integration

**Why This Is Better:**
- **Automatic categorization** - Distinguishes real issues from test data
- **Structured output** - JSON for tooling, markdown for reports
- **Maintainable** - Python is easier to extend than bash pipelines
- **Testable** - Can add unit tests for categorization logic
- **Portable** - Works anywhere Python + ripgrep are available

### 2. Workflow Documentation
**File:** `claude-notes/workflows/2025-11-23-error-code-audit-workflow.md`

Complete workflow with:
- Quick start using Python script
- Manual process documentation (for reference/debugging)
- Integration with beads
- Maintenance schedule recommendations

### 3. Audit Results
**File:** `claude-notes/investigations/2025-11-23-error-code-audit-results.md`

Initial audit findings from running the script on your codebase.

### 4. Quick Fix Guide
**File:** `claude-notes/investigations/2025-11-23-add-missing-catalog-entries.md`

Step-by-step instructions for adding the 5 most critical missing codes.

### 5. Bash Script (Legacy)
**File:** `scripts/quick-error-audit.sh`

Simple bash version for quick checks. Kept for compatibility, but Python script is recommended.

### 6. Documentation
**File:** `scripts/README.md`

Documentation for all scripts in the scripts/ directory.

## Usage Examples

### Basic Audit
```bash
# Run audit and see results
./scripts/audit-error-codes.py

# Exit code 0 = no issues, 1 = issues found
echo $?
```

### For CI/CD
```bash
# Generate JSON report
./scripts/audit-error-codes.py --format json > audit.json

# Check exit code
if [ $? -ne 0 ]; then
  echo "Error codes out of sync with catalog!"
  exit 1
fi
```

### Generate Reports
```bash
# Markdown report for documentation
./scripts/audit-error-codes.py --format markdown \
  -o docs/error-code-audit.md

# Text report for review
./scripts/audit-error-codes.py > audit-$(date +%Y-%m-%d).txt
```

### Extract Specific Data
```bash
# Get just the summary stats
./scripts/audit-error-codes.py --format json | jq '.summary'

# List all legitimate missing codes
./scripts/audit-error-codes.py --format json | \
  jq -r '.legitimate_missing | keys[]'

# Get locations for a specific code
./scripts/audit-error-codes.py --format json | \
  jq '.legitimate_missing."Q-1-90".locations'
```

## Current State

Based on the initial audit:

**Statistics:**
- 67 codes in catalog ✅
- 119 codes in source
- 9 legitimate missing codes ❌ HIGH PRIORITY
- 42 test/example codes ℹ️ LOW PRIORITY
- 1 invalid format code ⚠️ INVESTIGATE
- 0 orphaned codes ✅ PERFECT

**High Priority Actions:**
1. Add Q-1-90, Q-1-91, Q-1-92, Q-1-93 (YAML validation)
2. Add Q-3-38 (JSON serialization)
3. Decide on Q-0-99 <!-- quarto-error-code-audit-ignore --> (migration aid)
4. Review Q-1-1, Q-1-2, Q-1-100 (used in multiple places)

## Comparison: Python vs Bash

| Feature | Python Script | Bash Script |
|---------|--------------|-------------|
| **Categorization** | ✅ Automatic (legitimate/test/invalid) | ❌ Lists all missing |
| **Output formats** | ✅ Text, JSON, Markdown | ⚠️ Text only |
| **Location details** | ✅ File, line, context | ⚠️ First occurrence only |
| **Subsystem breakdown** | ✅ With gap analysis | ⚠️ Basic counts |
| **Extensibility** | ✅ Easy to add features | ❌ Complex bash |
| **CI/CD integration** | ✅ JSON + exit codes | ⚠️ Exit codes only |
| **Dependencies** | Python 3.6+, ripgrep | bash, jq, ripgrep |

## Iterating on the Workflow

The Python script makes it easy to iterate:

### Add New Categorization Rules
Edit `_is_test_or_example()` or `_has_format_issues()`:

```python
def _is_planned_code(self, usage: CodeUsage) -> bool:
    """Check if code is documented as planned for future use."""
    for loc in usage.locations:
        if 'future-codes.md' in loc.file:
            return True
    return False
```

### Add New Output Formats
Add to `ReportFormatter`:

```python
@staticmethod
def format_csv(results: AuditResults) -> str:
    """Format as CSV for spreadsheet import."""
    # Implementation
```

### Add Validation Checks
Extend the audit to check:
- Error corpus coverage (Q-2-* codes should have test files)
- Subsystem consistency (Q-1-* should have subsystem="yaml")
- Code number gaps (Q-1-10, Q-1-11, Q-1-13 - missing 12?)

### Integration Ideas

1. **Pre-commit hook**
   ```bash
   # .git/hooks/pre-commit
   ./scripts/audit-error-codes.py --format json > /tmp/audit.json
   if [ $? -ne 0 ]; then
     echo "New error codes detected without catalog entries"
     # Could auto-create issue or just warn
   fi
   ```

2. **Monthly report**
   ```bash
   # cron job or GitHub Action
   ./scripts/audit-error-codes.py --format markdown \
     -o reports/audit-$(date +%Y-%m).md
   ```

3. **Documentation generation**
   ```bash
   # Extract catalog entries and generate docs
   jq -r 'to_entries[] | "## \(.key)\n\n\(.value.title)\n\n\(.value.message_template)\n"' \
     crates/quarto-error-reporting/error_catalog.json > docs/error-codes.md
   ```

## Next Steps

### Immediate (Today)
1. Review the Python script output
2. Decide if categorization logic is correct
3. Run with different output formats to verify

### Short-term (This Week)
1. Add the 5 critical missing codes to catalog
2. Update example codes in documentation
3. Investigate invalid format codes

### Long-term (Ongoing)
1. Run monthly and track trends
2. Add to CI/CD pipeline
3. Consider pre-commit hook
4. Extend with additional validation checks

## Files to Track

All audit-related files:

```
scripts/
  ├── audit-error-codes.py       # Main tool ⭐
  ├── quick-error-audit.sh        # Legacy bash version
  └── README.md                   # Documentation

claude-notes/
  ├── workflows/
  │   └── 2025-11-23-error-code-audit-workflow.md  # Process doc
  └── investigations/
      ├── 2025-11-23-error-code-audit-results.md   # Initial findings
      ├── 2025-11-23-add-missing-catalog-entries.md # Fix guide
      └── 2025-11-23-error-code-audit-summary.md   # This file
```

## Questions for Iteration

As you use the script, consider:

1. **Categorization accuracy:**
   - Are legitimate codes correctly identified?
   - Should any test codes be moved to legitimate?
   - Are there other categories needed?

2. **Output format:**
   - Is the text format readable?
   - Does JSON include everything needed for tooling?
   - Should markdown format be different?

3. **Additional checks:**
   - Subsystem consistency validation?
   - Error corpus coverage for Q-2-* codes?
   - Code number gap detection?
   - Documentation URL validation?

4. **Integration:**
   - Should this run in CI/CD?
   - Pre-commit hook?
   - Automated issue creation?

## Success Criteria

The workflow is successful when:

1. ✅ Script runs without errors
2. ✅ Categorization is accurate
3. ✅ Output formats are useful
4. ⏳ All legitimate missing codes are added to catalog
5. ⏳ Test/example codes are documented or updated
6. ⏳ No orphaned codes exist
7. ⏳ Process is documented and repeatable

## Feedback Loop

After using this workflow:
1. Note what worked well
2. Note what was confusing or manual
3. Update scripts to automate pain points
4. Update documentation with learnings
5. Repeat

The Python script is designed to be the foundation for iteration - start with it and evolve based on what you discover.
