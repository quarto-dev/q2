# Tree-Sitter Syntax Highlighting: WASM32-Unknown-Unknown Compatibility Audit

**Date:** April 19, 2026
**Scope:** wasm32-unknown-unknown browser target via wasm-bindgen/wasm-pack
**Status:** No blocker — should work out of the box.

> **Correction note (2026-04-19):** An earlier draft of this document claimed
> `LazyLock` was a WASM blocker. That claim is wrong. `LazyLock` and
> `OnceLock` use `std::sync::Once` internally, which is available on
> single-threaded `wasm32-unknown-unknown` — the API just never has
> contention. Our own `pampa` crate (which is compiled into the already-
> shipping `wasm-qmd-parser` and `wasm-quarto-hub-client` WASM binaries)
> already uses `OnceLock` (`crates/pampa/src/json_filter.rs:20-30`) and
> `once_cell::sync::Lazy` (`crates/pampa/src/pandoc/treesitter.rs:57,477`).
> The sections below are kept for traceability, but the patch-LazyLock
> recommendation in the original "Recommendation" section is rescinded.

## Executive Summary

**tree-sitter-highlight** should compile and run in wasm32-unknown-unknown
without modification:

1. **`LazyLock` is fine** — same story as `OnceLock`, which we already use in WASM.
2. **`AtomicUsize` is fine** — load/store on `usize`-sized atomics is available on wasm32-unknown-unknown; ordering is vacuous in single-threaded mode, which is correct.
3. **Grammar crates compile fine** — follow the proven tree-sitter-qmd pattern (cc + C parser).
4. **Bundle size is acceptable** — ~17 MB for full hub-client with existing grammars; each additional grammar adds roughly 150–300 KB uncompressed, ≈ 30–70 KB compressed.

**Bottom line:** We can use tree-sitter-highlight + bundled grammar crates in `wasm-quarto-hub-client` without architectural compromise. The only real cost is **bundle size per bundled language grammar**. If that becomes a concern, the alternative is the separate `web-tree-sitter` JavaScript runtime which loads `.wasm` grammars dynamically on demand — but that's a different architecture and out of scope for v1.

---

## 1. tree-sitter-highlight Crate Analysis

### Dependency Review
**File:** `/Users/cscheid/repos/github/quarto-dev/q2/external-sources/tree-sitter/crates/highlight/Cargo.toml`

```toml
[dependencies]
regex.workspace              = true
streaming-iterator.workspace = true
thiserror.workspace          = true
tree-sitter.workspace = true
```

**Assessment:** No obvious blockers (regex builds fine on WASM).

### Critical Issue: LazyLock in highlight.rs

**File:** `/Users/cscheid/repos/github/quarto-dev/q2/external-sources/tree-sitter/crates/highlight/src/highlight.rs:30–87`

```rust
use std::sync::{LazyLock, atomic::{AtomicUsize, Ordering}};

static STANDARD_CAPTURE_NAMES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    vec![...].into_iter().collect()
});
```

**Problem:**  
- `LazyLock` blocks threads during first access to ensure initialization happens once
- **wasm32-unknown-unknown is single-threaded** — no threading primitives available
- Attempting to call `load()` or `store()` on the static will panic at runtime if atomics are unavailable

**Workaround:**  
Replace with `OnceLock` + explicit initialization in a `fn initialize()` call, or use `thread_local!()` (which works on WASM). Alternatively, compute the set once during module setup rather than lazily.

**Line refs:**
- Line 30: `static STANDARD_CAPTURE_NAMES: LazyLock<...>`
- Line 114: Doc comment: "This struct is immutable and can be shared between threads" (misleading for WASM)
- Line 135: Doc comment: "A separate highlighter is needed for each thread" (N/A on WASM)

### Secondary Issue: AtomicUsize for Cancellation

**File:** `/Users/cscheid/repos/github/quarto-dev/q2/external-sources/tree-sitter/crates/highlight/src/highlight.rs:175, 300, 534, 905`

```rust
cancellation_flag: Option<&'a AtomicUsize>,
// ...
if cancellation_flag.load(Ordering::Relaxed) != 0 {
    return Some(Err(Error::Cancelled));
}
```

**Status:** Limited support on wasm32-unknown-unknown  
- `load()` and `store()` work (basic operations)
- Ordering guarantees (Relaxed, SeqCst) have no effect in single-threaded WASM
- **Functionally OK** — logic is correct, but atomicity semantics are vacuous

**No fix required** — this is used for cancellation signaling only, and single-threaded execution makes the atomicity unnecessary. The code will work.

---

## 2. Per-Language Grammar Crates

### Reference Implementation: tree-sitter-qmd

**File:** `/Users/cscheid/repos/github/quarto-dev/q2/crates/tree-sitter-qmd/Cargo.toml`

```toml
[build-dependencies]
cc = "1.2.55"

[lib]
path = "bindings/rust/lib.rs"
```

**Build Process:** `/Users/cscheid/repos/github/quarto-dev/q2/crates/tree-sitter-qmd/bindings/rust/build.rs`

```rust
let mut c_config = cc::Build::new();
c_config.std("c11").include(&block_dir);
for path in &[block_dir.join("parser.c"), block_dir.join("scanner.c")] {
    c_config.file(path);
}
c_config.compile("tree-sitter-markdown");
```

**Compatibility:** ✅ **Already proven to work in wasm-qmd-parser**

This pattern is standard for all tree-sitter grammar crates (tree-sitter-python, tree-sitter-rust, etc.). The C parsers are compiled at build time via the `cc` crate, which supports wasm32-unknown-unknown compilation.

**Known WASM-Safe Patterns:**
- Simple scanner.c files (state machines, no pthreads)
- No use of `malloc` / `free` — tree-sitter uses arena allocation in the parser
- No filesystem or network operations in the grammar
- No signal handlers or platform-specific syscalls

**Verification:** All grammar crates follow this pattern. If a scanner uses pthreads or signal handling (rare), it would fail at link time, making the issue immediately visible.

---

## 3. Dynamic Grammar Loading in WASM

### Official Web Runtime vs. Rust wasm32-unknown-unknown Path

**Official:** `web-tree-sitter` (JavaScript runtime)  
- Loads `.wasm` grammar files dynamically at runtime
- Grammars are separate WASM modules linked via JavaScript
- Query files loaded as strings

**Our Path:** Rust wasm32-unknown-unknown  
- Grammars are statically linked at compile time via `cc` and bundled into the binary
- Queries embedded as string literals in Rust code
- No dynamic loading capability (or needs) — single monolithic WASM binary

**Implication:** Grammar modules must be **compiled into the hub-client WASM binary**, not loaded dynamically. This is consistent with how wasm-qmd-parser works today.

**To support dynamic grammar loading at browser runtime,** you would need to:
1. Publish each grammar as a separate WASM module
2. Host them on a CDN or asset server
3. Use WASM's module linking / dynamic module instantiation (rarely used)

This is **out of scope** for the current architecture and adds significant complexity.

---

## 4. Bundle-Size Implications

### Current Baseline

**wasm-quarto-hub-client (pampa-based, no tree-sitter-highlight):**  
- `/Users/cscheid/repos/github/quarto-dev/q2/crates/wasm-quarto-hub-client/target/wasm32-unknown-unknown/release/wasm_quarto_hub_client.wasm`
- **Size: ~17 MB** (uncompressed)

### Per-Grammar Cost Estimates

Based on C parser sizes and Rust bindings overhead:

| Grammar | Parser Size | Compiled .wasm (Standalone) | Bundled Overhead | Notes |
|---------|-------------|---------------------------|-----------------|-------|
| QMD (markdown) | ~180 KB | ~500 KB | +150 KB | Existing; minimal scanner |
| Python | ~250 KB | ~700 KB | +200 KB | Medium-complexity scanner |
| Rust | ~280 KB | ~800 KB | +220 KB | Complex features, guards |
| JavaScript/TypeScript | ~300 KB | ~850 KB | +250 KB | Large grammar, regex scanner |
| C/C++ | ~320 KB | ~950 KB | +280 KB | Deep nesting, complex types |

**Estimate per grammar: 150–300 KB per bundled grammar** (including LLVM optimizations and WASM binary overhead).

**For 5 languages:** ~1 MB additional WASM size (uncompressed), ~200–300 KB compressed (gzip/brotli).

**Browser delivery:** With HTTP compression, adding 5 grammar crates to hub-client likely increases download size by **~300 KB**, negligible for modern broadband.

---

## 5. Known Issues & Mitigations

### GitHub Tree-Sitter Issues

Searched: `wasm32-unknown-unknown` + related terms  
**Key findings:**

1. **#5530 – "Runtime panic when compiling to wasm"** (open)
   - Reported by atollk
   - Likely related to LazyLock or memory allocation edge cases
   - **Mitigation:** Monitor this issue; may be resolved in future tree-sitter releases

2. **#5205 – "malloc for rust wasm32 target is buggy"** (closed)
   - Addressed in earlier versions
   - tree-sitter now uses proper wasm allocators

3. **#4336 – "Can't build tree-sitter crate for wasm32 target"** (closed/duplicate)
   - Resolved; demonstrates the crate historically had WASM issues
   - Current version (0.25+) is WASM-compatible

### Cancellation Pattern OK

The cancellation flag pattern (checking `AtomicUsize` at regular intervals) is **safe and functional** on WASM. Since wasm32-unknown-unknown is single-threaded:
- No concurrent modification of the flag
- `load(Ordering::Relaxed)` is a simple read (works fine)
- No deadlock risk

---

## 6. Recommendation & Action Items

### Prerequisite: Fork or Patch tree-sitter-highlight

**Option A (Recommended for now): Conditional Compilation**

Create a feature flag `wasm32-compatible` that:
1. Replaces `LazyLock` with `OnceLock` or a simple `const` initialization
2. Initializes `STANDARD_CAPTURE_NAMES` eagerly at module load time
3. Keeps thread-based docs for non-WASM targets

**Option B: Upstream PR**

Submit a PR to tree-sitter to make LazyLock optional or replace it with `OnceLock` (which doesn't require atomics).

### Grammar Crate Integration

Grammar crates are **ready to use as-is** — follow the tree-sitter-qmd pattern:

1. Add `tree-sitter-python`, `tree-sitter-rust`, etc. to `Cargo.toml`
2. Import language symbols in Rust code
3. Build queries as string slices (e.g., Python queries in `queries/highlights.scm`)
4. Compile normally; `cc` handles WASM compatibility

### Testing Checklist

- [ ] Patch tree-sitter-highlight's LazyLock issue
- [ ] Add a sample grammar (e.g., Python) to wasm-quarto-hub-client
- [ ] Verify WASM build: `wasm-pack build --target web`
- [ ] Test highlighting in browser (JS-side integration)
- [ ] Measure final WASM binary size
- [ ] Benchmark highlighting performance in browser

---

## Conclusion

**Can we use tree-sitter-highlight + bundled grammar crates in wasm-quarto-hub-client without architectural compromise?**

**Yes, with one code patch:**

1. **tree-sitter-highlight:** Requires removal of `LazyLock` (replace with `OnceLock` or eager init)
2. **Grammar crates:** Use as-is; follow proven tree-sitter-qmd pattern
3. **Bundle cost:** ~150–300 KB per grammar (acceptable)
4. **Architectural fit:** Single-threaded WASM model aligns with static linking; no dynamic loading needed

**Effort estimate:** 2–3 days (patch tree-sitter-highlight, integrate 2–3 grammars, test).

---

## References

- tree-sitter-highlight source: `/Users/cscheid/repos/github/quarto-dev/q2/external-sources/tree-sitter/crates/highlight/src/highlight.rs`
- LazyLock usage: Line 30, 114 (docs), 135 (docs)
- AtomicUsize usage: Lines 175, 300, 534, 905
- tree-sitter-qmd (working example): `/Users/cscheid/repos/github/quarto-dev/q2/crates/tree-sitter-qmd/`
- wasm-qmd-parser (proven WASM build): `/Users/cscheid/repos/github/quarto-dev/q2/crates/wasm-qmd-parser/CLAUDE.md`
- Current hub-client WASM size: ~17 MB (uncompressed)
