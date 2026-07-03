# Plan 6 — Pass-1 engine resolution (per-doc lift): implementation plan

**Status:** implementation plan (design ratified with Gordon; final revision
2026-07-05 — claim overrides are **whole-table claim replacement**, a claim
*source*, not a resolution tier).
**Sequence:** post-Plan-1c; **executes after Plan 4b** (ratified 2026-07-06 —
4b lands the `engines:` ordering splice + name-validation this plan builds on,
and exercises the resolution tiers this plan's claim tables intercept; see
decision 3, the § Post-4b reconciliation note below, and 4b's § Coordination
with Plan 6). **Additive** on top of the shipping 1a stack (which resolves in
Pass-2 and works without any of this). Orthogonal to Plans 3/5 (Plan 4
complete).
**Branch:** `plan6-pass1-engine-resolution` off `feature/ts-engine-extensions`.
**Depends on (all landed on the epic branch, code-verified 2026-07-02):**
`resolve_engines` + `EngineResolution` (`crates/quarto-core/src/engine/resolution.rs:340`),
static-claims `_extension.yml` parsing (`extension/read.rs:412-542`), Pass-2
stage wiring (`stage/stages/engine_execution.rs:230-236`), `DocumentProfile` +
two-pass orchestrator (`project/orchestrator.rs:1038`, `document_profile.rs:278`).

## Driver

**The resolved execution languages will feed the LSP, and that indexing data
only comes from Pass-1** (the indexing pass, before render) — so engine
resolution needs to happen there rather than in Pass 2. The `DocumentProfile`
is the project's per-doc index record; a doc's resolved engine set and
language→engine ownership belong on it for the LSP (and, later, freeze
planning and kernel pooling) to consume without running Pass 2.
`resolve_engines` is a **pure function** of `(meta, ast, registry, claimed)`
and is deliberately availability- and capability-blind
(`engine-resolution.md` §9, §10), so the result is deterministic,
environment-independent, and stampable on `DocumentProfile`.
The original design gated the lift on **every** engine in the project being
fully static; this plan replaces that with a **per-doc "resolution provably
needs no load"** test plus two static-metadata inputs — **claim tables**
(metadata-supplied replacements for an engine's `_extension.yml` claims) and
**`generated-languages`** — that extend the set of docs that qualify.

The claim-table input doubles as the **backward-compatibility escape hatch**:
a legacy extension that declares no static `claims:` normally forces every
doc's resolution to render time, but a user can supply that engine's complete
claim table in `_quarto.yml` and the whole project becomes Pass-1-resolvable —
without touching the extension.

## Settled decisions

Ratified with Gordon 2026-07-02 through 2026-07-05:

1. **V1 lift shape = load-free-only stamp.** Pass-1 stamps a *complete*
   resolution only when the needs-no-load predicate passes; otherwise the
   profile field stays `None` and Pass-2 resolves exactly as today. **No
   partial/pending representation, no Pass-1 engine loading.** The driving
   consumer, the LSP, **tolerates `None`**: it degrades gracefully for
   fall-through docs rather than requiring every doc resolved. Options A
   (load contested imports at Pass-1) and B (partial + pending set) stay
   deferred until a consumer needs the *complete* set — freeze and pooling
   don't exist yet (`use_freeze` is hardcoded `false` at every construction
   site: `render.rs:643`, `pass2_renderer.rs:809,1066`; Plan 5 is a stub;
   `ProjectIndex` carries no engine data today).

2. **Claim tables are whole-table replacement — a claim *source*, not a
   tier.** Metadata may supply an engine's **complete** claim table, with the
   same schema and semantics as a `claims:` block in `_extension.yml`: when a
   table is present for engine E, it **replaces E's entire claim surface** —
   E answers every `claims_language` query from the table without loading;
   a language absent from the table is not claimed by E — unless the table
   carries a universal `fallback:` entry, which (as in `_extension.yml`)
   claims every language at fallback kind. The T1–T4 resolution
   tiers are **unchanged** and still arbitrate between engines; a table only
   changes where one engine's answers come from. Source precedence per
   engine: doc-level `engine:`-entry table > `engines:` table >
   `_extension.yml` static > dynamic `claims_language`. Winner takes all —
   no per-language merging across sources.
   Consequences, all deliberate:
   - **Load-freedom:** table present ⇒ the engine is load-free for
     resolution, exactly like static `_extension.yml` claims. Supplying a
     table for a claims-less legacy engine makes it Pass-1-compatible.
   - **Empty table is meaningful:** `claims: {}` / `claims: []` = "this
     engine claims nothing" — a full mask, and load-free. (Power-user move:
     `engines: [{jupyter: {claims: []}}]` disables jupyter's universal
     fallback project-wide.)
   - **Masking works, including built-ins:** a table for knitr containing
     only `r: primary` suppresses reticulate — the §12 motivating example.
     Interception is engine-agnostic; built-ins need no `_extension.yml`.
     Built-ins have no load moment, so no validation ever applies to their
     masks (intentional).
   - **Claim kinds keep their §3/§4 meanings.** A `fallback:` entry in a
     table is a real floor (T2/T4); an `interop` entry is presence-gated by
     the tiers as usual. Nothing about a table outranks another engine's
     Primary by fiat.
   - **Forcing ownership is priority-based and best-effort.** To route a
     language to engine E against another engine's static Primary, give E's
     table entry a higher priority (`{kind: primary, priority: 999}`).
     Against an *unoverridden dynamic* engine nothing can be guaranteed at
     Pass-1 (the doc falls through and Pass-2 loads the contender). Tables
     configure an engine; they do not overrule the resolution model.
   - **No §3.3 validation for user tables.** The user is deliberately
     overruling the engine; the table is authoritative. The author-side
     `_extension.yml` hard-error validation in `TsEngine::ensure_loaded`
     (`ts_engine.rs:223`, validation loop ~247-291) is untouched — while a
     user table shadows an engine, its own claims are simply never consulted,
     so there is no comparison moment. (A load-time "your table diverges from
     the engine's actual claims" advisory is future polish, not in scope.)
   - **Failure story for a nonsensical table (§10 gating applies as
     written):** in a **multi-engine** sequence, an owner that cannot execute
     an owned language fails loudly at execute (§10 case 4). In a
     **single-engine** sequence the engine self-selects and passes unowned
     cells through as display code — silently, Q1-parity. Tables do not
     change this gating; the contract documents it (Phase 0).

3. **Two keys, two roles — `engine:` names the engines at play; `engines:`
   configures engines without naming any.** The full user-facing grammar,
   Q1 comparison, and key history live in the companion contract
   `claude-notes/designs/engine-and-engines-keys.md` (2026-07-06); this
   decision records the plan-relevant specifics. Confirmed against Q1
   source 2026-07-05:
   - **`engine:`** — read from **merged metadata** (all layers; a deliberate
     q2 divergence from Q1, which reads it from file frontmatter only,
     `engine.ts:149-170`; ratified: q2 always uses merged metadata). An
     explicit `engine:` declares the doc's sequence: T4 off (§4.3), listed
     engines present by declaration, order significant. Entries may carry a
     claim table as **doc-level sugar** (`engine: [{jupyter: {claims: [r]}}]`)
     — the table feeds decision 2's source precedence; the entry otherwise
     behaves like any explicit listing. The `claims` key is **stripped** from
     the config threaded to the engine at execute time (resolution metadata,
     reserved by this schema). The top-level engine-key shorthand
     (`knitr: {claims: ...}` with no `engine:` list) participates
     identically — it lands in `raw_explicit` with its config.
     Same-name entries across layers keep today's semantics: default
     `MergeOp::Concat` puts project entries first
     (`quarto-config/src/merged.rs:294-319`, test `engine_merge.rs:65-72`)
     and dedup keeps the **first** occurrence including its config
     (`detection.rs:253-268`), so the project's entry wins — surfaced by a
     `ConflictingDuplicateEngineConfig` warning pointing at `!prefer`
     (`engine_merge.rs:84` `prefer_replaces_engine_array`). Users who want
     project-wide engine *config* without sacrificing the jupyter fallback
     are directed to `engines:` instead (user docs, Phase 6).
   - **`engines:`** — Q1's project-config key (`project.config?.engines`,
     Q1 `engine.ts:223`; top-level in `_quarto.yml`), kept as a
     **Q1-syntax-compatible array**. Q1 entry forms: engine-name strings and
     `{path: ...}` objects (external-engine loading — q2's counterpart is
     `_extensions/` discovery; `path`-keyed entries are **left to Plan 4b**
     and ignored by this plan's table reader). q2 adds **single-key map
     entries `{<engine-name>: {claims: <table>}}`**. `engines:` affects
     ordering (Plan 4b Task 9 owns that splice — cross-referenced both ways)
     and claim tables (this plan) and **nothing else**: it never makes a
     sequence explicit, never disables T4, never seeds presence. Keys other
     than `claims` in an entry's config are ignored and documented as
     reserved. **Unknown-name policy (ratified 2026-07-06), two grains:**
     a *project-level* `engines:` map entry naming an engine not in the
     registry is a **hard error at registry/`ProjectContext` construction**
     — once, early, before Pass-1, with Q1's message ("…specified in the
     list of engines in the project settings but it is not a valid engine.
     Available engines are …"). This is the same site and message as 4b-C's
     validation-parity item (the Task-9 marker in `build_engine_registry`,
     `project/mod.rs:749`), which extends the identical check to *string*
     entries when the ordering splice lands — one validation home for the
     key. All *per-doc-layer* unknown names (doc-frontmatter `engines:`
     tables, `engine:`-entry tables) **warn-and-skip** via `ResolutionNote`
     — the resolver stays pure and infallible; `notes` remain
     advisory-only. Because the resolver reads merged metadata, a
     doc-frontmatter `engines:` block is technically effective; it is
     documented as project-level, and restricting it is out of scope
     (needs schema validation, which runs before metadata merge and is not
     fully enabled yet).

4. **Both metadata inputs are implemented in this plan** (claim tables and
   `generated-languages`), not folded into plan1c. plan1c gets only a
   cross-reference (Phase 6).

5. **The primary user-facing surface is a warning at index-pass completion,
   with the *engine* as the subject.** When Pass-1 cannot resolve engines for
   all documents, the orchestrator prints one warning the moment `pass_one`
   returns, naming each engine that must load to answer language claims (with
   its `_extension.yml` path), the affected portion of docs as an impact
   clause, and both fixes (author-side `claims:` in `_extension.yml`;
   user-side `engines:` table in `_quarto.yml`). See Phase 5 § Warning for
   the message spec. No per-doc language reporting — fall-through is a
   project-grain condition (one claims-less engine affects every doc that
   isn't otherwise exempt), so per-doc detail adds plumbing, not signal.
   The `QUARTO_PERF_STATS`-gated lifted/fell-through counter stays as quiet
   telemetry. Longer-term homes (a render-free registry check in `q2 check`,
   `crates/quarto/src/commands/check.rs`; an LSP diagnostic on the
   `_extension.yml` itself) are strand candidates, not this plan — the
   warning is human-facing only (the LSP reads profile fields directly), so
   relocating it later has no architectural cost.

6. **Reduced profile type**: stamp a purpose-built
   `ProfileEngineResolution { sequence: Vec<String>, ownership }` — names
   only, no `ConfigValue` blobs. Enough for every foreseen consumer (LSP =
   language→engine ownership; freeze key = resolved engine set; pooling =
   engine names). **The field reports resolved engines only:** a fall-through
   doc's profile says `engine_resolution: None` and nothing more. Pass-2
   keeps re-resolving via the pure function; nothing reads configs from the
   profile.

7. **No tier-dominance shortcut for unoverridden dynamic engines.** "A static
   `Primary` beats anything a non-static engine could declare" is unsound —
   the unloaded engine could declare `Primary(999)` (priority orders within
   kind, §3.1). Without a load **or a claim table**, a claims-less engine's
   claims cannot be bounded; one such engine makes every doc with uncovered
   computational languages fall through. That is correct, and it is what the
   counters and the warning (decision 5) surface.

8. **`generated-languages` (static handoff-target declaration):** flat
   top-level string list, consumer-only, no per-engine attribution; ordering
   is controlled by the explicit `engine:` list, never by this key;
   `HANDLED_LANGUAGES` exclusion applies transitionally (drains via Plan 8);
   arrays Concat-merge across layers (union), dedup on read. **A cell-less
   doc with `generated-languages` is a no-op** — the key is consulted only
   when the doc's language scan is non-empty, so such a doc stays markdown
   passthrough (a warning can be added later if this surprises anyone). No
   declared-but-unclaimed warning (in implicit sequences jupyter's
   `Fallback(0)` claims everything, so it could effectively never fire).
   The T9 handoff-loss ratification (§6.1) is *not* relaxed —
   `generated-languages` is the static escape, never runtime sequence growth.

9. **Engine-extension `_extension.yml` bytes join the Pass-1 cache key.**
   The stamped `engine_resolution` is a function of the registry — which
   engine extensions exist and what claims they declare — and none of that
   was in `pass1_key`'s hash domain (`cache_key.rs:141-178`;
   `extension_contributions` is passed empty at `orchestrator.rs:1639` with
   a format-extensions TODO). Without this, editing an extension's `claims:`
   (exactly what the decision-5 warning tells developers to do) would serve
   stale cached profiles to the LSP. Claim tables need no key change — they
   live in `_quarto.yml`/frontmatter/`_metadata.yml`, all already hashed.
   Engine bundles (`.js`) stay **out** of the key: resolution is load-free by
   design and never reads them. (Known pre-existing residue: a claimed
   non-QMD file's Pass-1 conversion runs `markdown_for_file` in the engine's
   `.js`, which is unhashed — out of scope, disclosed.)

## Rebase note (latest: 2026-07-06, base `ba2802d3c` → `c8b1eebb8`)

**2026-07-06 rebase onto the feature tip `c8b1eebb8` (Plan 4d).** Two things:

- **`resolution.rs` citations refreshed `+6`** for the landed BUILTIN_ORDER
  fix (`4f55da534`), which inserted 6 doc-comment lines at `:57`. All 20
  `resolution.rs:NNN` citations below are against the post-fix file; other
  code citations are unchanged (4d did not touch `resolution.rs`).
- **Plan 4d touched `engine-resolution.md` (§5/§9/§13)** — it adds
  `owned_languages(k) = { lang : ownership[lang] == k }`, a *positive
  projection of the same ownership map this plan stamps*
  (`ProfileEngineResolution.ownership`), plus an `owned_languages_for`
  accessor (§9) and a parity item (§13). Additive, informational, no design
  conflict — but **this plan's Phase 0 edits to §9 and §13 must coexist with
  4d's additions, not overwrite them** (both are views of the one ownership
  map). No profile change needed: a consumer wanting `owned_languages` can
  project it from the stamped `ownership` pairs.

Earlier rebase (2026-07-03, `115ea5dca` → `ba2802d3c`) — two upstream changes
that still matter:

- **4c0 Vec-per-language claims (plan4c, COMPLETE):** static claims are
  `Option<HashMap<String, Vec<StaticLanguageClaim>>>` (on both
  `EngineContribution::External.claims`, `types.rs:115`, and
  `TsEngine.claims`, `ts_engine.rs:141`). `parse_claims_map` returns the
  Vec-valued map (`read.rs:449-461`); `parse_static_language_claims`
  (`read.rs:469`) accepts a per-language **sequence of claim objects**;
  `combine_claims(&[StaticLanguageClaim], first_class)` (`types.rs:230`,
  public) reduces a language's Vec to its strongest applicable claim via the
  private `ClaimKind::combine_rank()` (Primary=2, Interop=1, Fallback=0).
  Claim tables reuse this model and parser wholesale. plan4c explicitly
  defers the Pass-1 lift to this plan (its lines 904-906).
- **`resolve_engines` wrapper split:** the public `resolve_engines`
  (`resolution.rs:340`) is a thin wrapper emitting a `tracing::info!` event
  around **`resolve_engines_inner` (`resolution.rs:360`)**, which holds the
  body. **All resolver-body edits in Phases 2-4 target
  `resolve_engines_inner`.**

## Post-4b reconciliation (do at the rebase onto the post-4b integration tip)

Plan 4b executes first (ratified 2026-07-06). Several spots below still read as
if **this** plan creates the shared `engines:` validation seam — but 4b lands
it first, so this plan **inherits** it. Do NOT pre-edit these now: they would
describe 4b code that has not merged yet (the whole reason for the ordering).
Reconcile them when rebasing this branch onto the post-4b tip, alongside the
usual citation refresh:

- **Decision 3**, the "4b-C *later* extends the same check to string entries
  when the ordering splice lands" clause → invert: 4b already validates *every*
  entry form (string + single-key-map key) at `build_engine_registry`
  construction; this plan reuses 4b's entry→name parser and adds only the
  claim-table extraction.
- **Phase 3 "Project-load validation" item** → it is **not** new validation
  under 4b-first; 4b provides it. This plan's remaining work at that site is
  the **tabled-name-set stash** + the claim-table map, both built on 4b's
  parser. Reframe from "validate … → hard error" to "reuse 4b's validated
  entry parser."
- **Phase 3 build-map (b) note** — the "4b-C later extends" citation, same flip.
- **Phase 6 reconciliation item for 4b** → the coordination note was already
  added to 4b (`71cf07394`, 2026-07-06); downgrade the item from "add" to
  "verify 4b's note still matches the final entry grammar."
- **`unknown_engine_in_project_engines_errors` test** → keep only if it adds
  map-entry-specific coverage 4b's validation test lacks; otherwise it
  duplicates 4b.

## Code facts the design rests on (verified 2026-07-02/05)

- **There is no resolver-level "hint pre-filter" for language claims.**
  T1 consults *every* candidate engine for *every* doc language
  (`resolution.rs:459-475`). The load-avoidance is distributed inside
  `TsEngine`: static `claims:` answers without loading
  (`ts_engine.rs:632-667`); only `claims: None` falls to `ensure_loaded`
  (`:668-697`). `file_extensions` pre-filters *file* claims only
  (`:700-716`). There is no static surface that eliminates a claims-less
  engine from *language* contention — hence decision 7.
- **`detect_engines` returns duplicates intact** (`detection.rs:206-227`);
  only `detect_engine_sequence` dedups. So `resolve_engines_inner`'s
  `raw_explicit` sees both configs of a cross-layer duplicate, and the
  duplicate-config warning is emittable in the resolver (decision 3).
- **`engines:` survives into merged metadata.** `resolve_format_config`
  strips only the `format` key (`quarto-config/src/format.rs:79-99`); all
  other top-level `_quarto.yml` keys flow through the merge. The resolver
  reads both `engine:` and `engines:` from `meta` — no new inputs, no layer
  plumbing.
- **`parse_claims_map` accepts only per-language forms** (`read.rs:449-461`):
  values may be `true`/int/`fallback:`-map/kinded map, or a per-language
  sequence of claim objects. The **top-level list-of-language-names shorthand
  (`claims: [r, python]`) is genuinely new work** (Phase 1) — a different
  shape at a different nesting level from 4c0's per-language list form.
  Unparseable entries are silently skipped (`filter_map`), and a language
  whose Vec parses empty is dropped (`:456`) — Phase 1 must preserve
  "table present but empty" as `Some(empty)` for the override context
  (decision 2's mask semantics).
- **Pass-1 today**: the profile pipeline is exactly `EngineClaimsFileStage →
  ParseDocument → MetadataMerge → IncludeExpansion → DocumentProfileStage →
  LinkResolution` (`orchestrator.rs:1704-1723`); no `resolve_engines`. The
  file-claim conversion runs in both passes (`engine_claims_file.rs:125-165`;
  `ctx.claimed_engine_name` at `stage/context.rs:177`).
- **`DOCUMENT_PROFILE_VERSION` is 6** (`document_profile.rs:60`). This plan
  bumps **6 → 7**. The version is folded into the Pass-1 cache key
  (`cache_key.rs:147`), so the shape change self-invalidates old entries
  once; ongoing registry-change invalidation is decision 9's key extension.
- **Pass-2 does not need the stamped field.** `EngineExecutionStage` calls
  the pure `resolve_engines` on the re-parsed doc
  (`engine_execution.rs:230-235`) and stashes it on ctx (`:236`); the profile
  field exists for project-scoped consumers (index, freeze, pooling).
  Engine diagnostics there are plain `DiagnosticMessage::warning(...)` with
  **no `Q-*` codes** — the note drain (Phase 3) follows that convention.
- **`print_pass1_stats_if_enabled` is defined in the orchestrator
  (`orchestrator.rs:151-158`) but called from the CLI**
  (`crates/quarto/src/commands/render.rs:855`). The decision-5 warning's
  emission in `run_inner` (after `pass_one` returns at `:831`, before
  `pass_two` at `:867`) is a **new** emission point inside `quarto-core`,
  which compiles for WASM — the `eprintln!` must be verified WASM-safe or
  cfg-gated native. Behaviorally WASM can't fire it anyway (its registry has
  no TS engines, so every engine is load-free).
- **Project diagnostics print with `to_text(None)`** at
  `crates/quarto/src/commands/render.rs:915` (the CLI, not `quarto-core`) —
  which is why the warning carries a plain `_extension.yml` path rather than
  a `SourceInfo` span (the extension reader registers no `SourceContext`;
  the spanned fix is the out-of-scope reader-diagnostics strand).
- **Q1 grain, for the record (2026-07-05):** Q1 reads `engine:` from file
  frontmatter + CLI flags only (`engine.ts:149-170`) and `engines:` from
  project config only (`engine.ts:223`). q2 deliberately reads both from
  merged metadata (decision 3).
- **Resolved upstream (2026-07-06, `4f55da534` on the epic branch):**
  `registry.rs`'s `engines_in_order` had its own `BUILTIN_ORDER` with
  markdown first, diverging from `resolution.rs:63` while its docstring
  claimed parity. Behaviorally unobservable (markdown claims nothing and
  never co-occurs in a sequence), fixed by sharing the resolver's constant
  + a regression test. Rebasing this branch picks it up; no plan-6 work.

## The needs-no-load predicate (final form)

Let `languages = computational_languages(ast)`, and when that scan is
non-empty, `∪ meta.generated-languages` (minus `HANDLED_LANGUAGES`,
transitional). Let `tables` = the per-engine claim tables from merged
metadata (decision 2/3). A doc resolves **load-free at Pass-1** iff any of:

- **P1** — the file is claimed (`ctx.claimed_engine_name` is `Some`): the §8
  short-circuit consults no claims at all (`resolution.rs:370-375`). (The
  file-*claim* itself may have loaded a content-inspecting engine in
  `EngineClaimsFileStage` — pre-existing Pass-1 behavior, not resolution
  loading; the predicate is about `resolve_engines`.)
- **P2** — the language scan is empty (markdown passthrough,
  `resolution.rs:380-386`; `generated-languages` not consulted, decision 8).
- **P3** — explicit `engine: markdown` opt-out (`resolution.rs:425-430`).
- **P4** — **every registered engine answers `claims_language` without
  loading**: built-in (pure Rust), statically claimed (`claims: Some` from
  `_extension.yml`), or **covered by a metadata claim table**. One registered
  engine that is claims-less *and* untabled makes every doc with a non-empty
  language set fall through — that is correct (decision 7), and it is what
  the warning and counters show.

Otherwise the doc **falls through to Pass-2** — exactly today's path. The
lift is therefore mostly **project-grain** (P4 depends on the registry and
project config), with per-doc variation via P1–P3 and doc-layer tables.

---

## Checklist

Work TDD within each phase: write the listed tests first, watch them fail,
implement, watch them pass, then `cargo nextest run --workspace`. Full
`cargo xtask verify` (NOT `--skip-hub-build`) before any push — everything
here lives in `quarto-core`, which feeds `wasm-quarto-hub-client`.

### Phase 0 — Design contracts first

The contracts are where the change is written first; plan/code edits are
downstream. A third contract, `claude-notes/designs/engine-and-engines-keys.md`
(the user-facing `engine:`/`engines:` grammar), was authored during this design
work and is **already committed** — the two edits below are the remaining
contract work. Type shapes referenced in these edits (`ResolutionNote`,
`ProfileEngineResolution`, `resolve_engines_pass1`) are **normative in
Phases 3–5** — copy them from there rather than improvising a shape the
code phases would then diverge from.

- [ ] `claude-notes/designs/engine-resolution.md`:
  - [ ] §3.3: add **claim-table sources and precedence** — metadata may
        supply an engine's complete claim table (same schema as
        `_extension.yml` `claims:`); whole-table replacement, winner takes
        all; precedence doc `engine:`-entry > `engines:` > `_extension.yml`
        > dynamic; table ⇒ load-free; empty table = full mask (and the
        disable-jupyter-fallback idiom); built-ins maskable, never
        validated; user tables skip the §3.3 author validation (no
        comparison moment while shadowed; divergence advisory = future
        polish); forcing ownership is priority-based/best-effort. Document
        the **two-key model**: `engine:` names the engines at play (explicit
        sequence, T4 off, presence, order; may carry tables as sugar, with
        the `claims` key stripped from execute config); `engines:` (Q1
        array, project config; q2 single-key-map entries add tables)
        configures without naming — never touches T4/presence/sequence.
        Record the q2-divergence: both keys read from merged metadata.
  - [ ] §3.2/§3.3: document the top-level list shorthand
        (`claims: [a, b]` → `Primary(default)` each) and disambiguate from
        4c0's *per-language* claim-object sequence.
  - [ ] §3.3: relax the project-wide "every engine fully static" gate to the
        per-doc predicate (P1–P4); record that the tier-dominance shortcut
        was considered and rejected as unsound (decision 7).
  - [ ] §4.1: `languages = scan(ast) ∪ generated-languages` (generated
        consulted only when the scan is non-empty); generated entries carry
        `first_class = None`.
  - [ ] §6.1: note `generated-languages` as the *static* escape from the T9
        handoff-loss limitation (no runtime re-resolution).
  - [ ] §7: rewrite Pass placement — per-doc lift + fall-through; resolution
        result is stamped complete-or-absent (decision 1), never partial.
  - [ ] §9: the profile artifact is the reduced `ProfileEngineResolution`
        (names only); `EngineResolution` stays the Pass-2 `StageContext`
        artifact; document the new `notes: Vec<ResolutionNote>` warning
        channel (purity preserved — warnings are returned data). While
        here, fix §9's stale sketch: `ownership` is a `LinkedHashMap`
        (`resolution.rs:286`), not the `HashMap` §9 shows — the profile's
        `Vec<(String, String)>` conversion relies on insertion order.
  - [ ] §10: one clarifying sentence at case 4 — a nonsensical claim table
        follows the existing gating: loud in multi-engine sequences, silent
        display-code pass-through in single-engine sequences (decision 2).
  - [ ] §12: promote "Pass-1 resolution" and "project-level claim overrides"
        from future to in-scope (the latter is decision 2/3's design);
        restate the freeze-key caveat (a doc whose profile field is `None`
        cannot be frozen until resolution completes — forces option A *for
        freeze*, later).
- [ ] `claude-notes/designs/document-profile-contract.md`:
  - [ ] Add `engine_resolution: Option<ProfileEngineResolution>` to the
        fields table (`None` = fell through / not load-free — NOT an error).
  - [ ] Reconcile the "no engine output on the profile" wording: resolution
        is pure/pre-load and profile-eligible; execution *results* still are
        not.
  - [ ] Fix the stale header (`Version tag: … = 2` — versions 3–6 never
        updated it) and add the 6 → 7 changelog entry.
- [ ] Commit: `docs(plan6): contract edits for Pass-1 engine resolution`.

### Phase 1 — Claim-schema list shorthand + empty-table semantics (shared parser)

One claim schema in three places (`_extension.yml`, `_quarto.yml`, doc
frontmatter); the widening lands once in the shared parser.

- [ ] Tests first, in `crates/quarto-core/src/extension/read.rs` unit tests
      (existing module test convention). Post-4c0, the map is Vec-valued
      (`HashMap<String, Vec<StaticLanguageClaim>>`):
  - [ ] `parse_claims_list_shorthand_primary_default` — `claims: [r, sql]` →
        each language maps to `vec![StaticLanguageClaim { kind: Primary,
        priority: None, when_class: None }]` (a Vec of one).
  - [ ] `parse_claims_list_shorthand_skips_non_string` — `claims: [r, 3]` →
        the non-string entry is skipped (same lenient behavior the map parser
        uses for unparseable entries), `r` still parsed.
  - [ ] `parse_claims_list_shorthand_distinct_from_4c0_form` — the
        *top-level* string list is not confused with 4c0's *per-language*
        claim-object sequence: `claims: {sql: [{kind: primary}, {kind:
        interop}]}` still parses via `parse_static_language_claims`
        (`read.rs:469`) exactly as before.
  - [ ] `parse_claims_empty_table_yields_empty_map` — `claims: {}` and
        `claims: []` parse to an **empty map** (no error, no skip). The
        parser's return type stays a bare `HashMap` — present-vs-absent is
        the **caller's** key-presence check (`config.get("claims").is_some()`),
        which Phase 3's table builder uses to implement decision 2's mask
        semantics. `_extension.yml` behavior for empty claims is unchanged.
  - [ ] Existing map-form tests stay green (regression).
- [ ] Widen `parse_claims_map` (`read.rs:449-461`) to accept a **top-level
      YAML sequence of strings** in addition to the per-language map —
      detect the seq-of-strings case **before** the existing
      `ConfigValueKind::Map` guard; each string `lang` →
      `vec![StaticLanguageClaim { kind: Primary, priority: None, when_class:
      None }]` (mirrors §3.2's `true` normalization). Map form, 4c0's
      per-language claim-object sequences, and the `fallback:` entry are
      unchanged. No `Option` in the return type: table-present-but-empty vs
      table-absent is distinguished by the caller's key-presence check (see
      the empty-table test above).
- [ ] Raise `parse_claims_map` (and the claim-object helpers Phase 3 needs)
      from module-private to **`pub(crate)`** — Phase 3 calls them from
      `engine/resolution.rs` (cross-module within `quarto-core`).
- [ ] Run the tests; `cargo nextest run -p quarto-core`; commit.

### Phase 2 — `generated-languages` (static handoff-target declaration)

- [ ] Tests first, in `crates/quarto-core/src/engine/resolution.rs` unit
      tests (mock-registry convention already used there):
  - [ ] `generated_language_gets_an_owner` — register a mock
        `Primary(codegen)` engine; doc with only `{codegen}` cells +
        `generated-languages: [python]` + implicit sequence → ownership
        `codegen → codegen-engine` AND `python → jupyter` (T4), sequence
        `[codegen-engine, jupyter]`.
  - [ ] `generated_language_static_primary_owner` — same, with a mock static
        `Primary(python)` engine → that engine owns python.
  - [ ] `generated_language_dedup_and_union` — duplicate entries, and entries
        already present as real cells, dedup to one language occurrence.
        **Pin the `first_class` tie:** a generated entry duplicating a real
        cell's language must NOT overwrite the cell's first-occurrence
        `first_class` (assert `{python .marimo}` cell + `generated-languages:
        [python]` keeps `first_class = Some("marimo")`) — generated entries
        only *add* languages not already present, mirroring
        `computational_languages`' first-occurrence-wins dedup
        (`resolution.rs:256-262`).
  - [ ] `generated_language_in_handled_languages_excluded` —
        `generated-languages: [mermaid]` → excluded (transitional, Plan 8).
  - [ ] `generated_language_alone_is_noop` — doc with **no** computational
        cells + `generated-languages: [python]` → markdown passthrough
        (empty sequence), identical to the key being absent (decision 8).
- [ ] Parse `generated-languages` in `resolve_engines_inner`: **after** the
      empty-scan early return (`resolution.rs:380-386`), read
      `meta.get("generated-languages")` as a string array (Concat-merged
      union across layers is free), dedup, subtract `HANDLED_LANGUAGES`,
      append to the scan result with `first_class = None`. **Read each
      element with `as_plain_text()`**, not `.as_str()` — bare YAML strings
      in front-matter context are `ConfigValueKind::PandocInlines` (the
      `metadata-as-str` lint enforces this). No ordering logic: ordering is
      the explicit `engine:` list's job, never this key's.
- [ ] Run tests; workspace suite; commit.

### Phase 3 — Claim tables (whole-table replacement source)

- [ ] Tests first (`resolution.rs` unit tests):
  - [ ] `table_beats_static_priority` — registered mock static `Primary(r)`
        engine "otherengine" is beaten by `engine: [{jupyter: {claims:
        {r: {kind: primary, priority: 5}}}}]` at T1 by priority (5 > 1).
  - [ ] `tabled_engine_not_consulted` — the **tabled engine is the counting
        mock**: register mock "legacy" whose `claim_fn` increments an
        `Arc<AtomicUsize>` (the existing `MockEngine` stores a `Box<dyn Fn>`
        closure, `resolution.rs:613-627` — no new infra); supply
        `engines: [{legacy: {claims: [python]}}]`; resolve a `{python}` doc;
        assert legacy owns python AND the counter is **zero**.
  - [ ] `table_list_shorthand` — `claims: [r]` behaves as `r: primary`.
  - [ ] `engines_key_supplies_table` — project-surface: meta with
        `engines: [{legacy: {claims: [python]}}]` (no `engine:` key) → the
        registered claims-less mock "legacy" wins `python` at T1 from the
        table; the sequence **stays implicit** (T4 still available for other
        languages; no presence seeding from `engines:`).
  - [ ] `engine_entry_table_wins_over_engines_key` — both surfaces supply a
        table for the same engine → the `engine:`-entry table replaces the
        `engines:` table **entirely** (whole-table, no per-language merge).
  - [ ] `masking_suppresses_interop` — the §12 example: table for knitr =
        `{r: primary}` only, doc `{r}` + `{python}`, implicit → r → knitr,
        python → jupyter (T4); knitr's reticulate Interop is masked.
        (Also pins that built-ins are tableable.)
  - [ ] `empty_table_masks_engine` — `engines: [{jupyter: {claims: []}}]` +
        implicit doc `{python}` with no other claimant → **`ownership`
        empty, `sequence` empty** (the markdown-passthrough shape, reached
        because jupyter's universal fallback is disabled; the doc still
        lifts at Pass-1 once Phase 4 lands — every engine is load-free —
        stamping that empty resolution).
  - [ ] `table_when_class_gating` — a table entry with `whenClass` applies
        only when the language's `first_class` matches (reuse
        `combine_claims`, `types.rs:230` — it already applies `when_class`
        gating and per-Vec reduction).
  - [ ] `table_fallback_is_a_real_floor` — a table giving engine E
        `fallback: {}` loses language L to another engine's static
        `Primary(L)` — kinds keep their tier meanings (decision 2).
  - [ ] `unknown_engine_in_project_engines_errors` — **not a resolver test**:
        lives with the `build_engine_registry` tests (`project/mod.rs`).
        A project config whose `engines:` map entry names an unregistered
        engine → `ProjectContext` construction fails with the Q1-parity
        message (decision 3's project grain). String entries are NOT
        validated here (4b-C's item).
  - [ ] `unknown_engine_in_entry_warns` — `engine: [{ghost: {claims: [r]}}]`
        with no registered `ghost` → `ResolutionNote::UnknownOverrideEngine`,
        entry's table ignored, tiers proceed. Same expectation for an
        unknown name in a *doc-layer* `engines:` table reaching the
        resolver (decision 3's doc grain) — cover both surfaces in the
        test.
  - [ ] `duplicate_engine_conflicting_config_warns` — same engine name twice
        in the merged `engine:` array with differing configs →
        `ResolutionNote::ConflictingDuplicateEngineConfig` (message names
        `!prefer` and `engines:`); first occurrence's config used
        (decision 3). Content-only comparison (see `config_content_eq`
        below); identical content at different source offsets → no note.
  - [ ] `claims_key_stripped_from_execution_config` — the config attached to
        the resolved engine (`DetectedEngine.config`) has the `claims` key
        removed but keeps sibling keys (e.g. `{jupyter: {claims: [r],
        kernel: python3}}` → config `{kernel: python3}`).
  - [ ] `tiers_unchanged_without_tables` — no tables anywhere → byte-identical
        behavior to today (regression pin for the interception seam).
- [ ] Implement in `resolve_engines_inner` (`resolution.rs:360`):
  - [ ] Add `notes: Vec<ResolutionNote>` to `EngineResolution`
        (`resolution.rs:278-287`) with
        ```rust
        #[derive(Debug, Clone, PartialEq)]
        pub enum ResolutionNote {
            UnknownOverrideEngine { engine: String },
            ConflictingDuplicateEngineConfig { engine: String },
        }
        ```
        Purity preserved: warnings are returned data. Initialize `notes` at
        the three early returns (`resolution.rs:371,376,420`) and the final
        build (`:577`) — four sites, all inside `resolve_engines_inner`.
  - [ ] **Project-load validation (decision 3, project grain):** after the
        registry is assembled at `ProjectContext` construction, validate the
        project config's `engines:` **map-entry names** against it — unknown
        name → hard error, Q1 message (site: the Task-9 marker in
        `build_engine_registry`, `project/mod.rs:749`; 4b-C later extends
        the same check to string entries). While there, **stash the
        validated tabled-name-set** (e.g. a `HashSet<String>` on
        `ProjectContext` next to the registry) — Phase 5's warning consumes
        it, and the parse is shared with the resolver's table reader
        (same `pub(crate)` parser, no drift).
  - [ ] **Build the per-engine table map** from merged metadata:
        (a) `meta.get("engines")` array — for each single-key map entry
        keyed by a registered engine name, parse its `claims` via the shared
        `parse_claims_map` (Phase 1); `path`-keyed entries skipped (Plan 4b
        territory); non-`claims` config keys ignored (reserved); unknown
        engine name → **note + skip** (project-layer unknowns were already
        rejected at load, so a name reaching this path is doc-layer —
        decision 3's doc grain).
        (b) each `raw_explicit` entry (`resolution.rs:393-419` — covers both
        the `engine:` list and the top-level shorthand) whose config has a
        `claims` key — parsed the same way; unregistered name → note + skip.
        Precedence: (b) replaces (a) per engine, whole-table — no
        per-language merge across the two surfaces.
  - [ ] **Interception seam:** a private
        `claim_for(engine_name, lang, first_class)` helper used by **all
        four tiers** (T1 `:453-469`, T2 `:474-494`, T3 `:496-519`,
        T4 `:521-543`): if the table map has the engine, answer via
        `combine_claims` on the table's Vec for `lang` (absent language →
        `None`; universal `fallback:` entry per the existing two-site
        combine idiom); otherwise call `engine.claims_language`. No other
        tier logic changes; no ownership guard is added (nothing owns
        anything before T1 runs).
  - [ ] **Strip the `claims` key** from the config attached to resolved
        owners in the sequence build (explicit-config lookup built at
        `resolution.rs:559-567`, consulted at `:564-574`): `claims` is
        resolution metadata, not engine execution config — without the strip
        it leaks into `ExecutionContext` via `with_engine_config`. Sibling
        keys pass through untouched. (`engines:` entries never feed execute
        config — no leak path there.)
  - [ ] Emit the duplicate-config note where the explicit-config lookup is
        built. **"Differing configs" must be a content-only comparison, and
        no helper exists** (`ConfigValue` has only the derived `PartialEq`,
        which includes `SourceInfo`). Write a small private recursive
        `fn config_content_eq(a: &ConfigValue, b: &ConfigValue) -> bool` in
        `resolution.rs` comparing `ConfigValueKind` structure/values and
        ignoring source-info fields.
- [ ] **Wire pass-through hygiene:** `build_engine_config_map`
      (`project/mod.rs:541`) already forwards `_quarto.yml`'s `engines:`
      value verbatim onto `LaunchEngine.project.config.engines`, and Q1
      engines type it `string[]`. With map-form entries now legal, lower
      **names only**: strings pass through; a single-key map contributes
      its key; `path`-maps are skipped (no name known Rust-side until 4b).
      Unit test in `project/mod.rs` alongside the existing
      `build_engine_config_map` tests (`:2133+`).
- [ ] `EngineExecutionStage::run`: drain `resolution.notes` into
      `ctx.diagnostics` after the existing stash (`engine_execution.rs:236`).
      Each variant becomes a **warning**-severity `DiagnosticMessage` (no
      `Q-*` codes — matches surrounding engine diagnostics):
      `UnknownOverrideEngine`: "engine `<name>` in this document's claim
      configuration is not a registered engine; its claims are ignored"
      (covers both `engine:`-entry and doc-layer `engines:` sources);
      `ConflictingDuplicateEngineConfig`: "engine `<name>` appears more than
      once in the merged `engine:` list with different configs; the first
      entry wins (with default merging, the project layer's) — use
      `engine: !prefer [...]` in the document to override, or configure the
      engine via `engines:` in `_quarto.yml`". (Duplicates within a single
      layer fire the same note; the wording covers both.)
      `DocumentProfileStage` never
      drains (avoids double-reporting; a Pass-1-only doc surfaces its notes
      when actually rendered).
- [ ] Run tests; workspace suite; commit.

### Phase 4 — Load-free predicate + engine staticness surface

- [ ] Tests first:
  - [ ] Trait surface (`engine/mod.rs` / `ts_engine.rs` tests):
        `builtin_engines_are_load_free` (markdown/knitr/jupyter → `true`),
        `ts_engine_load_free_iff_static_claims` (`claims: Some` → `true`,
        `None` → `false`).
  - [ ] Predicate (`resolution.rs` tests), one per prong:
        `pass1_lifts_claimed_file` (P1), `pass1_lifts_no_languages` (P2 —
        including with `generated-languages` present, decision 8),
        `pass1_lifts_explicit_markdown` (P3),
        `pass1_lifts_all_static_registry` (P4),
        `pass1_lifts_tabled_dynamic_engine` (P4 — **the backward-compat
        case**: claims-less mock engine + an `engines:` claim table for it →
        lift; this is the feature's headline),
        `pass1_falls_through_dynamic_engine_present` (claims-less untabled
        engine + any computational language → `None`),
        `pass1_registry_grain_is_deliberate` (pins P4's conservatism: an
        explicit `engine: [knitr]` doc **still falls through** while a
        claims-less untabled engine is merely *registered*, even though
        resolving it would consult only knitr — do NOT "optimize" P4 to
        candidate engines; decision 7's unboundedness applies to the whole
        registry. Add a comment at the predicate saying so).
        **MockEngine needs a knob first:** add a `load_free: bool` field
        (default `true`) to `MockEngine` (`resolution.rs:613-627`) and
        override the trait method with it, so the claims-less-engine tests
        are expressible.
  - [ ] `pass1_result_equals_pass2_result` — for every lifted case, the
        Pass-1 resolution equals a direct `resolve_engines` call
        (purity/consistency guard). **Compare field-wise** (sequence names +
        `ownership` entries): `EngineResolution` and `DetectedEngine` do not
        derive `PartialEq` (`resolution.rs:278`, `detection.rs:37`), and the
        existing tests already compare fields individually
        (`resolution.rs:752-765`) — follow that convention.
- [ ] Add to the **`ExecutionEngine`** trait (`engine/traits.rs:61` — that is
      the trait's name; there is no `Engine` trait):
      ```rust
      /// True when `claims_language` answers without loading anything
      /// (built-ins: pure Rust; TsEngine: static `claims:` declared).
      /// Metadata claim tables make an engine load-free regardless of
      /// this answer — that is accounted for at the resolution layer,
      /// which is the only sanctioned caller for lift decisions.
      fn claims_language_is_load_free(&self) -> bool { true }
      ```
      Override in `TsEngine`: `self.claims.is_some()` (`ts_engine.rs:141`;
      matches the load-free branch at `ts_engine.rs:632-667`).
- [ ] Registry/resolution helpers: `EngineRegistry::all_claims_load_free()`
      (all registered engines answer `true`) plus a resolution-side
      `registry_is_load_free(registry, tabled: &HashSet<String>)` that
      treats a tabled engine as load-free (P4's actual test); and
      `EngineRegistry::engines_needing_load(&self, tabled: &HashSet<String>)
      -> Vec<(name, Option<PathBuf>)>` — the complement, for the Phase-5
      warning (an engine tabled in project config is *not* listed). A
      name-set suffices for both — neither consumer needs table contents.
- [ ] Add to `resolution.rs`:
      ```rust
      /// Pass-1 entry point: Some(resolution) iff resolving this doc
      /// provably consults no loadable claim (predicate P1-P4); None = fall
      /// through to Pass-2. Shares the language scan + claim-table
      /// construction with resolve_engines.
      pub fn resolve_engines_pass1(
          meta: &ConfigValue, ast: &Pandoc,
          registry: &EngineRegistry, claimed: Option<&str>,
      ) -> Option<EngineResolution>;
      ```
      Internals: factor the language scan + table-map construction out of
      `resolve_engines_inner` into private helpers so the predicate shares
      them rather than duplicating logic; route the lifted path through the
      public wrapper (or emit its own tracing event) so the wrapper split's
      observability isn't bypassed. Re-export alongside `resolve_engines` in
      `engine/mod.rs` (`:143`).
- [ ] Run tests; workspace suite; commit.

### Phase 5 — Profile stamp, version bump, cache key, counters, warning

**Two files are named `document_profile.rs` — don't conflate them:** the
`ProfileEngineResolution` type, `DOCUMENT_PROFILE_VERSION` (`:60`), and the
pinned version assert (`:1460`) live in the crate root
`crates/quarto-core/src/document_profile.rs`; the *producer* edit
(`DocumentProfileStage`, stamp pattern at `:90`) lives in
`crates/quarto-core/src/stage/stages/document_profile.rs`.

- [ ] Tests first:
  - [ ] Crate-root `document_profile.rs`: serde round-trip of
        `ProfileEngineResolution` (both `Some` and `None` on the profile);
        version-mismatch rejection unchanged; update the pinned
        `assert_eq!(DOCUMENT_PROFILE_VERSION, 6)` at `:1460` to 7.
  - [ ] Cache key: extend the relational tests (`cache_key.rs:232,351`
        convention) — same inputs + different engine-extension
        `_extension.yml` bytes → different key; adding/removing an
        extension pair → different key.
  - [ ] Fixture first (the integration test below consumes it): plan1c's
        existing extension fixtures all declare static `claims:`
        (`crates/quarto-core/tests/fixtures/extensions/…` — julia-engine,
        echo-engine, marimo) — **author a new claims-less fixture**: an
        `_extension.yml` with `path:` + `name:` but no `claims:` key.
        Registration validates the bundle file exists, so include a trivial
        stub `.js` — never executed (the predicate fails before any load).
  - [ ] Orchestrator integration test
        (`crates/quarto-core/tests/integration/`, registered in `main.rs`,
        alphabetized — per `.claude/rules/integration-tests.md`): a
        three-doc project with the claims-less extension registered. With
        that engine untabled, P4 is off for every doc — docs lift only via
        P1–P3 — so: doc A markdown-only (lifts via P2); doc B with
        computational cells and a claim table for the claims-less engine
        **in its own frontmatter via the `engine:`-entry sugar** (e.g.
        `engine: [{<fixture-name>: {claims: [python]}}]` — lifts via P4 and
        exercises the doc-layer surface; the project variant below covers
        `engines:`); doc C with cells and no table (falls through).
        Assert A and B stamp `engine_resolution: Some` with expected
        sequence/ownership, C stamps `None`. Then the project-level variant:
        add the table to `_quarto.yml` `engines:` → **all three** lift.
- [ ] Add to the crate-root `document_profile.rs`:
      ```rust
      /// Reduced, serializable form of EngineResolution for the profile
      /// (names only — configs stay in merged metadata; decision 6).
      #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
      pub struct ProfileEngineResolution {
          pub sequence: Vec<String>,                 // ordered distinct owners
          pub ownership: Vec<(String, String)>,      // language → engine, insertion order
      }
      ```
      Field on `DocumentProfile`: `pub engine_resolution:
      Option<ProfileEngineResolution>` (serde default). Bump
      `DOCUMENT_PROFILE_VERSION` to 7 with a doc-comment changelog entry
      citing this plan.
- [ ] Producer: `DocumentProfileStage`
      (`stage/stages/document_profile.rs`; verified in scope: `doc.ast.meta`,
      `doc.ast`, `ctx.registry` — `Arc<EngineRegistry>`, not `Option`,
      `context.rs:172` — and `ctx.claimed_engine_name`, `context.rs:177`)
      calls `resolve_engines_pass1` and stamps the reduced form — follow the
      existing post-`extract` stamp pattern (`profile.includes`, `:90`). The
      stage also runs in Pass-2's head pipeline (`pipeline.rs:284,508`);
      re-stamping there is harmless (pure function, same result). The Pass-1
      stage list (`orchestrator.rs:1704-1723`) is unchanged. Notes are NOT
      drained here (Phase 3 rationale). For a claimed file the stamped
      ownership is intentionally empty (§8) — consumers read `sequence`.
      **Name-collision warning:** `ctx.engine_resolution`
      (`Option<EngineResolution>`, the Pass-2 execute-stage stash) and
      `profile.engine_resolution` (`Option<ProfileEngineResolution>`, this
      stamp) are different types on different carriers — don't conflate.
- [ ] **Cache key (decision 9):** gather `(extension-name,
      _extension.yml raw bytes)` pairs for every engine-contributing
      extension, sorted by name, and pass them as
      `Pass1KeyInputs.extension_contributions` (currently hardcoded empty at
      `orchestrator.rs:1639`). **Byte source:** the registry exposes the
      `(name, _extension.yml path)` pairs (the provenance field added for
      the warning, below); the key builder **re-reads each file's bytes at
      key-build time**, exactly the per-file read idiom `_quarto.yml` and
      `_metadata.yml` already use (`pass1_read_quarto_yml_bytes`,
      `pass1_layered_metadata_raw_bytes` — small files, per-doc reads are
      the established cost model; no bytes retained at registration). An
      unreadable file hashes as empty bytes (degrades to over-invalidation,
      never a stale hit). Raw bytes, matching the `_quarto.yml` treatment
      (comment-only edits over-invalidate — the safe direction). Re-word
      the `cache_key.rs` docstring and the orchestrator TODO: the slot now
      carries engine-extension bytes; proper *format*-extension hashing
      remains the pre-existing follow-up.
- [ ] Counters: count lifted vs fell-through **from each returned profile's
      `engine_resolution` field** (`Some`/`None`) — regardless of cache
      hit/miss (a cached profile carries the field too; the version bump +
      decision 9 keep cached stamps trustworthy). **Compute the tally once,
      in `run_inner`, from the `Vec<DocumentProfile>` `pass_one` returns**
      (`:831`) — the same numbers feed both the perf counter and the
      warning below (do not fold a second count inside `pass_one`). Print
      under `QUARTO_PERF_STATS=1` with prefix
      `perf.pass1-engine-resolution`, following the
      `print_pass1_stats_if_enabled` idiom (`orchestrator.rs:151-158`; note
      that gauge is *called* from the CLI, `commands/render.rs:855` — mirror
      whichever call point fits, keep reasons coarse: `lifted` /
      `fell_through`).
- [ ] **Warning at index-pass completion (decision 5).**
  - [ ] Provenance plumbing: keep the extension's `_extension.yml` path on
        `EngineContribution::External` → `TsEngine` (one new field; the
        reader has the path in `read_extension_with_org`, `read.rs:65-66`,
        and must thread it to `parse_external_engine`, `read.rs:382`).
        A plain path, not `SourceInfo`: project diagnostics print with
        `to_text(None)` (`commands/render.rs:915`), so a span could not
        render (the spanned fix is the out-of-scope reader-diagnostics
        strand).
  - [ ] Emission: in `run_inner` immediately after `pass_one` returns
        (`orchestrator.rs:831`), before Pass-2 dispatch (`:867`). The
        engine list comes from `engines_needing_load(&tabled)` with the
        **project-grain tabled-name-set stashed on `ProjectContext` by
        Phase 3's validation item** (an engine tabled in `_quarto.yml` is
        not listed; doc-layer tables can't exempt an engine project-wide
        and don't feed this set). Build with
        `DiagnosticMessageBuilder::warning` + details + hint and `eprintln!`
        its `to_text(None)`. This is a **new** print site inside
        `quarto-core` — verify `eprintln!` is WASM-safe or cfg-gate it
        native (behaviorally WASM never fires: no TS engines in its
        registry). Do **not** push into `project_diagnostics`: advisory by
        construction (immune to a future fail-on-warnings mode, bd-creo),
        printed exactly once per render.
  - [ ] Message spec (engine as subject; portion is an impact clause;
        both fixes in the hint; no per-doc language reporting):
        ```
        Warning: engine extension `legacy-python` declares no static language claims
        (_extensions/acme/legacy-python/_extension.yml), so engine resolution must
        wait for render time. Execution-language indexing is unavailable for
        3 of 12 documents.

          hint: declare the extension's claims statically in its _extension.yml —
          e.g. `claims: [python]`, one line — or, if you cannot edit the extension,
          supply its claim table in _quarto.yml:

            engines:
              - legacy-python:
                  claims: [python]

          Affected documents will then resolve at index time. Rendering is
          unaffected.
        ```
        Multiple claims-less engines → one warning listing each engine +
        path. A no-fall-through project emits nothing.
  - [ ] Tests: unit test for the message builder (single + multiple engines;
        counts); the Phase-5 integration fixture asserts the warning fires
        and names the claims-less engine + its `_extension.yml` path in the
        no-table variant, and is silent in the `engines:`-table variant.
- [ ] `cargo xtask verify` (full — WASM leg required: `quarto-core` types
      feed `wasm-quarto-hub-client`; WASM Pass-1 runs the same stage list via
      `pass_one_dispatch_async`, `orchestrator.rs:1340`, and its registry has
      no TS engines, so every doc lifts trivially).
- [ ] Commit.

### Phase 6 — End-to-end verification, reconciliation, user docs

- [ ] **E2E (CLAUDE.md contract — real binary, inspected output, recorded
      here when done):**
  - [ ] `QUARTO_PERF_STATS=1 cargo run --bin q2 -- render <fixture project>`
        on the Phase-5 three-doc fixture **without** the project table:
        observe the index-pass warning (engine + `_extension.yml` path,
        `1 of 3` impact clause) and `perf.pass1-engine-resolution`
        `lifted=2 fell_through=1`. Then add the `engines:` claim table to
        `_quarto.yml` and re-render: warning gone, `lifted=3` — **and
        confirm the second render actually recomputes** (decision 9: the
        `_quarto.yml` edit changes the cache key; also exercise an
        `_extension.yml` edit invalidating the cache). Paste invocations +
        output snippets into this section.
  - [ ] Same invocation on `docs/` (all-builtin project): every doc lifts,
        no warning; record the counter numbers.
  - [ ] Inspect one cached Pass-1 profile JSON and confirm the stamped
        `engine_resolution` content matches the doc.
- [ ] Reconciliation edits (secondary artifacts):
  - [ ] `2026-04-16-ts-engine-extensions-subprocess.md` — update the
        "Multi-engine resolution (post-merge)" summary (Pass-2 placement +
        file-claim-only-Pass-1 wording, lines ~63-82) and this plan's row in
        the sub-plans table (research stub → implementation plan).
  - [ ] `2026-04-16-plan1c-extension-integration.md` — D1's "fully static →
        Pass-1 precondition" wording (lines 163-165) becomes the per-doc
        predicate; cross-reference this plan for the metadata inputs.
  - [ ] `2026-04-16-plan1a-engine.md` — tighten the "zero-cost Pass-1 lift"
        assertions (lines ~436-448) to "per-doc, load-free-only".
  - [ ] `2026-07-01-plan4b-shadow-engine-features.md` — Task 9 coordination
        note: this plan defines the `engines:` entry grammar (strings,
        reserved `path`-maps, name-keyed config maps with `claims`); the
        ordering splice implemented there must accept all three entry forms.
  - [ ] `2026-04-23-website-project-epic.md` — note the profile-version bump
        + new field + the cache-key extension (it owns the
        orchestrator/profile/cache).
- [ ] User-facing docs (`docs/` website — usage, not internals): the
      `engine:` vs `engines:` distinction ("names the engines at play" vs
      "configures engines"; use `engines:` for project-wide engine config if
      you want the jupyter fallback preserved); claim tables (map form, list
      shorthand, empty-table mask, whole-table semantics, `whenClass`);
      the backward-compat recipe for legacy extensions; forcing ownership
      via priority (best-effort caveat); `generated-languages`; the
      project-wins + `!prefer` note for duplicated `engine:` entries.
      Verify with `cargo run --bin q2 -- render docs/` (never Q1).
- [ ] Reconcile this checklist against reality (per finishing-a-branch
      practice), commit, then ask Gordon before any push / merge to
      `feature/ts-engine-extensions` (`--no-ff` per worktree rules).

## Explicitly out of scope

- Options A/B (Pass-1 loading, partial/pending resolution) — deferred until
  freeze or Plan 5 pooling exists; the freeze-key caveat is recorded in the
  contracts (Phase 0).
- Restricting `engines:` to project config (doc-frontmatter `engines:` is
  technically effective via merged metadata) — needs schema validation,
  which runs before metadata merge and is not fully enabled yet.
- A load-time advisory when a user claim table diverges from the engine's
  actual dynamic claims — future polish; there is no natural comparison
  moment while a table shadows the engine.
- `engines:` ordering semantics and `path:`-entry handling — Plan 4b Task 9
  (this plan only reads `claims` from name-keyed entries).
- Plan 8 work (mermaid/#241 absorption, graphviz TS extension,
  `HANDLED_LANGUAGES` drain) — a claim table naming a handled language
  (e.g. `mermaid`) is a no-op until that drain lands.
- Spanned (file:line) diagnostics from the extension reader / project
  loader — the readers discard their `DiagnosticCollector`, register no
  `SourceContext`, and `project_diagnostics` print with `to_text(None)`
  (`commands/render.rs:915`); fixing that class is a strand candidate. The
  warning carries a plain path instead.
- The `markdown_for_file` conversion gap in the cache key: a claimed
  non-QMD file's Pass-1 profile depends on engine `.js` conversion logic,
  which is unhashed — pre-existing, rare, severable (verify-on-load pattern
  available if it ever matters).
- Wire-protocol changes — none (claim tables are metadata; the profile is
  Rust-side only; the pre-existing `engines:` wire pass-through gets the
  names-only lowering in Phase 3, no shape change).
- Replay — drives from captures, untouched (§6.2).
