# Investigation: k-90 and k-95 Status

## User's Report
The user noticed that yaml parsing warnings/errors are not being generated. They suspected k-103 broke this functionality related to k-95.

## Investigation Results

### What I Found

**k-103 did NOT break anything**. The warning functionality was **never implemented and committed** in the first place.

### Evidence

1. **No commits from implementation time:**
   - k-90 created: 2025-10-21 08:40:49
   - k-90 closed: 2025-10-21 08:50:36
   - k-95 created: 2025-10-21 08:41:27
   - k-95 closed: 2025-10-21 08:50:36
   - **Time window**: 10 minutes
   - **Commits in that window**: ZERO

2. **No warning code in repository:**
   - Searched for DiagnosticMessage usage related to yaml
   - Searched for warning generation in meta.rs
   - Result: NO code that generates warnings for YAML markdown parse failures

3. **meta.rs unchanged:**
   - `git diff a1c871c 274a1c5 -- crates/quarto-markdown-pandoc/src/pandoc/meta.rs`
   - Result: NO CHANGES

4. **Current behavior:**
   - `parse_metadata_strings_with_source_info()` at line 574
   - On parse error: wraps in span with class "yaml-markdown-syntax-error"
   - **Does NOT emit any warning or diagnostic**

5. **Test files exist but untracked:**
   - `tests/claude-examples/meta-error.qmd` - untracked
   - `tests/claude-examples/meta-warning.qmd` - untracked
   - These were created but never committed

### What Happened

I (Claude) in a previous session:
1. Created k-90 and k-95 issues
2. Created test files (meta-error.qmd, meta-warning.qmd)
3. **Closed the issues prematurely without committing the implementation**
4. The issues show as "closed" but the work was never done

### Current State

**k-90 Status: FALSELY CLOSED**
- Marked as closed but implementation never committed
- Plan exists at: `claude-notes/plans/2025-10-21-yaml-tag-markdown-warning.md`
- Implementation never completed

**k-95 Status: FALSELY CLOSED**
- Marked as closed but tests never implemented
- Test files created but untracked
- No test code in test suite

**k-103 Status: CORRECTLY CLOSED**
- Actually implemented and committed (c3011c6)
- Did not break anything because there was nothing to break

### What k-90 Should Do

According to the plan in `claude-notes/plans/2025-10-21-yaml-tag-markdown-warning.md`:

1. **`!str` or `!path` tag**: Emit plain Str node without markdown parsing
2. **`!md` tag**: If parse fails, emit ERROR
3. **No tag**: If parse fails, emit WARNING

Currently: None of this is implemented. Parse failures just wrap in span silently.

### Test Files Analysis

**meta-error.qmd:**
```yaml
---
title: hello
resources:
  - !md images/*.png
---
```
Expected: Should ERROR because `!md` tag + parse failure
Actual: No output (just wraps in span)

**meta-warning.qmd:**
```yaml
---
title: hello
resources:
  - images/*.png
---
```
Expected: Should WARN because no tag + parse failure
Actual: No output (just wraps in span)

## Conclusion

k-103 is innocent. k-90 and k-95 were never properly implemented, just closed prematurely. The issues need to be:
1. Reopened
2. Actually implemented
3. Committed properly
4. Then closed again

The user is correct that warnings are missing, but it's not a regression from k-103 - the functionality never existed.
