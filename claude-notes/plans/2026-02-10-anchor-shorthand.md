# Anchor Shorthand `<#foo>` Support

## Overview

Add support for `<#foo>` as a shorthand notation for `[foo](#foo){.anchor}` in qmd. This involves two changes:

1. **Reader (tree-sitter→Pandoc AST)**: Intercept `html_element` nodes matching `<#...>` and convert them to `Link` nodes with class `.anchor` instead of `RawInline("html", ...)`.
2. **Writer (Pandoc AST→qmd)**: Detect `Link` nodes matching the anchor pattern and emit `<#foo>` instead of `[foo](#foo){.anchor}`.

## Design

### Reader side

Currently, `<#foo>` is parsed by tree-sitter as an `html_element` node, which gets converted to `RawInline("html", "<#foo>")` at `treesitter.rs:1048-1073`.

The change: In the `"html_element"` match arm, before creating the `RawInline`, check if the trimmed text matches the pattern `<#identifier>`. If so, produce a `Link` node instead:

```
Link {
    attr: ("", ["anchor"], {}),
    content: [Str("foo")],
    target: ("#foo", ""),
}
```

This is analogous to how `uri_autolink.rs` converts `<https://...>` into `Link` nodes with class `"uri"`.

**Pattern matching**: The text inside `<#...>` must be non-empty and contain no whitespace or `>` characters. Strip `<#` and `>`, use whatever's in between as both the link text and the fragment target. `<#>` (empty) silently remains a RawInline. Numeric-only IDs like `<#123>` are allowed.

**No tree-sitter parser changes needed** — we intercept at the AST conversion layer.

### Writer side

In `write_link()` at `qmd.rs:1348-1367`, add a check before the normal link serialization: if the link has exactly one class `"anchor"`, no id, no key-value pairs, no title, content is a single `Str` node, and target URL is `#` + that same string, emit `<#text>` instead of the standard `[text](#text){.anchor}`.

**Normalization rule**: The writer normalizes to shorthand only when the link text exactly matches the fragment identifier. So `[foo](#foo){.anchor}` → `<#foo>`, but `[Foo](#foo){.anchor}` stays as `[Foo](#foo){.anchor}` because "Foo" ≠ "foo".

### What this means for the Pandoc AST

The Pandoc JSON representation of `<#foo>` will be:
```json
{
  "t": "Link",
  "c": [
    ["", ["anchor"], []],
    [{"t": "Str", "c": "foo"}],
    ["#foo", ""]
  ]
}
```

This means:
- External tools can produce anchor links by creating this AST pattern
- The roundtrip is: `<#foo>` → AST → `<#foo>` (perfect roundtrip)
- `[foo](#foo){.anchor}` written explicitly normalizes to `<#foo>` (only when link text == fragment id)
- `[Foo](#foo){.anchor}` stays as-is (link text "Foo" ≠ fragment id "foo")

## Work Items

### Phase 1: Tests (write first, verify they fail)

- [x] Add JSON snapshot tests: anchor-shorthand-01 through 06 (simple, in-paragraph, hyphenated, underscored, numeric, empty)
- [x] Add QMD snapshot tests: anchor-shorthand-01 through 04 (simple, in-paragraph, explicit long-form normalization, case-mismatch no-normalize)
- [x] Add roundtrip tests: anchor_shorthand_simple.qmd, anchor_shorthand_variants.qmd
- [x] Verify all tests fail with current code (confirmed: snapshot tests fail with RawInline output)

### Phase 2: Reader implementation

- [x] Add `parse_anchor_shorthand()` helper function in `treesitter.rs`
- [x] Add anchor detection logic in `treesitter.rs` `"html_element"` arm
- [x] Create `Link` node with `.anchor` class, `Str` content, and `#identifier` target
- [x] Handle leading/trailing whitespace (emit Space nodes, like `uri_autolink.rs`)
- [x] Suppress the Q-2-9 warning for anchor shorthand (falls into the `if` branch, bypasses warning)

### Phase 3: Writer implementation

- [x] Add `anchor_shorthand_id()` helper in `qmd.rs`
- [x] Add anchor shorthand detection in `write_link()` in `qmd.rs`
- [x] Emit `<#identifier>` for matching links
- [x] Non-matching `.anchor` links (case mismatch, extra attrs) serialize normally

### Phase 4: Full verification

- [x] Run `cargo nextest run --workspace` — 6394 tests pass, 0 failures
- [x] Run `cargo build --workspace` — clean build
- [x] Manually verify: `echo '<#foo>' | cargo run -p pampa --bin pampa -- -t json` — produces Link with .anchor class
- [x] Manually verify roundtrip: `<#foo>` → JSON → `<#foo>` — perfect roundtrip
