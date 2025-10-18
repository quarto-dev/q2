# Versioning Strategy for Kyoto/Quarto 2.0

**Date:** 2025-10-12
**Status:** Decided
**Decision:** Option A - Separate Internal vs User-Facing Version

## Context

Quarto uses version numbering for extension compatibility checks. Extensions declare minimum version ranges (but not maximum), and the CLI reports a version that's used for these compatibility comparisons.

For the Rust port (codename "kyoto", but will be called "quarto" in crates), we need:
1. A version number that's idiomatic for Rust/Cargo ecosystem
2. A version number that works correctly with Quarto's extension compatibility system
3. Clear signaling that this is development/alpha code

## The Problem

**Tension:**
- Rust ecosystem convention: Use `0.x.y` for "unstable, expect breaking changes"
- Quarto ecosystem needs: Version must compare >= existing 1.x versions for extension compatibility

Using `0.1.0` would break extension compatibility checks that require `>= 1.4.0` for example.

## Solution: Option A - Separate Internal vs User-Facing Version

### Cargo.toml Version
```toml
[package]
version = "0.1.0"
```

**Purpose:**
- Idiomatic for Rust ecosystem
- Signals "experimental, unstable" to developers
- Follows SemVer conventions for crates.io
- Can increment through `0.x.y` during development
- Jump to `2.0.0` when production-ready

### Runtime/CLI Reported Version
```rust
// In code
const DEV_VERSION: &str = "99.9.9-dev";

pub fn get_cli_version() -> &'static str {
    // During development
    DEV_VERSION

    // After release (via build script or feature flag)
    // env!("CARGO_PKG_VERSION")
}
```

**Purpose:**
- `99.9.9-dev` compares > any practical 1.x or 2.x version
- Satisfies all extension compatibility checks (e.g., `>= 1.4.0`)
- Clear `-dev` suffix signals development status to users
- Proven approach from quarto-cli v1

### Implementation Details

**build.rs approach:**
```rust
// build.rs
fn main() {
    let cargo_version = env!("CARGO_PKG_VERSION");

    // If cargo version is 0.x.y, report dev version
    // If cargo version is 2.x.y, report actual version
    if cargo_version.starts_with("0.") {
        println!("cargo:rustc-env=QUARTO_VERSION=99.9.9-dev");
    } else {
        println!("cargo:rustc-env=QUARTO_VERSION={}", cargo_version);
    }
}
```

**Usage in CLI:**
```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "quarto")]
#[command(version = env!("QUARTO_VERSION"))]  // Shows 99.9.9-dev
#[command(about = "Quarto document processor (Development Build)")]
struct Cli {
    // ...
}
```

### Version Comparison Logic

When checking extension compatibility:
```rust
// Extension requires: minimum_version = "1.4.0"
// Current version: "99.9.9-dev"

fn is_compatible(current: &str, required: &str) -> bool {
    // Strip -dev suffix for comparison
    let current_base = current.trim_end_matches("-dev");

    // Parse as SemVer
    let current_ver = Version::parse(current_base)?;
    let required_ver = Version::parse(required)?;

    // 99.9.9 > 1.4.0 → compatible ✓
    current_ver >= required_ver
}
```

### Migration Path

**Phase 1: Development (Now)**
```toml
# Cargo.toml
version = "0.1.0"

# CLI reports
99.9.9-dev
```

**Phase 2: Alpha Testing**
```toml
# Cargo.toml
version = "0.2.0"  # Can increment minor for features

# CLI reports
99.9.9-dev  # Still dev version
```

**Phase 3: Beta**
```toml
# Cargo.toml
version = "0.9.0"  # Approaching 1.0 semantically

# CLI reports (optional transition)
2.0.0-beta.1
```

**Phase 4: Production Release**
```toml
# Cargo.toml
version = "2.0.0"

# CLI reports
2.0.0
```

## Alternatives Considered

### Option B: Use 99.9.9 Everywhere
```toml
version = "99.9.9"
```

**Pros:**
- Simple, consistent
- Proven in quarto-cli v1

**Cons:**
- Not idiomatic for Rust/Cargo
- Awkward on crates.io
- Would need to change all Cargo.toml files at release

**Verdict:** ❌ Too un-idiomatic for Rust ecosystem

### Option C: Use 2.0.0-alpha.X
```toml
version = "2.0.0-alpha.1"
```

**Pros:**
- Semantic and clear
- Explicitly version 2

**Cons:**
- Requires custom comparison logic (2.0.0-alpha < 1.4.0 in SemVer!)
- Complex compatibility checks
- Pre-release versions compare differently

**Verdict:** ❌ Breaks extension compatibility

### Option D: Custom Version Comparison
```rust
// Treat 2.0.0-alpha as "satisfies any >= 1.x requirement"
```

**Pros:**
- Semantically cleaner

**Cons:**
- Complex implementation
- Error-prone
- Needs custom logic throughout

**Verdict:** ❌ Unnecessary complexity

## Benefits of Option A

1. **Ecosystem Compliance**
   - Rust: `0.x.y` correctly signals instability
   - Quarto: `99.9.9` correctly satisfies compatibility

2. **Clear Communication**
   - Developers see `0.1.0` → "unstable crate"
   - Users see `99.9.9-dev` → "development build"

3. **Proven Approach**
   - quarto-cli v1 uses `99.9.9` for dev builds
   - Many Rust projects separate internal/external versions

4. **Simple Migration**
   - Change one constant when ready to release
   - No Cargo.toml mass updates needed

5. **Testing Flexibility**
   - Can test release behavior with feature flags
   - Easy to simulate production version

## Implementation Checklist

- [ ] Add version constants to `quarto-util` crate
- [ ] Implement `build.rs` for version switching
- [ ] Update CLI to use `QUARTO_VERSION` env var
- [ ] Add `--version` flag that shows `99.9.9-dev`
- [ ] Implement extension compatibility checker
- [ ] Add tests for version comparison logic
- [ ] Document versioning in CONTRIBUTING.md

## References

- quarto-cli v1: Uses `99.9.9` for development builds
- Cargo Book: [SemVer Compatibility](https://doc.rust-lang.org/cargo/reference/semver.html)
- SemVer Spec: Pre-release versions compare lower than release versions
- Rust API Guidelines: [C-SEMVER](https://rust-lang.github.io/api-guidelines/necessities.html#c-semver)
