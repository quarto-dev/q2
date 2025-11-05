# Unicode Offset Bug Investigation Plan (k-328)

**Issue**: Error diagnostics show incorrect column positions when unicode characters precede errors.

## Observed Behavior

### Test Case 1: With Unicode (bug.qmd)
```
✓
[#no]{.hello}
```
- `✓` = 3 bytes in UTF-8 (e2 9c 93), 1 character
- Error at `#` (byte offset 5, should be column 2)
- **Reports**: line 2, column 4 (WRONG - off by 2)

### Test Case 2: ASCII Control (bug-ascii.qmd)
```
x
[#no]{.hello}
```
- `x` = 1 byte, 1 character
- Error at `#` (byte offset 3, should be column 2)
- **Reports**: line 2, column 2 (CORRECT)

### Key Insight
The 2-byte discrepancy = 3 bytes (UTF-8) - 1 character (logical position)

This suggests **byte offsets are being used where character offsets are expected** (or vice versa).

## Investigation Strategy

### Phase 1: Isolate the Layer
Determine which component has the bug:
1. **Tree-sitter layer**: Does tree-sitter itself report wrong positions?
2. **Offset translation layer**: Does our code mishandle tree-sitter positions?
3. **Error reporting layer**: Does miette/ariadne miscalculate spans?

### Phase 2: Examine Tree-sitter Output
- [ ] Run `tree-sitter parse` with `-v` flag to see raw byte positions
- [ ] Check if tree-sitter reports byte offsets or character offsets
- [ ] Verify if tree-sitter's ERROR node has correct byte positions
- [ ] Document tree-sitter's position semantics

### Phase 3: Trace Through Rust Code
Key areas to examine:

#### A. Source Position Types (`quarto-source-map`)
- [ ] Check `SourcePosition` struct - does it store bytes or chars?
- [ ] Review `SourceLocation` - how are offsets stored?
- [ ] Examine `SourceMap::offset_to_position()` conversion
- [ ] Look for any UTF-8/UTF-16 confusion

#### B. Tree-sitter Integration
- [ ] Find where tree-sitter positions are extracted
- [ ] Check `Node::start_byte()` vs `Node::start_position()` usage
- [ ] Verify if we're using byte offsets or point (row, column) positions
- [ ] Look for conversions between tree-sitter coords and our coords

#### C. Error Construction (`quarto-error-reporting`)
- [ ] Trace how parse errors are created
- [ ] Check `Report` construction with source spans
- [ ] Verify if miette/ariadne expects byte or char offsets
- [ ] Look for labeled span creation

### Phase 4: Create Diagnostic Tests
Before fixing, create tests that expose the bug:
- [ ] Test with various unicode characters (1, 2, 3, 4 byte sequences)
- [ ] Test with multiple unicode chars before error
- [ ] Test with unicode on same line vs previous line
- [ ] Test with mixed ASCII + unicode

### Phase 5: Fix Strategy
Once root cause is identified:
1. Determine the canonical offset type (byte vs char vs UTF-16)
2. Add explicit conversion functions where needed
3. Add documentation about offset semantics
4. Add assertions/type safety to prevent future confusion

## Areas of Concern

### Tree-sitter Position Semantics
Tree-sitter uses:
- **Byte offsets**: `Node::start_byte()`, `Node::end_byte()`
- **Points**: `Node::start_position()` returns `Point { row, column }` where column is in **bytes**, not characters

**Critical**: Tree-sitter's `Point.column` is a **byte offset from line start**, not a character count!

### Rust String Indexing
- Rust strings are UTF-8
- `.len()` returns bytes, not characters
- `.chars().count()` returns character count
- Slicing must happen at char boundaries or it panics

### Miette/Ariadne Expectations
Need to verify what these libraries expect:
- Do they want byte offsets into the source string?
- Do they want character positions?
- Do they handle UTF-8 correctly?

## Investigation Results

### Tree-sitter Position Semantics (CONFIRMED)
- Tree-sitter uses `Point { row, column }` where **column is a byte offset from line start**, not character count
- bug.qmd: `ERROR [1, 1]` means row=1 (line 2), column=1 byte from line start
- bug-ascii.qmd: `ERROR [1, 1]` - same position
- Tree-sitter correctly reports byte positions

### Code Flow Analysis

#### 1. `calculate_byte_offset()` (qmd_error_messages.rs:424)
**Status**: Working CORRECTLY
- Takes tree-sitter's (row, column) where column is BYTE offset from line start
- Returns absolute byte offset into the file
- Verified: returns 5 for bug.qmd, 3 for bug-ascii.qmd - both correct!

#### 2. `offset_to_location()` (quarto-source-map/utils.rs:8)
**Status**: Has SEMANTIC MISMATCH
- Takes byte offset, returns `Location { offset, row, column }`
- Increments `column` by 1 per CHARACTER (line 26)
- Increments `current_offset` by bytes (line 29: `ch.len_utf8()`)
- **Result**: column field is CHARACTER COUNT, not byte offset!
- This creates inconsistency: column semantics differ from tree-sitter

#### 3. `render_ariadne_source_context()` (quarto-error-reporting/diagnostic.rs:517)
**Status**: Uses byte offsets correctly
- Passes `start_mapped.location.offset` to ariadne (line 554)
- Ariadne expects byte offsets and handles them correctly
- But the header "2:4" comes from ariadne's internal line:col calculation from the byte offset

### Root Cause: OFFSET TYPE MISMATCH (CONFIRMED)

**The bug IS in our code!**

Ariadne has two modes (via `IndexType` enum):
- `IndexType::Char` (DEFAULT): Expects character offsets
- `IndexType::Byte`: Expects byte offsets

We are passing **byte offsets** to ariadne without setting `IndexType::Byte`.

Created minimal test case (`test-unicode/`) proving ariadne works correctly:
- **Passing char offset 3 (default mode)**: Reports `2:2` and highlights '#' ✅
- **Passing byte offset 5 with `IndexType::Byte`**: Reports `2:2` and highlights '#' ✅
- **Passing byte offset 5 with default (char mode)**: Reports `2:4` and highlights 'o' ❌ (OUR BUG!)

The checkmark '✓' is 3 bytes (UTF-8: e2 9c 93) but 1 character.
When we pass byte offset 5 in char mode, ariadne treats it as char offset 5, which IS the 'o'.

**Our mistake**: Using ariadne's default config (char mode) while passing byte offsets.

## Next Steps

1. ✅ Confirmed tree-sitter reports byte offsets correctly
2. ✅ Found semantic mismatch in `offset_to_location` (character count vs byte offset)
3. ✅ Verified `calculate_byte_offset` works correctly
4. ✅ Created minimal ariadne test case (`test-unicode/`)
5. ✅ Confirmed root cause: ariadne unicode bug (affects 0.4 and 0.5.1)
6. **Decision point: How to fix?**

## Solution Options

### Option 1: Use IndexType::Byte (SIMPLE, keeps byte offsets)
- Add `.with_config(Config::default().with_index_type(IndexType::Byte))` to Report::build calls
- Keep all our existing byte-based offset calculations
- **Benefit**: Minimal code changes
- **Downside**: Byte-based columns may be confusing to users who think in characters

### Option 2: Convert byte offsets to char offsets (ACCURATE for users)
- Create a function to convert byte offsets to character counts
- Use default ariadne config (char mode)
- **Benefit**: User-facing column numbers match character positions (more intuitive)
- **Downside**: Need conversion function, may be slower

### Option 3: Fix semantic mismatch in offset_to_location (CLEANUP)
- Currently `offset_to_location` creates Location with `column` as char count
- Make it consistent: either all bytes or all chars throughout
- Then use appropriate IndexType in ariadne
- **Benefit**: Cleaner, more consistent internal code
- **Can combine with**: Option 1 or Option 2

### Recommendation
**Option 1 + Option 3 combo**:
1. Immediate fix: Use `IndexType::Byte` in ariadne (one-line change in diagnostic.rs:554)
2. Cleanup: Make offset_to_location semantics consistent (use byte offsets for column)
3. Later: Consider Option 2 if users complain about byte-based column numbers

This gives us:
1. Quick fix to stop showing wrong error positions
2. Cleaner internal semantics
3. Easy to switch to char offsets later if needed

## Related Code Locations
- Tree-sitter integration: `crates/quarto-markdown-pandoc/src/pandoc/location.rs:127-142`
- Error construction: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs:217-421`
- Offset calculations:
  - `calculate_byte_offset`: `qmd_error_messages.rs:424-453`
  - `offset_to_location`: `quarto-source-map/src/utils.rs:8-37`
  - `line_col_to_offset`: `quarto-source-map/src/utils.rs:42-68`
- Error display: `crates/quarto-error-reporting/src/diagnostic.rs:517-620`
