# Research: libc Implementations for wasm32-unknown-unknown

**Date:** 2026-03-16
**Purpose:** Investigate existing Rust crates and projects that provide C standard library implementations for wasm32-unknown-unknown targets, to avoid hand-implementing C functions for Lua.

## Summary

There are several options available, but **none provide a complete, drop-in libc replacement**. Most are minimal implementations focused on specific use cases:

### Best Options for Lua

1. **libm** (math functions) - ✅ Production-ready, comprehensive
2. **tinyrlibc** (string/stdlib functions) - ⚠️ Incomplete but usable
3. **compiler_builtins** (mem functions) - ✅ Production-ready

### Not Suitable

- **rlibc** - Deprecated, superseded by compiler_builtins
- **relibc** - Only supports Redox OS and Linux, not wasm32-unknown-unknown
- **wasi-libc** - Requires WASI system calls, won't work with wasm32-unknown-unknown

---

## Detailed Findings

### 1. libm - Math Functions ✅ RECOMMENDED

**Crates.io:** https://crates.io/crates/libm
**Repository:** https://github.com/rust-lang/compiler-builtins (merged into compiler-builtins)
**Version:** 0.2.16
**Status:** Production-ready, widely used

**Description:**
- Pure Rust port of MUSL's libm
- Designed specifically for `no_std` environments including `wasm32-unknown-unknown`
- Part of the official Rust compiler toolchain ecosystem

**What it provides:**
Complete C math library implementation including:
- Trigonometric: sin, cos, tan, asin, acos, atan, atan2
- Hyperbolic: sinh, cosh, tanh, asinh, acosh, atanh
- Exponential/logarithmic: exp, exp2, exp10, expm1, log, log2, log10, log1p
- Power: pow, sqrt, cbrt, hypot
- Rounding: ceil, floor, round, trunc, rint, roundeven
- Special: erf, erfc, tgamma, lgamma, j0, j1, jn, y0, y1, yn
- Floating-point manipulation: frexp, ldexp, modf, scalbn, copysign, nextafter
- Classification: fabs, fmin, fmax, fmod, remainder, remquo, fdim, fma
- Both f32 and f64 variants for all functions

**For Lua:** Covers all math functions Lua needs (sin, cos, sqrt, exp, log, pow, etc.)

---

### 2. tinyrlibc - String/stdlib Functions ⚠️ USABLE BUT INCOMPLETE

**Crates.io:** https://crates.io/crates/tinyrlibc
**Repository:** https://github.com/rust-embedded-community/tinyrlibc
**Version:** 0.5.1
**Status:** Active, designed for bare-metal embedded

**Description:**
- Tiny libc implementation mostly written in Rust
- Originally created for Nordic nRF9160 SDK
- Each function in its own file with its own license
- Features are optional (compile-time controlled)

**What it provides:**

String functions:
- strcmp, strncmp, strncasecmp
- strcat, strcpy, strncpy
- strlen, strchr, strrchr, strstr
- memchr

Character classification:
- isspace, isdigit, isalpha, isupper

Conversion:
- atoi, strtol, strtoll, strtoul, strtoull, strtoimax, strtoumax

Formatting:
- snprintf, vsnprintf (via C code)

Utilities:
- abs
- qsort
- rand, rand_r, srand (optional)

Memory allocation (optional, requires `alloc` feature):
- malloc, calloc, realloc, free

Signal handling (optional):
- signal, raise, abort

**What it's missing:**
- No memcpy, memset, memmove (use compiler_builtins instead)
- No printf family (only snprintf/vsnprintf)
- Limited locale support
- No file I/O (expected)
- No strtod (critical for Lua!)

**For Lua:** Covers most string operations but **missing strtod** which Lua needs for parsing numbers.

---

### 3. compiler_builtins - Memory Functions ✅ RECOMMENDED

**Crates.io:** https://crates.io/crates/compiler_builtins
**Repository:** https://github.com/rust-lang/compiler-builtins
**Version:** 0.1.160
**Status:** Official Rust compiler infrastructure

**Description:**
- Compiler intrinsics used by the Rust compiler
- Now includes libm (merged)
- Provides optimized implementations of basic memory operations

**What it provides:**
With the `mem` feature enabled:
- memcpy, memmove, memset, memcmp
- Optimized for various architectures including WASM

**For Lua:** Essential for basic memory operations. This is what Rust itself uses.

---

### 4. rlibc ❌ DEPRECATED

**Crates.io:** https://crates.io/crates/rlibc
**Repository:** https://github.com/rust-lang/rlibc
**Version:** 1.0.0
**Status:** Deprecated

**Description:**
- Originally provided memcpy/memmove/memset for bare-metal
- Now superseded by compiler_builtins with `mem` feature
- No longer maintained

**Verdict:** Don't use, use compiler_builtins instead.

---

### 5. relibc ❌ NOT COMPATIBLE

**Repository:** https://gitlab.redox-os.org/redox-os/relibc
**Crates.io:** Not published (not found)
**Status:** Active, but platform-specific

**Description:**
- Redox OS's full libc implementation in Rust
- Supports Redox OS and Linux via system calls
- Comprehensive POSIX compatibility layer

**Platforms supported:**
- Redox OS
- Linux (via `sc` crate for syscalls)
- x86_64, aarch64, i586, riscv64gc

**Why not suitable:**
- Requires OS syscalls (fork, exec, etc.)
- Not designed for bare-metal or wasm32-unknown-unknown
- Would need extensive modifications to remove OS dependencies

---

### 6. wasi-libc ❌ WASI ONLY

**Repository:** https://github.com/WebAssembly/wasi-libc
**Status:** Official WASI project, stable

**Description:**
- Full libc built on WASI system calls
- Provides comprehensive POSIX-compatible APIs
- Used by wasi-sdk

**Why not suitable:**
- Requires WASI system interface (file I/O, environment, etc.)
- Target is `wasm32-wasi` not `wasm32-unknown-unknown`
- Cannot be used without WASI host providing system calls

---

### 7. wasm32-unknown-unknown-openbsd-libc ⚠️ INTERESTING BUT LIMITED

**Crates.io:** https://crates.io/crates/wasm32-unknown-unknown-openbsd-libc
**Repository:** https://github.com/trevyn/wasm32-unknown-unknown-openbsd-libc
**Version:** 0.2.0
**Downloads:** 6,528

**Description:**
- Subset of OpenBSD's libc packaged as a Rust crate
- Specifically designed for wasm32-unknown-unknown
- Used by other wasm32-unknown-unknown C++ projects

**Status:**
- Appears to be a small maintained project
- OpenBSD source is well-tested and stable
- Only includes "parts that make sense" for wasm32-unknown-unknown

**What it likely provides:**
- String functions
- Memory functions
- Basic stdlib utilities
- (Specific function list not readily available without deeper inspection)

**For Lua:** Could be useful but scope unclear. Worth investigating if tinyrlibc proves insufficient.

---

### 8. PigWasmStdLib ⚠️ MINIMAL

**Repository:** https://github.com/PiggybankStudios/PigWasmStdLib
**Stars:** 3
**Status:** Small personal project

**Description:**
- Very minimal C stdlib implementation for WebAssembly
- Based on musl libc and stb_sprintf
- Designed for use with Pig Engine

**What it provides:**
- assert.h: assert
- math.h: Full set (fmin, fmax, fabs, fmod, round, floor, ceil, trig, exp, log, pow, sqrt)
- stdio.h: vsnprintf (via stb_sprintf)
- stdlib.h: abs, malloc, calloc (stub), realloc (stub), free (stub), rand/srand, atof, qsort, exit
- string.h: memset, memcmp, memcpy, memmove, strcpy, strstr, strcmp, strncmp, strlen, wcslen

**JavaScript imports required:**
- jsStdAbort, jsStdAssertFailure, jsStdDebugBreak
- jsStdGrowMemory, jsStdGetHeapSize

**Limitations:**
- Requires JavaScript host functions
- Some functions are stubs (calloc, realloc, free)
- Math uses WASM builtins (good!)
- Only ~5,300 lines of code

**For Lua:** Interesting because it has `atof`, but requires JavaScript integration and has stub implementations.

---

### 9. Other Projects

**WebContainer/musl-wasm** (4 stars) - No description, unclear status
**okuoku/wasmlinux-musl** (4 stars) - WIP port of musl for WasmLinux
**maximmaxim345/wasm32-unknown-unknown-libcxx** (1 star) - LibC++ and LibC (OpenBSD-based) for Rust-C++ interop

None of these appear to be production-ready or widely adopted.

---

## Recommendations for Lua on wasm32-unknown-unknown

### Tier 1: Use These ✅

1. **libm** (from compiler_builtins or standalone)
   - For all math functions: sin, cos, exp, log, sqrt, pow, etc.
   - Production-ready, well-tested, official Rust ecosystem

2. **compiler_builtins** with `mem` feature
   - For memcpy, memmove, memset, memcmp
   - What Rust itself uses

### Tier 2: Consider These ⚠️

3. **tinyrlibc**
   - For string functions: strcmp, strlen, strcpy, etc.
   - For conversions: atoi, strtol
   - For utilities: snprintf, qsort
   - **Missing: strtod (need to implement or find alternative)**

### Tier 3: Investigate If Needed 🔍

4. **wasm32-unknown-unknown-openbsd-libc**
   - If tinyrlibc proves insufficient
   - Check if it includes strtod
   - 6,500+ downloads suggests some production use

### Must Implement Ourselves 🛠️

Functions Lua needs that aren't in the above:

1. **strtod** - Critical for parsing floating-point numbers
   - tinyrlibc doesn't have it
   - Could implement using Rust's str::parse or port from musl
   - PigWasmStdLib has `atof` but not full strtod with endptr

2. **setjmp/longjmp** - For error handling
   - Not in any pure Rust libc
   - Might need to use Lua's panic/catch_unwind strategy instead

3. **stdio functions** (if Lua uses them)
   - Most libcs don't provide FILE* operations for wasm32-unknown-unknown
   - May need to stub out or redirect to Rust I/O

---

## Implementation Strategy

### Phase 1: Use Proven Crates
```toml
[dependencies]
libm = "0.2"  # All math functions
compiler_builtins = { version = "0.1", features = ["mem"] }  # memcpy, memset, etc.
tinyrlibc = { version = "0.5", features = ["strcmp", "strlen", "strncmp", "strcpy", "atoi", "snprintf"] }
```

### Phase 2: Fill Gaps
- Implement strtod in Rust (or adapt from musl source)
- Handle setjmp/longjmp via catch_unwind
- Stub or implement any missing string functions

### Phase 3: Test & Optimize
- Verify all Lua C API calls resolve
- Run Lua test suite
- Profile performance

---

## Conclusion

**There is no single complete libc crate for wasm32-unknown-unknown**, but combining a few well-maintained crates covers most of what Lua needs:

- ✅ **libm** for math (complete)
- ✅ **compiler_builtins** for memory operations (complete)
- ⚠️ **tinyrlibc** for strings/stdlib (mostly complete, missing strtod)
- 🛠️ Custom implementation for strtod, setjmp/longjmp

This is much better than implementing everything from scratch, as we get production-tested implementations of 90%+ of needed functions.
