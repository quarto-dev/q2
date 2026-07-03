# Plan 7a — Static content-pattern file claims (research plan)

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Precedes:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md) (Plan 7 consumes 7a's claim mechanism)
**Coordinates with:** [2026-07-01-plan1c2-engine-extensions-loose-ends.md](2026-07-01-plan1c2-engine-extensions-loose-ends.md) (P4 schema; P2 discovery)
**Design docs to amend:** `engine-resolution.md §3.3`; `engine-api-surface.md` (static-claim expressiveness)
**Status:** RESEARCH — design finalized below where marked ✅; open questions (§Open research questions) resolved in Phase 0 before implementation.
**Date:** 2026-07-07

---

## Overview

**The finding that motivates this plan.** A full census of every Q1 execution-engine `claimsFile`
(markdown, knitr, jupyter, and the external julia engine) established that **every content-inspecting
file claim is exactly `extension-gate → read-file → one regex`** — no YAML parse, no R/Python
shell-out, no runtime state. Knitr's spin sniff is a *single hardcoded regex literal*
(`/^\s*#'\s*---[\s\S]+?\s*#'\s*---/`); jupyter's percent sniff is one regex with a per-language
comment marker interpolated in (`^\s*#\s*%%+\s+\[(markdown|raw)\]`). The knitr `spin`→`Rscript`
subprocess is the **conversion** (`markdownFromKnitrSpinScript`), which fires *after* the file is
already claimed — it is not part of the claim decision.

**What this overturns.** `engine-resolution.md §3.3` calls content-inspecting `claims_file`
*"the one genuine must-load case,"* and `engine-api-surface.md` restates it as *"an accepted
static-vs-dynamic boundary."* Both are wrong: a content claim is **data** (`{extension, regex}`),
not **behaviour** (a method that must run). Because a regex over file bytes is pure, deterministic,
and host-free, a content claim can be **declared statically** and evaluated **natively in-process** —
at both the claim stage *and* Pass-1 project discovery — with **zero engine load**.

**What 7a delivers.**

1. A static **content-pattern** on the file-claim declaration (`claims-files`), evaluated natively.
2. A **single shared predicate** that both `EngineClaimsFileStage` (claim time) and project discovery
   (Pass-1) use — so "discovery admits a file" ⟺ "the claim stage claims it," by construction.
3. **Built-in engines declare their claims as data** (jupyter percent, knitr spin) instead of
   hardcoded imperative sniffs — one source of truth shared by claim + discovery.
4. **Q1-parity project discovery** for content-claimed scripts: a `.py`/`.jl`/`.R` file dropped in a
   `_quarto.yml` project renders **iff** it matches its engine's declared pattern — the decision
   `1c.2` Corollary 4 explicitly deferred ("the sniff-at-discovery decision") is answered here: **yes,
   statically.**
5. The **design-contract correction** in `engine-resolution.md`/`engine-api-surface.md`.

**Why it precedes Plan 7.** Plan 7 (native percent/spin conversion + SourceInfo) implements
`JupyterEngine::claims_file`/`KnitrEngine::claims_file` in Phase 7A/7B. If 7a lands first, Plan 7
*declares patterns as data* rather than hardcoding imperative sniffs, and Plan 7's Julia `.jl` claim
(7D/7E) drops from "spawn Deno, load module, call `claimsFile`" to "evaluate a declared regex,
zero subprocess." 7a is the claim half; Plan 7 keeps the conversion half (which stays dynamic).

**Why "research" plan.** The mechanism is clear, but several cross-cutting decisions must be settled
before code (regex flavour, whole-file vs bounded read, built-in declaration home, the `.r`/`.R`
two-engine tie-break, freeze/profile cache-key impact). Phase 0 resolves them; the rest is TDD build.

---

## Schema decision (ratified 2026-07-07, Gordon) ✅

- **Keep the name `claims-files`.** The P4 rename to `claims-extensions` is **dropped** — it was
  premised on "this is only an extension set," which the census disproved. `claims-files` is a genuine
  file-claim surface (extension **+** optional content-pattern), so the original name is correct.
- **Structured entry form.** `claims-files` becomes a list whose entries are `{extension,
  content-pattern?}` objects, with a **bare-string shorthand** (`.echo` ≡ `{extension: .echo}`)
  accepted via `#[serde(untagged)]`. Example:

  ```yaml
  contributes:
    engines:
      - path: julia-engine.js
        name: julia
        claims-files:
          - extension: .jl
            content-pattern: '^\s*#\s*%%+\s+\[(markdown|raw)\]'   # omit ⇒ unconditional claim
  ```

- **Two-step landing.**
  - **Now — in `1c.2` P4 (replacing the rename task):** adopt the structured `claims-files` form with
    **`extension` only** (no `content-pattern` field yet), keep the parse-time undotted-lowercase
    normalization of the extension. This lands *before* Plan 4b Phase-A (same sequencing guarantee the
    rename had) so 4b writes the structured shape from the start — **no re-migration.**
  - **Plan 7a (this plan):** add the `content-pattern` field, native evaluation, built-in declarations,
    and discovery admission.
- **Wire contract unchanged.** `content-pattern` is evaluated **natively in Rust** (regex over file
  bytes) and **never crosses the wire** — it is not sent to the engine. The engine's dynamic
  `claimsFile`-over-the-wire survives *only* as the fallback when no static claim is declared (the
  genuinely-dynamic residue). The dot-adapter (`to_wire_ext`) still applies to the `extension` field
  only; a pattern has no dot semantics.

> **Coordination note:** this reverses `1c.2` P4's "rename → `claims-extensions`" checklist item and
> the `engine-resolution.md §3.3` rename block. Those edits are a prerequisite for 7a and are listed
> in §Coordination below. Nothing in 4b/4c/5/6/9/10 depends on the *name* `claims-extensions` other
> than fixture text authored under the old plan.

---

## The stages of a content-pattern (end-to-end)

The pipeline from YAML to a render decision. Each stage names its owner, its cross-target concerns,
and the invariant it must preserve.

### Stage 1 — Declaration (where a pattern is written)

- **Extension engines** (`EngineContribution::External`): in `_extension.yml`
  `contributes.engines[].claims-files[]`, as `{extension, content-pattern?}`.
- **Built-in engines** (jupyter/knitr/markdown): they have **no `_extension.yml`**. Their claims must
  be declared as **Rust-side static data** (Stage 5), in the *same* `FileClaim` shape, so the
  evaluator is engine-source-agnostic.
- **Invariant:** every claim, whichever source, reduces to the same `FileClaim { extension,
  content_pattern }` value. Discovery and the claim stage never branch on "built-in vs extension."

### Stage 2 — Parse, normalize, compile (fail fast)

- Owner: `parse_contributes` (`extension/read.rs`, currently the `parse_string_list` call at `:425`).
- Replace `parse_string_list` for `claims-files` with a structured parser accepting bare-string OR
  object entries.
- **Normalize** the `extension` to canonical **undotted lowercase** (already the 1c.2-P4 rule; extend
  to the structured form).
- **Compile the regex at parse time** with the `regex` crate (already a quarto-core dep, `1.12`).
  A malformed pattern is a **loud parse error** surfaced through `quarto-error-reporting`
  (`_extension.yml` provenance), never a silently-dropped claim.
- **Invariant:** after parse, a `FileClaim` holds a *compiled* pattern (or `None`); no raw regex string
  reaches evaluation, and no invalid regex survives read.

### Stage 3 — Storage / types

- `EngineContribution::External.claims_files: Option<Vec<String>>` → `Option<Vec<FileClaim>>`.
- `FileClaim { extension: String /* undotted lc */, content_pattern: Option<CompiledPattern> }`.
- `CompiledPattern` wraps `regex::Regex` (+ retains the source string for `Debug`/serialization/error
  display; `regex::Regex` is not `PartialEq`, so derive comparisons on the source string).
- **Cross-target (WASM):** `regex` compiles to `wasm32-unknown-unknown`. Note the code-size cost in the
  hub-client WASM budget (measure in Phase 0); if material, gate the discovery-time read behind the
  native path (WASM builds discover `.qmd` only today — `FIXED` alone — so WASM need not evaluate
  content-patterns at discovery, only the claim stage does, and only when a non-QMD file is handed in).

### Stage 4 — Evaluation: **one predicate, two call sites**

The load-bearing coherence rule (Corollary 3, extended). Define exactly one function:

```rust
/// True iff this claim matches `path` given its already-read `content`.
/// Extension is compared undotted-lowercase; a None pattern is unconditional.
fn file_claim_matches(claim: &FileClaim, ext_undotted_lc: &str, content: &str) -> bool
```

- **Call site A — claim stage** (`EngineClaimsFileStage`, `stage/stages/engine_claims_file.rs`):
  single-file arg or a file already in the pipeline. Reads the file, evaluates each engine's claims in
  `contribution_order`, first match wins (§8 single-engine claim; §Tie-break below).
- **Call site B — Pass-1 project discovery** (`project/discovery.rs` walk): for a candidate whose
  extension appears in some claim, read the file and evaluate the pattern to decide **admission**.
- **Invariant (anti-divergence):** both sites call `file_claim_matches` with the **same** read
  semantics (§Whole-file vs bounded read). If they diverge, discovery can admit a file the claim stage
  then rejects → the §10 case-1 "can't determine execution engine" incoherence. A dedicated coherence
  test (T-coherence) guards this.

### Stage 5 — Built-in engine integration (data, not launch)

- Add to the `ExecutionEngine` trait a **data accessor**: `fn file_claims(&self) -> &[FileClaim]`
  (default `&[]`). The default `claims_file(file, ext)` is **re-expressed** to evaluate `file_claims`
  via `file_claim_matches` — so built-ins that populate `file_claims` get `claims_file` for free, and
  hand-written `claims_file` overrides disappear.
- jupyter/knitr populate `file_claims()` with their percent/spin `FileClaim`s (the patterns Plan 7
  would otherwise hardcode). `TsEngine::file_claims()` returns the parsed `claims_files`.
- **Discovery must not launch engines** (Corollary 1). But it *may* read their **static claim data**.
  Resolve the ordering tension (Corollary 0: registry is built after the walk) by exposing built-in
  claims as **construction-free static data** — an associated `const`/free function
  (`builtin_file_claims()`), gathered before the walk alongside the parsed `External` claims. No engine
  object, no host.
- **Corollary-1 restatement (design-doc edit):** discovery's rule sharpens from *"never the registry"*
  to *"never **launch** engines at discovery — but do read their static claim **declarations** (data,
  not behaviour)."* This is the precise boundary that keeps Pass-1 host-free and profile-liftable while
  admitting content claims.

### Stage 6 — Discovery admission (the Pass-1 answer)

- `RenderableExtensions` (1c.2 P2) gains a second tier. Admission for a walked file becomes:
  1. extension ∈ `FIXED_RENDERABLE` → admit (unconditional); else
  2. extension ∈ *unconditional* claim set → admit (no read); else
  3. extension ∈ *content-pattern* claim set → **read file, evaluate pattern(s)** → admit iff a claim
     matches; else exclude.
- Tier 3 is the only tier that reads file content. It is bounded by the extension pre-filter (only
  script extensions any engine claims are ever read), exactly as Q1 bounds its scan.
- **Q1 parity confirmed:** a percent `.py` with a `[markdown]`/`[raw]` cell is admitted; a plain `.py`
  module is silently excluded (Q1-faithful — a non-document is not a project input; **no** loud error,
  per non-enforcement). A code-only percent script with no `[markdown]` cell is **excluded** (Q1's
  exact predicate — call this out so it's a conscious choice, not an accident).

### Stage 7 — Conversion (unchanged; stays in Plan 7)

- Once claimed, `markdown_for_file` converts. That stays fully dynamic/native and is **Plan 7's**
  domain (percent/spin → qmd with column-precise SourceInfo; knitr's `spin`→Rscript). 7a touches the
  **claim decision only**. The claim/conversion split is clean and preserved.

### Stage 8 — Design-contract correction

- `engine-resolution.md §3.3`: retract *"the one genuine must-load case."* Replace the binary with a
  **three-way split**: (a) **extension-only** (`claims-files` entry, no pattern) — unconditional;
  (b) **content-pattern** (`claims-files` entry with pattern) — static, native, zero-load, eligible for
  discovery; (c) **genuinely dynamic** (`claims_file` that isn't regex-expressible) — the *true*
  residue, **empty across every known Q1 engine**, retained only as the fallback when no static claim
  is declared.
- `engine-api-surface.md`: mirror the split; drop "an accepted static-vs-dynamic boundary."

---

## Open research questions (Phase 0 closes these before code)

1. **Regex flavour + flags. — RESOLVED (2026-07-07, Gordon).** The declared contract is **Rust `regex`
   crate syntax** (engine authors write Rust-regex, not JS-regex; documented in the schema reference).
   Patterns are compiled with the **multiline flag enabled uniformly**
   (`RegexBuilder::new(pat).multi_line(true)`), so `^`/`$` anchor to line boundaries — Q1 `m`-flag
   parity, and authors write `^\s*#\s*%%…` naturally. Newline-spanning is via `[\s\S]` (works verbatim,
   as in Q1's spin pattern) or inline `(?s)`; other inline flags are permitted. No per-pattern flag
   plumbing — one `RegexBuilder` config for every declared pattern.
2. **Whole-file vs bounded read. — RESOLVED (2026-07-07, Gordon).** Read the **whole file** (Q1 parity):
   percent `[markdown]`/`[raw]` cells can appear anywhere, so a head-cap would falsely exclude a script
   whose only markdown cell is late. Bounded by the extension pre-filter (only claimed script
   extensions are ever read). The read must be **byte-identical** at both call sites (the Stage-4
   coherence invariant) — one shared reader, one shared `file_claim_matches`.
3. **`.r` / `.R` claimed by two engines. — RESOLVED (2026-07-07, Gordon).** jupyter (percent) and knitr
   (spin) both register `.r`/`.R`; a file may match one, both, or neither. Resolve by
   **`contribution_order`, first-matching claim wins** (§8 single-engine; Q1 resolves via engine order).
   Both-match → the earlier engine in `contribution_order`; neither-match → unclaimed (excluded from
   discovery / §10 case-1 for an explicit file). Bound by T-tiebreak.
4. **Built-in claim declaration home.** Associated `const` vs a free `builtin_file_claims()` vs a
   trait method on a construction-free handle. Constraint: readable at discovery **without** building
   the registry or launching engines (Stage 5). Pick the shape that keeps one source of truth for
   claim-stage + discovery.
5. **ReDoS / pathological patterns.** Rust `regex` is linear-time (no catastrophic backtracking) — a
   safety win over JS. Confirm no `regex` feature we enable reintroduces exponential behaviour; note
   the linear-time guarantee as an explicit property (semi-trusted extension authors).
6. **Freeze / profile cache-key impact. — RESOLVED as a downstream contract (2026-07-07).** Not an open
   question so much as a landmine to flag; the contract below is the resolution.

   **The consequence.** Before content-pattern, "which files are documents in this project?" was a pure
   function of **name-level metadata** (directory tree, filenames, extensions, `_quarto.yml` globs) —
   you never opened a file to decide membership. That blindness was load-bearing: project-structure
   caches (sidebar, cross-reference graph, render list, the source→`DocumentProfile` index) could be
   keyed on the **file list**, and a *content* edit to a file invalidated only *that file's own output/
   profile*, never the project's *membership*. Content-pattern breaks that separation: `model.py` is a
   document **iff its bytes match the declared pattern**, so **a plain content edit can flip project
   membership** — add a `# %% [markdown]` cell and `model.py` must enter the index/sidebar/render list;
   remove it and it must be evicted — with the **filename unchanged the whole time**.

   **Why it's a cache bug and not a determinism bug.** Pattern evaluation is a pure, deterministic
   function of file bytes (same bytes → same answer, every machine), so the admission bit is a
   *legitimate, hashable* cache-key input — exactly like a content hash. The hazard is narrow: a cache
   keyed on the *file list* (names) silently misses a membership flip, because the name set is identical
   before and after the edit. It is a sin of omission (a missing key input), not of nondeterminism.

   **Which caches are exposed.** Per-document freeze/output is **mostly fine** — its key already hashes
   the file's content. The exposed layer is **project-structure caches**, which gain a new dependency:
   the admission bit of each content-claimed candidate, which depends on that candidate's content.

   **The contract (for whoever builds freeze / incremental-rebuild / a persistent profile index):**
   *Project membership is a pure function of `(candidate content, declared patterns)`. Any cache over
   the project document set — or anything derived from it — MUST include the content-derived admission
   bit for content-claimed extensions in its key, and MUST treat an **admission flip** as a
   structure-level invalidation, a **third trigger** alongside "file added/removed/renamed" and
   "`_quarto.yml` changed." Do NOT key project membership on filenames alone.* (This reflex is what
   every static-site generator does; it is wrong for exactly the content-claimed extensions.)

   **7a's two concrete obligations** (the rest is downstream, and there is **no live bug today** — q2
   re-walks and re-sniffs every render, caching no membership, exactly as Q1 does):
   - Phase 7a-4's discovery result **records, per content-claimed candidate, its content hash (or
     mtime+size) and its admission boolean**, so a future incremental layer can detect flips without
     re-reading every file.
   - This contract is **written into the DocumentProfile/freeze design notes** (not implemented here).
7. **WASM code-size.** Measure `regex`'s contribution to the hub-client WASM budget (Stage 3). If
   material, confirm the discovery-time read is native-only (WASM discovers `.qmd` only) so WASM pays
   the regex cost only in the claim stage, only for handed-in non-QMD files.
8. **`.q` edge case.** Q1's `.q` percent extension maps to no comment char (`undefined`). Decide q2's
   handling (drop `.q`, or require the pattern be fully specified by the declaring engine — which the
   static model does anyway, since the engine writes its own expanded pattern).

---

## Design ratification (carried context)

- **One predicate, two sites** (Stage 4) is the coherence spine; do not duplicate the match logic.
- **Data not launch** (Stage 5): discovery reads static claim declarations, never spawns.
- **Claim ≠ conversion** (Stages 4/7): 7a owns the regex claim; Plan 7 owns `markdown_for_file`.
- **Non-enforcement preserved** (§10): a script that matches no pattern is silently excluded from a
  project (not a loud error); an explicitly-named single non-QMD file that matches nothing still hits
  §10 case-1 (loud) as today.

---

## Phased checklist

### Phase 7a-0 — Research + design-contract correction (do first)
- [x] Q1 regex flavour/flags — RESOLVED: Rust `regex` syntax, multiline uniform. (2026-07-07)
- [x] Q2 whole-file read — RESOLVED: whole file, identical at both sites. (2026-07-07)
- [ ] Resolve remaining Open research questions 3–8; record decisions inline.
- [ ] Amend `engine-resolution.md §3.3` (three-way split; retract "one genuine must-load case";
      restate Corollary-1 as "never launch, do read static declarations").
- [ ] Amend `engine-api-surface.md` (mirror).
- [ ] Amend `1c.2` P4: drop the `claims-extensions` rename; specify structured `claims-files`
      (extension-only) as the P4 deliverable (see §Coordination).

### Phase 7a-1 — Schema (tests first)
- [ ] `FileClaim` + `CompiledPattern` types (Stage 3).
- [ ] Structured `claims-files` parser: bare-string + object, undotted-lowercase extension, regex
      compiled at parse, malformed-regex loud error (Stage 2).
- [ ] Migrate existing fixtures/tests to the structured form (echo, marimo, `engine_registry_build`,
      TS specs) — **only** the shape, no `content-pattern` yet if 1c.2-P4 already did extension-only.

### Phase 7a-2 — Shared predicate + claim stage
- [ ] `file_claim_matches` (single definition).
- [ ] Re-express `EngineClaimsFileStage` to evaluate claims via the predicate in `contribution_order`,
      first-match-wins; two-engine tie-break (Q3).
- [ ] `ts_engine.rs` static claim path consults the same `FileClaim`s (retire the wire round-trip for
      declared claims; keep the dynamic wire `ClaimsFile` only as the no-declaration fallback).

### Phase 7a-3 — Built-in engines as data
- [ ] Trait `file_claims()` + default `claims_file` derived from it (Stage 5).
- [ ] jupyter percent / knitr spin `FileClaim`s (the patterns; Plan 7 then reuses them).
- [ ] Construction-free `builtin_file_claims()` for discovery (Q4).

### Phase 7a-4 — Pass-1 discovery admission
- [ ] Extend `RenderableExtensions` / the walk predicate with the content-pattern tier (Stage 6).
- [ ] Read candidate files (whole-file, Q2) at the walk; union built-in + External claims.
- [ ] Discovery result records, per content-claimed candidate, its **content hash + admission bit**
      (Q6 obligation — enables future incremental flip-detection without re-reading).
- [ ] Write the Q6 membership-cache contract into the DocumentProfile/freeze design notes.
- [ ] Coherence guard test (discovery admit ⟺ claim-stage claim).

### Phase 7a-5 — End-to-end (Q1 parity)
- [ ] `.py` percent script in a `_quarto.yml` project renders; plain `.py` module silently excluded;
      code-only percent script excluded (conscious Q1 predicate).
- [ ] `.R` spin script analogue.
- [ ] `q2 render a.py` single-file path unaffected.

---

## Test Seam Spec (TDD — write before implementing)

| # | item | tier | seam / revert → RED |
|---|------|------|----------------------|
| T-parse-obj | Stage 2 | unit | structured `claims-files` with `{extension, content-pattern}`; bare-string shorthand; dotted+uppercase extension normalizes to undotted-lc; **malformed regex → parse error** (not silent drop). Revert structured parser → object form mis-parses → RED |
| T-compile | Stage 2 | unit | valid pattern compiles once and is reused; source string retained for Debug/eq. Revert compile-at-parse → RED |
| T-match | Stage 4 | unit | `file_claim_matches`: extension-match + pattern-match ⇒ true; extension-match + pattern-miss ⇒ false; `None` pattern ⇒ unconditional true. Revert predicate → RED |
| T-claim-stage | Stage 4 | unit | `EngineClaimsFileStage` on a matching `.py` ⇒ claimed by percent engine; non-matching `.py` ⇒ unclaimed (§10 case-1 for explicit file). Revert predicate wiring → RED |
| T-tiebreak | Q3 | unit | `.R` matching spin (not percent) ⇒ knitr; matching percent (not spin) ⇒ jupyter; matching both ⇒ `contribution_order` winner; neither ⇒ unclaimed. Revert order rule → RED |
| T-builtin-data | Stage 5 | unit | `builtin_file_claims()` returns jupyter/knitr claims **without** constructing the registry; default `claims_file` derives from `file_claims`. Revert data accessor → RED |
| T-discovery | Stage 6 | unit | walk admits a percent `.py`, excludes a plain `.py`, excludes a code-only percent `.py`, still excludes underscore/dot/output-dir files. Revert content-pattern tier → matching `.py` excluded → RED |
| T-coherence | Stage 4 | unit | for a fixed file set, discovery admission set == set the claim stage would claim (anti-divergence). Revert to divergent read semantics → RED |
| T-e2e-py | Phase 7a-5 | e2e | percent `.py` in a project renders → `ProjectIndex` entry with converted title; plain `.py` absent from index. Revert discovery union → no entry → RED |
| T-e2e-R | Phase 7a-5 | e2e | spin `.R` analogue. Revert → RED |

**Accepted-untested / deferred (logged):**
- Freeze/incremental cache-key on content-dependent admission (Q6) — flagged for the profile/freeze
  design, not implemented here.
- The genuinely-dynamic `claims_file` residue — remains covered by 4b's existing dynamic
  `content-claim` fixture (relabel it away from "the one genuine must-load case").

---

## Coordination

- **`1c.2` P4 (blocking prerequisite):** replace "rename `claims-files → claims-extensions`" with
  "adopt structured `claims-files` (extension-only), keep the name, keep undotted-lowercase
  normalization." Fixture/test touchpoints listed in 1c.2 P4 stay, minus the rename; they migrate
  flat-list → object-list (extension only). This preserves 1c.2's "land before Plan 4b Phase-A"
  sequencing so 4b authors the structured shape from the start.
- **`1c.2` P2 (Corollary 4):** its "content axis is Plan 7's — the sniff-at-discovery decision" is
  now **answered by 7a** (statically). Update the cross-reference to point here; P2 itself still ships
  extension-only (echo + any unconditional claim), and 7a adds the content-pattern discovery tier
  additively over the same `RenderableExtensions` seam.
- **Plan 7:** depends on 7a. Phase 7A/7B declare percent/spin as `FileClaim` data (not hardcoded
  sniffs); Phase 7D/7E's Julia `.jl` claim uses the static path (zero-subprocess); conversion +
  SourceInfo work in Plan 7 is unchanged.
- **Plan 4b:** its `content-claim` fixture stays valid (exercises the dynamic *fallback*); update its
  prose that calls the path "the one genuine must-load case."
- **Plan 6:** the "may have loaded a content-inspecting engine in `EngineClaimsFileStage`"
  parenthetical shrinks — declared-pattern claims no longer load. One-line footnote, no mechanics
  change.

## References
- Q1 census (this session): every `claimsFile` sniff = extension + one regex; conversion (spin→Rscript)
  is post-claim. Q1 files: `src/project/project-context.ts` (`projectInputFilesInternal`, `addFile`),
  `src/execute/engine.ts` (`fileExecutionEngine`), `src/execute/jupyter/jupyter.ts`,
  `src/execute/rmd.ts` (`isKnitrSpinScript`), `src/core/jupyter/percent.ts` (`isJupyterPercentScript`).
- `engine-resolution.md` §3.3 (static claims / the retracted "must-load" row), §8 (single-engine
  file-claim), §10 (non-enforcement / case-1 loud failure).
- `engine-api-surface.md` (static-claim expressiveness; DQ set).
- `2026-06-27-plan7-native-percent-spin-sourceinfo.md` (conversion + SourceInfo; the claim half moves
  here).
- `2026-07-01-plan1c2-engine-extensions-loose-ends.md` (P4 schema; P2 Corollaries 0–6).
