# Project `_environment` files are not loaded (bd-environment-files-372u9qbs)

**Date:** 2026-08-09
**Braid:** bd-environment-files-372u9qbs (feature, P1, label `parity`)
**Checkout:** committed on `main` in the bd-eb2wnxkp worktree (the checkout `/investigate-beads` was invoked in; no new branch created — user decides where implementation lands)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, the Q1 reference implementation is small and fully understood, and q2 has an exact structural analog (`_variables.yml` loading) to model the fix on. The open questions are genuine design choices (injection mechanism, scope of consumers, profile support), not missing information.

## Issue context

q2 never reads Quarto 1's project environment-variable files (`_environment`, `_environment.local`, `_environment.required`, `_environment-<profile>`). `EnvShortcodeHandler` resolves `{{< env NAME >}}` against `std::env::var` only, so a variable defined solely in `_environment` renders the unresolved marker `?env:NAME` plus a Q-16-5 warning per use.

Real-world hit: the Posit Connect docs port (352 pages) sets `CONNECT_VERSION` etc. in `_environment`; every `{{< env … >}}` use renders `?env:CONNECT_VERSION` site-wide unless the caller exports the variables manually.

Strand filed 2026-08-10 (yesterday, UTC) by Carlos from the connect-docs porting effort (origin strand in that skein: `br-environment-files-n0joqkug`). Fresh — no stale assumptions.

## Dependency graph

- **related (incoming):** bd-shortcodes-in-metadata-bp06aub8 (open, P1) — filed alongside this one from the same connect-docs port. Shortcodes in `website.title` / `page-footer` / include files are not expanded at all (a *different* failure: config strings never pass through `ShortcodeResolveTransform`). The two compose: even after this strand is fixed, `{{< env CONNECT_VERSION >}}` in `website.title` stays literal until that one is fixed too. Whatever mechanism this strand introduces (project env map vs. process env) must be consumable from wherever that strand ends up expanding config-string shortcodes — worth designing with one eye on it.

No blocks edges, no discovered-from in the braid skein (origin context lives in the connect-docs skein).

## What the code looks like today

Confirmed at HEAD (`8518ac79`); pre-flight `cargo xtask verify --skip-hub-build` green; symptom reproduced end-to-end (see `environment-files-loading-investigation/repro/`, rendered output shows `?env:REPRO_VERSION`).

**q2 side:**

- `crates/quarto-core/src/transforms/shortcode_resolve.rs:213` — `EnvShortcodeHandler::resolve` calls `std::env::var(&name)` directly, falls back to the optional second positional arg, else warns Q-16-5.
- `rg _environment crates/` — zero hits. Nothing in project discovery/load touches these files.
- **The structural analog:** `crates/quarto-core/src/stage/context.rs:246` + `load_project_variables` (`context.rs:761`) reads `<project>/_variables.yml` via `SystemRuntime` at `StageContext` construction (skipped when `project.is_single_file`), stores it on the context, and hands it to `ShortcodeResolveTransform::builtin_handlers(variables)` → `VarShortcodeHandler::new(variables)` (`shortcode_resolve.rs:454-458`). An `_environment` map can follow this path line-for-line.
- **Profiles don't exist in q2:** `quarto render --profile` is parsed (`crates/quarto/src/main.rs:144`) but *dropped* — the `Commands::Render` destructure at `main.rs:748` elides it with `..` and `RenderArgs` has no field for it. No `QUARTO_PROFILE` handling anywhere. So `_environment-<profile>` has no active-profile source to key off yet.
- **Engines spawn subprocesses:** `crates/quarto-core/src/engine/knitr/subprocess.rs`, `engine/jupyter/*`, and `project/render_scripts.rs` spawn real processes. Under Q1's process-env injection these inherit `_environment` values (R code doing `Sys.getenv("CONNECT_VERSION")` works). A map-only design must decide whether to pass the map at these spawn sites.

**Q1 reference** (`external-sources/quarto-cli/src/quarto-core/dotenv.ts`, called from `project-context.ts:237` during project-context creation):

- Files considered, later-wins priority: `_environment` < `_environment-<profile>` (per active profile) < `_environment.local`; a variable already present in the real process env is **never overwritten** (real env wins over all files).
- Values are injected into the process env (`Deno.env.set`), with bookkeeping to back them out on re-render and a change event that triggers preview re-renders.
- `QUARTO_PROFILE` itself may be defined in `_environment`/`_environment.local` (`dotenvQuartoProfile`) — a small bootstrapping wrinkle.
- `_environment.required` validation is **effectively vestigial** in Q1: the code carries a FIXME noting the dotenv `safe` option it relied on was removed upstream; the current call validates nothing. Q1 in practice parses the file and does not enforce it.

## Proposed phases (draft — contents wait on design answers)

- Phase 0 — Test plan (TDD): failing tests first. Unit tests for the dotenv-file parser + precedence; an end-to-end test driving the real render path (the repro fixture) asserting `from-env-file` appears in output; a precedence test (exported real env beats file value); WASM-path consideration per `.claude/rules/wasm.md`.
- Phase 1 — Dotenv file parsing + `load_project_environment` in `StageContext` (modeled on `load_project_variables`), with Q1 precedence.
- Phase 2 — Consumption by `EnvShortcodeHandler` (real env first, then project env map, then fallback arg; Q-16-5 hint updated to mention `_environment`).
- Phase 3 — (scope-dependent) propagation to engine subprocess + render-script spawn sites.
- Phase 4 — (scope-dependent) `_environment.required` behavior; profile variants.
- Phase 5 — Docs (`docs/` user-facing; render with q2, not Q1).

## Open design questions for the user

1. **Injection mechanism.** Q1 mutates the process env. In Rust that means `std::env::set_var` — `unsafe` in edition 2024 and genuinely thread-unsafe (UB if another thread reads the env concurrently, and q2 renders documents in parallel). The `_variables.yml`-style alternative — a project env map on `StageContext`, consulted by `EnvShortcodeHandler` after `std::env::var` misses — is thread-safe and WASM-friendly (loads through `SystemRuntime`, so hub-client preview gets it via the VFS for free). I'd recommend the map. Agreed, or do you want process-env injection for maximal Q1 fidelity?
2. **Which consumers, in this strand?** The map alone fixes the `env` shortcode (the filed symptom). Q1's injection also makes the variables visible to executed code (knitr/jupyter subprocesses) and pre/post render scripts. Include `.envs(project_env)` at those spawn sites now, or file it as a follow-up strand?
3. **Profile variants.** q2 has no active-profile machinery (`--profile` is parsed and dropped). Options: (a) scope `_environment-<profile>` out of this strand and file profiles separately; (b) implement a minimal `QUARTO_PROFILE` env-var + `--profile` plumbing just enough for env files. I'd lean (a) — profiles deserve their own design (they also affect `_quarto-<profile>.yml` config merging, which q2 lacks too).
4. **`_environment.required`.** Q1's validation is vestigial (broken upstream, FIXME in their source). Implement *actual* enforcement (error or warning when a required variable is undefined — arguably the useful behavior Q1 intended), or match Q1's de-facto no-op and just not choke on the file's presence?
5. **Parser.** Hand-roll a small KEY=VALUE parser (comments, blank lines, optional quotes) in-tree, or take a dependency (`dotenvy`'s parser handles quoting/escaping/multiline)? The Connect docs file is plain `KEY=value` lines; a small in-tree parser keeps the WASM build lean, but a dependency buys edge-case fidelity with Q1's dotenv dialect.
6. **Single-file mode.** `_variables.yml` is project-scoped in q2 (`is_single_file` → skipped), matching Q1. Same for `_environment`? (Q1 loads it during *project*-context creation, so project-scoped matches.)
7. **Preview reactivity.** Q1 watches env files and re-renders on change. In scope here, or follow-up?

## Risks / tradeoffs (draft)

- **Coordinate with bd-shortcodes-in-metadata-bp06aub8.** If config-string shortcode expansion lands with a different context type, the env source needs to be reachable from there too. Designing the env map as project-load data (not something buried in `StageContext` construction) keeps both consumers possible.
- **WASM leg.** `shortcode_resolve.rs` runs in `wasm-quarto-hub-client`; anything touching it needs full `cargo xtask verify` (not `--skip-hub-build`) before commit. `std::env` in wasm32 is a per-instance stub — another reason to prefer the map.
- **Per-document reload.** `load_project_variables` re-reads `_variables.yml` per `StageContext` (per document). Copying that pattern re-reads `_environment` per document too — fine for correctness (files are tiny), noted in case profiling objects on 352-page sites.
- **Precedence subtlety.** "Real env wins" must be evaluated per-variable at *resolve* time (`std::env::var` first, then map), not by snapshotting the env at load time, or `-M`-style overrides and test isolation get weird.
