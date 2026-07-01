# Plan 6 — Pass-1 engine resolution (per-doc lift)

**Status:** research stub — not yet designed in depth. **Created:** 2026-06-29.
**Sequence:** post-Plan-1c; **additive** on top of the shipping 1a stack (which
resolves in Pass-2 and works without any of this). Orthogonal to Plans 3/4/5.
**Depends on:** the resolution machinery landing — `resolve_engines` +
`EngineResolution` (plan1a-engine), the static-claims `_extension.yml` parsing
(plan1c D1), and the Pass-2 stage wiring (plan1c) — plus `DocumentProfile` +
the two-pass orchestrator (already on `main`).

## Driver

Carlos prefers **engine resolution to happen in Pass 1** (the indexing pass,
before render) rather than Pass 2. The TS-engine design was built *toward* this
from the start — `resolve_engines` is a **pure function** of `(meta, ast,
registry, claimed)` and is deliberately **availability- and capability-blind**
(`engine-resolution.md` §9, §10) precisely so the result is deterministic,
environment-independent, and stampable on `DocumentProfile`. §7 calls the lift a
"zero-cost future move," and §3.3 names the static `_extension.yml` claims as
"the precondition for the Pass-1 resolution lift."

The catch the original design left as all-or-nothing: §7/§3.3/§12 gate the lift
on **every** engine in the project being *fully static*. We still want
**Q1-compatible engines that have not yet declared static claims** to work — a
`path`-only TS extension is a "legacy Q1 engine" that needs a `LoadEngine`
(module `import()`) to answer `claims_*` (plan1c D1, lines 152-165). This plan
researches a **per-doc partial lift** so a project can mix static and non-static
engines and still get Pass-1 resolution for the docs that don't need a load.

## Why "nice" is achievable (the cost reality)

The expensive engine cost (Julia control server / Jupyter kernel, **seconds**)
is lazy inside `execute()`. Resolving a non-static TS engine costs only its
`LoadEngine` **import (~10-50 ms)** — never launch/execute. §7's objection
("don't `LoadEngine` expensive engines merely to index") is really "don't pay
that import × every-doc for an irrelevant engine," which the **hint pre-filter**
(plan1a-engine) already mitigates. So the obstacle is bounded, and the
file-claim path already does a static/dynamic split *at Pass-1 today* (static
`claims-files` resolve free; content-inspecting `claims_file` like Julia's
`# %%` loads) — Plan 6 generalizes that precedent from file-claims to
language-claims.

## Design direction (to be validated)

Replace the project-wide "every engine static" gate with a **per-doc
"resolution provably needs no load"** test:

- Resolve in Pass-1 when, for the doc's computational languages (§4.1), every
  contending engine is either **static** (zero-load `claims`) or **eliminable
  without loading** — ruled out by its static hint (`name` /
  `file-extensions` / superset language hint doesn't match) **or** dominated by
  a static higher-tier claim (kind dominates priority, §3.1: a static `Primary`
  beats any `Interop`/`Fallback` a non-static engine could declare, so we need
  not load it to know it loses) **or** fully specified by a metadata claim
  override (§"Two static-metadata inputs" below), which needs no load at all.
- Otherwise **fall through to Pass-2** for that doc only — exactly today's path.

Two shapes for the genuinely-contested case (a non-static engine that can't be
ruled out), to be chosen **per consumer**:

- **(A) Load the contested engines at Pass-1** — bounded cost (imports only;
  daemon still lazy), full resolution, profile fully stamped. Required by
  consumers that need the **complete** resolved set — **freeze** (the §6.2
  cache key must hash the resolved engine set) and kernel pooling (Plan 5).
- **(B) Stamp a partial `EngineResolution`** (static portion resolved + a
  "pending" set) and complete it in Pass-2 where the load happens anyway.
  Cheaper, but only safe for consumers that tolerate an incomplete set.

## Two static-metadata inputs that keep resolution load-free

Both keys below are **static metadata** consumed by `resolve_engines` (which
already takes `meta`). Because they are static, they preserve the pure-function
property (§9) — no engine load, no runtime re-resolution — so they *extend* the
set of docs that resolve completely at Pass-1. That is why they belong in Plan 6:
each adds a third satisfier to the "resolution provably needs no load" test —
*"…or the metadata itself fully specifies it."* (Design decisions ratified with
Gordon 2026-07-02.)

### 1. Claim overrides in `engine:` (Carlos's syntax)

Let the merged metadata declare language claims per engine, overriding the
static/dynamic claim table:

```yaml
engine:
  - knitr:
      claims:
        r: primary            # or { kind: primary, priority: 2 }
        sql: interop
        python: interop
  - jupyter:
      claims: [r]             # list shorthand → Primary(1) each
```

- **This is `engine-resolution.md` §12's "project-level claim overrides,"** now
  requested for real. It's the *static portion* of language claiming, expressed
  as config instead of engine code.
- **Model (settled 2026-07-02):** an override just **edits the claim table the
  tiers consume**; *both* selection and division fall out of the resulting
  ownership map (the sequence is "distinct owners" — `resolution.rs:544-547`).
  There is no separate "listing selects / claims divide" split — one lever.
- **Schema:** reuse the `_extension.yml` claim schema and its parser
  (`parse_claims_map` / `parse_static_language_claim` in `extension/read.rs`) —
  one claim schema in three places (`_extension.yml`, `_quarto.yml`, doc
  frontmatter). Widen it to accept the **list shorthand** (`[a, b]` → `Primary(1)`
  each, matching §3.2's `true`/number normalization) in addition to the
  per-language map + top-level `fallback:`.
- **Authoritative, NOT validated (the one real divergence from `_extension.yml`
  claims).** §3.3 validates an engine author's static claims against the dynamic
  method on first load. A metadata override is the *user* deliberately
  reassigning ownership (`jupyter: claims: [r]`), so it must **win by fiat and
  skip §3.3 validation.** A nonsensical override resolves fine and **fails loudly
  at execute** via §10 case 4 ("owner cannot execute an owned language") — no new
  failure mode; the capability-blind model already covers it.
- **Precedence:** per-language granularity; override > `_extension.yml` static >
  dynamic `claims_language`.
- **Merge (Q5, 2026-07-02):** `engine:` is an **array**, and the default
  `MergeOp` is **`Concat`** (`config_value.rs:75`), so project + document
  `engine:` layers **concatenate (append)** by default — project-level claim
  overrides *survive* into a document that also sets `engine:` (unless `!prefer`).
  **Open sub-question:** `detect_engines` dedups repeated engine names
  (first-occurrence wins); since Concat appends the document layer *after* the
  project layer, a same-named engine declared in both may resolve to the
  **project's** claims winning over the document's — backwards from the usual
  "document overrides project." Verify the layer-application + dedup direction and
  decide (a warning on conflicting same-name claim entries may be the pragmatic
  answer).
- **Bad references (Q6, 2026-07-02):** an override naming an engine not in the
  sequence/registry → **warn, ignore** (don't silently expand the sequence). An
  override for a language the doc doesn't contain → harmless no-op.
- **Pass-1 payoff:** a doc whose `engine:` block claims every computational
  language resolves with **zero engine loading — even for non-static engines**.
  Metadata claims are the maximally-load-free claim source.

### 2. `generated-languages` (declare handoff targets statically)

Declares languages that will exist *after* an engine runs, even though the
original source has no executable cells for them:

```yaml
engine: [my-codegen, jupyter]     # explicit list controls order (Q2)
generated-languages: [python]     # "python will be present; give it an owner"
```

- **This lifts the T9-ratified limitation for the declarable case.**
  `engine-resolution.md` §6.1 ("Resolution-driven handoff loss — RATIFIED
  2026-07-01") rules out injected-cell handoff to an engine *absent from the
  sequence*, because the sequence is fixed from the original AST and the fix
  would need **runtime sequence growth** (mid-execute re-resolution), which breaks
  the determinism guard + replay + freeze key (§6.2). `generated-languages`
  sidesteps that entirely: the language becomes a **static input**, so
  `languages = computational_languages(ast) ∪ meta.generated-languages`,
  resolution stays pure/once, and the consumer engine is selected pre-execution.
  Determinism, replay, and the resolved-set freeze key are all preserved (the
  declaration is part of the metadata → part of the input). The empty-code-block
  workaround does the same thing crudely by injecting into the AST; this is the
  declarative form.
- **Consumer-only, by design (Q1, 2026-07-02).** Execution engines are *not*
  markdown-in/markdown-out — they must claim real executable cells to run and
  can't start from nothing. So the *generator* is always in the sequence by
  owning its own original cells; it is never the dropped-ownerless case.
  `generated-languages` therefore only needs to bring in the **consumer** of the
  generated language. (You *could* abuse it to make an engine fire "from nothing"
  by declaring a language that isn't really there — discouraged, and the key's
  name deliberately doesn't advertise that use.) → **flat top-level list**, no
  per-engine `generates:` attribution needed.
- **Ordering (Q2, 2026-07-02):** when generator-before-consumer order matters,
  use the explicit `engine:` list (the same top-level ordered list the `claims:`
  override lives in). `generated-languages` does not encode order.
- **Declared-but-unclaimed (Q3, 2026-07-02):** a `generated-languages` entry no
  engine claims → **polite warning**, harmless (nothing runs it).
- **Merge (Q5, 2026-07-02):** as an array it inherits the default `Concat` =
  union/append across layers (the desired behavior); **dedup on read** (it's a
  presence set).
- **`HANDLED_LANGUAGES` (Q4, 2026-07-02):** an epic goal is that **languages are
  no longer hard-coded anywhere.** Merging #241 reframes `mermaid` as an ordinary
  built-in Rust engine (implicit-claiming), removing it from `HANDLED_LANGUAGES`;
  `ojs`/`dot` follow. As that set empties, there is no hard-coded exclusion to
  apply to `generated-languages` and no need to pre-declare handler languages
  statically. Near-term, apply the same exclusion for consistency, but treat it
  as transitional. **This depends on [Plan 8](2026-07-02-plan8-mermaid-absorption-graphviz-ts-extension.md) (see below).**

## Prerequisite: absorb #241 (mermaid as a built-in engine)

Q4 above depends on **[Plan 8](2026-07-02-plan8-mermaid-absorption-graphviz-ts-extension.md)**,
which merges PR #241 (`feature/mermaid-engine`) into the epic and reframes its
`MermaidEngine` as an ordinary built-in that *implicitly claims* the `mermaid`
language (add `claims_language("mermaid") → Primary(1)`, remove `mermaid` from
`HANDLED_LANGUAGES`, loosen the `info == "{mermaid}"` scanner to tolerate cell
attributes). Plan 8 also ports Quarto 1's graphviz handler as a **TS engine
extension** that statically claims `dot`, draining a second
`HANDLED_LANGUAGES` entry. `ojs` follows in a later step. Not part of Plan 6.

## Affected artifacts (blast radius)

**Design contracts (primary — define the behavior):**

- `claude-notes/designs/engine-resolution.md` — **§7 (Pass placement)** is the
  core edit (per-doc lift + fall-through); **§3.3** relax the project-wide gate
  to per-engine/per-doc "needs-no-load"; **§9** allow a *partial*
  `EngineResolution` (a "pending" concept); **§12** promote *both* the "Pass-1
  resolution" **and** "project-level claim overrides" bullets from future to
  in-scope, and restate the freeze-key caveat (forces option A for freeze).
  Also: **§3.2/§3.3** document the metadata claim-override source + list
  shorthand; **§4.1** document `languages = scan(ast) ∪ generated-languages`;
  **§6.1** note that `generated-languages` is the static escape from the T9
  handoff-loss limitation (no runtime re-resolution).
- `claude-notes/designs/document-profile-contract.md` — add
  `engine_resolution: Option<EngineResolution>` (+ a complete/partial marker)
  to the fields table; reconcile the "no engine output on the profile" wording
  (resolution is pure/pre-load, so it *is* profile-eligible — execution results
  still are not); bump `DOCUMENT_PROFILE_VERSION` (2 → 3) with a changelog
  entry.

**Plans (primary — work items change):**

- `2026-04-16-plan1c-extension-integration.md` — most affected: the "fully
  static → Pass-1 precondition" wording (D1, lines 163-165) becomes per-doc;
  the Pass-1 builder `pass1_profile_single_file_live` (lines 797-806) gains the
  conditional `resolve_engines` call + profile stamp; the `resolve_engines`
  stage wiring (lines 647-654, 880-886) changes where/when it's called and what
  is stamped vs. completed in Pass-2.
- `2026-04-16-plan1a-engine.md` — `EngineResolution` may need a partial/"pending"
  representation; the **hint pre-filter** becomes load-bearing for the per-doc
  decision; the "zero-cost Pass-1 lift" assertions (lines 436, 522) and the
  `ProjectContext`-ownership notes (1009, 1079) tighten from "zero-cost" to
  "per-doc, partial-capable."

**Implementation touchpoints for the two metadata keys:**

- `crates/quarto-core/src/engine/resolution.rs` — `resolve_engines` reads the
  per-engine `claims:` from `DetectedEngine.config` (already carried) as an
  overlay on the claim table, and augments `languages` with
  `meta.generated-languages`. Both are pure/static inputs — no signature change.
- `crates/quarto-core/src/engine/detection.rs` — parse `generated-languages`;
  the `engine:` per-entry `claims:` already lands in `config`.
- `crates/quarto-core/src/extension/read.rs` — reuse `parse_claims_map` /
  `parse_static_language_claim`; widen to accept the **list shorthand**.
- Landing plan: fold these into **plan1c** (it owns the claim parser + the
  `resolve_engines` wiring), cross-referenced from here as Pass-1 enablers.

**Plans (secondary — references to reconcile):**

- `2026-04-16-ts-engine-extensions-subprocess.md` (grand plan) — the
  "Multi-engine resolution (post-merge)" summary (lines 63-82) currently states
  resolution runs in Pass 2 with only the file-claim half in Pass 1; update.
- `2026-04-23-website-project-epic.md` — owns the orchestrator + DocumentProfile
  + `profile_version`; coordinate the version bump and new field.

**Explicitly not affected:** plan1a-protocol (the wire shapes don't change —
static claims come from `_extension.yml`, partial resolution is Rust-side/profile
only); Plans 1b, 2, 2A, 3, 4; the replay-engine plan (replay drives from
captures, not re-resolution — only the already-flagged freeze-key note touches
this, and that's future).

## Open before writing (research)

1. **Enumerate the consumers and their tolerance for a partial set.** Which
   Pass-1 readers (project index, kernel pooling/Plan 5, freeze planning) need
   the *complete* resolved engine set, and which can act on a partial one? This
   decides A-vs-B per consumer and whether a "pending" profile field is worth
   the complexity. Freeze (§6.2) is the hard constraint — its cache key must
   hash the resolved engine set, so any doc with a pending engine cannot be
   frozen until resolution completes.
2. **Specify the per-doc "needs-no-load" predicate precisely** — including the
   tier-dominance shortcut (a static `Primary` lets us skip loading a non-static
   engine that could only claim a lower tier) and confirm it's sound for every
   resolution tier (T1-T4, §4).
3. **Profile-shape + version-bump mechanics.** Exact `EngineResolution` form on
   the profile, the complete/partial marker, and how a Pass-2 completion writes
   back (the profile is read-only post-checkpoint — §"Profiles are read-only";
   a Pass-2 completion is a *separate* `StageContext` artifact, not a profile
   mutation).
4. **MEASURE the win.** How many real docs in a mixed project actually resolve
   load-free at Pass-1 vs. fall through? If most docs contend a non-static
   engine, the lift buys little — quantify before committing. This gates the
   plan, same as Plan 5's measurement gate.
5. **Verify the `engine:` concat + dedup-by-name precedence** (Q5). Confirm the
   metadata-layer application order and `detect_engines` dedup direction, then
   decide whether a same-named engine's claims override resolves project-wins or
   document-wins — and whether to warn on a conflict. Affects claim-override
   semantics but not the Pass-1 mechanics.
6. **Sequence the `HANDLED_LANGUAGES` elimination** (Q4). The
   `generated-languages` `HANDLED_LANGUAGES`-exclusion question is transitional —
   it resolves once [Plan 8](2026-07-02-plan8-mermaid-absorption-graphviz-ts-extension.md)
   reframes `mermaid` and `dot` (then `ojs` later) as ordinary claiming engines.
   Track Plan 8 as the real dependency.

## Non-blocking note

This is purely additive: the 1a series resolves in Pass-2 and ships without it.
Plan 6 is a later lift, so it neither blocks nor reorders current epic work. The
two design contracts (`engine-resolution.md`, `document-profile-contract.md`)
are where the change must be written first; the plan edits are downstream of
those contracts.
