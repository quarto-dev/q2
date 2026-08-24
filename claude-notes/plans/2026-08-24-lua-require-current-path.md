# Lua require + "current path" contract: GH #587, GH #588, shortcode stack leak

**Strands:**
- bd-sr0nipl7 — GH #588: `resolve_path` returns module dir inside a required file (primary)
- bd-9uqdoy0e — GH #587: `require` unavailable in Lua filters (blocked-by bd-sr0nipl7)
- bd-9xa0yui7 — shortcode load-time `push_script_dir` never popped

**Design/assessment doc:** `claude-notes/research/2026-08-24-lua-current-path-gh588.md`
(read it first — it holds the full Q1/Q2 mechanism inventory and the rationale
for the chosen approach).

## Overview

All three bugs live in the same subsystem — pampa's Lua script-dir machinery
(`crates/pampa/src/lua/{quarto_api.rs,filter.rs,shortcode.rs,dofile_wasm.rs}`)
— and are fixed under one contract, so they ship as one coherent PR:

> The script-dir stack moves only at top-level script boundaries: filter
> script load, shortcode script load, and shortcode handler invocation.
> Loaders — `require`, `dofile`, `loadfile` — never move it. A loader that
> needs the location of the file it is currently executing tracks that
> privately.

Chosen approach (avenue A of the research doc, confirmed with Carlos):
**split the stacks.** `_quarto_script_dir_stack` becomes the exact analogue
of Q1's `scriptFile` stack; `register_scoped_require` gets a private
module-dir stack that only its own candidate walk consults (module stack
top-down, then script stack top-down — byte-identical search order to today,
so #450's shipped require behavior is unchanged).

Phase order matters: #588 first, so that when #587 installs the scoped
require into filter environments, filters get the *corrected* require and
never inherit the resolve_path breakage.

## Phase 1 — GH #588: split the stacks (bd-sr0nipl7)

Tests first:

- [x] Unit test (quarto_api.rs tests): a module loaded via the scoped
      require calls `quarto.utils.resolve_path("_modules/x.lua")` at module
      load time; assert it resolves against the *extension root* (script
      stack top), not the module's dir. Run, verify it fails with the
      doubled `_modules/_modules/` segment. *(Verified failing with exactly
      the doubled segment before the fix:
      `test_resolve_path_inside_required_module_uses_script_root`.)*
- [x] Unit test: nested-require candidate order is preserved — a module can
      still require a sibling by bare name (today's #450 behavior). Should
      pass before AND after (regression guard). *(Two guards:
      `test_require_bare_sibling_name_from_nested_module`,
      `test_require_root_relative_name_from_nested_module`, plus
      `test_script_dir_stack_unchanged_across_require`.)*
- [x] Smoke-all fixture `extensions/contract-resolve-path/` mirroring
      mcanouil's #588 repro: shortcode extension whose top-level script
      computes `top` / `via_require` / `via_dofile` (the issue's three-row
      table) and emits them; `ensureFileRegexMatches` asserts all three are
      the same root-resolved path. Verify it fails. *(Verified failing:
      `rp-require=DIFF` before the fix, dofile row SAME as expected.)*

Implementation:

- [x] In `register_scoped_require`: push/pop the module's dir on a **private**
      stack (registry-held table or Rust-side state in the closure), not on
      `_quarto_script_dir_stack`. Candidate walk = private stack top-down,
      then script-dir stack top-down, deduped — same effective order as
      today. Keep the existing pop-before-`?` discipline on the error path.
      *(Implemented as a named-registry-value table,
      `REQUIRE_DIR_STACK_KEY` — invisible to user Lua code.)*
- [x] All Phase-1 tests pass; existing `contract-require` fixture and the
      #450 contract corpus (`crates/quarto/tests/smoke-all/extensions/contract-*`)
      stay green. *(Full pampa suite 4600 passed; SMOKE_FILTER=contract all
      green.)*

## Phase 2 — GH #587: scoped require in filter environments (bd-9uqdoy0e)

Tests first:

- [x] Smoke-all fixture `extensions/contract-filter-require/` mirroring
      mcanouil's #587 repro: extension contributes a Lua *filter* whose
      top-level script does `require("_modules/greet")`. Verify it fails
      today with the stock-Lua "module not found" error. *(Verified failing:
      `[C]: in function 'require'` at fr.lua:7 before the fix.)*
- [x] Same fixture (or a sibling) also exercises the absolute form
      extensions use: `require(quarto.utils.resolve_path("_modules/greet.lua"):gsub("%.lua$", ""))`.
      (The scoped require handles this by accident of `PathBuf::join`
      semantics with a rooted argument — pin it with a test so it stays.)
      *(`fr-abs=OK` row; asserts on module contents, not table identity,
      to stay robust to key-format differences across platforms.)*
- [x] Fixture asserts resolve_path parity *inside the filter-required
      module* too — the #588 contract must hold on the filter path, not
      just the shortcode path. *(`fr-resolve=OK` row.)*

Implementation:

- [x] Call `register_scoped_require(&lua, runtime.clone())` in
      `create_filter_environment` (`filter.rs`) — the runtime `Arc` is
      already a parameter. On native, the captured original `require`
      remains the fallback (filters get the full stdlib); on WASM there is
      no `package` lib, matching the shortcode state today.
      *(`create_filter_environment` is the single filter-state constructor;
      `apply_lua_filter` reaches it at filter.rs:256.)*
- [x] Note (doc-only, acceptable divergence): filter states are per-filter,
      so the require cache is per-filter-state; Q1's single emulated state
      shares one module cache. No action, record it in the research doc.

## Phase 3 — shortcode load-push leak + contract comment (bd-9xa0yui7)

Tests first:

- [x] Unit test (shortcode.rs tests): load two shortcode scripts into one
      registry; assert the script-dir stack depth returns to its baseline
      after each load (fails today — one leaked entry per script).
      *(Two tests, both verified failing before the fix:
      `test_load_script_restores_script_dir_stack` and
      `..._on_error` for the eval-error exit path.)*

Implementation:

- [x] Pop the load-time push in `load_script` after script evaluation —
      on **all** exit paths (the current code has `?`-early-returns between
      push and end; use a guard or restructure so the pop always runs).
      Handler registration bookkeeping (`handler_script_dirs`) is
      unaffected; call-time push/pop already covers handler execution.
      *(Added `ScriptDirGuard` / `push_script_dir_scoped` (RAII, owns a
      cloned `Lua` handle) in quarto_api.rs; `load_script` holds the guard
      for the whole load, and the call-time push/pop in `call` was
      converted to the same guard for panic-safety.)*
- [x] Write the contract statement (Overview above) as the header comment
      of the script-dir-stack section in `quarto_api.rs`; point
      `dofile_wasm.rs`'s header at it instead of restating it.

## Phase 4 — verification and wrap-up

- [x] WASM smoke coverage per `.claude/rules/wasm.md`: add a
      `crates/pampa/tests/wasm_lua.rs` case exercising require-then-
      resolve_path under the `/project/` VFS prefix (shortcode and filter
      environments both instantiate the scoped require there).
      *(`scoped_require_and_resolve_path_wasm`; ran locally on the real
      wasm32-unknown-unknown target — needs homebrew LLVM clang, Apple
      clang rejects `-fwasm-exceptions` — 8 passed, 0 failed.)*
- [x] `cargo build --workspace` && `cargo nextest run --workspace`.
      *(Green at every phase boundary; final counts 13258 passed.)*
- [x] Full `cargo xtask verify` (pampa is in wasm-quarto-hub-client's
      closure — the hub/WASM leg is affected; `--skip-hub-build` is not
      enough for the final gate). *(Passed clean 2026-08-24. First run
      failed on preview-renderer's KaTeX `.katex-tag` assertion — stale
      local node_modules (katex 0.17 installed vs 0.18.1 in the lockfile),
      pure-TS test, unrelated to this branch; fixed by `npm install` from
      the repo root. Incidental lockfile `peer:` churn reverted.)*
- [x] End-to-end per repo policy: `cargo run --bin q2 -- render` on the two
      new fixtures; inspect the emitted HTML; record invocation + output
      snippet here. *(Verified 2026-08-24, output inspected:*

      ```
      $ cargo run --bin q2 -- render crates/quarto/tests/smoke-all/extensions/contract-resolve-path/test.qmd
      $ grep -o "rp-top=[^<]*" .../contract-resolve-path/test.html
      rp-top=OK;rp-require=SAME;rp-dofile=SAME

      $ cargo run --bin q2 -- render crates/quarto/tests/smoke-all/extensions/contract-filter-require/test.qmd
      $ grep -o "fr-require=[^<]*" .../contract-filter-require/test.html
      fr-require=OK;fr-abs=OK;fr-resolve=OK
      ```

      *All three #588 rows agree through the real render path, and the
      #587 filter loads its module via both require forms.)*
- [x] Strand bookkeeping: bd-sr0nipl7, bd-9uqdoy0e, bd-9xa0yui7 moved to
      `in_review` (close at PR merge, matching repo practice — e.g.
      bd-8b0af414). GH #587/#588 comment wording drafted for Carlos in the
      session summary; posting awaits his approval, as does pushing the
      branch / opening the PR.

## Design decisions already settled

- Avenue A (split stacks) over Q1-literal (B) and tagged entries (C) —
  rationale in the research doc; confirmed by Carlos 2026-08-24.
- Keep Q2's bare-name sibling require (superset of Q1) and module-first
  candidate order; document the theoretical shadowing divergence
  (`<root>/util.lua` vs `<root>/_modules/util.lua`) rather than chase it.
- Q2 innermost-first vs Q1 outermost-first multi-script search order:
  document, don't chase.
- No `debug`-lib dependency anywhere (unavailable on WASM); all
  calling-file tracking is Rust-side.
