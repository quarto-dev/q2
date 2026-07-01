# Plan 4b: Shadow-Engine Feature Validation

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 4 (Julia validation) complete — plus its dependency chain (1a/1b/1c/2/3)
**Blocks:** Nothing (validation + gap-closing plan)
**Estimated sessions:** 2–3

## Overview

Plan 4 validates the TS-engine subsystem against **one** real engine — Julia — which
is a single statically-claiming `Primary` that executes via the jupyter `toMarkdown`
path with `dependencies: true` (inline). That shape is a **weak forcing function**:
large parts of the shipped subsystem are structurally unreachable by it. This plan
covers the **"shadow features"** — code that ships today but that a single-Primary
Julia render never exercises, so it passes green while testing nothing.

Two failure modes motivate this plan:

1. **Shipped-but-inert surfaces.** Real code that runs but does nothing observable to
   a Julia render (console styling options silently discarded, `withSpinner` a no-op,
   jupyter preserve/restore a constant-`false` port, ANSI color stripped, `pandoc`
   metadata emitted as `undefined`). A Julia render exercises these and they *look*
   covered — but nothing asserts their behavior.
2. **The single-Primary blind spot.** The entire resolution **tier model**
   (Interop / explicit-Fallback / implicit-Fallback / presence-gating / multi-engine
   "case-4" loud failure / `whenClass` conditional claims / `contribution_order`
   tiebreak / the `_quarto.yml engines:` project key) is only reachable with **≥2
   contending engines**. Julia is one engine; T1 (`Primary`) always wins and the rest
   never fire.

### Mechanism

Plan 4b needs **no second heavyweight engine**. It uses two cheap mechanisms:

- **A synthetic contending-engine fixture matrix** — a handful of ~50–100-line TS
  test engines with *no daemon* (echo-engine-shaped: they transform cells in-process),
  declaring different `claims` kinds/priorities/languages. These make the tier model
  observable at pennies. They live beside the existing `echo-engine` / `echo-legacy`
  fixtures under `crates/quarto-core/tests/fixtures/extensions/`.
- **Direct unit / integration assertions** on shipped-but-inert code paths — but only where
  there is *real* behavior to pin. For a surface that is inert *because a feature isn't built
  yet* (styling dropped, spinner a no-op, preserve/restore off), do **not** write a test that
  asserts the limitation — that freezes "not implemented" into "must stay unimplemented," and a
  later improvement would read as a regression (deleting a passing test). Instead record an
  **`accepted-untested` prose note** (known v1 limitation, fine to fill in later; matching the
  honesty pass Plans 1c/2/3 do) and, at most, a **loose positive guard** — the call doesn't
  throw and real output/text still flows — never an assertion that the option is discarded or
  the feature is absent. Surfaces with *real* behavior (claim constructors, lifecycle verbs)
  get genuine assertions.

### Scope boundary (READ THIS)

Plan 4b covers features that are **shipped now and testable now**. It deliberately does
**not** absorb work owned by other plans. See the **Out of scope — owned elsewhere**
table at the bottom before adding anything here. In particular Plan 4b does **not** touch:
Pass-1 resolution lift (**Plan 6**), subprocess pooling (**Plan 5**), native percent/spin
conversion + precise `SourceInfo` + A′ byte-range provenance (**Plan 7**), the
`quarto_required` version gate + compat spoof (**Phase 12**), or the loopback-TCP/token
protocol move (**Phase 1.6**).

One item that looks like a 4b candidate is **not**: `set_project` / per-render
`EngineProjectContext` wiring (1c deferral "M1") is a **Julia blocker owned by Plan 1c.2 P1**
— see the note at the end.

## Prerequisites

- [ ] Plan 4 complete (Julia single-Primary render validated end-to-end).
- [ ] `set_project` / per-render `EngineProjectContext` wired (**Plan 1c.2 P1**) — several
  4b fixtures set project context; confirm it is real, not `unwrap_or_default()`.
- [ ] Deno 2 on PATH (synthetic engines are bundled via `q2 build-ts-extension`).

---

## Phase 4b-A: Synthetic contending-engine fixtures

The test substrate the rest of the plan builds on. Each mirrors `echo-engine`'s layout —
`src/<name>.ts` source + a committed `dist/<name>.js` bundle built with `q2 build-ts-extension`,
`_extension.yml` `path: dist/<name>.js` (the parser requires a `.js` path). The first group are
*resolution-shaped* (they make the tier model observable); the second group are *behavioral*
(they drive the QuartoAPI / lifecycle surfaces in Phases D–F).

- [ ] **`alpha` / `beta`** — two engines that both `Primary`-claim language `synth` at
  **equal kind+priority**. Used for `contribution_order` and `_quarto.yml engines:`
  tiebreak tests.
- [ ] **`interop-r`** — `Primary` on `rsynth`, `Interop` on `pysynth`. Exercises T3
  presence-gating (extends its ownership to `pysynth` only when already present).
- [ ] **`fallback-univ`** — declares a universal `Fallback` (the `fallback:` key).
  Exercises T2 (explicit) and T4 (implicit) fallback.
- [ ] **`whenclass-marimo`** — static claim with `whenClass` (claims `pysynth` only when
  the cell's first class is `.marimo`). Exercises `whenClass` first-class conditioning.
- [ ] **`mismatch`** — declares static `claims: synth: {kind: primary}` but its dynamic
  `claimsLanguage("synth")` returns `interop()`. Exercises the static-vs-dynamic
  hard-error guard on first execute-time load.
- [ ] **`content-claim`** — omits `claims-extensions` (né `claims-files`; rename lands in 1c.2
  P4 — write the new key); implements a content-inspecting `claims_file` (e.g. first line
  `# synth-claim`) on `.syn`. Exercises the "one genuine must-load" dynamic `claims_file` path
  **generically** (NOT the `.jl` percent-script case — that specific conversion is Plan 7).
- [ ] **`claims-cant-run`** — `Primary`-claims `synth` but **declares** it cannot run it: empty
  `handled_languages`, so the mismatch is caught at partition time from declared data. (Wording
  tightened 2026-07-02: this is a declaration-driven capability mismatch, NOT a post-hoc check
  that execute ran anything — q2 never verifies execution results; `engine-resolution.md` §10.)
  Paired with a second engine, this is the owned-but-unrunnable half of Phase 4b-B's case-4
  (`|sequence| > 1`) test.
- [ ] **`behave`** — one configurable *behavioral* engine used by Phases D & F: on the
  relevant input it invokes `system.pandoc`, sets each `execProcess` knob, returns
  `intermediateFiles`, runs a long/cancellable `execute`, emits `dependencies: false`, and
  populates the FC-1 result fields. Keeps the resolution fixtures minimal. (Split into a small
  family if one engine can't express all behaviors cleanly.)

Fixtures are Deno-gated exactly like `echo_engine_e2e.rs` (skip when `deno` absent, not `#[ignore]`).

---

## Phase 4b-B: Resolution tier matrix

Each row is a document + engine-set → asserted resolved sequence/ownership, with a named
revert hunk (TDD: write the assertion, revert the tier logic, watch it redden). These
extend `resolution.rs`'s unit tables and add end-to-end renders where the sequence is
observable in output.

- [ ] **T1 baseline** — `{synth}` + `alpha` → `[alpha]` (sanity; already implied).
- [ ] **T2 explicit Fallback** — `engine: [interop-r, fallback-univ]`, doc has an
  unclaimed language → `fallback-univ` picks it up (explicit fallback preempts interop).
- [ ] **T3 Interop presence-gating (positive)** — `engine: [interop-r]`, doc has
  `{rsynth}` + `{pysynth}` → `interop-r` owns **both** (present via `rsynth`, extends to
  `pysynth`).
- [ ] **T3 presence-gating (negative)** — doc has **only** `{pysynth}`, `interop-r` not
  otherwise present → `interop-r` does **not** get dragged in (Interop is gated).
- [ ] **T4 implicit Fallback** — no `engine:` key, `{pysynth}` unclaimed by any Primary,
  `fallback-univ` installed → T4 fires **only because the sequence is implicit**.
- [ ] **T4 gated off for explicit sequences** — same doc but with an explicit `engine:`
  list → T4 does **not** fire (implicit-only gate).
- [ ] **Kind-dominates-priority** — `alpha` `Primary(-100)` vs `fallback-univ`
  `Fallback(100)` on the same language → `alpha` wins (kind beats priority).
- [ ] **`contribution_order` tiebreak (same-language, equal-priority)** — `alpha` and `beta`
  both `Primary(1)` on `synth`, no `_quarto.yml engines:` → first-occurrence in
  `contribution_order` wins (the strict-`>` tiebreak, `resolution.rs:440`). Note `p1_2` covers
  cross-*language* ordering, **not** this same-language tie — this is a new assertion.
- [ ] **`whenClass` conditional** — `{pysynth .marimo}` → `whenclass-marimo` claims;
  bare `{pysynth}` → it does not.
- [ ] **Static-vs-dynamic mismatch** — render a doc using `mismatch` → first execute-time
  load hard-errors (declared `primary`, dynamic `interop`), message points at the
  `_extension.yml` claim.
- [ ] **Case-4 multi-engine loud failure** (`|sequence| > 1`, owned-but-unrunnable) —
  construct a 2-engine sequence where an owning engine can't run its cell → the loud
  failure path fires (single-engine Julia never reaches it).

---

## Phase 4b-C: `_quarto.yml engines:` project-key splice (implement + test)

**This is the one phase with an implementation step, not just a test.** The project-level
`_quarto.yml engines:` key is currently **read by nothing** — `contribution_order` is
built only from `_extension.yml` `contributes.engines` (External names + `Reorder` hints).
The `// Task 9: splice _quarto.yml engines: list here` marker sits at
`crates/quarto-core/src/project/mod.rs:604`. Plan 1c's ordering item was reworded (this
plan's prompt) to carve this out; here we finish it. Q1 semantics are already documented
in Plan 1c's ordering item.

- [ ] **RED test first** — project `_quarto.yml` with `engines: [beta, alpha]` + both
  extensions installed, both `Primary(1)` on `synth`. Before the splice: resolved owner is
  extension-registration order (e.g. `alpha`). Assert `beta` wins → **fails** (RED).
- [ ] **Thread the project `engines:` list** into `build_engine_registry`
  (`project/mod.rs`) — it currently takes only `extensions`. Read the project config's
  `engines:` key and prepend those entries to `order` at the Task-9 site, **before**
  extension auto-promotion, matching Q1: user `_quarto.yml` entries first (deduped, in
  listed order), then extension-contributed names, then `BUILTIN_ORDER`.
- [ ] **Validation parity** — a name in `_quarto.yml engines:` that resolves to no
  registered engine → the existing "not a valid engine … Available engines are: …" hard
  error (same path as the `Reorder`-hint check, `project/mod.rs:606-618`).
- [ ] **GREEN** — the RED test now passes (`beta` wins via project-key ordering).
- [ ] **Cross-check** — the existing `p1_2` / `p1_3` extension-contribution tests still
  pass (the splice adds a *source* of order entries; it must not disturb the extension path).
- [ ] Update Plan 1c's reworded ordering items to point here as **done** once landed
  (they currently say "deferred to Plan 4b").

---

## Phase 4b-D: QuartoAPI inert / unexercised surfaces

Shipped `@quarto/api` behavior a Julia render runs but never pins. Per the Mechanism note:
for surfaces inert *because a feature isn't built yet*, prefer an `accepted-untested` prose
note over a hard assertion; add at most a **loose positive guard** (no throw, real output
flows). Test at the TS unit level (`quarto-api` has vitest) or via the `behave` fixture.

- [ ] **`console.*` styling options ignored** (`console/index.ts:54-64`) — `bold`/`format`/
  color are silently dropped. **Decided (2026-07-01): intended for v1** (diagnostic styling is
  cosmetic in the host context). Treatment: an `accepted-untested` note that styling is
  currently ignored and may be filled in later — **no test asserting the drop** (that would
  freeze the limitation). Optional loose guard: `console.info(msg, {bold:true})` doesn't throw
  and `msg` still reaches the host log.
- [ ] **`withSpinner` runs its fn (spinner is a v1 no-op)** (`console/index.ts:75-109`) —
  positive guard: assert it invokes the wrapped fn and returns its result, and emits
  start/finish logs. Do **not** assert the *absence* of animation — record "neutral no-op
  spinner" as an `accepted-untested` v1 note so a real spinner later isn't a regression.
- [ ] **`system.pandoc` happy path** (`system/index.ts:262-270`) — ships real but *no in-scope
  engine calls it* (Julia uses raw `Deno.Command`). **Default: `accepted-untested`** (Plan 2's
  "no consumer" rationale). Optional stretch: the `behave` fixture invokes `system.pandoc` and
  asserts it shells out.
- [ ] **`execProcess` knobs** (`mergeOutput`/`stderrFilter`/`respectStreams`/`timeout`,
  `system/index.ts:123-130` + Plan 2 B1) — carried, unexercised. **Default: `accepted-untested`**
  (no in-scope consumer). Optional stretch: the `behave` fixture sets each and asserts effect.
- [ ] **`text.postProcessRestorePreservedHtml` is an unimplemented stub** (`text/index.ts:159-163`,
  body is Plan 2 B2) — `accepted-untested` note it's not built. Optional loose guard: it **fails
  loud** (throws a clear not-implemented error) rather than silently no-op'ing — catching a silent
  regression *without* asserting "throwing" as the desired end state.
- [ ] **`path.dataDir(roaming)` ignores `roaming`** — `accepted-untested` note the documented
  Q1-source-compat no-op divergence (no test required).
- [ ] **`interop()` / `fallback()` claim constructors** — reachable only via the object form
  (Julia returns bare `primary()`). *Real* behavior to pin: a fixture returns each; assert the
  wire `TsLanguageClaim` round-trips the kind correctly.
- [ ] **`env.get` / `realPath` PlatformHost members** (RTQ B3b) — verified no production caller
  (only test fakes). The keep-or-remove call is **Plan 2 Phase A's**; 4b only records that no
  witness appeared here.

---

## Phase 4b-E: Jupyter inert / divergent conversion surfaces

Julia routes output through `toMarkdown`, so these run — but their *behavior* is inert or
divergent and unasserted. Test via `quarto-api/jupyter` unit fixtures (notebook JSON in →
markdown out). Same principle as Phase D: don't assert a not-yet-built limitation must persist.

> **Gated on Plan 3 / Plan 4.** The jupyter TS layer (`preserve.ts`, the `NotImplemented`
> throwers) and `julia-engine.ts` are **not in this worktree yet** (the julia engine is an
> out-of-tree checkout). The P3-* citations and line numbers below are **provisional** —
> re-derive them once Plan 3 lands.

- [ ] **Preserve/restore is a constant-`false` v1 no-op** (`preserve.ts`, P3-15; matches Q1
  today) — `accepted-untested` note that the preserve/postProcess path is inert (live restore
  is Plan 2 B2 / RTQ F2). No hard "always false" assertion (it would freeze the no-op).
- [ ] **ANSI on HTML output is strip-only** (P3-16) — feed a cell output with ANSI color codes
  and assert the *positive* correctness property: **no raw escape sequences leak into the HTML**
  (latex/md/ipynb unaffected). Record the "no colorization" divergence as an `accepted-untested`
  note rather than asserting color must be absent.
- [ ] **`pandoc` field is `undefined`, not accumulated** (Plan 3 `pandoc?` note) — the cross-cell
  metadata accumulation is out-of-scope mainline behavior, not a gap. `accepted-untested` note;
  no assertion.
- [ ] **`resultIncludes` widget path** (`julia-engine.ts`, out-of-tree; line TBD) — Plan 4C
  tests plain figures, not widget-bearing outputs. **Default: `accepted-untested`** (blocked on
  the out-of-tree julia engine). Optional stretch: a notebook fixture with an htmlwidget-style
  output asserts `resultIncludes` produces the include.
- [ ] **The jupyter `NotImplemented` throwers** (P3-6; count TBD once Plan 3 lands) — loose
  guard that they fail loud (so the namespace object is total and a silent no-op can't slip in);
  record that no q2 TS runtime consumer needs them.

---

## Phase 4b-F: Lifecycle / protocol verbs

Verbs implemented on both wire ends that a clean single-Julia render never drives. These are
*real* behavior (not not-yet-built), so they get genuine assertions, driven by the `behave`
fixture.

- [ ] **`intermediateFiles` verb** (both ends shipped, production sender at
  `ts_engine.rs:686-702`) — the `behave` fixture returns intermediate files; assert the
  round-trip and that they surface where expected.
- [ ] **Cancellation** (`Cancel`/`Cancelled` + AbortController, `host.ts:880-893`;
  poison/relaunch at `host.ts:455` + `~600-615`) — drive the `behave` fixture into a
  long/cancellable execute; assert cooperative cancellation + transparent re-launch of a
  poisoned instance. (1c's optional crash-path E2E, P3-4, can fold in here — a deliberate
  pull-in of that deferred test, flagged as such.)
- [ ] **Deferred-dependencies round-trip** (`Dependencies` verb; `ts_protocol.rs:84-93`,
  `403-440`; `host.ts:796-877`) — TS handler shipped, **no production Rust sender** (the
  orchestrator consumer is **book-feature owned**, RTQ FC-2). A *serde* round-trip already
  exists (`test_fc2_dependencies_verb_round_trip`); 4b's added value is an **engine-driven**
  round-trip: the `behave` fixture emits `dependencies: false` and the harness drives the
  `Dependencies` verb end-to-end. 4b does **not** build the orchestrator consumer.
- [ ] **FC-1 carried result fields** (`metadata`/`pandoc`/`resourceFiles`/`preserve`/
  `postProcess`, `ts_protocol.rs:385-402`) — the `behave` fixture populates them; assert they
  survive the wire round-trip into `TsExecuteResult` (Julia populates none). Assert *carriage,
  not consumption* — consumers land with the features that need them.

---

## Phase 4b-G: Security posture — `--allow-all` (decided)

**Decided (2026-07-01): `--allow-all` is the accepted v1 posture.** The production
engine-host spawn is `deno run --allow-all` at **`ts_process.rs:510`** (the `:2361`/`:2530`
spawns are `#[cfg(test)]`, not production). Extension bundles are third-party code at full
Deno privilege; for v1 the trust model is "the user installed the extension deliberately,"
and the eventual real boundary is the **Phase 1.6** loopback-TCP/token move, not a Deno
permission set.

- [ ] Annotate the `ts_process.rs:510` spawn site with the accepted-v1 rationale + a pointer
  to Phase 1.6, so the choice is visible and any future narrowing is a deliberate change.
- [ ] No sandbox work in 4b. Narrowing to `--allow-read/write/net/run`, if ever wanted, is
  its own plan.

---

## Out of scope — owned elsewhere (do NOT add to 4b)

| Capability | Owner | Why not 4b |
|---|---|---|
| Pass-1 per-doc engine-resolution lift | **Plan 6** | Additive on top of the shipping Pass-2 resolver; separate research plan. |
| Subprocess pooling (cross-render warmth) | **Plan 5** | "MEASURE FIRST — win is bounded"; separate. Plan 4H already checks one PID *within* a render. |
| Native percent/spin conversion; precise `SourceInfo`; A′ byte-range provenance | **Plan 7** | The `.jl` `# %%` content-claim + faithful provenance are Plan 7's core. 4b's `content-claim` fixture exercises the *generic* dynamic `claims_file` path only. |
| `quarto_required` version gate + `engine_compat_version()` spoof | **Phase 12** | 1c/RTQ ship the field inert on purpose; the gate is Phase 12. |
| Protocol off stdout → loopback TCP + one-time token auth | **Phase 1.6** | The `console.log` footgun and local-connect gap are Phase 1.6; 4b's security item only records the v1 decision. |
| `@quarto/engine-host-wasm`; `EngineClaimsFileStage` in WASM pipeline; jsr/npm publish | future | Browser host + distribution, out of this epic. |
| Multi-class engine (list of claims); per-cell routing; `_quarto.yml` claim overrides | future | Design-doc §12 "least urgent / deferred." |
| `run()` interactive; `filterFormat`/`executeTargetSkipped`/`postRender`/`canKeepSource` | future | "Deferred until q2 grows callers." |

## Note: per-render project context (M1) is owned by Plan 1c.2 P1, not 4b

1c deferred wiring `set_project` / per-render `EngineProjectContext`; `TsEngine::set_project`
has **zero call sites**, so `ensure_launched` `unwrap_or_default()`s (`ts_engine.rs:339` +
field `TODO(plan1c)` at `:122-125`) and the subprocess gets an empty context. **Plan 1c.2 P1**
owns the fix and has already settled the previously-open question: the call site is "a
code-reading task, not a design decision" — one early render-setup point dominates both the
Pass-1 file-claim and Pass-2 resolution launches, and first-write-wins is correct because the
context is project-invariant within a render (so the "render-boundary reset" is a no-op within
one render). Plan 4 depends on it (Julia consumes project context); Plan 4b's
project-context-setting fixtures ride on the same wired path. 4b does not build it.

## Success Criteria

- [ ] Synthetic contending-engine fixtures exist and are Deno-gated like `echo_engine_e2e`.
- [ ] Every resolution tier (T2/T3±/T4±, presence-gating, kind-dominates-priority,
  `whenClass`, static-vs-dynamic mismatch, case-4) has a binding test with a named revert.
- [ ] `_quarto.yml engines:` project-key ordering is **implemented** and tested (RED→GREEN),
  with unregistered-name validation parity; Plan 1c's pointers updated to "done."
- [ ] Every shipped-but-inert QuartoAPI / jupyter surface is documented `accepted-untested`
  (known v1 limitation, improvable later), with at most a loose positive guard (no throw, real
  output flows) — **no test asserts that a limitation must persist**. Surfaces with real
  behavior (claim constructors, lifecycle verbs) get genuine assertions.
- [ ] `intermediateFiles`, cancellation/poison-relaunch, and the deferred-deps **wire**
  round-trip are exercised by synthetic engines; the book-feature-owned orchestrator
  consumer is recorded as out of scope, not tested.
- [ ] The `--allow-all` posture is recorded as accepted-for-v1 and the `ts_process.rs:510`
  spawn site is annotated.
- [ ] No regressions: `cargo nextest run --workspace`, `cd hub-client && npm run test:ci`,
  and `cargo xtask verify` all pass.
