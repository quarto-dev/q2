# WASM Code Rules

## Never add `test` to wasm32 cfg guards

The cfg pattern `#[cfg(any(target_arch = "wasm32", test))]` is prohibited. It forces
native tests through the WASM-restricted Lua stdlib, which fails on Windows.

Correct pattern:
```rust
#[cfg(target_arch = "wasm32")]
// WASM-specific code (restricted Lua stdlib, synthetic io/os)

#[cfg(not(target_arch = "wasm32"))]
// Native code (full Lua stdlib via Lua::new())
```

## Async traits use `#[async_trait(?Send)]`

All async traits in this project must use `#[async_trait(?Send)]`,
not the default `#[async_trait]`.

Correct pattern:
```rust
use async_trait::async_trait;

#[async_trait(?Send)]
impl PipelineStage for MyStage {
    async fn run(&self, input: PipelineData, ctx: &mut StageContext)
        -> Result<PipelineData, PipelineError> { /* ... */ }
}
```

**Why.** The `#[async_trait]` macro rewrites `async fn` on a trait into a
function returning a `Pin<Box<dyn Future + 'a>>`. The default form adds a
`+ Send` bound to that future, requiring everything captured across `await`
points to be `Send`. The `?Send` form drops that requirement.

The same trait definitions are used by both:

- **Native CLI** — could satisfy `Send`, but the codebase uses
  single-task execution; `Send` would be over-restrictive.
- **WASM (hub-client)** — `wasm32-unknown-unknown` is single-threaded;
  `Send` is meaningless there. Several captured types in WASM contexts
  (e.g. `Rc<RefCell<…>>`, JS interop handles) are not `Send` and would
  make the trait uncompilable for WASM if `Send` were required.

`?Send` is the lowest-common-denominator that lets one trait definition
serve both targets. The cost is that you cannot `tokio::spawn` such a
future onto a multi-threaded runtime — but the pipeline doesn't do that;
stages run sequentially within a single task.

If you find yourself wanting to drop `?Send`, that is a signal something
is wrong with the design of the calling context, not with the trait.
Stop and ask before changing it.

## Verify WASM tests when editing WASM code

When modifying any of these files, update `crates/pampa/tests/wasm_lua.rs`:
- `crates/pampa/src/lua/filter.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/shortcode.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/io_wasm.rs`
- `crates/pampa/src/lua/os_wasm.rs`

WASM tests can't run locally on Windows — they run in Linux CI.
See `dev-docs/wasm.md` for the local run command (Linux/macOS).
