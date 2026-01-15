# Workspace Warnings Cleanup Plan

**Date**: 2026-01-15
**Status**: Planning

## Overview

This plan addresses 16 compiler warnings in the workspace, primarily in `quarto-core` and `pampa` crates. The warnings fall into several categories, with the key consideration being that much of the "unused" code represents API surface for features not yet fully integrated.

## Warning Inventory

### 1. pampa crate (1 warning)

| Location | Type | Description |
|----------|------|-------------|
| `lib.rs:2` | `unexpected_cfgs` | `coverage_nightly` cfg attribute not registered |

**Resolution**: Add the cfg check to `Cargo.toml`:
```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(coverage_nightly)'] }
```

### 2. quarto-core crate (15 warnings)

#### 2.1 Unused Imports in `knitr/mod.rs` (4 warnings)

| Line | Import | Analysis |
|------|--------|----------|
| 44 | `RErrorInfo`, `RErrorType`, `format_r_error`, `parse_r_error` | Re-exported for public API but not used internally |
| 46 | `has_inline_r_expressions` | Re-exported but not used (could optimize preprocessing) |
| 48 | `within_active_renv` | Re-exported but not used externally |
| 50 | `KnitrRequest` | Re-exported but not used externally |

**Analysis**: These are intentional public API re-exports. The functions are used internally but the re-exports in `mod.rs` are for external consumers. We have two options:
1. Remove re-exports and mark functions `pub(crate)`
2. Add `#[allow(unused_imports)]` with documentation explaining these are public API

**Recommendation**: Option 2 - Keep as public API with allow attribute, since these functions have tests and will be useful to external consumers.

#### 2.2 Unused Variable (1 warning)

| Location | Variable | Analysis |
|----------|----------|----------|
| `jupyter/session.rs:143` | `request_id` | Parameter unused in `wait_for_kernel_info_reply` |

**Analysis**: The `request_id` is passed to correlate request/response but the current implementation doesn't use it (see comment at line 161: "runtimelib doesn't expose recv for it"). This is a known limitation.

**Resolution**: Prefix with `_request_id` to indicate intentionally unused.

#### 2.3 Dead Code - Functions (7 warnings)

| Location | Function | Usage | Recommendation |
|----------|----------|-------|----------------|
| `kernelspec.rs:90` | `find_kernelspec_for_language` | Called by `resolve_kernel` | Keep - part of kernel resolution API |
| `kernelspec.rs:117` | `resolve_kernel` | Entry point for kernel resolution | Keep - integrate with tests |
| `kernelspec.rs:146` | `extract_kernel_from_metadata` | Helper for `resolve_kernel` | Keep - has unit tests |
| `output.rs:266` | `mime_priority_for_format` | Format-specific MIME ordering | Keep - has unit test, will be needed |
| `error_parser.rs:290` | `format_r_error` | Formats errors for display | Keep - has unit tests, useful API |
| `preprocess.rs:98` | `has_inline_r_expressions` | Optimization check | Keep - has tests, useful for optimization |
| `format.rs:234` | `KnitrFormatConfig::new` | Constructor | Keep or remove - `with_defaults` is used instead |
| `subprocess.rs:205` | `CallROptions::quiet` | Constructor | Keep - useful factory method |

**Analysis**: Most of these functions are:
1. Part of a coherent API (kernel resolution, error handling)
2. Have existing unit tests
3. Will be needed when features are fully integrated

**Strategy**:
- Add integration tests that exercise the code paths
- Mark remaining unused items with `#[allow(dead_code)]` and TODO comments
- For `KnitrFormatConfig::new` - consider removing if `with_defaults` covers all use cases

#### 2.4 Unused Struct Fields (4 warnings)

| Location | Field | Analysis |
|----------|-------|----------|
| `session.rs:47` | `connection_info` | Stored for future use (restart, reconnect) |
| `session.rs:55` | `session_id` | Stored for message correlation |
| `types.rs:117` | `engine_dependencies` | Deserialized from R, not yet processed |
| `types.rs:121` | `preserve` | Deserialized from R, not yet processed |

**Analysis**: These fields are legitimately needed but the processing logic hasn't been implemented yet.

**Resolution**: Add `#[allow(dead_code)]` with a comment linking to this plan document for context:
```rust
// TODO: Processing not yet implemented. See analysis in:
// claude-notes/plans/2026-01-15-workspace-warnings-cleanup.md (Section 2.4)
#[allow(dead_code)]
```

## Implementation Plan

### Phase 1: Simple Fixes (Low Risk)
1. Fix `coverage_nightly` cfg in pampa's Cargo.toml
2. Rename `request_id` to `_request_id` in jupyter/session.rs

### Phase 2: Public API Clarification
1. Review knitr re-exports and decide which should remain public API
2. Add `#[allow(unused_imports)]` with doc comments for intentional re-exports
3. Mark internal-only items as `pub(crate)`

### Phase 3: Integration Testing
For the dead code that represents useful API, add integration tests:

1. **Kernel Resolution Tests** (kernelspec.rs)
   - Test `resolve_kernel` with various metadata configurations
   - Test `find_kernelspec_for_language` (requires mocking or integration env)

2. **MIME Priority Tests** (output.rs)
   - Already has unit test, consider expanding

3. **Error Formatting Tests** (error_parser.rs)
   - `format_r_error` already has unit tests
   - Consider integration test with actual error scenarios

4. **Preprocessing Tests** (preprocess.rs)
   - `has_inline_r_expressions` already has unit tests
   - Consider using it in the actual preprocessing path as optimization

### Phase 4: Deliberate Allowances
For items that are intentionally unused pending future work:
1. Add `#[allow(dead_code)]` with `// TODO: ` comment
2. Create beads issues for the integration work

### Phase 5: Cleanup
For items that are truly unnecessary:
1. `KnitrFormatConfig::new` - evaluate if `with_defaults` suffices
2. Remove if not needed, or add test coverage if useful

## Testing Requirements

Before closing this issue, ensure:
- [ ] All warnings are resolved (either fixed or explicitly allowed)
- [ ] `cargo build --workspace` produces no warnings
- [ ] `cargo nextest run --workspace` passes
- [ ] Any new `#[allow(...)]` attributes have explanatory comments
- [ ] Any truly unused code has been removed

## Risk Assessment

- **Low Risk**: Phase 1 changes are mechanical
- **Medium Risk**: Phase 2 changes affect public API surface
- **Integration Tests**: Phase 3 may require R/Jupyter to be installed for some tests

## Notes

The key insight is that this isn't just dead code cleanup - much of this code represents thoughtfully designed API that isn't yet wired up. The goal should be to either:
1. Wire it up with proper integration tests
2. Explicitly mark it as "pending integration" with allow attributes
3. Remove it only if it's truly not needed

This preserves the value of the existing implementation while maintaining a clean build.
