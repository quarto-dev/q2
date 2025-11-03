# k-313: Blockquote Header Roundtrip - Already Fixed

## Date: 2025-11-03

## Issue Description
k-313 claimed that `> ## Header` roundtrips with an extra Space at the end when going through QMD→JSON→QMD→JSON.

## Investigation Results
**The issue is already resolved!** The test passes cleanly.

### Test Results

#### Test Input
```markdown
> ## Header
```

#### Step 1: QMD → JSON
Header inline content:
```json
[
  {
    "c": "Header",
    "s": 0,
    "t": "Str"
  }
]
```
✅ Only one Str element, no extra Space

#### Step 2: JSON → QMD
Output:
```markdown
> ## Header {#header}
```
The auto-generated ID is added, but no extra Space in the header text itself.

#### Step 3: QMD → JSON (Second Pass)
The JSON matches the first pass (modulo location info).

### Test Status
```bash
cargo test --test test -- test_empty_blockquote_roundtrip
# test result: ok. 1 passed; 0 failed
```

The test `test_empty_blockquote_roundtrip` **PASSES** ✅

This test:
1. Reads `tests/roundtrip_tests/qmd-json-qmd/blockquote_with_elements.qmd`
2. Converts QMD → JSON → QMD → JSON
3. Compares the two JSON outputs (with location fields removed)
4. Asserts they are identical

### Tree-Sitter Parse
```
pandoc_block_quote [0, 0] - [1, 0]
  block_quote_marker [0, 0] - [0, 2]     # "> "
  section [0, 2] - [1, 0]
    atx_heading [0, 2] - [1, 0]
      atx_h2_marker [0, 2] - [0, 4]      # "##"
      pandoc_str [0, 5] - [0, 11]        # "Header"
```

The parse is clean - just the "Header" string with no trailing spaces.

## Conclusion
k-313 can be **CLOSED** as the issue is already fixed. The roundtrip works correctly:
- ✅ No extra Space in header content
- ✅ JSON roundtrip is consistent
- ✅ Test passes

The issue was likely created based on an old state of the code, or was fixed as a side effect of the tree-sitter refactoring or another fix.

## Recommendation
Close k-313 with note: "Already fixed - blockquote header roundtrip working correctly."
