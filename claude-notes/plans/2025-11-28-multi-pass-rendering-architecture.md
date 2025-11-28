# Multi-Pass Rendering Architecture for quarto-citeproc

**Issue**: k-444
**Created**: 2025-11-28
**Status**: Proposed

## Executive Summary

This document proposes a refactoring of quarto-citeproc to adopt a multi-pass rendering architecture similar to Pandoc's citeproc. The current implementation mixes evaluation and rendering, causing issues with delimiter handling, disambiguation, and collapse. The proposed architecture cleanly separates evaluation (building Output AST) from rendering (converting to final format), enabling proper punctuation handling and multi-pass disambiguation.

## Problem Statement

### Current Issues

1. **Delimiter handling bugs**: Multiple tests fail due to spurious delimiters (`;`, `.`) appearing between elements. Examples:
   - `disambiguate_AllNamesGenerally`: `"Cold;  (1980)"` instead of `"Cold (1980)"`
   - `bugreports_AuthorPosition`: `"K..  (2010)"` instead of `"K. (2010)"`

2. **Substitute inheritance**: 126 tests use `<substitute>` but only 20 pass. When `<names>` with `<name form="short">` falls back to substitute, the form isn't inherited.

3. **Year-suffix assignment**: Cannot properly assign "2020a", "2020b" because it requires rendering, detecting ambiguities, assigning suffixes, then re-rendering.

4. **Collapse with affixes**: Collapse features (`year`, `year-suffix`, `citation-number`) don't handle affixes correctly.

### Root Cause

Our current architecture renders to strings too early:

```rust
// Current: join_outputs inserts delimiter literals during evaluation
pub fn join_outputs(outputs: Vec<Output>, delimiter: &str) -> Output {
    let mut children = Vec::new();
    for (i, output) in non_null.into_iter().enumerate() {
        if i > 0 && !delimiter.is_empty() {
            children.push(Output::Literal(delimiter.to_string())); // ← Problem!
        }
        children.push(output);
    }
    Output::Formatted { formatting: Formatting::default(), children }
}
```

Pandoc keeps delimiters as metadata until render time:

```haskell
-- Pandoc: delimiter is part of Formatting, applied at render time
renderOutput opts locale (Formatted formatting xs) =
  addFormatting locale formatting . mconcat . fixPunct .
    (case formatDelimiter formatting of
       Just d  -> addDelimiters (fromText d)  -- ← Applied at render time
       Nothing -> id) . filter (/= mempty) $ map (renderOutput opts locale) xs
```

## Pandoc's Architecture

### Output Type (Types.hs:1510-1517)

```haskell
data Output a =
    Formatted Formatting [Output a]  -- Formatting + children
  | Linked Text [Output a]           -- Hyperlink
  | InNote (Output a)                -- Footnote
  | Literal a                        -- Actual content
  | Tagged Tag (Output a)            -- Semantic tag
  | NullOutput                       -- Empty
```

### Formatting Type (Types.hs:553-569)

```haskell
data Formatting = Formatting
  { formatLang           :: Maybe Lang
  , formatFontStyle      :: Maybe FontStyle
  , formatFontVariant    :: Maybe FontVariant
  , formatFontWeight     :: Maybe FontWeight
  , formatTextDecoration :: Maybe TextDecoration
  , formatVerticalAlign  :: Maybe VerticalAlign
  , formatPrefix         :: Maybe Text
  , formatSuffix         :: Maybe Text
  , formatDisplay        :: Maybe DisplayStyle
  , formatTextCase       :: Maybe TextCase
  , formatDelimiter      :: Maybe Text    -- ← KEY: delimiter is metadata
  , formatStripPeriods   :: Bool
  , formatQuotes         :: Bool
  , formatAffixesInside  :: Bool          -- ← Affix ordering control
  }
```

### CiteprocOutput Typeclass (Types.hs:199-216)

```haskell
class (Semigroup a, Monoid a, Show a, Eq a, Ord a) => CiteprocOutput a where
  toText                      :: a -> Text
  fromText                    :: Text -> a
  addFontVariant              :: FontVariant -> a -> a
  addFontStyle                :: FontStyle -> a -> a
  addFontWeight               :: FontWeight -> a -> a
  addTextDecoration           :: TextDecoration -> a -> a
  addVerticalAlign            :: VerticalAlign -> a -> a
  addTextCase                 :: Maybe Lang -> TextCase -> a -> a
  addDisplay                  :: DisplayStyle -> a -> a
  addQuotes                   :: a -> a
  movePunctuationInsideQuotes :: a -> a
  inNote                      :: a -> a
  mapText                     :: (Text -> Text) -> a -> a
  addHyperlink                :: Text -> a -> a
  localizeQuotes              :: Locale -> a -> a
```

### Render Pipeline (Types.hs:1604-1608)

```haskell
renderOutput opts locale (Formatted formatting xs) =
  addFormatting locale formatting   -- 4. Apply formatting
  . mconcat                         -- 3. Concatenate
  . fixPunct                        -- 2. Fix punctuation collisions
  . (case formatDelimiter formatting of
       Just d  -> addDelimiters (fromText d)
       Nothing -> id)               -- 1. Add delimiters
  . filter (/= mempty)              -- 0. Remove empty
  $ map (renderOutput opts locale) xs
```

### Multi-Pass Disambiguation (Eval.hs:424-512)

```haskell
disambiguateCitations style bibSortKeyMap citations = do
  -- Pass 1: Initial render
  allCites <- renderCitations citations'

  -- Pass 2: Name disambiguation (for non-ByCite rules)
  allCites' <- case disambiguateAddGivenNames strategy of
    Just ByCite -> return allCites
    Just rule   -> do
      -- Extract names from TagNames, compute hints, store in refmap
      -- ...
      renderCitations citations'  -- Re-render with hints

  -- Pass 3+: Iterative disambiguation
  case getAmbiguities allCites' of
    [] -> return ()
    ambiguities -> analyzeAmbiguities ...

  -- Final render
  renderCitations citations
```

### Disambiguation Loop (Eval.hs:531-553)

```haskell
analyzeAmbiguities mblang strategy cs ambiguities = do
  return ambiguities
    -- Try adding more names from et-al
    >>= tryAddNames >> refreshAmbiguities
    -- Try adding given names (ByCite only)
    >>= tryAddGivenNames >> refreshAmbiguities
    -- Try year suffixes
    >>= addYearSuffixes >> refreshAmbiguities
    -- Set disambiguate condition
    >>= tryDisambiguateCondition
```

## Proposed Architecture

### Phase 1: Add Delimiter to Formatting

**File**: `crates/quarto-csl/src/types.rs`

```rust
pub struct Formatting {
    // ... existing fields ...

    /// Delimiter between children (applied at render time)
    pub delimiter: Option<String>,

    /// Whether affixes go inside other formatting
    pub affixes_inside: bool,

    /// Language for text-case operations
    pub lang: Option<String>,
}
```

### Phase 2: Modify Output Construction

**File**: `crates/quarto-citeproc/src/output.rs`

Replace `join_outputs` with delimiter-aware `Output::formatted`:

```rust
impl Output {
    /// Create a formatted node with children and optional delimiter.
    /// The delimiter is stored in formatting and applied at render time.
    pub fn formatted_with_delimiter(
        formatting: Formatting,
        children: Vec<Output>,
        delimiter: Option<String>,
    ) -> Self {
        let children: Vec<_> = children.into_iter()
            .filter(|c| !c.is_null())
            .collect();
        if children.is_empty() {
            Output::Null
        } else {
            let mut fmt = formatting;
            fmt.delimiter = delimiter;
            Output::Formatted { formatting: fmt, children }
        }
    }
}
```

### Phase 3: Deferred Rendering

**File**: `crates/quarto-citeproc/src/output.rs`

Add render trait and smart punctuation handling:

```rust
/// Trait for output formats (plain text, HTML, Pandoc Inlines)
pub trait CslRenderer {
    type Output: Clone;

    fn literal(&self, text: &str) -> Self::Output;
    fn concat(&self, parts: Vec<Self::Output>) -> Self::Output;
    fn add_font_style(&self, style: FontStyle, content: Self::Output) -> Self::Output;
    fn add_font_weight(&self, weight: FontWeight, content: Self::Output) -> Self::Output;
    // ... other formatting methods ...
}

impl Output {
    pub fn render_with<R: CslRenderer>(&self, renderer: &R, locale: &Locale) -> R::Output {
        match self {
            Output::Null => renderer.literal(""),
            Output::Literal(s) => renderer.literal(s),
            Output::Formatted { formatting, children } => {
                // 1. Render children
                let rendered: Vec<_> = children.iter()
                    .map(|c| c.render_with(renderer, locale))
                    .filter(|r| !is_empty(r))
                    .collect();

                // 2. Add delimiters with smart punctuation
                let with_delims = match &formatting.delimiter {
                    Some(d) => add_delimiters_smart(&rendered, d),
                    None => rendered,
                };

                // 3. Concatenate
                let content = renderer.concat(with_delims);

                // 4. Apply formatting
                apply_formatting(renderer, formatting, content, locale)
            }
            // ... other variants ...
        }
    }
}

/// Smart delimiter insertion that handles punctuation collisions
fn add_delimiters_smart<T>(items: &[T], delimiter: &str) -> Vec<T> {
    // Implementation of Pandoc's addDelimiters + fixPunct logic
    // Handles cases like:
    // - Child ends with "." and delimiter is ";" → no extra punctuation
    // - Child ends with "!" and delimiter is "." → keep "!" only
    // etc.
}
```

### Phase 4: Evaluation Context for Substitutes

**File**: `crates/quarto-citeproc/src/eval.rs`

```rust
pub struct EvalContext<'a> {
    // ... existing fields ...

    /// Name format from parent <names> for substitute inheritance
    pub substitute_names_format: Option<&'a NamesElement>,

    /// Whether we're inside a substitute block
    pub in_substitute: bool,
}

fn evaluate_names(ctx: &mut EvalContext, names_el: &NamesElement) -> Result<Output> {
    // Try each variable...
    for var in &names_el.variables {
        if let Some(names) = ctx.reference.get_names(var) {
            if !names.is_empty() {
                return Ok(format_names(ctx, names, names_el));
            }
        }
    }

    // No names found - try substitute
    if let Some(ref substitute) = names_el.substitute {
        // Set substitute context for child evaluations
        let mut sub_ctx = ctx.clone();
        sub_ctx.in_substitute = true;
        sub_ctx.substitute_names_format = Some(names_el);

        for element in substitute {
            let sub_output = evaluate_element(&mut sub_ctx, element)?;
            if !sub_output.is_null() {
                return Ok(sub_output);
            }
        }
    }

    Ok(Output::Null)
}

fn format_names(ctx: &EvalContext, names: &[Name], names_el: &NamesElement) -> String {
    // Use substitute_names_format if available and this names_el has no <name>
    let effective_name_el = if ctx.in_substitute && names_el.name.is_none() {
        ctx.substitute_names_format
            .and_then(|parent| parent.name.as_ref())
    } else {
        names_el.name.as_ref()
    };

    // ... rest of formatting logic using effective_name_el ...
}
```

### Phase 5: Multi-Pass Disambiguation

**File**: `crates/quarto-citeproc/src/disambiguation.rs`

```rust
/// Disambiguate all citations using multi-pass algorithm.
pub fn disambiguate_citations(
    processor: &mut Processor,
    citations: &[Citation],
) -> Result<Vec<Output>> {
    let strategy = &processor.style.citation.disambiguation;

    // Pass 1: Initial render
    let mut outputs = render_all_citations(processor, citations)?;

    // Pass 2: Global name disambiguation (non-ByCite rules)
    if let Some(rule) = strategy.add_givenname {
        if rule != GivenNameDisambiguationRule::ByCite {
            apply_global_name_disambiguation(processor, &outputs, rule);
            outputs = render_all_citations(processor, citations)?;
        }
    }

    // Pass 3+: Iterative disambiguation for remaining ambiguities
    let mut ambiguities = find_ambiguities(&outputs);
    let max_iterations = 10; // Prevent infinite loops

    for _ in 0..max_iterations {
        if ambiguities.is_empty() {
            break;
        }

        // Try: add names (expand et-al)
        if strategy.add_names {
            try_add_names(processor, &ambiguities);
            outputs = render_all_citations(processor, citations)?;
            ambiguities = find_ambiguities(&outputs);
            if ambiguities.is_empty() { break; }
        }

        // Try: add given names (ByCite)
        if matches!(strategy.add_givenname, Some(GivenNameDisambiguationRule::ByCite)) {
            try_add_given_names_bycite(processor, &ambiguities);
            outputs = render_all_citations(processor, citations)?;
            ambiguities = find_ambiguities(&outputs);
            if ambiguities.is_empty() { break; }
        }

        // Try: year suffixes
        if strategy.add_year_suffix {
            assign_year_suffixes(processor, &ambiguities);
            outputs = render_all_citations(processor, citations)?;
            ambiguities = find_ambiguities(&outputs);
            if ambiguities.is_empty() { break; }
        }

        // Set disambiguate condition for remaining
        set_disambiguate_condition(processor, &ambiguities);
        break;
    }

    Ok(outputs)
}
```

## Implementation Plan

### Stage 1: Foundation (Est. 2-3 days)
1. Add `delimiter`, `affixes_inside`, `lang` to `Formatting` struct
2. Create `CslRenderer` trait with implementations for:
   - Plain text (for testing/debugging)
   - CSL HTML (for test suite)
   - Pandoc Inlines (for Quarto integration)
3. Implement `render_with` method on `Output`

### Stage 2: Delimiter Refactor (Est. 2-3 days)
1. Update `Output::formatted` to accept optional delimiter
2. Modify `evaluate_elements`, `evaluate_group` to use new API
3. Implement smart punctuation handling (`fixPunct` equivalent)
4. Update all callers of `join_outputs`

### Stage 3: Substitute Inheritance (Est. 1-2 days)
1. Add `substitute_names_format` and `in_substitute` to `EvalContext`
2. Update `evaluate_names` to set context when entering substitute
3. Update `format_names` to check substitute context

### Stage 4: Multi-Pass Disambiguation (Est. 2-3 days)
1. Refactor `disambiguate_citations` to use iterative approach
2. Implement `render_all_citations` helper
3. Ensure year-suffix assignment works with re-rendering
4. Test with disambiguation test suite

### Stage 5: Validation & Cleanup (Est. 1-2 days)
1. Run full CSL test suite
2. Fix any regressions
3. Remove deprecated code paths
4. Update documentation

## Expected Impact

### Tests Unlocked
- **Delimiter bugs**: ~20-30 tests across categories
- **Substitute inheritance**: ~50-100 tests (126 use substitute, 20 pass)
- **Year-suffix**: ~20-30 tests (magic, disambiguation)
- **Total estimated**: 80-150 additional tests

### Architecture Benefits
1. **Cleaner separation** between evaluation and rendering
2. **Multiple output formats** supported cleanly
3. **Proper punctuation handling** at render time
4. **Multi-pass disambiguation** matches CSL spec
5. **Easier debugging** - can inspect Output AST before rendering

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Regressions in passing tests | High | Run full test suite at each stage; keep old paths until validated |
| Performance impact from re-rendering | Medium | Profile and optimize; cache where possible |
| Complexity increase | Medium | Good documentation; staged rollout |
| Edge cases in punctuation handling | Medium | Port Pandoc's fixPunct logic carefully |

## References

- Pandoc citeproc source: `external-sources/citeproc/src/Citeproc/`
- Key files:
  - `Types.hs` - Output type, Formatting, CiteprocOutput typeclass
  - `Eval.hs` - evalStyle, disambiguateCitations, renderOutput
  - `CslJson.hs` - HTML output format implementation
- CSL Spec: https://docs.citationstyles.org/en/stable/specification.html
