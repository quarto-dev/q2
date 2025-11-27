# Citeproc Output Architecture Analysis

**Date:** 2025-11-26
**Related Issue:** k-410

## How CiteprocOutput Works

### The Typeclass

Citeproc is parameterized on an output type `a` via the `CiteprocOutput` typeclass:

```haskell
class (Semigroup a, Monoid a, Show a, Eq a, Ord a) => CiteprocOutput a where
  toText                      :: a -> Text
  fromText                    :: Text -> a
  addFontVariant              :: FontVariant -> a -> a   -- small-caps
  addFontStyle                :: FontStyle -> a -> a     -- italic, oblique
  addFontWeight               :: FontWeight -> a -> a    -- bold
  addTextDecoration           :: TextDecoration -> a -> a -- underline
  addVerticalAlign            :: VerticalAlign -> a -> a  -- superscript, subscript
  addTextCase                 :: Maybe Lang -> TextCase -> a -> a
  addDisplay                  :: DisplayStyle -> a -> a
  addQuotes                   :: a -> a
  movePunctuationInsideQuotes :: a -> a
  inNote                      :: a -> a
  mapText                     :: (Text -> Text) -> a -> a
  addHyperlink                :: Text -> a -> a
  localizeQuotes              :: Locale -> a -> a
```

### Two Implementations in Haskell Citeproc

**1. Pandoc Inlines** (`Citeproc.Pandoc`):
```haskell
instance CiteprocOutput Inlines where
  fromText t = B.text t
  addFontStyle ItalicFont = B.emph
  addFontStyle ObliqueFont = B.emph
  addFontWeight BoldWeight = B.strong
  addFontVariant SmallCapsVariant = B.smallcaps
  addVerticalAlign SubAlign = B.subscript
  addVerticalAlign SupAlign = B.superscript
  addHyperlink url = B.link url ""
  addDisplay DisplayBlock = B.spanWith ("",["csl-block"],[])
  addDisplay DisplayLeftMargin = B.spanWith ("",["csl-left-margin"],[])
  addQuotes = B.spanWith ("",["csl-quoted"],[])
  -- etc.
```

**2. CslJson** (`Citeproc.CslJson`) - simple markup for testing:
```haskell
instance CiteprocOutput (CslJson Text) where
  fromText t = CslText t
  addFontStyle ItalicFont = CslItalic
  addFontWeight BoldWeight = CslBold
  -- etc.
```

### Data Flow

```
CSL Style (XML) ──parse──► Style a
                              │
References (JSON) ────────────┤
                              │
Citations (JSON) ─────────────┤
                              ▼
                         citeproc()
                              │
                              ▼
                         Result a
                         ├── citations: [a]      (formatted in-text citations)
                         └── bibliography: [(id, a)]  (formatted bib entries)
                              │
                              │ when a = Inlines
                              ▼
                    Pandoc AST (Inlines)
                              │
                              ▼
                    Pandoc Writers (HTML, LaTeX, DOCX, etc.)
```

### Key Insight

Citeproc outputs **structured markup**, not final text. The CiteprocOutput methods produce intermediate representations that downstream writers convert to final formats.

When `a = Inlines`:
- `addFontStyle ItalicFont` → `Emph [...]`
- `addFontWeight BoldWeight` → `Strong [...]`
- `addHyperlink url` → `Link attr [...] (url, "")`

Pandoc's writers then convert these to format-specific output:
- HTML: `<em>`, `<strong>`, `<a>`
- LaTeX: `\emph{}`, `\textbf{}`, `\href{}{}`
- DOCX: Word formatting runs

---

## Why the Typeclass? (Historical Reasons)

### 1. Decoupling from Pandoc

The citeproc library was designed to be **independent of Pandoc**. From the module docs:

> "The library may be used with any structured format that defines these operations."

This allows citeproc to be used by tools that don't use Pandoc's AST.

### 2. Testing Convenience

The `CslJson` implementation provides a simpler format for test assertions. Comparing:

```
CslItalic (CslText "hello")
```

is easier than comparing full Pandoc AST:

```haskell
Many (fromList [Emph [Str "hello"]])
```

### 3. Historical Architecture

The original `pandoc-citeproc` was tightly coupled to Pandoc. When it was rewritten as standalone `citeproc`, the typeclass was introduced for generality.

---

## For Our Rust Port: Direct Pandoc AST Output

### The Case for Skipping CiteprocOutput

The CiteprocOutput methods map almost 1:1 to Pandoc AST constructors:

| CiteprocOutput Method | Pandoc AST |
|----------------------|------------|
| `addFontStyle Italic` | `Inline::Emph` |
| `addFontWeight Bold` | `Inline::Strong` |
| `addFontVariant SmallCaps` | `Inline::SmallCaps` |
| `addVerticalAlign Sub` | `Inline::Subscript` |
| `addVerticalAlign Sup` | `Inline::Superscript` |
| `addHyperlink url` | `Inline::Link` |
| `addDisplay *` | `Inline::Span` with class |
| `addQuotes` | `Inline::Span` with class |

Given this tight mapping, **we could just produce `Vec<Inline>` directly**.

### Benefits of Direct Output

1. **Simpler code**: No trait indirection, no type parameter threading
2. **Easier to understand**: Direct construction of familiar types
3. **Fewer abstractions**: One less concept to maintain
4. **Better error messages**: Rust errors reference concrete types

### Potential Concerns (and Mitigations)

**Concern 1: Testing**

> "CslJson is simpler for test assertions"

**Mitigation:** Create a `stringify(inlines: &[Inline]) -> String` function for test assertions. We already have similar utilities in the codebase.

```rust
#[test]
fn test_citation_output() {
    let result = process_citation(&style, &refs, &citation);
    assert_eq!(stringify(&result), "Smith, 2020");
}
```

**Concern 2: Flexibility**

> "What if we need non-Pandoc output?"

**Mitigation:**
- We're building for quarto-markdown-pandoc specifically
- If we ever need other formats, we can add a trait layer then (YAGNI)
- Pandoc AST can be serialized to many formats anyway

**Concern 3: Display styles**

CSL has display styles (block, left-margin, right-inline, indent) that don't map to semantic Pandoc types. Haskell citeproc uses `Span` with CSS classes.

**Mitigation:** Same approach works:
```rust
fn add_display(style: DisplayStyle, content: Inlines) -> Inline {
    let class = match style {
        DisplayStyle::Block => "csl-block",
        DisplayStyle::LeftMargin => "csl-left-margin",
        DisplayStyle::RightInline => "csl-right-inline",
        DisplayStyle::Indent => "csl-indent",
    };
    Inline::Span(Span {
        attr: (String::new(), vec![class.to_string()], vec![]),
        content,
        ..
    })
}
```

### Recommended Approach

**Start with direct Pandoc AST output.** Create helper functions that mirror the CiteprocOutput methods but produce Inlines directly:

```rust
// src/output.rs

pub fn italic(content: Inlines) -> Inline {
    Inline::Emph(Emph { content, source_info: SourceInfo::none() })
}

pub fn bold(content: Inlines) -> Inline {
    Inline::Strong(Strong { content, source_info: SourceInfo::none() })
}

pub fn smallcaps(content: Inlines) -> Inline {
    Inline::SmallCaps(SmallCaps { content, source_info: SourceInfo::none() })
}

pub fn link(url: &str, content: Inlines) -> Inline {
    Inline::Link(Link {
        attr: default_attr(),
        content,
        target: (url.to_string(), String::new()),
        source_info: SourceInfo::none(),
        ..Default::default()
    })
}

pub fn span_with_class(class: &str, content: Inlines) -> Inline {
    Inline::Span(Span {
        attr: (String::new(), vec![class.to_string()], vec![]),
        content,
        source_info: SourceInfo::none(),
        ..Default::default()
    })
}

// For testing
pub fn stringify(inlines: &[Inline]) -> String {
    inlines.iter().map(inline_to_text).collect()
}
```

If we later discover a genuine need for the trait abstraction, we can refactor. But starting simple is better than adding abstraction we may not need.

---

## Summary

| Approach | Pros | Cons |
|----------|------|------|
| **CiteprocOutput trait** | Flexible, matches Haskell design, better test format | More complex, type parameter threading, extra abstraction |
| **Direct Pandoc AST** | Simpler, fewer concepts, direct mapping | Less flexible (but we don't need flexibility) |

**Recommendation:** Use direct Pandoc AST output for the Rust port. Add trait abstraction only if a concrete need arises.

---

## Deep Dive: Why addQuotes Uses Span, Not Quoted

Yes, `addQuotes` uses `Span` with class `"csl-quoted"` instead of Pandoc's `Quoted` type:

```haskell
addQuotes = B.spanWith ("",["csl-quoted"],[])
```

This is intentional. CSL quote handling has a two-phase design:

### Phase 1: Mark quoted content

During CSL processing, `addQuotes` marks content as "needs quotes" without committing to specific quote characters. The `Span` with `"csl-quoted"` class acts as a placeholder.

### Phase 2: Localize quotes

Later, `localizeQuotes` (which calls `convertQuotes`) traverses the AST and:
1. Finds `Span ("",["csl-quoted"],[])` elements
2. Looks up locale-appropriate quote characters (e.g., English uses `"` and `"`, French uses `«` and `»`)
3. Inserts the actual quote characters as `Str` elements around the content
4. Handles flip-flopping for nested quotes (outer uses double, inner uses single, etc.)

From `Citeproc/Pandoc.hs:97-99`:
```haskell
go q (Span ("",["csl-quoted"],[]) ils) =
  Span ("",["csl-quoted"],[])
    (Str (oq q) : map (go (flipflop q)) ils ++ [Str (cq q)])
```

Using `Quoted` directly wouldn't work because:
- `Quoted` requires knowing SingleQuote vs DoubleQuote upfront
- CSL needs to flip-flop based on nesting depth
- Locale-specific characters are only known at localization time

**For our Rust port:** Use the same approach—`Span` with class `"csl-quoted"`, plus a `localize_quotes` post-processing step.

---

## Deep Dive: CslJson's Role in Testing

### The CslJson Data Type

CslJson is a recursive algebraic data type that preserves formatting structure:

```haskell
data CslJson a =
     CslText a
   | CslEmpty
   | CslConcat (CslJson a) (CslJson a)
   | CslQuoted (CslJson a)
   | CslItalic (CslJson a)
   | CslBold   (CslJson a)
   | CslSmallCaps (CslJson a)
   | CslSup       (CslJson a)
   | CslSub       (CslJson a)
   | CslDiv Text  (CslJson a)
   | CslLink Text (CslJson a)
   -- etc.
```

### How Tests Actually Work

Looking at `test/Spec.hs`, the test workflow is:

1. **Load test**: Parse as `CiteprocTest (CslJson Text)` (line 76)
2. **Run citeproc**: Produces `Result (CslJson Text)` (line 117-118)
3. **Render output**: Convert to HTML text via `renderCslJson True loc` (line 126, 140)
4. **Compare**: Plain text comparison against expected result (line 185)

The test files (e.g., `affix_SpaceWithQuotes.txt`) have a `RESULT` section with **plain HTML text**, not structured CslJson:

```
>>==== RESULT ====>>
The Title And "so it goes"
<<==== RESULT ====<<
```

### Why CslJson Exists (Internal Benefits)

CslJson is valuable **internally** for processing, not for test assertions:

1. **Flip-flop rendering**: The `RenderContext` tracks whether we're inside italic/bold/quotes to handle "flip-flopping" (where nested italic becomes normal, nested quotes alternate single/double)

2. **Structured manipulation**: Operations like `punctuationInsideQuotes` pattern-match on structure:
   ```haskell
   go (CslConcat (CslQuoted x) y) = ...  -- move punctuation inside
   ```

3. **JSON serialization**: `cslJsonToJson` produces structured JSON for CSL JSON bibliographies

### Implications for Rust Port

Since test comparison is text-based (not structure-based), we have two options:

**Option A: Direct Pandoc AST (recommended)**
- Process using `Vec<Inline>` directly
- Use Span markers for quotes, displays, etc.
- Render to HTML/text for test comparison
- Simpler overall design

**Option B: Intermediate CslOutput type**
- Create a Rust enum similar to CslJson
- Process in this intermediate format
- Convert to Pandoc AST at the end
- More complex, but matches Haskell structure

The CslJson nesting structure provides **internal processing benefits** but is **not required for test assertions**. For our Rust port with direct Pandoc AST output, we can achieve the same processing benefits using:

- `Span` markers for quotes, displays, notes
- Post-processing passes for localization and punctuation movement
- Pandoc AST's own nesting structure (Emph contains inlines, etc.)

**Updated Recommendation:** Direct Pandoc AST remains the simpler choice. The nesting benefits of CslJson can be achieved using Pandoc's existing structure plus span markers for CSL-specific semantics.
