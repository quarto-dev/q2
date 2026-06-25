# Plan 1b (@quarto/engine-host-deno) — pre-implementation review findings

**Date:** 2026-06-24
**Reviewer branch:** `review/1b-engine-host-deno` (off `feature/ts-engine-extensions`)
**Plan under review:** `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md` (1307-line, multiplexed/parallel-Pass-2 version)
**Compared against:** landed `crates/quarto-core/src/engine/ts_process.rs` (2192 lines) + `ts_protocol.rs` (1105 lines); `@quarto/api` §2aa namespaces; Q1 `external-sources/quarto-cli`.

## Branch-selection caveat (resolved)

The `ts-engines-work` worktree carries a **stale, serial-dispatch** copy of Plan 1b
(predates the parallel-Pass-2 / async-multiplexed rework `9015c6d24`). The live design
lives on `review/1a-host`, `review/1a-engine`, and `feature/ts-engine-extensions`, and
**matches the landed Rust host**. This review targets the live version on
`feature/ts-engine-extensions` per user direction.

## What checks out (foundation is solid)

- **Protocol frozen & implemented** (`ts_protocol.rs`): all of 1b's wire assumptions match —
  `Request/Response { id: u64, msg }`, `ToEngine::Cancel { target }`, `TsSourceMapEntry { start,
  length, source: Option<TsSourcePosition> }`, `TsExecuteOptions` (`lib_dir: String`, `params`
  own field), `TsFormatIdentifier` quad, `TsExecuteResult { markdown, supporting, filters,
  includes, html_dependencies }` (no `needsPostprocess`).
- **Rust host matches the multiplexed model**: background reader thread + `pending:
  HashMap<u64, PendingSlot>`; timeout sends a `Cancel` frame (cooperative, **not** SIGKILL);
  SIGKILL reserved for Drop/shutdown/malformed-channel; malformed-stdout detection names
  `console.log/console.info`. Demux/cancel/crash all have mock + proc-tier tests.
- **Plan 2A §2aa real on this branch**: five config lists populated (`pdf-standard` in both
  render+pandoc — the T1 discriminator), `PlatformHost` shape matches `deno-host.ts`, `QuartoAPI`
  + `ExecutionEngineDiscovery.init?` present.
- **Every Q1 citation accurate** (metadataAsFormat 4-array order + Stage-1 language peel,
  tail-norm, `execute/types.ts` line refs, `julia-engine.ts:644`).

## Contradictions

1. **Bundle embed path is plan-vs-code wrong, in two places.** Plan §Build-model (lines 47–52)
   and §"Where is engine-host-deno.js at runtime" (line 1122) say
   `include_str!("../../../../ts-packages/.../engine-host-deno.js")` "relative to ts_process.rs —
   four `..`s reach the repo root." The **landed code** (`ts_process.rs:56–59`) uses
   `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ts-packages/.../engine-host-deno.js"))`
   — crate-root-anchored, two `..`, chosen deliberately to survive file moves. Fix both spots.

2. **Plan contradicts itself on the embedding *model*.** §Build-model (lines 36–44) and
   §runtime (line 1121) cite the reveal.js "bundle is *source*, commit-and-`include_str!`"
   precedent; **Phase 4 (lines 1058–1060) explicitly says that framing is wrong** ("treated
   exactly like `q2 mcp`'s `dist-bundle/`… the earlier 'bundle is source like reveal.js' framing
   was wrong; the cited `quarto-system-runtime`/`ejs-bundle.js` precedent does not exist").
   Reconcile to the Phase-4 (generated-artifact) framing.

3. **"Only jupyter is still a stub" vs the §2aa sequencing caveat.** Line 1142 ("at 1b landing the
   pure + host-only namespaces are already real… only `jupyter` is still a stub") contradicts
   lines 751–760, which admit `path.runtime`/`path.resource`/`path.dataDir` and `system.pandoc`
   real *bodies* are still stubbed in §2aa. Source confirms: those four are throwing stubs.
   Success-criterion line 1275 ("`system`… real") is therefore overbroad — `system.pandoc` is a
   stub. State precisely which methods are real vs stub at 1b landing.

4. **Two named build entrypoints.** Phase 1 `package.json` defines `"build": "node
   esbuild.config.mjs"`, and success criteria (lines 1279, 1282) say the bundle "builds with
   `npm run build`" and CI checks "what `npm run build` produces." Phase 4 (line 1050) mandates a
   new `cargo xtask build-engine-host-bundle` wired into `build-all`, and the CI check (1069–1077)
   runs "the xtask." Pick the canonical entrypoint (or state that the xtask wraps esbuild) and make
   the success criteria consistent.

## Underspecified

5. **Poison policy ignores cancel-before-start.** "Poison the instance when the cancelled request
   was an `Execute`" (lines 298–301) reasons Execute is "the only daemon-engaging request." But a
   same-engine Execute can sit **queued** on the per-engine queue, not yet started → no daemon
   engaged. Cancelling *that* still drops the instance under the rule as written, forcing a
   needless re-launch for siblings. Condition should be "an Execute that actually began executing,"
   and the plan should say what happens to a queued-but-unstarted cancelled Execute.

6. **`Cancel` has no ack, and the harness's `Cancelled` response may be discarded.** Read loop does
   `inflight.get(target)?.abort(); continue;` — no response frame for `cancel`, silent no-op on
   unknown target. T6 says the request "resolves as `Cancelled`," but the **Rust side already
   removed the pending slot** on timeout (`ts_process.rs:631–641`) and returns `Timeout` locally —
   so the harness's `Cancelled` frame arrives for an unknown id and is dropped. The plan never
   states that (a) `Cancel` is fire-and-forget with no ack, and (b) the `Cancelled` response is
   often unobserved. Clarify who, if anyone, consumes `Cancelled`.

7. **`handledLanguages` loud-failure obligation is unbound by any test.** Lines 520–529 add a real
   semantic rule (engine handed a cell in a language it *owns* but cannot execute → must return a
   protocol `error`, never silently skip; maps to Rust `NoHandlerForLanguage`). No Phase-0 / contract
   test binds the harness's faithful-surfacing role. Decide whether 1b owns a test here or whether
   it's wholly a plan1a-engine concern, and say so.

8. **Two context stores not reconciled.** Re-launch uses a per-engine "stashed `context`"
   (lines 305–310), while the API contract asserts a single shared `state.context` invariant across
   engines (lines 699–705). Which one feeds `engine.launch(stashedContext)` on re-launch, and how do
   they stay consistent? Spell out the relationship.

## Ambiguities

9. **When is `inflight[id]` populated — at dispatch, or after the per-engine queue admits the
   task?** The pseudocode (lines 277–283) sets `inflight` in the fire-and-forget `dispatch`, which
   also chains on `perEngineQueue`. If `inflight[id]` is set *before* the queue wait, a `Cancel`
   can abort a still-queued request (ties into #5). If set *after*, a `Cancel` arriving while queued
   finds nothing (`?.` no-op) and the request runs uncancelled. This ordering materially changes
   cancel semantics and isn't pinned down.

10. **`writeMutex` vs "single-threaded-atomic writes" — is the mutex needed?** Lines 275/312 require
    a `writeMutex` (`AsyncMutex`) around frame writes, but framing.ts (lines 1031–1035) argues a
    full-line `writeSync` on Deno's single thread "can't interleave." If each frame is one
    `writeSync` of a complete line, the mutex is redundant; it only matters if a frame is written in
    multiple awaited chunks. State which, and where `AsyncMutex` comes from (no source named).

11. **Dangling "#N in the review" references.** Lines 828 ("#2 in the review") and 869 ("#6 in the
    review") point at an unnamed/unlinked review document; a reader can't resolve them. Link or inline.

## Open questions

### Written (deferred in the plan)
- **Phase 1.6** — move protocol off stdout to loopback TCP, retiring the `console.log` footgun. Deferred.
- **A′ deferral** — Rust-side `SourceInfo::Concat` reconstruction of the `markdownForFile`
  `source_map` is deferred; v1 carries `source_map` **unconsumed**. So the TS `markdownForFile`
  serializer (T4) is forward-wiring with no consumer in v1. (Note: `mapped-source.ts` *rehydration*,
  T2, *is* consumed — it backs the engine's `execute`-input `.map()`.) Worth confirming the
  serializer is still worth building in v1 vs trimming to match A′.
- Bundle-size embedding gate (post-Plan-3); deferred-deps (`dependencies:false`) limitation;
  `target()` memoization; per-engine context extension point; deferred Q1 lifecycle hooks.

### Unwritten (surfaced by this review)
- **Daemon liveness after cancel→poison→re-launch (central risk).** T7 asserts "transparent
  re-launch → completes normally," but its test engine is a *launch-counter double*.
  `engine.launch()` only reconstructs the JS instance object; the **daemon** (Julia/Jupyter) started
  in the *aborted* execute may be wedged mid-computation, and the re-launched instance reconnects to
  that same daemon via transport file. "Daemon ambiguity is q2's problem" (lines 637–641) waves this
  off, but nothing specifies how the sibling request B gets a *clean* daemon — reconnecting to a busy
  daemon could hang B. The "completes normally" guarantee is proven only for the test double.
- **stdin-EOF exit vs in-flight concurrent requests.** Lines 401–409 exit the read loop on stdin EOF
  → `Deno.exit(0)`, the same terminal as `shutdown`. With concurrent in-flight tasks (parallel
  Pass-2), an immediate exit on EOF could truncate a still-running `Execute`'s response. The Rust
  side closes stdin *after* `Shutdown` and joins on child exit, expecting a drain — but the harness
  has no specified drain-before-exit step.
- **Per-engine queue is an unbounded promise chain.** A project with thousands of files on one engine
  builds a long promise tail in `perEngineQueue` (memory/latency). No backpressure discussed (bounded
  by #engines for *parallelism*, but the per-engine *queue depth* is unbounded).

## Research verdicts (per initial-review item)

Each initial item, run to ground against the landed code, the canonical concurrency
design doc (`engine-host-concurrency.md`), the sibling plans, and Q1.

**Headline meta-finding:** most of the "underspecified / ambiguous / open" items are
**actually specified in `engine-host-concurrency.md`** (the canonical doc the plan defers
to) — they are gaps in Plan 1b's *self-containment*, not in the *design*. An implementer
reading 1b alone would miss them. The genuinely-actionable items reduce to the two
build-model contradictions, the "only jupyter is a stub" inaccuracy, the missing
`store_html_dependencies` dedup, and a few sentences 1b should pull forward from the
design doc.

### Contradictions
1. **Bundle `include_str!` path** — **CONFIRMED drift, both spots** (lines 47–52, 1122).
   Landed code uses `concat!(env!("CARGO_MANIFEST_DIR"), "/../../ts-packages/…")`
   (`ts_process.rs:56–59`). Aggravating: `plan1a-host:1331–1332` explicitly warns against
   the source-file-relative form 1b still prescribes — so 1b contradicts its sibling plan's
   own guidance. **Action: fix both spots.** Moderate.
2. **Embedding-model self-contradiction** — **CONFIRMED.** §Build-model (36–44) + §runtime
   (1121) use the stale "reveal.js *source*" framing; Phase 4 (1055–1060) and
   `plan1a-host:1352–1369` use the correct "generated artifact like `q2 mcp` `dist-bundle/`"
   model. **Action: rewrite the two stale halves to match Phase 4.** Moderate.
3. **"Only jupyter is a stub" (1142) vs §2aa caveat (751–760)** — **CONFIRMED inaccurate.**
   `path.runtime/resource/dataDir` + `system.pandoc` are *also* stubs at 1b landing
   (landed source). The caveat is correct; line 1142 and the gated-table ("unblock after
   launch", 688–697) overstate. **Action: fix 1142; cross-link the gated-table to the
   caveat.** Low.
4. **Two build entrypoints** — **NOT a contradiction (withdrawn).** The `npm run build` (in
   package) + `cargo xtask build-engine-host-bundle` (wrapper) split is the established
   pattern (`build_hub_mcp_bundle.rs` wraps `npm run bundle`). Internally consistent; just
   not implemented yet (greenfield).

### Underspecified
5. **Poison on cancel-before-start** — **Intentional, not a gap.** Design §2: *any* `Execute`
   cancel/timeout "always poisons"; a queued Execute that times out during queue-wait
   poisoning the instance is the expected path, cost = one ~0 re-launch. 1b just doesn't
   restate it. **Action: optional cross-ref to design §2–3.** Low.
6. **`Cancel` ack / dropped `Cancelled`** — **Resolved by design.** `Cancel` is
   fire-and-forget; the Rust worker resolves its own slot locally on cancel/timeout, and the
   harness's later `Cancelled` frame is dropped as a late id. Wire `FromEngine::Cancelled`
   *does* exist (`ts_protocol.rs:127`). 1b's "resolves as `Cancelled`" wording obscures this.
   **Action: clarify wording.** Low.
7. **`handledLanguages` loud-failure untested** — **Stands, Low.** Obligation is well-
   documented (`engine-resolution.md` §10 case 4 + `plan1a-engine` `NoHandlerForLanguage`,
   both confirmed to exist); it's primarily an engine-author + Rust concern, harness role is
   just faithful error-forwarding. **Action: optional harness test.** Low.
8. **Two context stores** — **Minor.** In the single-context invariant the per-engine
   `stashedContext` (for re-launch) equals shared `HostState.context`. **Action: one
   reconciling sentence.** Low.

### Ambiguities
9. **`inflight[id]` ordering (pre/post per-engine queue)** — **Resolved by inference:**
   must be **pre-queue**, because "timeout includes same-engine queue wait" (design §"Timeout
   includes same-engine queue wait") means a *queued* request must already be cancellable.
   **Action: state it explicitly in 1b.** Low–Moderate.
10. **`writeMutex` vs atomic `writeSync`** — design keeps the mutex *and* claims atomic
    per-line writes; the mutex only matters if a frame is written across `await`s/chunks.
    **Action: state whether writes are a single `writeSync` (no mutex) or async (mutex
    needed); name `AsyncMutex`'s source.** Low.
11. **Dangling "#N in the review"** (828, 869) — **CONFIRMED dangling.** **Action: link/
    inline.** Cosmetic.

### Open questions
- **Written deferrals** (Phase 1.6 loopback-TCP, A′ Rust source-map reconstruction, bundle-
  size gate, deferred-deps, `target()` memoization, deferred Q1 hooks) — all legitimate,
  well-scoped, and cross-referenced to docs that exist. No action.
- **Daemon liveness after cancel→poison→re-launch (top risk)** — **Addressed in design §3
  (142–145):** self-healing — if the daemon is wedged, B re-times-out on its own `window`
  and re-poisons; B does **not** necessarily "complete normally." 1b's T7 "completes
  normally" is the happy path (test-double). **Action: surface the self-healing caveat in 1b
  and cross-ref the design doc.** Moderate (scariest behavior, under-documented in 1b).
- **stdin-EOF exit vs in-flight requests** — **Genuinely open.** Neither 1b nor the design
  doc specifies a drain-before-`Deno.exit(0)` on EOF. Unlikely in practice (shutdown precedes
  EOF), but unspecified. **Action: specify drain-or-not.** Low–Moderate.
- **Unbounded per-engine promise-queue** — minor, unaddressed anywhere. **Action: note as
  accepted, or bound it.** Low.

### Bonus drift items (from the "all other citations" audit)
- **R5** — `claimsLanguage` returns `boolean | number` in Q1 (`execute/types.ts:56`); the
  `LanguageClaim` *object* form (lines 379–385) is a **q2 extension**, not Q1 parity. The
  Julia validation target returns only `boolean|number`. **Action: mark as q2-introduced.**
  Moderate.
- **`store_html_dependencies` dedup is missing** — plan (909–912) asserts dedup-by-name
  (first-wins) + `DiagnosticMessage::warning`; `dependency.rs:37–100` has **no** dedup/warning.
  A relied-upon contract for the `htmlDependency()` helper's idempotency; it's a
  `plan1a-engine` deliverable, not yet built. **Action: track as a gate on 1b's helper.**
  Moderate.
- **PlatformHost sketch prose** flags only 3 of 8 omitted required members (omits
  `fs.ensureDir/remove`, `process.onExit/exit`, `log.*`). The "must implement the full landed
  interface" catch-all covers it, but the list is partial. Low.
- **Gated-error wording** — harness gate ("unavailable before engine launch") and the §2aa
  stub ("…requires launch context (resolved by the engine host at launchEngine)") are
  distinct strings; the gating tests must distinguish them (the caveat already says so). Low.
- **Confirmed accurate** (no action): the `console.log` "category 9" ref (`plan1a-host:1289/
  1303`); wire `FromEngine::Cancelled`; all remaining Q1 citations (text/mappedString split,
  `resolveDependencies` defaults, `instance.*` shapes, `ProjectContext`, jupyter deps,
  `toc-title-document`); all 10 cross-reference anchors (plan1a-protocol appendices,
  plan1a-engine SEAM-3/NoHandlerForLanguage/race-free-init, plan1a-host teardown/Phase-1.6/
  bundle-embedding, design §§3.2/5/10.4 + poison §3, ipynb-filters subsumption,
  `test_resolve_format_nested_objects`); the nine-namespace `QuartoAPI` type; namespace
  method names.

## Meta

- **"Estimated sessions: 1" (line 14) is unrealistic.** The live plan now spans 7 Phase-0 seams +
  ~12 contract tests, multiplexed dispatch, cooperative cancel, poison/re-launch, `framing.ts`, a new
  `cargo xtask` bundle step, a staleness diagnostic, and a CI freshness check. Re-estimate.
- **Dependency header (line 4) is slightly stale**: "plan1a-host… runs in parallel with 1b" — 1a-host
  is already **landed** on this branch (Part 1+2). Harmless, but worth a touch-up.
- Package skeleton (`@quarto/engine-host-deno/{package.json,src,esbuild.config.mjs}`) is **not yet
  created** — only the 179-byte placeholder bundle exists. Phase 1 is greenfield, as expected.
- No CI lint enforcing committed-bundle freshness yet (Phase 4 deliverable).

## Round 2 — fresh-eyes pass (applied) + second drift-list evaluation

After committing round 1, a fresh read of the edited plan + a second drift list surfaced more,
including a **second-order bug in one of my own round-1 edits**. All applied to the plan:

- **writeMutex (my round-1 edit was WRONG — corrected).** I had recommended a synchronous
  `writeSync` with no mutex. But a synchronous write blocks the single Deno thread including the
  read loop, and `engine-host-concurrency.md` (41, 88–89) specifies writes go *under a write-mutex*
  precisely so the continuous drain is preserved — a blocking write can deadlock both pipes under
  concurrent large frames. Reverted to: **async writes (`await out.write`) serialized under
  `writeMutex`**; documented the deadlock-avoidance rationale and that `AsyncMutex` is a small
  hand-rolled promise-chain serializer.
- **`format.*` is NOT gated (HIGH).** The plan listed it gated "when called without an explicit
  format argument," but every predicate requires a `Format` arg (`core/api/types.ts:132`), Q1's own
  engine always passes one (`julia-engine.ts:236`), and `EngineHostContext` carries **no** format
  (it arrives per-`Execute`). Degated: removed from the gated table + error contract; re-vehicled
  the gating tests onto `path.runtime`/`path.dataDir`/`system.pandoc`; the §2aa caveat's real-value
  vehicle moved to the ungated `format.isHtmlCompatible(format)`. Added the correct rationale for the
  gated four (they need `runtimeDir`/`resourceDir`/`dataDir`/`pandocPath` from launch context).
- **`console.*` mislabeled "(pure)"** → host-only (writes stderr via `host.log`); bucket unchanged.
- **`EngineProjectContext` vs `ProjectContext`.** `launch()` takes the narrower `EngineProjectContext`
  (`execute/types.ts:86`), distinct from `ExecuteOptions.project: ProjectContext` (`:162`). Noted so
  the two shims aren't conflated.
- **`htmlDependency()` → return-based (HIGH, the big one).** Removed the imperative stateful
  `quarto.htmlDependency()` method entirely (it was an accumulator on the shared `HostState`, which
  races under concurrent cross-engine `execute()`). Replaced with an optional `htmlDependencies?:
  HtmlDependency[]` **return field** on the engine's execute result — the harness reads it off the
  return value. No accumulator, no race, matches Q1's "deps are return values" model (Q1 has no such
  registration on its engine surface; the imperative form is the Lua-filter API only). Updated the
  wire description, the helper section, the test (return-based, dropped "called outside execute()
  throws"), and the success criterion.

**Second drift-list evaluation (items checked against Q1 source):** H1/format = valid (= the degating
above); claimsLanguage prose = already fixed in round 1 (R5); console "(pure)" = valid (fixed);
EngineProjectContext = valid (fixed); `types.ts:236`→`:238/:243` cosmetic = nudged. **Two items in
that list were wrong and discarded:** the claim that the gated three are gated by "ambient host env
paths" (they're launch-context-gated), and the claim that `executeResultIncludes` should be
`resultIncludes` (`executeResultIncludes` is the real Q1 symbol at `jupyter.ts:2155`; the plan's
cite is correct).

## Round 3 — ExecuteResult field routing (Item A; Q1 read directly)

Read Q1 `execute/types.ts:166-178` (`ExecuteResult`) **directly** this round (prior
rounds verified Q1 only via subagent line-quotes — too thin for a "what to wire"
judgment). Q1's `ExecuteResult` = `{ markdown, supporting, filters, metadata?, pandoc?,
includes?, engine?, engineDependencies?, preserve?, postProcess?, resourceFiles? }`.

The plan's step 7 routed `includes`/`htmlDependencies` and explicitly dropped
`engineDependencies`/`preserve`/`pandoc`/`postProcess`, but was **silent on
`supporting`, `filters`, `metadata`, `resourceFiles`** — an implementer wouldn't know
to forward or drop them.

Checked the q2 side directly:
- Wire `TsExecuteResult` (`ts_protocol.rs:315-321`) = `{ markdown, supporting:
  Vec<String>, filters: Vec<String>, includes, html_dependencies }` — **`supporting`
  and `filters` are already on the wire**; `metadata`/`resourceFiles` are not.
- Core `ExecuteResult` (`engine/context.rs:135-170`) = `{ markdown, supporting_files:
  Vec<PathBuf>, filters: Vec<String>, includes, needs_postprocess }`.
- **`supporting` is load-bearing:** `supporting_files` is "drained from
  `StageContext.resource_report` after engine execution… copied into the project's
  output directory" (bd-o8pr). `project_resources.rs::add_engine_files` is the path
  knitr's `<doc>_files/figure-html/*.png` already take. Dropping `supporting` orphans
  TS-engine figures.
- `filters` → core `ExecuteResult.filters` exists ("e.g. the 'quarto' filter") but is
  only carried/traced (`trace.rs:451`); q2 has no filter-application stage acting on it
  yet. Forward for fidelity; inert downstream for now (not a drop).

**Fix applied (step 7 rewritten to route every field):** forward `supporting`
(load-bearing) and `filters`; document `metadata`/`resourceFiles`/`engine` as off-wire
(with reasons + how to add later); keep the existing
`engineDependencies`/`preserve`/`pandoc`/`postProcess` drops. Added a matching success
criterion. (Rust-side mapping `TsExecuteResult.supporting → ExecuteResult.supporting_files`
is plan1a-engine's `TsEngine` wire→core conversion, not yet written — flagged.)

**Correction to the incoming drift note:** it guessed `filters` was "very likely a
deliberate drop." Not so — `filters` has both a wire field and a consumed-ish core
field, so it's forwarded, not dropped.
