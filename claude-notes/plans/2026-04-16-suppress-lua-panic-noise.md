# Suppress noisy `lua error` panic stack traces in WASM

## Problem

`cargo xtask verify` passes, but the test output (and hub-client production
console) contains many noisy stack traces of the form:

```
panicked at src/c_shim.rs:452:5:
lua error

Stack:

Error:
    at __wbg_new_8a6f238a6ece86ea (...wasm_quarto_hub_client.js:1091:25)
    at <console_error_panic_hook[…]::Error>::new::__wbg_new_…
    at console_error_panic_hook[…]::hook
    ...
    at rust_lua_throw (wasm://wasm/...)
```

These appear on `cargo xtask verify` runs and on hub-client deployments
(visible to users in the browser console).

## Root cause (NOT a bug — expected control flow)

The Lua interpreter is compiled for `wasm32-unknown-unknown`, where
`setjmp`/`longjmp` are not available. As designed
(`claude-notes/designs/lua-wasm.md`), Lua's error mechanism is rewired:

| Lua macro | Native default | WASM replacement |
|---|---|---|
| `LUAI_THROW(L,c)` | `longjmp((c)->b, 1)` | `rust_lua_throw()` → `panic!("lua error")` |
| `LUAI_TRY(L,c,a)` | `if (setjmp(...) == 0) { a }` | `rust_lua_protected_call(f, L, ud)` → `catch_unwind` |

So **every** Lua error (including `pcall`-caught user errors, expected
runtime errors during filter execution, normal Lua control flow) raises a
Rust panic, which is then caught at the `LUAI_TRY` boundary by
`catch_unwind` in `rust_lua_protected_call`
(`crates/wasm-quarto-hub-client/src/c_shim.rs:451-467`).

The panics are **caught and handled correctly** — the design works. The
problem is purely cosmetic: `console_error_panic_hook::set_once()` in
`crates/wasm-quarto-hub-client/src/lib.rs:97` installs a global panic hook
that prints the full stack trace for *every* panic to `console.error`,
including the expected `lua error` panics that get caught microseconds later.

The hub-client e2e test helper already knows about this and explicitly
filters out "lua error" panics as transient/non-fatal
(`hub-client/e2e/helpers/previewExtraction.ts:38-45`):

```typescript
// Lua panics ("panicked at ... lua error") are transient — they happen
// when extension files haven't synced yet, and the app retries on re-render.
```

That comment is *partially* wrong — they're not just from extension sync
timing; they're from any Lua error, including ones that successfully
propagate through `pcall` handlers in user filters.

## Goals

1. **Stop spamming `console.error` with stack traces for `rust_lua_throw`
   panics** that are caught by `rust_lua_protected_call` (this is normal Lua
   control flow, not an error condition).
2. **Preserve panic hook behavior for genuinely unexpected panics** (e.g.,
   Rust bugs, unwrap failures in our own code) — those should still print
   useful stack traces.
3. **Don't change the Lua semantics** — errors must still propagate, be
   catchable by `pcall`/`xpcall`, and surface to users with a real message
   when uncaught.

## Non-goals (separate work)

- **Better error messages**: `claude-notes/designs/lua-wasm.md:355-357`
  notes that the actual Lua error message should be preserved and surfaced
  when an error escapes `pcall`. That is a *separate* (and complementary)
  improvement — file as a follow-up issue under the same epic if not already
  tracked.
- **Avoiding panics entirely**: Replacing the panic-based unwind with
  another mechanism (e.g., Wasm exception handling) is much larger scope
  and tracked elsewhere (`bd-gk74`).

## Design options

### Option A — Suppress via custom panic hook (recommended)

Replace `console_error_panic_hook::set_once()` with a wrapper that delegates
to the underlying hook for "real" panics but silently swallows panics whose
message is exactly `"lua error"` (or originates from `c_shim.rs:452`).

```rust
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Lua's LUAI_THROW is implemented as panic!("lua error") and is
        // always caught by rust_lua_protected_call. Skip the noisy
        // console.error stack trace for these expected control-flow panics.
        if let Some(s) = info.payload().downcast_ref::<&'static str>() {
            if *s == "lua error" {
                return;
            }
        }
        default_hook(info);
    }));
    // Install console_error_panic_hook so default_hook is the rich one.
    // Order matters: set_once first, then wrap.
}
```

**Caveats**:
- Order of installation must put `console_error_panic_hook` first, then
  wrap it via `take_hook`.
- Must match against `&'static str` payload — `panic!("lua error")` with a
  literal produces `&'static str`, not `String`.
- The wrapper still allows the unwind to proceed; it only suppresses the
  hook's *side effect* (printing).

**Pros**: minimal, surgical, no semantic change, works in both test and
production builds.

**Cons**: the suppression is by message string match, which is fragile if
someone changes the panic message. Mitigation: pull the literal into a
shared `pub const LUA_PANIC_MSG: &str = "lua error";` constant and use it
both at the panic site and the filter.

### Option B — Use a sentinel payload type

Define a unit struct `LuaThrow;` and `panic_any(LuaThrow)` from
`rust_lua_throw`. Match on `info.payload().downcast_ref::<LuaThrow>()` in
the hook.

**Pros**: type-safe; immune to message changes.

**Cons**: `panic_any` requires the panic payload type to be `'static +
Send`, which `LuaThrow` is. Slight added complexity. Worth it for
robustness.

### Option C — Don't install `console_error_panic_hook` at all

Remove the hook in production builds; rely on Lua's own error message
surface for user-visible errors.

**Pros**: zero noise.

**Cons**: throws away useful debugging for *real* panics (Rust bugs).
Strongly not recommended.

### Recommendation

**Option B** (sentinel type) is the most robust. **Option A** is simpler
and acceptable if we prefer minimal change. Decide during implementation.

## Test strategy

**Decision (2026-04-16):** Option B (sentinel payload type) chosen.

### Why not native unit tests?

`wasm-quarto-hub-client` is its own workspace (not in the main workspace)
and `crate-type = ["cdylib"]`. It does not build natively (tried
`cargo check` from its directory — fails with type errors in WASM-gated
code paths that are compiled only for wasm32). `cargo nextest run
--workspace` therefore never touches this crate.

Options considered for native unit coverage:
- Extract `LuaThrow` sentinel to a new workspace crate
  (`quarto-lua-panic-wasm`): over-engineered for ~15 lines of code.
- Add `rlib` to crate-type and fix the native build: unrelated large
  refactor.
- Use `wasm-bindgen-test` harness: this crate has zero existing tests
  and no harness setup; nontrivial to introduce.

None of these buy much because the behavior being tested
(`panic_any(T)` → `catch_unwind` → `downcast_ref::<T>()`) is
std-library functionality with obvious correctness. What we actually
need to verify is the **end-to-end console behavior** inside a real
WASM runtime.

### Tests (TDD)

Tests must come *before* implementation.

### Phase 1 — Tests

- [x] **Failing integration test** (Node.js):
      `crates/wasm-quarto-hub-client/test-panic-suppression.mjs`
      1. Installs a `console.error` capture before `mod.default(...)`.
      2. Runs four `pcall`-caught error scripts (plain `error`, structured
         error object, nil-access, three-in-a-row).
      3. Asserts captured errors contain NO text matching
         `/panicked at.*c_shim\.rs/`, `/^lua error$/m`, or
         `/rust_lua_throw/`.
      4. Part 2: calls `test_unwind()` (which uses `panic!("test panic")`)
         and asserts that `"test panic"` IS captured — guards against the
         filter becoming too broad.
- [x] **Failing test run**: ran before implementation — captured 10
      console.error calls, all containing `panicked at src/c_shim.rs:452:5:
      lua error`, confirming bug reproduction.

### Phase 2 — Implementation

- [x] Define `pub struct LuaThrow;` sentinel type at the crate root
      (`crates/wasm-quarto-hub-client/src/lib.rs`) so it is always
      compiled regardless of target. A unit struct is trivially `'static
      + Send`, satisfying `panic_any`'s requirements.
- [x] Replace `panic!("lua error")` in
      `crates/wasm-quarto-hub-client/src/c_shim.rs::rust_lua_throw` with
      `std::panic::panic_any(crate::LuaThrow)`. `panic_any` produces a
      standard Rust panic and unwinds through `extern "C-unwind"` frames
      identically.
- [x] In `crates/wasm-quarto-hub-client/src/lib.rs::init()`:
      1. Call `console_error_panic_hook::set_once()` (installs rich hook).
      2. Call `std::panic::take_hook()` to capture it.
      3. Install a wrapper hook that short-circuits when
         `info.payload().downcast_ref::<LuaThrow>()` is `Some`, otherwise
         delegates to the captured hook.

### Phase 3 — Verification

- [x] Rebuilt WASM; Phase 1 test now passes (part 1: 0 captures with
      Lua-panic noise; part 2: `"test panic"` still captured → 1 call).
- [x] Ran `cargo xtask verify` — exit 0, all steps passed. `grep -c "lua
      error"` on full output: 0. `grep -c "c_shim.rs"`: 0. No panic
      traces in output.
- [x] Existing `test-lua-wasm.mjs` (which includes a `pcall error` case
      that previously produced noise) now passes silently — 10/10 tests
      still pass; only `test_unwind`'s deliberate `panic!("test panic")`
      surfaces (expected).
- [x] Updated `hub-client/e2e/helpers/previewExtraction.ts` comment to
      reflect new behavior. Kept the `unreachable`/`RuntimeError`
      fatal-filter as a defensive safety net for genuinely fatal WASM
      traps.

## Outcome

Shipped in-branch. Follow-ups (not blocking):

- Improve Lua error-message propagation when `pcall` is not present —
  `rust_lua_throw` currently carries no Lua error message through the
  unwind, so uncaught errors surface to users with a generic panic.
  Tracked separately under the error-reporting work referenced in
  `claude-notes/designs/lua-wasm.md:355-357`.
- If future WASM components (e.g., `wasm-qmd-parser` if it ever grows a
  Lua VM) need the same treatment, extract `LuaThrow` + the hook wrapper
  into a tiny shared crate at that point.

## Files involved

- `crates/wasm-quarto-hub-client/src/lib.rs:94-98` — `init()` panic hook
  installation site.
- `crates/wasm-quarto-hub-client/src/c_shim.rs:448-467` — `rust_lua_throw`
  / `rust_lua_protected_call` definitions.
- `crates/wasm-qmd-parser/src/c_shim.rs` — same shim (check for parity;
  this crate also has a `c_shim.rs` and may have the same panic).
- `crates/lua-src-wasm/lua-5.4.8/luaconf_wasm.h:23-25` — macro override
  that wires Lua's `LUAI_THROW` to `rust_lua_throw`.
- `hub-client/e2e/helpers/previewExtraction.ts:38-45` — existing tolerance
  for these panics; comment may need updating after fix.
- `claude-notes/designs/lua-wasm.md:355-357` — design doc note about error
  message surfacing (related but separate work).

## Cross-crate scoping

`crates/wasm-qmd-parser/src/c_shim.rs` also exists and may define the same
`rust_lua_throw`. If so, the fix applies there too. Verify during Phase 1.

## Risks

- **Hiding real Lua bugs**: If a Lua error escapes `pcall` and there's no
  user-facing surface, suppressing the hook output could make debugging
  harder. Mitigation: ensure uncaught Lua errors are surfaced via the
  `quarto-error-reporting` path (already partially in place; verify).
- **Panic hook ordering**: If something else in the wasm-bindgen stack
  installs a hook after ours, our wrapper is bypassed. Verify hook is
  installed last in `#[wasm_bindgen(start)]`.

## Out of scope

- Switching from `panic!`-based unwind to native Wasm exception handling
  (`bd-gk74`).
- Rewriting Lua error message propagation
  (`claude-notes/designs/lua-wasm.md:355-357`).
