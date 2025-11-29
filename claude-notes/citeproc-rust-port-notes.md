# Citeproc Rust Port Implementation Notes

## CRITICAL: Test Regression Policy

**NEVER end a session with fewer passing tests than when you started.**

Before making changes to quarto-citeproc:
1. Run `cargo nextest run -p quarto-citeproc` and note the passing test count
2. Check the header of `tests/enabled_tests.txt` for the baseline (e.g., "463/858")
3. After ALL changes, verify the test count is >= the starting count
4. If tests regressed, FIX THEM before ending the session

Always verify test counts before and after implementation work.

---

## Critical Design Decisions for Rust Implementation

### 1. Output Type System - Trait vs Enum vs Generic

**Haskell Approach**: Uses parametric polymorphism with a typeclass. The entire library is generic over the output type.

**Rust Options**:

**Option A: Generic Trait (Most Similar to Haskell)**
```rust
pub trait CiteprocOutput: Semigroup + Monoid + Clone + Debug + Eq + Ord {
    fn to_text(&self) -> Text;
    fn from_text(t: Text) -> Self;
    fn add_font_variant(&self, v: FontVariant) -> Self;
    // ... 13 more methods
}

// Your processor
pub struct CitationProcessor<O: CiteprocOutput> {
    // ...
}

impl<O: CiteprocOutput> CitationProcessor<O> {
    pub fn process(&self, citations: &[Citation<O>]) -> Result<Vec<O>, Error> {
        // Generic implementation
    }
}
```
**Pros**: Maximum code reuse, matches Haskell design
**Cons**: Trait object overhead if you need dynamic dispatch, harder to optimize per format

**Option B: Enum with Format Variants (Simplest)**
```rust
pub enum OutputFormat {
    CslJson(CslJsonOutput),
    Pandoc(PandocOutput),
    Custom(Box<dyn CustomOutput>),
}

impl OutputFormat {
    fn add_font_variant(&mut self, v: FontVariant) {
        match self {
            OutputFormat::CslJson(o) => o.add_font_variant(v),
            // ...
        }
    }
}
```
**Pros**: Simple, fast (monomorphic dispatch), no lifetime issues
**Cons**: Must anticipate all output types upfront, more boilerplate

**Option C: Procedural AST + Backend (Recommended)**
Build an intermediate AST in the evaluation phase (analogous to Haskell's `Output` type), then have separate backends that convert to different formats. This decouples rendering from citation logic.

```rust
pub enum Output {
    Formatted(Formatting, Vec<Output>),
    Linked(String, Vec<Output>),
    InNote(Box<Output>),
    Literal(String),
    Tagged(Tag, Box<Output>),
    NullOutput,
}

// Then separate backends:
pub trait OutputBackend {
    fn render(&self, output: &Output) -> Self;
}
```

**Recommendation**: Start with **Option C** (intermediate AST), then implement backends for each target format. This matches the Haskell approach more closely while being idiomatic Rust.

### 2. Evaluation Context Threading

**Haskell**: Uses RWS monad (Reader-Writer-State). Context is implicit.

**Rust Approach**: Explicit threading with a context struct:

```rust
struct EvalContext<'a> {
    locale: &'a Locale,
    style: &'a Style,
    macros: &'a Map<String, Vec<Element>>,
    position: Position,
    // ... other fields
}

struct EvalState {
    last_cited_map: HashMap<ItemId, (u32, Option<u32>, u32, bool, Option<String>, Option<String>)>,
    note_map: HashMap<u32, HashSet<ItemId>>,
    ref_map: ReferenceMap,
    warnings: Vec<String>,
    // ...
}

fn eval_element(elem: &Element, ctx: &EvalContext, state: &mut EvalState) -> Result<Output, Error> {
    // Implementation
}
```

**Key Points**:
- Context is immutable (use references)
- State is mutable (use `&mut`)
- Return `Result<Output, Error>` instead of monadic computations
- Collect warnings in state rather than writer monad

### 3. Immutable Maps and Updates

**Haskell**: Uses `M.adjust` and `M.insert` on immutable Maps.

**Rust**: Two approaches:

**Option A: Use Immutable Structures (im-rs crate)**
```rust
use im::HashMap;

let mut refs = state.ref_map.clone();
refs.alter(item_id, |_| {
    // Transform the reference
});
state.ref_map = refs;
```
**Pros**: Closer to Haskell semantics, easier to reason about
**Cons**: Performance overhead, requires additional dependencies

**Option B: Interior Mutability + Mutable References**
```rust
state.ref_map.get_mut(item_id).map(|r| {
    r.variables.insert("citation-number".into(), Val::Num(1));
});
```
**Pros**: Standard Rust idiom, better performance
**Cons**: Less immutable semantics

**Recommendation**: Use **Option B** for the reference map and similar structures. Only use immutable structures where Haskell's RWS demands it.

### 4. Monadic Composition to Iterative Loops

**Problem**: Haskell uses monadic do-notation for sequential operations:

```haskell
do
  assignCitationNumbers sortedIds
  bibSortKeyMap <- M.fromList <$> mapM ... refs
  sortKeyMap <- foldM (\m citeId -> ...) M.empty citeIds
  cs <- disambiguateCitations style bibSortKeyMap citCitations
  bs <- case styleBibliography style of ...
```

**Rust Solution**: Use explicit imperative loops:

```rust
assign_citation_numbers(&mut state, &sorted_ids)?;

let mut bib_sort_key_map = HashMap::new();
for reference in &refs {
    let sort_keys = eval_sort_keys(&bibliography_layout, reference.id(), ctx, state)?;
    bib_sort_key_map.insert(reference.id(), sort_keys);
}

let mut sort_key_map = HashMap::new();
for cite_id in &cite_ids {
    let sort_key = eval_sort_keys(&citation_layout, cite_id, ctx, state)?;
    sort_key_map.insert(cite_id, sort_key);
}

let cs = disambiguate_citations(&style, &bib_sort_key_map, &citations, ctx, state)?;
```

### 5. Tree Traversal and Transformation

**Haskell**: Uses `Data.Generics.Uniplate.Operations` for generic tree walking:

```haskell
let handleSuppressAuthors = transform removeNamesIfSuppressAuthor
```

**Rust**: Must write explicit recursive functions:

```rust
fn remove_names_if_suppress_author(output: &mut Output) {
    match output {
        Output::Tagged(Tag::Item(CitationItemType::SuppressAuthor, _), ref mut inner) => {
            if let Some(author_output) = get_authors(inner) {
                transform_output(inner, |node| {
                    if node == &author_output {
                        Output::NullOutput
                    } else {
                        node.clone()
                    }
                });
            }
        }
        Output::Formatted(_, ref mut children) => {
            for child in children.iter_mut() {
                remove_names_if_suppress_author(child);
            }
        }
        // ... other variants
        _ => {}
    }
}

fn transform_output<F>(output: &mut Output, f: &F) where F: Fn(&Output) -> Output {
    match output {
        Output::Formatted(fmt, children) => {
            for child in children.iter_mut() {
                transform_output(child, f);
            }
            *output = f(output);
        }
        // ... recursive for other node types
    }
}
```

**Better Approach**: Use a visitor pattern or write small, focused traversal functions rather than a generic library.

### 6. Pattern Matching and Conditions

**Haskell**: Comprehensive pattern matching at multiple levels:

```haskell
evalCondition (HasVariable v) ref = isJust $ lookupVariable v ref
evalCondition (HasType types) ref = referenceType ref `elem` types
evalCondition (IsNumeric v) ref = case lookupVariable v ref of
  Just (NumVal _) -> True
  Just (TextVal t) -> isJust $ readAsInt t
  _ -> False
```

**Rust**: Use match or if-let:

```rust
fn eval_condition(condition: &Condition, reference: &Reference) -> bool {
    match condition {
        Condition::HasVariable(v) => reference.lookup_variable(v).is_some(),
        Condition::HasType(types) => types.contains(&reference.type_),
        Condition::IsNumeric(v) => {
            match reference.lookup_variable(v) {
                Some(Val::Num(_)) => true,
                Some(Val::Text(t)) => t.parse::<i32>().is_ok(),
                _ => false,
            }
        }
        // ...
    }
}
```

### 7. Sorting and Collation

**Haskell**: Higher-order function:
```haskell
let sortedIds = sortBy (\x y -> collate 
    (fromMaybe [] $ M.lookup x sortKeyMap)
    (fromMaybe [] $ M.lookup y sortKeyMap))
    (map referenceId refs)
```

**Rust**: Use Ord or custom sort:

```rust
let mut sorted_ids: Vec<ItemId> = refs.iter().map(|r| r.id).collect();

sorted_ids.sort_by(|x, y| {
    let x_keys = sort_key_map.get(x).unwrap_or(&vec![]);
    let y_keys = sort_key_map.get(y).unwrap_or(&vec![]);
    collate(x_keys, y_keys)
});
```

**Key Implementation**: The `collate` function must implement Unicode Collation Algorithm (UCA). Use the `unicode-collation` crate or implement minimally for your needs.

```rust
fn collate(x: &[String], y: &[String]) -> std::cmp::Ordering {
    for (xi, yi) in x.iter().zip(y.iter()) {
        match xi.cmp(yi) {  // Case-insensitive comparison
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    x.len().cmp(&y.len())
}
```

### 8. Disambiguation Algorithm

**Critical Difference**: Haskell's approach is iterative with re-rendering. Rust should follow the same pattern:

```rust
pub fn disambiguate_citations(
    style: &Style,
    bib_sort_key_map: &HashMap<ItemId, Vec<SortKeyValue>>,
    citations: &[Citation],
    ctx: &EvalContext,
    state: &mut EvalState,
) -> Result<Vec<Output>, Error> {
    loop {
        let rendered = render_citations(citations, ctx, state)?;
        let ambiguities = get_ambiguities(&rendered);
        
        if ambiguities.is_empty() {
            return Ok(rendered);
        }
        
        // Try to resolve ambiguities
        if style.options.disambiguation.add_names {
            try_add_names(&ambiguities, ctx, state)?;
        } else if let Some(rule) = &style.options.disambiguation.add_given_names {
            try_add_given_names(rule, &ambiguities, ctx, state)?;
        } else if style.options.disambiguation.add_year_suffix {
            add_year_suffixes(bib_sort_key_map, &ambiguities, state)?;
        } else {
            break;  // Can't resolve further
        }
    }
    
    Ok(rendered)
}
```

### 9. Name Handling Complexity

Names are the most complex part. Key structures:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Name {
    pub family: Option<String>,
    pub given: Option<String>,
    pub dropping_particle: Option<String>,
    pub non_dropping_particle: Option<String>,
    pub suffix: Option<String>,
    pub comma_suffix: bool,
    pub static_ordering: bool,
    pub literal: Option<String>,
}

// Name formatting is complex
#[derive(Clone, Debug)]
pub struct NameFormat {
    pub given_formatting: Option<Formatting>,
    pub family_formatting: Option<Formatting>,
    pub and_style: Option<TermForm>,     // "and" vs "&"
    pub delimiter: Option<String>,
    pub delimiter_precedes_et_al: Option<DelimiterPrecedes>,
    pub delimiter_precedes_last: Option<DelimiterPrecedes>,
    pub et_al_min: Option<usize>,
    pub et_al_use_first: Option<usize>,
    pub et_al_subsequent_use_first: Option<usize>,
    pub et_al_subsequent_min: Option<usize>,
    pub et_al_use_last: Option<bool>,
    pub form: Option<NameForm>,           // long vs short
    pub initialize: Option<bool>,
    pub initialize_with: Option<String>,
    pub as_sort_order: Option<NameAsSortOrder>,
    pub sort_separator: Option<String>,
}
```

**Important Implementation Notes**:
1. Particles (van, de, von, etc.) must be extracted during parsing
2. Name initialization requires Unicode-aware character handling
3. Et al. cascades must be handled carefully (et al., et al.*, et al.+)
4. Name sorting differs from name display

### 10. Date Handling

Implement EDTF (Extended Date Time Format) parsing:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Date {
    pub parts: Vec<DateParts>,       // Can have multiple for ranges
    pub circa: bool,
    pub season: Option<u8>,          // 1-4 or 13-16
    pub literal: Option<String>,     // Fallback text
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateParts {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

// For sorting, dates need special handling
fn date_to_sort_key(d: &Date) -> String {
    // Negative years sort before positive
    // Format: P/N + padded year + padded month + padded day
    d.parts.iter().map(|p| {
        if p.year < 0 {
            format!("N{:09}", 999_999_999 + p.year)
        } else {
            format!("P{:09}", p.year)
        }
    }).collect()
}
```

### 11. Performance Considerations

Unlike Haskell's lazy evaluation, Rust is eager. This matters:

1. **Pre-compute Sort Keys**: Don't recompute during sort operations
2. **String Handling**: Pre-allocate capacity, avoid repeated concatenation
3. **HashMap vs BTreeMap**: Use HashMap for most lookups (reference map), BTreeMap only if ordering matters
4. **Avoid Clones**: Use references where possible, especially in Context

```rust
// Bad
for ref in &refs {
    let mut r = ref.clone();
    r.variables.insert(k, v);
    state.ref_map.insert(ref.id, r);
}

// Good
for ref in &mut refs {
    ref.variables.insert(k, v);
}
state.ref_map = refs.into_iter()
    .map(|r| (r.id, r))
    .collect();
```

### 12. Error Handling Strategy

Haskell's approach is implicit via Maybe/Either. Rust requires explicit Result:

```rust
#[derive(Debug, Clone)]
pub enum CiteprocError {
    XmlError(String),
    ParseError(String),
    LocaleNotFound(String),
    ReferenceNotFound(ItemId),
    UndefinedMacro(String),
    InvalidDate(String),
}

impl std::fmt::Display for CiteprocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CiteprocError::XmlError(s) => write!(f, "XML Error: {}", s),
            CiteprocError::ParseError(s) => write!(f, "Parse Error: {}", s),
            // ...
        }
    }
}

impl std::error::Error for CiteprocError {}
```

Use `?` operator instead of monadic error handling:

```rust
fn eval_element(elem: &Element, ctx: &EvalContext, state: &mut EvalState) -> Result<Output, CiteprocError> {
    match elem {
        Element::Text(TextType::Variable(var)) => {
            let ref = state.get_reference()?;  // Returns early on None
            ref.lookup_variable(var)
                .ok_or_else(|| CiteprocError::UndefinedVariable(var.clone()))
        }
        // ...
    }
}
```

### 13. Testing Strategy

Key test categories:

1. **Unit Tests**: Individual functions (sort, disambiguation, formatting)
2. **Integration Tests**: Full citation processing with known outputs
3. **Spec Compliance Tests**: CSL test suite from https://github.com/citation-style-language/test-suite
4. **Edge Cases**: 
   - Unicode names and diacritics
   - Multiple et al. rules
   - Year suffix disambiguation
   - Empty/missing data

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_citation_numbering() {
        let style = parse_style(CHICAGO_STYLE).unwrap();
        let refs = vec![make_reference("a"), make_reference("b")];
        let citations = vec![make_citation("b"), make_citation("a")];
        
        let result = citeproc(&style, &refs, &citations).unwrap();
        
        assert_eq!(result.citations[0].to_text(), "[1]");
        assert_eq!(result.citations[1].to_text(), "[2]");
        assert_eq!(result.bibliography[0].0, "a");
        assert_eq!(result.bibliography[1].0, "b");
    }
}
```

## Module Organization for Rust Port

```
quarto-citeproc/
├── src/
│   ├── lib.rs                 # Main library interface
│   ├── types.rs               # Core data types (~1000 lines)
│   ├── parse/
│   │   ├── mod.rs
│   │   ├── style.rs           # XML style parsing
│   │   ├── locale.rs          # Locale file parsing
│   │   └── reference.rs       # CSL JSON parsing
│   ├── eval/
│   │   ├── mod.rs             # Main evaluation (~1500 lines)
│   │   ├── sort.rs            # Sorting and collation
│   │   ├── disambiguate.rs    # Disambiguation algorithm
│   │   └── names.rs           # Name formatting
│   ├── output/
│   │   ├── mod.rs
│   │   ├── csl_json.rs        # CSL JSON output backend
│   │   ├── text.rs            # Plain text output
│   │   └── pandoc.rs          # Pandoc integration
│   └── formatting/
│       ├── mod.rs
│       ├── case.rs            # Case transformation
│       ├── locale.rs          # Locale-specific formatting
│       └── punctuation.rs     # Punctuation rules
└── tests/
    ├── integration_tests.rs
    └── fixtures/              # Test CSL files and data
```

## Critical Implementation Order

1. **Types**: Get data structures right first
2. **Parsing**: Parse styles and references
3. **Basic Output**: Render simple output (text or JSON)
4. **Element Evaluation**: Implement element-by-element evaluation
5. **Names**: Complex name logic
6. **Sorting**: Sort keys and collation
7. **Disambiguation**: Add names, given names, year suffixes
8. **Bibliography**: Separate layout
9. **Polish**: Quote handling, punctuation, locale rules

## Resources

- CSL Spec: https://docs.citationstyles.org/en/stable/specification.html
- Pandoc citeproc: https://github.com/jgm/citeproc
- CSL Test Suite: https://github.com/citation-style-language/test-suite
- Unicode Collation: https://www.unicode.org/reports/tr10/
- EDTF Spec: https://www.loc.gov/standards/datetime/
