# Engine resolution & multi-engine ownership (design contract)

**Status:** design contract — authoritative for how q2 selects engines and
divides cells among them. The TS-engine-extension plans (grand plan,
plan1a-protocol, plan1a-engine, plan1c) reference this document for the
model and contain only their plan-specific work items.
**Created:** 2026-06-22 (during the TS-engine epic rebase onto post-multi-engine `main`).
**Related code:** `crates/quarto-core/src/engine/detection.rs`,
`crates/quarto-core/src/stage/stages/engine_execution.rs`,
`crates/quarto-core/src/engine/registry.rs`.
**Related strands:** bd-5yff4 (multi-engine, merged — this extends it),
bd-iq0hp / bd-8h3sn / bd-r8n4r (Carlos's multi-engine follow-ups — see §11).

---

## 1. Why this exists

The TS-engine epic was authored single-engine in April 2026. Between then and
the June rebase, three things landed on `main` that the April plans never saw:

- **Sequential multi-engine execution** (bd-5yff4, #238) — `engine: [a, b]`
  runs N engines in order; `EngineExecutionStage` is an N-engine loop with
  N+1 FileId slots; `detect_engine_sequence(meta) -> EngineSequence`.
- **Replay / capture / trace** (bd-45yw, bd-5qnj) — `ExecuteResult` is
  `Serialize`/`Deserialize`; `engine_captures: Vec<EngineCapture>`;
  `CaptureSpliceStage` folds captures for preview.
- **Discovery cache** (bd-c5u2g) — memoized binary-on-PATH lookup.

Carlos's multi-engine added the **mechanism** (loop, slots, capture, replay)
but deliberately left engine **coordination** unsolved — engines run on the
whole document and grab whatever they can, which is why `[knitr, jupyter]`
breaks (knitr's reticulate runs `{python}`, jupyter sees nothing; bd-iq0hp).
This document is the **coordination layer**: how q2 decides which engine(s)
run, and which engine runs which cell.

## 2. The two questions Q1 fused

Single-engine Q1 only ever answered one question. Multi-engine has two:

1. **Selection** — *which engines run* (the ordered, distinct sequence).
2. **Division** — *which engine runs which cell* (ownership).

Q1 fused them because one engine owned the whole document (cross-language was
internal, e.g. knitr's reticulate). The claim methods answer **both**: "engine
A claims language L" simultaneously means "A belongs in the sequence (if L is
present)" and "A owns L's cells." Explicit `engine:` preempts *selection* but
**not** division — division is a new axis, and the resolution tiers (§4) answer
it for explicit and implicit sequences alike.

## 3. The claim interface

### 3.1 Rust trait

`claims_language` returns a **kind-tagged claim**, not a bare priority. The
April `Option<i32>` is replaced because the multi-engine semantics need three
distinct roles that don't fit in a sign convention.

```rust
pub enum LanguageClaim {
    Primary(i32),   // I execute this. (default priority 1)
    Interop(i32),   // extend my ownership to this iff I'm already present. (default 0)
    Fallback(i32),  // universal kernel; jupyter's role, now declarable by any engine. (default 0)
    None,
}

fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> LanguageClaim {
    LanguageClaim::None
}
```

- **`kind` sets the resolution tier; `priority` orders only *within* a kind.**
  Kind dominates priority: `Primary(-100)` still beats `Fallback(100)` for the
  same language (a committed engine outranks a safety net regardless of
  numbers). Priority breaks ties among same-kind claimants; registry/`engines:`
  order is the final tiebreak.
- **`Interop` is presence-gated** (§4): it only fires for an engine already in
  the sequence via a positive claim — "extend if I'm already here," *not*
  "claim this anywhere." This is what preserves knitr's reticulate for an
  implicit `{r}`+`{python}` doc while not dragging knitr into a pure-`{python}`
  doc.

### 3.2 TS extension API (back-compatible widening)

The Q1 publication went `boolean` → `boolean | number`. We go one more:
`boolean | number | object`, with the object exposed to extension authors.

```ts
type LanguageClaim =
  | { kind: "primary";  priority?: number }
  | { kind: "interop";  priority?: number }
  | { kind: "fallback"; priority?: number };

claimsLanguage?: (language: string, firstClass?: string)
  => boolean | number | LanguageClaim | null;
```

Harness normalization (in `@quarto/engine-host-deno`, before crossing the
wire) — **no sign games**:

| return | → wire |
|---|---|
| `false` / `null` / `undefined` | `None` |
| `true` | `Primary(1)` |
| `number n` | `Primary(n)` — negative = low-priority primary, **never** interop |
| `{kind:"primary", priority?}` | `Primary(priority ?? 1)` |
| `{kind:"interop", priority?}` | `Interop(priority ?? 0)` |
| `{kind:"fallback", priority?}` | `Fallback(priority ?? 0)` |

`Interop` and `Fallback` are reachable **only** through the object. Legacy
engines could not have meant them (the concepts didn't exist), so a bare
`number` is always a `Primary`. This is the first deliberate Q1-API change in
the epic, justified by the multi-engine semantic shift; everything else stays
Q1-compatible.

### 3.3 Static claims (zero-load resolution)

Claiming can be declared **statically** in `_extension.yml`, so resolution
loads **no** TS engine — an engine is spawned only to *execute*, once it has
won ownership:

```yaml
contributes:
  engines:
    - path: julia-engine.js
      name: julia                 # complete static name (registration + YAML lookup, zero load)
      claims:
        julia: { kind: primary, priority: 1 }
        # reticulate-style:        r: { kind: primary }, python: { kind: interop }
        # first_class-conditional: python: { whenClass: marimo, kind: primary }
        # universal fallback:      fallback: { priority: 0 }
      file-extensions: [".jl"]     # valid_extensions — complete static (the pre-filter)
      # NOTE: julia does NOT declare `claims-files` — its `claimsFile` inspects
      # file content (isPercentScript / `# %%`), so it loads to decide. `.jl` in
      # `file-extensions` is the pre-filter; the precise file-claim is dynamic.
```

A static declaration is a **complete** replacement for its dynamic method
exactly when the engine's logic is a pure function of statically-known inputs;
otherwise it is a **superset pre-filter** that still loads to get the precise
answer:

| dynamic method | static form | complete when… | falls back to load when… |
|---|---|---|---|
| `name()` / registration | `name:` | declared | omitted (lazy alias map) |
| `valid_extensions()` | `file-extensions:` | always (it *is* the list) | — |
| `claims_file()` | `claims-files:` (unconditional) | extension-only logic | **content inspection** (e.g. Julia `# %%`) — *the one genuine must-load case* |
| `claims_language()` | `claims:` (kind/priority/`whenClass`) | language **and** `first_class` logic (both finite/known) | only genuine runtime/global-state logic |

**`first_class` is statically expressible — it is *not* a must-load case.**
`claims_language(language, first_class)` is a pure function of its two
arguments, so a `claims:` entry may carry `whenClass: <class>`: the claim then
applies **only** when the cell's first class equals `<class>` (absent
`whenClass` = any/no first class). A marimo engine therefore declares
`python: { whenClass: marimo, kind: primary }` and is **fully static** —
`{python .marimo}` → `Primary`, plain `{python}` / `{python .other}` → no
claim. (One rule per language key in v1; a multi-class engine would use a list,
deferred.) The **only** dynamic-method power that static resolution genuinely
cannot reach is **content-inspecting `claims_file`** (Julia's `isPercentScript`
reads the file's bytes for `# %%`) — that engine loads to decide, using
`file-extensions` as its pre-filter. Everything else — language, `first_class`,
kind/priority, fallback — is statically declarable.

A statically-declared claim used for resolution is validated against the
dynamic method **only if/when the engine loads to execute** (mismatch → hard
error, like the `name` check). Static claims are **authoritative for
resolution**; authors who declare them own their accuracy. `Fallback` cannot
be a finite language list, so a universal-fallback engine declares
`fallback:` rather than a per-language entry. Full-static resolution requires a
declared `name` (zero-load needs the name to place the engine in the
sequence). When *every* engine a project uses is fully static, resolution
loads nothing and can move to Pass-1 (§7) — that lift is the payoff of static
claims.

## 4. Resolution algorithm

`resolve_engines` is a **pure function** (§9) of merged metadata, the parsed
AST, the registry, and the file-claim engine. **If `claimed = Some(engine)`
the tiers below are skipped entirely — a claimed file resolves to that single
engine (§8, Q1-faithful).** The tiers run only for the implicit/explicit
`.qmd` path (`claimed = None`):

```
languages = computational languages of the doc        // §4.1
present    = explicitly-listed engines

T1 Primary:           per language, highest-priority Primary wins → owns it; add to `present`.
T2 explicit Fallback: language with no Primary → an explicitly-listed engine that returned
                      Fallback for it owns it (highest Fallback priority, then order).
T3 Interop:           still-unclaimed → highest-priority Interop among `present` engines.
T4 implicit Fallback: still-unclaimed computational → an engine that returned Fallback for it
                      (highest priority, then order). GATED: implicit sequences only (§4.3).

sequence  = distinct owners, in registry/`engines:` order.
ownership = language -> owning engine name.            // per-language (§4.2)
```

### 4.1 What counts as a "computational language"

Not a list — **structural**, from the parsed AST: the language of every
**executable** cell (a braced `{lang}` fence; pampa preserves the braces in
the class name), **minus** `HANDLED_LANGUAGES` (`ojs`/`mermaid`/`dot` — cell
handlers, not engines) and minus raw `{=fmt}` blocks. No allowlist, no kernel
registry. An empty set → no engine → markdown passthrough.

### 4.2 Per-language ownership; `first_class` drives *selection*

Ownership is keyed by **language**, not `(language, first_class)`. `first_class`
sharpens the *claim* (a marimo engine returns `Primary` for `{python .marimo}`,
`None` for plain `{python}`), so it influences **which engine is selected**, but
a language has **one owner**. This is because enforcement (§5) is per-language;
per-cell routing would require per-cell enforcement (a future possibility, §10).
A doc that mixes `{python}` and `{python .marimo}` wanting *different* engines is
a v1 limitation — the same limitation Q1 had (its single winner ran all cells of
a language).

### 4.3 jupyter is `Fallback(0)`; T4 is implicit-only

jupyter declares `Fallback(0)` for everything it is asked about (asked only
about the doc's actual executable, non-handler languages — it never
enumerates). Any engine can declare `Fallback` per-language; jupyter is just
the default universal one. **T4 fires only for implicit sequences** — an
explicitly-listed `engine:` never silently grows a non-listed fallback engine
(matching Q1's P4 gating: fallback only when nothing explicit/claimed
selected). An explicit `[knitr]` with a `{julia}` cell knitr can't run leaves
julia to the listed engine (best-effort), it does **not** add jupyter.

### 4.4 Worked cases

| Doc / sequence | non-R lang → owner | sequence | why |
|---|---|---|---|
| implicit `{r}`+`{python}` | python → knitr (reticulate) | `[knitr]` | T1 knitr→r; T3 knitr `Interop` python (present) |
| implicit `{r}`+`{sql}` | sql → knitr (`eng_sql`) | `[knitr]` | T1 knitr→r; T3 knitr `Interop` sql (present) |
| explicit `[knitr, jupyter]`, `{r}`+`{python}` | python → jupyter | `[knitr, jupyter]` | T1 knitr→r; **T2** jupyter (explicit `Fallback`) preempts knitr's `Interop`; knitr cedes python |
| explicit `[knitr, jupyter]`, `{r}`+`{sql}` | sql → jupyter ⚠️ | `[knitr, jupyter]` | same: **T2** explicit `Fallback` preempts `Interop`. But jupyter has no SQL kernel → **§10 loud failure** at execute (named, not silent) |
| implicit `{python}` only | python → jupyter | `[jupyter]` | T1–T3 ∅ (knitr not present); **T4** jupyter |
| implicit `{julia}` + Julia ext | julia → julia-ext | `[julia]` | T1 julia-ext `Primary(1)` |
| implicit `{julia}`, no ext | julia → jupyter | `[jupyter]` | T4 fallback — Q1 parity |
| implicit `{python}` + fallback ext `Fallback(5)` | python → ext | `[ext]` | T4 by priority (`5 > 0`), not by registration order |
| weak engine `Primary(-100)` vs jupyter `Fallback(0)` | weak engine | — | kind dominates priority |

**Consequence — explicit-`Fallback` preempts `Interop` for *every* non-primary
language, including ones the fallback engine can't execute.** The rule that
makes `python → jupyter` desirable in `[knitr, jupyter]` (T2 > T3) applies
uniformly: `sql`/`bash`/`sh` go to jupyter too, even though jupyter has no
kernel for them while knitr's `eng_sql`/`eng_bash` do. This is **intentional**
(the user explicitly listed jupyter as the non-R owner), **not** silent
breakage — the resolver is availability-blind by design (§4.3), and the
`sql`-with-no-kernel case is caught **loudly at execute** by §10's "owner
cannot execute an owned language" rule. The valuable common case (a
*knitr-only* `{r}`+`{sql}` doc → `sql → knitr`) is unaffected: `Interop` wins
when no explicit fallback is present. Authors who want `sql` to stay on knitr
simply omit jupyter from the explicit sequence.

## 5. Ownership enforcement (`handled_languages`)

**Single source of truth = the ownership map.** The sequence and every engine's
leave-alone set are *projections* of it; nothing is re-derived independently
(or they could drift). For engine *k*:

```
handled_languages(k) = HANDLED_LANGUAGES ∪ { lang : ownership[lang] != k }
```

threaded into execution via a new `ExecutionContext` field (and
`TsExecuteOptions.handled_languages` for TS engines).

- **knitr** already enforces: `execute.R` does `knit_engines$set(lang = <re-emit
  verbatim>)` for each `handled_languages` entry, which **replaces** knitr's
  default engine for that language — so adding `python` to knitr's set makes it
  re-emit `{python}` cells unexecuted (suppressing reticulate), and *not* adding
  it leaves reticulate intact. Reticulate-vs-handoff falls out of the same list.
  No new knitr mechanism — only the *population* changes from the static
  `[ojs, mermaid, dot]` constant to the per-engine ownership projection.
- **jupyter** has *no* `handled_languages` consumption today and runs every cell
  it's given. It needs **execution-time** enforcement (skip/re-emit cells whose
  language is in its leave-alone set) **when it is non-terminal** in a sequence
  (e.g. explicit `[jupyter, knitr]`). As the terminal/fallback engine it owns
  the remainder and never needs to cede. Its *claiming* is already correct;
  this is purely an execute-time gate.
- **TS engines** honor `handled_languages` by contract (pass through what they
  don't own), mirroring knitr.

These are three clocks, kept separate: **`claims_language` runs at
resolution** (recording/normal execution); **`handled_languages` enforces at
execute**; **replay touches neither** (§6).

## 6. Multi-engine execution & replay

### 6.1 Execution is sequential threading

Engines run **in order, each consuming the previous engine's output** — *not*
independently-then-stitched (`engine_execution.rs` loop):

```
ast = original AST
for engine in sequence:
    qmd = serialize_ast_to_qmd(ast)      // current AST, incl. prior engines' output
    result = engine.execute(qmd, ctx)    // ctx.handled_languages = leave-alone set for this engine
    ast = reconcile(ast, parse(result.markdown))
```

This is what enables handoff (engine A re-emits a `{python}` cell that engine B
executes) — and **why non-terminal engines must enforce `handled_languages`**:
without ceding, an earlier engine executes cells before they reach their owner.
Because knitr's cede re-emits at **top level**, ceded cells land where the next
engine (and the preview splicer) can reach them.

### 6.2 Replay drives from recorded captures, not re-resolution

On `main`, replay re-runs `detect_engine_sequence(meta)` and looks up
`ReplayEngine`s by name — fine when execution was explicit-only (meta fully
determined the sequence). But **implicit docs now resolve via claims, and
`ReplayEngine`s carry no claims**, so re-resolving during replay would produce
the wrong sequence. Therefore:

- **Replay iterates the recorded `engine_captures` in order** (they carry
  engine names + order); it does **not** call the resolver. It still
  serialize→reconcile-threads and validates `input_qmd` byte-equality as the
  determinism guard.
- **Ownership / `handled_languages` is a recording-time concern, baked into the
  recorded results** (knitr's recorded output already has the ceded `{python}`
  verbatim; jupyter's already executed). Replay is pure playback.
- This decouples replay from the resolution machinery entirely and likely lets
  `ReplayEngine` / `with_replay_many` be **replaced** by a capture-driven
  replay path — which also sidesteps injecting engines into the now-immutable
  `Arc<EngineRegistry>` (§ plan1a-engine / plan1c).
- **Freeze caveat:** "replay as freeze" must key invalidation on the **resolved
  engine set**, not only the input hash — installing an extension changes
  resolution while `input_qmd` still byte-matches. (Freeze is future; recorded
  here so its design accounts for it.)

Preview (`CaptureSpliceStage`) is the same story: resolution runs at *record*
time; the browser folds captures.

## 7. Pass placement

**Resolution runs in Pass 2 (Option A).** Resolving by language in Pass 1 would
`LoadEngine` expensive TS engines just to index docs that never run them. The
file-claim half (§8) *is* in Pass 1 (it must, to convert non-QMD input before
parse), but it only spawns an engine when a doc genuinely needs conversion, and
it must be inserted into **both** the full pipeline and the Pass-1 builder
(`pass1_profile_single_file_live`) — otherwise non-QMD docs get a garbage
`DocumentProfile`.

Because `resolve_engines` is pure, recording its result on the `DocumentProfile`
and lifting resolution to Pass-1 is a **zero-cost future move** (§10), enabled
once a project's engines are all fully-static (§3.3) so resolution loads
nothing.

## 8. File-claim semantics (Q1-faithful: claimed file → single engine)

`claims_file` → `markdown_for_file` runs pre-parse (Pass 1) and is the
converter. **A file claim resolves to that one engine, full stop** — exactly
Q1's `fileExecutionEngine`, which `return`s the first engine whose `claimsFile`
matches and never consults anything else (`engine.ts:320-325`, verified
2026-06-28). When `resolve_engines` is called with `claimed = Some(engine)` it
**short-circuits the tiers** and returns a single-engine resolution: that
engine is the whole `sequence`. No tiers, no seed, no native-language
inference. As the **sole** engine it is handed the whole converted document
(`handled_languages` = just `HANDLED_LANGUAGES`, the standard cell-handlers)
and **self-selects** — it runs the cells it recognizes and passes the rest
through. There is no ownership handoff, so **§10 case-4 does not apply** (case-4
is a *multi-engine* behavior — see below).

- **The `engine:` YAML inside a claimed file is ignored** — Q1's `claimsFile`
  match preempts the YAML-engine reader entirely (it is only reachable for
  `.md`/`.qmd`; `engine.ts:329-350`). q2 matches this: a claimed file does not
  consult its own front-matter `engine:`.
- **Non-executed languages pass through, they do not fail.** The claiming
  engine executes the cells it recognizes (its kernel/native language) and
  **passes the rest through unexecuted** — emitted as display code — exactly
  Q1, whose `quartoMdToJupyter` converts a non-kernel `{bash}` cell to a
  markdown cell that the kernel never runs (`core/jupyter/jupyter.ts:321-324`,
  verified 2026-06-28). This is **not** a §10 case-4 loud failure: **case-4 is
  gated on `|sequence| > 1`** (multi-engine, `engine: [knitr, jupyter]` +
  `{sql}` → jupyter, where the user *chose* the owner). A single-engine sequence
  — a claimed file *or* a `.qmd` resolving to one engine — runs what it can and
  passes the rest through, exactly Q1. **NB this is a landed-code change**: the
  enforcement in `engine/jupyter/text_execute.rs` (`partition_cells`) currently
  errors on *any* owned-but-unrunnable cell regardless of sequence length; it
  must gate the loud branch on multi-engine (single-engine → pass through).

**Why this replaced the "seed `Primary` + resolve" design (reverted
2026-06-28).** The earlier draft tried to make a claimed file participate in
multi-engine resolution by seeding the claiming engine as a synthetic `Primary`
and re-running the tiers — to let a stray `{bash}` cell reach a secondary
engine. That required the resolver to know the file's *native* language (which
it can't infer from an engine name alone), and the landed `resolution.rs`
silently never implemented the seed (it only marked the seed "present"),
leaving a real theft hole (a generic `Primary(1)` python extension would steal
a jupyter-`Fallback(0)` `.ipynb`'s cells). The single-engine rule is simpler,
removes that hole by construction, needs no native-language plumbing, and is
what Q1 actually does. Multi-engine remains a **`.qmd`-authoring** feature
(`engine: [a, b]`); converted non-`.qmd` files are single-engine.

## 9. Resolution as an artifact

```rust
// crates/quarto-core/src/engine/resolution.rs
pub struct EngineResolution {
    pub sequence:  Vec<DetectedEngine>,        // ordered, distinct owners
    pub ownership: HashMap<String, String>,    // language -> owning engine name
}
impl EngineResolution {
    pub fn handled_languages_for(&self, engine: &str) -> Vec<String>;  // §5
}
pub fn resolve_engines(
    meta: &ConfigValue, ast: &Pandoc, registry: &EngineRegistry, claimed: Option<&str>,
) -> EngineResolution;   // tiers, presence-gating, fallback ordering all live here
```

`EngineExecutionStage::run` calls it once and stashes `EngineResolution` on
`StageContext` (mirroring `project_index` in `run_pipeline`). The loop reads
`ownership` for each engine's `handled_languages`; the trace records
`sequence`. Benefits: the tier/presence/fallback logic is **unit-testable in
isolation** with mock claim tables (no subprocess); the trace can emit a
"resolved engines" entry; the Pass-1 lift is just "move the call, stamp the
profile." It is a function + `StageContext` artifact, **not** a pipeline stage
(it transforms no `PipelineData`).

## 10. Failure model (Q1 parity, with one deliberate q2 divergence)

Resolution is **availability-blind *and* capability-blind**: `resolve_engines`
picks owners purely by claim — a pure function of `(meta, ast, registry,
claimed)` (§9) — so **which engine owns which language is deterministic and
environment-independent.** This is load-bearing, not incidental: it is exactly
what lets resolution lift to **Pass-1** and stamp on `DocumentProfile`. An
eager "can this engine actually run language L?" probe would couple the chosen
sequence to whether a kernel happens to be installed on *this* machine, making
engine selection non-deterministic and un-liftable. So environment checks run
**after** resolution and **never change the chosen sequence** (never silently
re-route to a fallback — Q1 parity). Two distinct kinds, at two distinct times:

- **Availability** — is the owner's binary on PATH (`is_available()`)? Cheap;
  checked **after resolution, before execute** (cases 1–2).
- **Capability** — can the chosen owner actually *run* language L? An
  **execute-time** failure (cases 3–4): q2 starts kernels **lazily** inside
  `execute()`, and — more fundamentally — keeping capability out of resolution
  is what buys deterministic selection. **Deliberate divergence from Q1's eager
  kernel check (decided 2026-06-24):** a `[knitr, jupyter]`+`{sql}` doc runs
  knitr's `{r}` cells, *then* halts loudly at the `{sql}` cell — partial work
  before the halt, **traded for deterministic, Pass-1-liftable engine
  selection.** We do not add an eager capability probe.

- **Graceful (no error) — language-fallback axis:** a computational language no
  one claims → jupyter fallback; no executable cells → markdown. q2 already
  does this via the tiers.
- **Loud (halt render, actionable, Q1 message-style):**
  1. **No engine claims the file extension** — a non-QMD file whose extension
     is in no engine's `valid_extensions` → `"Can't determine execution engine
     for <file>"` (Q1 `engine.ts:317→366`). `.qmd`/`.md` always resolve.
     *(Resolution-time.)*
  2. **A resolved owning engine's runtime is missing** (`is_available()` false)
     — name the engine + what is missing + how to install (Q1: *"Unable to
     locate an installed version of R / Python 3…"*). No degradation.
     *(Availability, pre-execute.)*
  3. **Jupyter kernel not found** — name the missing kernel, list available,
     suggest `quarto check jupyter` (Q1 parity). *(Capability, execute-time —
     surfaces when the lazy kernel start fails.)*
  4. **A resolved owner cannot execute a language it owns** — the general case
     of (3), made explicit because the four-tier model can hand an engine a
     language it has no handler/kernel for (e.g. `[knitr, jupyter]` routes
     `{sql}` to jupyter via explicit-`Fallback`, but jupyter has no SQL kernel;
     §4.4 — note jupyter is **single-kernel-per-doc**, so it can only faithfully
     own its one kernel's language). The owner MUST fail loudly **at execute**
     (by design — see the capability note above) — a clear `ExecutionError`
     naming the engine **and** the language ("engine `jupyter` has no kernel
     for `sql`") — and MUST NOT silently skip the cell or emit it unexecuted.
     This is part of bringing the built-in engines up to the
     TsEngine/Quarto-API execution contract (see plan1a-engine's jupyter
     enforcement item). Silent no-op here would re-introduce exactly the kind
     of "cell quietly didn't run" failure the ownership model exists to prevent.
     **Not a forcible abort:** this error is a clean refusal (the engine never
     started computing), so it does **not** poison the instance — it behaves
     like `ExecutionFailed`, not `Cancelled`/`Timeout` (plan1a-engine poison
     policy).
     **Scope: case 4 is gated on `|sequence| > 1` (multi-engine only).** It
     fires only when the tiers / `engine:` list routed a language to an owner
     that is one of *several* engines — the case where silent-skip betrays the
     user's explicit composition. A **single-engine sequence** — a claimed file
     (§8) *or* a `.qmd` resolving to one engine — is handed the whole document
     and **self-selects**: it runs what it can and **passes the rest through
     unexecuted** (Q1's `quartoMdToJupyter` makes a non-kernel `{bash}` cell a
     display-only markdown cell — verified 2026-06-28), never a loud failure.
     This is full Q1 parity (Q1 is always single-engine and always passes
     through); case 4 is the deliberate q2 *multi-engine* divergence.
     **Landed-code consequence:** `engine/jupyter/text_execute.rs`'s
     `partition_cells` currently raises `NoHandlerForLanguage` for *any*
     owned-but-unrunnable cell regardless of sequence length — it must gate the
     loud branch on `|sequence| > 1`, ceding (passing through) in the
     single-engine case. The TS-engine "must error, never silently pass
     through" obligation likewise applies **only** to a TS engine that is a
     non-sole participant in a multi-engine sequence.
- **Multi-engine:** any unavailable **owner** in a sequence → fail the whole
  render loudly, naming the engine/language. Q1 never degrades; neither do we.

## 11. Relationship to Carlos's multi-engine follow-ups

- **bd-iq0hp** (multi-engine preview E2E) — **unblocked.** Its blocker is
  "knitr/jupyter don't compose cleanly (knitr claims python)." The ownership
  model is the principled fix (`[knitr, jupyter]` → r→knitr, python→jupyter via
  T2), and the epic also ships composable engines (echo, Julia). The browser
  test still needs writing; the composition blocker is gone.
- **bd-8h3sn** (cross-engine source attribution) — **shared fix, folded in.** TS
  engines inherit the same engine-2+ gap (their `source_map` references
  intermediate-slot FileIds). Thread the accumulating merged context into each
  engine's per-position `source_map`, designed once for built-in + TS.
- **bd-r8n4r** (nested-handoff splice) — **tangential, increased exposure.** Our
  cede mechanism re-emits at top level (within `splice_cells`' reach), so the
  nested-in-`Div.cell` case it tracks is not produced by ceding; but auto-split
  makes handoff more common, so it stays a live preview limitation.

## 12. Future possibilities (not in scope)

- **Per-cell routing.** The `claims_language` *interface* already supports it;
  it would change ownership granularity to per-cell and enforcement
  (`handled_languages`) to a per-cell skip set + stable cell identity through
  the threading round-trip. Reversible; least urgent of these three.
- **Pass-1 resolution.** Once every engine in a project is fully static (§3.3),
  resolution loads nothing and can run in Pass 1, recording `EngineResolution`
  on the `DocumentProfile` (profile-version bump) — making the index
  engine-aware (kernel pooling, freeze planning) and the freeze cache key
  correct (§6.2).
- **Project-level claim overrides.** Static claims as config opens
  `_quarto.yml` overriding an engine's claims (e.g. "knitr does not interop
  python here"). Deferred.

## 13. Verification items (during implementation)

- Full `cargo xtask verify` (not `--skip-hub-build`) — the new types
  (`LanguageClaim`, ownership map, `ExecutionContext` field,
  `ExecuteResult.html_dependencies`) live in `quarto-core` and feed
  `wasm-quarto-hub-client`; resolution must compile + degrade gracefully in
  WASM (markdown-only registry, execution bypassed by `CaptureSpliceStage`).
- `knit_engines` re-emit fidelity for attributed cells (`{python .marimo}` vs
  `{python}`) — relevant only if per-cell routing is ever adopted.
- `python.reticulate: false` honoring (now that ceding python is via
  `handled_languages`).
- Conversion-provenance: **faithful** original-file mapping is **deferred** (no
  current consumer) — plan1a-engine scopes `markdown_for_file` to "C′": the
  converted text is registered as an ephemeral intermediate file under an
  engine-reflecting synthetic name, giving honest provenance *into the converted
  buffer* (not back to the original non-QMD bytes). Plan 1c inherits only the
  claim/seed (§8) + this ephemeral-FileId registration, **not** a faithful
  remap. When a consumer needs converted-cell → source-cell positions, the
  preferred path is the "A′" generalized FileId-remap (extends the existing
  include/engine remap idiom); see plan1a-engine SEAM-3.
- `html_dependencies` survives the `EngineCapture` round-trip (`HtmlDependency`
  / `TextInclude` need `Serialize`/`Deserialize`).
