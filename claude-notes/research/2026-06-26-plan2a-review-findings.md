# Plan 2A (@quarto/api foundation + @quarto/types vendor) — review findings

**Date:** 2026-06-26
**Plan under review:** `claude-notes/plans/2026-04-16-plan2a-quarto-api-foundation.md` (305 lines)
**Status of the work:** **landed and executing-complete** on `feature/ts-engine-extensions`
(git: `263d06a52`…`365461ee4`, the §2aa namespace commits + two fidelity-fix rounds).
**Compared against:** Q1 source at `/Users/gordon/src/quarto-cli` (`src/config/constants.ts`,
`src/core/api/*`, `src/core/lib/*`, `packages/quarto-types/src/*`) and the landed q2 code
(`ts-packages/quarto-api/`, `ts-packages/quarto-types/`).
**Method:** three detection agents (config+vendored-types drift; namespace-port drift;
self-consistency+completeness), each finding reconciled and then **re-grounded against source
by the reviewer**. Build + tests run locally.

---

## Bottom line

**Plan 2A is faithfully implemented and the foundation is solid. Nothing here is
blocking, and nothing here needs a code fix to unblock the rest of the epic.** The
config key-lists, the vendored `@quarto/types`, and all eight `@quarto/api` namespaces
are accurate ports of their Q1 originals; the package builds clean and **217 tests pass /
1 skipped** (the skip is the CI-fallback parity test, correctly inactive because the live
STRONG-mode parity test is running). No `Deno.*`/`node:*` leaked into `@quarto/api`. The
deferred surface (the QuartoAPI *aggregation*, `jupyter`, the launch-context method bodies)
is deferred **with seams present**, not dropped.

What surfaced is a short list of **plan-document bookkeeping inconsistencies** (the plan
prose and checklist disagree with each other and with the landed code in a few places) plus
**two real forward items** worth carrying forward so they are not forgotten:

- **2a-1 (Moderate):** the runtime `system.execProcess` silently drops `mergeOutput` and
  `stderrFilter` — two params **knitr actively uses** (`rmd.ts:440`) — and the rationale
  given for the drop ("plan-sanctioned simplification" / "no current engine uses them")
  does not survive a source survey. Nothing in scope breaks today (the dropped params have
  no current TS-extension consumer; the Julia benchmark doesn't call `execProcess`), but the
  capabilities have no home in the design and the vendored `QuartoAPI` interface still
  declares the full Q1 signature, so Plan 2's aggregation can't typecheck against it until
  reconciled. The plan-compliant fix is to *flatten* the knobs into `ExecProcessOptions`,
  not drop them.
- **2a-2 (Low):** the config parity test is `Set`-equality, so it catches added/removed/
  mutated keys but **not** reordering or a lost duplicate — slightly weaker than the plan's
  "fail on any difference" wording.

Per your steer ("if we need to fix it, we'll probably add to the return-to-q1 plan"): the
items that plausibly belong in a return-to-Q1 pass are **2a-1** (restore/flatten the
`execProcess` knobs + reconcile the interface) and, optionally, **2a-2** (parity-test
strength). Everything else is a one-line plan-doc edit, not code.

---

## What checks out (faithfully ported — do not touch)

- **Config key lists — exact, all five.** `kExecuteDefaultsKeys` (27),
  `kRenderDefaultsKeys` (53), `kPandocDefaultsKeys` (80, **including all ~33 inline string
  literals** like `"defaults"`/`"file-scope"`/`"trace"`), `kIdentifierDefaultsKeys` (3),
  `kLanguageDefaultsKeys` (131) each match Q1's resolved values member-for-member. Q1's
  *duplicate* language keys (`title-block-author-single`, `-published`, `-modified`,
  `-keywords`) are faithfully preserved and annotated `// duplicated in Q1 source`
  (`config/index.ts:331,335-337`). The careful touches — `base-format` deliberately *not*
  added to the identifier list (`config/index.ts:188-194`) — are correct against Q1.
- **`@quarto/types` is a true vendor.** All 17 files present on both sides; type
  declarations identical. The only changes are mechanical: a `// parity: vendored from …`
  header on every file and `.ts`→`.js` import specifiers (ESM). Crucially the **full Q1
  engine surface survives**: `ExecuteResult` still carries `metadata?`/`pandoc?`/`includes?`/
  `engineDependencies?`/`preserve?`/`postProcess?`/`resourceFiles?`
  (`execution.ts`), `ExecutionEngineDiscovery.quartoRequired?` is present
  (`execution-engine.ts:128`). So the well-known downstream drops (FC-1 ExecuteResult
  fields, FC-2 `quartoRequired` carrier) are **downstream of 2A**, not introduced here —
  at the type level 2A's seams are complete.
- **All eight namespaces are accurate ports** of `core/api/*` + `core/lib/*`: `format`,
  `text`, `markdownRegex`, `mappedString`, `crypto`, `console`, `path`, `system`. Renames
  are interface-faithful (e.g. Q1 `asMappedString`→`fromString`, `mappedLines`→`splitLines`,
  `readYamlFromMarkdown`→`extractYaml`); the documented simplifications (`md5Hash` via the
  `blueimp-md5` npm package — Q1 also uses blueimp-md5; `withSpinner` neutral, no cliffy
  ANSI) are honest and labeled.
- **Platform neutrality holds.** No `Deno.*`/`node:*`/`require(` in any `@quarto/api`
  production source (only test files import `node:fs`/`node:path`/`node:url`, which is
  fine). Every host-only namespace takes IO through an injected `PlatformHost`
  (`make<Ns>(host)` / `make<Ns>Host(host)` factories).
- **Stubs throw the right errors.** Context-dependent methods (`path.runtime/resource/
  dataDir`, `system.pandoc`) throw `"@quarto/api: <ns>.<m>() requires launch context
  (resolved by the engine host at launchEngine)"`; Plan-2 deferrals (`system.checkRender`,
  `runExternalPreviewServer`) throw `"… is not yet implemented (Plan 2)"`. This is the
  distinguishable wording Plan 1b's gated-method tests rely on.
- **Package shape correct.** `exports` lists `.`, `./config`, `./platform`, and the eight
  namespace subpaths; `./jupyter` correctly absent (Plan 3). Deps `blueimp-md5` + `yaml`
  declared and used.
- **Aggregation deferred *with* a seam.** The QuartoAPI-object assembly is Plan 2, and
  `src/index.ts:8-19` documents the exact factory convention the Plan-2 aggregator should
  follow (`make<Ns>` for fully-host namespaces, `make<Ns>Host` for mostly-pure). This is
  framework-complete (deferred-but-seam-present), not dropped.

---

## Findings

Severities are calibrated honestly: none is blocking; most are plan-doc hygiene.

### 2a-1 — `system.execProcess` silently drops two params that knitr actively uses; the stated rationale does not survive a source survey (Moderate; reconcile in Plan 2/2E and the return-to-Q1 pass)

- **Code:** `ts-packages/quarto-api/src/system/index.ts:97-100` —
  `execProcess(options: ExecProcessOptions, stdin?: string): Promise<ProcessResult>`, with
  its own `ExecProcessOptions` (`:43-58`) labeled *"Mirrors the **subset** of Q1's
  `ExecProcessOptions` that is platform-neutral."* It drops Q1's four trailing optionals.
- **Q1 / vendored evidence:** the vendored interface keeps the full Q1 shape —
  `ts-packages/quarto-types/src/quarto-api.ts` `execProcess: (options, stdin?, mergeOutput?,
  stderrFilter?, respectStreams?, timeout?) => Promise<ProcessResult>` (matches Q1
  `core/api/types.ts:165-172`).
- **The stated rationale, and why it doesn't hold up.** Two justifications were offered;
  both are weaker than they look:
  1. *"Plan-sanctioned simplification."* The grand plan (lines 402-408) licenses exactly
     two signature changes: *"Simplified type signatures (flattened options objects)"* and
     *"Missing methods that no current engine uses (stubbed)."* The `execProcess` reduction
     is **neither** — `execProcess` already takes an options object (this drops trailing
     *positional* params, the opposite of flattening), and it is a fully-implemented method,
     not a stub. So the plan does **not** actually authorize this reduction. (Tellingly, the
     plan's own "flattened options objects" phrasing points at the lossless fix: fold the
     four knobs *into* `ExecProcessOptions`.)
  2. *"Seam architecture — they're Deno-transport knobs that belong below the seam."*
     (`.superpowers/sdd/2aa-portmap.md:330`; `platform/index.ts:16-18` calls `PlatformHost`
     "a higher-level seam than `Deno.Command`".) Partly true for `respectStreams`/`timeout`,
     but **false for `mergeOutput` and `stderrFilter`**, which are engine-author *semantics*,
     not transport. And they were not relocated below the seam either — the q2 `ExecOptions`
     (`platform/index.ts:25-32`) carries only `cwd`/`env`/`stdin`, so those two capabilities
     have **no home anywhere** in the current design.
- **The survey ("no current engine uses them") was not done thoroughly — it's empirically
  false for knitr.** Exactly two engine-author-API call sites exist in Q1:
  - **knitr `src/execute/rmd.ts:440`** passes four args:
    `execProcess({cmd: Rscript, …}, input, "stdout>stderr", (output) => colors.red(…))` —
    i.e. it **uses `mergeOutput` (arg 3) and `stderrFilter` (arg 4)** to route R's stdout
    into stderr and to colorize/filter R's stderr. Both are dropped by q2.
  - **jupyter `src/execute/jupyter/jupyter-kernel.ts:181`** passes two args (options +
    stdin), using only the `stdout:"piped"` mode field q2 keeps — unaffected.
  - **julia (the benchmark) and markdown:** no `execProcess` calls (Julia uses TCP to its
    control server).
  So the claim holds only under an unstated, much narrower scope — *"no current
  **TypeScript-extension** engine uses them, and Julia doesn't"* — which is true today only
  because in q2 knitr/jupyter are **native Rust** engines that don't route through this TS
  API. The grand plan markets `@quarto/api` as *"consumable… in the future by Quarto 1
  itself"* (line 418); Q1's knitr would break on the reduced signature. `mergeOutput:
  "stdout>stderr"` is also exactly the stream-routing that matters when stdout is the
  protocol channel.
- **Severity — why Moderate, not blocking.** Nothing in scope breaks *today*: the only TS
  consumer is engine extensions, the Julia benchmark doesn't call `execProcess`, and
  knitr/jupyter are native-Rust in q2. But the rationale is wrong-as-stated, two real
  capabilities are gone with no home, and a future knitr-like TS extension (or the
  advertised Q1-consumes-`@quarto/api` path) cannot express them. Separately, the runtime
  namespace and the vendored `QuartoAPI.system` now have **different** `execProcess`
  signatures + `ProcessResult`/`ExecProcessOptions` shapes, so Plan 2's aggregation can't
  typecheck against the vendored interface until reconciled.
- **Recommended action:** in Plan 2 / Plan 2E (and a candidate for the return-to-Q1 pass) —
  prefer the plan-compliant *flatten-into-options* fix: add `mergeOutput?`/`stderrFilter?`
  (at least) and `respectStreams?`/`timeout?` as fields on `ExecProcessOptions`, thread them
  through `PlatformHost.ExecOptions` to `host.process.exec`, and align the vendored
  `QuartoAPI.system.execProcess`/`ProcessResult`/`ExecProcessOptions` to match. If any knob
  is to be *permanently* dropped, justify it per-knob against the `rmd.ts:440` usage rather
  than under the (inaccurate) "no engine uses them" blanket. No change needed in 2A's own
  files; this is a forward item.

### 2a-2 — Config parity test is `Set`-equality: misses reordering and lost duplicates (Low)

- **Code:** `ts-packages/quarto-api/src/config/config.test.ts:126-154` —
  `expect(new Set(kExecuteDefaultsKeys)).toEqual(new Set(q1.kExecuteDefaultsKeys))` (and the
  same for the other four lists).
- **Plan claim:** lines 109-114 — *"diff `@quarto/api/config` against … `constants.ts` …
  and fail on any difference."* The test header itself (`config.test.ts:6`) says *"Deleting
  or altering any key … must make this test RED."*
- **The gap, in plain terms:** `Set` equality catches an added key, a removed key, and a
  value mutation (all change the set), but it does **not** catch a reordering, nor the loss
  of one copy of a duplicated key (the set already deduped it). For these lists, order and
  multiplicity are functionally irrelevant — they back a membership lookup that partitions
  flat metadata — so this is harmless in practice. But it is marginally weaker than the
  plan's "any difference" wording, and the `kLanguageDefaultsKeys` duplicates the code went
  out of its way to preserve are not actually guarded by the test.
- **Recommended action:** optional. If you want the test to match the prose, compare the
  resolved arrays directly (sorted-array or exact-array `toEqual`) instead of `Set`s. Low
  priority; reasonable to leave as-is and just soften the plan's "any difference" wording.

### 2a-3 — Plan status prose says §2aa is "not yet built" while the checklist and code show it complete (Low; plan-doc only)

- **Plan:** line 7 (*"The **§2aa** runtime surface below … is **not yet built**"*), line 140
  (*"**Status: not yet built.**"*), line 251 (*"### §2aa — runtime surface (not yet
  built)"*) — yet every §2aa checkbox (lines 158-233, 253-263) is `- [x]`.
- **Code:** all nine namespace dirs + `platform/` exist with real bodies and tests; git log
  shows the §2aa build commits. Verified: `npm run build` clean, `npm test` = 217 pass / 1
  skip.
- **The gap:** stale status banner. An agent skimming the header would wrongly believe §2aa
  is unbuilt.
- **Recommended action:** one-line plan edit — flip the three "not yet built" markers to
  "landed / done." No code change.

### 2a-4 — Work-item bullet lists `text.postProcessRestorePreservedHtml`, but resolved-decision #3 and the code defer it (Low; plan-internal contradiction)

- **Plan:** work item line 194-196 lists six `text` functions *including*
  `postProcessRestorePreservedHtml`; resolved-decision #3 (lines 301-302) says it is
  **DEFERRED** ("plan mis-labels it 'pure'"; "Port the other 5 … only").
- **Code:** `ts-packages/quarto-api/src/text/index.ts` exports exactly five —
  `lines`, `trimEmptyLines`, `lineColToIndex`, `executeInlineCodeHandler`, `asYamlText`
  (grepped). The sixth is intentionally absent, documented at `text/index.ts:9-11`.
- **The gap:** the work-item bullet and resolved-decision #3 contradict each other; the code
  correctly follows the decision. Purely a stale checklist line.
- **Recommended action:** one-line plan edit — strike `postProcessRestorePreservedHtml`
  from the line-194 bullet (or annotate it "deferred, see decision #3"). No code change.
  (Substantively fine: the function does file IO and no consumer needs it yet; it lives in
  the vendored `@quarto/types` QuartoAPI surface, so the seam exists for a later port.)

### 2a-5 — Two cosmetic plan-vs-code mismatches (Note; no action needed)

- **PlatformHost has `fs.remove`, not enumerated in resolved-decision #1.** Landed
  `platform/index.ts:69-70` adds `remove(path, opts?)` (used by `tempContext` cleanup,
  `system/index.ts:276`); decision #1 (plan line 289-290) listed `fs` without it. The plan
  itself says to treat the **landed** `platform/index.ts` as authoritative ("read it rather
  than this sketch", line 165), so this is expected drift of a pre-build sketch, not a
  defect.
- **`kLanguageDefaultsKeys` annotation is a JSDoc block, not the exact inline string the
  plan dictated.** Plan line 63-65 asked for `// not used by metadataAsFormat partition;
  present for parity`; the code (`config/index.ts:201-211`) carries a fuller JSDoc with the
  same semantic content (and a `config/metadata.ts:200-210` cite). The richer form is
  strictly better; no action.

---

## Completeness (framework) check — clean

No dropped-without-seam items in 2A's scope. 2A is SDK scaffolding; the protocol/engine
surfaces that the epic's FC-1/FC-2/FC-3 concern (ExecuteResult wire fields, `quartoRequired`
carrier, the `static claims:` block) are **not** 2A's responsibility, and where 2A touches
their *types* (the vendored `@quarto/types`) the **full Q1 surface is preserved**, so the
seams those later plans need are present. The deferred runtime surface (QuartoAPI
aggregation → Plan 2; `jupyter` → Plan 3; `@quarto/types` refinements → Plan 2/2E) is
deferred with explicit seams (the `src/index.ts` factory convention; the vendored interface;
the throwing stubs).

---

## For the downstream 2A reviewer — what to verify if you want to double-check

1. Re-run `cd ts-packages/quarto-api && npm run build && npm test` (expect clean + 217/1).
2. The parity test is STRONG-mode only when `external-sources/quarto-cli` resolves — it does
   here (symlink → `/Users/gordon/src/quarto-cli`). In CI without it, the test silently
   downgrades to anchor-key checks (2a-2).
3. If you act on 2a-1, do it in Plan 2/2E, not here — touching `system.execProcess` in 2A
   would be out of scope.
4. Items 2a-3, 2a-4, 2a-5 are plan-document edits; the code is correct as landed.
