# Output Format Design Proposal

**Created**: 2025-11-27
**Related**: k-422 (CSL conformance testing)

## Problem Statement

Our current `Output` AST renders to markdown-style formatting (`**bold**`, `*italic*`), but:
1. The CSL test suite expects HTML output (`<b>bold</b>`, `<i>italic</i>`)
2. Quarto integration will likely need Pandoc AST output
3. We need a clean abstraction to support multiple output formats

## Analysis of Pandoc's citeproc Design

### Architecture

```
                      ┌─────────────────┐
                      │   Output a      │  (parameterized AST)
                      │  - Formatted    │
                      │  - Literal a    │  ← leaf type is `a`
                      │  - Tagged       │
                      │  - Linked       │
                      └────────┬────────┘
                               │
                               │ renderOutput
                               ▼
              ┌────────────────────────────────────┐
              │     CiteprocOutput a (typeclass)   │
              │  - toText :: a -> Text             │
              │  - fromText :: Text -> a           │
              │  - addFontWeight :: FontWeight -> a -> a │
              │  - addFontStyle :: FontStyle -> a -> a  │
              │  - addHyperlink :: Text -> a -> a  │
              │  - ...                              │
              └────────────────────────────────────┘
                      │                    │
         ┌────────────┘                    └────────────┐
         ▼                                              ▼
┌─────────────────────┐                    ┌─────────────────────┐
│   CslJson Text      │                    │   Pandoc Inlines    │
│   (intermediate AST)│                    │   (Pandoc's AST)    │
│                     │                    │                     │
│  CslBold, CslItalic │                    │  B.strong, B.emph   │
│  CslSup, CslSub     │                    │  B.superscript      │
│  etc.               │                    │  etc.               │
└──────────┬──────────┘                    └─────────────────────┘
           │
           │ renderCslJson
           ▼
    ┌─────────────┐
    │   HTML Text │  (<b>, <i>, <sup>, etc.)
    └─────────────┘
```

### Key Design Points

1. **`Output a` is parameterized**: The `a` type parameter is the leaf type. `Literal a` contains an `a`, not a `String`.

2. **`CiteprocOutput` typeclass**: Defines formatting operations that work on any output type `a`. This is how formatting is applied polymorphically.

3. **Two-stage rendering for HTML**:
   - First, `renderOutput` converts `Output (CslJson Text)` to `CslJson Text` (an intermediate AST)
   - Then, `renderCslJson` converts `CslJson Text` to HTML `Text`

4. **Direct rendering for Pandoc**:
   - `renderOutput` converts `Output Inlines` directly to `Inlines`
   - No intermediate step needed because `Inlines` IS the Pandoc AST

### CSL Test Suite

The test harness (line 117-140 of `test/Spec.hs`):
```haskell
let actual = citeproc opts style Nothing (input test) cites
-- ...
(T.intercalate "\n" $ map (renderCslJson' loc) (resultCitations actual))
-- ...
renderCslJson' loc x = renderCslJson True loc x  -- True = escape HTML entities
```

Tests use `CslJson Text` and render to HTML via `renderCslJson`.

## Design Options for quarto-citeproc

### Option A: Parameterized Output (Full Pandoc Design)

```rust
enum Output<T> {
    Formatted { formatting: Formatting, children: Vec<Output<T>> },
    Literal(T),
    Tagged { tag: Tag, child: Box<Output<T>> },
    Linked { url: String, children: Vec<Output<T>> },
    InNote(Box<Output<T>>),
    Null,
}

trait CiteprocOutput: Sized + Default + Clone {
    fn from_text(s: &str) -> Self;
    fn to_text(&self) -> String;
    fn add_bold(self) -> Self;
    fn add_italic(self) -> Self;
    fn add_superscript(self) -> Self;
    fn add_subscript(self) -> Self;
    fn add_small_caps(self) -> Self;
    fn add_quotes(self) -> Self;
    fn add_hyperlink(self, url: &str) -> Self;
    // ...
}

fn render_output<T: CiteprocOutput>(output: &Output<T>, locale: &Locale) -> T { ... }
```

**Pros**: Most faithful to Pandoc design, type-safe
**Cons**: Requires changing `Output<T>` everywhere, generic proliferation

### Option B: Output Format Enum

```rust
#[derive(Clone, Copy)]
enum OutputFormat {
    Html,
    Markdown,
    PlainText,
}

impl Output {
    fn render(&self, format: OutputFormat, locale: &Locale) -> String {
        match format {
            OutputFormat::Html => self.render_html(locale),
            OutputFormat::Markdown => self.render_markdown(locale),
            OutputFormat::PlainText => self.render_plain(locale),
        }
    }
}
```

**Pros**: Simple, minimal changes to existing code
**Cons**: All formats must be strings, can't support structured output (Pandoc AST)

### Option C: Trait-Based Renderer (Visitor Pattern)

```rust
trait OutputRenderer {
    type Target;

    fn render_literal(&mut self, s: &str) -> Self::Target;
    fn render_with_formatting(&mut self, formatting: &Formatting, inner: Self::Target) -> Self::Target;
    fn render_linked(&mut self, url: &str, inner: Self::Target) -> Self::Target;
    fn render_sequence(&mut self, items: Vec<Self::Target>) -> Self::Target;
}

struct HtmlRenderer { locale: Locale }
struct MarkdownRenderer;
struct PlainTextRenderer;
// Future: struct PandocRenderer;

impl OutputRenderer for HtmlRenderer {
    type Target = String;

    fn render_with_formatting(&mut self, formatting: &Formatting, inner: String) -> String {
        let mut result = inner;
        if let Some(FontWeight::Bold) = formatting.font_weight {
            result = format!("<b>{}</b>", result);
        }
        // ...
        result
    }
}

impl Output {
    fn render_with<R: OutputRenderer>(&self, renderer: &mut R) -> R::Target { ... }
}
```

**Pros**:
- Clean separation of AST and rendering
- Supports string AND structured targets (Pandoc AST via `Target = Inlines`)
- Easy to add new renderers
- Similar conceptually to Pandoc's approach

**Cons**:
- More complex than Option B
- Renderer needs to be passed through

### Option D: Intermediate AST (Like CslJson)

```rust
// Intermediate format-aware AST
enum FormattedText {
    Text(String),
    Bold(Box<FormattedText>),
    Italic(Box<FormattedText>),
    Superscript(Box<FormattedText>),
    Subscript(Box<FormattedText>),
    SmallCaps(Box<FormattedText>),
    Link { url: String, content: Box<FormattedText> },
    Sequence(Vec<FormattedText>),
}

impl FormattedText {
    fn to_html(&self) -> String { ... }
    fn to_markdown(&self) -> String { ... }
    fn to_plain(&self) -> String { ... }
}

impl Output {
    fn to_formatted_text(&self, locale: &Locale) -> FormattedText { ... }
}
```

**Pros**:
- Matches Pandoc's `CslJson` intermediate representation
- Clean separation: Output AST → FormattedText → String
- FormattedText can be extended for Pandoc integration

**Cons**:
- Two-stage conversion
- Another AST to maintain

## Recommendation

**Option C (Trait-Based Renderer)** is recommended because:

1. **Minimal disruption**: Doesn't require parameterizing `Output<T>`
2. **Flexible targets**: Can render to strings (HTML, markdown) or structured formats (Pandoc AST)
3. **Conceptually similar to Pandoc**: The renderer trait is analogous to `CiteprocOutput`
4. **Easy testing**: Can add `HtmlRenderer` for tests without changing evaluation code
5. **Future-proof**: Adding a Pandoc renderer later is straightforward

### Implementation Plan

1. **Phase 1**: Add `OutputRenderer` trait and `HtmlRenderer`
   - Keep existing `render()` method for backwards compatibility
   - Add `render_with<R: OutputRenderer>(&self, renderer: &mut R) -> R::Target`
   - Update test harness to use `HtmlRenderer`
   - Tests should start passing

2. **Phase 2**: Implement `MarkdownRenderer`
   - Move current markdown logic to this renderer
   - Deprecate old `render()` method

3. **Phase 3**: (Future) Implement `PandocRenderer` for Quarto integration
   - `type Target = pandoc_types::Inlines` or similar

## Files to Modify

1. `crates/quarto-citeproc/src/output.rs` - Add `OutputRenderer` trait and implementations
2. `crates/quarto-citeproc/src/eval.rs` - Use renderer in `evaluate_citation`, `evaluate_bibliography_entry`
3. `crates/quarto-citeproc/tests/csl_conformance.rs` - Use `HtmlRenderer` for test comparison
