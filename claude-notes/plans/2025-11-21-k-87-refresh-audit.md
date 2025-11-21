# k-87 SourceInfo::default() Audit - 2025-11-21 Refresh

## Current State
Total instances: **109** (up from original 43)

## Breakdown by Category

### Test Files (55 instances) - LEGITIMATE
- `test_attr_source_structure.rs`: 43
- `test_json_roundtrip.rs`: 5
- `quarto-yaml-validation/tests.rs`: 3
- `test_metadata_source_tracking.rs`: 2
- `test_inline_locations.rs`: 2

**Action**: None - test code is allowed to use defaults

### JSON Reader (19 instances) - LEGITIMATE
File: `crates/quarto-markdown-pandoc/src/readers/json.rs`

**Reason**: Backward compatibility with JSON that doesn't include source info
**Action**: Should already have documentation comments (need to verify)

### Meta (19 instances) - LEGITIMATE
File: `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`

**Reason**: Legacy format conversion, Default trait impl
**Action**: Should already have documentation comments (need to verify)

### YAML Source Info (7 instances) - CHECK
File: `crates/quarto-yaml/src/yaml_with_source_info.rs`

Lines: 269, 283, 285, 288, 299, 301, 304

**Context**: Need to check if these are in test functions
**Action**: Verify these are test-only

### Validation Crate Schema (4 instances) - LIKELY LEGITIMATE
Files:
- `private-crates/quarto-yaml-validation/src/schema/merge.rs`: 3 (lines 31, 49, 85)
- `private-crates/quarto-yaml-validation/src/schema/mod.rs`: 1 (line 255)

**Context**: Creating SchemaError for structural schema problems (not user data errors)
**Reason**: These are schema structure errors without specific source locations
**Action**: Add documentation comments explaining why default is appropriate

### Validation Crate Validator (2 instances) - CHECK
File: `private-crates/quarto-yaml-validation/src/validator.rs`

Lines: 737, 752

**Context**: Need to check if test code or production
**Action**: Verify context

### Postprocess (2 instances) - MIXED
File: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs`

#### Line 667: Math+Attr wrapper
```rust
// TODO: Should combine() source info from math and attr (see k-82)
source_info: quarto_source_map::SourceInfo::default(),
```
**Status**: Already documented, blocked by k-82
**Action**: None (waiting on k-82)

#### Line 752: Synthetic Space in citations
```rust
// Synthetic Space: inserted to separate citation from suffix
source_info: quarto_source_map::SourceInfo::default(),
```
**Status**: Already documented as synthetic
**Action**: Could enhance comment to explain it's legitimate

### Document (1 instance) - LEGITIMATE (documented)
File: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/document.rs`

Line 47:
```rust
// Legitimate default: Initial document creation - metadata populated later from YAML
meta: MetaValueWithSourceInfo::default(),
```
**Status**: Already documented
**Action**: None

## Summary

Out of 109 instances:
- **55** - Test code (legitimate)
- **19** - JSON reader backward compat (legitimate, need to verify docs)
- **19** - Meta legacy conversion (legitimate, need to verify docs)
- **7** - YAML source info (need to verify if tests)
- **4** - Validation schema errors (likely legitimate, need docs)
- **2** - Validation validator (need to check)
- **2** - Postprocess (1 blocked by k-82, 1 documented synthetic)
- **1** - Document (documented legitimate)

## Action Items

### 1. Verify test-only instances (7 + 2 = 9)
- [ ] Check yaml_with_source_info.rs lines 269-304
- [ ] Check validator.rs lines 737, 752

### 2. Verify existing documentation (38)
- [ ] Check json.rs instances have comments
- [ ] Check meta.rs instances have comments

### 3. Add documentation (4)
- [ ] Document schema/merge.rs instances (3)
- [ ] Document schema/mod.rs instance (1)

### 4. Optional enhancement (1)
- [ ] Enhance postprocess.rs:752 comment

### 5. Blocked (1)
- postprocess.rs:667 - waiting on k-82

## Conclusion

The vast majority of instances are legitimate. Main work is:
1. Verify which instances are in test code
2. Ensure production code has documentation comments
3. Add docs to validation crate schema errors
