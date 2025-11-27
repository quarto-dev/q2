# Pandoc Citeproc Implementation Architecture

## Overview

The Pandoc citeproc implementation is a Haskell library that processes CSL (Citation Style Language) JSON citation data according to CSL XML stylesheets. It's designed as a pluggable system with a type-parameterized output abstraction layer, making it work with any structured format that implements the `CiteprocOutput` typeclass.

**Key Location**: `external-sources/citeproc/src/Citeproc/`

## Core Module Structure

```
Citeproc.hs                      # Main entry point - exports citeproc function
├── Types.hs                     # Core data types and output abstraction
├── Style.hs                     # CSL stylesheet parsing
├── Element.hs                   # XML element parsing utilities
├── Eval.hs                      # Evaluation/rendering pipeline (2836 lines)
├── CslJson.hs                   # CSL JSON pseudo-HTML format implementation
├── CaseTransform.hs             # Text case transformation logic
├── Locale.hs                    # Locale loading and merging
├── Pandoc.hs                    # Pandoc integration
└── Unicode.hs                   # Unicode collation and normalization
```

## Key Data Types

### 1. **Output Abstraction: CiteprocOutput Typeclass**

The entire system is built around a polymorphic `CiteprocOutput` typeclass that abstracts over output formats:

```haskell
class (Semigroup a, Monoid a, Show a, Eq a, Ord a) => CiteprocOutput a where
  toText                      :: a -> Text
  fromText                    :: Text -> a
  dropTextWhile               :: (Char -> Bool) -> a -> a
  dropTextWhileEnd            :: (Char -> Bool) -> a -> a
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

**Design Pattern**: Type-class polymorphism enables the library to work with:
- `CslJson Text` - CSL JSON pseudo-HTML format
- `Pandoc.Inlines` - Pandoc's document structure
- Any other format that implements the operations

This is fundamentally different from a Rust implementation which would likely use traits or enums with pattern matching.

### 2. **Output Type**

The `Output` type is the intermediate representation before final rendering:

```haskell
data Output a =
    Formatted Formatting [Output a]   -- Formatting with child elements
  | Linked Text [Output a]            -- Hyperlink with URL and children
  | InNote (Output a)                 -- Footnote/endnote wrapper
  | Literal a                         -- Leaf literal value (polymorphic)
  | Tagged Tag (Output a]             -- Metadata tag for post-processing
  | NullOutput                        -- Sentinel for empty content
```

**Tags** provide metadata for later processing:
- `TagTerm Term` - Locale term reference
- `TagCitationNumber Int` - Numbered citations
- `TagCitationLabel` - Label-based citations
- `TagTitle` - Title for hyperlinking
- `TagItem CitationItemType ItemId` - Citation item reference
- `TagName Name` - Author/editor name
- `TagNames Variable NamesFormat [Name]` - Multiple names
- `TagDate Date` - Structured date
- `TagYearSuffix Int` - Disambiguation suffix
- `TagLocator`, `TagPrefix`, `TagSuffix` - Affixes

### 3. **Formatting**

Formatting is orthogonal to structure - all formatting attributes are collected separately:

```haskell
data Formatting =
  Formatting
  { formatLang           :: Maybe Lang
  , formatFontStyle      :: Maybe FontStyle
  , formatFontVariant    :: Maybe FontVariant
  , formatFontWeight     :: Maybe FontWeight
  , formatTextDecoration :: Maybe TextDecoration
  , formatVerticalAlign  :: Maybe VerticalAlign
  , formatPrefix         :: Maybe Text
  , formatSuffix         :: Maybe Text
  , formatDisplay        :: Maybe DisplayStyle      -- block, left-margin, etc.
  , formatTextCase       :: Maybe TextCase
  , formatDelimiter      :: Maybe Text
  , formatStripPeriods   :: Bool
  , formatQuotes         :: Bool
  , formatAffixesInside  :: Bool
  }
```

**Design Pattern**: Formatting is applied compositionally using `addFormatting` function which chains operations in a specific order (affixes, quotes, case, style, etc.).

### 4. **Bibliography Data**

```haskell
-- Reference contains all metadata about a single work
data Reference a =
  Reference
  { referenceId             :: ItemId
  , referenceType           :: Text
  , referenceDisambiguation :: Maybe DisambiguationData
  , referenceVariables      :: M.Map Variable (Val a)
  }

-- Values in references have different types
data Val a =
    TextVal Text           -- Plain strings
  | FancyVal a             -- Formatted values (locale-specific)
  | NumVal Int             -- Numeric values
  | NamesVal [Name]        -- Structured author/editor names
  | DateVal Date           -- Structured dates
  | SubstitutedVal         -- Suppressed by substitution rules

-- Structured name representation
data Name =
  Name
  { nameFamily              :: Maybe Text
  , nameGiven               :: Maybe Text
  , nameDroppingParticle    :: Maybe Text    -- "van", "von", etc.
  , nameNonDroppingParticle :: Maybe Text    -- "de", "di", etc.
  , nameSuffix              :: Maybe Text    -- "Jr.", "III", etc.
  , nameCommaSuffix         :: Bool
  , nameStaticOrdering      :: Bool
  , nameLiteral             :: Maybe Text
  }
```

**Key Insight**: Names have sophisticated particle handling following CSL spec (non-dropping vs dropping particles for proper alphabetization).

### 5. **Dates**

```haskell
data Date =
  Date
  { dateParts     :: [DateParts]    -- Can be multiple for ranges
  , dateCirca     :: Bool            -- "circa" flag
  , dateSeason    :: Maybe Int       -- 1-4 for season, 13-16 for seasons
  , dateLiteral   :: Maybe Text      -- Fallback literal string
  }

newtype DateParts = DateParts [Int]  -- [year], [year, month], or [year, month, day]
```

Dates are EDTF-aware and support ranges, seasons, and literal fallbacks.

### 6. **Variables**

The system uses case-insensitive variables (50+ defined types):

```haskell
newtype Variable = Variable (CI.CI Text)  -- Case-insensitive Text wrapper

-- Variable types are classified:
data VariableType =
    DateVariable         -- issued, accessed, etc.
  | NameVariable         -- author, editor, translator, etc.
  | NumberVariable       -- volume, issue, page, etc.
  | StringVariable       -- DOI, ISBN, URL, etc.
  | StandardVariable     -- title, abstract, etc.
  | UnknownVariable      -- Custom/unknown variables
```

## Citation Processing Pipeline

### 1. **Input Types**

```haskell
data Citation a =
  Citation
  { citationId         :: Maybe Text
  , citationNoteNumber :: Maybe Int           -- For note styles
  , citationPrefix     :: Maybe a
  , citationSuffix     :: Maybe a
  , citationItems      :: [CitationItem a]    -- One or more works
  }

data CitationItem a =
  CitationItem
  { citationItemId      :: ItemId
  , citationItemLabel   :: Maybe Text         -- Custom label
  , citationItemLocator :: Maybe Text         -- "p. 30", "ch. 2"
  , citationItemType    :: CitationItemType   -- normal-cite, author-only, suppress-author
  , citationItemPrefix  :: Maybe a
  , citationItemSuffix  :: Maybe a
  , citationItemData    :: Maybe (Reference a) -- Inline reference data
  }

data CitationItemType = AuthorOnly | SuppressAuthor | NormalCite
```

### 2. **Evaluation Context (RWS Monad)**

The evaluation uses Haskell's RWS (Reader-Writer-State) monad for clean threading:

```haskell
type Eval a = RWS (Context a) (Set.Set Text) (EvalState a)

data Context a =
  Context
  { contextLocale              :: Locale
  , contextCollate             :: [SortKeyValue] -> [SortKeyValue] -> Ordering
  , contextAbbreviations       :: Maybe Abbreviations
  , contextStyleOptions        :: StyleOptions
  , contextMacros              :: M.Map Text [Element a]
  , contextLocator             :: Maybe Text
  , contextLabel               :: Maybe Text
  , contextPosition            :: [Position]
  , contextInSubstitute        :: Bool
  , contextInSortKey           :: Bool
  , contextInBibliography      :: Bool
  , contextSubstituteNamesForm :: Maybe NamesFormat
  , contextNameFormat          :: NameFormat
  }

data EvalState a =
  EvalState
  { stateVarCount       :: VarCount
  , stateLastCitedMap   :: M.Map ItemId (Int, Maybe Int, Int, Bool, Maybe Text, Maybe Text)
  , stateNoteMap        :: M.Map Int (Set.Set ItemId)
  , stateRefMap         :: ReferenceMap a
  , stateReference      :: Reference a           -- Current reference being rendered
  , stateUsedYearSuffix :: Bool                  -- For disambiguation tracking
  , stateUsedIdentifier :: Bool                  -- For hyperlink tracking
  , stateUsedTitle      :: Bool
  }
```

**Design Pattern**: Monadic threading allows clean separation of concerns - Reader for immutable context, Writer for warnings, State for mutable tracking.

### 3. **Main Evaluation Flow (evalStyle)**

```
evalStyle :: CiteprocOutput a
          => Style a          -- CSL stylesheet
          -> Maybe Lang       -- Locale override
          -> [Reference a]    -- Bibliographic data
          -> [Citation a]     -- Citations to process
          -> ([Output a], [(Text, Output a)], [Text])
          --  (citations, (id, bibentry) pairs, warnings)
```

**High-level pipeline**:

1. **Prepare**: Extract inline item data from citations, create reference map
2. **Sort bibliography**: Evaluate sort keys, apply collation, assign citation numbers
3. **Sort citations**: Sort citation items within groups (honoring prefix/suffix isolation)
4. **Disambiguate**: Resolve ambiguities by adding names, given names, or year suffixes
5. **Render citations**: Apply citation layout template
6. **Handle special cases**: Author-only citations, suppress-author, note styles
7. **Render bibliography**: Apply bibliography layout template
8. **Apply substitution rules**: Replace names with symbols in subsequent entries
9. **Post-process**: Apply quote localization and punctuation rules

### 4. **Sorting Implementation**

```haskell
data SortKey a =
     SortKeyVariable SortDirection Variable
   | SortKeyMacro SortDirection NameFormat Text

data SortKeyValue = SortKeyValue SortDirection (Maybe [Text])

-- Sorting is case-insensitive and collation-aware
normalizeSortKey :: Text -> [Text]
normalizeSortKey = filter (not . T.null) . T.split isWordSep . T.toCaseFold
```

**Key Feature**: Sort keys split on word boundaries and are normalized for case-insensitive collation using Unicode collation algorithm.

### 5. **Disambiguation Strategy**

```haskell
data DisambiguationStrategy =
  DisambiguationStrategy
  { disambiguateAddNames      :: Bool
  , disambiguateAddGivenNames :: Maybe GivenNameDisambiguationRule
  , disambiguateAddYearSuffix :: Bool
  }

data GivenNameDisambiguationRule =
    AllNames
  | AllNamesWithInitials
  | PrimaryName
  | PrimaryNameWithInitials
  | ByCite
```

**Flow**:
1. Detect ambiguities (different items rendering identically)
2. Try adding more authors incrementally
3. Try adding given names/initials
4. Apply year suffixes if still ambiguous
5. Mark citations with `WouldDisambiguate` condition

**Design Pattern**: Iterative refinement - each disambiguation level runs in isolation, and the pipeline re-renders to check for remaining ambiguities.

## Element System (Intermediate Representation)

The `Element` type represents CSL markup before evaluation:

```haskell
data Element a = Element (ElementType a) Formatting

data ElementType a =
    EText TextType                          -- Render text/variable/macro/term
  | EDate Variable DateType (Maybe ShowDateParts) [DP]  -- Render date
  | ENumber Variable NumberForm             -- Render number with formatting
  | ENames [Variable] NamesFormat [Element a]  -- Render multiple names
  | ELabel Variable TermForm Pluralize      -- Render label for variable
  | EGroup Bool [Element a]                 -- Conditional group
  | EChoose [(Match, [Condition], [Element a])]  -- If-then-else logic

data TextType =
    TextVariable VariableForm Variable      -- Variable reference
  | TextMacro Text                          -- Macro invocation
  | TextTerm Term                           -- Locale term
  | TextValue Text                          -- Literal text
```

**Conditional Logic**:
```haskell
data Condition =
    HasVariable Variable
  | HasType [Text]
  | IsUncertainDate Variable
  | IsNumeric Variable
  | HasLocatorType Variable
  | HasPosition Position                   -- position tests (first, ibid, etc.)
  | WouldDisambiguate                      -- Disambiguation condition

data Position = FirstPosition | IbidWithLocator | Ibid | NearNote | Subsequent
```

## Name Formatting

Names have sophisticated formatting rules:

```haskell
data NameFormat =
  NameFormat
  { nameGivenFormatting        :: Maybe Formatting
  , nameFamilyFormatting       :: Maybe Formatting
  , nameAndStyle               :: Maybe TermForm              -- "and" vs "&"
  , nameDelimiter              :: Maybe Text
  , nameDelimiterPrecedesEtAl  :: Maybe DelimiterPrecedes     -- Before "et al."
  , nameDelimiterPrecedesLast  :: Maybe DelimiterPrecedes     -- Before last name
  , nameEtAlMin                :: Maybe Int                   -- When to use "et al."
  , nameEtAlUseFirst           :: Maybe Int                   -- Show first N names
  , nameEtAlSubsequentUseFirst :: Maybe Int
  , nameEtAlSubsequentMin      :: Maybe Int
  , nameEtAlUseLast            :: Maybe Bool
  , nameForm                   :: Maybe NameForm              -- long vs short
  , nameInitialize             :: Maybe Bool
  , nameInitializeWith         :: Maybe Text
  , nameAsSortOrder            :: Maybe NameAsSortOrder       -- Sort order (all names)
  , nameSortSeparator          :: Maybe Text
  }

data NameAsSortOrder = NameAsSortOrderFirst | NameAsSortOrderAll
```

**Design Pattern**: All name formatting is collected and applied after rendering, with special handling for et al abbreviation cascades.

## Layout System

```haskell
data Layout a =
  Layout
  { layoutOptions        :: LayoutOptions
  , layoutFormatting     :: Formatting
  , layoutElements       :: [Element a]
  , layoutSortKeys       :: [SortKey a]
  }

data LayoutOptions =
  LayoutOptions
  { layoutCollapse               :: Maybe Collapsing
  , layoutYearSuffixDelimiter    :: Maybe Text
  , layoutAfterCollapseDelimiter :: Maybe Text
  , layoutNameFormat             :: NameFormat
  }

data Collapsing =
     CollapseCitationNumber
   | CollapseYear
   | CollapseYearSuffix
   | CollapseYearSuffixRanged
```

The citation and bibliography layouts are processed separately with their own templates.

## Style and Locale System

```haskell
data Style a =
  Style
  { styleCslVersion    :: (Int,Int,Int)
  , styleOptions       :: StyleOptions
  , styleCitation      :: Layout a
  , styleBibliography  :: Maybe (Layout a)
  , styleLocales       :: [Locale]
  , styleAbbreviations :: Maybe Abbreviations
  , styleMacros        :: M.Map Text [Element a]
  }

data Locale =
  Locale
  { localeLanguage               :: Maybe Lang
  , localePunctuationInQuote     :: Maybe Bool
  , localeLimitDayOrdinalsToDay1 :: Maybe Bool
  , localeDate                   :: M.Map DateType (Element Text)
  , localeTerms                  :: M.Map Text [(Term, Text)]
  }
```

**Locale Merging**: Complex fallback chain:
1. Exact language match
2. Primary dialect match (e.g., en-GB if en-US specified)
3. Two-letter language match
4. No-language locale
5. Built-in US English fallback

## Subsequent Author Substitution

```haskell
data SubsequentAuthorSubstitute =
  SubsequentAuthorSubstitute Text SubsequentAuthorSubstituteRule

data SubsequentAuthorSubstituteRule =
      CompleteAll         -- Replace all authors of matching entries
    | CompleteEach        -- Process each entry independently
    | PartialEach         -- Partial replacement per entry
    | PartialFirst        -- Only first matching entry
```

Uses tree-walking with `transform` to find and replace names in Output trees.

## CSL JSON Format Implementation

The CSL JSON format example shows how CiteprocOutput is implemented for a specific format:

```haskell
data CslJson a =
     CslText a
   | CslEmpty
   | CslConcat (CslJson a) (CslJson a)
   | CslQuoted (CslJson a)
   | CslItalic (CslJson a)
   | CslBold (CslJson a)
   | CslSup (CslJson a)
   | CslSub (CslJson a)
   | CslSmallCaps (CslJson a)
   | CslNoDecoration (CslJson a)
   | CslLink Text (CslJson a)
   | CslDiv Text (CslJson a)      -- display: block, left-margin, etc.
```

**Design Pattern**: Recursive algebraic data type with explicit markup. The `CiteprocOutput` instance transforms formatting operations into these constructors.

## Key Rendering Functions

### renderOutput

Converts `Output` to the final output format by:
1. Handling special tags (citations, names, etc.)
2. Applying formatting chains
3. Adding hyperlinks when requested
4. Managing punctuation and delimiters
5. Preserving case for names with unusual capitalization

### fixPunct

Post-processing function implementing punctuation rules (comma/period collision handling).

### movePunctuationInsideQuotes

Locale-aware adjustment of punctuation relative to quotation marks.

## Important Design Differences from Typical Rust Implementations

1. **Type Polymorphism over Variants**: Uses Haskell typeclasses instead of enums - the output format is a type parameter throughout, not a runtime variant

2. **Monadic Threading**: RWS monad provides implicit threading of context, state, and warnings - would be explicit parameter threading in Rust

3. **Immutable Data Structures**: Extensive use of persistent data structures (Maps) - updates via `M.adjust` on immutable maps rather than mutable references

4. **Higher-Order Formatting**: `addFormatting` chains operations functionally - would be a sequence of imperative calls in Rust

5. **Generic Tree Walking**: Uses `Data.Generics.Uniplate` for tree transformation - Rust would use explicit recursive functions or visitor pattern

6. **Lazy Evaluation**: Some computations defer until needed - Rust evaluation is eager

7. **Rich Type System**: Haskell's type system catches many errors at compile time that Rust would handle with runtime checks or Result types

8. **Parser Combinators**: Attoparsec for parsing - Rust would likely use pest, nom, or manual parsing

## Performance Characteristics

- **Sorting**: O(n log n) with collation-aware comparison
- **Disambiguation**: Iterative refinement, potentially multiple passes
- **Output Rendering**: Single pass through element tree
- **Memory**: Persistent data structures have copy-on-write semantics

The implementation emphasizes correctness and spec compliance over raw performance, making it suitable as a reference implementation rather than a high-performance system.
