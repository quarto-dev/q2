# k-310 Raw Attribute Fix - November 2, 2025

## Problem
Input: `# Hello {=world}`
Expected: Parse error (raw attributes not allowed in QMD)
Actual (tree-sitter-qmd-2): Panics with "Expected Attr in attribute, got IntermediateRawFormat"

## Kyoto Branch Behavior
On the kyoto branch, the same input properly returns a parse error:
```
Error: Parse error
   ╭─[<stdin>:1:10]
   │
 1 │ # Hello {=world}
   │          ──┬──
   │            ╰───── unexpected character or token here
───╯
```

Verbose output shows:
```
detect_error lookahead:raw_specifier
skip_token symbol:raw_specifier
```

## Grammar Investigation

### kyoto/common/common.js (lines 107-111):
```javascript
_qmd_attribute: $ => choice(
  $.language_attribute,
  $.raw_attribute,      // <-- INCLUDES raw_attribute
  $.commonmark_attribute
),
```

### kyoto/tree-sitter-markdown/grammar.js (lines 119-124):
```javascript
_atx_heading1: $ => prec(1, seq(
    $.atx_h1_marker,
    optional($._atx_heading_content),
    optional(alias($._qmd_attribute, $.attribute)),  // <-- uses _qmd_attribute
    $._newline
)),
```

## Key Finding

The grammar DOES allow `raw_attribute` as part of `_qmd_attribute`, but tree-sitter's error detection marks it as an error in this context and skips it (`skip_token symbol:raw_specifier`).

This suggests the error detection is happening in the tree-sitter grammar's conflict resolution or in the scanner, NOT in the Rust code.

## Root Cause Found!

### Grammar Comparison

**kyoto branch (OLD):**
```javascript
_atx_heading1: $ => prec(1, seq(
    $.atx_h1_marker,
    optional($._atx_heading_content),
    optional(alias($._qmd_attribute, $.attribute)),  // ← attribute at heading level
    $._newline
)),
```

**tree-sitter-qmd-2 branch (NEW):**
```javascript
_atx_heading1: $ => prec(1, seq(
    $.atx_h1_marker,
    optional($._atx_heading_content),  // ← attributes inside content as inlines
    choice($._newline, $._eof)
)),
```

### Parse Tree for `# Hello {=world}`:
```
atx_heading:
  pandoc_str: "Hello"
  pandoc_space
  attribute_specifier: {=world}  ← parsed as inline content!
```

### The Problem

In the NEW grammar:
1. `attribute_specifier` is parsed as part of `$._inlines` (inline content)
2. It gets processed by `atx_heading.rs:53-60`
3. Code expects `IntermediateAttr` but gets `IntermediateRawFormat`
4. Panics instead of returning an error

In the OLD grammar:
- Attributes were at the heading level, handled by the grammar
- Grammar itself rejected raw attributes in that context
- Tree-sitter emitted ERROR nodes

## The Fix

In `atx_heading.rs`, add handling for `IntermediateRawFormat`:

```rust
} else if node_kind == "attribute" || node_kind == "attribute_specifier" {
    match child {
        PandocNativeIntermediate::IntermediateAttr(inner_attr, inner_attr_source) => {
            attr = inner_attr;
            attr_source = inner_attr_source;
        }
        PandocNativeIntermediate::IntermediateRawFormat(format, range) => {
            // ERROR: Raw attributes are not allowed in QMD
            writeln!(buf, "Error: Raw attributes like {{={}}} are not allowed in headers", format).unwrap();
            // Don't set attr - leave it empty
        }
        _ => {
            panic!("Expected Attr or RawFormat in attribute, got {:?}", child);
        }
    }
}
```

But this just writes to buf - we need to actually fail the parse. Need to check how errors are properly reported.
