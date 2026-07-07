# Plan 4b: Shadow-Engine Feature Validation

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 4 (Julia validation) — **✓ complete (2026-07-06); this plan
is unblocked.** Plan 3 (`@quarto/api/jupyter`) has **also landed** — Phases
3A–3F are in-tree, so the jupyter TS layer (`preserve.ts`, the `NotImplemented`
throwers, the ANSI-strip in `to-markdown.ts`) and the `julia-engine.ts` fixture
are **present in this worktree**. Phase 4b-E is therefore **not** blocked (its
old "not in this worktree yet" gating note was stale — corrected in place). (The
grand plan's sub-plans table still lists **both Plan 3 and Plan 4** as remaining
— stale, fix when touching that file.)
**Blocks:** Plan 6 execution (ratified sequencing 2026-07-06: this plan runs
first — see § Coordination with Plan 6 in Phase 4b-C). Otherwise nothing.
**Estimated sessions:** 3–4 (revised up from 2–3 — Phase A ships ~7 fixtures, each with a
committed `q2 build-ts-extension` bundle, feeding ~30 assertions plus Phase C's implementation
leg; two independent reviews flagged the original estimate as optimistic).

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
   (Interop / explicit-Fallback / implicit-Fallback / presence-gating / `whenClass`
   conditional claims / `contribution_order` tiebreak / the `_quarto.yml engines:`
   project key) is only reachable with **≥2
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

- [x] Plan 4 complete (Julia single-Primary render validated end-to-end).
- [x] `set_project` / per-render `EngineProjectContext` wired (**Plan 1c.2 P1 — landed**):
  production call site `engine.set_project(...)` in `build_engine_registry` (`project/mod.rs:713`),
  e2e-pinned in `echo_engine_e2e.rs`; the `unwrap_or_default()` remains only as a fallback behind
  that writer, and the old `TODO(plan1c)` field marker is gone. Several 4b fixtures set project context.
- [x] Deno 2 on PATH (synthetic engines are bundled via `q2 build-ts-extension`) — verified `deno 2.9.0`.

---

## Phase 4b-A: Synthetic contending-engine fixtures

The test substrate the rest of the plan builds on. Each mirrors `echo-engine`'s layout —
`src/<name>.ts` source + a committed `dist/<name>.js` bundle built with `q2 build-ts-extension`,
`_extension.yml` `path: dist/<name>.js` (the parser requires a `.js` path). The first group are
*resolution-shaped* (they make the tier model observable); the second group are *behavioral*
(they drive the QuartoAPI / lifecycle surfaces in Phases D–F).

- [x] **`alpha` / `beta`** — two engines that both `Primary`-claim language `synth` at
  **equal kind+priority**. Used for `contribution_order` and `_quarto.yml engines:`
  tiebreak tests.
- [x] **`interop-r`** — `Primary` on `rsynth`, `Interop` on `pysynth`. Exercises T3
  presence-gating (extends its ownership to `pysynth` only when already present).
- [x] **`fallback-univ`** — declares a universal `Fallback` (the `fallback:` key).
  Exercises T2 (explicit) and T4 (implicit) fallback.
- [x] **`whenclass-marimo`** — static claim with `whenClass` (claims `pysynth` only when
  the cell's first class is `.marimo`). Exercises `whenClass` first-class conditioning.
- [x] **`mismatch`** — declares static `claims: synth: {kind: primary}` but its dynamic
  `claimsLanguage("synth")` returns `interop()`. Exercises the static-vs-dynamic
  hard-error guard on first execute-time load.
- [x] **`content-claim`** — omits `claims-files` (see the typing note below); implements a
  content-inspecting *dynamic* `claims_file` (e.g. first line `# synth-claim`) on `.syn`. Exercises
  the **dynamic-fallback** `claims_file` path **generically** — the case where an engine declares no
  static claim and falls back to the wire round-trip. (A *static* `content-pattern` alternative is
  Plan 7a; the `.jl` percent-script *conversion* is Plan 7. This fixture is NOT "the one genuine
  must-load case" — that framing was overturned by the 2026-07-07 content-pattern census.)
  - **Note on `claims-files` typing** — the typed-entry restructure landed in 1c.2 P4
    (`claims_files: Option<Vec<FileClaim>>`, `extension/types.rs:121`), but today `FileClaim`
    carries **only** `extension: String`; the additive `content_pattern` field is a **Plan 7a**
    future and is **not present now** (it exists only as a doc-comment promise). This fixture
    deliberately uses the *dynamic* `claims_file` wire path, so it does not need
    `content_pattern`. (The earlier rename to `claims-extensions` is **withdrawn** — keep the
    `claims-files` name; the YAML key is hand-parsed, not serde-derived.)
- ~~**`claims-cant-run`**~~ — **removed 2026-07-07 (moved to Plan 4d).** The premise (empty
  `handled_languages` → q2 catches an owned-but-unrunnable engine "from declared data") maps to no
  real mechanism: `handled_languages` is the leave-alone set, not a capability declaration, and q2
  has **no static capability model** and does not police what engines execute
  (`engine-resolution.md` §10). The only real case-4 gate is jupyter's hardcoded
  `is_executable_language`, which reads no engine's declaration; TS engines have no Rust-side case-4
  check. A TS engine's owned-but-unrunnable behavior is its own §10 **self-enforcement** obligation,
  which `owned_languages` (Plan 4d) makes cleanly testable — that test lives in **Plan 4d**
  (Phase 4d-D negative test), not here.
- [x] **`behave`** — **one** configurable *behavioral* engine used by Phases D & F.
  **Decomposition decided (2026-07-08): keep it a single engine, sentinel-gated** — do
  **not** split into a family. Rationale: a Deno engine's `execute(opts)` branches on document
  content (the `echo-engine` precedent uses the `QUARTO_ECHO_CRASH` sentinel + `Deno.exit(1)`,
  `echo-engine.ts:151-192`), and the behaviors never collide because each is reached by a
  *different fixture document* hitting a different branch — so one engine expresses all of them
  cleanly, and one engine means one committed `dist/behave.js` bundle instead of several. The
  branches:
  - **default** — echo-like passthrough that **populates the FC-1 result fields with fixed
    sentinel values** (Phase F asserts these verbatim — the fixture and the assertion must share
    *one* agreed payload, since "carriage, not consumption" is only checkable against known
    values). Suggested sentinels: `metadata: {behaveFc1: "md"}`, `pandoc: {behaveFc1: "pandoc"}`,
    `resourceFiles: ["behave.res"]`, `preserve: {"BEHAVE_KEY": "behave-preserve"}`,
    `postProcess: true`. Emits `dependencies: false`;
  - **`intermediate_files`** — implemented as the separate instance verb (not an `execute`
    branch) so Phase F can round-trip it;
  - **`QUARTO_PANDOC` / `QUARTO_EXEC` sentinels** — `execute` calls `_quarto.system.pandoc(...)` /
    `_quarto.system.execProcess(...)` with each knob set, for the Phase D optional stretches;
  - **`QUARTO_SLOW` sentinel** — the **long/cancellable** path (see below), for Phase F
    cancellation + timeout-poison-relaunch.

  **Long/cancellable `execute` — how (researched 2026-07-08).** The engine **can** cooperate with
  cancellation: although `ExecuteOptions` (`@quarto/types`, `execution.ts:16-55`) declares **no**
  `signal` field, the host splices a live `AbortSignal` onto the options at runtime via
  `Object.assign` (`@quarto/engine-host-deno/src/host.ts:686-696`, comment "q2 extension: attach
  signal for cooperative cancellation"). So the fixture reads it defensively with a cast and
  models the long task as an **event-listener promise** (resolve after N ms **or** reject on
  `abort`), **not** a busy poll:
  ```ts
  const signal = (opts as unknown as { signal?: AbortSignal }).signal;
  if (!opts.target.markdown.value.includes("QUARTO_SLOW"))
    return { markdown: opts.target.markdown.value, supporting: [], filters: [] }; // fast path
  await new Promise<void>((resolve, reject) => {
    if (signal?.aborted) return rejectAbort(reject);        // may be aborted while still queued
    const t = setTimeout(resolve, 60_000);
    signal?.addEventListener("abort", () => { clearTimeout(t); rejectAbort(reject); }, { once: true });
  });
  // rejectAbort throws an Error whose .name === "AbortError" — REQUIRED to hit host.ts:750's
  // clean {type:'cancelled'} branch; any other name becomes an `error` response, not a cancel.
  ```
  **Load-bearing caveat:** the **Rust** host (`ts_process.rs::request`, ~`:637-739`) decides
  `Cancelled`/`Timeout` and calls `poison_instance()` (`ts_engine.rs:805-815`) **independently of
  whether the engine cooperates** — on a cancel-token flip or timeout-window elapse it sends
  `Cancel{target}` fire-and-forget and returns immediately without awaiting the engine's
  `{cancelled}` frame. The engine's `AbortSignal` cooperation only makes the Deno process actually
  stop working. **There is no cancel-that-doesn't-poison on the wire — cancel and timeout both
  poison by design;** the only non-poison outcome is a normal `ExecuteResult` return. This is what
  drives the three-binding split in Phase F (F-cancel / F-relaunch / F-crash; see E).

Fixtures are Deno-gated exactly like `echo_engine_e2e.rs` (skip when `deno` absent, not `#[ignore]`).

**Synthetic languages are safe — no known-language gate (verified 2026-07-08).** The tokens these
fixtures claim (`synth`, `pysynth`, `rsynth`, and the `.syn` extension) are treated as **opaque
string keys** by resolution — there is no allowlist that rejects an unknown language. The four-tier
loops (`resolution.rs:460/480/504/529`) only ever use the language as a `LinkedHashMap` key; static
claims resolve via a bare `claims.get(language)` (`extension/types.rs:214-223`). The one
language-set in the tree, `HANDLED_LANGUAGES` (`engine/mod.rs:123` = `["ojs","mermaid","dot"]`), is
an *exclusion* set, not an admission list; `is_executable_language` is **jupyter-internal**
(`jupyter/text_execute.rs:201-219`, only caller `partition_cells`) and never gates what a TS engine
may claim. Precedent: `echo-engine` already claims the synthetic language `echo` / extension `.echo`
and resolves end-to-end today.

---

## Phase 4b-B: Resolution tier matrix

Each row is a document + engine-set → asserted resolved sequence/ownership, with a named
revert hunk (TDD: write the assertion, revert the tier logic, watch it redden). These
extend `resolution.rs`'s unit tables and add end-to-end renders where the sequence is
observable in output.

**Two different keys — read this before writing any row.** `engine:` (**singular**) and
`engines:` (**plural**) are *distinct keys with distinct consumers* — not a typo for each
other:
- **`engine:`** — a **document / merged-metadata** key, list-valued (`engine: [a, b]`).
  `resolve_engines` reads it via `meta.get("engine")` (`resolution.rs:392`) to build the
  **explicit sequence** and to decide explicit-vs-implicit (which gates T2/T4). **This
  detection ships today** — every Phase B row below runs against the *shipped* resolver and
  is **independent of Phase C**. The `engine: [...]` values in the rows below are this key.
- **`engines:`** — the **project `_quarto.yml`** ordering key that **Phase C implements**
  (feeds `contribution_order`). It is read by *neither* consumer today, which is why Phase C's
  RED test is genuinely red. Do not confuse a Phase B `engine:` row with Phase C's project
  `engines:` splice.

**Named revert seams (for the `fail-on-revert` binding).** The per-tier comparators live at
`resolution.rs:466` (T1 Primary) / `:491` (T2 explicit Fallback) / `:516` (T3 Interop) / `:539`
(T4 implicit Fallback) — grep the tier comment, lines drift. Each tier row reddens its tier's
comparator/loop; **kind-dominates-priority** reddens the T1-before-T2/T4 tier precedence;
**`whenClass`** reddens the `first_class` conditioning in `combine_claims`/`claims_language`;
**static-vs-dynamic mismatch** reddens the guard at `ts_engine.rs:243-336` (already pinned by
the MockTransport test P1-13 at `ts_engine.rs:2268`). Name the specific hunk in each test's
revert comment before writing code.

**Doc composition rule for the fallback/interop rows.** For rows that assert a *fallback* picks
up an "unclaimed" language (T2, T4), choose the doc's cell language(s) so that `interop-r` is
**not** independently present — i.e. use a token claimed by *no* `Primary` (e.g. a fresh
`orphan` language that only `fallback-univ`'s universal `Fallback` catches), and do **not**
also include `{rsynth}` (which would make `interop-r` present and let it extend to `pysynth`
first). Pin each row's exact cell languages in the fixture doc so the intended tier — not
presence-gating — is what fires.

- [x] **T1 baseline** — `{synth}` + `alpha` → `[alpha]` (sanity; already implied).
- [x] **T2 explicit Fallback** — `engine: [interop-r, fallback-univ]`, doc has an
  unclaimed language → `fallback-univ` picks it up (explicit fallback preempts interop).
- [x] **T3 Interop presence-gating (positive)** — `engine: [interop-r]`, doc has
  `{rsynth}` + `{pysynth}` → `interop-r` owns **both** (present via `rsynth`, extends to
  `pysynth`).
- [x] **T3 presence-gating (negative)** — doc has **only** `{pysynth}`, `interop-r` not
  otherwise present → `interop-r` does **not** get dragged in (Interop is gated).
- [x] **T4 implicit Fallback** — no `engine:` key, `{pysynth}` unclaimed by any Primary,
  `fallback-univ` installed → T4 fires **only because the sequence is implicit**.
- [x] **T4 gated off for explicit sequences** — same doc but with an explicit `engine:`
  list → T4 does **not** fire (implicit-only gate).
- [x] **Kind-dominates-priority** — `alpha` `Primary(-100)` vs `fallback-univ`
  `Fallback(100)` on the same language → `alpha` wins (kind beats priority).
- [x] **`contribution_order` tiebreak (same-language, equal-priority)** — `alpha` and `beta`
  both `Primary(1)` on `synth`, no `_quarto.yml engines:` → first-occurrence in
  `contribution_order` wins (the strict-`>` per-tier comparators at `resolution.rs:466/491/516/539`;
  `:440` is the candidate-order comment that sets up the tie). Read the order via the
  `contribution_order()` getter (`registry.rs:113`, landed 1c.2 P4 T12), not the now-`pub(crate)`
  field. Note `p1_2` only asserts name *membership* (no ordering) — so this same-language tie is a
  genuinely new assertion.
- [x] **`whenClass` conditional** — `{pysynth .marimo}` → `whenclass-marimo` claims;
  bare `{pysynth}` → it does not.
- [x] **Static-vs-dynamic mismatch** — render a doc using `mismatch` → first execute-time
  load hard-errors (declared `primary`, dynamic `interop`), message points at the
  `_extension.yml` claim. (Confirm the shipped guard's message actually references the
  `_extension.yml` claim; if it doesn't, assert what it *does* say rather than improving the
  message here — message polish is out of scope.)
- [x] **Dynamic `claims_file` round-trip (drives `content-claim`)** — a whole-file `.syn` input
  whose first line is `# synth-claim` → `content-claim` claims the file via its content-inspecting
  dynamic `claims_file`; a `.syn` file *without* the marker → it does **not**. This is the row that
  gives the `content-claim` fixture (Phase A) a binding assertion — without it the fixture is an
  orphan. The wire path is shipped (`ClaimsFile`/`ClaimsFileResult` verbs, `ts_protocol.rs:63-64`,
  `:125-126`); revert seam is the `ClaimsFile` dispatch/consultation, not a resolution tier.
- ~~**Case-4 multi-engine loud failure**~~ — **removed 2026-07-07 (moved to Plan 4d).** There is no
  q2-side declaration-driven capability check to test: jupyter's case-4 gate is a hardcoded language
  list (`is_executable_language`, reads no engine declaration) and TS engines have no Rust-side gate.
  The in-scope test is a TS engine *self-enforcing* the §10 contract using `owned_languages`; it lives
  in Plan 4d (Phase 4d-D negative test). Building a q2-side declaration-driven capability check is a
  new capability model, out of scope for this epic.

---

## Phase 4b-C: `_quarto.yml engines:` project-key splice (implement + test)

**This is the one phase with an implementation step, not just a test.** The project-level
`_quarto.yml engines:` key is currently **read by nothing** for ordering —
`contribution_order` is built only from `_extension.yml` `contributes.engines` (External
names + `Reorder` hints). The `// Task 9: splice _quarto.yml engines: list here` marker
sits in `build_engine_registry` (`crates/quarto-core/src/project/mod.rs` — `:749` in the
current tree; the file has grown since this plan was drafted, so grep for the marker text
rather than trusting a line number). Plan 1c's ordering item was reworded (this plan's
prompt) to carve this out; here we finish it. Q1 semantics are already documented in Plan
1c's ordering item.

### Coordination with Plan 6 (ratified sequencing, 2026-07-06)

**4b executes before Plan 6.** Two reasons, one about this splice and one about the
tier model:

1. **This splice is the last deferred half of the `engines:` key.** Plan 6
   (`2026-06-29-plan6-pass1-engine-resolution.md`) introduces the *other* half —
   per-engine **claim tables** carried on `engines:` entries — and the two share one
   key, one entry grammar, and one validation site. Landing 4b first means Plan 6
   *extends* a live key rather than co-defining it, and the `engines:` key never ships
   "half-alive" (ordering accepted-but-ignored) to users: Plan 6's Phase 6 is what first
   documents `engines:` for end users, and it should describe full semantics.
2. **4b validates the resolution tier machinery that Plan 6 then modifies.** Plan 6's
   claim tables intercept the claim-consultation point of **all four tiers**; its
   `tiers_unchanged_without_tables` regression pin is only meaningful against a baseline
   that real extensions have exercised — which is exactly what Phases 4b-A/B do (Interop,
   explicit-Fallback, presence-gating, `whenClass`). Prove the tiers green with
   shadow engines first; then Plan 6 changes where one engine's claims come from.

**Grammar this splice must accept (defined by Plan 6, decision 3 + design contract
`claude-notes/designs/engine-and-engines-keys.md`).** `engines:` is a Q1-compatible array
whose entries come in three forms, and the ordering splice must recognize all three:
- **string** — `knitr` — an ordering entry (this phase's primary case);
- **`{path: ...}` map** — Q1's external-engine loader — **reserved / skipped** by q2
  (engines arrive via `_extensions/` discovery); it contributes no ordering entry;
- **`{<name>: {claims: ...}}` single-key map** — Plan 6's claim-table entry — its **key
  is the engine name**, so it *also* contributes an ordering entry (the same name).
  Even though Plan 6 lands after this phase, the parser here must extract the name from
  a single-key map, not assume every entry is a bare string.

**Validation is shared, not duplicated.** The "unknown engine name → hard error at
`build_engine_registry` construction, Q1 message" check this phase adds is the *same*
check Plan 6 relies on for its map entries (Plan 6 decision 3, "Option B"). Land it here
generically — validate the name of **every** entry form (string and single-key-map key)
against the registry, once, at construction — and Plan 6 needs no second validation site.

**Latent tie-flip note (for the changelog / a test comment).** Once this splice is live,
a project that wrote `engines: [{legacy: {claims: [...]}}]` *purely* for a Plan-6 claim
table also promotes `legacy` to the front of the candidate order — which can flip an
equal-priority tie. Because 4b lands first, this is simply the key's behavior from day
one (no retroactive surprise), but a co-located test comment noting that a name-map entry
is dual-purpose (order + claims) will save a future reader the double-take.

- [x] **RED test first** — project `_quarto.yml` with `engines: [beta, alpha]` + both
  extensions installed, both `Primary(1)` on `synth`. Before the splice: resolved owner is
  extension-registration order (e.g. `alpha`). Assert `beta` wins → **fails** (RED).
  *Readiness check:* confirm the test is *genuinely* red first — the plural project `engines:`
  key must be read by **neither** `resolve_engines` (which reads the **singular** `engine:` via
  `meta.get("engine")`, `resolution.rs:392`) **nor** `contribution_order` (Task-9 is still a
  placeholder) today. If it were somehow already honored, this would land green and the wrong
  lever would be in play.
  **Landed as `c1_project_engines_key_orders_beta_before_alpha`
  (`crates/quarto-core/tests/integration/engine_registry_build.rs`).** First attempt (two
  sibling `_extensions/alpha`, `_extensions/beta` dirs) landed green with ZERO implementation —
  `fs::read_dir`'s filesystem-dependent order happened to return beta before alpha, an
  incidental pass unrelated to the splice. Fixed by installing alpha+beta as two
  `contributes.engines` entries of a SINGLE "combo" extension (`install_combo_alpha_beta_extension`),
  whose YAML array order is deterministic — confirmed genuinely RED
  (`left: Some("alpha"), right: Some("beta")`, `contribution_order: ["alpha", "beta"]`) before
  any production code existed.
- [x] **Thread the project `engines:` list** into `build_engine_registry`
  (`project/mod.rs`) — it currently takes only `extensions`. Read the project config's
  `engines:` key and prepend those entries to `order` at the Task-9 site, **before**
  extension auto-promotion, matching Q1: user `_quarto.yml` entries first (deduped, in
  listed order), then extension-contributed names, then `BUILTIN_ORDER`. **Parse all
  three entry forms** (see § Coordination with Plan 6): a bare string is a name; a
  single-key `{<name>: …}` map contributes its key as a name; a `{path: …}` map is
  reserved/skipped (no ordering entry). A helper that maps an entry → `Option<name>`
  keeps this phase and Plan 6's table reader consistent.
  **Landed:** `build_engine_registry`'s existing `config: Option<&ProjectConfig>` parameter
  already carried the parsed `_quarto.yml` (it was only being read for the wire `config` map,
  `build_engine_config_map`) — no signature change was needed. Added `engine_entry_name`
  (`project/mod.rs`, just above `build_engine_registry`) as the entry→`Option<name>` helper,
  and the Task-9 splice reads `config.metadata.get("engines").as_array()`, maps each entry
  through the helper, and prepends the deduped result ahead of the existing
  `registry.contribution_order`.
- [x] **Validation (shared with Plan 6)** — a name in `_quarto.yml engines:` that
  resolves to no registered engine → the existing "not a valid engine … Available
  engines are: …" hard error (same path as the `Reorder`-hint check; the check block is
  `project/mod.rs:752-762`, message at `:757-758`). Validate **every** entry form's name (string + single-key-map
  key), once, at `build_engine_registry` construction — this is the single validation site
  Plan 6 relies on for its claim-table map entries (Plan 6 decision 3 / Option B), so no
  second check is added there.
  **Landed:** no second check was added — the splice prepends project-declared names directly
  into `registry.contribution_order` BEFORE the existing step-6 validation loop runs, so that
  loop validates project names for free.
- [x] **GREEN** — the RED test now passes (`beta` wins via project-key ordering).
- [x] **Validation test (missing-test pass, added 2026-07-08)** — the validation *item* above had
  no bound test. Add one: project `engines: [nonexistent-synth]` → `build_engine_registry`
  construction returns the "not a valid engine … Available engines are: …" hard error. This is a
  **distinct path from `p1_3`** (which only exercises the `Reorder`-hint validation); the new
  `engines:`-key validation needs its own binding. Revert seam: the new entry-name validation over
  the project `engines:` list → no error → assertion (expects `Err`) RED.
  **Landed as `c2_unknown_project_engine_name_errors_listing_available`.**
- [x] **`{path:}` reserved-skip test (missing-test pass)** — `engines: [{path: ./x.js}]` must
  contribute **no** ordering entry **and not error** (Q1's external-loader form is reserved in q2).
  Assert the resolved `contribution_order` is unchanged by the `{path:}` entry. Revert seam: the
  entry→`Option<name>` mapper's `{path:}`⇒`None` arm → a `path` string leaks in as a name →
  ordering changes or validation errors → assertion RED.
  **Landed as `c3_path_map_entry_reserved_skip_no_error_no_order_change`.**
- [x] **`{name:{claims}}` ordering-extraction test (missing-test pass; strengthens Plan 6 seam)** —
  `engines: [{beta: {claims: […]}}]` (a single-key claim-table map, whose *payload* is Plan 6's and
  is **ignored** in 4b) must still promote `beta` in the candidate order exactly as the bare string
  `beta` would. Assert `beta` wins the `alpha`/`beta` tie via the map-form entry. This binds the
  parse branch that would otherwise ship untested until Plan 6 (see the *Latent tie-flip note*
  above). Revert seam: the single-key-map name-extraction arm → `beta`'s name not extracted → `beta`
  does not order first → assertion RED.
  **Landed as `c4_single_key_map_entry_orders_beta_first_payload_ignored`.**
- [x] **Cross-check** — (a) the existing `p1_2` / `p1_3` extension-contribution tests still
  pass (the splice adds a *source* of order entries; it must not disturb the extension path);
  (b) **Phase B's `contribution_order` tiebreak row still holds** — with **no** project
  `engines:` key, `alpha`/`beta` still resolve by registration order (this splice only fires
  when the key is present), so the B baseline and the C override are complementary, not
  contradictory.
  **Confirmed:** full `cargo nextest run -p quarto-core` (2688 tests, 34 skipped) all green,
  including `p1_2_contribution_order_contains_declared_engines`,
  `p1_3_unknown_reorder_hint_errors_listing_available`, and
  `engine::resolution::tests::test_b8_contribution_order_tiebreak`.
- [x] Update Plan 1c's reworded ordering items to point here as **done** once landed
  (they currently say "deferred to Plan 4b").
  **Done** — see `claude-notes/plans/2026-04-16-plan1c-extension-integration.md`.

---

## Phase 4b-D: QuartoAPI inert / unexercised surfaces

Shipped `@quarto/api` behavior a Julia render runs but never pins. Per the Mechanism note:
for surfaces inert *because a feature isn't built yet*, prefer an `accepted-untested` prose
note over a hard assertion; add at most a **loose positive guard** (no throw, real output
flows). Test at the TS unit level (`quarto-api` has vitest) or via the `behave` fixture.

- [x] **`console.*` styling options ignored** (`console/index.ts:54-64`) — `bold`/`format`/
  color are silently dropped. **Decided (2026-07-01): intended for v1** (diagnostic styling is
  cosmetic in the host context). Treatment: an `accepted-untested` note that styling is
  currently ignored and may be filled in later — **no test asserting the drop** (that would
  freeze the limitation). Optional loose guard: `console.info(msg, {bold:true})` doesn't throw
  and `msg` still reaches the host log.
- [x] **`withSpinner` runs its fn (spinner is a v1 no-op)** (`console/index.ts:75-109`) —
  positive guard: assert it invokes the wrapped fn and returns its result, and emits
  start/finish logs. Do **not** assert the *absence* of animation — record "neutral no-op
  spinner" as an `accepted-untested` v1 note so a real spinner later isn't a regression.
- [x] **`system.pandoc` happy path** (`system/index.ts:262-270`) — ships real but *no in-scope
  engine calls it* (Julia uses raw `Deno.Command`). **Default: `accepted-untested`** (Plan 2's
  "no consumer" rationale). Optional stretch: the `behave` fixture invokes `system.pandoc` and
  asserts it shells out.
- [x] **`execProcess` knobs** (`mergeOutput`/`stderrFilter`/`respectStreams`/`timeout`,
  `system/index.ts:123-130` + Plan 2 B1) — carried, unexercised. **Default: `accepted-untested`**
  (no in-scope consumer). Optional stretch: the `behave` fixture sets each and asserts effect.
- [x] **`text.postProcessRestorePreservedHtml` is an unimplemented stub** (`text/index.ts:159-163`,
  body is Plan 2 B2) — `accepted-untested` note it's not built. Optional loose guard: it **fails
  loud** (throws a clear not-implemented error) rather than silently no-op'ing — catching a silent
  regression *without* asserting "throwing" as the desired end state.
- [x] **`path.dataDir(roaming)` ignores `roaming`** — `accepted-untested` note the documented
  Q1-source-compat no-op divergence (no test required).
- [x] **`interop()` / `fallback()` claim constructors** — reachable only via the object form
  (Julia returns bare `primary()`). **Reuse the Phase A resolution fixtures — no new fixture:**
  `interop-r`'s `claimsLanguage("pysynth")` already returns `interop()`, and `fallback-univ`'s
  returns `fallback()`; pin those two (add a `primary()` check via `alpha` if desired). *Real*
  behavior to pin: the fixture's `claimsLanguage` returns each constructor's result, and the
  assertion is on the **harness-normalized wire form** — the Rust
  `LanguageClaim` enum (`Primary`/`Interop`/`Fallback`) that `@quarto/engine-host-deno` produces from
  the author value per the normalization table in `engine-resolution.md §3.2`. **Layer note:** the
  author SDK constructors return `@quarto/types`' `LanguageClaim` (`{kind, priority?}`), *not*
  `TsLanguageClaim` (an earlier draft conflated the two); assert that each `kind` survives normalization
  into the correct wire-enum variant, not that a type named `TsLanguageClaim` round-trips.
- [x] **`env.get` / `realPath` PlatformHost members** (RTQ B3b) — verified no production caller
  (only test fakes). The keep-or-remove call is **Plan 2 Phase A's**; 4b only records that no
  witness appeared here.

---

## Phase 4b-E: Jupyter inert / divergent conversion surfaces

Julia routes output through `toMarkdown`, so these run — but their *behavior* is inert or
divergent and unasserted. Test via `quarto-api/jupyter` unit fixtures (notebook JSON in →
markdown out). Same principle as Phase D: don't assert a not-yet-built limitation must persist.

> **In-tree — Plan 3 landed (corrected 2026-07-08).** The jupyter TS layer is
> **present in this worktree**, so these items are **testable now** (the earlier
> "not in this worktree yet / out-of-tree julia engine" gating note was stale):
> - `preserve.ts` → `ts-packages/quarto-api/src/jupyter/preserve.ts`
> - the `NotImplemented` throwers → `jupyter/index.ts:38-49` (`notImplemented<T>()`),
>   **15 of them**, wired further down `index.ts`
> - ANSI-strip → `jupyter/to-markdown.ts:228-235` (`stripAnsiCode` / `ANSI_PATTERN`,
>   applied at `:395,:405`)
> - `julia-engine.ts` → **in-tree fixture** at
>   `crates/quarto-core/tests/fixtures/extensions/julia-engine/src/julia-engine.ts`
>   (there is no separate out-of-tree checkout); `widgets.ts` also present.
>
> Line numbers still drift — grep the symbol, don't trust the number. **One real
> gap:** the jupyter `pandoc?` field cited below does **not** exist anywhere under
> `quarto-api/src/` — that bullet is corrected accordingly.

- [x] **Preserve/restore is a constant-`false` v1 no-op** (`preserve.ts`, P3-15; matches Q1
  today) — `accepted-untested` note that the preserve/postProcess path is inert (live restore
  is Plan 2 B2 / RTQ F2). No hard "always false" assertion (it would freeze the no-op).
- [x] **ANSI on HTML output is strip-only** (`to-markdown.ts:228-235`, P3-16) — feed a cell
  output with ANSI color codes and assert the *positive* correctness property: **no raw escape
  sequences leak into the HTML** (latex/md/ipynb unaffected). Record the "no colorization"
  divergence as an `accepted-untested` note rather than asserting color must be absent. **Testable
  now** (`to-markdown.ts` + its vitest are in-tree).
- [x] **No cross-cell `pandoc` metadata accumulation** — **corrected:** there is **no `pandoc?`
  field** on the jupyter result type under `quarto-api/src/` today (the "field is `undefined`"
  framing assumed a field that doesn't exist). Cross-cell metadata accumulation is simply not
  built. `accepted-untested` prose note that it's absent; **no assertion**, and no fixture needed.
  (If a future reader adds a `pandoc?` field, re-derive this item then.)
- [x] **`resultIncludes` widget path** (`julia-engine.ts` in-tree fixture; `widgets.ts` present) —
  Plan 4C tests plain figures, not widget-bearing outputs. **Testable now** (not blocked). The
  **binding deliverable is the `accepted-untested` note**; the widget test is a **genuinely optional
  stretch** (do it if cheap, skip without guilt). If taken, derive the fixture shape from
  `jupyter/widgets.ts` at implementation time — a notebook cell output carrying an
  `application/vnd.jupyter.widget-view+json` (or equivalent htmlwidget) MIME payload — and assert
  the exact include string `widgetDependencyIncludes`/`resultIncludes` emits for it. Do **not**
  treat this as required; the notebook JSON + expected include are not pre-pinned here on purpose.
- [x] **The jupyter `NotImplemented` throwers** (`jupyter/index.ts:38-49`, **15 throwers**) — loose
  guard that they fail loud (so the namespace object is total and a silent no-op can't slip in);
  record that no q2 TS runtime consumer needs them. **Testable now** via `quarto-api`'s vitest.

---

## Phase 4b-F: Lifecycle / protocol verbs

Verbs implemented on both wire ends that a clean single-Julia render never drives. These are
*real* behavior (not not-yet-built), so they get genuine assertions, driven by the `behave`
fixture.

- [x] **`intermediateFiles` verb** (both ends shipped, production sender `fn intermediate_files`
  at `ts_engine.rs:827-860` — grep the fn name, the line drifts) — the `behave` fixture returns
  intermediate files; assert the round-trip and that they surface where expected.

### Cancellation / poison — three bindings (decided 2026-07-08, per E)

The original single "cancellation" bullet conflated *distinct* mechanisms at *distinct* layers.
The research (see the `behave` fixture note in Phase A) showed cancel and timeout **both poison**
— there is no clean-cancel-without-poison — and that the Rust host decides the outcome
independently of engine cooperation. So this is **three** separate bindings, below (`F-cancel`,
`F-relaunch`, `F-crash`).

- [x] **F-cancel — cooperative cancel via token flip (unit, MockTransport).** Template:
  `ts_process.rs:1723` `test_cancel_distinguishable_and_prompt` (and `:1771`
  `test_none_window_still_cancellable`). Drive a long execute, flip the `Cancellation` token, assert
  `Err(Cancelled)`, that it returns **promptly** (well under the window), that `ToEngine::Cancel
  { target }` was sent, and that `poison_instance()` fired (`ts_engine.rs:805-815`). This is a
  *unit* test because `RenderToFileOptions` doesn't surface the `Cancellation` handle — an explicit
  token flip can only be injected at the `ts_process`/`ts_engine` layer.
- [x] **F-relaunch — timeout → poison → transparent relaunch (real-Deno, render path).** Use the
  `behave` fixture's `QUARTO_SLOW` branch with `execute: timeout: 1` in the fixture front matter
  (`resolve_execute_timeout` reads the `execute: timeout:` key, `engine_execution.rs:597-615`).
  Assert the first execute yields `Err(Timeout)` + poison, then a **second** execute on the same
  engine **succeeds** via transparent relaunch through `stashedContextByName` reconstruction
  (`host.ts:600-628`). This is the end-to-end proof that relaunch works; it is the only cancel/poison
  behavior drivable through the real-Deno render path.
  - **Path-exercised guard (mandatory — vacuity trap):** "second execute succeeds" alone passes
    *whether or not the first execute actually poisoned* (if the timeout never fired, the second
    call trivially works). Assert a **relaunch witness**: a fresh `LaunchEngine` was issued between
    execute-1 and execute-2 (observable in the wire trace / launch counter — the poison-policy unit
    tests at `ts_engine.rs:1348-1452` assert exactly "a new `LaunchEngine` after poison"), **or** the
    process PID changed. Without this the test is theater.
- [x] **F-crash — basic crash-path relaunch (real-Deno) — folds in 1c's optional crash E2E (P3-4).**
  Keep it **basic** (per request): a `behave` sentinel branch that `Deno.exit(1)`s mid-execute
  (mirroring `echo-engine`'s `QUARTO_ECHO_CRASH`; the existing `t13_crash_mid_execute_yields_
  process_crashed_with_stderr` in `echo_engine_e2e.rs:908` already asserts `ProcessCrashed` + stderr)
  — assert `ProcessCrashed`, then assert the **next** execute transparently relaunches. **Same
  relaunch-witness guard as F-relaunch** (fresh `LaunchEngine` / new PID between the crash and the
  recovery execute — not merely "next execute Ok"). Flagged as a deliberate pull-in of the deferred
  1c test.
  **DEVIATION (user-approved 2026-07-08): this stopped being "basic, test-only."** Driving the
  crash test surfaced a real **production gap**, not just a missing test: the poison guard
  (`ts_engine.rs:811`) didn't cover `ProcessCrashed`, and the transport handle
  (`TsEngineHost.write`, `ts_process.rs:386`, a `OnceLock`) couldn't be reset — so a crashed
  engine hard-failed the *next* execute with a raw "Broken pipe" instead of relaunching. Given
  the choice, the user said **"Fix the production gap now."** F-crash therefore shipped as a
  **production fix**, not a pure test-binding: poison guard extended to `ProcessCrashed`; the
  `OnceLock` write handle made resettable (`Mutex<Option<...>>`); a `loaded_generation`-driven
  `LoadEngine` resend on relaunch; and a **generation guard** on `reset_after_crash` (added in a
  post-review fix, commit `da78c647e`) so a stale crash observer under concurrent renders can't
  tear down a healthy respawned transport. Commits `aac787481..da78c647e`. **Scope note:** the
  fix covers **Execute-verb-observed crashes only** — a crash during `LoadEngine`/`LaunchEngine`
  still surfaces as a raw broken-pipe error, matching the pre-existing Cancel/Timeout scope (not
  a new limitation introduced here).

  **Why the gap existed (the core insight).** Timeout/Cancel relaunch worked with a `OnceLock`
  transport because a *timeout leaves the subprocess alive* — relaunch reuses the same stdio
  transport and only re-sends `LaunchEngine` (logical instance reset). A *crash kills the
  subprocess*, so its stdin pipe is dead and relaunch needs a **fresh subprocess + fresh
  transport** — which a `OnceLock` (write-once) cannot provide. That asymmetry is exactly why the
  bug was invisible until crash-relaunch: `Mutex<Option<…>>` makes the transport clearable so the
  next `ensure_started()` genuinely respawns, and `loaded_generation` re-sends `LoadEngine` because
  the fresh Deno process starts with an empty `loadedByPath` registry.

  **The generation guard was driven by a review-caught Critical.** The first fix (`70fcf6264`)
  cleared the transport unconditionally in `reset_after_crash`. Review found that production shares
  **one `Arc<TsEngineHost>` per engine across parallel document renders** (`pass2_renderer.rs`
  `docs.par_iter()` + `stage/context.rs`'s `registry.clone()`), so a crash broadcasts
  `ProcessCrashed` to *every* in-flight page. A **stale** observer arriving after a sibling had
  already respawned would tear down the *healthy new* transport — hanging on a `.join()` of a live
  reader thread (while holding the coarse lock → freezing the host) and possibly `kill()`ing a
  healthy child. The fix (`da78c647e`) captures the transport generation at **send time** and makes
  `reset_after_crash` a no-op unless the host is still at that generation, compared under the
  `reader` lock (which `ensure_started_inner` also holds while bumping the generation + committing
  the new transport). This makes the guard **only ever too conservative, never too aggressive** —
  it can never tear down a newer healthy transport. A deterministic regression test
  (`test_reset_after_crash_generation_guard_ignores_stale_observer`) binds it.

  **Known benign residual (documented, not fixed).** There is a narrow TOCTOU: if a sibling
  completes a full respawn in the ~two-instruction window between a request's generation capture
  and its send, and that newer generation then crashes with this observer as the *sole* witness,
  its stale-generation `reset_after_crash` no-ops and the crash goes unrecovered (next execute =
  broken pipe). This equals the *pre-fix* behavior for that one observer (a crash that isn't
  recovered), never corruption, and requires a second crash inside a two-instruction window with no
  same-generation sibling — practically unreachable. Judged benign in final review; recorded here so
  a future maintainer who wants full coverage knows the remaining seam.
- [x] **Deferred-dependencies round-trip** (`Dependencies` verb; `ts_protocol.rs:84-93`,
  `403-440`; `host.ts:796-877`) — TS handler shipped, **no production Rust sender** (the
  orchestrator consumer is **book-feature owned**, RTQ FC-2). A *serde* round-trip already
  exists (`test_fc2_dependencies_verb_round_trip`); 4b's added value is an **engine-driven**
  round-trip: the `behave` fixture emits `dependencies: false` and the harness drives the
  `Dependencies` verb end-to-end. 4b does **not** build the orchestrator consumer.
- [x] **FC-1 carried result fields** (`metadata`/`pandoc`/`resourceFiles`/`preserve`/
  `postProcess`, `ts_protocol.rs:412-431` on `TsExecuteResult` — grep the fields, line drifts) —
  the `behave` default branch populates them with the **agreed sentinel payload** specified in the
  Phase A `behave` spec (`{behaveFc1: …}`, `["behave.res"]`, etc.); assert those exact values
  survive the wire round-trip into `TsExecuteResult` (Julia populates none). Assert *carriage, not
  consumption* — consumers land with the features that need them.

---

## Phase 4b-G: Security posture — `--allow-all` (decided)

**Decided (2026-07-01): `--allow-all` is the accepted v1 posture.** The production
engine-host spawn is `deno run --allow-all` in `fn ensure_started` at **`ts_process.rs:520`**
(verified 2026-07-08 — grep `--allow-all`, line drifts; the two `:2676`/`:2845` spawns are
`#[cfg(test)]`, not production). Extension bundles are third-party code at full
Deno privilege; for v1 the trust model is "the user installed the extension deliberately,"
and the eventual real boundary is the **Phase 1.6** loopback-TCP/token move, not a Deno
permission set.

- [x] Annotate the `ts_process.rs:520` spawn site (`fn ensure_started`, grep `--allow-all`) with
  the accepted-v1 rationale + a pointer to Phase 1.6, so the choice is visible and any future
  narrowing is a deliberate change.
- [x] No sandbox work in 4b. Narrowing to `--allow-read/write/net/run`, if ever wanted, is
  its own plan.

---

## Out of scope — owned elsewhere (do NOT add to 4b)

| Capability | Owner | Why not 4b |
|---|---|---|
| Pass-1 per-doc engine-resolution lift; per-engine **claim tables** on `engines:` | **Plan 6** | Additive on top of the shipping Pass-2 resolver. Plan 6 is a **reviewed implementation plan** that executes **after** this one (see § Coordination with Plan 6 in 4b-C); it *extends* the `engines:` key and validation site this plan lands. |
| Subprocess pooling (cross-render warmth) | **Plan 5** | "MEASURE FIRST — win is bounded"; separate. Plan 4H already checks one PID *within* a render. |
| Native percent/spin conversion; precise `SourceInfo`; A′ byte-range provenance | **Plan 7** | The `.jl` `# %%` content-claim + faithful provenance are Plan 7's core. 4b's `content-claim` fixture exercises the *generic* dynamic `claims_file` path only. |
| `quarto_required` version gate + `engine_compat_version()` spoof | **Phase 12** | 1c/RTQ ship the field inert on purpose; the gate is Phase 12. |
| Protocol off stdout → loopback TCP + one-time token auth | **Phase 1.6** | The `console.log` footgun and local-connect gap are Phase 1.6; 4b's security item only records the v1 decision. |
| `@quarto/engine-host-wasm`; `EngineClaimsFileStage` in WASM pipeline; jsr/npm publish | future | Browser host + distribution, out of this epic. |
| Multi-class engine (list of claims); per-cell routing | future | Design-doc §12 "least urgent / deferred." (`_quarto.yml` claim overrides are **no longer future** — they are Plan 6's claim tables, row above.) |
| `run()` interactive; `filterFormat`/`executeTargetSkipped`/`postRender`/`canKeepSource` | future | "Deferred until q2 grows callers." |

## Note: per-render project context (M1) was owned by Plan 1c.2 P1 (landed), not 4b

**1c.2 P1 landed this.** `TsEngine::set_project` now has a production call site in
`build_engine_registry` (`project/mod.rs:713`) — one early render-setup point that dominates both the
Pass-1 file-claim and Pass-2 resolution launches; first-write-wins is correct because the context is
project-invariant within a render (the "render-boundary reset" is a no-op within one render). The
`ensure_launched` `unwrap_or_default()` remains only as a fallback behind that writer, and the old
`TODO(plan1c)` field marker is gone. Plan 4 depended on it (Julia consumes project context); Plan 4b's
project-context-setting fixtures ride on the same wired path. 4b does not build it.

## Test Seam Spec (frozen — prevalidated 2026-07-08)

One row per **binding** test. Once a row goes green its harness + assertion are **frozen** — never
edited to go green. Tiers: **RU** = Rust unit (pure `resolution.rs` tables / `MockTransport`, no
Deno) · **RE** = Rust e2e (real-Deno render path, Deno-gated skip) · **RI** = Rust integration
(`build_engine_registry`, no Deno) · **TV** = TS vitest (`quarto-api`). "Revert → RED" names the one
production hunk whose removal reddens the named assertion. Line numbers drift — grep the symbol.

| # | Test | Tier | Real unit (never mocked) | Seam: setup → trigger → assertion surface | Mock boundary | Revert hunk → assertion RED |
|---|---|---|---|---|---|---|
| B1 | T1 baseline | RU | `resolve_engines` | `{synth}` + `alpha` → assert owner == `alpha` | registry `claim_fn` | T1 comparator `resolution.rs:466` → `alpha` not owned |
| B2 | T2 explicit Fallback | RU | `resolve_engines` | `engine:[interop-r,fallback-univ]` + an `orphan` lang claimed by no Primary → owner == `fallback-univ` | registry | T2 loop `:491` → fallback not picked |
| B3 | T3 presence-gate + | RU | `resolve_engines` | doc `{rsynth}`+`{pysynth}`, `engine:[interop-r]` → ownership has **both** `rsynth→interop-r` **and** `pysynth→interop-r` (the "both" is the discriminator) | registry | T3 Interop loop `:504–516` → `pysynth` unowned |
| B4 | T3 presence-gate − | RU | `resolve_engines` | doc **only** `{pysynth}` → assert `interop-r` **absent** from sequence | registry | presence-gate cond `:504–516` → `interop-r` dragged in |
| B5 | T4 implicit Fallback | RU | `resolve_engines` | no `engine:` key, `{pysynth}` unclaimed → `fallback-univ` owns `pysynth` | registry | T4 loop `:539` → not fired |
| B6 | T4 gated-off | RU | `resolve_engines` | same doc + explicit `engine:[…]` → assert `fallback-univ` **absent** | registry | implicit gate `has_engine_key` `resolution.rs:392` → T4 fires under explicit |
| B7 | kind-dominates-priority | RU | `resolve_engines` | `alpha` Primary(−100) vs `fallback-univ` Fallback(100), same lang → owner == `alpha` | registry | tier order (T1 before T2/T4) → Fallback(100) wins |
| B8 | `contribution_order` tiebreak | RU | `resolve_engines` + `registry.contribution_order()` (`registry.rs:113`) | `alpha`,`beta` both Primary(1), **no** `engines:` → owner == first-in-order (name discriminates) | registry | `>`→`>=` at `:466` → last-occurrence wins, `alpha`/`beta` flip |
| B9 | `whenClass` conditional | RU | `combine_claims`/`claims_language` (pure) | `{pysynth .marimo}` → claims; bare `{pysynth}` → `None` | none | `when_class` guard `extension/types.rs:181–182` → bare also claims |
| B10 | static-vs-dynamic mismatch | RE (+ RU) | `TsEngine::ensure_loaded` | load `mismatch` → hard `Err` naming `_extension.yml` claim | none (real Deno) | guard `ts_engine.rs:243–336` → no error (RU leg already pinned: P1-13 `:2268`) |
| B11 | dynamic `claims_file` | RE | `TsEngine` + `ClaimsFile` wire | `.syn` w/ `# synth-claim` → `content-claim` claims; w/o marker → not | none | `ClaimsFile` dispatch `ts_engine.rs:~315` → marker not consulted |
| C1 | `engines:` ordering RED→GREEN | RI | `build_engine_registry` | project `engines:[beta,alpha]`, both Primary(1) → owner == `beta` | real registry | Task-9 splice `project/mod.rs:749` → `beta` not first |
| C2 | unknown-name validation | RI | `build_engine_registry` | `engines:[nonexistent]` → `Err` "not a valid engine…" | — | new `engines:`-key validation → no error (distinct from `p1_3` Reorder path) |
| C3 | `{path:}` reserved-skip | RI | `build_engine_registry` | `engines:[{path:./x.js}]` → `contribution_order` unchanged, **no** error | — | entry→`Option<name>` `{path:}⇒None` arm → path leaks as name |
| C4 | `{name:{claims}}` ordering | RI | `build_engine_registry` | `engines:[{beta:{claims:…}}]` (payload ignored in 4b) → `beta` wins tie | — | single-key-map name-extraction arm → `beta` not ordered |
| D1 | `withSpinner` runs fn | TV | `quarto-api` `console` | `withSpinner(fn)` → fn invoked + result returned + start/finish logs | `host.log` spy | `await fn()`/`return result` `console/index.ts:95,108` → undefined/not-invoked |
| D2 | claim-constructor normalization | TV/RU | `engine-host-deno` `mapLanguageClaim` | `claimsLanguage` returns `interop()`/`fallback()` → wire enum `Interop`/`Fallback` (variants discriminate) | none | `mapLanguageClaim` kind map `host.ts:164–178` → wrong variant |
| E1 | ANSI strip on HTML | TV | `quarto-api` `jupyter/to-markdown` (pure) | cell w/ ANSI codes → HTML out → **no raw ESC sequence** | none | `stripAnsiCode` call `to-markdown.ts:395,405` → raw ANSI leaks |
| E2 | `resultIncludes` widget (**optional**) | TV/RE | `jupyter/widgets` | widget-MIME cell → include emitted | none | producer → no include *(optional stretch; else accepted-untested)* |
| F1 | `intermediateFiles` round-trip | RE | `TsEngine::intermediate_files` + `behave` | `behave` returns intermediate files → assert they surface | none | sender `ts_engine.rs:827–860` → files absent |
| F2 | F-cancel (token flip) | RU (MockTransport) | `ts_process::request` | long execute + flip `Cancellation` → `Err(Cancelled)` prompt + `Cancel` sent + `poison_instance()` | transport | cancel-flip `ts_process.rs:693–701` → no `Cancelled` (template `:1723`) |
| F3 | F-relaunch (timeout→poison) | RE | `TsEngine` + `behave` `QUARTO_SLOW` | `execute:timeout:1` → `Err(Timeout)`+poison; 2nd execute Ok **+ fresh `LaunchEngine`/PID witness** | none | reconstruction `host.ts:600–628` (or poison `ts_engine.rs:805–815`) → 2nd fails |
| F4 | F-crash relaunch | RE | `TsEngine` + `behave` crash sentinel | `Deno.exit(1)` → `ProcessCrashed`; next execute relaunch **+ witness** | none | relaunch-after-crash path → next fails (crash-detect half = t13 `:908`) |
| F5 | deferred-deps round-trip | RE | `Dependencies` verb + `behave` | `behave` `dependencies:false` → drive `Dependencies` e2e → round-trip | none | TS handler `host.ts:796–877` → fails (serde leg = existing `test_fc2_…`) |
| F6 | FC-1 carriage | RE | `map_execute_result` + `behave` | `behave` populates **sentinel** FC-1 fields → survive verbatim into `TsExecuteResult` | none | field copies `ts_engine.rs:514–519` → sentinels lost (post_process leg pinned `:1692`) |

**Accepted-untested (logged, NOT revert-bound — loose smoke guards or not-yet-built; asserting the
limitation persists is forbidden per the Mechanism note):** `console.*` styling drop (guard: no
throw + msg reaches log) · `system.pandoc` happy path · `execProcess` knobs · `postProcessRestore-
PreservedHtml` stub (guard: fails loud — **not** asserting "throws" as the end state) ·
`path.dataDir(roaming)` no-op · `env.get`/`realPath` no-caller (record only) · jupyter preserve/
restore const-`false` · jupyter no-`pandoc?`-field · jupyter `NotImplemented` throwers (guard: fail
loud). These have **no named revert hunk by design** — they are gates against silent regression, not
bindings on a shipped behavior.

**Refactor-vacuity note.** The discriminators above are chosen to still differ across the states each
test distinguishes: B8 keys on the `alpha`/`beta` **name** (not a class-independent glyph); B4/B6
key on engine **absence vs presence**; F3/F4 add an explicit **relaunch witness** so "second execute
Ok" cannot pass a first-execute-that-never-poisoned; F6 uses **sentinel values** distinct from the
empty default. If a later refactor collapses any of these discriminators, move the discriminator to a
still-differing surface — do not migrate the expected value blind.

## Success Criteria

- [x] Every binding test in the plan matches a row in the **Test Seam Spec** above with its tier,
  real unit, seam, mock boundary, and **named revert hunk** filled in; each names the hunk in its
  revert comment (`fail-on-revert` discipline). Accepted-untested surfaces are logged there, not
  silently omitted.
- [x] Synthetic contending-engine fixtures exist and are Deno-gated like `echo_engine_e2e`.
- [x] Every resolution tier (T2/T3±/T4±, presence-gating, kind-dominates-priority,
  `whenClass`, static-vs-dynamic mismatch, **and the dynamic `claims_file` round-trip that drives
  `content-claim`**) has a binding test with a named revert. (Case-4 owned-but-unrunnable is
  **not** a 4b item — moved to Plan 4d as an engine self-enforcement test; see the removed-item
  notes in Phases 4b-A/4b-B.) Every Phase A fixture that ships has at least one binding assertion
  — **no orphan fixtures**.
- [x] `_quarto.yml engines:` project-key ordering is **implemented** and tested (RED→GREEN),
  with unregistered-name validation parity; Plan 1c's pointers updated to "done."
- [x] Every shipped-but-inert QuartoAPI / jupyter surface is documented `accepted-untested`
  (known v1 limitation, improvable later), with at most a loose positive guard (no throw, real
  output flows) — **no test asserts that a limitation must persist**. Surfaces with real
  behavior (claim constructors, lifecycle verbs) get genuine assertions.
  **Caveat (pre-existing, not introduced by 4b):** `quarto-api/src/jupyter/preserve.test.ts`
  (from Plan 3, not touched by Phase E) has hard `isPreservedHtml(...) === false` assertions that
  read as limitation-freezing under this plan's own discipline. Phase E correctly did not modify
  it or add new such assertions — flagged for the plan owner as a candidate cleanup, not a 4b gap.
- [x] `intermediateFiles` and the deferred-deps **wire** round-trip are exercised by synthetic
  engines; the book-feature-owned orchestrator consumer is recorded as out of scope, not tested.
- [x] Cancellation is covered by **three** bindings (per the Phase F split): **F-cancel** (token
  flip → `Cancelled` + poison, unit/MockTransport), **F-relaunch** (timeout → poison → transparent
  relaunch, real-Deno render path), and **F-crash** (crash → `ProcessCrashed` → relaunch, real-Deno).
  **Met, with a scope deviation on F-crash:** the crash binding is not test-only as originally
  framed — it required (and, per user decision 2026-07-08, got) a **production fix** to a real gap
  (poison guard didn't cover `ProcessCrashed`; the transport `OnceLock` couldn't reset). See the
  F-crash bullet above for the fix detail and the Execute-verb-only scope note.
- [x] The `--allow-all` posture is recorded as accepted-for-v1 and the `ts_process.rs:520`
  spawn site is annotated.
- [x] No regressions: `cargo nextest run --workspace`, `cd hub-client && npm run test:ci`,
  and `cargo xtask verify` all pass. **Verified 2026-07-08 at `06ee8bf7c`: `cargo xtask verify` →
  exit 0 ("All verification steps passed!"); Rust workspace 10657 passed / 198 skipped; all
  ts-packages + hub-client vitest suites green (WASM build included).**
