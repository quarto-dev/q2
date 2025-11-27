# Citeproc Rust Port Design Report

**Issue:** k-410
**Created:** 2025-11-26

## Executive Summary

This report analyzes the feasibility and design of porting the Haskell citeproc library to Rust, with a key enhancement: **source location tracking for CSL style files**. This enables precise error messages when CSL styles contain errors or when citations fail to render correctly.

The port is feasible but substantial. The Haskell implementation is ~7,000 lines of dense code with complex algorithms for disambiguation, sorting, and name formatting. The recommended approach is phased implementation starting with CSL parsing (the novel contribution) and progressively adding citation processing features.

## Background

### What is Citeproc?

Citeproc is a citation processing engine that:
1. Parses **CSL (Citation Style Language)** style files (XML format)
2. Accepts **references** (bibliographic data in CSL-JSON format)
3. Accepts **citations** (requests to cite specific references)
4. Produces **formatted output** (in-text citations and bibliography entries)

CSL is an open standard with 10,000+ styles available (APA, Chicago, MLA, etc.).

### Why Port to Rust?

1. **Integration**: Native integration with quarto-markdown-pandoc
2. **Performance**: No process spawning or JSON serialization overhead
3. **Error Messages**: Source-tracked CSL enables precise error reporting
4. **Single Binary**: No Haskell runtime or pandoc dependency

### Why Source-Tracked CSL?

CSL files can be complex (1,400+ lines for Chicago style) with deep nesting. When something goes wrong:
- "Variable 'author' not found" → Where in the CSL?
- "Date format error" → Which `<date>` element?
- "Macro 'title' references undefined macro 'container'" → Which line?

Source tracking enables error messages like:
```
error[CSL-101]: undefined macro 'container-title-short'
  --> chicago.csl:234:15
    |
234 |     <text macro="container-title-short"/>
    |                 ^^^^^^^^^^^^^^^^^^^^^^^ macro not defined
    |
note: did you mean 'container-title'?
  --> chicago.csl:45:1
    |
45  | <macro name="container-title">
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

## Haskell Citeproc Architecture

### Module Structure (6,901 lines)

| Module | Lines | Purpose |
|--------|-------|---------|
| `Types.hs` | 2,043 | Core data types |
| `Eval.hs` | 2,836 | Citation processing algorithm |
| `Style.hs` | 623 | CSL XML parsing |
| `CslJson.hs` | 517 | CSL-JSON format handling |
| `Element.hs` | 226 | XML element utilities |
| `Locale.hs` | 108 | Locale data management |
| `CaseTransform.hs` | 159 | Text case utilities |
| `Pandoc.hs` | 304 | Pandoc AST integration |

### Core Types

```haskell
-- Style representation
data Style a = Style
  { styleCslVersion :: (Int, Int, Int)
  , styleOptions :: StyleOptions
  , styleCitation :: Layout a
  , styleBibliography :: Maybe (Layout a)
  , styleLocales :: [Locale]
  , styleMacros :: M.Map Text [Element a]
  }

-- Formatting instructions
data ElementType a
  = EText TextType
  | EDate Variable DateType [DP]
  | ENumber Variable NumberForm
  | ENames [Variable] NamesFormat [Element a]
  | ELabel Variable TermForm Pluralize
  | EGroup Bool [Element a]
  | EChoose [(Match, [Condition], [Element a])]

-- Reference data
data Reference a = Reference
  { referenceId :: ItemId
  , referenceType :: Text
  , referenceVariables :: M.Map Variable (Val a)
  }

-- Citation request
data Citation a = Citation
  { citationId :: Maybe Text
  , citationNoteNumber :: Maybe Int
  , citationItems :: [CitationItem a]
  }
```

### Processing Algorithm

The `Eval.hs` module implements:
1. **Citation numbering** based on appearance order
2. **Disambiguation** (year-suffix, add-names, add-givenname)
3. **Position tracking** (first, ibid, subsequent, near-note)
4. **Sorting** with Unicode collation
5. **Grouping and collapsing** (e.g., [1-3] instead of [1,2,3])
6. **Name formatting** (particles, initials, et al.)
7. **Date formatting** with locale-specific patterns

### Output Model

The Haskell library is parameterized on output type via `CiteprocOutput` typeclass:
```haskell
class CiteprocOutput a where
  toText :: a -> Text
  fromText :: Text -> a
  addFontVariant :: FontVariant -> a -> a
  addFontStyle :: FontStyle -> a -> a
  -- ... more formatting methods
```

This allows output to any format (plain text, HTML, Pandoc Inlines).

**For our Rust port:** We will **skip the CiteprocOutput abstraction** and produce Pandoc `Inlines` directly. The CiteprocOutput methods map almost 1:1 to Pandoc AST constructors (`addFontStyle Italic` → `Emph`, `addFontWeight Bold` → `Strong`, etc.), so the trait adds indirection without clear benefit for our use case.

See `claude-notes/research/2025-11-26-citeproc-output-architecture.md` for detailed analysis.

## CSL Format Analysis

### Structure

CSL is XML with these main elements:
- `<style>` - Root element with version and class
- `<info>` - Metadata (title, author, license)
- `<locale>` - Language-specific terms and date formats
- `<macro>` - Reusable formatting templates
- `<citation>` - In-text citation layout
- `<bibliography>` - Bibliography entry layout

### Complexity

| Metric | Simple Style | APA | Chicago |
|--------|--------------|-----|---------|
| Lines | 70 | 474 | 1,430 |
| Max nesting | 5 | 10 | 13 |
| Macros | 3 | 17 | 45 |
| Conditionals | 5 | 29 | 120+ |

### Element Types

**Structural:** `<group>`, `<choose>`, `<if>`, `<else-if>`, `<else>`, `<sort>`, `<key>`

**Content:** `<text>`, `<names>`, `<name>`, `<label>`, `<date>`, `<date-part>`, `<number>`

**Formatting attributes:** `font-style`, `text-case`, `quotes`, `prefix`, `suffix`, `delimiter`

## Quick-XML Position Tracking

### API Overview

quick-xml provides event-based (pull) parsing with position tracking:

```rust
let mut reader = Reader::from_str(xml);
loop {
    let pos_before = reader.buffer_position();
    match reader.read_event()? {
        Event::Start(e) => {
            // e.name() - tag name
            // e.attributes() - attribute iterator
            let pos_after = reader.buffer_position();
            // Span is pos_before..pos_after
        }
        Event::End(e) => { /* ... */ }
        Event::Text(e) => { /* ... */ }
        Event::Eof => break,
        _ => {}
    }
}
```

### Position Capabilities

| Feature | Available | Notes |
|---------|-----------|-------|
| Element start position | Yes | `buffer_position()` before read |
| Element end position | Yes | `buffer_position()` after read |
| Content span | Yes | `read_to_end()` returns `Range<u64>` |
| Attribute positions | Manual | Must calculate from raw bytes |
| Error position | Yes | `error_position()` method |

### Limitation: Attribute Positions

Unlike yaml-rust2 which provides markers for each event, quick-xml doesn't track individual attribute positions. We must calculate them:

```rust
if let Event::Start(e) = event {
    let raw = e.as_ref();  // Raw bytes of tag
    // Parse attributes manually to find positions
    for attr in e.attributes() {
        // attr.key, attr.value available
        // Position requires manual calculation
    }
}
```

**Mitigation:** For CSL, element-level positions are usually sufficient for error messages. Attribute-level precision can be added later if needed.

## Quarto-YAML Source Tracking Patterns

### Key Design Patterns

1. **Parallel Structure**: Store owned data + parallel source info
   ```rust
   pub struct YamlWithSourceInfo {
       pub yaml: Yaml,           // The data
       pub source_info: SourceInfo,  // Position info
       children: Children,       // Source-tracked children
   }
   ```

2. **Stack-Based Parsing**: Build nodes bottom-up during event processing
   ```rust
   enum BuildNode {
       Sequence { start_marker: Marker, items: Vec<...> },
       Mapping { start_marker: Marker, entries: Vec<...> },
   }
   ```

3. **Contiguous Span Creation**: Combine child spans into parent span
   ```rust
   fn create_contiguous_span(start: &SourceInfo, end: &SourceInfo) -> SourceInfo
   ```

4. **Three-Level Hash Tracking**: Key span, value span, entry span

### SourceInfo Variants

```rust
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Rc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
}
```

## Proposed Rust Architecture

### Design Philosophy: Layered Parsing

Following the pattern established by quarto-yaml, we separate concerns into layers:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         quarto-citeproc                             │
│                 (citation processing algorithm)                      │
│     References + Citations + Style → Formatted Pandoc Inlines       │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          quarto-csl                                  │
│                    (CSL semantics layer)                             │
│      XmlWithSourceInfo → Style, Element, Macro, Locale, etc.        │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          quarto-xml                                  │
│               (generic XML with source tracking)                     │
│            Raw XML string → XmlWithSourceInfo                        │
│                   (analogous to quarto-yaml)                         │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
                            ▼
                    ┌───────────────┐
                    │   quick-xml   │
                    │  (external)   │
                    └───────────────┘
```

This layering provides:
- **Reusability**: quarto-xml can parse any XML (JATS, DocBook, etc.)
- **Clean boundaries**: Each crate has one responsibility
- **Testability**: Each layer can be tested independently
- **Familiar patterns**: Mirrors quarto-yaml architecture

### Crate Structure

The architecture uses three crates with clear separation of concerns:

```
crates/
├── quarto-xml/              # XML parsing with source tracking (analogous to quarto-yaml)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── parser.rs        # quick-xml adaptor with position tracking
│   │   ├── types.rs         # XmlWithSourceInfo, XmlElement, XmlAttribute
│   │   └── error.rs         # XML parse errors with spans
│   └── Cargo.toml
│
├── quarto-csl/              # CSL-specific parsing and types
│   ├── src/
│   │   ├── lib.rs
│   │   ├── parser.rs        # XmlWithSourceInfo → Style conversion
│   │   ├── style.rs         # Style type with SourceInfo
│   │   ├── element.rs       # Element types with SourceInfo
│   │   ├── locale.rs        # Locale types
│   │   ├── validate.rs      # CSL validation (macro refs, etc.)
│   │   └── error.rs         # CSL-specific errors with spans
│   ├── build.rs             # Test generation
│   ├── tests/
│   │   ├── data/
│   │   │   ├── unit/        # Hand-written unit tests
│   │   │   └── csl-suite/   # CSL conformance test files
│   │   ├── enabled_tests.txt    # Promoted tests manifest
│   │   └── csl_conformance.rs   # Generated test harness
│   └── Cargo.toml
│
├── quarto-citeproc/         # Citation processing
│   ├── src/
│   │   ├── lib.rs
│   │   ├── types.rs         # Reference, Citation, etc.
│   │   ├── eval.rs          # Processing algorithm
│   │   ├── names.rs         # Name formatting
│   │   ├── dates.rs         # Date formatting
│   │   ├── sort.rs          # Collation and sorting
│   │   ├── disambig.rs      # Disambiguation
│   │   └── output.rs        # Pandoc Inlines builders (italic, bold, link, etc.)
│   ├── build.rs             # Test generation
│   ├── tests/
│   │   ├── data/
│   │   │   ├── unit/        # Hand-written unit tests
│   │   │   └── citeproc-suite/  # Citeproc test files
│   │   ├── enabled_tests.txt    # Promoted tests manifest
│   │   └── citeproc_conformance.rs
│   └── Cargo.toml
```

### Dependency Graph

```
quarto-citeproc
    │
    └──► quarto-csl
              │
              └──► quarto-xml
                        │
                        └──► quick-xml (external)
```

### quarto-xml: Generic XML with Source Tracking

Analogous to quarto-yaml, this crate provides source-tracked XML parsing:

```rust
// quarto-xml/src/types.rs

use quarto_source_map::SourceInfo;

/// XML document with source tracking (analogous to YamlWithSourceInfo)
pub struct XmlWithSourceInfo {
    pub root: XmlElement,
    pub source_info: SourceInfo,
}

/// An XML element with source tracking
pub struct XmlElement {
    pub name: String,
    pub name_source: SourceInfo,
    pub attributes: Vec<XmlAttribute>,
    pub children: XmlChildren,
    pub source_info: SourceInfo,  // Entire element span
}

/// An XML attribute with source tracking
pub struct XmlAttribute {
    pub name: String,
    pub name_source: SourceInfo,
    pub value: String,
    pub value_source: SourceInfo,
}

/// Children of an XML element
pub enum XmlChildren {
    Elements(Vec<XmlElement>),
    Text { content: String, source_info: SourceInfo },
    Mixed(Vec<XmlChild>),
    Empty,
}

pub enum XmlChild {
    Element(XmlElement),
    Text { content: String, source_info: SourceInfo },
}
```

```rust
// quarto-xml/src/parser.rs

use quick_xml::{Reader, events::Event};

pub struct XmlParser<'a> {
    source: &'a str,
    reader: Reader<&'a [u8]>,
    file_id: FileId,
}

impl<'a> XmlParser<'a> {
    /// Parse XML source into XmlWithSourceInfo
    pub fn parse(source: &'a str, file_id: FileId) -> Result<XmlWithSourceInfo, XmlParseError> {
        let mut parser = Self::new(source, file_id);
        parser.parse_document()
    }
}
```

**Key design points:**
- Thin wrapper around quick-xml
- General-purpose: could parse JATS, DocBook, or any XML
- Tracks positions at element and attribute level
- Uses stack-based parsing like quarto-yaml

### Core Types with Source Tracking

```rust
// quarto-csl/src/style.rs

use quarto_source_map::SourceInfo;

pub struct Style {
    pub version: CslVersion,
    pub class: StyleClass,
    pub options: StyleOptions,
    pub info: StyleInfo,
    pub locales: Vec<Locale>,
    pub macros: HashMap<String, Macro>,
    pub citation: Layout,
    pub bibliography: Option<Layout>,
    pub source_info: SourceInfo,  // Whole style span
}

pub struct Macro {
    pub name: String,
    pub name_source: SourceInfo,  // Just the name attribute
    pub elements: Vec<Element>,
    pub source_info: SourceInfo,  // Whole macro element
}

pub struct Element {
    pub element_type: ElementType,
    pub formatting: Formatting,
    pub source_info: SourceInfo,
}

pub enum ElementType {
    Text(TextElement),
    Names(NamesElement),
    Date(DateElement),
    Number(NumberElement),
    Label(LabelElement),
    Group(GroupElement),
    Choose(ChooseElement),
}

pub struct TextElement {
    pub source: TextSource,
    pub source_info: SourceInfo,
}

pub enum TextSource {
    Variable { name: String, name_source: SourceInfo },
    Macro { name: String, name_source: SourceInfo },
    Term { name: String, form: TermForm },
    Value { value: String },
}

pub struct ChooseElement {
    pub branches: Vec<ChooseBranch>,
    pub source_info: SourceInfo,
}

pub struct ChooseBranch {
    pub conditions: Vec<Condition>,
    pub elements: Vec<Element>,
    pub source_info: SourceInfo,
}

pub struct Condition {
    pub test: ConditionTest,
    pub source_info: SourceInfo,
}
```

### quarto-csl: CSL-Specific Parsing

quarto-csl takes `XmlWithSourceInfo` from quarto-xml and produces semantic CSL types:

```rust
// quarto-csl/src/parser.rs

use quarto_xml::{XmlWithSourceInfo, XmlElement, XmlAttribute};
use quarto_source_map::SourceInfo;

/// Parse CSL from pre-parsed XML with source info
pub fn parse_csl(xml: &XmlWithSourceInfo) -> Result<Style, CslError> {
    let parser = CslParser::new();
    parser.parse_style(&xml.root)
}

/// Convenience function: parse CSL from source string
pub fn parse_csl_source(source: &str, file_id: FileId) -> Result<Style, CslError> {
    let xml = quarto_xml::parse(source, file_id)?;
    parse_csl(&xml)
}

struct CslParser {
    macros: HashMap<String, Macro>,
    locales: Vec<Locale>,
}

impl CslParser {
    fn parse_style(&mut self, element: &XmlElement) -> Result<Style, CslError> {
        self.expect_element_name(element, "style")?;

        let version = self.parse_version(element)?;
        let class = self.parse_class(element)?;
        let options = self.parse_options(element)?;

        for child in element.children.elements() {
            match child.name.as_str() {
                "info" => { /* parse info */ }
                "locale" => {
                    self.locales.push(self.parse_locale(child)?);
                }
                "macro" => {
                    let macro_def = self.parse_macro(child)?;
                    self.macros.insert(macro_def.name.clone(), macro_def);
                }
                "citation" => { /* parse citation layout */ }
                "bibliography" => { /* parse bibliography layout */ }
                _ => {}
            }
        }

        Ok(Style {
            version,
            class,
            options,
            macros: std::mem::take(&mut self.macros),
            locales: std::mem::take(&mut self.locales),
            // ...
            source_info: element.source_info.clone(),
        })
    }

    fn parse_macro(&self, element: &XmlElement) -> Result<Macro, CslError> {
        let name_attr = self.require_attr(element, "name")?;

        let elements = element.children.elements()
            .map(|child| self.parse_element(child))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Macro {
            name: name_attr.value.clone(),
            name_source: name_attr.value_source.clone(),
            elements,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_element(&self, element: &XmlElement) -> Result<Element, CslError> {
        let element_type = match element.name.as_str() {
            "text" => ElementType::Text(self.parse_text_element(element)?),
            "names" => ElementType::Names(self.parse_names_element(element)?),
            "date" => ElementType::Date(self.parse_date_element(element)?),
            "number" => ElementType::Number(self.parse_number_element(element)?),
            "label" => ElementType::Label(self.parse_label_element(element)?),
            "group" => ElementType::Group(self.parse_group_element(element)?),
            "choose" => ElementType::Choose(self.parse_choose_element(element)?),
            other => return Err(CslError::UnknownElement {
                name: other.to_string(),
                source_info: element.name_source.clone(),
            }),
        };

        let formatting = self.parse_formatting(element)?;

        Ok(Element {
            element_type,
            formatting,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_text_element(&self, element: &XmlElement) -> Result<TextElement, CslError> {
        let source = if let Some(attr) = self.get_attr(element, "variable") {
            TextSource::Variable {
                name: attr.value.clone(),
                name_source: attr.value_source.clone(),
            }
        } else if let Some(attr) = self.get_attr(element, "macro") {
            TextSource::Macro {
                name: attr.value.clone(),
                name_source: attr.value_source.clone(),
            }
        } else if let Some(attr) = self.get_attr(element, "term") {
            TextSource::Term {
                name: attr.value.clone(),
                form: self.parse_term_form(element)?,
            }
        } else if let Some(attr) = self.get_attr(element, "value") {
            TextSource::Value { value: attr.value.clone() }
        } else {
            return Err(CslError::MissingTextSource {
                source_info: element.source_info.clone(),
            });
        };

        Ok(TextElement {
            source,
            source_info: element.source_info.clone(),
        })
    }

    // Helper methods
    fn require_attr<'a>(&self, element: &'a XmlElement, name: &str) -> Result<&'a XmlAttribute, CslError> {
        element.attributes.iter()
            .find(|a| a.name == name)
            .ok_or_else(|| CslError::MissingAttribute {
                element: element.name.clone(),
                attribute: name.to_string(),
                source_info: element.source_info.clone(),
            })
    }

    fn get_attr<'a>(&self, element: &'a XmlElement, name: &str) -> Option<&'a XmlAttribute> {
        element.attributes.iter().find(|a| a.name == name)
    }
}
```

**Key design points:**
- Takes `XmlWithSourceInfo` as input (source tracking already done)
- Produces semantic CSL types (`Style`, `Element`, `TextSource`, etc.)
- Source info is propagated from XML elements to CSL types
- Validation (macro references, etc.) happens separately in `validate.rs`

### Error Types with Source Info

```rust
// quarto-csl/src/error.rs

use quarto_source_map::SourceInfo;

#[derive(Debug)]
pub enum CslError {
    ParseError(CslParseError),
    ValidationError(CslValidationError),
    ProcessingError(CslProcessingError),
}

#[derive(Debug)]
pub struct CslParseError {
    pub kind: ParseErrorKind,
    pub source_info: SourceInfo,
    pub message: String,
}

#[derive(Debug)]
pub enum ParseErrorKind {
    InvalidXml,
    MissingAttribute { element: String, attribute: String },
    InvalidAttributeValue { attribute: String, value: String },
    UnexpectedElement { expected: String, found: String },
    UnclosedElement { element: String },
}

#[derive(Debug)]
pub struct CslValidationError {
    pub kind: ValidationErrorKind,
    pub source_info: SourceInfo,
    pub message: String,
    pub notes: Vec<ValidationNote>,
}

#[derive(Debug)]
pub enum ValidationErrorKind {
    UndefinedMacro { name: String },
    UndefinedVariable { name: String },
    UndefinedTerm { name: String },
    CircularMacro { chain: Vec<String> },
    DuplicateMacro { name: String, first_defined: SourceInfo },
}

#[derive(Debug)]
pub struct ValidationNote {
    pub message: String,
    pub source_info: Option<SourceInfo>,
}
```

### Integration with quarto-error-reporting

```rust
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

impl CslValidationError {
    pub fn to_diagnostic(&self, context: &SourceContext) -> DiagnosticMessage {
        let mut builder = DiagnosticMessageBuilder::error(&self.message)
            .with_code(self.error_code())
            .with_primary_label(&self.source_info, &self.label_text());

        for note in &self.notes {
            if let Some(ref src) = note.source_info {
                builder = builder.with_secondary_label(src, &note.message);
            } else {
                builder = builder.with_note(&note.message);
            }
        }

        builder.build(context)
    }
}
```

## Implementation Phases

### Phase 0: XML Parsing with Source Tracking (quarto-xml)

**Goal:** Generic XML parser with source location tracking

- [ ] Set up quarto-xml crate with dependencies
- [ ] Define XmlWithSourceInfo, XmlElement, XmlAttribute types
- [ ] Implement stack-based parser wrapping quick-xml
- [ ] Track element positions (start, end, full span)
- [ ] Track attribute positions (name, value)
- [ ] Handle text content with positions
- [ ] Handle mixed content (elements + text)
- [ ] Create XmlParseError types with SourceInfo
- [ ] Unit tests with various XML structures

**Deliverable:** `quarto_xml::parse(source, file_id) -> Result<XmlWithSourceInfo, XmlParseError>`

### Phase 1: CSL Parsing (quarto-csl)

**Goal:** Parse CSL files into semantic types using quarto-xml

- [ ] Set up quarto-csl crate with quarto-xml dependency
- [ ] Define Style, Element, Formatting types with SourceInfo
- [ ] Implement XmlElement → Style conversion
- [ ] Parse all CSL element types (text, names, date, number, etc.)
- [ ] Parse formatting attributes
- [ ] Parse locale blocks
- [ ] Create CslError types with SourceInfo
- [ ] Unit tests with CSL examples

**Deliverable:** `quarto_csl::parse(xml: &XmlWithSourceInfo) -> Result<Style, CslError>`

### Phase 1b: CSL Validation

**Goal:** Validate CSL semantics and produce helpful errors

- [ ] Validate macro references exist
- [ ] Detect circular macro dependencies
- [ ] Validate variable names
- [ ] Validate term names
- [ ] Integrate with quarto-error-reporting

**Deliverable:** `quarto_csl::validate(style: &Style) -> Result<(), Vec<CslValidationError>>`

### Phase 2: Locale and Term Support

**Goal:** Handle localization

- [ ] Define Locale type with SourceInfo
- [ ] Parse inline locale overrides
- [ ] Load external locale files (embedded resources)
- [ ] Implement term lookup with fallback
- [ ] Date format localization
- [ ] Ordinal number formatting

**Deliverable:** `Locale::get_term(name, form) -> Option<&str>`

### Phase 3: Basic Citation Processing (quarto-citeproc)

**Goal:** Minimal citation rendering

- [ ] Define Reference, Citation, CitationItem types
- [ ] Implement CSL-JSON parsing for references
- [ ] Create evaluation context
- [ ] Evaluate Text elements (variable, macro, term, value)
- [ ] Evaluate Group elements with delimiters
- [ ] Apply formatting (font-style, text-case, prefix, suffix)
- [ ] Basic Names rendering (single author)
- [ ] Basic Date rendering

**Deliverable:** Simple citations like "(Smith, 2020)" work

### Phase 4: Complete Citation Processing

**Goal:** Full CSL 1.0.2 support

- [ ] Complex Names (et al., initialize-with, particles)
- [ ] Name substitution chains
- [ ] Conditional evaluation (choose/if/else)
- [ ] All condition types (type, variable, is-numeric, position)
- [ ] Number formatting (ordinal, long-ordinal)
- [ ] Page range formatting
- [ ] Sorting with Unicode collation
- [ ] Citation grouping and collapsing

### Phase 5: Disambiguation

**Goal:** Handle ambiguous citations

- [ ] Track citation positions (first, subsequent, ibid)
- [ ] Year-suffix disambiguation (2020a, 2020b)
- [ ] Add-names disambiguation
- [ ] Add-givenname disambiguation
- [ ] Near-note detection

### Phase 6: Output Integration

**Goal:** Integrate with quarto-markdown-pandoc

- [ ] Create output.rs with Inlines builder functions (italic, bold, link, span_with_class, etc.)
- [ ] Implement stringify() for test assertions
- [ ] Create filter entry point
- [ ] CLI integration (--citeproc flag)
- [ ] Bibliography generation

### Phase 7: Validation and Errors

**Goal:** Excellent error messages

- [ ] Validate CSL on load
- [ ] Report undefined macros with suggestions
- [ ] Report undefined variables
- [ ] Report missing required elements
- [ ] Integrate with quarto-error-reporting

## Potential Pitfalls and Mitigations

### 1. Complexity of Eval Algorithm

**Risk:** The Haskell Eval module is 2,836 lines of dense functional code with complex state threading.

**Mitigation:**
- Start with minimal subset (Text, Group, simple Names)
- Add features incrementally based on test suite
- Port the CSL test suite as acceptance tests
- Use Haskell implementation as reference, not literal translation

### 2. Unicode Collation

**Risk:** CSL requires language-aware sorting (e.g., Swedish ä after z, German ä = ae).

**Mitigation:**
- Use `icu_collator` crate for proper Unicode collation
- Or `rust_icu` bindings if needed
- Fall back to byte-order sorting for MVP

### 3. Name Parsing Edge Cases

**Risk:** Particles (von, de, van der), suffixes (Jr., III), non-Western names.

**Mitigation:**
- Port name parsing logic carefully
- Comprehensive test cases from citeproc test suite
- Document known limitations

### 4. Position Information for Attributes

**Risk:** quick-xml doesn't provide attribute positions directly.

**Mitigation:**
- Element-level positions are sufficient for most errors
- Calculate attribute positions manually if needed
- Focus on high-value error cases first

### 5. Locale Data Size

**Risk:** 60+ locale XML files embedded in binary.

**Mitigation:**
- Embed as compressed data
- Lazy loading of non-default locales
- Consider build-time code generation

### 6. Memory Usage

**Risk:** Source tracking doubles memory for large styles.

**Mitigation:**
- SourceInfo uses byte offsets only (small)
- Can drop source info after validation if needed
- Profile with real-world styles

## Dependencies

### quarto-xml

```toml
[dependencies]
quick-xml = "0.37"
quarto-source-map = { path = "../quarto-source-map" }
thiserror = "2.0"
```

### quarto-csl

```toml
[dependencies]
quarto-xml = { path = "../quarto-xml" }
quarto-source-map = { path = "../quarto-source-map" }
quarto-error-reporting = { path = "../quarto-error-reporting" }
thiserror = "2.0"
hashlink = "0.9"  # Ordered hash map for macros (preserves definition order)
```

### quarto-citeproc

```toml
[dependencies]
quarto-csl = { path = "../quarto-csl" }
quarto-source-map = { path = "../quarto-source-map" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"

# Optional
icu_collator = { version = "1.5", optional = true }  # Unicode collation
rust_embed = "8.0"    # Embed locale files
```

## Testing Strategy

### Overview: Two-Tier Test Suite

We will maintain two test tiers to balance development velocity with comprehensive coverage:

1. **Main Suite** - Tests that must pass; run by default with `cargo nextest run`
2. **Conformance Suite** - Full CSL/citeproc tests; pending tests are ignored by default

This design prevents a large number of failing tests from obscuring new regressions while still giving access to the full upstream test suite.

### Upstream Test Suites

**CSL Test Suite** (for quarto-csl):
- Location: `external-sources/citeproc/test/csl/`
- Count: 858 test cases
- Format: Multi-section text files with CSL, INPUT, RESULT sections
- Categories: 34 (name, date, disambiguate, sort, etc.)

**Citeproc Extra Tests** (for quarto-citeproc):
- Location: `external-sources/citeproc/test/extra/`
- Count: 38 additional tests
- Purpose: Issue-specific regression tests

### Test File Format

Each upstream test is a `.txt` file with sections:
```
>>===== MODE =====>>
citation
<<===== MODE =====<<

>>===== RESULT =====>>
(Smith, 2020)
<<===== RESULT =====<<

>>===== CSL =====>>
<style ...>...</style>
<<===== CSL =====<<

>>===== INPUT =====>>
[{"id": "smith2020", "type": "book", ...}]
<<===== INPUT =====<<
```

### Implementation: Build Script Generation

The `build.rs` generates test functions based on a manifest file:

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=tests/enabled_tests.txt");
    println!("cargo:rerun-if-changed=tests/data/csl-suite");

    let enabled = load_enabled_tests("tests/enabled_tests.txt");
    let mut generated = String::new();

    for test_file in glob("tests/data/csl-suite/*.txt") {
        let test_name = sanitize_name(&test_file);
        let ignored = if enabled.contains(&test_name) { "" } else { "#[ignore]" };

        generated.push_str(&format!(r#"
            {ignored}
            #[test]
            fn csl_{test_name}() {{
                run_csl_test(include_str!("{path}"));
            }}
        "#, path = test_file.display()));
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(
        Path::new(&out_dir).join("generated_csl_tests.rs"),
        generated
    ).unwrap();
}
```

```rust
// tests/csl_conformance.rs
mod test_runner;
use test_runner::run_csl_test;

include!(concat!(env!("OUT_DIR"), "/generated_csl_tests.rs"));
```

### Manifest File Format

`tests/enabled_tests.txt`:
```
# Tests that must pass (promoted from conformance suite)
# Add tests here as features are implemented
# Format: one test name per line, # for comments

# Basic text rendering
affix_WithCommas
textcase_Lowercase
textcase_Uppercase

# Date formatting
date_YearOnly
date_MonthYear

# Names (basic)
name_SingleAuthor
name_TwoAuthors
```

### Running Tests

```bash
# Default: unit tests + enabled conformance tests
cargo nextest run

# Run only pending conformance tests (to check progress)
cargo nextest run -- --ignored

# Run ALL tests including pending (full conformance check)
cargo nextest run -- --include-ignored

# Run specific category
cargo nextest run csl_name_

# Filter by pattern
cargo nextest run -E 'test(/csl_date/)'
```

### Test Promotion Workflow

When implementing a feature:

1. **Identify relevant tests**: `cargo nextest run -- --ignored 2>&1 | grep PASS`
2. **Verify tests pass**: `cargo nextest run csl_name_SingleAuthor -- --ignored`
3. **Promote to main suite**: Add `name_SingleAuthor` to `enabled_tests.txt`
4. **Commit**: The test is now part of the regression suite

Git diff shows exactly which tests were promoted:
```diff
+ name_SingleAuthor
+ name_TwoAuthors
```

### Potential Issues and Mitigations

#### 1. Compile Time for 900+ Tests

**Issue:** Generating and compiling 900+ test functions could be slow.

**Mitigations:**
- **Feature flag**: Add `conformance-suite` feature; without it, only enabled tests compile
- **Incremental compilation**: Rust handles this well; only changed tests recompile
- **Separate test binary**: Put conformance tests in separate integration test file

```toml
# Cargo.toml
[features]
default = []
conformance-suite = []  # Compile all 900+ tests

# Without feature: only enabled tests compile
# With feature: all tests compile, pending ones are #[ignore]
```

#### 2. Test Discovery in IDEs

**Issue:** Generated tests may not appear in IDE test explorers.

**Mitigations:**
- Generated code is deterministic; some IDEs handle this
- Use `include!` with stable paths
- Fall back to command-line test running
- Consider `test-case` crate for better IDE support in unit tests

#### 3. Parsing Upstream Test Format

**Issue:** Need robust parser for CSL test file format.

**Mitigations:**
- Format is simple (section markers + content)
- We have 900 examples to validate parser
- Parser lives in test infrastructure, not production code

#### 4. Test Output Comparison

**Issue:** Output may differ in whitespace, entity encoding, etc.

**Mitigations:**
- Normalize HTML before comparison
- Strip citation numbers as Haskell citeproc does
- Document known differences
- Allow fuzzy matching for specific cases

#### 5. Keeping Upstream Tests in Sync

**Issue:** CSL test suite may update upstream.

**Mitigations:**
- Vendor tests in our repo (copy, not symlink)
- Document upstream version in README
- Periodic sync script with diff review

### Test Categories

Tests will be organized by category (matching upstream):

| Category | Count | MVP Target | Notes |
|----------|-------|------------|-------|
| affix | 9 | 9 | Prefix/suffix handling |
| textcase | 31 | 31 | Case transformations |
| date | 101 | 30 | Basic dates first |
| name | 111 | 20 | Single/two authors first |
| nameattr | 97 | 10 | Basic attributes |
| number | 20 | 10 | Simple formatting |
| label | 19 | 10 | Basic labels |
| group | 7 | 7 | Grouping logic |
| condition | 17 | 10 | Basic conditionals |
| sort | 66 | 0 | Phase 4 |
| disambiguate | 72 | 0 | Phase 5 |
| collapse | 21 | 0 | Phase 4 |
| position | 16 | 0 | Phase 5 |

**MVP Target:** ~150 tests enabled (covering Phases 1-3)

### Unit Tests (Hand-Written)

In addition to conformance tests, write focused unit tests:

```rust
// tests/unit/parser_tests.rs
#[test]
fn test_parse_empty_style() {
    let result = parse_csl("<style/>");
    assert!(result.is_err());
}

#[test]
fn test_source_info_accuracy() {
    let csl = "<style><macro name=\"test\"/></style>";
    let style = parse_csl(csl).unwrap();
    let macro_def = &style.macros["test"];
    assert_eq!(macro_def.source_info.start_offset, 7);
    assert_eq!(macro_def.name_source.start_offset, 20);
}

#[test]
fn test_circular_macro_detection() {
    let csl = r#"
        <style>
            <macro name="a"><text macro="b"/></macro>
            <macro name="b"><text macro="a"/></macro>
        </style>
    "#;
    let err = parse_csl(csl).unwrap_err();
    assert!(matches!(err.kind, ValidationErrorKind::CircularMacro { .. }));
}
```

### Integration Tests

```rust
// tests/integration/full_styles.rs
#[test]
fn test_apa_style_parses() {
    let apa = include_str!("data/styles/apa.csl");
    let style = parse_csl(apa).unwrap();
    assert_eq!(style.info.title, "American Psychological Association 7th edition");
}

#[test]
fn test_chicago_style_parses() {
    let chicago = include_str!("data/styles/chicago-fullnote-bibliography.csl");
    let style = parse_csl(chicago).unwrap();
    assert!(style.bibliography.is_some());
}
```

### CI Configuration

```yaml
# .github/workflows/test.yml
jobs:
  test:
    steps:
      - name: Run main test suite
        run: cargo nextest run

      - name: Check conformance progress (informational)
        run: |
          echo "=== Conformance Suite Progress ==="
          cargo nextest run -- --ignored 2>&1 | grep -E "(PASS|FAIL)" | sort | uniq -c
        continue-on-error: true  # Don't fail CI on pending tests
```

## Estimated Effort

| Phase | Effort | Notes |
|-------|--------|-------|
| Phase 0: quarto-xml | 1 week | Analogous to quarto-yaml |
| Phase 1: quarto-csl parsing | 2 weeks | XmlWithSourceInfo → Style |
| Phase 1b: CSL validation | 0.5 weeks | Macro refs, circular deps |
| Phase 2: Locales | 1 week | Straightforward |
| Phase 3: Basic Processing | 2 weeks | Core algorithm |
| Phase 4: Complete Processing | 3-4 weeks | Complex logic |
| Phase 5: Disambiguation | 2 weeks | Tricky edge cases |
| Phase 6: Output Integration | 1 week | Glue code |
| Phase 7: Error polish | 1 week | Beautiful error messages |

**Total:** 13-16 weeks for full implementation

**MVP (Phases 0-3):** 6-7 weeks for basic citations

## Alternatives Considered

### 1. Wrap Haskell Citeproc via FFI

**Pros:** Full compatibility immediately
**Cons:** Complex build, Haskell runtime, no source tracking

### 2. Use citationberg (Rust CSL library)

**Pros:** Existing Rust implementation
**Cons:** No source tracking, may not be Pandoc-compatible, unclear maintenance status

### 3. Call pandoc --citeproc

**Pros:** Simplest, full compatibility
**Cons:** Process spawning overhead, external dependency, no source tracking

### Recommendation

Build custom implementation (this design) because:
1. Source tracking is a key differentiator
2. Native integration is valuable
3. We control the implementation
4. Phased approach manages risk

## References

- CSL 1.0.2 Specification: https://docs.citationstyles.org/en/stable/specification.html
- Haskell citeproc: `external-sources/citeproc/`
- quick-xml: `external-sources/quick-xml/`
- quarto-yaml (pattern for quarto-xml): `crates/quarto-yaml/`
- CiteprocOutput analysis: `claude-notes/research/2025-11-26-citeproc-output-architecture.md`
