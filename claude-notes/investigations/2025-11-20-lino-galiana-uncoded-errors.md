# Investigation: Uncoded Errors in lino-galiana Corpus

**Date**: 2025-11-20
**Corpus**: external-sites/lino-galiana (170 .qmd files)
**Uncoded errors remaining**: 18 files

## Summary

After fixing Q-2-7 to handle apostrophe-backtick sequences in headings, we reduced uncoded errors from 33 to 18 files. This investigation examines the remaining 18 files to identify patterns that could be supported with error codes or grammar fixes.

## Error Patterns Found

### Pattern 1: Emoji Characters (High Priority - Grammar Bug)

**Affected files**:
- `04_webscraping/_exo1_solution.qmd` (23 errors)
- `04_webscraping/_exo2b_suite.qmd` (2 errors)
- `git/exogit.qmd` (4 errors)
- Others with numbered emojis

**Description**: Multi-byte emoji characters (particularly numbered emoji like 1️⃣, 2️⃣, 3️⃣, 4️⃣) cause parse errors.

**Example**:
```markdown
::: {.content-visible when-profile="fr"}
1️⃣ Trouver le tableau
:::
```

**Error**: `Parse error: unexpected character or token here` at the emoji position (bytes: `31 efb8 8fe2 83a3` for 1️⃣)

**Analysis**:
- The emoji is a composition: digit + variation selector (U+FE0F) + combining enclosing keycap (U+20E3)
- Simplified emoji test cases parse correctly
- Error only occurs in actual corpus files, suggesting context-specific issue
- Likely a tree-sitter grammar issue with multi-byte UTF-8 sequences in certain contexts

**Recommendation**: **Grammar bug** - needs tree-sitter grammar investigation. The grammar should handle multi-byte UTF-8 characters properly in all inline contexts.

### Pattern 2: Inline Code Execution in Image URLs (Document Error)

**Affected files**:
- `manipulation/04_api/_exo3_solution.qmd`

**Description**: Attempting to use Quarto inline code execution inside image URLs.

**Example**:
```markdown
![](`{python} url_image`)
```

**Error**: `Parse error: unexpected character or token here` at the backtick

**Analysis**: This syntax is invalid - you cannot use inline code execution `` `{python}` `` inside an image URL. The correct Quarto syntax would be:
```markdown
![](`{python} url_image`)  <!-- This is wrong -->

<!-- Correct alternatives: -->
```{python}
#| output: asis
print(f"![]({url_image})")
```
```

**Recommendation**: **Document error** - could add an error code to detect and explain this specific misuse. Error code could be something like "Q-2-XX: Inline code execution not allowed in image URLs".

### Pattern 3: Curly Braces in URLs/Markdown Text

**Affected files**:
- `manipulation/04_webscraping/_exo2b_suite.qmd`
- `manipulation/05_parquet_s3.qmd`

**Description**: URLs or text containing curly braces like `{pokemon}` or `{python}` that might be confused with Quarto syntax.

**Example**:
```markdown
Les URL des images prennent la forme "https://example.com/{pokemon}.jpg"
```

**Error**: Parse errors around the curly braces

**Analysis**: The parser might be interpreting `{pokemon}` as an attempted code execution or attribute syntax. Need to verify if this is actually the source of errors or if it's cascading from earlier parse failures.

**Recommendation**: Needs more investigation. Could be:
- Grammar issue with curly braces in plain text
- Cascading errors from other issues
- Edge case in string/URL parsing

### Pattern 4: Footnote Definitions (Potentially Grammar Bug)

**Affected files**:
- `getting-started/intro/_pourquoi_python_data.qmd` (4 errors)
- `getting-started/intro/_intro.qmd`
- `NLP/01_intro/exercise2.qmd`
- Others

**Description**: Errors occurring at or near footnote definitions.

**Example**:
```markdown
Some text[^scikit-and-co].

[^scikit-and-co]:
    [`Scikit Learn`](https://scikit-learn.org/stable/) est une librairie...
```

**Error**: `Parse error: unexpected character or token here` at various positions near footnotes

**Analysis**:
- Isolated footnote tests parse correctly
- Error location (offset 1442) is right at the start of footnote content
- May be related to:
  - Indented blocks following footnote markers
  - Backticks/links inside footnote content
  - Interaction between footnotes and other inline elements

**Recommendation**: **Not a bug** - this syntax is **not supported** in quarto-markdown. We don't support pure-indentation blocks. Need to add error code with diagnostic explaining this and suggesting alternatives.

**Beads issue**: k-367

### Pattern 5: Link Target Attributes

**Affected files**:
- `visualisation/01_matplotlib/_exo4_solution.qmd`

**Description**: Parse errors near link constructs with target attributes.

**Example** (from error context):
```markdown
when-adjustments.html){target='_blank'}) when we have reversed the axes.
```

**Error**: `Parse error: unexpected character or token here` at offset 6536-6552

**Analysis**: The error spans 16 bytes, suggesting it's hitting a multi-character construct. The `{target='_blank'}` attribute syntax after a link might not be correctly recognized in all contexts.

**Recommendation**: **Possible grammar bug** - verify if link attributes are properly supported in all contexts. Could also be cascading from earlier errors in the file.

### Pattern 6: Unicode/Accented Characters in Specific Contexts

**Affected files**:
- Multiple French-language files

**Description**: Parse errors in French text with accented characters (é, è, à, etc.).

**Example contexts**:
- `développeur.euse(s)`
- Various French paragraphs with accents

**Error**: Parse errors at seemingly random positions

**Analysis**: UTF-8 encoded accented characters should be handled correctly by the parser. These errors are likely:
- Cascading from earlier parse failures
- Context-specific issues (e.g., accents inside specific markdown constructs)
- Not actually caused by the accents themselves

**Recommendation**: Low priority - likely not the root cause. Focus on other patterns first.

## Priority Recommendations

### High Priority

1. **Emoji Support** (Pattern 1)
   - Type: Grammar Bug
   - Impact: 3+ files, 29+ errors
   - Action: Fix tree-sitter grammar to properly handle multi-byte UTF-8 emoji sequences
   - Difficulty: Medium-High (requires tree-sitter grammar work)

2. **Footnote Parsing** (Pattern 4)
   - Type: Likely Grammar Bug
   - Impact: 5+ files
   - Action: Create detailed minimal reproductions, fix grammar if needed
   - Difficulty: Medium (requires investigation first)

### Medium Priority

3. **Inline Code in Image URLs** (Pattern 2)
   - Type: Document Error (could add error code)
   - Impact: 1 file
   - Action: Add error code Q-2-XX with helpful message
   - Difficulty: Low (just add error code to catalog)

4. **Link Target Attributes** (Pattern 5)
   - Type: Possible Grammar Bug
   - Impact: 1 file
   - Action: Verify attribute syntax support, fix if needed
   - Difficulty: Medium

### Low Priority

5. **Curly Braces in Text** (Pattern 3)
   - Type: Unclear
   - Impact: 2 files
   - Action: Investigate after fixing higher priority items
   - Difficulty: Unknown

6. **Unicode/Accents** (Pattern 6)
   - Type: Likely Cascading
   - Impact: Multiple files (but not root cause)
   - Action: Re-evaluate after fixing other patterns
   - Difficulty: N/A

## Testing Strategy

For each pattern:

1. Create minimal reproduction in `tests/` directory
2. Write test that should **fail** with current grammar
3. Fix grammar/add error code
4. Verify test now passes
5. Re-run corpus validation to measure improvement

## Beads Issues Created

- **k-368**: Fix parser handling of multi-byte emoji characters (P1, bug)
  - Impact: ~29+ errors across 3+ files
  - **HIGHEST PRIORITY**

- **k-367**: Add error diagnostic for indented footnote content (P1, task)
  - Impact: ~5+ files
  - Note: Indented footnotes are **not supported** in qmd (we don't support pure-indentation blocks)

- **k-369**: Add error diagnostic for inline code execution in image URLs (P2, task)
  - Impact: 1 file
  - Easy win - just add error code

## Next Steps

1. **Immediate**: Fix emoji support in tree-sitter grammar (k-368 - highest impact)
2. **Quick win**: Add error code for inline-code-in-image-URL pattern (k-369 - low effort)
3. **Next**: Add error code for indented footnote pattern (k-367)
4. **Future**: Address remaining patterns after main issues fixed

## Notes

- Total progress so far: 33 uncoded → 18 uncoded (45% reduction from Q-2-7 fix)
- Clean files: 95 → 117 (23% increase)
- Most uncoded errors appear to be grammar bugs rather than document errors
- Emoji issue is highest impact and should be prioritized
