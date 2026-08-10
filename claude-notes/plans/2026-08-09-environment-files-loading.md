# Project `_environment` files are not loaded (bd-environment-files-372u9qbs)

**Date:** 2026-08-09
**Braid:** bd-environment-files-372u9qbs (feature, P1, label `parity`)
**Checkout:** committed on `main` in the bd-eb2wnxkp worktree (the checkout `/investigate-beads` was invoked in; no new branch created — user decides where implementation lands)
**Status:** Design questions answered by user (2026-08-10, recorded below); dotenv-parser research done. Awaiting follow-up on the parser recommendation before implementation.
**Follow-up strand:** bd-ev8mk1rp (render profiles) — filed 2026-08-10, `discovered-from` this strand.

## Design policy: Quarto 2 does not mutate the process environment

Decided 2026-08-10. Q1 implements `_environment` (and `--profile`) by mutating the
process env (`Deno.env.set`); that is not UB in Deno, but it still races in some
scenarios and has caused real Q1 bugs. Rust's edition-2024 `unsafe` on
`std::env::set_var` is a restriction we *want* to model, not work around: in q2,
project environment values are **data plumbed to consumers** (shortcode handlers,
subprocess spawn sites), never writes to the ambient environment — even if some
rare Quarto projects need slight behavior changes as a result. Reading the real
env (`std::env::var`) remains fine; it always wins over file-defined values.

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

## Design decisions (answered by user, 2026-08-10)

1. **Injection mechanism: project env map, no process-env mutation.** See the
   policy section above. Map lives on `StageContext` (modeled on `variables`),
   loaded through `SystemRuntime` so WASM/hub-client gets it via the VFS.
   `EnvShortcodeHandler` consults `std::env::var` first, then the map, then the
   fallback arg.
2. **Consumers: all of them, in this strand.** Besides the `env` shortcode, pass
   the project env at spawn sites: knitr subprocess
   (`engine/knitr/subprocess.rs`), jupyter (`engine/jupyter/*`, including the
   daemon), and pre/post render scripts (`project/render_scripts.rs`) — via
   `Command::envs(project_env)` so the child sees file-defined values but real
   inherited env still wins (set only keys absent from the real env, preserving
   the precedence rule).
3. **Profiles: scoped out.** Filed **bd-ev8mk1rp** (render profiles:
   `--profile`/`QUARTO_PROFILE` plumbing, `_quarto-<profile>.yml`,
   `_environment-<profile>`), `discovered-from` this strand. The loader built
   here should take a (currently always-empty) active-profile list so
   bd-ev8mk1rp can feed it later without rework.
4. **`_environment.required`: issue diagnostics.** Real enforcement, not Q1's
   de-facto no-op: a required variable undefined at load time gets a diagnostic
   (severity TBD in implementation — start with warning). With the span-carrying
   parser (below), the diagnostic can point at the requiring line in
   `_environment.required` and, when applicable, at where a value was defined.
5. **Parser: hand-rolled, span-annotated via quarto-source-map.** Research
   below; dotenvy is not event-based and carries no source positions, so we
   write a small parser that produces `SourceInfo`-annotated entries, following
   the quarto-yaml technique.
6. **Single-file mode: project-scoped only**, matching `_variables.yml` and Q1
   (`is_single_file` → no env files).
7. **Preview reactivity: out of scope.** Q1's env-file watching is itself a
   source of races. q2 semantics: environment changes require restarting the
   process. (Documented behavior, not a gap.)

## Parser research (2026-08-10)

Question: can `dotenvy` give us event-based parsing we could hang
`quarto-source-map` annotations on (the way `quarto-yaml` builds
`YamlWithSourceInfo` from `yaml-rust2`'s `MarkedEventReceiver` events, each
carrying a `Marker`)? Answer: **no — hand-roll.**

**dotenvy** (inspected from git `allan2/dotenvy`, scratchpad clone; released
0.15.7 is 2023-03-22, git main is an unreleased 0.16 rework):

- The parser (`dotenvy/src/parse.rs`, ~620 lines) is an internal
  **line-oriented** recursive-descent parser: `parse_line(&str, …) ->
  Option<(String, String)>`. Not event-based; there is no callback/visitor
  surface to intercept. The public API is an iterator of `(String, String)`
  pairs (`Iter`) or whole-file loads.
- **No source positions on success.** Positions appear only inside
  `ParseBufError::LineParse(line_contents, char_index)` on *errors* — and even
  there it's a char offset into a detached line string, no line number, no file
  offset. Annotating values would mean rewriting the parser, at which point the
  dependency buys nothing.
- **Hidden process-env read inside parsing:** `$VAR`/`${VAR}` substitution in
  values falls back to `std::env::var` (`parse.rs:259`,
  `apply_substitution`). Parsing output depends on ambient env state — hostile
  to WASM, to determinism, and to our data-not-ambient policy.
- Maintenance signal: no release since 2023; main carries an unreleased
  breaking rework. (That rework, notably, returns a non-mutating `EnvMap` by
  default and marks the env-mutating loads `unsafe` — independent validation of
  our no-mutation policy.)
- Dialect differences from Q1 anyway: e.g. dotenvy expands `$VAR` in unquoted
  *and* double-quoted values; Q1's dialect expands only unquoted values. So the
  dependency wouldn't even buy Q1 fidelity.

**Q1's actual dialect** is `@std/dotenv` (JSR; import map pins 0.225.3,
vendored dev copy 0.224.2 — the vendor tree isn't checked out in
`external-sources/`, so this was read from `denoland/std` upstream). It is a
whole-text regex parser with these semantics:

- Optional `export ` prefix; keys must match `[a-zA-Z_][a-zA-Z0-9_]*` (invalid
  keys are *skipped with a warning*, not fatal).
- Three value forms: **single-quoted** (literal, may span multiple lines, no
  escapes/expansion); **double-quoted** (multiline, escape sequences `\n` `\r`
  `\t` `\"` `\'` `\\` expanded, no `$` expansion); **unquoted** (trimmed,
  ` #` starts a trailing comment, and `$VAR` / `${VAR}` / `${VAR:-default}`
  expansion applies).
- Expansion resolves against earlier entries in the same parse, then the real
  process env (`Deno.env.get`), then the `:-` default. (Again: env *read*
  during parse — acceptable for us as a read, resolved at load time against
  `std::env::var`, which is safe; in WASM the real-env lookup simply finds
  nothing.)
- Full-line comments (`#`) and blank lines skipped.

**Plan:** hand-roll a parser targeting the `@std/dotenv` grammar above,
producing per-entry `SourceInfo` spans (key span, value span) via
`quarto-source-map`, with a `parse_with_parent`-style entry point like
`quarto_yaml::parse_file` so diagnostics (Q-16-5, `.required` misses, future
validation) can point at the defining line. Proposed shape:

```rust
struct EnvEntry {
    key: String,
    value: String,           // after unquoting/escapes/expansion
    key_span: SourceInfo,
    value_span: SourceInfo,  // span of the raw value text
}
fn parse_env_file(content: &str, filename: &str) -> (Vec<EnvEntry>, Vec<DiagnosticMessage>)
```

Open sub-questions for implementation (small, can be settled at design
review): where the parser lives (proposal: a module in `quarto-core`, e.g.
`crates/quarto-core/src/project/environment.rs`, extractable to its own crate
later if Posit external consumers want it); and whether multiline quoted
values are worth supporting in v1 (proposal: yes — the grammar is small and
Q1 accepts them).

## Proposed phases (draft)

- Phase 0 — Test plan (TDD): failing tests first. Parser unit tests (dialect
  cases above, span correctness); precedence tests (real env > `.local` >
  `_environment`; real env never overwritten); `.required` diagnostic tests;
  an end-to-end test driving the real render path (the repro fixture)
  asserting `from-env-file` appears in output; WASM-path consideration per
  `.claude/rules/wasm.md`.
- Phase 1 — Span-annotated dotenv parser (quarto-source-map), `@std/dotenv`
  dialect.
- Phase 2 — `load_project_environment` in `StageContext` (modeled on
  `load_project_variables`): read `_environment` + `_environment.local` (+
  profile variants via a for-now-empty profile list), apply precedence, build
  the project env map; `_environment.required` diagnostics.
- Phase 3 — `EnvShortcodeHandler` consults the map (real env first, then map,
  then fallback arg); Q-16-5 hint updated to mention `_environment`.
- Phase 4 — Subprocess propagation: `Command::envs` for keys absent from the
  real env at knitr/jupyter/render-script spawn sites.
- Phase 5 — Docs (`docs/` user-facing; render with q2, not Q1). Document the
  no-mutation policy's user-visible consequence: env-file changes require
  restart (no preview re-watch).

## Risks / tradeoffs (draft)

- **Coordinate with bd-shortcodes-in-metadata-bp06aub8.** If config-string shortcode expansion lands with a different context type, the env source needs to be reachable from there too. Designing the env map as project-load data (not something buried in `StageContext` construction) keeps both consumers possible.
- **WASM leg.** `shortcode_resolve.rs` runs in `wasm-quarto-hub-client`; anything touching it needs full `cargo xtask verify` (not `--skip-hub-build`) before commit. `std::env` in wasm32 is a per-instance stub — another reason to prefer the map.
- **Per-document reload.** `load_project_variables` re-reads `_variables.yml` per `StageContext` (per document). Copying that pattern re-reads `_environment` per document too — fine for correctness (files are tiny), noted in case profiling objects on 352-page sites.
- **Precedence subtlety.** "Real env wins" must be evaluated per-variable at *resolve* time (`std::env::var` first, then map), not by snapshotting the env at load time, or `-M`-style overrides and test isolation get weird.
