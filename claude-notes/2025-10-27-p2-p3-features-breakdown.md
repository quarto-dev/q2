# P2/P3 YAML Schema Features Breakdown

Created: 2025-10-27

## Overview

This document tracks the remaining P2 (Medium priority) and P3 (Lower priority) YAML schema patterns that were identified during the bd-8 audit but deferred after Phase 2 implementation.

**Parent Issue**: k-244 - Implement P2/P3 YAML schema patterns for completeness

## Status

**Current**: All P0 (Critical) and P1 (High priority) patterns are complete with 100% success rate on tested quarto-cli schemas.

**Remaining**: 7 patterns (6 P2 + 1 P3) tracked as subtasks of k-244

## P2 Patterns (Medium Priority)

These patterns are used in some quarto-cli schemas but are not critical for current functionality.

### 1. Nested Property Extraction (k-245)
**Status**: Open
**Priority**: P2
**Estimated**: 2-3 hours

**Description**: Implement double setBaseSchemaProperties pattern where annotations can be applied at multiple levels and merge/override.

**Example**:
```yaml
schema:
  anyOf:
    - boolean
    - string
description: "Outer description"
completions: ["value1", "value2"]
```

**Implementation Location**: `src/schema/parsers/wrappers.rs` (TODO comment exists)

**References**:
- quarto-cli: `src/core/lib/yaml-schema/from-yaml.ts` lines 59-82 (setBaseSchemaProperties)
- Plan: `claude-notes/plans/2025-10-27-bd-8-yaml-schema-from-yaml-comprehensive-plan.md`

---

### 2. Schema Inheritance (k-246)
**Status**: Open
**Priority**: P2
**Estimated**: 3-4 hours

**Description**: Implement super/baseSchema pattern for schema composition and extension.

**Pattern**:
```yaml
super: base/schema
properties:
  additional: string
```

**Implementation Needs**:
- Add `super` field to schema types
- Schema resolution and merging logic
- Circular dependency detection

**References**:
- Plan: `claude-notes/plans/2025-10-27-bd-8-yaml-schema-from-yaml-comprehensive-plan.md`

---

### 3. ResolveRef vs Ref Distinction (k-247)
**Status**: Open
**Priority**: P2
**Estimated**: 1-2 hours

**Description**: Implement distinction between `ref` and `resolveRef` for different resolution strategies.

**Current**: Both `ref` and `$ref` are treated identically
**Needed**: Different resolution behavior for `resolveRef`

**Implementation Location**:
- `src/schema/parsers/ref.rs`
- `src/schema/types.rs` (RefSchema)

**References**:
- quarto-cli: `src/core/lib/yaml-schema/from-yaml.ts` (resolveRef handling)

---

### 4. PropertyNames Support (k-248)
**Status**: Open
**Priority**: P2
**Estimated**: 2 hours

**Description**: Add propertyNames validation to object schemas (JSON Schema standard feature).

**Pattern**:
```yaml
object:
  propertyNames:
    pattern: "^[a-z]+$"
```

**Implementation Location**:
- `src/schema/parsers/objects.rs` (parsing)
- `src/schema/types.rs` (ObjectSchema struct)
- Validation logic (future)

---

### 5. NamingConvention Validation (k-249)
**Status**: Open
**Priority**: P2
**Estimated**: 2 hours

**Description**: Quarto extension for validating property naming conventions.

**Pattern**:
```yaml
object:
  namingConvention: camelCase  # or snake_case, kebab-case
```

**Implementation Location**: `src/schema/parsers/objects.rs`

**Validation Rules**:
- camelCase: `myPropertyName`
- snake_case: `my_property_name`
- kebab-case: `my-property-name`

---

### 6. AdditionalCompletions Support (k-250)
**Status**: Open
**Priority**: P2
**Estimated**: 1-2 hours

**Description**: Support additional sources for IDE completions beyond static values.

**Pattern**:
```yaml
string:
  completions: ["static1", "static2"]
  additionalCompletions:
    - source: "function"
      function: "getAvailableFormats"
```

**Implementation Location**: `src/schema/annotations.rs`

## P3 Patterns (Lower Priority)

These patterns are rarely used in quarto-cli and have the lowest priority.

### 7. Pattern as Schema Type (k-251)
**Status**: Open
**Priority**: P3
**Estimated**: 2-3 hours

**Description**: Implement `pattern` as a primary schema type (not just a string validation constraint).

**Pattern**:
```yaml
pattern: "^[a-z]+$"  # Top-level pattern type
```

**Current**: Pattern only works as constraint in string schemas
**Needed**: Pattern as standalone schema type

**Implementation Location**:
- `src/schema/parser.rs` (add pattern case)
- `src/schema/types.rs` (add PatternSchema)
- `src/schema/parsers/` (new pattern.rs module)

## Implementation Strategy

### Phase 1: P2 Features (Estimated 13-17 hours)
Work through P2 features in order of impact:
1. k-245: Nested property extraction (most commonly needed)
2. k-248: PropertyNames (JSON Schema standard)
3. k-249: NamingConvention (Quarto-specific validation)
4. k-247: ResolveRef distinction (affects reference resolution)
5. k-250: AdditionalCompletions (IDE support)
6. k-246: Schema inheritance (most complex, do last)

### Phase 2: P3 Features (Estimated 2-3 hours)
1. k-251: Pattern as type (if needed based on usage)

### Testing Strategy
For each feature:
1. Write failing test first (TDD)
2. Implement feature
3. Verify test passes
4. Add integration test with quarto-cli example
5. Update documentation

### Documentation Updates Needed
After implementing each feature:
- Update `SCHEMA-FROM-YAML.md` with pattern documentation
- Add to pattern correspondence table
- Include real-world examples
- Update README.md status section

## Priority Rationale

**P2 Features**: Medium priority because they appear in some quarto-cli schemas but are not blocking current functionality. Implementing these provides completeness and may unblock future quarto-cli schema updates.

**P3 Features**: Lower priority because they are rarely used. Can be implemented on-demand if specific schemas need them.

## Dependencies

All subtasks (k-245 through k-251) are children of the parent epic k-244. They can be worked on independently, though k-246 (schema inheritance) may benefit from k-247 (resolveRef) being done first.

## References

- **Comprehensive Plan**: `claude-notes/plans/2025-10-27-bd-8-yaml-schema-from-yaml-comprehensive-plan.md`
- **Phase 1 Audit**: `claude-notes/audits/2025-10-27-bd-8-phase1-schema-from-yaml-audit.md`
- **Phase 2 Complete**: `claude-notes/completion/2025-10-27-phase2-implementation-complete.md`
- **Phase 3 Testing**: `claude-notes/completion/2025-10-27-phase3-testing-complete.md`
- **bd-8 Complete**: `claude-notes/completion/2025-10-27-bd-8-complete.md`
- **quarto-cli source**: `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`
