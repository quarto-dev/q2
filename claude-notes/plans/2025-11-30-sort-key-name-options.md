# Sort Key Name Options Implementation

## Problem Summary

The CSL `<key>` element in `<sort>` can have name-related attributes that override the default name formatting when evaluating macros for sorting. These attributes are currently not being parsed or applied.

## Failing Test

**Test**: `sort_NumberOfAuthorsAsKey`
**Status**: Unknown (not enabled, not deferred)

### Test Details

The test uses these sort keys:
```xml
<sort>
  <key macro="author-one" names-min="1" names-use-first="1" />
  <key macro="author-count" names-min="3" names-use-first="3" />
  <key macro="theyear" />
</sort>
```

The `names-min="1" names-use-first="1"` on the first key should limit the author name output to just the first author. Without this, the full author list is used, causing incorrect sort order.

### Debug Output (from investigation)

Current sort key values show the problem:
```
item-1 keys: ["Doe John", "1", "2000"]
item-2 keys: ["Doe John, Doe Jake, Jones Robert", "3", "2000"]
item-3 keys: ["Doe John, Roe Jane", "2", "2000"]
```

With `names-use-first="1"`, all three should have just "Doe John" for the first key, making them equal and allowing the second key (count) to determine order.

## CSL Specification

From the CSL 1.0.2 spec, the `<key>` element can have these name-related attributes:

| Attribute | Type | Description |
|-----------|------|-------------|
| `names-min` | integer | Overrides `et-al-min` when evaluating the key |
| `names-use-first` | integer | Overrides `et-al-use-first` when evaluating the key |
| `names-use-last` | boolean | Overrides `et-al-use-last` when evaluating the key |

These allow styles to sort by abbreviated author lists while displaying full lists.

## Implementation Plan

### 1. Update `SortKey` struct in `quarto-csl/src/types.rs`

```rust
pub struct SortKey {
    /// Variable or macro to sort by.
    pub key: SortKeyType,
    /// Sort order.
    pub sort_order: SortOrder,
    /// Override for et-al-min when evaluating this key.
    pub names_min: Option<u32>,
    /// Override for et-al-use-first when evaluating this key.
    pub names_use_first: Option<u32>,
    /// Override for et-al-use-last when evaluating this key.
    pub names_use_last: Option<bool>,
    /// Source location.
    pub source_info: SourceInfo,
}
```

### 2. Update parser in `quarto-csl/src/parser.rs`

In the `parse_sort_key` function (or wherever `<key>` elements are parsed), add:

```rust
let names_min = get_optional_attr::<u32>(node, "names-min")?;
let names_use_first = get_optional_attr::<u32>(node, "names-use-first")?;
let names_use_last = get_optional_attr::<bool>(node, "names-use-last")?;
```

### 3. Update `evaluate_macro_for_sort` in `quarto-citeproc/src/eval.rs`

Currently at line 1148, this function doesn't receive the sort key options. It needs to:

1. Accept the `SortKey` (not just macro name) to access the override options
2. Build an `InheritableNameOptions` from the sort key overrides
3. Pass these as the inherited options when creating the `EvalContext`

```rust
pub fn evaluate_macro_for_sort(
    processor: &Processor,
    reference: &Reference,
    elements: &[Element],
    sort_key: &quarto_csl::SortKey,  // Add this parameter
) -> Result<String> {
    // ... existing setup ...

    // Build name options from sort key overrides
    let sort_key_name_options = InheritableNameOptions {
        et_al_min: sort_key.names_min,
        et_al_use_first: sort_key.names_use_first,
        et_al_use_last: sort_key.names_use_last,
        ..Default::default()
    };

    // Merge with layout options (sort key takes precedence)
    let name_options = sort_key_name_options.merge(&layout_name_options.merge(&style_name_options));

    // ... rest of function ...
}
```

### 4. Update `compute_sort_keys` in `quarto-citeproc/src/types.rs`

Pass the full `SortKey` to `evaluate_macro_for_sort`:

```rust
quarto_csl::SortKeyType::Macro(name) => {
    self.get_sort_value_for_macro(reference, name, key)  // Pass key
}
```

And update `get_sort_value_for_macro`:

```rust
fn get_sort_value_for_macro(
    &self,
    reference: &Reference,
    macro_name: &str,
    sort_key: &quarto_csl::SortKey,
) -> String {
    if let Some(macro_def) = self.style.macros.get(macro_name) {
        crate::eval::evaluate_macro_for_sort(self, reference, &macro_def.elements, sort_key)
            .unwrap_or_default()
    } else {
        String::new()
    }
}
```

## Related Tests

These tests in the `sort` category likely also need this feature:

- `sort_NumberOfAuthorsAsKey` - Primary test for this feature
- `sort_NamesUseLast` - Tests `names-use-last` attribute
- `sort_DropNameLabelInSort` - May be related

## Haskell Reference

In Pandoc's citeproc (`external-sources/citeproc/src/Citeproc/Eval.hs`), the sort key evaluation handles this around line 1018:

```haskell
evalSortKey citeId (SortKeyMacro sortdir nameformat macroname) = do
  -- nameformat contains the names-min, names-use-first overrides
  -- ...
  k <- ... withRWS newContext (eElements elts)
   where
    newContext oldContext s =
      (oldContext{ contextNameFormat = combineNameFormat
                     nameformat (contextNameFormat oldContext)},
       s{ stateReference = ref })
```

The key insight is that `nameformat` (containing the overrides) is combined with the context's name format before evaluating.

## Testing

After implementation:
1. Run `python3 scripts/csl-test-helper.py inspect sort_NumberOfAuthorsAsKey --diff`
2. Expected: First key values should all be "Doe John" (or similar single-author format)
3. Run full test suite to check for regressions

## Implementation Status: COMPLETE (2025-11-30)

### Changes Made

1. **quarto-csl/src/types.rs:419-437** - Added `names_min`, `names_use_first`, `names_use_last` fields to `SortKey` struct

2. **quarto-csl/src/parser.rs:902-921** - Added parsing of `names-min`, `names-use-first`, `names-use-last` attributes in `parse_sort_key`

3. **quarto-citeproc/src/eval.rs:1151-1190** - Updated `evaluate_macro_for_sort` to accept `SortKey` and apply its name options with highest priority in the merge chain

4. **quarto-citeproc/src/types.rs:763-779** - Updated `get_sort_value_for_macro` to pass `SortKey` to the evaluation function

### Tests Enabled

- `sort_NumberOfAuthorsAsKey` - Primary test for `names-min`, `names-use-first`
- `sort_NamesUseLast` - Tests `names-use-last` attribute

### Remaining Sort Tests (Unknown)

- `sort_DropNameLabelInSort` - Requires subsequent-author-substitute feature
- Other sort tests may have different root causes
