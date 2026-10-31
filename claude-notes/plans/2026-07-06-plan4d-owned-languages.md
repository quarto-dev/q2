---
title: 'Plan 4d: `owned_languages` — a positive engine-ownership wire field'
date: 2026-07-06
status: in-progress
description: 'Adds a positive `owned_languages` field to the engine-execute wire, so TypeScript engines can ask directly which languages q2 assigned to them instead of inferring this from the leave-alone `handled_languages` set.'
---

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** the shipping resolver + TS-engine wire (Plans 1a–c, 2, 3, 4). No new
machinery — this is a small, additive wire field. Inside the records epic, JOL-1
(Phases 4d-A/B) depends on nothing else in the epic, and JOL-4 (Phases 4d-C/D/E)
depends on JOL-1.
**Blocks:** nothing outside the records epic; purely additive. Per the dependency table
in [the epic](2026-10-28-execution-records-epic.md): JOL-1 blocks JOL-2b and JOL-4; JOL-4
blocks EXR-8 (hard), EXR-7 and EXR-9 (soft) and JOL-5.
**Lane:** B (`workspace-5`) for JOL-1 and JOL-4; run them under the epic\'s "Running the plans in four lanes" section.

**Subsumed (2026-10-28):** by [the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md), which executes as part of [the engine-execution-records epic](2026-10-28-execution-records-epic.md). Phases 4d-A/B become that epic's Phase 1 (JOL-1) and 4d-C/D/E its Phase 4 (JOL-4). The epic reverses this plan's earlier "q2 core does not read `owned_languages`" decision for the Jupyter engine (JOL-2b reads `ctx.owned_languages`), and the design decisions below are amended accordingly. The "Verification" and "Success Criteria" sections cover Phases 4d-A/B (JOL-1) only; the criteria that need the TypeScript side and the end-to-end checks sit in the Phase 4d-C/D/E (JOL-4) sections.
**Estimated sessions:** 1–2 (the Phase 4d-D case-4 self-enforcement test adds a new synthetic `owns-cant-run` fixture + committed `dist/` build)

> **For implementers:** this plan is TDD, task-by-task. Steps use `- [ ]` checkboxes.
> Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`.

## Goal

Add a **positive** per-engine ownership set — `owned_languages` (wire:
`ownedLanguages`) — alongside the existing `handled_languages` leave-alone set on
the engine-execute wire, so TS engine authors can ask *"did q2 give me language
L?"* directly (`ownedLanguages.includes("L")`) instead of inferring the complement
of a leave-alone set.

## Why this exists

Today q2 carries only the **leave-alone set** to engines — `handled_languages`,
produced by `EngineResolution::handled_languages_for`
(`crates/quarto-core/src/engine/resolution.rs:324`), defined as
`HANDLED_LANGUAGES ∪ { lang : ownership[lang] != engine }`: q2's built-in
cell-handler languages (`ojs`/`mermaid`/`dot`, `engine/mod.rs:123`) plus every
language this render assigned to a **different** engine. It is the *negative*
projection of the ownership map. The *positive* projection — the languages this
engine was actually chosen to own — is computed during resolution and then thrown
away; the engine never receives it.

`owned_languages` carries that positive projection down the wire. It is **unique,
expected information**: it says *why* q2 selected this engine (its resolved
ownership, honoring any user override via `engine:`/`engines:`), which cannot be
recovered unambiguously from `handled_languages` alone — "not handled" conflates
*owned by me* with *owned by nobody* (see Background). Completing the information
flow is the point; this is getting the q2 interface right, not fixing a bug.

**Engines do not strictly need it.** The engine contract is "process the
executable blocks you know how to process," full stop — an engine that ignores
`owned_languages` is still correct. The value is that an engine *can* now select
cells by ownership (`ownedLanguages.includes(lang)`) and stay automatically
consistent with q2's resolution, instead of selecting "everything not handled"
(which also sweeps in the ambiguous *owned-by-nobody* blocks) and then having to
reason about those blocks ad hoc.

> **On the marimo `bareSqlOwned` bug (plan4c FINDING #4):** that was an inverted
> `includes`/`!includes` on `sql` — a language that *was* present and resolved, so
> `handled_languages` alone was the correct input and this field would not have
> prevented it. It is **not** the motivation here; do not cite it as one.

## Background (researched — architecture facts)

Three facts establish that `owned_languages` is unique information, not a redundant
duplicate of `handled_languages`:

1. **Ownership is not total.** The resolver does **not** guarantee every present
   language gets an owner: a present-but-unclaimed language (e.g. explicit
   `engine: knitr` + a `{julia}` cell, T4 gated) is silently absent from the
   `ownership` map — no error, no default — and passes through as display code
   (T1–T4 are insert-only; test `test_t4_implicit_gate_explicit_knitr_no_jupyter`,
   `resolution.rs:1474`; design §4.3/§6.1/§10).
2. **`!handled.includes(L)` is therefore ambiguous** — it means "L owned by me
   **or** L owned by nobody," not "owned by me." With both projections a language
   is exactly one of \{`owned` = mine, `handled` = someone else's / a cell handler,
   neither = owned by nobody\}. `owned_languages` is the only signal that isolates
   the positive set. (The existing `handled_languages` wire doc comment overstates
   this as "coincide"; Phase 4d-D corrects it.)
3. **Resolution is one-shot and never re-run.** `resolve_engines` is called once
   over the original AST and cached (`engine_execution.rs:230`); engines run once
   each in the fixed sequence; an earlier engine's output is re-parsed and visible
   to later engines (`engine_execution.rs:461`) but ownership is **never**
   recomputed — a deliberate non-goal (design §6.1/§6.2: determinism + freeze
   cache-key). So injected/generated cells in never-resolved languages are the
   *owned-by-nobody* bucket **by design**; `owned_languages` makes that bucket
   legible but does **not** change its ratified pass-through fate.

### Carriage (verified) — where the parallel field must be threaded

The leave-alone set flows: `handled_languages_for` (`resolution.rs:324`) →
`EngineExecutionStage` calls it (`stage/stages/engine_execution.rs:552`, builder call at `:582`) →
`ExecutionContext.handled_languages` (`context.rs:104`, default seeding at `:189`, builder `:242`) →
`TsEngine::execute` copies it onto the wire (`ts_engine.rs:560`) →
`TsExecuteOptions.handled_languages` (`ts_protocol.rs:391`, `#[serde(rename_all =
"camelCase")]` → `handledLanguages`) → Deno host `JSON.parse ... as` (`framing.ts:119`)
→ `TsExecuteOptions` TS interface (`types.ts:185`) → engine `execute()` (`host.ts:680`).
`owned_languages` rides the exact same rails, one hop at a time.

### Backward-compatibility (verified)

**Additive and non-breaking.** The field is send-only (Rust → engine). No consumer
deserializes `TsExecuteOptions` with `#[serde(deny_unknown_fields)]` (absent), and the
Deno host does a bare `JSON.parse(line) as Request` with **no runtime schema**
(`framing.ts:119`), so any already-built engine bundle (the committed `dist/*.js`
fixtures) silently ignores the new `ownedLanguages` key. Adding `#[serde(default)]` on
the Rust field keeps every deserialize path (round-trip tests) tolerant of its absence.

## Design decisions (resolved — do not re-litigate)

- **Name:** `owned_languages` / `ownedLanguages`. Matches the existing `ownership`
  vocabulary in `resolution.rs`. Rejected `won_languages` (cute, non-idiomatic).
- **Scope:** populate `ExecutionContext.owned_languages` for every engine (it is the
  shared carrier) but forward it only on the **TS-engine wire** (Phase 4d-B adds the Rust
  wire field, `TsExecuteOptions.owned_languages`, `ts_protocol.rs:361/391`). This plan
  changes no native engine: `subprocess.rs` and `hooks.R` stay untouched, and knitr keeps
  reading `handled_languages`. The Jupyter engine does read the carrier, in JOL-2b, where
  `ctx.owned_languages` replaces its language allowlist; that change is not part of this
  plan.
- **Membership:** `{ lang : ownership[lang] == engine }`, sorted — only languages
  **present in this render** that q2 assigned to this engine. An engine that owns
  nothing this render gets `[]`. This is the exact complement of `handled_languages_for`
  restricted to present languages.
- **Reflects *resolved* ownership, not static extension claims.** If the user
  reordered/overrode claims via `engine:`/`engines:`, `owned_languages` shows the
  effective decision — that is the point: the engine sees what q2 actually chose.
- **Informational on the wire, not enforcement.** Phases 4d-A/B change no resolution or
  partition logic, and `handled_languages` remains the execute-time gate for TS engines
  (cede / re-emit). JOL-2b later makes `owned_languages` the Jupyter engine\'s execution
  gate (its own change, in `partition_cells`).
- **`handled_languages` is retained unchanged** (Q1 compatibility; native consumers
  and the R hooks read it). This plan **adds**, it does not deprecate or remove.
- **Who reads `owned_languages`.** Phases 4d-A/B only produce and carry it. On the TS wire
  it is engine-facing convenience; inside q2, the Jupyter engine reads
  `ctx.owned_languages` from JOL-2b onward. No other q2 core code reads it.

## Open questions (none blocking)

- Should the marimo/echo fixtures be *rewritten* to consume `ownedLanguages` instead of
  the complement inference? **Not in this plan.** Phase 4d-D adds one positive assertion
  proving the field arrives correct; migrating real fixtures is optional follow-up so this
  plan stays a pure carriage addition.

---

## Phase 4d-A: Rust — produce the positive set (resolver + context)

**Files:**
- Modify: `crates/quarto-core/src/engine/resolution.rs` (add `owned_languages_for`; new unit test near `test_handled_languages_for` at `:1549`; the test helpers `mock_registry`, `engine_cell`, `map_config`, `array_config`, `string_config` are at `:1120-1200`)
- Modify: `crates/quarto-core/src/engine/context.rs` (add field `:104` region + builder `:242` region)
- Modify: `crates/quarto-core/src/stage/stages/engine_execution.rs` (wire at `:552`)

**Interfaces produced:**
- `EngineResolution::owned_languages_for(&self, engine: &str) -> Vec<String>`
- `ExecutionContext.owned_languages: Vec<String>` + `ExecutionContext::with_owned_languages(self, Vec<String>) -> Self`

- [x] **Step 1: Write the failing test** — append to the `tests` module in `resolution.rs` (mirrors `test_handled_languages_for`\'s setup at `:1549`):

```rust
/// `owned_languages_for` returns { lang present in doc : owned by this engine }.
#[test]
fn test_owned_languages_for() {
    let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
    let ast = ast_with_blocks(vec![
        engine_cell("r"),
        engine_cell("python"),
        engine_cell("sql"),
    ]);
    // Explicit [knitr, jupyter]: r→knitr (T1), python→jupyter (T2), sql→jupyter (T2).
    let meta = map_config(vec![(
        "engine",
        array_config(vec![string_config("knitr"), string_config("jupyter")]),
    )]);

    let res = resolve_engines(&meta, &ast, &registry, None);

    // Positive set: only this engine's owned, present languages.
    assert_eq!(res.owned_languages_for("knitr"), vec!["r".to_string()]);
    assert_eq!(
        res.owned_languages_for("jupyter"),
        vec!["python".to_string(), "sql".to_string()]
    );
    // HANDLED_LANGUAGES base is NOT in the owned set (it is not "owned" by anyone).
    assert!(!res.owned_languages_for("knitr").contains(&"ojs".to_string()));
    // Complement invariant: for present languages, owned ∩ handled == ∅.
    let knitr_handled = res.handled_languages_for("knitr");
    for l in res.owned_languages_for("knitr") {
        assert!(
            !knitr_handled.contains(&l),
            "owned and handled must be disjoint: {l}"
        );
    }
}
```

- [x] **Step 2: Run it, verify it fails**

Run: `cargo nextest run -p quarto-core owned_languages_for`
Expected: FAIL — `no method named owned_languages_for`.

- [x] **Step 3: Implement `owned_languages_for`** — add to `impl EngineResolution` in `resolution.rs`, directly after `handled_languages_for` (`:324-335`):

```rust
/// Compute the **positive ownership set** for `engine`: every language present
/// in this render that q2 assigned to `engine`.
///
/// ```text
/// { lang : ownership[lang] == engine }
/// ```
///
/// The exact complement of [`handled_languages_for`] restricted to present
/// languages, and **sorted** for deterministic output. Unlike the leave-alone
/// set, an engine's own name *is* the membership test — engines can read this
/// directly instead of inferring the complement. It is carried to TS engines
/// for engine-author convenience (Plan 4d) and read by the Jupyter engine (JOL-2b).
pub fn owned_languages_for(&self, engine: &str) -> Vec<String> {
    let mut result: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (lang, owner) in &self.ownership {
        if owner.as_str() == engine {
            result.insert(lang.clone());
        }
    }
    result.into_iter().collect()
}
```

- [x] **Step 4: Run it, verify it passes**

Run: `cargo nextest run -p quarto-core owned_languages_for`
Expected: PASS.

- [x] **Step 5: Add the `ExecutionContext` carrier** — in `context.rs`, add the field immediately after `handled_languages` (`:104`):

```rust
    /// Languages present in this render that q2 assigned to **this** engine
    /// (the positive complement of `handled_languages`). Populated by
    /// `EngineExecutionStage` via `EngineResolution::owned_languages_for`.
    /// Defaults to empty (direct-construction tests do not set it). Carried to TS
    /// engines on the wire (Plan 4d) and read by the Jupyter engine (JOL-2b).
    pub owned_languages: Vec<String>,
```

Ensure the `Default`/constructor that initializes `handled_languages` (the
`HANDLED_LANGUAGES`-seeding block at `:189`) also initializes
`owned_languages: Vec::new()`. Then add the builder after `with_handled_languages`
(`:242-245`):

```rust
    /// Set the positive owned-language set for this engine.
    ///
    /// Computed by `EngineResolution::owned_languages_for(engine_name)`.
    pub fn with_owned_languages(mut self, languages: Vec<String>) -> Self {
        self.owned_languages = languages;
        self
    }
```

- [x] **Step 6: Wire it in `EngineExecutionStage`** — in `engine_execution.rs`, beside the existing `handled_languages` computation (`:552`) and its `.with_handled_languages(...)` (`:582`):

```rust
            let handled_languages = resolution.handled_languages_for(engine.name());
            let owned_languages = resolution.owned_languages_for(engine.name());
```

and chain the builder call next to `.with_handled_languages(handled_languages)`:

```rust
            .with_handled_languages(handled_languages)
            .with_owned_languages(owned_languages)
```

- [x] **Step 7: Build, commit**

Run: `cargo build -p quarto-core`
Expected: clean.

```bash
git add crates/quarto-core/src/engine/resolution.rs \
        crates/quarto-core/src/engine/context.rs \
        crates/quarto-core/src/stage/stages/engine_execution.rs
git commit -m "feat(engine): compute positive owned_languages set beside handled_languages (Plan 4d-A)"
```

---

## Phase 4d-B: Rust wire — `TsExecuteOptions.owned_languages`

**Files:**
- Modify: `crates/quarto-core/src/engine/ts_protocol.rs` (struct `:361`, test builder `:508-518`, camel-case test `:1223`, dependencies-default test `:1468`)
- Modify: `crates/quarto-core/src/engine/ts_engine.rs` (execute-options construction `:560`)

**Interfaces produced:**
- `TsExecuteOptions.owned_languages: Vec<String>` (wire key `ownedLanguages`)

- [x] **Step 1: Extend the camel-case wire test** — in `ts_protocol.rs`, `test_ts_execute_options_camel_case` (`:1223`) currently asserts `j["handledLanguages"] == json!([])`. Add, right below it:

```rust
        assert_eq!(j["ownedLanguages"], json!([]));
```

- [x] **Step 2: Run it, verify it fails**

Run: `cargo nextest run -p quarto-core test_ts_execute_options_camel_case`
Expected: FAIL — `ownedLanguages` is `Null` (field absent).

- [x] **Step 3: Add the wire field** — in `TsExecuteOptions` (`ts_protocol.rs:361`), immediately after `handled_languages` (`:391`):

```rust
    /// The **positive** owned-language set for this engine — every language
    /// present in this render that q2's resolution assigned to it
    /// (`ownedLanguages` on the wire). The positive projection of the ownership
    /// map, disjoint from [`Self::handled_languages`] over present languages: an
    /// engine may read `ownedLanguages.includes("L")` directly to select the
    /// cells it owns, instead of inferring them as the complement of the
    /// leave-alone set (which conflates "owned by me" with "owned by nobody").
    /// **Informational, not enforcement** — `handled_languages` remains the
    /// execute-time gate. Lists only Pass-1-resolved languages, so a language
    /// that appears only in a *generated* cell is owned by nobody and is absent
    /// here (a ratified pass-through, design §6.1 — not a re-resolution feature).
    /// `#[serde(default)]` so an absent key deserializes as `[]`. Added by Plan 4d.
    #[serde(default)]
    pub owned_languages: Vec<String>,
```

- [x] **Step 4: Fix the two construction sites** — the test builder `make_execute_options` (`ts_protocol.rs:517`, beside `handled_languages: vec![]`):

```rust
            handled_languages: vec![],
            owned_languages: vec![],
```

and the production builder in `ts_engine.rs` (`:560`, beside `handled_languages: ctx.handled_languages.clone()`):

```rust
            handled_languages: ctx.handled_languages.clone(),
            owned_languages: ctx.owned_languages.clone(),
```

- [x] **Step 5: Update the other wire fixture** — `test_fc2_execute_options_dependencies_default_true` (`ts_protocol.rs:1468`) builds a JSON blob with `"handledLanguages": []` (`:1486`). Add `"ownedLanguages": []` beside it so the round-trip stays exact. (With `#[serde(default)]`, omitting it also works — but keep the pinned shape explicit.)

- [x] **Step 6: Run the wire tests, verify they pass**

Run: `cargo nextest run -p quarto-core ts_protocol`
Expected: PASS (all).

- [x] **Step 7: Commit**

```bash
git add crates/quarto-core/src/engine/ts_protocol.rs crates/quarto-core/src/engine/ts_engine.rs
git commit -m "feat(engine): carry ownedLanguages on the TS execute wire (Plan 4d-B)"
```

---

## Phase 4d-C: TypeScript — types + host passthrough + wire-parity

**Files:**
- Modify: `ts-packages/quarto-types/src/execution.ts` (author-facing `ExecuteOptions`, `:51`)
- Modify: `ts-packages/quarto-engine-host-deno/src/types.ts` (wire `TsExecuteOptions`, `:185`)
- Modify: `ts-packages/quarto-engine-host-deno/src/host.ts` (assemble `ExecuteOptions`, `:680`)
- Modify: `ts-packages/quarto-engine-host-deno/src/wire-parity.deno-test.ts` (pin the new field)

- [ ] **Step 1: Add to the wire type** — in `quarto-engine-host-deno/src/types.ts`, `TsExecuteOptions` (`:185`), after `handledLanguages`:

```ts
  handledLanguages: string[];
  ownedLanguages: string[];
```

- [ ] **Step 2: Add to the author-facing type** — in `quarto-types/src/execution.ts`, the `ExecuteOptions` interface (`:51`), after `handledLanguages`:

```ts
  handledLanguages: string[];
  /** Positive set: languages present in this render that q2 assigned to THIS
   *  engine. The complement of `handledLanguages`; read
   *  `ownedLanguages.includes("L")` to ask "did q2 give me L?". (Plan 4d) */
  ownedLanguages: string[];
```

- [ ] **Step 3: Thread it through the host** — in `host.ts`, the `executeOptions` assembly (`:680`), after `handledLanguages: opts.handledLanguages`:

```ts
              handledLanguages: opts.handledLanguages,
              ownedLanguages: opts.ownedLanguages,
```

- [ ] **Step 4: Extend the wire-parity pin** — in `wire-parity.deno-test.ts`, wherever the `TsExecuteOptions` shape is asserted (the `assertEquals` block), add `ownedLanguages: []` to the expected object so Rust↔Deno shape parity stays enforced. If the test builds its input from a Rust-emitted fixture, regenerate/extend that fixture to include the key.

- [ ] **Step 5: Build + test the TS packages**

Run (from repo root): `npm run build -w ts-packages/quarto-types -w ts-packages/quarto-engine-host-deno`
Then: `cd ts-packages/quarto-engine-host-deno && deno test src/wire-parity.deno-test.ts`
Expected: build clean; wire-parity PASS.

> Note: `quarto-types/dist/execution.d.ts` is generated — rebuilding regenerates the
> `ownedLanguages` line at `dist/execution.d.ts:38`. Do not hand-edit `dist/`.

- [ ] **Step 6: Commit**

```bash
git add ts-packages/quarto-types/src/execution.ts \
        ts-packages/quarto-engine-host-deno/src/types.ts \
        ts-packages/quarto-engine-host-deno/src/host.ts \
        ts-packages/quarto-engine-host-deno/src/wire-parity.deno-test.ts
git commit -m "feat(ts): pass ownedLanguages through the Deno host to engines (Plan 4d-C)"
```

**Phase 4d-C verification (JOL-4):** `cd ts-packages/quarto-engine-host-deno && deno test`
(wire-parity) is green.

---

## Phase 4d-D: End-to-end positive assertion + doc cross-reference

Prove the positive set arrives at a real engine correct, and cross-link the two fields
so the next reader sees both. This is the end-to-end check (per repo policy, unit round-trips
are necessary but not sufficient).

**Files:**
- Modify: `crates/quarto-core/tests/integration/echo_engine_e2e.rs` (add one Deno-gated positive assertion + the Step-2b negative test)
- Create: `crates/quarto-core/tests/fixtures/extensions/owns-cant-run/` (synthetic engine for the Step-2b case-4 self-enforcement test)
- Modify: `crates/quarto-core/src/engine/ts_protocol.rs` (add a cross-ref line to the `handled_languages` doc comment `:370`)

- [ ] **Step 1: Add a Deno-gated e2e assertion** — extend an existing multi-language `echo_engine_e2e.rs` render (or add one) so echo-engine is asked to echo back `options.ownedLanguages`, and assert it equals the language(s) q2 assigned to it (not the leave-alone set). Mirror the existing `deno_available()` skip guard (`echo_engine_e2e.rs:38-43`, early-return, not `#[ignore]`). If echo-engine's fixture source does not already surface options back, add a minimal echo of `ownedLanguages` to its `src/echo-engine.ts` and rebuild its committed `dist/echo-engine.js` with `q2 build-ts-extension`.

- [ ] **Step 2: Run it**

Run: `cargo nextest run -p quarto-core --test integration echo_engine`
Expected: PASS (or SKIP if deno absent — record which in the transcript).

- [ ] **Step 2b: Case-4 self-enforcement negative test (owned-but-unrunnable, driven by `owned_languages`).**
  This is the in-scope replacement for Plan 4b's removed `claims-cant-run` fixture / case-4 row. q2 does
  **not** police engine execution and has **no** static capability model, so owned-but-unrunnable is the
  engine's own §10 contract obligation — and `owned_languages` is precisely what lets a TS engine
  self-enforce it *cleanly*, distinguishing "mine" from "owned by nobody" (which `handled_languages`
  alone cannot). What this test proves is engine-behavior + wire-field carriage, **not** any q2-side
  partition check.
  - Add a minimal synthetic fixture `owns-cant-run` (echo-engine-shaped, Deno-gated, committed
    `dist/…js` built with `q2 build-ts-extension`): it statically `Primary`-claims language `synth` (so
    q2 assigns it ownership) but handles **no** computational language. Its `execute` reads
    `options.ownedLanguages`; if any entry falls outside its (empty) capability set it throws a clear
    `owns "<L>" but cannot run it` error.
  - **Bind the test to the wire field.** The failure must key on `ownedLanguages.includes(...)`, so the
    test reddens if `owned_languages` is not carried — i.e. it is a genuine assertion about Phase 4d-B/C,
    not about cell contents. (Do not have the engine scan cells to reach the error; drive it from the
    positive set.)
  - Render a doc with a `{synth}` cell through the binary; assert the render fails with the engine's
    self-enforcement error naming `synth`. Deno-gated skip exactly like Step 1.
  - **Do NOT** assert any q2-*side* case-4 behavior: q2 forwards `owned_languages` and the engine decides.
    Building a q2-side declaration-driven capability check would be a new capability model, explicitly out
    of scope for this epic.

- [ ] **Step 3: Correct the overstated `handled_languages` doc comment + cross-reference** — the block at `ts_protocol.rs:370-390` currently claims q2 "assigns every language present in the document an owner, or hard-fails … so 'L absent from this set' and 'L owned by me' coincide." That is **false** (present-but-unclaimed languages are silently unowned — Background fact 1). Replace that soundness sentence with the accurate statement and point at the new field:

```rust
    /// NOTE: "absent from this set" means *owned by me OR owned by nobody* — a
    /// present-but-unclaimed or execute-time-injected language is owned by no
    /// engine and is absent here too (design §6.1, pass-through). To ask
    /// unambiguously "did q2 assign me L?", read `owned_languages` (the positive
    /// projection), not the complement of this set.
```

- [ ] **Step 4: Commit**

```bash
git add crates/quarto-core/tests/integration/echo_engine_e2e.rs \
        crates/quarto-core/src/engine/ts_protocol.rs \
        crates/quarto-core/tests/fixtures/extensions/echo-engine/ \
        crates/quarto-core/tests/fixtures/extensions/owns-cant-run/
git commit -m "test(engine): e2e-verify ownedLanguages reaches an engine + case-4 self-enforcement; cross-ref docs (Plan 4d-D)"
```

**Phase 4d-D success criteria (JOL-4):**

- [ ] `ownedLanguages` is carried end-to-end (Rust context → wire → Deno host → engine),
  with wire-parity and camel-case round-trips pinning it; the exact `echo_engine_e2e`
  assertion and the observed `ownedLanguages` value (the positive set, not the leave-alone
  set) are recorded in the transcript or this plan.
- [ ] Case-4 self-enforcement (Step 2b): a synthetic engine that owns a language it
  cannot run detects it via `ownedLanguages` and errors loudly, with the test bound to the wire
  field (reddens if `owned_languages` is not carried). No q2-side capability check is added.
- [ ] The two wire fields cross-reference each other in the doc comments, and the
  overstated `handled_languages` "coincide" claim is corrected.
- [ ] Before the push: `cargo xtask verify` is green (this touches `quarto-core` wire types
  the WASM client depends on, so the full hub build must pass; do **not**
  `--skip-hub-build`). Recommended for Gordon: `cargo nextest run --workspace`.

---

## Phase 4d-E: reconcile design docs with the implementation

The two design docs were updated **ahead of** implementation (the session that
authored this plan) to describe what 4d delivers. The action item here is to
**review them against the landed code and fix any drift** — do not assume they are
still correct after Phases A–D.

**Files:**
- Review/modify: `claude-notes/designs/engine-resolution.md` (§5 positive projection, §9 `owned_languages_for` accessor, §13 verification item)
- Review/modify: `claude-notes/designs/engine-api-surface.md` (the `ExecuteOptions → TsExecuteOptions` table row for `ownedLanguages`)

- [ ] **Step 1: Reconcile `engine-resolution.md`.** After Phases A–D land, re-read §5/§9/§13 and confirm: the `owned_languages(k) = { lang : ownership[lang] == k }` formula matches `owned_languages_for` as implemented; the §9 accessor signature matches the real method; the §13 disjointness claim holds. Fix any mismatch (e.g. if the accessor name/signature or field name changed during implementation).
- [ ] **Step 2: Reconcile `engine-api-surface.md`.** Re-read the `ExecuteOptions → TsExecuteOptions` table and confirm the `owned_languages` row's wire name (`ownedLanguages`), class (`q2-native`), and description match the shipped `TsExecuteOptions` field and the TS `ExecuteOptions`/`TsExecuteOptions` types.
- [ ] **Step 3: Commit** any doc corrections.

```bash
git add claude-notes/designs/engine-resolution.md claude-notes/designs/engine-api-surface.md
git commit -m "docs(design): reconcile engine-resolution + api-surface with owned_languages as landed (Plan 4d-E)"
```

**Phase 4d-E success criterion (JOL-4):**

- [ ] `engine-resolution.md` (§5/§9/§13) and `engine-api-surface.md` describe
  `owned_languages` as landed — reviewed against the implementation, not just the
  pre-written text.

---

## Verification (Phases 4d-A/B)

- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings`.
- [ ] `cargo nextest run -p quarto-core`, including the touched tests by name:
  `owned_languages_for` (Phase 4d-A) and `test_ts_execute_options_camel_case` plus the
  `ts_protocol` tests (Phase 4d-B; they live in `quarto-core`, so no other crate needs a
  separate `-p` run).
- Recommended pre-push step, for Gordon to decide (not a gate): `cargo nextest run
  --workspace`. The Deno tests, `cargo xtask verify` and the recorded end-to-end example
  belong to JOL-4 (Phases 4d-C/D/E).

## Success Criteria (Phases 4d-A/B)

- [ ] `EngineResolution::owned_languages_for` returns the sorted positive ownership set,
  disjoint from `handled_languages_for` for present languages, with a binding unit test.
- [ ] `ExecutionContext.owned_languages` is populated by `EngineExecutionStage` and
  `TsExecuteOptions.owned_languages` carries it on the Rust wire, pinned by the camel-case
  round trip.
- [ ] Backward-compatible: `#[serde(default)]` on the Rust field; no schema tightening;
  existing engine bundles ignore the new key (verified: echo/marimo fixtures unchanged pass).
- [ ] `handled_languages` is unchanged, and no native engine is changed by this plan (the
  Jupyter engine\'s own read of `ctx.owned_languages` is JOL-2b).
