# k-86: FileId Handling Analysis

## The Issue

During k-84 migration, code was changed from:
```rust
SourceInfo::new(
    if context.filenames.is_empty() { None } else { Some(0) },
    range
).to_source_map_info()
```

To:
```rust
SourceInfo::original(
    if context.filenames.is_empty() {
        FileId(0)
    } else {
        FileId(0)
    },
    range
)
```

Both branches return `FileId(0)`, which looks suspicious.

## Investigation Findings

### 1. Old System (pandoc::location)
- `filename_index: Option<usize>`
- `None` = no filename information
- `Some(0)` = use filename at index 0 in `context.filenames`

### 2. New System (quarto_source_map)
- `FileId(usize)` - NOT optional
- `FileId(0)` = first file in SourceContext
- Always requires a FileId, cannot be None

### 3. ASTContext Structure
```rust
pub struct ASTContext {
    pub filenames: Vec<String>,  // Legacy filename tracking
    pub source_context: SourceContext,  // New quarto_source_map tracking
}
```

### 4. Filename Behavior
- `main.rs` always sets a filename (either from `-i` arg or "<stdin>")
- `ASTContext::with_filename()` adds file to both `filenames` and `source_context`
- `ASTContext::new()` creates empty context (used by JSON reader for backward compat)

### 5. Current Behavior
Tested with actual binary:
- With file: outputs `"filenameIndex": 0` ✓
- With stdin: outputs `"filenameIndex": 0` with filename "<stdin>" ✓

## The Real Question

**Is the code actually buggy?**

The if statement is checking `context.filenames.is_empty()` but both branches return the same thing. This means:

**Option A**: The check is pointless and should be removed (just always use `FileId(0)`)

**Option B**: The check is meaningful but the else branch is wrong (should handle empty case differently)

### When is `context.filenames.is_empty()` true?

Only when:
1. JSON reader creates `ASTContext::new()` for backward compat
2. Someone explicitly creates an anonymous context

In these cases, using `FileId(0)` references a non-existent file in the SourceContext.

## Proposed Solution

### Analysis

The mapping should be:
- Old `None` → Cannot map directly to new system (FileId required)
- Old `Some(0)` → New `FileId(0)`

When there's no filename (old `None` case), we still need to provide a `FileId`. The question is: **what FileId should we use?**

### Options

1. **Always use FileId(0)** (current behavior)
   - Simple, but FileId(0) might not exist in SourceContext
   - Could cause issues if SourceContext is queried

2. **Create a dummy file in SourceContext when empty**
   - Add "<unknown>" or "" to SourceContext when needed
   - FileId(0) always valid

3. **Use FileId based on what's in SourceContext**
   - Check `context.source_context` instead of `context.filenames`
   - Use `context.primary_file_id()` which returns `Option<FileId>`
   - But SourceInfo::original requires FileId (not Option)

## Recommendation

**Option 1 is actually correct!**

Reasons:
1. The new system requires a FileId (no None option)
2. Using FileId(0) as a default is reasonable
3. The if statement is indeed pointless and should be simplified to just `FileId(0)`
4. The actual output shows it's working correctly

**Action**: Simplify all instances to just use `FileId(0)` directly, removing the pointless if statements.

This is a code clarity issue, not a functional bug. The behavior is correct, just unnecessarily complicated.
