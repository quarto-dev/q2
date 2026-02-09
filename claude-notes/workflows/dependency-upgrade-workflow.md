# Dependency Upgrade Workflow

Date: 2026-02-06
File: claude-notes/workflows/dependency-upgrade-workflow.md

## Purpose

Periodic upgrade of Rust crate dependencies in the workspace. This workflow is
designed to be run by Claude Code autonomously, invoked with something like:

> Let's upgrade dependencies. Follow the workflow at claude-notes/workflows/dependency-upgrade-workflow.md

## Prerequisites

### Required Tools

These tools must be installed before running this workflow:

```bash
# cargo-edit: provides `cargo upgrade` (modifies Cargo.toml version strings)
cargo install cargo-edit

# cargo-outdated: shows dependencies with newer versions available
cargo install cargo-outdated

# cargo-audit: checks for known security vulnerabilities
cargo install cargo-audit
```

Verify they're installed:

```bash
cargo upgrade --version   # Should show cargo-upgrade version
cargo outdated --version  # Should show cargo-outdated version
cargo audit --version     # Should show cargo-audit version
```

### Workspace Context

- Dependencies are centralized in `[workspace.dependencies]` in the root `Cargo.toml`
- `Cargo.lock` is checked into the repo
- WASM crates (`wasm-quarto-hub-client`, `wasm-qmd-parser`) are excluded from
  the default workspace but depend on workspace crates — changes can break them

## Key Concepts

There are two layers of dependency versions:

| Layer | File | What it controls |
|-------|------|------------------|
| Constraints | `Cargo.toml` | Allowed version range (e.g., `"1.0.215"` means `>=1.0.215, <2.0.0`) |
| Resolution | `Cargo.lock` | Exact version used in builds |

This gives us a natural progression of increasing risk:

1. **`cargo update`** — updates `Cargo.lock` within existing constraints (safe)
2. **`cargo upgrade`** — bumps version strings in `Cargo.toml` (moderate risk)
3. **`cargo upgrade --incompatible`** — crosses major version boundaries (high risk)

## Workflow

### Phase 0: Preparation

```bash
# Start from a clean working tree
git status

# Check for security vulnerabilities first
cargo audit
```

If `cargo audit` reports vulnerabilities, note them. Security fixes take priority
and should be addressed in Phase 1 or 2 even if they require breaking changes.

Create a plan file at `claude-notes/plans/YYYY-MM-DD-dependency-upgrade.md` to
track progress. Use this template:

```markdown
# Dependency Upgrade — YYYY-MM-DD

## Phase 0: Preparation
- [ ] Clean working tree
- [ ] Run cargo audit
- [ ] Note any security advisories: (list here)

## Phase 1: Lock File Update
- [ ] cargo update --dry-run (review changes)
- [ ] cargo update
- [ ] cargo build --workspace
- [ ] cargo nextest run --workspace
- [ ] Commit Cargo.lock

## Phase 2: Bump Declared Minimums
- [ ] cargo outdated --workspace (record what's available)
- [ ] cargo upgrade --workspace (compatible only)
- [ ] cargo build --workspace
- [ ] cargo nextest run --workspace
- [ ] cargo xtask verify (WASM builds)
- [ ] Commit Cargo.toml + Cargo.lock

## Phase 3: Breaking Upgrades
- [ ] Review cargo outdated output for major version bumps
- [ ] (list each breaking upgrade as a sub-item)

## Summary
(fill in after completion)
```

### Phase 1: Lock File Update (Low Risk)

This updates `Cargo.lock` to the latest versions that satisfy existing
`Cargo.toml` constraints. This is safe because nothing changes about the
*allowed* version range — only the *resolved* version within that range.

```bash
# Preview what would change
cargo update --dry-run

# Apply
cargo update

# Verify everything still builds and passes with no warnings
cargo build --workspace
cargo build --workspace --tests
cargo nextest run --workspace
```

**Important: Fix new compiler warnings immediately.** Dependency upgrades
frequently deprecate methods or change APIs in ways that introduce warnings.
Check for warnings after both `cargo build --workspace` (library/binary code)
and `cargo build --workspace --tests` (test code), and fix them before
committing. Common warning categories after upgrades:

- **Deprecated methods** — check the crate's changelog/migration guide for
  the replacement API
- **Unused `Result`** — new versions may change return types; propagate
  with `?` or use `let _ =`
- **Changed trait bounds** — may require updating generic constraints

If tests pass and warnings are clean, commit:

```bash
git add Cargo.lock
git commit -m "Update Cargo.lock to latest compatible versions"
```

If tests fail, investigate. A test failure here means a dependency published a
semver-compatible version that changed behavior. This is rare but does happen.
Options:
- Fix the test if the new behavior is correct
- Pin the problematic dependency: `cargo update <crate> --precise <old-version>`
- Report the issue upstream if it's a semver violation

### Phase 2: Bump Declared Minimums (Moderate Risk)

This updates the version strings in `Cargo.toml` to match what's actually
available. For example, if `Cargo.toml` says `serde = "1.0.215"` but `1.0.230`
is out, this bumps the declared minimum to `"1.0.230"`.

```bash
# First, see the full picture of what's outdated
cargo outdated --workspace

# Bump compatible (non-breaking) versions in Cargo.toml
# Note: cargo upgrade operates on the whole workspace by default
# (there is no --workspace flag)
cargo upgrade --dry-run    # Preview changes
cargo upgrade              # Apply

# Verify (build + tests + warnings)
cargo build --workspace
cargo build --workspace --tests
cargo nextest run --workspace

# Also verify WASM builds since Cargo.toml changed
cargo xtask verify
```

**Fix any new compiler warnings before committing** (see Phase 1 for details).

If everything passes and is warning-clean, commit:

```bash
git add Cargo.toml Cargo.lock
git commit -m "Bump dependency versions to latest compatible releases"
```

### Phase 3: Breaking Upgrades (High Risk — Interactive)

Major version bumps can introduce breaking API changes. These should be done
**one dependency at a time**, each in its own commit.

```bash
# See what has major version bumps available
cargo outdated --workspace
```

**Before upgrading each dependency:**
1. Check its changelog/release notes
2. Assess the scope of breaking changes
3. Discuss with the user if the upgrade is worthwhile

**For each breaking upgrade:**

```bash
# Upgrade a single dependency
cargo upgrade --incompatible -p <crate_name>

# See what broke
cargo build --workspace 2>&1 | head -100

# Fix compile errors AND warnings, then verify
cargo build --workspace --tests   # also check test code for warnings
cargo nextest run --workspace
cargo xtask verify

# Commit separately
git add Cargo.toml Cargo.lock <any modified source files>
git commit -m "Upgrade <crate_name> to <new_version>"
```

### Coupled Dependencies

Some dependencies must be upgraded together. Currently known groups:

**Deno runtime** (version comment in root `Cargo.toml`):
- `deno_core`
- `deno_web`
- `deno_webidl`
- `serde_v8`

These must be upgraded atomically — `deno_core` version must match what
`deno_web` depends on. Check the Deno repository for compatible version sets
before upgrading.

**tree-sitter**:
- `tree-sitter` (the main crate)
- Any tree-sitter grammar crates that depend on it

When you discover new coupling groups during upgrades, add them to this section.

## Phase 4: Final Verification

After all upgrades are complete:

```bash
# Full verification including WASM
cargo xtask verify
```

Update the plan file with a summary of what was upgraded.

## Decision Guidelines

### When to skip a breaking upgrade

- The new major version removes APIs we use heavily (high migration cost)
- The crate is in maintenance mode and the current version works fine
- The upgrade pulls in heavy new dependencies

### When to prioritize a breaking upgrade

- Security advisory on the current version
- The new version fixes bugs we've encountered
- The new version has features we need

### Handling failures

- **Build failure in Phase 1**: Likely a semver violation upstream. Pin with
  `--precise` and report.
- **Test failure in Phase 2**: Investigate whether the test expectation or the
  dependency behavior is correct.
- **Build failure in Phase 3**: Expected — fix the API migration. If it's too
  large, skip and note it in the plan.
- **WASM build failure**: Changes to `quarto-core` or `quarto-pandoc-types`
  types can break the WASM crate. Run `cargo xtask verify` to catch this early.

## Frequency

| Action | Recommended frequency |
|--------|----------------------|
| Phase 0 (`cargo audit`) | Weekly, or before releases |
| Phase 1 (`cargo update`) | Weekly, or before releases |
| Phase 2 (`cargo upgrade`) | Monthly |
| Phase 3 (breaking upgrades) | As needed (security, features) |
