# Pre-Commit Review Checklist

**Read this file and complete the checklist before making any commit. Do not skip items.**

**When checklist is finished, stage your changes and report results to the user.** Whether to then commit without waiting depends on the mode of work:

- **Plan-driven execution the user has already approved** (a plan document with phases, user said "go ahead"): **commit-and-continue at clean phase boundaries.** A phase boundary is clean when the checklist passes and the full workspace test suite is green. Report the commit in the running summary; do not stop to ask. (Policy set 2026-08-10, project-profiles session.)
- **Anything else** (ad-hoc changes, work outside an approved plan, a phase that ended dirty — failing tests, skipped items, surprising snapshot diffs): report and **wait for approval before committing**.

Pushing always requires explicit approval regardless of mode (see CLAUDE.md).

## Determinism

- [ ] Grep for `use std::collections::HashMap` in changed files - verify none affect output ordering
- [ ] Grep for `use rustc_hash::FxHashMap` in changed files - verify none are serialized or iterated
- [ ] Any new map in a `#[derive(Serialize)]` struct uses `hashlink::LinkedHashMap`
- [ ] No `#[serde(flatten)]` on HashMap fields

### HashMap Usage Guide

| Use Case | Allowed Type |
|----------|--------------|
| Serialized struct field | `LinkedHashMap` only |
| Field with `#[serde(flatten)]` | `LinkedHashMap` only |
| Iterated to produce output | `LinkedHashMap` only |
| Internal cache (pointer/index keys) | `FxHashMap` OK |
| Lookup-only, never iterated | `FxHashMap` OK |

When in doubt, use `LinkedHashMap`.

## Tests

- [ ] All tests pass (`cargo nextest run`)
- [ ] New functionality has corresponding tests
- [ ] Bug fixes have regression tests (written BEFORE the fix)
- [ ] Snapshot tests reviewed for unintended changes

## Code Coverage

- [ ] Check coverage for modified crates before and after changes
- [ ] New code should have meaningful test coverage (aim for >80% on new code paths)
- [ ] Don't let coverage regress significantly on modified files

### Checking Coverage

```bash
# Coverage for a specific crate
cargo llvm-cov --package <crate-name> --html

# Coverage for specific test
cargo llvm-cov --package <crate-name> --html -- <test-name>

# Open the HTML report
open target/llvm-cov/html/index.html
```

### Coverage Guidelines

| Situation | Expectation |
|-----------|-------------|
| New module/feature | Aim for >80% coverage |
| Bug fix | Add test that covers the fixed code path |
| Refactoring | Maintain or improve existing coverage |
| Modified file | Coverage should not decrease |

When adding tests, prioritize:
1. Error handling paths (often missed)
2. Edge cases and boundary conditions
3. Integration points between modules

## Code Quality

- [ ] No TODO comments without braid strand IDs
- [ ] `cargo fmt` has been run on changed files
- [ ] `cargo clippy` passes (or warnings explained)
- [ ] No over-engineering: only changes directly requested or clearly necessary

## Security

- [ ] No hardcoded secrets or credentials
- [ ] User input validated at system boundaries
- [ ] No command injection vulnerabilities in shell calls

## Final Verification

Run these commands before committing:

```bash
# Check for HashMap anti-patterns in staged files
git diff --cached --name-only | xargs rg "use std::collections::HashMap" 2>/dev/null
git diff --cached --name-only | xargs rg "use rustc_hash::FxHashMap" 2>/dev/null

# Verify tests pass
cargo nextest run

# Format check
cargo fmt --check
```
