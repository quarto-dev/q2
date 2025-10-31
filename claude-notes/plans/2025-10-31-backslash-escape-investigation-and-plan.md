# Backslash Escape Investigation and Implementation Plan

**Date**: 2025-10-31
**Epic**: k-274 (Tree-sitter Grammar Refactoring)
**Phase**: 2 (Basic Formatting)
**Priority**: HIGH
**Status**: Investigation and Planning

## Current Situation

### What We Found

1. **Existing Handler**: There's already a `backslash_escape.rs` handler that:
   - Removes the leading backslash
   - Returns just the escaped character
   - Implementation looks correct for our needs

2. **Handler is Commented Out**: In `treesitter.rs` line 1037:
   ```rust
   // "backslash_escape" => process_backslash_escape(node, input_bytes, context),
   ```

3. **Grammar Defines backslash_escape**: In `common/common.js`:
   ```javascript
   backslash_escape: $ => $._backslash_escape,
   _backslash_escape: $ => new RegExp('\\\\[' + PUNCTUATION_CHARACTERS_REGEX + ']'),
   ```
   Where `PUNCTUATION_CHARACTERS_REGEX` includes all ASCII punctuation.

4. **Tree-sitter NOT Producing backslash_escape Nodes**:
   - Input: `test \* here`
   - Current tree: `pandoc_str "\\*"` (backslash included, as part of Str)
   - Expected tree: `backslash_escape` node
   - Our output: `Str "\\*"` (backslash kept)
   - Pandoc output: `Str "*"` (backslash removed)

5. **Escapable Characters** (from PUNCTUATION_CHARACTERS_ARRAY):
   ```
   ! " # $ % & ' ( ) * + , - . / : ; = ? @ [ \ ] ^ _ ` { | } ~
   ```

## Problem Analysis

### Why Aren't backslash_escape Nodes Being Produced?

Possible reasons:
1. **Grammar precedence issue**: `pandoc_str` might be capturing the text before `backslash_escape` gets a chance
2. **Grammar ordering issue**: The rules might be defined in wrong order
3. **Parser rebuild needed**: The grammar might be correct but the parser wasn't rebuilt
4. **Regex pattern issue**: The pattern might not be matching correctly

### Current Behavior vs. Desired Behavior

| Input | Current Output | Pandoc Output | Desired Output |
|-------|----------------|---------------|----------------|
| `\*` | `Str "\\*"` | `Str "*"` | `Str "*"` |
| `\[` | `Str "\\["` | `Str "["`  | `Str "["` |
| `\{` | `Str "\\{"` | `Str "{"` | `Str "{"` |
| `\\` | `Str "\\ "` (with space!) | `LineBreak` | `Str "\"` (NOT LineBreak) |
| `\a` | `Str "\\a"` | `Str "\a"` (keeps backslash!) | `Str "\a"` (keep backslash) |
| `test\*here` | `Str "test\\*here"` | `Str "test*here"` | `Str "test*here"` |
| `test \* here` | `Str "test", Space, Str "\\*", Space, Str "here"` | `Str "test", Space, Str "*", Space, Str "here"` | Same as Pandoc |

### Important Pandoc Behaviors to Match

1. **Only ASCII Punctuation**: Backslash only escapes punctuation characters, not letters
   - `\a` → keeps the backslash → `Str "\a"`
   - `\*` → removes the backslash → `Str "*"`

2. **Backslash-Space**: This is special in Pandoc - it becomes a hard line break (LineBreak node)
   - `\\` → `LineBreak`
   - **BUT** - we might NOT want this behavior if it's LaTeX-specific

3. **Prevents Markdown Interpretation**:
   - `\*text\*` → prevents emphasis, outputs `Str "*text*"`
   - `\[link\]` → prevents link, outputs `Str "[link]"`

## User's Requirements

> "We want backslash escapes to simply be a mechanism for typing characters that would otherwise be used in quarto-markdown syntax (like [, {, etc)."

> "Pandoc will often try to convert some backslash sequences to RawBlocks. We do _not_ want that."

> "'\a' is converted by Pandoc to a RawBlock in tex format, and we don't want that."

**Note**: Upon testing, `\a` actually produces `Str "\a"` in Pandoc, not a RawInline. But the principle stands - we want simple character escaping, not LaTeX processing.

## Test Cases Design

### Category 1: Basic Markdown Special Characters (MUST WORK)

These should all remove the backslash and output just the character:

```rust
#[test]
fn test_backslash_escape_asterisk() {
    // Prevents emphasis
    let input = "\\*";
    let result = parse_qmd_to_pandoc_ast(input);
    // Expected: Para [ Str "*" ]
    assert!(result.contains("Str \"*\""));
    assert!(!result.contains("\\"));
}

#[test]
fn test_backslash_escape_brackets() {
    let input = "\\[\\]";
    // Expected: Para [ Str "[" , Str "]" ]
    // OR: Para [ Str "[]" ]
}

#[test]
fn test_backslash_escape_braces() {
    let input = "\\{\\}";
    // Expected: Para [ Str "{" , Str "}" ]
}

#[test]
fn test_backslash_escape_underscore() {
    let input = "\\_";
    // Expected: Para [ Str "_" ]
}

#[test]
fn test_backslash_escape_backtick() {
    let input = "\\`";
    // Expected: Para [ Str "`" ]
}

#[test]
fn test_backslash_escape_hash() {
    let input = "\\#";
    // Expected: Para [ Str "#" ]
}

#[test]
fn test_backslash_escape_tilde() {
    let input = "\\~";
    // Expected: Para [ Str "~" ]
}

#[test]
fn test_backslash_escape_caret() {
    let input = "\\^";
    // Expected: Para [ Str "^" ]
}
```

### Category 2: Backslash with Letters (should KEEP backslash)

```rust
#[test]
fn test_backslash_letter_a() {
    let input = "\\a";
    // Expected: Para [ Str "\a" ]
    // Should NOT remove backslash for non-punctuation
    assert!(result.contains("\\"));
}

#[test]
fn test_backslash_letter_alpha() {
    let input = "\\alpha";
    // Expected: Para [ Str "\alpha" ]
    // Should NOT convert to math/LaTeX
}
```

### Category 3: Backslash in Context

```rust
#[test]
fn test_backslash_escape_in_text() {
    let input = "test \\* here";
    // Expected: Para [ Str "test" , Space , Str "*" , Space , Str "here" ]
}

#[test]
fn test_backslash_escape_no_spaces() {
    let input = "test\\*here";
    // Expected: Para [ Str "test*here" ]
    // Should merge into single Str
}

#[test]
fn test_backslash_escape_prevents_emphasis() {
    let input = "\\*not emphasized\\*";
    // Expected: Para [ Str "*not" , Space , Str "emphasized*" ]
    // Should NOT create Emph node
}
```

### Category 4: Double Backslash

```rust
#[test]
fn test_double_backslash() {
    let input = "\\\\";
    // This is tricky - Pandoc produces LineBreak
    // We might want: Para [ Str "\" ]
    // Need to decide: do we want Pandoc compat or our own behavior?
}
```

### Category 5: Edge Cases

```rust
#[test]
fn test_backslash_at_end_of_line() {
    let input = "test\\";
    // What should this do?
}

#[test]
fn test_multiple_backslashes() {
    let input = "\\*\\*\\*";
    // Expected: Para [ Str "***" ]
}

#[test]
fn test_backslash_in_code_span() {
    let input = "`\\*`";
    // Expected: Para [ Code (...) "\\*" ]
    // Backslashes inside code should NOT be processed
}
```

## Implementation Steps

### Step 1: Investigate Why Grammar Isn't Working

**Actions**:
1. Check if parser needs to be rebuilt:
   ```bash
   cd crates/tree-sitter-qmd/tree-sitter-markdown-inline
   tree-sitter generate
   tree-sitter build
   ```

2. Test if grammar produces nodes:
   ```bash
   echo '\*' | cargo run -- --verbose 2>&1 | grep backslash
   ```

3. If nodes still aren't produced, investigate grammar:
   - Check precedence rules
   - Check where backslash_escape is used
   - Verify regex pattern is correct

### Step 2: Fix Grammar (if needed)

If grammar needs fixing:
- Identify why `pandoc_str` is capturing backslash escapes
- Adjust precedence or ordering
- Rebuild parser
- Test that `backslash_escape` nodes are produced

### Step 3: Uncomment and Test Handler

1. Uncomment line 1037 in `treesitter.rs`:
   ```rust
   "backslash_escape" => process_backslash_escape(node, input_bytes, context),
   ```

2. Test basic functionality:
   ```bash
   echo '\*' | cargo run --
   # Should output: [ Para [Str "*"] ]
   ```

### Step 4: Verify Handler Behavior

The existing handler:
```rust
pub fn process_backslash_escape(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let text = node.utf8_text(input_bytes).unwrap();
    let content = &text[1..]; // remove the leading backslash
    // Returns IntermediateBaseText which becomes Str
}
```

**This should be correct** - it:
- ✅ Removes the backslash
- ✅ Returns just the escaped character
- ✅ Doesn't try to interpret LaTeX commands

### Step 5: Write Comprehensive Tests

Add all test cases from "Test Cases Design" section to `test_treesitter_refactoring.rs`.

### Step 6: Handle Edge Cases

Decisions needed:
1. **Double backslash `\\`**:
   - Pandoc: LineBreak
   - Our choice: `Str "\"` or LineBreak?
   - **Recommendation**: Match Pandoc (LineBreak) for compatibility

2. **Backslash + letter (e.g., `\a`)**:
   - Should keep backslash
   - Might need special handling if grammar produces backslash_escape node

3. **Merging adjacent Str nodes**:
   - `test\*here` should become `Str "test*here"` not three Strs
   - May need postprocessing

### Step 7: Integration Testing

Test with real-world examples:
```markdown
This is \*not emphasized\* text.
Use \[ and \] for literal brackets.
Escape \{ and \} for literal braces.
A backslash: \\.
```

## Decision Points

### 1. Double Backslash Behavior

**Options**:
- A: Match Pandoc - `\\` becomes LineBreak (hard line break)
- B: Literal backslash - `\\` becomes `Str "\"`

**Recommendation**: Option A (match Pandoc) for compatibility

### 2. Non-Punctuation Characters

**Options**:
- A: Keep backslash for non-punctuation (e.g., `\a` → `Str "\a"`)
- B: Always remove backslash

**Recommendation**: Option A (keep for non-punctuation) - matches Pandoc and prevents accidental interpretation

### 3. Grammar vs. Handler Fix

**If grammar doesn't produce nodes**:
- Option A: Fix grammar to produce backslash_escape nodes
- Option B: Handle in pandoc_str processing

**Recommendation**: Option A (fix grammar) - cleaner separation of concerns

## Expected Outcomes

After implementation:
- ✅ All backslash-escaped punctuation produces plain text
- ✅ `\*` → `Str "*"` (prevents emphasis)
- ✅ `\[` → `Str "["` (prevents links)
- ✅ `\{` → `Str "{"` (prevents attributes)
- ✅ No LaTeX processing (no RawInline nodes)
- ✅ Output matches Pandoc for all standard escapes
- ✅ All tests pass

## Risks and Mitigations

### Risk: Grammar might need significant changes
**Mitigation**: Start with investigation step, assess scope before implementing

### Risk: Breaking existing behavior
**Mitigation**: Comprehensive test suite, compare with Pandoc on many examples

### Risk: Edge cases might have unexpected behavior
**Mitigation**: Extensive testing, document any deviations from Pandoc

## Time Estimate

- Investigation (Steps 1-2): 1-2 hours
- Handler activation and testing (Step 3-4): 30 minutes
- Test writing (Step 5): 1-2 hours
- Edge case handling (Step 6): 1 hour
- Integration testing (Step 7): 30 minutes

**Total**: 4-6 hours

## Next Actions

1. **Investigate grammar** - Rebuild parser, test if nodes are produced
2. **Create beads issue** once investigation is complete
3. **Implement based on findings**

## References

- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`
- Common rules: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/../common/common.js`
- Handler: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/backslash_escape.rs`
- GFM Spec: https://github.github.com/gfm/#backslash-escapes
