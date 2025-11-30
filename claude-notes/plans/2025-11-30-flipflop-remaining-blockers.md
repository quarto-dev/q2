# Flip-Flop Tests: Remaining Blockers

**Date**: 2025-11-30
**Related to**: k-432 (flip-flop formatting)

## Context

The core nodecor (no decoration) rendering was fixed in `extract_display_regions` (output.rs:1082-1104). However, several flip-flop tests remain blocked by three separate architectural issues.

## Blocker 1: affixesInside Flag

### Affected Tests
- `flipflop_ItalicsWithOk`
- `flipflop_ItalicsWithOkAndTextcase`
- (Also blocks `simplespace_case1` which is already deferred)

### Problem

Suffixes appear **inside** formatting instead of **outside**:

```
Expected: <i>Lessard <span style="font-style:normal;">v.</span> Schmidt</i>. 1972
Actual:   <i>Lessard <span style="font-style:normal;">v.</span> Schmidt. </i>1972
                                                                      ^^^
                                                          suffix inside italics
```

### Haskell Reference

In Pandoc's citeproc (Types.hs:218-242), the `formatAffixesInside` flag controls affix placement:

```haskell
addFormatting locale f x =
  ...
  (if affixesInside then id else addPrefix . addSuffix) .  -- affixes OUTSIDE
  ...
  (if affixesInside then addPrefix . addSuffix else id) .  -- affixes INSIDE
  ...
 where
  affixesInside = formatAffixesInside f
```

- When `affixesInside=true`: affixes go inside formatting (for layout elements)
- When `affixesInside=false`: affixes go outside formatting (for regular text elements)

### Our Implementation Gap

In our Rust implementation (output.rs:889-932), we always apply prefix/suffix in step 2, before font_style in step 3. This means affixes always end up inside formatting.

We don't have a `affixes_inside` flag in our `Formatting` struct or the logic to conditionally place affixes.

### Required Changes

1. Add `affixes_inside: bool` field to `quarto_csl::Formatting`
2. Set it to `true` for layout elements, `false` for other elements during parsing
3. Modify `to_inlines` in output.rs to conditionally apply affixes before or after formatting based on this flag

### Effort Estimate

Medium - requires changes across CSL parsing and output rendering.

---

## Blocker 2: HTML Entity Parsing in Text Values

### Affected Tests
- `flipflop_BoldfaceNodeLevelMarkup`

### Problem

HTML entities in `<text value="..."/>` should be unescaped and interpreted as markup:

```xml
<text value="&#60;b&#62;friend&#60;/b&#62;"/>
```

- `&#60;` = `<`
- `&#62;` = `>`

So this should render `<b>friend</b>` as bold markup.

```
Expected: <b>Speak, <span style="font-weight:normal;">friend</span>, and enter.</b>
Actual:   <b>Speak, &#60;b&#62;friend&#60;/b&#62;, and enter.</b>
```

### Haskell Reference

In Pandoc's citeproc, the `parseCslJson` function (Types.hs:1820-1825) parses HTML-like markup from text values. The parsing happens at evaluation time when text values are processed.

### Our Implementation Gap

We do parse HTML markup via `parse_csl_rich_text()` for **variable values**, but not for **literal text values** from `<text value="..."/>`.

In eval.rs, `TextSource::Value` handling (around line 1075) doesn't call `parse_csl_rich_text`:

```rust
TextSource::Value { value } => {
    Output::literal(value.clone())  // No rich text parsing!
}
```

### Required Changes

1. In `evaluate_text()`, when handling `TextSource::Value`, call `parse_csl_rich_text()` instead of `Output::literal()`
2. Ensure HTML entity decoding happens before parsing (may already be handled by XML parser)

### Effort Estimate

Low - simple change to call `parse_csl_rich_text` for text values.

---

## Blocker 3: Quote/Apostrophe Handling in Tags

### Affected Tests
- `flipflop_ApostropheInsideTag`
- `flipflop_QuotesNodeLevelMarkup`
- `flipflop_QuotesInFieldNotOnNode`
- `flipflop_OrphanQuote`
- `flipflop_SingleBeforeColon`

### Problem

Smart quote and apostrophe handling isn't working correctly inside markup tags:

```
Expected: l'''  (l + right single quote + right single quote + right single quote)
Actual:   l'    (l + apostrophe)
```

### Haskell Reference

In Pandoc's citeproc (Types.hs:1850-1868), the `pCslJson` parser handles apostrophes specially:

```haskell
isApostrophe '\'' = True
isApostrophe '''  = True
isApostrophe _    = False

pCslText = fromText . addNarrowSpace <$>
  (  do t <- P.takeWhile1 (\c -> isAlphaNum c && not (isSpecialChar c))
        -- apostrophe
        P.option t $ do _ <- P.satisfy isApostrophe
                        t' <- P.takeWhile1 isAlphaNum
                        return (t <> "'" <> t')
  ...
```

The parser also has special handling for quotes in `pCslQuoted` and quote localization in `convertQuotes`.

### Our Implementation Gap

Our `parse_csl_rich_text()` function doesn't have sophisticated quote/apostrophe handling. It treats quotes as simple formatting markers without:
- Smart quote conversion (straight to curly)
- Locale-aware quote styles
- Apostrophe vs. single quote distinction

### Required Changes

1. Enhance `parse_csl_rich_text()` to handle apostrophes within words
2. Implement smart quote conversion based on locale
3. Handle nested quote levels (outer vs inner quotes)

### Effort Estimate

Medium-High - quote handling is complex and locale-dependent.

---

## Summary Table

| Blocker | Tests Affected | Effort | Priority |
|---------|---------------|--------|----------|
| affixesInside | 2 | Medium | Medium |
| HTML entity parsing | 1 | Low | Low |
| Quote/apostrophe | 5 | Medium-High | Medium |

## Recommendation

1. **HTML entity parsing** is the easiest fix - could be done quickly
2. **affixesInside** is a more fundamental architecture issue but well-understood
3. **Quote handling** is the most complex and should be tackled separately

These should be tracked as separate issues since they're distinct from the nodecor fix and affect other test categories as well.
