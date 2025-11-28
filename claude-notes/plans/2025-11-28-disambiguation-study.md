# CSL Disambiguation Study

**Date**: 2025-11-28

## Executive Summary

Disambiguation in CSL resolves ambiguous citations (citations that render identically but refer to different works). Currently 13/72 disambiguation tests pass (18%). This document analyzes the Haskell reference implementation and proposes an implementation plan for quarto-citeproc.

## Current State

### Tests Passing (13)
- Tests that pass by coincidence (no actual disambiguation needed)
- `disambiguate_DisambiguateTrueAndYearSuffixOne` - works because dates differ
- `disambiguate_AddNamesFailure`, `disambiguate_AddNamesFailureWithAddGivenname` - tests that disambiguation fails correctly
- Several tests that don't need active disambiguation

### Tests Failing (59)
- Year suffix assignment (`YearSuffixAndSort`, `YearSuffixMacroSameYearExplicit`, etc.)
- Name expansion (`AddNamesSuccess`, `AllNamesGenerally`, etc.)
- Given name expansion (`ByCiteGivennameExpandCrossNestedNames`, etc.)
- `disambiguate="true"` condition (`DisambiguateTrueReflectedInBibliography`)

## CSL Disambiguation Strategy

CSL 1.0.2 defines three disambiguation methods (in order of application):

### 1. `disambiguate-add-names="true"`
Add more names from et-al truncated lists until unique.

**Example:**
- "Smith et al. (1980)" and "Smith et al. (1980)" with different co-authors
- Becomes: "Smith, Brown, et al. (1980)" and "Smith, Beefheart, et al. (1980)"

### 2. `disambiguate-add-givenname="true"` with `givenname-disambiguation-rule`
Add given names (initials or full) to distinguish:

**Rules:**
- `all-names`: Expand all ambiguous names everywhere
- `all-names-with-initials`: Like above but use initials only
- `primary-name`: Only expand the first author
- `primary-name-with-initials`: Like above but use initials
- `by-cite`: Expand names per-citation (minimal expansion)

### 3. `disambiguate-add-year-suffix="true"`
Add letter suffixes (a, b, c...) to years.

**Example:**
- "Smith (2020)" and "Smith (2020)"
- Becomes: "Smith (2020a)" and "Smith (2020b)"

**Key rule:** Suffixes are assigned based on **bibliography sort order**, not citation order.

## Haskell Reference Implementation Analysis

### Key Data Structures

```haskell
-- Per-reference disambiguation state
data DisambiguationData = DisambiguationData
  { disambYearSuffix  :: Maybe Int        -- Assigned year suffix (1=a, 2=b, etc.)
  , disambNameMap     :: Map Name NameHints  -- Per-name expansion hints
  , disambEtAlNames   :: Maybe Int        -- Override et-al-use-first
  , disambCondition   :: Bool             -- Should disambiguate="true" match?
  }

data NameHints =
    AddInitials
  | AddGivenName
  | AddInitialsIfPrimary
  | AddGivenNameIfPrimary
```

### Algorithm Overview (`disambiguateCitations`)

1. **Collect all citation renderings** (including ghost items for all refs)
2. **Find ambiguities** - group by rendered text, keep groups with >1 unique item
3. **Apply disambiguation in order:**
   a. If `disambiguate-add-names`: try adding names to et-al lists
   b. If `disambiguate-add-givenname` with rule: expand given names
   c. If `disambiguate-add-year-suffix`: assign year suffixes
   d. Finally: set `disambCondition=true` for remaining ambiguities
4. **Re-render** with disambiguation state applied

### Key Functions

```haskell
getAmbiguities :: [Output a] -> [[DisambData]]
-- Groups citations by rendered text, returns groups with >1 member

tryAddNames :: [DisambData] -> Eval a ()
-- Incrementally adds names until disambiguation or exhausted

addYearSuffixes :: Map ItemId [SortKeyValue] -> [[DisambData]] -> Eval a ()
-- Assigns suffixes based on bibliography sort order

tryDisambiguateCondition :: [DisambData] -> Eval a ()
-- Sets disambCondition=true for remaining ambiguous items
```

### Year Suffix Assignment Logic

Critical insight from the spec and Haskell implementation:
- Year suffixes follow **bibliography order**, not citation order
- Multiple ambiguous groups are handled independently
- Uses a Map from ItemId to SortKeyValue for ordering

```haskell
addYearSuffixes bibSortKeyMap' as = do
  let companions a = sortBy (collate bibSortKeyMap') (concat [x | x <- as, a `elem` x])
  let groups = Set.map companions $ Set.fromList (concat as)
  mapM_ (\xs -> zipWithM addYearSuffix (map ddItem xs) [1..]) groups
```

## Current quarto-citeproc Gaps

### 1. No Disambiguation Strategy Parsing
The parser doesn't parse disambiguation attributes from `<citation>`:
- `disambiguate-add-names`
- `disambiguate-add-givenname`
- `disambiguate-add-year-suffix`
- `givenname-disambiguation-rule`

### 2. No DisambiguationData Structure
References don't carry disambiguation state:
- No year suffix storage
- No name hint maps
- No et-al override
- No disambCondition flag

### 3. No Disambiguation Pass
Citation evaluation doesn't:
- Detect ambiguities
- Apply disambiguation methods
- Re-render with state

### 4. Condition ConditionType::Disambiguate Not Implemented
Line 1000 in eval.rs returns `false` for all disambiguate conditions:
```rust
ConditionType::Disambiguate(_) => false, // TODO: Implement
```

### 5. year-suffix Variable Not Handled
Text element doesn't render `year-suffix` variable.

## Proposed Implementation Plan

### Phase 1: Infrastructure (~2 sessions)

**1.1 Parse Disambiguation Options**

Add to `quarto_csl::Layout`:
```rust
pub struct DisambiguationStrategy {
    pub add_names: bool,
    pub add_givenname: Option<GivenNameRule>,
    pub add_year_suffix: bool,
}

pub enum GivenNameRule {
    AllNames,
    AllNamesWithInitials,
    PrimaryName,
    PrimaryNameWithInitials,
    ByCite,
}
```

Parse from citation element:
- `disambiguate-add-names="true"`
- `disambiguate-add-givenname="true"`
- `givenname-disambiguation-rule="..."`
- `disambiguate-add-year-suffix="true"`

**1.2 Add DisambiguationData to Reference**

```rust
pub struct DisambiguationData {
    pub year_suffix: Option<i32>,
    pub name_hints: HashMap<Name, NameHint>,
    pub et_al_names: Option<u32>,
    pub disamb_condition: bool,
}

pub enum NameHint {
    AddInitials,
    AddGivenName,
    AddInitialsIfPrimary,
    AddGivenNameIfPrimary,
}
```

Add to `Reference`:
```rust
pub disambiguation: Option<DisambiguationData>
```

**1.3 Implement year-suffix Variable**

In `evaluate_text`:
```rust
"year-suffix" => {
    if let Some(ref disamb) = ctx.reference.disambiguation {
        if let Some(suffix) = disamb.year_suffix {
            let letter = (b'a' + (suffix - 1) as u8) as char;
            Output::tagged(Tag::YearSuffix(suffix), Output::literal(letter.to_string()))
        } else {
            Output::Null
        }
    } else {
        Output::Null
    }
}
```

### Phase 2: Ambiguity Detection (~1 session)

**2.1 Add DisambData Structure**

```rust
struct DisambData {
    item_id: String,
    names: Vec<Name>,
    rendered: String,
}
```

**2.2 Implement getAmbiguities Equivalent**

```rust
fn find_ambiguities(outputs: &[(String, Output)]) -> Vec<Vec<DisambData>> {
    // Extract tagged items from outputs
    // Group by rendered text
    // Return groups with >1 unique item
}
```

### Phase 3: Year Suffix Assignment (~1-2 sessions)

**3.1 Implement addYearSuffixes**

- Get bibliography sort order
- Group ambiguous items by "companions" (same rendered text)
- Sort each group by bibliography order
- Assign suffixes 1, 2, 3... (rendered as a, b, c...)

**3.2 Store in Reference DisambiguationData**

### Phase 4: Name Disambiguation (~2-3 sessions)

**4.1 Implement tryAddNames**

- For each ambiguous group:
  - Increment names shown (et_al_use_first override)
  - Re-render and check if disambiguated
  - Continue until max names or disambiguated

**4.2 Implement tryAddGivenNames (ByCite)**

- For each position in author list:
  - Find names that need disambiguation
  - Compute appropriate hint (AddInitials vs AddGivenName)
  - Store in disambNameMap

**4.3 Apply Name Hints in Rendering**

Modify `format_names` to check disambiguation hints and expand accordingly.

### Phase 5: Disambiguation Condition (~1 session)

**5.1 Implement tryDisambiguateCondition**

Set `disamb_condition = true` for items still ambiguous after all methods.

**5.2 Implement Condition Check**

```rust
ConditionType::Disambiguate(val) => {
    ctx.reference.disambiguation
        .as_ref()
        .map(|d| d.disamb_condition == *val)
        .unwrap_or(false)
}
```

### Phase 6: Integration (~1-2 sessions)

**6.1 Two-Pass Rendering**

Modify `evaluate_citation_to_output`:
1. First pass: render all citations naively
2. Run disambiguation algorithm
3. Second pass: re-render with disambiguation state

**6.2 Bibliography Coordination**

Ensure year suffixes use bibliography sort order, not citation order.

## Test-Driven Approach

For each phase:
1. Identify specific failing tests
2. Enable them in `enabled_tests.txt`
3. Implement the feature
4. Verify tests pass
5. Look for regressions

### Suggested Test Order

**Year Suffix Tests (Phase 3):**
- `disambiguate_YearSuffixMacroSameYearExplicit` - basic year suffix
- `disambiguate_YearSuffixMacroSameYearImplicit` - implicit year suffix in date
- `disambiguate_YearSuffixAndSort` - suffix follows bib order
- `disambiguate_YearSuffixFiftyTwoEntries` - beyond 'z' (aa, ab, etc.)

**Name Addition Tests (Phase 4):**
- `disambiguate_AddNamesSuccess` - basic name addition
- `disambiguate_AllNamesGenerally` - expand all names
- `disambiguate_ByCiteGivennameExpandCrossNestedNames` - ByCite rule

**Disambiguation Condition Tests (Phase 5):**
- `disambiguate_DisambiguateTrueReflectedInBibliography`

## Estimated Effort

- Phase 1 (Infrastructure): 2 sessions
- Phase 2 (Detection): 1 session
- Phase 3 (Year Suffix): 1-2 sessions
- Phase 4 (Names): 2-3 sessions
- Phase 5 (Condition): 1 session
- Phase 6 (Integration): 1-2 sessions

**Total: 8-11 sessions**

Expected test improvement: 13 → 60+ passing (83%+)

## Risks and Mitigations

1. **Complexity of ByCite rule** - Start with simpler rules (AllNames, PrimaryName)
2. **Performance** - Re-rendering all citations is expensive; may need caching
3. **Edge cases** - Many subtle interactions; rely heavily on test suite
4. **Two-pass architecture** - May require refactoring eval to support state updates

## Recommendation

Start with **Phase 1 + Phase 3 (Year Suffix)** as the first milestone:
- Year suffix is the most commonly used disambiguation method
- It's relatively self-contained
- Would unlock ~15-20 tests immediately
- Provides foundation for subsequent phases

This would be a meaningful first step that demonstrates the architecture while providing immediate value.
