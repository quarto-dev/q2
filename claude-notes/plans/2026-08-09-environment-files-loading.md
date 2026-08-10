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

## Implementation settled (2026-08-10, session 3)

- Parser lives at `crates/quarto-core/src/project/environment.rs` (extractable
  to its own crate later if external consumers appear).
- Multiline quoted values (single- and double-quoted) ARE supported, matching
  `@std/dotenv`.
- FileId scheme: reuse `quarto_yaml::file_id_for_filename` so `_environment`
  diagnostics bind file content through the existing
  `config_sources::bind_config_source` machinery.

## Work items

### Phase 0 — Failing end-to-end test (TDD)

- [x] End-to-end test driving the real render path: smoke-all fixture
  `crates/quarto/tests/smoke-all/metadata/environment-files/` (`_quarto.yml` +
  `_environment` + `_environment.local` + `index.qmd`); asserts both the base
  value and the `.local` override appear, and that `?env:` markers and the
  shadowed base value do NOT. **Observed failing at HEAD 2026-08-10**
  (`SMOKE_FILTER=environment-files cargo nextest run -p quarto -E
  'test(smoke_all)'`): 3 regex mismatches + 2 Q-16-5 warnings.

### Phase 1 — Span-annotated dotenv parser

- [x] Unit tests for the `@std/dotenv` dialect (37 tests, all listed cases
  plus CRLF, BOM, UTF-8 values, duplicate keys, unterminated quotes, junk
  lines, `$$`/lone-`$` literals, escaped `\$`, cycle termination).
- [x] `parse_env_file(content, filename, lookup) -> ParsedEnvFile` in
  `crates/quarto-core/src/project/environment.rs`, spans via
  quarto-source-map, FileId via `quarto_yaml::file_id_for_filename`.
  Expansion diverges from `@std/dotenv` only as documented in the module doc
  (empty-string + diagnostic instead of `"undefined"`; bounded passes +
  diagnostic instead of infinite loop; junk-line warning; first-`}` default).

### Phase 2 — Project env map with precedence

- [x] Precedence unit tests (`merge_env_layers`): `.local` beats
  `_environment`; expansion sees higher-priority layers; real env wins in
  expansion; same-file beats real env in expansion (@std semantics); layer
  diagnostics collected.
- [x] `_environment.required` tests (`check_required`): undefined required
  var → diagnostic whose span resolves to the requiring key in
  `_environment.required`; satisfied by real env or map → quiet.
- [x] `load_project_environment(runtime, project, profiles: &[String], …)`
  called from `StageContext::new` alongside `load_project_variables`;
  `project_env` map (`LinkedHashMap`) stored on `StageContext`. Skipped in
  single-file mode. Present-but-unreadable file warns (via `path_exists`);
  missing files silent.

### Phase 3 — env shortcode consults the map

- [x] Tests: `{{< env NAME >}}` resolves from map when real env unset (map
  beats fallback arg); real env wins over map; Q-16-5 still fires when
  absent everywhere (hint updated to mention `_environment`).
- [x] `EnvShortcodeHandler::new(project_env)` plumbed like `variables`
  through `builtin_handlers` / `with_lua_support` /
  `build_transform_pipeline` / `build_q2_preview_transform_pipeline` /
  `AstTransformsStage` (ctx.project_env). WASM preview gets it free via
  `StageContext::new` + VFS reads.
- [x] **Phase 0 fixture now passes** (`SMOKE_FILTER=environment-files
  cargo nextest run -p quarto -E 'test(smoke_all)'` → PASS).

### Phase 4 — Subprocess propagation

- [x] Tests: `env_for_subprocess` filters real-env-shadowed keys (unit);
  `scripts_receive_project_env` spawns a real `sh` pre-render script that
  reads the variable (unix-gated; Windows coverage = the shared filter unit
  test + mechanical `cmd.env` application). knitr/jupyter spawn sites carry
  the same pre-filtered pairs via `ExecutionContext::project_env` — not
  spawn-tested (no R/jupyter in CI; replay bypasses spawning).
- [x] Injection at all sites, real-env-wins preserved by pre-filtering with
  `env_for_subprocess` (children inherit the real env; we only add keys it
  lacks): knitr `CallROptions::project_env` → `call_r`'s `Command::envs`;
  jupyter `JupyterDaemon::get_or_start_session(..., extra_env)` →
  `start_kernel` spawn (spawn-time input; session key unchanged — a reused
  session keeps its birth env, fine while a process serves one project);
  render scripts `RenderScriptsContext::project_env` (applied before
  `QUARTO_PROJECT_*` so those win), loaded at the five call sites (render
  pre/post, publish pre/post, preview boot) via
  `subprocess_env_for_project`.

### Phase 5 — Docs + verification

- [x] User-facing docs page `docs/guides/projects/environment.qmd` (files,
  dialect, precedence, consumers, `.required`, restart-required note), added
  to the docs sidebar and guides index; rendered with
  `cargo run --bin q2 -- render docs/guides/projects/environment.qmd` and
  output inspected.
- [x] `cargo build --workspace` clean; `cargo nextest run --workspace`:
  **11263 passed** (1 leaky, pre-existing), 197 skipped. `cargo xtask lint`
  clean. Full **`cargo xtask verify` passed** (all 14 steps, WASM leg
  included; one clippy nit — unnested or-pattern — fixed along the way).
- [x] End-to-end verification through the real binary, output inspected:
  - **Shortcode** — `env -u REPRO_VERSION cargo run --bin q2 -- render
    claude-notes/plans/environment-files-loading-investigation/repro` →
    `Version is <strong>from-env-file</strong>.` (was `?env:REPRO_VERSION` +
    Q-16-5 before the fix; now zero warnings).
  - **Engines (real R + real jupyter on this machine)** — scratchpad project
    with `_environment` containing `Q2_E2E_ENGINE_VAR=engine-sees-me`;
    `engine: jupyter` doc printing `os.environ.get(...)` rendered
    `PYVAL:engine-sees-me`; `engine: knitr` doc with `Sys.getenv(...)`
    rendered `RVAL: engine-sees-me`. Re-render with
    `Q2_E2E_ENGINE_VAR=real-wins` exported → both cells print `real-wins`
    (real environment beats the file in subprocesses too).
  - **Render scripts** — covered by the spawning unit test
    `scripts_receive_project_env` (real `sh` child reads the variable).
- [ ] review.md checklist; commit (await user approval per review.md).

## Risks / tradeoffs (draft)

- **Coordinate with bd-shortcodes-in-metadata-bp06aub8.** If config-string shortcode expansion lands with a different context type, the env source needs to be reachable from there too. Designing the env map as project-load data (not something buried in `StageContext` construction) keeps both consumers possible.
- **WASM leg.** `shortcode_resolve.rs` runs in `wasm-quarto-hub-client`; anything touching it needs full `cargo xtask verify` (not `--skip-hub-build`) before commit. `std::env` in wasm32 is a per-instance stub — another reason to prefer the map.
- **Per-document reload.** `load_project_variables` re-reads `_variables.yml` per `StageContext` (per document). Copying that pattern re-reads `_environment` per document too — fine for correctness (files are tiny), noted in case profiling objects on 352-page sites.
- **Precedence subtlety.** "Real env wins" must be evaluated per-variable at *resolve* time (`std::env::var` first, then map), not by snapshotting the env at load time, or `-M`-style overrides and test isolation get weird.
