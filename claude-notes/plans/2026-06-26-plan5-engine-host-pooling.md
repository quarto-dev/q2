# Plan 5 — engine-host pooling (preview re-compute warmth)

**Status:** research stub — not yet designed in depth. **Created:** 2026-06-26.
**Sequence:** post-Plan-4 capstone optimization; orthogonal to Plan 3; runs **last**.
**Depends on:** the full TS-engine stack (1a–c, Plan 2, validated by Plan 4), the
**preview↔TS-engine wiring** (the plan1c gap, R5 in RTQ), and **DQ-7** (RTQ Item A).

## Driver

Interactive `q2 preview` **re-compute** for TS-engine extension users. Preview is capture+replay:
a prose edit replays a stored `EngineCapture` in the browser (no engine); a **re-compute** (the
code changed; user clicks re-execute, or `preview.engine: auto` fires) **runs the engine**. For a
TS engine, a re-compute that respawns the Deno subprocess + re-`import()`s the module is interactive
jitter. Plan 5 keeps the host warm across re-computes.

## Scope (decided)

- **Preview-session, SINGLE PROJECT.** A q2 process never opens multiple projects (Q1 didn't; CLI
  preview is one project; hub-client sees one). So there is **no cross-project case** — the survey's
  cross-project machinery (render-boundary signals, per-render state reset, stashed-context
  invalidation, eviction/LRU, the julia transport-key audit) is **not applicable**, not merely
  deferred.
- **Generalizes to the WASM future.** The same shape recurs when hub-client gains a browser engine
  runtime (WASM engines). Design at the **`EngineTransport`/host abstraction**, transport-agnostic
  (`StdioTransport` today, a future `WebSocketTransport`/WASM host) — not at the Deno-subprocess
  level.

## Design (decided)

- **Own the warm host at SESSION scope** (the open project's preview/hub session), behind
  `EngineTransport`. plan1c's "`ProjectContext` owns the host" becomes "**the session owns the host;
  `ProjectContext` borrows it**." A re-compute is then just another `execute()` on the already-warm
  instance. (This is what fixes the "`ProjectContext::discover` fresh per re-compute would respawn"
  hazard.)
- **Invalidation (single-project):** reuse the warm instance unless (a) project config changed
  (`_quarto.yml` → new `EngineProjectContext` → **DQ-7's per-`launchEngine` project context is the
  re-launch trigger**), or (b) crash (existing poison/relaunch).
- **Concurrency:** single-flight re-computes (one kernel, single-threaded) — compose with preview's
  existing debounce + the per-engine serialization queue.
- **Graceful degradation:** a dead warm subprocess re-spawns on the next re-compute
  (`ensure_started` is idempotent). Already handled.

## Open before writing (research)

1. **MEASURE FIRST — the win is bounded.** The *kernel* (Julia control server / Jupyter kernel —
   the *seconds*-scale cost) **already survives a subprocess respawn** because it's transport-file
   keyed. So pooling saves only **Deno-spawn + module-`import()`** (~hundreds of ms), not the kernel
   rewarm. Measure the real re-compute respawn cost first; if it's small relative to the engine run,
   the complexity isn't justified. **This gates the plan.**
2. Confirm the capture/replay flow is untouched (pooling only speeds capture-production).

## Prerequisite (NOT Plan 5 — it's a plan1c gap)

Preview must **use** TS engines at all — today `engine_registry: None` (`preview.rs:216`), and the
preview capture path never reads a registry from the `ProjectContext` it discovers, so it falls back
to built-ins-only `EngineRegistry::new()`. The **functional** wiring (preview capture path reads
`project.registry`) is **finishing plan1c's registry-ownership move** — queued as **R5** in RTQ, not
Plan 5. Plan 5 (warmth) builds on it.

**R5 is a three-site repoint, not one.** The built-ins-only registry (`engine_registry: None`,
`preview.rs:216`) reaches the engine through **three** native call sites, all of which R5 must
source from `project.registry`:
- `record_eager_captures` — startup capture (`lib.rs:214`)
- `recompute_staleness` — on-edit staleness → re-run when `EnginePolicy::Auto && is_stale`
  (`lib.rs:260` → `capture_driver.rs:311`)
- `re_execute.rs` — the live re-execution path (`re_execute.rs:309`)

All three funnel through `record_capture_cached` → `record_capture`. If R5 repoints only the eager
driver, a TS-engine doc captures at startup but every live re-compute silently falls back to
built-ins-only — the engine vanishes on edit.

**Plan 5 owns the *latency* of the last two.** The `re_execute.rs` / `recompute_staleness(Auto)`
re-compute path is exactly what the warm host keeps fast (no Deno respawn + module-`import()` per
edit). The boundary: **R5 makes re-compute *correct*** (the right registry runs); **Plan 5 makes it
*interactive*** (the host is already warm). The measure-first gate (§Open #1) applies to this path
specifically — the kernel already survives a respawn, so the win is bounded to the Deno-spawn +
`import()` cost on each re-compute.
