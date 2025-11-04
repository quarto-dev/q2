# Parsing Failure Investigation Template

Date: 2025-11-04
File: claude-notes/plans/2025-11-04-parsing-failure-investigation-template.md

## Purpose

This template provides a systematic approach for investigating qmd files that fail to parse after the grammar migration. Use this when encountering parsing failures to determine if they are:
1. Expected failures (invalid qmd syntax)
2. Tree-sitter grammar bugs
3. Rust processing bugs (in quarto-markdown-pandoc)

## Investigation Steps

### 1. Initial Reproduction

**Objective**: Confirm the failure and capture error details

**Steps**:
- [ ] Run `cargo run -p quarto-markdown-pandoc -- -i <file-path>` to see the error
- [ ] Note the error message and panic location (if applicable)
- [ ] Note which line/construct is failing

**What to capture**:
- Error message text
- Panic location (file:line)
- The problematic construct from the source file

**Example from this investigation**:
```bash
cargo run -p quarto-markdown-pandoc -- -i external-sites/quarto-web/docs/blog/posts/2024-04-01-manuscripts-rmedicine/index.qmd
```
Result: Panic at `uri_autolink.rs:25` - "Invalid URI autolink: <https://...>" with leading space

### 2. Create Minimal Test Case

**Objective**: Isolate the exact construct causing the failure

**Steps**:
- [ ] Extract just the failing construct into a minimal .qmd file
- [ ] Verify the minimal case reproduces the error
- [ ] Simplify further if possible (e.g., shorten URLs, reduce text)

**What to create**:
- A file like `test-<construct>-debug.qmd` with minimal content
- Keep it in the repo root for easy testing

**Example from this investigation**:
```qmd
at <https://example.com>.
```

### 3. Analyze Tree-Sitter Parse Tree

**Objective**: Understand how the grammar is parsing the construct

**Steps**:
- [ ] Run with `-v` flag: `cargo run -p quarto-markdown-pandoc -- -i <test-file> -v 2>&1 | tail -100`
- [ ] Examine the parse tree structure
- [ ] Identify the node boundaries (row, col) for the problematic construct
- [ ] Check if node boundaries match expected character positions

**What to look for**:
- Unexpected nodes or missing nodes
- Incorrect node boundaries (spans)
- ERROR nodes in the tree
- Mismatch between what the node *should* span vs what it *does* span

**Example from this investigation**:
```
pandoc_str: {Node pandoc_str (0, 0) - (0, 2)}     # "at" ✓
autolink: {Node autolink (0, 2) - (0, 24)}        # " <https://example.com>" ✗ (includes space!)
pandoc_str: {Node pandoc_str (0, 24) - (0, 25)}   # "." ✓
```

### 4. Verify Character Positions

**Objective**: Confirm the exact bytes the grammar is capturing

**Steps**:
- [ ] Use `echo -n "<text>" | od -c` to see character positions
- [ ] Or use `cut` to extract the exact range: `echo -n "<text>" | cut -c START-END`
- [ ] Compare with node boundaries from step 3

**Example from this investigation**:
```bash
echo -n "at <https://example.com>." | cut -c 3-24
# Output: " <https://example.com>"
# Position 3 is the space (1-indexed), which corresponds to position 2 (0-indexed)
```

### 5. Classify the Bug

**Objective**: Determine whether this is a grammar bug or Rust processing bug

**Decision tree**:

```
Is the tree-sitter parse tree structure correct?
├─ NO → Grammar bug (tree-sitter-qmd)
│       Go to Section 6A: Grammar Investigation
└─ YES → Is the Rust code correctly interpreting the tree?
          ├─ NO → Rust processing bug (quarto-markdown-pandoc)
          │       Go to Section 6B: Rust Processing Investigation
          └─ YES → Is this valid qmd syntax?
                   ├─ NO → Expected failure (document in notes)
                   └─ YES → Unclear - consult with team
```

**Example from this investigation**:
- Tree structure has wrong boundaries → Grammar bug
- Need to investigate external scanner

### 6A. Grammar Investigation (for grammar bugs)

**Objective**: Locate the grammar rule causing the problem

**Steps**:
- [ ] Search for the node type in `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
  ```bash
  grep -n "node_name" crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js
  ```
- [ ] Determine if it's an external scanner token
  ```bash
  # Look in the externals array
  ```
- [ ] If external, examine `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
  ```bash
  grep -n "TOKEN_NAME" crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c
  ```
- [ ] Read the scanner function to understand the parsing logic
- [ ] Identify where the bug occurs (e.g., consuming wrong characters, not marking token end)

**Key questions**:
- Is the scanner consuming more/less than it should?
- Is `lexer->mark_end(lexer)` being called at the right time?
- Is there unwanted whitespace consumption?
- Are the boundary conditions (start/end of construct) correct?

**Example from this investigation**:
- `autolink` is defined as `$._autolink` external token
- Scanner function: `parse_open_angle_brace()` in scanner.c:1532
- Root cause: Whitespace consumed in lines 1973-1979 for indentation calculation is included in token because `lexer->mark_end()` not called after whitespace

### 6B. Rust Processing Investigation (for Rust bugs)

**Objective**: Find the Rust code misinterpreting the tree

**Steps**:
- [ ] Find the processing function for the node type
  ```bash
  grep -r "node_type\|NodeType" crates/quarto-markdown-pandoc/src/
  ```
- [ ] Read the function to understand what it expects
- [ ] Check if assumptions match the actual tree structure
- [ ] Identify the mismatch

**Key questions**:
- Does the code expect different node boundaries?
- Are child nodes being accessed incorrectly?
- Is the text extraction wrong (e.g., wrong slice indices)?
- Are there assumptions about whitespace that don't hold?

**Example** (hypothetical):
- If the tree was correct but Rust code assumed no leading whitespace
- Fix would be to trim whitespace in Rust code

### 7. Document Findings

**Objective**: Record investigation results for reference

**What to document**:
- File path and line number of failure
- Minimal reproducing test case
- Root cause (grammar or Rust, with specific location)
- Whether fix is needed or if it's expected behavior

**Where to document**:
- For bugs to fix: Create beads issue with `bd create`
- For investigation notes: `claude-notes/investigations/YYYY-MM-DD-<topic>.md`

### 8. Fix Strategy (if needed)

Once root cause is identified:

**For grammar bugs**:
- [ ] Write a tree-sitter test in `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/`
- [ ] Verify test fails with current grammar
- [ ] Fix scanner.c or grammar.js
- [ ] Run `tree-sitter generate && tree-sitter build` in the grammar directory
- [ ] Run `tree-sitter test` to verify
- [ ] Test with original failing file

**For Rust bugs**:
- [ ] Write a test in `crates/quarto-markdown-pandoc/tests/`
  - Test files for Rust crashes (panics) go on `crates/quarto-markdown-pandoc/tests/smoke`
  - Test files for misparsing go on snapshots for the relevant format. **Important**: make the smallest snapshot tests possible that exercise the bug.
- [ ] Verify test fails
- [ ] Fix the Rust code
- [ ] Run `cargo test` to verify
- [ ] Test with original failing file

## Quick Reference Commands

```bash
# Run parser on file
cargo run -p quarto-markdown-pandoc -- -i <file>

# Run with verbose tree output
cargo run -p quarto-markdown-pandoc -- -i <file> -v 2>&1 | tail -200

# Run with full backtrace
RUST_BACKTRACE=1 cargo run -p quarto-markdown-pandoc -- -i <file>

# Check character positions
echo -n "text" | od -c
echo -n "text" | cut -c START-END

# Search grammar
grep -n "pattern" crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js

# Search scanner
grep -n "PATTERN" crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c

# Rebuild grammar (from tree-sitter-markdown directory)
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter generate && tree-sitter build && tree-sitter test

# Run Rust tests
cargo test -p quarto-markdown-pandoc
cargo test  # all tests
```