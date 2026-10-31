---
title: 'pandoc.system.with_environment mutates process env across parallel renders (bd-with-environment-process-env-5folaygy)'
date: 2026-10-31
description: 'with_environment rewrites the real process environment under parallel renders; replace it with a per-Lua-state overlay that readers and spawn paths consult.'
status: active
braid:
  strand: bd-with-environment-process-env-5folaygy
---

**Branch:** `braid/bd-with-environment-process-env-5folaygy-pandocsystemwithenvironment-mutates-process-env`, in the main checkout, based on `origin/main` @ `d67792183`.

## Triage verdict

**Ready to implement.** The strand's diagnosis reproduces at HEAD, and the design decisions below are settled.

## Issue context

P1 bug, filed 2026-10-31, open until this investigation. `pandoc.system.with_environment` calls `set_var` / `remove_var` on the real process environment, while project renders run filters on parallel rayon workers. One document's call therefore replaces the environment of every concurrently rendering document, including deleting `PATH`. The user has confirmed the diagnosis.

## Dependency graph

Empty: no parents, children, or edges. Nothing else is blocked on this, so the urgency comes from the bug itself.

Unlinked but relevant: bd-irkfpip1 (merged in PR #950) moved the `env` shortcode onto the per-document snapshot. That is the same direction as this fix.

## What the code looks like today

All paths in the strand still exist. `claude-notes/plans/with-environment-process-env-investigation/repro.sh <q2>` reproduces all three symptoms with q2 0.39.0 at `d67792183`:

```
READER samples=1183996 saw_Q2_LEAK=1183433 PATH_missing=1183349
PROBE os.getenv=nil environment=set popen=set pipe=set os.getenv(HOME)=/Users/cscheid
BAD after-error leaked=19/40 PATH=true
```

Facts the design relies on, each checked in the code:

- **Mutation sites.** `crates/pampa/src/lua/system.rs:298-347` is the only non-test `set_var` / `remove_var` outside xtask. Every other hit in `crates/*/src` falls inside a `mod tests`.
- **Bad values.** A non-string value exits through `?` part-way through the set loop. The callback and the restore never run, so the variables already set stay set for the rest of the process.
- **A further leak the strand does not list.** Restore writes back `rt.env_all()`, which includes the runtime's `env_overrides`. An override such as the preview server's `--profile` therefore lands in the real process environment after any call. Removing the mutation removes this too.
- **Two sources of truth, even outside the callback.** `os.getenv` reads the host snapshot when one exists (`host_context.rs:104`). `pandoc.system.environment` always reads the live environment (`system.rs:272`), so it misses `_environment` values and the forced `QUARTO_PROFILE`.
- **Spawn paths.** `pandoc.pipe` and `pandoc.system.command` both go through `SystemRuntime::exec_command`, which has no env parameter and is implemented by many test mocks. `exec_pipe` is not called from Lua. No Lua code in pampa uses `Command::new` directly.
- **Stock `io.popen` / `os.execute`.** Native uses `Lua::new()`, so these are libc `popen` / `system` and inherit the process environment; they cannot take an overlay. `popen` returns a real `LUA_FILEHANDLE`, so an emulation would also have to satisfy `io.type`. WASM has neither function.
- **Callers.** `with_environment` has no in-tree callers outside `system.rs` and its test. The vendored filter resources do use `io.popen` / `os.execute`, but none of them inside `with_environment`.
- **Registration order.** `register_pandoc_system` runs before `register_host_context`, so shared state fits in `Lua` app data. That is the existing pattern (`ModuleRoots`, `Dialects`, `PendingDiagnostics`).
- **No yield problem.** The callback runs through a Rust-side `call`, and Lua cannot yield across that boundary, so the overlay's extent is strictly nested and a stack is enough.
- **Sibling bug, out of scope.** `with_working_directory` (`system.rs:388`) uses `set_current_dir`, which has the same process-global flaw. Filed as bd-9hzg79zn.

## Proposed phases (draft)

- **Phase 0 — Failing tests.** Two Lua states on two threads, with one call invisible to the other. Readers and supported spawn paths agree inside the callback. The environment is restored after a callback error and after a bad value. Rewrite `test_with_environment`, which asserts on `std::env::var`.
- **Phase 1 — Overlay state.** A per-state `LuaEnv` in app data, holding the base (snapshot or runtime) and a stack of replacement maps. An RAII guard pops on every exit path. Validate the whole table before pushing, so a bad value raises without side effects.
- **Phase 2 — Readers.** `os.getenv` (native and WASM) and `pandoc.system.environment` both read `LuaEnv`. Install the `os.getenv` replacement permanently, so a script that saved `local getenv = os.getenv` still sees the overlay.
- **Phase 3 — Spawning.** Add an env-taking variant to `SystemRuntime`, implemented for native and sandbox, with a default that returns an error rather than silently dropping the env. `pandoc.pipe` and `pandoc.system.command` pass the overlay only while one is active; outside a callback children inherit as they do today.
- **Phase 4 — `io.popen` / `os.execute`.** Per question 1 below.
- **Phase 5 — Cleanup.** Delete the `unsafe` blocks and stale SAFETY comments. Add WASM coverage to `crates/pampa/tests/wasm_lua.rs`. Document the behaviour where Lua API differences are recorded. Cross-check clippy for Windows.

## Design decisions (settled with the user, 2026-10-31)

1. **`io.popen` / `os.execute` inside the callback raise an error** naming `pandoc.pipe` and `pandoc.system.command`, only while an overlay is active. This is a deliberate limitation compared to Pandoc, and the price of making parallel renders safe.
2. **`pandoc.system.environment` reads the snapshot outside a callback**, the same as `os.getenv`, so it now shows `_environment` values and the forced `QUARTO_PROFILE`.
3. **`pandoc.system.xdg` ignores the overlay, and that is documented.** It only reads, and no Lua API writes XDG state. The audit for mutating analogs found two other process-global mutators, filed separately: `with_working_directory` (bd-9hzg79zn) and stock `os.setlocale`.

## Risks / tradeoffs (draft)

- **Program lookup with a replaced `PATH`.** With an explicit env, a table without `PATH` changes how `Command` resolves a bare program name, and Unix and Windows differ. This is correct under replacement semantics, but tests must not assume a bare `sh` resolves, and it needs a Windows run.
- **WASM.** `os_wasm.rs` changes trigger the `wasm_lua.rs` rule. Those tests run in Linux CI; `dev-docs/wasm.md` has the local command for macOS.
- **Runtime trait.** A default error method avoids touching the many mocks, but a new real runtime must remember to override it.

## Implementation status (2026-10-31)

Phases 0 to 5 are done. `cargo xtask verify --skip-hub-build` and the 10 WASM tests in `crates/pampa/tests/wasm_lua.rs` pass, and `repro.sh` shows all three symptoms gone.

Still needing a Windows run, because `cargo clippy --target x86_64-pc-windows-msvc` cannot cross-check `pampa` or `quarto-system-runtime` from macOS (the `aws-lc-sys` and `tree-sitter` build scripts compile C):

- `cargo clippy -p pampa -p quarto-system-runtime --all-targets -- -D warnings`
- the new `with_environment` and `exec_command_with_env` tests, since `env_clear` on Windows also removes `SystemRoot`.

Overlay lookups match names exactly, so on Windows `os.getenv("path")` inside the callback does not find a `PATH` key, unlike the case-insensitive process environment.
