# Subsequent Author Substitute Implementation Plan

**Issue**: k-461
**Date**: 2025-12-01
**Status**: Design Complete - Ready for Implementation

## Overview

The `subsequent-author-substitute` feature replaces repeated author names in consecutive bibliography entries with a substitute string (typically "———" or "---"). This is a common requirement in styles like Chicago Manual of Style.

## CSL Specification Summary

### Attributes (on `<bibliography>` element)

| Attribute | Type | Description |
|-----------|------|-------------|
| `subsequent-author-substitute` | string | The substitute text (e.g., "———") |
| `subsequent-author-substitute-rule` | enum | How to apply substitution (default: "complete-all") |

### Substitute Rules

1. **complete-all** (default): When ALL names match the preceding entry, replace the **entire name list** (but keep cs:names affixes)

2. **complete-each**: When ALL names match, replace **each name individually**
   - "Doe, Smith" → "---, ---"

3. **partial-each**: When one or more names match (from start), replace **each matching name**
   - "Doe, Smith" following "Doe, Jones" → "---, Smith"

4. **partial-first**: When first name matches, replace **only the first name**
   - "Doe, Smith" following "Doe, Jones" → "---, Smith"

### Important Constraints

- Substitution is limited to the **first `cs:names` element rendered** in each entry
- Matching is based on semantic name equality, not rendered text
- Affixes on `cs:names` are preserved (prefix/suffix still rendered)

## Failing Tests

### Unknown Tests (need to enable)
1. `name_SubsequentAuthorSubstituteSingleField` - Basic single-author substitute
2. `name_SubsequentAuthorSubstituteMultipleNames` - Multiple-author substitute
3. `name_SubstitutePartialEach` - partial-each rule

### Related Deferred Tests
- `magic_SubsequentAuthorSubstituteNotFooled` - Ensures different authors don't get substituted
- `sort_DropNameLabelInSort` - Uses subsequent-author-substitute with sorting

## Current Implementation Analysis

### What We Have

1. **Output AST with tagging** (`output.rs:76-92`):
   ```rust
   pub enum Output {
       Tagged { tag: Tag, child: Box<Output> },
       // ...
   }

   pub enum Tag {
       Names { variable: String, names: Vec<Name> },
       Name(Name),
       // ...
   }
   ```

2. **Name tagging during evaluation** (`eval.rs:1492-1498`):
   ```rust
   let names_output = Output::tagged(
       Tag::Names {
           variable: var.clone(),
           names: names.to_vec(),
       },
       formatted,
   );
   ```

3. **Bibliography generation** (`types.rs:555-673`):
   - `generate_bibliography()` returns rendered strings
   - `generate_bibliography_to_outputs()` returns `Output` AST

4. **Name extraction helpers** (`output.rs:550-575`):
   - `extract_all_names()` - gets names from ALL `Tag::Names` elements
   - `extract_names_text()` - gets rendered text of first names element

### What We Need to Add

1. **CSL Parsing**: Add attributes to `Layout` struct and parser
2. **First-names extraction**: Method to get names from first `Tag::Names` only
3. **Output transformation**: Methods to replace names in the Output tree
4. **Post-processing step**: Apply substitution after bibliography entries are generated

## Haskell Reference Implementation

The Haskell implementation (`external-sources/citeproc/src/Citeproc/Eval.hs:323-405`) follows this approach:

1. **Tag names during evaluation**: Names output is wrapped in `TagNames Variable NamesFormat [Name]`

2. **Post-process after all entries rendered**:
   ```haskell
   subsequentAuthorSubstitutes :: SubsequentAuthorSubstitute -> [Output a] -> [Output a]
   subsequentAuthorSubstitutes (SubsequentAuthorSubstitute t rule) = groupCitesByNames
   ```

3. **Extract first TagNames from each entry**:
   ```haskell
   getNames (Formatted _ (x:_)) =
     case [(ns,r) | (Tagged (TagNames _ _ ns) r) <- universe x] of
       ((ns,r) : _) -> Just (ns,r)
       []           -> Nothing
   ```

4. **Compare names and apply replacement based on rule**:
   - `CompleteAll`: Replace entire names output if all names match
   - `CompleteEach`: Transform each name individually
   - `PartialEach`/`PartialFirst`: Replace matching names from start

5. **Key insight**: The `[Name]` list stored in the tag is used for semantic comparison, not the rendered text.

## Implementation Plan

### Phase 1: CSL Parsing (~1 hour)

**File: `crates/quarto-csl/src/types.rs`**

Add to `Layout` struct:
```rust
pub struct Layout {
    // ... existing fields ...

    /// Substitute string for repeated authors (bibliography only).
    pub subsequent_author_substitute: Option<String>,

    /// Rule for how to apply subsequent-author-substitute.
    pub subsequent_author_substitute_rule: SubsequentAuthorSubstituteRule,
}

/// Rule for subsequent-author-substitute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubsequentAuthorSubstituteRule {
    /// Replace entire name list when all names match (default).
    #[default]
    CompleteAll,
    /// Replace each name when all names match.
    CompleteEach,
    /// Replace each matching name (from start).
    PartialEach,
    /// Replace only first name if it matches.
    PartialFirst,
}
```

**File: `crates/quarto-csl/src/parser.rs`**

In bibliography parsing:
```rust
let subsequent_author_substitute = get_optional_attr::<String>(node, "subsequent-author-substitute")?;
let subsequent_author_substitute_rule = get_optional_attr::<String>(node, "subsequent-author-substitute-rule")?
    .map(|s| match s.as_str() {
        "complete-all" => SubsequentAuthorSubstituteRule::CompleteAll,
        "complete-each" => SubsequentAuthorSubstituteRule::CompleteEach,
        "partial-each" => SubsequentAuthorSubstituteRule::PartialEach,
        "partial-first" => SubsequentAuthorSubstituteRule::PartialFirst,
        _ => SubsequentAuthorSubstituteRule::CompleteAll,
    })
    .unwrap_or_default();
```

### Phase 2: Output Helper Methods (~1.5 hours)

**File: `crates/quarto-citeproc/src/output.rs`**

Add methods to `Output`:

```rust
impl Output {
    /// Extract (names, raw_output) from the first Tag::Names in the tree.
    /// Returns None if no Tag::Names is found.
    pub fn extract_first_names(&self) -> Option<(Vec<Name>, Output)> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(result) = child.extract_first_names() {
                        return Some(result);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_first_names(),
            Output::Tagged { tag, child } => match tag {
                Tag::Names { names, .. } => Some((names.clone(), (**child).clone())),
                _ => child.extract_first_names(),
            },
        }
    }

    /// Replace names in the first Tag::Names element based on rule.
    pub fn replace_first_names(
        &self,
        substitute: &str,
        rule: SubsequentAuthorSubstituteRule,
        prev_names: &[Name],
    ) -> Option<Output> {
        // Implementation depends on rule:
        // - CompleteAll: Replace child of Tag::Names with Literal(substitute)
        // - CompleteEach: Transform each Tag::Name to Literal(substitute)
        // - PartialEach/PartialFirst: Replace matching names only
    }
}
```

### Phase 3: Post-Processing Function (~2 hours)

**File: `crates/quarto-citeproc/src/output.rs` or new file**

```rust
/// Apply subsequent-author-substitute to a list of bibliography entries.
pub fn apply_subsequent_author_substitute(
    entries: Vec<(String, Output)>,
    substitute: &str,
    rule: SubsequentAuthorSubstituteRule,
) -> Vec<(String, Output)> {
    if entries.is_empty() {
        return entries;
    }

    let mut result = Vec::with_capacity(entries.len());
    let mut prev_names: Option<Vec<Name>> = None;

    for (id, output) in entries {
        let current_names = output.extract_first_names();

        let new_output = match (&prev_names, &current_names) {
            (Some(prev), Some((curr, _))) if should_substitute(prev, curr, rule) => {
                output.replace_first_names(substitute, rule, prev)
                    .unwrap_or(output)
            }
            _ => output,
        };

        // Update prev_names for next iteration
        prev_names = current_names.map(|(names, _)| names);

        result.push((id, new_output));
    }

    result
}

fn should_substitute(
    prev: &[Name],
    curr: &[Name],
    rule: SubsequentAuthorSubstituteRule,
) -> bool {
    match rule {
        SubsequentAuthorSubstituteRule::CompleteAll |
        SubsequentAuthorSubstituteRule::CompleteEach => prev == curr,
        SubsequentAuthorSubstituteRule::PartialEach |
        SubsequentAuthorSubstituteRule::PartialFirst => {
            !prev.is_empty() && !curr.is_empty() && prev[0] == curr[0]
        }
    }
}
```

### Phase 4: Integration (~1 hour)

**File: `crates/quarto-citeproc/src/types.rs`**

Modify `generate_bibliography_to_outputs`:

```rust
pub fn generate_bibliography_to_outputs(&mut self) -> Result<Vec<(String, Output)>> {
    let bib = match &self.style.bibliography {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    // ... existing sorting and entry generation code ...

    // Format entries in order
    let mut entries = Vec::new();
    for id in &final_ids {
        if let Some(output) = self.format_bibliography_entry_to_output(id)? {
            entries.push((id.clone(), output));
        }
    }

    // Apply subsequent-author-substitute if configured
    let entries = if let Some(ref substitute) = bib.subsequent_author_substitute {
        apply_subsequent_author_substitute(
            entries,
            substitute,
            bib.subsequent_author_substitute_rule,
        )
    } else {
        entries
    };

    Ok(entries)
}
```

Also update `generate_bibliography` to use `generate_bibliography_to_outputs` and render:

```rust
pub fn generate_bibliography(&mut self) -> Result<Vec<(String, String)>> {
    let outputs = self.generate_bibliography_to_outputs()?;
    Ok(outputs.into_iter()
        .map(|(id, output)| (id, output.render_csl_html()))
        .collect())
}
```

### Phase 5: Testing (~1 hour)

1. Enable the three unknown tests:
   - `name_SubsequentAuthorSubstituteSingleField`
   - `name_SubsequentAuthorSubstituteMultipleNames`
   - `name_SubstitutePartialEach`

2. Run full test suite to check for regressions

3. Consider enabling deferred tests if they now pass

## Edge Cases to Handle

1. **Empty names list**: If no names are found in an entry, skip substitution
2. **First entry**: Never substitute the first entry (no previous to compare)
3. **Substitute element**: When `<substitute>` provides a non-name value (like title), the comparison should still work because we tag the substituted value as names
4. **Different authors**: Ensure different authors don't get incorrectly substituted (test: `magic_SubsequentAuthorSubstituteNotFooled`)
5. **Affixes**: Keep prefix/suffix on `cs:names` element when replacing

## Name Equality

For comparing names, we need semantic equality. Our `Name` struct should derive `PartialEq` (which it likely already does). Key fields to compare:
- `family`
- `given`
- `dropping_particle`
- `non_dropping_particle`
- `suffix`
- `literal` (for literal names like "Alan Alto Inc.")

## Estimated Time

| Phase | Time |
|-------|------|
| CSL Parsing | 1 hour |
| Output Helpers | 1.5 hours |
| Post-Processing | 2 hours |
| Integration | 1 hour |
| Testing | 1 hour |
| **Total** | **6.5 hours** |

## Files to Modify

1. `crates/quarto-csl/src/types.rs` - Add structs/enums
2. `crates/quarto-csl/src/parser.rs` - Parse attributes
3. `crates/quarto-citeproc/src/output.rs` - Helper methods
4. `crates/quarto-citeproc/src/types.rs` - Integration
5. `crates/quarto-citeproc/tests/enabled_tests.txt` - Enable tests

## Success Criteria

1. All three unknown tests pass
2. No regressions in existing tests
3. `magic_SubsequentAuthorSubstituteNotFooled` can be evaluated (may need additional work)

## Future Considerations

- The `sort_DropNameLabelInSort` test also uses this feature but may require additional fixes related to sorting
- Some deferred tests mention "subsequent citation formatting" which is different from this feature (position-based, not bibliography-based)
