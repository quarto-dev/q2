# Plan: Fix qmd-syntax-helper Glob Pattern No-Match Handling

## Problem Statement

When a glob pattern matches no files, `expand_globs()` treats it as a literal filename, which then causes a confusing error.

**Test case:**
```bash
$ cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'file-that-totally-does-not-exist.qmd'
```

**Current output:**
```
file-that-totally-does-not-exist.qmd
  ✗ Error checking definition-lists: Failed to read file: file-that-totally-does-not-exist.qmd

=== Summary ===
Total files:         1
Files with issues:   0 ✓
Clean files:         1 ✓
Success rate:        100.0%
```

**Issues:**
1. Reports "Total files: 1" when no files were actually checked
2. Error is shown but summary says "100.0% success"
3. Non-existent literal files are treated same as glob patterns with no matches

## Root Cause Analysis

### Current Code (`glob_expand.rs` lines 8-30)

```rust
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for pattern in patterns {
        // Check if pattern contains glob characters
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // It's a glob pattern - expand it
            let paths = glob::glob(pattern)
                .with_context(|| format!("Invalid glob pattern: {}", pattern))?;

            for path in paths {
                let path = path.with_context(|| format!("Failed to read glob match for: {}", pattern))?;
                files.push(path);
            }
        } else {
            // It's a literal path - use as-is
            files.push(PathBuf::from(pattern));
        }
    }

    Ok(files)
}
```

**Logic:**
1. If pattern has `*`, `?`, or `[` → treat as glob
2. Expand glob and add all matches
3. If pattern has no glob characters → treat as literal path
4. Add literal path without checking if it exists

**Problems:**
1. **Glob with no matches** → returns empty `paths` iterator → adds nothing to `files`
2. **Literal non-existent file** → adds to `files` anyway → error occurs later during file read
3. **No distinction** between "glob matched nothing" vs "literal file"

### Why This Happens

The `glob::glob()` function returns an iterator. If the pattern matches no files, the iterator is empty, so nothing gets added to `files`. This is actually correct behavior for globs!

But for literal paths, we don't check existence, so non-existent files get added to the list.

## Solution Options

### Option A: Check Literal Paths Exist

Add existence check for literal paths before adding them.

```rust
} else {
    // It's a literal path - verify it exists
    let path = PathBuf::from(pattern);
    if !path.exists() {
        anyhow::bail!("File not found: {}", pattern);
    }
    files.push(path);
}
```

**Pros:**
- Early error detection
- Clear error message
- Prevents processing non-existent files

**Cons:**
- Changes behavior (currently allows non-existent files through)
- Might break if user expects to check files that will be created later (unlikely)

### Option B: Warn on Empty Results

After expansion, check if any files were found and warn if not.

```rust
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    // ... existing logic ...

    if files.is_empty() {
        eprintln!("Warning: No files matched the patterns provided");
    }

    Ok(files)
}
```

**Pros:**
- Non-breaking
- Helps user understand what happened

**Cons:**
- Doesn't prevent the error
- Just a warning, not a fix

### Option C: Error on Empty Results (Recommended)

Return an error if no files are found after glob expansion.

```rust
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    // ... existing logic ...

    if files.is_empty() {
        anyhow::bail!("No files found matching the patterns: {}", patterns.join(", "));
    }

    Ok(files)
}
```

**Pros:**
- Clear error message
- Fails fast
- User knows immediately that pattern didn't match

**Cons:**
- Changes behavior for edge cases (e.g., checking if directory is empty)
- Might be too strict

### Option D: Combination - Check Literals + Warn on Empty Globs

Check literal paths exist immediately, and provide helpful context for empty glob results.

```rust
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for pattern in patterns {
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // It's a glob pattern - expand it
            let paths = glob::glob(pattern)
                .with_context(|| format!("Invalid glob pattern: {}", pattern))?;

            let mut match_count = 0;
            for path in paths {
                let path = path.with_context(|| format!("Failed to read glob match for: {}", pattern))?;
                files.push(path);
                match_count += 1;
            }

            // Warn if glob matched nothing
            if match_count == 0 {
                eprintln!("Warning: No files matched pattern: {}", pattern);
            }
        } else {
            // It's a literal path - verify it exists
            let path = PathBuf::from(pattern);
            if !path.exists() {
                anyhow::bail!("File not found: {}", pattern);
            }
            files.push(path);
        }
    }

    Ok(files)
}
```

**Pros:**
- Best of both worlds
- Clear error for non-existent literal files
- Helpful warning for empty glob patterns
- User can distinguish between the two cases

**Cons:**
- Slightly more complex
- Warnings go to stderr (might be missed)

## Recommended Implementation: Option D

Check literal paths exist immediately and warn when glob patterns match nothing.

### Changes Required

**File:** `crates/qmd-syntax-helper/src/utils/glob_expand.rs`

**Updated function:**
```rust
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for pattern in patterns {
        // Check if pattern contains glob characters
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // It's a glob pattern - expand it
            let paths = glob::glob(pattern)
                .with_context(|| format!("Invalid glob pattern: {}", pattern))?;

            let mut match_count = 0;
            for path in paths {
                let path = path.with_context(|| format!("Failed to read glob match for: {}", pattern))?;
                files.push(path);
                match_count += 1;
            }

            // Warn if glob matched nothing
            if match_count == 0 {
                eprintln!("Warning: No files matched pattern: {}", pattern);
            }
        } else {
            // It's a literal path - verify it exists
            let path = PathBuf::from(pattern);
            if !path.exists() {
                anyhow::bail!("File not found: {}", pattern);
            }
            files.push(path);
        }
    }

    Ok(files)
}
```

### Test Cases

**Test 1: Non-existent literal file**
```bash
$ cargo run --bin qmd-syntax-helper -- check 'file-that-does-not-exist.qmd'
```

**Before:**
```
file-that-does-not-exist.qmd
  ✗ Error checking: Failed to read file
Total files: 1
Success rate: 100.0%
```

**After:**
```
Error: File not found: file-that-does-not-exist.qmd
```

**Test 2: Glob pattern with no matches**
```bash
$ cargo run --bin qmd-syntax-helper -- check 'nonexistent-dir/**/*.qmd'
```

**Before:**
```
=== Summary ===
Total files: 0
```

**After:**
```
Warning: No files matched pattern: nonexistent-dir/**/*.qmd

=== Summary ===
Total files: 0
```

**Test 3: Valid literal file**
```bash
$ cargo run --bin qmd-syntax-helper -- check 'README.md'
```

**Should still work** - no changes

**Test 4: Valid glob pattern**
```bash
$ cargo run --bin qmd-syntax-helper -- check 'external-sites/**/*.qmd'
```

**Should still work** - no changes

## Alternative: Less Strict Version

If we don't want to error on non-existent literal files (to match current behavior), we could just add the warning for empty globs:

**Option D-Lite:**
```rust
if match_count == 0 {
    eprintln!("Warning: No files matched pattern: {}", pattern);
}
```

But don't add the existence check for literals. This preserves current behavior while making glob results clearer.

## Summary

The recommended fix is to:
1. Check that literal file paths exist before adding them (fail fast with clear error)
2. Warn when glob patterns match no files (helpful feedback)

This provides clear, early feedback to users when their file patterns don't match anything.
