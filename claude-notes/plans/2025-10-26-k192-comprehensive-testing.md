# Comprehensive Testing Plan for k-192

**Date**: 2025-10-26
**Status**: In Progress
**Owner**: Claude Code
**Parent Task**: k-192 (Phase 5: Write comprehensive tests for annotated Pandoc AST)

## Current Status
- **74 tests passing** across 10 test files (~3200 lines of test code)
- **Good coverage**: Individual block types, inline types, metadata, source mapping
- **Strong foundation**: Substring invariant, offset invariant, type safety

## Gaps Identified

**1. DocumentConverter (HIGH PRIORITY)**
- No tests for the orchestrating `DocumentConverter` class
- Need to test: `convertDocument()`, `convertBlocks()`, `convertBlock()`, `convertInline()`

**2. Complex Documents (HIGH PRIORITY)**
- Current tests focus on single types in isolation
- Missing: Real-world documents with mixed content
- Missing: Deeply nested structures (lists in divs, notes with complex blocks, etc.)

**3. Edge Cases - k-199 (MEDIUM PRIORITY)**
- Empty content: empty paragraphs, empty lists, empty metadata
- Null/undefined handling
- Boundary conditions: single-item lists, minimal documents

**4. Components Tree Structure (MEDIUM PRIORITY)**
- Current tests check `components` exists and has correct count
- Missing: Systematic validation of tree structure
- Missing: Tests for navigating the components tree

**5. Performance (LOW PRIORITY)**
- No tests for large documents (100+ blocks)
- No tests for deeply nested structures (10+ levels)
- Need baseline performance metrics

**6. Documentation & Examples (LOW PRIORITY)**
- README has basic usage but missing:
  - Linting use case examples
  - Complete API reference
  - Migration guide

## Implementation Phases

### Phase 1: DocumentConverter Tests (CRITICAL)
**Beads Task**: k-226
**Priority**: Must Do

**Objectives**:
1. Create `test/document-converter.test.ts`
2. Test complete document conversion (meta + blocks)
3. Test blocks-only conversion
4. Test single block conversion
5. Test single inline conversion
6. Verify components interleave metadata and blocks correctly

**Tests to Write**:
- Complete document conversion (simple.json)
- Document with metadata only
- Document with blocks only
- Blocks array conversion
- Single block conversion (various types)
- Single inline conversion (various types)
- Verify component ordering

**Estimated Effort**: 2-3 hours

### Phase 2: Complex Document Tests (HIGH PRIORITY)
**Beads Task**: k-227
**Priority**: Must Do

**Objectives**:
1. Create `test/complex-documents.test.ts`
2. Create realistic complex document fixtures
3. Test end-to-end conversion with validation
4. Test deeply nested structures

**Fixtures to Create**:
1. **blog-post.qmd**: Mixed formatting, lists, images, code blocks, links
2. **academic-paper.qmd**: Sections, citations, footnotes, tables, math
3. **tutorial.qmd**: Nested callouts (divs), code blocks, figures, step-by-step lists

**Tests to Write**:
- Full document structure validation (not just types)
- Source mapping accuracy throughout
- Components tree navigation
- Nested structure validation

**Estimated Effort**: 3-4 hours

### Phase 3: Edge Cases (MEDIUM PRIORITY)
**Beads Task**: k-199 (existing)
**Priority**: Should Do

**Objectives**:
1. Create `test/edge-cases.test.ts`
2. Test empty content scenarios
3. Test minimal documents
4. Test boundary conditions
5. Ensure graceful handling without errors

**Tests to Write**:
- Empty paragraph
- Empty list (BulletList, OrderedList)
- Empty metadata
- Empty code block
- Empty string values
- Null/undefined handling
- Single-item list
- Minimal document (just metadata)
- Minimal document (just one block)

**Estimated Effort**: 2-3 hours

### Phase 4: Components Tree Validation (MEDIUM PRIORITY)
**Beads Task**: k-228
**Priority**: Should Do

**Objectives**:
1. Create helper `validateComponentsTree()` function
2. Enhance existing tests with tree structure validation
3. Test component ordering and nesting
4. Test tree navigation patterns

**Enhancements**:
- Add tree validation to existing block tests
- Add tree validation to existing inline tests
- Add tree validation to metadata tests
- Test parent-child relationships
- Test component ordering in lists

**Estimated Effort**: 2-3 hours

### Phase 5: Performance Baseline (OPTIONAL)
**Beads Task**: k-229
**Priority**: Nice to Have

**Objectives**:
1. Create `test/performance.test.ts`
2. Generate large test documents
3. Measure conversion time
4. Set baseline expectations

**Tests to Write**:
- Large document (100-200 blocks)
- Deeply nested structure (10+ levels)
- Large table (50+ rows)
- Performance baseline assertions (<100ms for typical docs)

**Estimated Effort**: 1-2 hours

### Phase 6: Documentation (OPTIONAL)
**Beads Task**: k-230
**Priority**: Nice to Have

**Objectives**:
1. Expand README with linting examples
2. Document AnnotatedParse tree structure
3. Add cookbook-style recipes
4. Document best practices

**Deliverables**:
- Linting use case examples
- API reference expansion
- Tree navigation examples
- Source mapping best practices

**Estimated Effort**: 3-4 hours

## Success Criteria

✅ DocumentConverter has comprehensive unit tests
✅ At least 3 complex end-to-end document tests
✅ All edge cases handled gracefully (k-199 closed)
✅ Components tree structure systematically validated
✅ Test count increases to 90-100+ tests
✅ All existing tests still pass

## Estimated Total Effort

**Core work (Phases 1-3)**: 7-10 hours
**With quality enhancements (Phase 4)**: 9-13 hours
**Complete with optional (Phases 5-6)**: 13-18 hours

## Dependencies

- Existing test infrastructure (jest/node:test)
- Example fixtures in `examples/` directory
- quarto-markdown-pandoc binary for generating fixtures

## Notes

- Focus on **Phases 1-3** for k-192 completion
- Phase 4 improves quality but not strictly required
- Phases 5-6 are deferred enhancements
- All phases maintain TDD approach: write test, verify failure, implement/fix, verify pass
