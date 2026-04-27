# WASM C-shim merge: unify with tree-sitter-language upstream sysroot

- **Parent plan**: `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.md` (this is a sub-plan of Phase 3.1)
- **Beads**: bd-n7x2 (overall syntax-highlighting epic)
- **Status**: **complete 2026-04-20**. All work items below merged on the `feature/quarto-2-highlighting` branch. Shim crate at `crates/tree-sitter-language-wasm-shim/`, merged snprintf/vsnprintf implementation factored into the new `crates/wasm-printf-fmt/` crate (29 unit tests). c_shim.rs reduced to a thin wasm-bindgen wrapper; `fputc`/`fputs`/`fwrite` now no-op instead of panic.

## Problem

`wasm-quarto-hub-client/src/c_shim.rs` provides Rust implementations of a set of C stdlib functions that `lua-src-wasm` and `tree-sitter-qmd` need when targeting `wasm32-unknown-unknown`. Originally this was the only source of those symbols in our WASM binary.

Modern tree-sitter grammar crates (2024+) adopted a different convention. Two of the 12 grammars we pull in during Phase 3 of syntax highlighting — `tree-sitter-lua 0.5.0` and `tree-sitter-css 0.25.0` — each have a `build.rs` that unconditionally compiles three C files from the `tree-sitter-language` crate's `wasm/src/` directory (`stdio.c`, `stdlib.c`, `string.c`) when the target is `wasm32-unknown-unknown`. These upstream C files define the same C stdlib symbols we already define in `c_shim.rs`.

`rust-lld` on `wasm32` does not accept multiple strong definitions of the same symbol. The link fails with 8 `duplicate symbol` errors:

```
rust-lld: error: duplicate symbol: snprintf
rust-lld: error: duplicate symbol: vsnprintf
rust-lld: error: duplicate symbol: fclose
rust-lld: error: duplicate symbol: fdopen
rust-lld: error: duplicate symbol: fputc
rust-lld: error: duplicate symbol: fputs
rust-lld: error: duplicate symbol: fwrite
rust-lld: error: duplicate symbol: fprintf
```

Workarounds we considered and rejected:

- **Delete our 8 functions; use only the upstream versions.** Silently loses format specifiers that Lua uses (`%lld`, `%llu`, `%g`, `%Lg`), degrading Lua runtime behavior.
- **`#[linkage = "weak"]` on our versions.** Same end result — upstream strong symbols would win for *all* callers including Lua, with the same silent regression.
- **Fork each conflicting grammar crate** (`tree-sitter-lua`, `tree-sitter-css`) to skip their stdio.c link. Multiplies maintenance burden every time those crates add or drop build steps.

## Decision

There must be exactly one set of C-stdlib shims in the binary, and it must be a superset of the behaviors both callers (Lua runtime + tree-sitter grammars) need. Ours is that superset, extended where needed.

Mechanism: `[patch.crates-io]` the `tree-sitter-language` crate to a local drop-in whose Rust API is identical to upstream but whose `wasm/src/*.c` files are **empty**. Since those C files are what `tree-sitter-lua`/`tree-sitter-css` compile, swapping them for empty files means grammars contribute no symbols for the conflicting 8 functions. Our `c_shim.rs` becomes the single source of truth.

Behavioral deltas between the two implementations, and how the merged version resolves them:

| Function | Our behavior (before) | Upstream behavior | Merged (target) |
|---|---|---|---|
| `snprintf`, `vsnprintf` | `%d %i %u %s %c %% %ld %lu %lld %llu %zu %zd`, **no flags/width/precision** | `%d %i %u %s %c %% %x %X %p %zu`, **flags/width/precision** | Union: all 15 specifiers + flags + width + precision + `%g`/`%Lg` for Lua `LUA_NUMBER_FMT` |
| `fputc`, `fputs`, `fwrite` | `panic!("not supported")` | silent no-op returning success-like value | silent no-op (upstream behavior) |
| `fclose`, `fdopen`, `fprintf` | identical no-op | identical no-op | unchanged |

Rationale for fputc/fputs/fwrite: panicking would be wrong if any code path inside tree-sitter grammars or Lua's internal error-reporting reached those functions. Silent no-op matches the rest of our stubbed IO layer (`luaopen_io` already returns a dummy, `fopen`/`freopen`/`fread` all no-op).

## Work items

### Crate scaffolding

- [ ] Create `crates/tree-sitter-language-wasm-shim/` (name TBD, could also be `crates/tree-sitter-language-shim/`).
- [ ] `Cargo.toml`: same `name = "tree-sitter-language"`, same `version = "0.1.7"`, same `links = "tree-sitter-language"`, same `[lib] name = "tree_sitter_language"` and `path = "src/language.rs"`. `edition = "2021"`. Dependencies empty.
- [ ] `src/language.rs`: verbatim copy of upstream's 23-line `LanguageFn` (`#![no_std]`, `#[repr(transparent)]`). No behavioral change to the Rust API.
- [ ] `build.rs`: verbatim copy of upstream's 10-line build script. Publishes `wasm-headers` and `wasm-src` metadata pointing at our directories.
- [ ] `wasm/include/*.h`: verbatim copies of upstream's headers (`assert.h`, `ctype.h`, `endian.h`, `inttypes.h`, `stdint.h`, `stdio.h`, `stdlib.h`, `string.h`, `wctype.h`). These declare types and static-inline helpers — no link collisions.
- [ ] `wasm/src/stdio.c`: empty (`// intentionally empty — wasm-quarto-hub-client's c_shim.rs provides these symbols` plus a pointer to this plan).
- [ ] `wasm/src/stdlib.c`: same — empty placeholder with comment.
- [ ] `wasm/src/string.c`: same — empty placeholder with comment.
- [ ] `README.md`: explain why this crate exists (fork-purpose, link to this plan), how to keep it in sync with upstream.

### Wire-up

- [ ] Add `tree-sitter-language = { path = "../tree-sitter-language-wasm-shim" }` under `[patch.crates-io]` in `crates/wasm-quarto-hub-client/Cargo.toml`.
- [ ] Verify `cargo tree -p wasm-quarto-hub-client --target wasm32-unknown-unknown | grep tree-sitter-language` shows the patched path.

### c_shim.rs extensions

- [ ] Extend `snprintf`/`vsnprintf` to parse flags (`-`, `+`, ` `, `#`, `0`), width (digits), precision (`.digits`). Apply left-justify / right-justify with space or zero padding. Apply precision to string truncation (for `%s`) and to minimum digits (for integer conversions).
- [ ] Add specifiers: `%x`, `%X` (hex lowercase/uppercase), `%p` (pointer as `0x<hex>`), `%g` (shortest representation of a double: pick `%e` or `%f` based on exponent magnitude per C standard, precision default 6, trailing zeros stripped), `%Lg` (same for long double — on wasm32 `long double` is the same as `double`, so handled identically).
- [ ] Change `fputc`, `fputs`, `fwrite` from `panic!` to no-op returning success-like values. Match upstream's return conventions exactly (`fputc` returns `c`, `fputs` returns 0, `fwrite` returns `size * nmemb`).
- [ ] Add a doc comment block above the stdio section in `c_shim.rs` explaining:
  - This is the single source of truth for these symbols in the WASM binary.
  - `tree-sitter-language`'s upstream `wasm/src/*.c` files are neutralized via the patch crate.
  - Format-specifier coverage is the union of Lua's needs + tree-sitter upstream's coverage.
  - Link to this plan file for full rationale.

### Tests

- [ ] Add unit tests in `c_shim.rs`'s test module covering:
  - `%d`, `%i`, `%u`, `%ld`, `%lld`, `%zu`, `%x`, `%X`, `%p`, `%s`, `%c`, `%%` — spot-check each specifier.
  - Flag combinations: `%-10d`, `%010d`, `%+d`, `% d`, `%#x`.
  - Width and precision: `%5.2d`, `%.3s`, `%10.5s`.
  - `%g` with varying magnitudes: `snprintf` a few representative doubles and assert the output shape (exact bit patterns are fragile — assert "matches a regex" level).
  - Buffer-size truncation (given `size = 5`, a longer output truncates and still null-terminates).
- [ ] Build + link succeed for `wasm-quarto-hub-client` with `quarto-highlight` re-enabled.
- [ ] `npm run test:wasm` from hub-client passes.
- [ ] Run the hub-client's existing Lua-driven tests (if any) and confirm no regression. Key thing to verify: `string.format("%d %s", 42, "hi")` still works; `tostring(3.14)` still produces a valid-looking number.

### Documentation

- [ ] Update `wasm-quarto-hub-client/README.md` (or `build.md`) with a note on the patched `tree-sitter-language` dep and why.
- [ ] Update the Phase 3 parent plan (`2026-04-20-syntax-highlighting-phase-3.md`) to mark this sub-plan as complete when done.

## Keeping in sync with upstream

The patch crate has three surfaces that could drift:

1. **Rust API** (`src/language.rs`). Stable since 0.1.0; 23 lines. If upstream adds API, mirror it.
2. **Headers** (`wasm/include/`). Mostly static-inline helpers. If upstream adds a helper, copy it.
3. **`build.rs`**. Trivial. Only publishes metadata.

Bump the upstream version constraint if the grammar crates we depend on require a newer `tree-sitter-language`. Update our patch to match.

A `CHECK_UPSTREAM.md` at the root of the patch crate (or a `# Upstream sync` section in its README) should note the pinned upstream version and a one-line diff summary.

## Alternatives deliberately not taken

- **Migrate fully to upstream's sysroot** (delete `c_shim.rs`'s 8 stdio functions). Would require upstream to cover `%lld`, `%llu`, `%g`, `%Lg`. Not worth blocking Phase 3 on upstream PRs.
- **Separate `c_shim.rs` for the grammar subset and another for Lua**. Same link-conflict problem at a different granularity.
- **Custom allocator override**. Orthogonal to the stdio conflict; doesn't help.

## Success criteria

- `npm run build:wasm` with all 12 grammar crates linked produces a valid `.wasm` binary with no duplicate-symbol errors.
- Lua string-formatting round-trips verified by test.
- Hub-client loads and renders a `.qmd` file containing highlighted code blocks (deferred to Phase 3.3).
