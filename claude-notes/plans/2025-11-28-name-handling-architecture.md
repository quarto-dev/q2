# Name Handling Architecture Fixes for quarto-citeproc

**Date**: 2025-11-28
**Parent Issue**: k-422 (CSL Conformance)
**Related Issue**: (to be created)

## Overview

The name handling in quarto-citeproc has several architectural issues that prevent it from passing ~56 CSL conformance tests. This document describes the issues and the implementation plan.

## Current State

- 930 total CSL conformance tests
- 551 passing (59.2%)
- 379 failing (40.8%)
  - 72 deferred (also fail in Pandoc citeproc)
  - 307 non-deferred failures
- **56 failures in the `name_*` category alone**

## Architectural Issues

### 1. Missing `<name-part>` Element Support (Critical)

The CSL spec allows `<name-part name="family">` and `<name-part name="given">` elements inside `<name>` to apply separate formatting to family vs given names:

```xml
<name>
  <name-part name="family" text-case="uppercase"/>
  <name-part name="given" font-style="italic"/>
</name>
```

**Haskell implementation** (in `Citeproc/Types.hs`):
```haskell
data NameFormat = NameFormat
  { nameGivenFormatting  :: Maybe Formatting
  , nameFamilyFormatting :: Maybe Formatting
  , ...
  }
```

**Our implementation**: Missing entirely. The `InheritableNameOptions` struct and CSL parser have no support for name-part formatting.

**Impact**: ~19 tests fail due to missing name case handling (UPPERCASE family names, etc.)

### 2. Name Formatting Returns `String`, Not `Output` (Critical Blocker)

**This is the root cause that blocks all other fixes.**

Our implementation:
```rust
fn format_names(ctx: &EvalContext, names: &[Name], names_el: &NamesElement) -> String
fn format_single_name(name: &Name, options: &InheritableNameOptions, ...) -> String
```

Haskell implementation:
```haskell
formatName :: CiteprocOutput a => NameFormat -> Formatting -> Int -> Name -> Eval a (Output a)
```

The Haskell implementation returns a structured `Output` AST where different name parts (family, given, particles, suffix) are wrapped in their own formatting nodes. Our implementation concatenates everything into a plain string, making it **impossible** to apply different formatting to different parts.

**Impact**: Cannot implement name-part formatting, text-case on name parts, or per-part decorations.

### 3. Missing `comma-suffix` and `static-ordering` Fields

The Haskell `Name` type has:
```haskell
data Name = Name
  { nameFamily              :: Maybe Text
  , nameGiven               :: Maybe Text
  , nameDroppingParticle    :: Maybe Text
  , nameNonDroppingParticle :: Maybe Text
  , nameSuffix              :: Maybe Text
  , nameCommaSuffix         :: Bool      -- MISSING
  , nameStaticOrdering      :: Bool      -- MISSING
  , nameLiteral             :: Maybe Text
  }
```

Our `Name` struct (in `reference.rs`):
```rust
pub struct Name {
    pub family: Option<String>,
    pub given: Option<String>,
    pub dropping_particle: Option<String>,
    pub non_dropping_particle: Option<String>,
    pub suffix: Option<String>,
    pub literal: Option<String>,
    pub parse_names: Option<bool>,
    // Missing: comma_suffix, static_ordering
}
```

- `comma_suffix`: Determines if suffix needs comma before it (e.g., "Smith, Jr." vs "Smith Jr.")
- `static_ordering`: For names that don't follow normal given/family ordering rules

**Impact**: Incorrect suffix formatting, incorrect handling of some non-Western names.

### 4. Missing "Byzantine Name" Detection

The Haskell implementation has `isByzantineName` (Types.hs:1305) which detects Western/Latin names vs non-Western names (CJK, Arabic, Hebrew, etc.):

```haskell
isByzantineName :: Name -> Bool
isByzantineName name = maybe False isByzantine (nameFamily name)

-- Checks if name contains only "Romanesque" characters
isByzantineChar :: Char -> Bool
isByzantineChar c = c == '-' ||
    (c >= '0' && c <= '9') ||
    (c >= 'a' && c <= 'z') ||
    (c >= 'A' && c <= 'Z') ||
    -- Latin Extended, Greek, Cyrillic, Hebrew, Arabic, Thai...
```

Names are formatted differently:
- **Western (Byzantine)**: "Given Family" or "Family, Given" (with comma in sort order)
- **Non-Western**: "Family Given" (no comma separator in sort order)

Our implementation treats all names identically.

**Impact**: Incorrect formatting of CJK and other non-Western names.

### 5. Missing `demote-non-dropping-particle` Style Option

The CSL spec has a style-level option `demote-non-dropping-particle` with values:
- `never` - particle stays with family name ("van Gogh" sorts under V)
- `sort-only` - particle demoted only in sort keys (display: "van Gogh", sort: "Gogh, van")
- `display-and-sort` - particle always demoted

The Haskell implementation handles this with ~6 different code paths in `getDisplayName` (Eval.hs:2534-2604):

```haskell
if isByzantineName name
   then
     case fromMaybe LongName (nameForm nameFormat) of
          LongName
            | demoteNonDroppingParticle == DemoteNever
            , inSortKey || nameAsSort -> ...
            | demoteNonDroppingParticle == DemoteSortOnly
            , inSortKey -> ...
            -- etc.
```

Our implementation doesn't consider this option at all.

**Impact**: Incorrect particle handling in names like "Ludwig van Beethoven", "Vincent van Gogh".

## Implementation Plan

### Phase 1: Refactor Name Formatting to Return `Output` (Prerequisite)

**Goal**: Change the name formatting pipeline to return structured `Output` AST instead of `String`.

**Files to modify**:
- `crates/quarto-citeproc/src/eval.rs`
  - `format_names()` → returns `Output`
  - `format_single_name()` → returns `Output`
  - Update `evaluate_names()` to use structured output

**Key changes**:
```rust
// Before
fn format_single_name(...) -> String

// After
fn format_single_name(...) -> Output
```

Each name part (family, given, particle, suffix) becomes a separate `Output` node that can have its own formatting applied.

### Phase 2: Add Name-Part Formatting to CSL Parser

**Goal**: Parse `<name-part>` elements and store their formatting.

**Files to modify**:
- `crates/quarto-csl/src/types.rs`
  - Add `NamePartFormatting` struct
  - Add `family_formatting` and `given_formatting` to `Name` element
- `crates/quarto-csl/src/parser.rs`
  - Parse `<name-part>` children of `<name>` element

### Phase 3: Add Missing Name Fields

**Goal**: Add `comma_suffix` and `static_ordering` to Name struct.

**Files to modify**:
- `crates/quarto-citeproc/src/reference.rs`
  - Add fields to `Name` struct
  - Update serde attributes for JSON parsing

### Phase 4: Add Byzantine Name Detection

**Goal**: Detect Western vs non-Western names and format accordingly.

**Files to modify**:
- `crates/quarto-citeproc/src/reference.rs` or new `name_utils.rs`
  - Add `is_byzantine_name()` function
- `crates/quarto-citeproc/src/eval.rs`
  - Update name formatting to use different logic for non-Byzantine names

### Phase 5: Add `demote-non-dropping-particle` Support

**Goal**: Implement the style option for particle handling.

**Files to modify**:
- `crates/quarto-csl/src/types.rs`
  - Add `DemoteNonDroppingParticle` enum
  - Add field to `StyleOptions`
- `crates/quarto-csl/src/parser.rs`
  - Parse the attribute from `<style>` element
- `crates/quarto-citeproc/src/eval.rs`
  - Implement particle positioning logic based on option value

## Test Strategy

For each phase:
1. Identify specific failing tests that should pass after the fix
2. Implement the fix
3. Run the targeted tests to verify
4. Run full test suite to check for regressions

Key test files to watch:
- `name_AfterInvertedName.txt` - tests name-part text-case
- `name_ParticleFormatting.txt` - tests particle handling
- `name_AsianGlyphs.txt` - tests non-Western name handling
- `name_LowercaseSurnameSuffix.txt` - tests suffix handling

## References

- CSL Spec: https://docs.citationstyles.org/en/stable/specification.html#name
- Haskell citeproc: `external-sources/citeproc/src/Citeproc/Eval.hs` (lines 2168-2604)
- Haskell types: `external-sources/citeproc/src/Citeproc/Types.hs` (lines 466-540, 1158-1330)
