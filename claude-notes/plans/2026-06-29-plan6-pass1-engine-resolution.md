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
  not load it to know it loses).
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

## Affected artifacts (blast radius)

**Design contracts (primary — define the behavior):**

- `claude-notes/designs/engine-resolution.md` — **§7 (Pass placement)** is the
  core edit (per-doc lift + fall-through); **§3.3** relax the project-wide gate
  to per-engine/per-doc "needs-no-load"; **§9** allow a *partial*
  `EngineResolution` (a "pending" concept); **§12** promote the "Pass-1
  resolution" bullet from future to partly-in-scope and restate the freeze-key
  caveat (forces option A for freeze).
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

## Non-blocking note

This is purely additive: the 1a series resolves in Pass-2 and ships without it.
Plan 6 is a later lift, so it neither blocks nor reorders current epic work. The
two design contracts (`engine-resolution.md`, `document-profile-contract.md`)
are where the change must be written first; the plan edits are downstream of
those contracts.
