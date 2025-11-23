# Error Code Audit Results

<!-- quarto-error-code-audit-ignore-file -->

Date: 2025-11-23
Status: Initial Audit Complete

## Executive Summary

Ran initial audit of error code consistency between `error_catalog.json` and source code.

**Key Findings:**
- ✅ **67 codes** properly cataloged and used
- ❌ **52 codes** used in source but missing from catalog
- ✅ **0 codes** orphaned (all catalog entries are used)

## Statistics

| Category | Count | Status |
|----------|-------|--------|
| Codes in catalog | 67 | ✅ |
| Codes in source | 119 | ⚠️ |
| Consistent | 67 | ✅ |
| Missing from catalog | 52 | ❌ |
| Orphaned in catalog | 0 | ✅ |

### By Subsystem

| Subsystem | Catalog | Source | Gap |
|-----------|---------|--------|-----|
| Q-0-* (Internal) | 1 | 3 | +2 |
| Q-1-* (YAML) | 12 | 32 | +20 |
| Q-2-* (Markdown) | 34 | 44 | +10 |
| Q-3-* (Writer) | 20 | 24 | +4 |

## Analysis of Missing Codes

### Category 1: Legitimate Missing Codes (HIGH PRIORITY)

These are real error codes used in production code:

| Code | Location | Purpose | Action |
|------|----------|---------|--------|
| Q-0-99 <!-- quarto-error-code-audit-ignore --> | `quarto-error-reporting/src/builder.rs` | Generic error for migration | Decide: Add to catalog or phase out |
| Q-3-38 | `quarto-markdown-pandoc/src/writers/json.rs` | JSON serialization failed | **Add to catalog** |
| Q-1-90 | `private-crates/quarto-yaml-validation/src/error.rs` | YAML: SchemaFalse | **Add to catalog** |
| Q-1-91 | `private-crates/quarto-yaml-validation/src/error.rs` | YAML: AllOfFailed | **Add to catalog** |
| Q-1-92 | `private-crates/quarto-yaml-validation/src/error.rs` | YAML: AnyOfFailed | **Add to catalog** |
| Q-1-93 | `private-crates/quarto-yaml-validation/src/error.rs` | YAML: OneOfFailed | **Add to catalog** |

**Action Required:** These MUST be added to error_catalog.json

### Category 2: Test/Example Codes (LOW PRIORITY)

These appear in tests, documentation, and design notes:

| Code | Context | Action |
|------|---------|--------|
| Q-999-999 | Test data for invalid code handling | Keep as-is (intentionally invalid) <!-- quarto-error-code-audit-ignore --> |
| Q-1-1, Q-1-2, Q-1-3, Q-1-4 | Documentation examples | Update docs to use real codes |
| Q-4-*, Q-5-*, Q-6-*, Q-7-*, Q-8-* | Design documents (future subsystems?) | Document intent or remove |

**Action Required:** Update examples in documentation to reference actual codes

### Category 3: Possible Typos/Formatting Issues (MEDIUM PRIORITY)

These may be formatting inconsistencies:

| Code | Issue | Possible Fix |
|------|-------|-------------|
| Q-1-010 | Leading zero? | Check if should be Q-1-10 |
| Q-1-10000, Q-1-9999 | Very high numbers | Verify intent (test data?) |

**Action Required:** Investigate and standardize

## Detailed Missing Code List

### Q-0-* (Internal)
- Q-0-2 - ?
- Q-0-99 <!-- quarto-error-code-audit-ignore --> - Generic error (migration aid)

### Q-1-* (YAML)
- Q-1-1, Q-1-2, Q-1-3, Q-1-4 - Documentation examples
- Q-1-21, Q-1-22, Q-1-23, Q-1-24, Q-1-25 - ?
- Q-1-30 - ?
- Q-1-50 - ?
- Q-1-90 - **ValidationErrorKind::SchemaFalse** ⚠️
- Q-1-91 - **ValidationErrorKind::AllOfFailed** ⚠️
- Q-1-92 - **ValidationErrorKind::AnyOfFailed** ⚠️
- Q-1-93 - **ValidationErrorKind::OneOfFailed** ⚠️
- Q-1-010, Q-1-100, Q-1-101 - Formatting issues?
- Q-1-9999, Q-1-10000 - Test data?

### Q-2-* (Markdown)
- Q-2-35, Q-2-39, Q-2-40 - ?
- Q-2-49, Q-2-50 - ?
- Q-2-99 - ?
- Q-2-100 - ?
- Q-2-301, Q-2-450, Q-2-500 - Very high numbers (test data?)

### Q-3-* (Writer)
- Q-3-38 - **JSON serialization error** ⚠️
- Q-3-48 - ?
- Q-3-405, Q-3-701 - ?

### Q-4+ (Unknown Subsystems)
These subsystems don't exist yet (Q-4 through Q-8):
- Q-4-1, Q-4-102, Q-4-550
- Q-5-1, Q-5-201, Q-5-403
- Q-6-1, Q-6-234, Q-6-501
- Q-7-1, Q-7-301, Q-7-502
- Q-8-1, Q-8-234, Q-8-501

**These likely appear in design documents discussing future subsystems.**

## Recommendations

### Immediate Actions (HIGH PRIORITY)

1. **Add Q-1-90, Q-1-91, Q-1-92, Q-1-93 to catalog**
   - These are actively used in `quarto-yaml-validation`
   - Already mapped in `ValidationErrorKind::error_code()`
   - Missing catalog metadata

2. **Add Q-3-38 to catalog**
   - Used in JSON writer for serialization errors
   - Need to define proper metadata

3. **Decide on Q-0-99** <!-- quarto-error-code-audit-ignore -->
   - Currently used as migration aid for generic errors
   - Either add to catalog or create plan to phase out

### Short-term Actions (MEDIUM PRIORITY)

4. **Audit documentation examples**
   - Find all uses of Q-1-1, Q-1-2, etc. in markdown files
   - Update to reference actual catalog codes
   - Or explicitly mark as examples: "Q-X-Y (example code)"

5. **Investigate unknown codes**
   - Q-0-2, Q-2-35, Q-2-39, Q-2-40, etc.
   - Determine if these are:
     - Planned codes (document intent)
     - Old codes (remove references)
     - Typos (fix)

6. **Review Q-4+ subsystem codes**
   - Check design documents for context
   - Document future subsystem plans
   - Or remove if no longer planned

### Long-term Actions (LOW PRIORITY)

7. **Establish code format standards**
   - No leading zeros (Q-1-10, not Q-1-010)
   - Document valid number ranges per subsystem
   - Add validation to prevent invalid formats

8. **Regular audits**
   - Run `scripts/quick-error-audit.sh` monthly
   - Before releases
   - After major refactoring

9. **Pre-commit hook**
   - Validate error codes match catalog
   - Enforce format standards
   - Flag new codes for review

## Next Steps

### Option 1: Aggressive Cleanup
1. Add all legitimate codes (Q-1-90, Q-1-91, Q-1-92, Q-1-93, Q-3-38)
2. Update all documentation to use real codes
3. Remove all invalid subsystem references (Q-4+)
4. Investigate and resolve all unknowns

**Timeline:** 2-3 sessions
**Benefit:** Clean, consistent codebase

### Option 2: Conservative Approach
1. Add only critical codes (Q-1-90-93, Q-3-38)
2. Document remaining codes as "under investigation"
3. Address incrementally as encountered

**Timeline:** 1 session for critical, ongoing for rest
**Benefit:** Minimal disruption, low risk

## Files Generated

The audit script created these temporary files:

```
/tmp/catalog-codes.txt  - All 67 codes in catalog
/tmp/source-codes.txt   - All 119 codes found in source
/tmp/consistent.txt     - 67 codes in both (perfect!)
/tmp/missing.txt        - 52 codes missing from catalog
/tmp/orphaned.txt       - 0 codes unused (perfect!)
```

## How to Re-run

```bash
# Quick check
./scripts/quick-error-audit.sh

# Detailed analysis
cat /tmp/missing.txt

# Find all uses of a specific code
rg "Q-3-38" --type rust -A3 -B3
```

## Related Documents

- Workflow: `claude-notes/workflows/2025-11-23-error-code-audit-workflow.md`
- Script: `scripts/quick-error-audit.sh`
- Catalog: `crates/quarto-error-reporting/error_catalog.json`
- YAML codes: `private-crates/quarto-yaml-validation/src/error.rs`
