# Pandoc Syntax Highlighting Integration with Skylighting

## Executive Summary

Pandoc uses the Skylighting library for syntax highlighting code blocks. The highlighting pipeline operates at the **writer stage only**—the AST remains unchanged. Each writer (HTML, LaTeX, ConTeXt, etc.) invokes a format-specific renderer that consumes a token stream and produces markup. This design makes the token taxonomy stable across all formatters while allowing flexible per-format output.

## 1. Info String → Lexer Mapping

**Location:** `external-sources/pandoc/src/Text/Pandoc/Highlighting.hs:139`

When Pandoc encounters a code block with a class attribute (e.g., ````python`, ````c`), the `highlight` function uses `msum (map (\`lookupSyntax\` syntaxmap) classes)` to resolve the class to a Syntax definition. The resolution order is implemented in Skylighting:

**Location:** `external-sources/skylighting/skylighting-core/src/Skylighting/Core.hs:52-61`

```haskell
lookupSyntax :: Text -> SyntaxMap -> Maybe Syntax
```

The lookup tries (in order):
1. Full language name (case-insensitive): `syntaxByName` (line 40-42)
2. Short name (case-insensitive): `syntaxByShortName` (line 45-48)
3. File extension match: `syntaxesByExtension` (line 28-32)

Special cases like "csharp" → "cs" and "fortran" → "for" are hardcoded (lines 55-57).

If no class resolves to a syntax and `numberLines` is not set, highlighting silently fails with `Left ""` (Highlighting.hs:146). If numberLines is set, the code is formatted as plain text using `NormalTok` throughout.

## 2. Token Taxonomy

**Location:** `external-sources/skylighting/skylighting-core/src/Skylighting/Types.hs:194-224`

Skylighting defines a fixed enum of 25 token types, all derived from KDE Kate syntax theme standards:

```
KeywordTok, DataTypeTok, DecValTok, BaseNTok, FloatTok, ConstantTok,
CharTok, SpecialCharTok, StringTok, VerbatimStringTok, SpecialStringTok,
ImportTok, CommentTok, DocumentationTok, AnnotationTok, CommentVarTok,
OtherTok, FunctionTok, VariableTok, ControlFlowTok, OperatorTok,
BuiltInTok, ExtensionTok, PreprocessorTok, AttributeTok, RegionMarkerTok,
InformationTok, WarningTok, AlertTok, ErrorTok, NormalTok
```

Each Kate syntax definition (XML-based) maps regex patterns and keyword lists to these token types. The token types are rendered as a Haskell `Enum` with `Show` and `Read` instances, enabling JSON serialization (lines 229-248).

The comment at line 243 confirms the KDE provenance: "JSON @"Keyword"@ corresponds to 'KeywordTok', and so on."

## 3. AST Representation of Highlighted Code

**Finding: Highlighting happens only at the writer stage; the AST is never modified.**

The `CodeBlock` element remains a bare `CodeBlock` in the Pandoc AST throughout parsing and transformation. Highlighting only occurs when a writer processes it.

**HTML Writer (Writer stage):** `external-sources/pandoc/src/Text/Pandoc/Writers/HTML.hs:986-1002`

The HTML writer calls `highlight (writerSyntaxMap opts) (if html5 then formatHtmlBlock else formatHtml4Block) (id'',classes',keyvals) adjCode`, which either returns `Right h` (highlighted HTML) or `Left msg` (fallback to plain code). The highlighted result is inserted directly into the output as pre-formatted HTML containing span elements with token class annotations.

**LaTeX Writer (Writer stage):** `external-sources/pandoc/src/Text/Pandoc/Writers/LaTeX.hs:552-553`

The LaTeX writer calls `highlight (writerSyntaxMap opts) formatLaTeXBlock ("",classes ++ ["default"],keyvalAttr) str`, which produces either LaTeX command sequences (e.g., `\KeywordTok{...}`) or falls back to raw `\begin{verbatim}...\end{verbatim}`.

The critical insight: **the formatted output is literal text/markup, not an AST fragment**. The HTML writer returns `Html` (from blaze), and the LaTeX writer returns `Text`. Both are embedded directly into the output representation of the writer, bypassing any further AST processing.

## 4. The "Per-Format Writer Owns Highlighting" Pattern

Skylighting follows a strict separation of concerns: **token stream is the stable interchange; formatters are format-specific**.

All token streams come from a single tokenizer:

**Location:** `external-sources/skylighting/skylighting-core/src/Skylighting/Tokenizer.hs`

The tokenizer is format-agnostic; it produces `[SourceLine]` where `type SourceLine = [Token]` and `type Token = (TokenType, Text)` (Types.hs:190-191).

Each format then provides a formatter:
- HTML: `formatHtmlBlock` (Format/HTML.hs:69)
- LaTeX: `formatLaTeXBlock` (Format/LaTeX.hs:74)
- ConTeXt: `formatConTeXtBlock` (skylighting-format-context/src/Skylighting/Format/ConTeXt.hs)
- ANSI: `formatANSI` (skylighting-format-ansi/src)
- Typst: `formatTypstBlock` (skylighting-format-typst/src)

Each formatter consumes `FormatOptions -> [SourceLine] -> OutputType`, where OutputType is format-specific (Html, Text, etc.). Pandoc exports wrapper functions (Highlighting.hs:14-36) that re-export all of these.

## 5. CSS Class Naming

**Location:** `external-sources/skylighting/skylighting-format-blaze-html/src/Skylighting/Format/HTML.hs:132-163`

The HTML formatter maps each TokenType to a 2-letter CSS class via the `short` function:

```
KeywordTok → "kw"
DataTypeTok → "dt"
DecValTok → "dv"
BaseNTok → "bn"
FloatTok → "fl"
CharTok → "ch"
StringTok → "st"
CommentTok → "co"
OtherTok → "ot"
AlertTok → "al"
FunctionTok → "fu"
... (and 14 more)
NormalTok → "" (no markup)
```

These are emitted as `<span class="kw">...</span>` for token type `kw`, etc. (tokenToHtml, line 124-130).

CSS generation (styleToCss, line 166-232) maps token styles to selector rules: `.sourceCode code span.kw { color: ...; font-weight: ...; ... }`. Themes (Style objects) define tokenStyles as a `Map TokenType TokenStyle` (Types.hs), each containing color, background, bold, italic, underline flags.

NormalTok is never wrapped in a span; normal text is emitted bare (line 125).

## 6. Does Pandoc Pass Highlighted Code as Native AST or Always as Literal Output?

**Answer: Always as literal output. Never as native AST.**

- **HTML:** The right-hand side of the `Right h` case (HTML.hs:1002) is of type `Html`, which is literal markup. It is inserted into the document output as pre-rendered HTML strings via `addAttrs`.

- **LaTeX:** The right-hand side of the `Right h` case (LaTeX.hs:558-561) is of type `Text`, which is a literal string of LaTeX commands. It is embedded via `literal h`.

- **ConTeXt, ANSI, Typst:** All follow the same pattern—tokenization happens, formatting produces literal output strings/markup objects, and those are inserted directly into the writer's output.

**Why this matters for Q2:** Pandoc treats highlighting as a serialization-stage concern, not an AST-stage concern. The token information is generated and discarded at write time. If Quarto 2 wants to expose highlighting information in the AST (e.g., to apply custom transformations, re-style on the fly, or annotate spans with semantic metadata), it would require a different design: either (a) represent highlighted code as nested Span elements in the AST with token type attributes, or (b) attach token metadata as hidden attributes on the CodeBlock itself.

## 7. Summary of Key Design Patterns

1. **Lexer lookup is format-agnostic:** The `highlight` function takes a formatter function as an argument. The same tokenization pipeline works for all formats.

2. **Token taxonomy is fixed and stable:** The 25 TokenType values are standardized, matching KDE Kate conventions. New token types would require changes to the Skylighting type definition and all downstream formatters.

3. **Highlighting is write-time only:** AST remains unmodified. No spans, no metadata. The CodeBlock → formatted markup happens in the writer.

4. **Per-format rendering is pluggable:** Each format has a standalone module implementing formatHtmlBlock, formatLaTeXBlock, etc. New formats just need to implement the token → output mapping.

5. **Styles and themes are separate from lexers:** Kate syntax definitions (`.xml`) and theme files (`.theme`, JSON) are completely independent. A syntax can be paired with any theme at rendering time.
