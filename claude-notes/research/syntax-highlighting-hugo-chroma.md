# Syntax Highlighting in Hugo: Chroma Deep Dive

Research into how Hugo (via the Chroma library) implements syntax highlighting, and comparison to alternative approaches like tree-sitter.

## A. Lexer Lookup: Language Name Resolution

Chroma resolves language names through a **layered fallback system** (`external-sources/chroma/registry.go:70-98`):

1. **Exact name match** (case-sensitive, then case-insensitive)
2. **Alias match** (case-sensitive, then case-insensitive)
3. **File extension matching**: treats `name` as a file extension, looks for lexers matching `filename.<name>` or exact `filename`
4. **Filename matching**: uses glob patterns from lexer config (`Filenames`, `AliasFilenames` fields)
5. **MIME type matching** (optional, via `MatchMimeType()`)
6. **Content analysis** (optional, via `Analyse()` which runs lexer-specific heuristics)
7. **Fallback**: returns `nil` if no match found (caller typically selects a plaintext fallback)

Registration maps are case-normalized for lookup robustness (`registry.go:201-206`). Each lexer has:
- `Name`: primary identifier (e.g., "Python")
- `Aliases`: shortcuts (e.g., "py", "py3", "python3")
- `Filenames`: glob patterns (e.g., "*.py")
- `AliasFilenames`: secondary patterns
- `Priority`: float32 to break ties; lexers with higher priority win if multiple match

**No hard failure on unknown language**—callers wrap the result with a fallback lexer. Quarto and Hugo typically use a plaintext/fallback lexer (`lexers.go:81-85`) with priority -1 and filename glob `*`.

## B. Token Taxonomy: Type Hierarchy and Naming

Chroma's token types form a **strict hierarchical structure** defined in `types.go:17-191`. Categories are organized by 1000-value ranges; subcategories by 100-value ranges:

```
Meta range:     -14 to 0   (Background, PreWrapper, Line, Error, EOF, etc.)
Keywords:      1000-1006  (Keyword, KeywordConstant, KeywordDeclaration, ...)
Names:         2000-2014  (Name, NameClass, NameFunction, NameVariable, ...)
  Builtins:    2100-2101  (NameBuiltin, NameBuiltinPseudo)
  Variables:   2200-2206  (NameVariable, NameVariableClass, ...)
  Functions:   2300-2301  (NameFunction, NameFunctionMagic)
Literals:      3000-3207
  Strings:     3100-3116  (LiteralString, StringDouble, StringEscape, ...)
  Numbers:     3200-3207  (LiteralNumber, NumberFloat, NumberHex, ...)
Operators:     4000-4002  (Operator, OperatorWord, OperatorReserved)
Punctuation:   5000-5000  (Punctuation)
Comments:      6000-6101  (Comment, CommentSingle, CommentPreproc, ...)
Generic:       7000-7012  (GenericDeleted, GenericEmph, GenericError, ...)
Text:          8000-8003  (Text, TextWhitespace, TextSymbol, ...)
```

This taxonomy **comes from Pygments** (`doc.go:4`). The hierarchy is navigable: `t.Parent()`, `t.Category()`, `t.SubCategory()` arithmetic (`types.go:327-343`). A token type with value 2250 (NameVariableClass) has:
- Parent: 2200 (NameVariable)
- Category: 2000 (Name)

Short CSS class names map to token types via `StandardTypes` map (`types.go:224-325`), e.g., `k` (keyword), `nc` (NameClass), `s1` (StringSingle).

## C. Output Formats and Token Stream Shape

Chroma ships **multiple output formatters** (`formatters/api.go:12-25`):

1. **HTML** (`html.New()`)—writes `<span>` tags with CSS classes or inline styles
2. **SVG** (`svg.New()`)—renders highlighted code as vector graphics
3. **Terminal (TTY)** (`tty_indexed.go`, `tty_truecolour.go`)—ANSI-256 or true-color terminal output
4. **JSON** (`json.go`)—machine-readable token + metadata
5. **NoOp** (`formatters.go:14-21`)—discards formatting, outputs raw text
6. **Custom formatters**: implement `Formatter` interface (`formatter.go:8-9`)

All formatters consume a token stream via an `Iterator` function (`iterator.go:10`):
```go
type Iterator func() Token
type Token struct {
  Type  TokenType  // Token classification (e.g., KeywordConstant)
  Value string     // Raw text (e.g., "def")
}
```

Formatters iterate until `EOF` token, collecting tokens into lines with `SplitTokensIntoLines()` (`iterator.go:59-93`), which splits tokens at `\n` to maintain line boundaries.

## D. HTML Output: CSS Classes

The HTML formatter emits semantic CSS class names mapped from `StandardTypes` (`types.go:224-325`):

A Python `def` keyword (type `KeywordDeclaration`, numeric value 1002) receives class name:
```
.chroma .kd  // "kd" = KeywordDeclaration short name
```

The short-vs-long class scheme is **not configurable**; chroma always uses short names from `StandardTypes`. Custom CSS via `WithCustomCSS()` option can override styles, but class names themselves are fixed.

HTML formatter options (`html/html.go:15-132`):
- `WithClasses(b bool)`: emit CSS classes (vs. inline `style=""`)
- `ClassPrefix(prefix string)`: prepend to class names (e.g., `"hljs-"` → `hljs-kd`)
- `WithAllClasses(b bool)`: emit all token type classes (default omits redundant ones)
- `Standalone(b bool)`: wrap in full HTML document
- `WithLineNumbers(b bool)`, `LineNumbersInTable(b bool)`, `HighlightLines(ranges)`: line-number and highlighting features
- `WithLinkableLineNumbers(prefix string)`: add `id` attributes for anchor linking

Output wraps code in `<pre><code>...</code></pre>` by default, with one `<span>` per token when `Classes` is true:
```html
<span class="chroma"><span class="kd">def</span> <span class="nf">foo</span>(...)</span>
```

## E. Theme Model: Style Definitions and Token Taxonomy Mapping

Themes are XML files (`styles/*.xml`) mapping token types to visual properties. Example from `styles/monokai.xml`:
```xml
<style name="monokai">
  <entry type="Keyword" style="#66d9ef"/>
  <entry type="NameFunction" style="#a6e22e"/>
  <entry type="LiteralString" style="#e6db74"/>
  <entry type="Comment" style="#75715e"/>
</style>
```

Parsing happens in `style.go`: `StyleEntry` (`style.go:42-53`) contains:
- `Colour`, `Background`, `Border`: hex color values
- `Bold`, `Italic`, `Underline`: trilean (Yes/No/Pass) for inheritance
- `NoInherit`: block cascading from parent types

The theme registry (`styles/api.go:16-39`) loads all XML files at init time and maps style names to lexer token types. Style lookup is hierarchical: if a specific token type (e.g., `StringDouble`) has no entry, the formatter walks up to parent (`String`), then category (`Literal`), using inherited values via `Inherit()` (`style.go:109-139`).

Token type inheritance follows the Chroma taxonomy: `NameVariableClass` (2203) inherits from `NameVariable` (2200), which inherits from `Name` (2000).

## F. Line Numbers, Highlighting, and Anchors

Line number and highlighting features are **formatter-level options**, not token-level:

HTML formatter (`html/html.go:93-125`):
- `WithLineNumbers(b bool)`: emit line numbers in `<span class="ln">N</span>`
- `LineNumbersInTable(b bool)`: wrap code and line numbers in a `<table><tr><td>` for copy-paste friendliness
- `HighlightLines(ranges [][2]int)`: highlight specified line ranges with `LineHighlight` token type styling
- `WithLinkableLineNumbers(prefix string)`: add `id="prefix1"`, `id="prefix2"`, etc., making lines linkable via `#prefix5`
- `BaseLineNumber(n int)`: start numbering at `n` instead of 1

These are **not exposed in fenced-code info strings** by chroma itself. Hugo (or a markdown processor) would need to parse code-fence attributes and pass them to the formatter. For example, Goldmark (Go markdown) supports HTML-like attributes: `\`\`\`python {linenos=inline,hl_lines=[2,3]}\`.

## G. Pygments Heritage and Industry Standing

Chroma's token taxonomy is **a direct port of Pygments** (`doc.go:4`). The type hierarchy (Keyword, Name, Literal, etc.) and naming scheme (KeywordDeclaration, NameFunction, StringDouble) are identical.

**Pygments status**: Pygments is the de facto standard for syntax highlighting in Python ecosystem and widely used in documentation, Sphinx, and other tools. Its token taxonomy is well-established but **not formally standardized** as an interchange format.

**tree-sitter comparison**: Tree-sitter uses a different capture name scheme for highlights:
```
@keyword           (vs. Pygments Keyword)
@function          (vs. Pygments NameFunction)
@string            (vs. Pygments LiteralString)
@constant          (vs. Pygments NameConstant or LiteralNumber)
@variable.builtin  (vs. Pygments NameBuiltin)
```

Tree-sitter names are dot-separated, more concise, and **hierarchical by convention** (e.g., `@variable.builtin`, `@variable.parameter`). The names are not standardized but are emerging as a de facto interchange format for modern syntax highlighters. Neither Pygments nor tree-sitter's taxonomy is formally registered with IANA or other standards bodies.

**Key differences**:
- Chroma uses numeric token types (enums) internally; short class names (`k`, `nc`) in HTML output
- Tree-sitter uses string capture names (@keyword, @function) throughout
- Chroma's hierarchy is numeric-based; tree-sitter's is string-based with dot notation
- Chroma supports inheritance and fallback styling; tree-sitter is query-driven, relying on theme configuration

## Implications for Quarto 2

1. **Lexer selection**: Chroma's layered fallback avoids hard failures; Quarto can safely pass unknown language codes
2. **Token stream**: All formatters consume the same iterator interface—Quarto can plug in custom formatters easily
3. **Theme extensibility**: Themes are XML files mapping token types to colors; Quarto can generate or load custom themes
4. **Line features**: Hugo exposes line numbers and highlighting via formatter options, not fenced-code syntax; Quarto should decide whether to support these and how to expose them (metadata, attributes, or post-processing)
5. **CSS class stability**: Short class names are fixed (`k`, `nc`, etc.); Quarto themes should expect these exact names for predictable styling
6. **Tree-sitter alternative**: If Quarto adopts tree-sitter for highlighting, expect to translate `@keyword` ↔ Chroma's `KeywordDeclaration` or provide a compatibility layer for themes
