# Error Code Audit: Ignore Feature

<!-- quarto-error-code-audit-ignore-file -->

Date: 2025-11-23

## Purpose

Allows marking specific error code usages to be excluded from audit reports. This is useful for:
- Test codes that are intentionally invalid (e.g., `Q-999-999`)
- Example codes in documentation
- Codes used to test error handling

## Usage

### Line-Level Ignore

Add `quarto-error-code-audit-ignore` as a comment on the same line as the error code.

**Examples:**

**Rust:**
```rust
// Test that invalid codes return None
assert_eq!(get_subsystem("Q-999-999"), None); // quarto-error-code-audit-ignore
```

**Markdown:**
```markdown
| Code | Description |
|------|-------------|
| Q-999-999 | Invalid test code <!-- quarto-error-code-audit-ignore --> |
```

**JSON (in comments if language supports):**
```json
{
  "test_code": "Q-999-999"  // quarto-error-code-audit-ignore
}
```

### File-Level Ignore

Add `quarto-error-code-audit-ignore-file` **anywhere** in a file (usually at the top) to ignore ALL error codes in that file.

**Examples:**

**Rust:**
```rust
// quarto-error-code-audit-ignore-file
//! Test module with many example error codes
```

**Markdown:**
```markdown
# Design Document

<!-- quarto-error-code-audit-ignore-file -->

This document discusses error codes Q-1-1, Q-1-2, Q-1-3...
```

**Any file type:**
```
# quarto-error-code-audit-ignore-file
// quarto-error-code-audit-ignore-file
<!-- quarto-error-code-audit-ignore-file -->
```

**Use cases for file-level ignore:**
- Design documents that reference many error codes
- Test data files with example codes
- Documentation with extensive error code examples
- Migration planning documents

## How It Works

### Line-Level Ignore

When the audit script (`scripts/audit-error-codes.py`) scans the codebase:

1. **Detection**: For each line containing a Q-*-* code, check if `quarto-error-code-audit-ignore` appears anywhere on that line
2. **Marking**: If found, the `CodeLocation` is marked as `ignored=True`
3. **Filtering**: When calculating statistics:
   - Ignored locations don't count toward occurrence totals
   - If ALL locations for a code are ignored, the code is completely excluded from results
4. **Reporting**: Ignored codes don't appear in any output category

### File-Level Ignore

The script efficiently handles file-level ignores:

1. **Match Collection**: As ripgrep finds matches, track which files have error codes
2. **File Checking**: After ripgrep completes, check ONLY files with matches for the file-level marker
   - This maintains performance by not reading every file in the repo
   - Only files that ripgrep already identified are checked
3. **Post-Processing**: Mark ALL locations in files with the marker as `ignored=True`
4. **Filtering**: Same as line-level - ignored locations are excluded from statistics

**Performance:**
- File content is read only once per file with matches
- Files without error codes are never checked
- Efficient for large repositories

## Before and After

### Example 1: Line-Level Ignore

**Before** (Q-999-999 reported as invalid):
```
INVALID FORMAT CODES (INVESTIGATE)
  • Q-999-999 (5 occurrences)
    Example: crates/quarto-error-reporting/src/catalog.rs:130
```

**After** (adding `// quarto-error-code-audit-ignore` to 4 lines):
```
INVALID FORMAT CODES (INVESTIGATE)
  • Q-999-999 (1 occurrences)
    Example: claude-notes/investigations/...
```

### Example 2: File-Level Ignore

**Before** (design doc with many codes):
```
LEGITIMATE MISSING CODES (HIGH PRIORITY)
  • Q-1-1
    Occurrences: 41
    Files: 15
  • Q-1-2
    Occurrences: 22
    Files: 10
```

**After** (adding `<!-- quarto-error-code-audit-ignore-file -->` to top of design doc):
```
LEGITIMATE MISSING CODES (HIGH PRIORITY)
  • Q-1-1
    Occurrences: 37  (↓4)
    Files: 14  (↓1)
  • Q-1-2
    Occurrences: 18  (↓4)
    Files: 9  (↓1)
```

All codes in that file are now excluded from the audit!

## When to Use

### ✅ Good Uses

**Line-level:**

1. **Test sentinel values**
   ```rust
   #[test]
   fn test_invalid_code() {
       assert!(get_error("Q-999-999").is_none()); // quarto-error-code-audit-ignore
   }
   ```

2. **Documentation examples of invalid codes**
   ```markdown
   Don't use codes like Q-999-999 <!-- quarto-error-code-audit-ignore --> as they're invalid.
   ```

3. **Code that explicitly tests error handling**
   ```rust
   // Verify we handle malformed codes gracefully
   let result = parse_code("Q-1000-1000"); // quarto-error-code-audit-ignore
   ```

**File-level:**

1. **Design documents**
   ```markdown
   # Error Code Design
   <!-- quarto-error-code-audit-ignore-file -->

   We propose codes Q-4-1 through Q-4-50 for the new subsystem...
   ```

2. **Test data files**
   ```rust
   // quarto-error-code-audit-ignore-file
   // Test data with many example error codes
   pub const EXAMPLE_CODES: &[&str] = &[
       "Q-1-1", "Q-1-2", "Q-1-3", ...
   ];
   ```

3. **Migration planning documents**
   ```markdown
   # Migration Plan
   <!-- quarto-error-code-audit-ignore-file -->

   Phase 1: Add Q-1-1, Q-1-2, Q-1-3
   Phase 2: Add Q-2-1, Q-2-2, Q-2-3
   ...
   ```

### ❌ Bad Uses

1. **Hiding legitimate missing codes**
   ```rust
   // DON'T DO THIS - this should be in the catalog!
   return Error::new("Q-3-38"); // quarto-error-code-audit-ignore
   ```

2. **Avoiding catalog work**
   ```rust
   // DON'T DO THIS - add Q-1-90 to the catalog instead
   ValidationErrorKind::SchemaFalse => "Q-1-90" // quarto-error-code-audit-ignore
   ```

3. **Suppressing real issues**
   ```rust
   // DON'T DO THIS - fix the typo!
   error_code: "Q-1-010" // quarto-error-code-audit-ignore  (should be Q-1-10)
   ```

## Verification

After adding ignore markers, verify they work:

```bash
# Run audit
./scripts/audit-error-codes.py

# Check that ignored code doesn't appear
./scripts/audit-error-codes.py --format json | \
  jq '.legitimate_missing | has("Q-999-999")'
# Should output: false
```

## Implementation Details

### Code Changes

**Script:** `scripts/audit-error-codes.py`

1. **CodeLocation dataclass**
   ```python
   @dataclass
   class CodeLocation:
       file: str
       line: int
       context: str
       ignored: bool = False  # Tracks both line and file-level ignores
   ```

2. **Line-level detection during search**
   ```python
   # Check if line has ignore marker
   has_ignore = 'quarto-error-code-audit-ignore' in line_text

   # Mark location
   CodeLocation(..., ignored=has_ignore)
   ```

3. **File-level post-processing**
   ```python
   # Track files with matches (for performance)
   files_with_matches: Set[str] = set()

   # After ripgrep completes, check for file-level markers
   file_ignore_cache = self._check_file_ignores(files_with_matches)

   # Mark all locations in ignored files
   for usage in codes.values():
       for location in usage.locations:
           if file_ignore_cache.get(location.file, False):
               location.ignored = True
   ```

4. **File checking helper**
   ```python
   def _check_file_ignores(self, files: Set[str]) -> Dict[str, bool]:
       """Check which files have file-level ignore markers.
       Only checks files that had matches (for performance).
       """
       cache = {}
       for file_path in files:
           with open(file_path, 'r') as f:
               content = f.read()
               cache[file_path] = 'quarto-error-code-audit-ignore-file' in content
       return cache
   ```

5. **Filtering in CodeUsage**
   ```python
   @property
   def count(self) -> int:
       """Count of non-ignored locations."""
       return len([loc for loc in self.locations if not loc.ignored])

   @property
   def all_ignored(self) -> bool:
       """True if all locations have ignore marker."""
       return all(loc.ignored for loc in self.locations)
   ```

6. **Exclusion from results**
   ```python
   # Filter out codes where ALL locations are ignored
   active_source_codes = {
       code: usage
       for code, usage in self.source_codes.items()
       if not usage.all_ignored
   }
   ```

### Files Changed

Applied ignore markers to existing test code:

1. `crates/quarto-error-reporting/src/catalog.rs`
   - Lines 130, 135, 136

2. `crates/quarto-error-reporting/src/diagnostic.rs`
   - Line 910

3. `claude-notes/investigations/2025-11-23-error-code-audit-results.md`
   - Line 57

## Future Enhancements

Possible improvements:

1. **Block-level ignores**
   ```rust
   // quarto-error-code-audit-ignore-block-start
   #[cfg(test)]
   mod tests {
       // All Q-*-* codes in this block ignored
   }
   // quarto-error-code-audit-ignore-block-end
   ```

2. **File-level ignores**
   ```rust
   // At top of file:
   // quarto-error-code-audit-ignore-file
   ```

3. **Specific code ignores**
   ```rust
   // Only ignore Q-999-999, not other codes on same line
   assert_eq!(code, "Q-999-999"); // quarto-error-code-audit-ignore: Q-999-999
   ```

4. **Reason documentation**
   ```rust
   // quarto-error-code-audit-ignore: test sentinel value
   assert!(get_error("Q-999-999").is_none());
   ```

## Best Practices

1. **Add comments explaining why**
   ```rust
   // Test invalid code handling - Q-999-999 is intentionally malformed
   assert!(validate("Q-999-999").is_err()); // quarto-error-code-audit-ignore
   ```

2. **Use sparingly**
   - Only for test/example code
   - Not as a shortcut to avoid catalog work

3. **Review periodically**
   - Check that ignored codes are still necessary
   - Remove ignore markers when code is deleted

4. **Document in tests**
   ```rust
   #[test]
   fn test_invalid_code_handling() {
       // Q-999-999 is a sentinel value used throughout tests to verify
       // that the system handles invalid error codes gracefully.
       // It should never appear in production code.
       assert!(is_valid("Q-999-999") == false); // quarto-error-code-audit-ignore
   }
   ```

## Related

- Main workflow: `claude-notes/workflows/2025-11-23-error-code-audit-workflow.md`
- Script: `scripts/audit-error-codes.py`
- Usage: `scripts/README.md`
