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
| `claims_file()` | `claims-files:` (extension + optional `content-pattern`) | extension-only claims **and** regex-expressible content sniffs (Plan 7a) | only a *non*-regex-expressible sniff — **empty across every known Q1 engine** |
| `claims_language()` | `claims:` (kind/priority/`whenClass`) | language **and** `first_class` logic (both finite/known) | only genuine runtime/global-state logic |

> **Restructure decided 2026-07-07 (Gordon) — supersedes the 2026-07-02 rename.**
> The earlier plan to rename `claims-files:` → `claims-extensions:` is
> **withdrawn.** A full census of every Q1 engine `claimsFile` (knitr, jupyter,
> markdown, julia) showed each is `extension-gate → read-file → **one regex**` —
> the knitr `spin`→Rscript work is the *conversion*, run **after** the claim, not
> part of it. So `claims-files` is a genuine **file-claim** surface (extension +
> an optional content pattern), *not* a bare extension set, and the name is
> correct. A content sniff is **data** (a regex), not a must-load operation, so it
> is **statically declarable** — overturning the old "one genuine must-load case."
> **Restructure:** `claims-files` entries become typed `{extension,
> content-pattern?}` (bare-string shorthand `- .echo` still accepted). The
> **extension-only** form lands in **plan 1c.2 P4**; the **`content-pattern`**
> field + native Pass-1/claim-stage evaluation lands in
> **[Plan 7a](../plans/2026-07-07-plan7a-static-content-pattern-claims.md)**.
> Normalization: YAML accepts dotted or undotted; parse stores canonical
> **undotted lowercase** for `file-extensions` and each `claims-files` entry's
> `extension`; the **JS/wire contract stays dotted** (Q1 `extname()`) — re-dot at
> the two Rust→TS seams (`ToEngine::ClaimsFile` construction and the
> synthetic-file load validation). `content-pattern` is evaluated **natively in
> Rust** and never crosses the wire.

**`first_class` is statically expressible — it is *not* a must-load case.**
`claims_language(language, first_class)` is a pure function of its two
arguments, so a `claims:` entry may carry `whenClass: <class>`: the claim then
applies **only** when the cell's first class equals `<class>` (absent
`whenClass` = any/no first class). A marimo engine therefore declares
`python: { whenClass: marimo, kind: primary }` and is **fully static** —
`{python .marimo}` → `Primary`, plain `{python}` / `{python .other}` → no
claim. **Content-inspecting `claims_file` is *also* statically declarable**
(corrected 2026-07-07): Julia's `isPercentScript` reading the file's bytes for
`# %%` is a **pure regex over those bytes**, so it is expressed as a
`content-pattern` on a `claims-files` entry and evaluated natively — no load
(Plan 7a). `file-extensions` remains the can-handle pre-filter; the pattern is
the definitive claim. The genuine dynamic residue — a sniff no regex can
express — is **empty across every known Q1 engine**; the dynamic `claims_file`
method survives only as a fallback for that hypothetical case. Everything —
language, `first_class`, kind/priority, fallback, **and content sniffs** — is
statically declarable.

**Vec-per-language claims (4c0).** A `claims:` entry's value is a **list** of
claim objects — `Vec<StaticLanguageClaim>` in Rust, a YAML sequence or a
single scalar/bool/int/object (back-compat 1-element-Vec shorthand) on the
wire:

```yaml
claims:
  sql:
    - { whenClass: marimo, kind: primary, priority: 2 }  # {sql .marimo} self-activates
    - { kind: interop }                                   # bare {sql} rides along
```

This is what lets one language key carry **both** a `whenClass`-conditioned
primary claim and an unconditional interop claim — marimo's bare-`{sql}`
feature: `{sql .marimo}` is `Primary` (tagged, self-activating), while a bare
`{sql}` is `Interop(0)` (rides along only when marimo already owns another
language via a positive claim). A plain scalar/object value (`echo: true`,
`fallback: { priority: 0 }`) still parses to a 1-element Vec — the pre-4c0
single-claim shape is unaffected.

**Combine rule.** `lookup_static_claim` maps every element of a language's Vec
through `static_claim_to_language_claim` (which returns `LanguageClaim::None`
on a `whenClass` mismatch), drops the `None`s, and reduces the survivors with
a dedicated per-Vec comparator (`ClaimKind::combine_rank` in
`extension/types.rs`): kind dominates priority — Primary > Interop > Fallback,
`priority` breaking ties within a kind — the same *shape* of ordering as the
cross-engine "kind dominates priority" rule below, but a separate, explicit
implementation scoped to one language key's Vec, not a reuse of the
cross-engine tiering. The universal `fallback:` key is combined the identical
way (`ts_engine.rs`'s two fallback call sites both route through the Vec
combiner), so a fallback engine can itself carry more than one claim.

**Top-level list shorthand (disambiguated from 4c0's per-language sequence).**
A `claims:` value may also be a **top-level list of language names** —
`claims: [python, r]` — rather than a map keyed by language. This is sugar
for "each named language gets a bare `Primary(default)` claim," equivalent to
`claims: { python: primary, r: primary }`. Do not confuse this with 4c0's
**per-language** claim-object sequence above (`claims: { sql: [{whenClass:
marimo, kind: primary}, {kind: interop}] }`): the top-level list is a
shorthand over the *set of languages claimed*, one default `Primary` per
entry; the per-language Vec is a sequence of *claim objects for one
language*, letting a single language carry more than one conditioned claim.
The two shapes are told apart by nesting, not by a discriminator key: a
`claims:` value that is a list *at the top level* (entries are bare language
names) is this shorthand; a `claims:` value that is a map whose *entry* value
is a list (entries are claim objects) is the 4c0 form.

A statically-declared claim used for resolution is validated against the
dynamic method **only if/when the engine loads to execute** (mismatch → hard
error, like the `name` check). Static claims are **authoritative for
resolution**; authors who declare them own their accuracy. `Fallback` cannot
be a finite language list, so a universal-fallback engine declares
`fallback:` rather than a per-language entry. Full-static resolution requires a
declared `name` (zero-load needs the name to place the engine in the
sequence). Whether resolution loads nothing is decided **per document, not
project-wide** — see "The needs-no-load predicate" below; that lift is the
payoff of static claims (§7).

**Claim tables from metadata (`engine:`/`engines:` sugar).** Metadata may
also supply an engine's complete claim table — same schema as
`_extension.yml`'s `claims:` — via a per-entry `claims:` key on either the
`engine:` or `engines:` metadata key (`engine-and-engines-keys.md` §2/§3 owns
the user-facing grammar; this is the resolution-side contract for what such a
table does). A table is a **whole-table replacement**, winner takes all: it
does not merge with the engine's `_extension.yml` claims or its dynamic
`claims_language` — it *replaces* them entirely for that engine. **Source
precedence** (highest wins, no merging across sources): a document's own
`engine:`-entry table > project `engines:` table > `_extension.yml` static
claims > the dynamic `claims_language` method. **A table makes its engine
load-free** — resolution answers every claim consultation for that engine
from the table, never loading it, which is what lets a legacy claims-less
extension become Pass-1-resolvable without an edit to the extension itself.
**An empty table (`claims: []`) is a full mask** — the engine claims nothing;
the idiom `engines: [{jupyter: {claims: []}}]` disables jupyter's universal
fallback project-wide. **Built-in engines are maskable but never validated**:
a table may replace a built-in's (knitr's, jupyter's) claims outright, and
because a masked engine never loads to compare, the author-validation
paragraph above has no comparison moment to run while a table shadows it —
reconciling the two later, as a divergence advisory, is future polish, not
required now. **Forcing ownership via a table is priority-based and
best-effort**: a table can outrank other candidates by declaring a
high-priority claim, but it competes in the same kind/priority tiering as
everyone else (§4) — against an unoverridden dynamic engine nothing can be
guaranteed at Pass-1 (the doc falls through and Pass-2 loads the contender).

**The two-key model, restated for resolution.** `engine:` *names the engines
at play*: an explicit sequence, presence for every listed engine, execution
order, and — per §4.3 — it turns T4 (implicit fallback) off. A per-entry
`claims:` inside `engine:` is sugar for a document-level table on an
already-named engine; the reserved `claims` key is stripped from the rest of
the entry's config before it reaches the engine at execute time. `engines:`
*configures without naming*: a Q1-syntax-compatible project-level array whose
single-key-map entries' `claims:` values supply tables for engines already in
the registry, without ever touching T4 gating, engine presence, or sequence.
**q2-divergence:** both keys are read from *merged* metadata (project +
document layers), not only from the one layer Q1 read each from (frontmatter
for `engine:`, project config for `engines:`) — `engine-and-engines-keys.md`
§4 has the full Q1 comparison.

**The needs-no-load predicate (P1–P4).** Let `languages` be the doc's
computational languages per §4.1 (including `generated-languages` when the
scan is non-empty). A doc resolves load-free at Pass-1 iff any of:

- **P1** — the file is claimed (`claimed = Some(engine)`): §8's short-circuit
  consults no claims at all.
- **P2** — the language scan is empty (markdown passthrough).
- **P3** — an explicit `engine: markdown` opt-out.
- **P4** — **every claim consultation the resolution needs returns a static
  answer.** For each `(candidate engine, language)` pair, either a metadata
  claim table answers it, or the engine answers without loading (a built-in,
  or a `TsEngine` with static `claims:`). P4 is not a separate precondition
  check: resolution attempts every consultation over the no-load claim path,
  and a single "would need to load" answer aborts the lift, falling through
  to Pass-2 exactly as today.

Otherwise the doc falls through. "Load-free" is *computed*, not flagged — it
is exactly "the no-load claim path never answered would-load for this doc."

**Tier-dominance shortcut, considered and rejected as unsound.** An earlier
design considered a shortcut: "a static `Primary` beats anything a
non-static engine could declare," letting resolution skip consulting an
unloaded engine once a static `Primary` claim is found elsewhere. This is
**unsound** — an unloaded engine could declare `Primary(999)` (priority
orders *within* a kind, §3.1), so without a load or a claim table a
claims-less engine's claims cannot be bounded, and the shortcut could pick
the wrong owner. The predicate above does not shortcut: a claims-less,
untabled engine registered in the project makes every doc with an uncovered
computational language fall through — correct, and exactly what the
lifted/fell-through counters and the index-pass warning (§12) surface.

**The no-load claim method.** The mechanism the predicate rests on is a
method on the `ExecutionEngine` trait, parallel to `claims_language` but
answerable without loading:

```rust
fn try_claims_language(&self, language: &str, first_class: Option<&str>)
    -> Option<LanguageClaim>;
```

`None` means "would have to load to answer" — the uniform per-engine signal
P4 treats as an abort. `Some(claim)` — including `Some(LanguageClaim::None)`
for "definitely doesn't claim this" — is a static answer. Built-ins override
it directly (their `claims_language` *is* static); a `TsEngine` with a static
`claims:` table (above) answers from that table without spawning the JS
runtime; a claims-less, untabled `TsEngine` returns `None` uniformly. Before
this method existed, the contract described "static claims answer without
loading" only in prose; this is the surface that makes it a checkable fact
rather than a convention.

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

**`generated-languages` widens the scan — but only when it is already
non-empty.** `languages = scan(ast) ∪ generated-languages`: a flat top-level
list of language names in `meta.generated-languages`, declared once,
consumer-only (no per-engine attribution), ordering unaffected (order is
controlled by the explicit `engine:` list, never by this key). Generated
entries are consulted with `first_class = None` — a generated language has no
cell of its own to carry a first class. **The union is consulted only when
`scan(ast)` is non-empty**: a cell-less doc with `generated-languages` set is
a no-op and stays markdown passthrough, matching the doc having nothing to
generate into. See §6.1 for why this is the *static* escape from the
handoff-loss limitation, not a relaxation of it.

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

**Positive projection (`owned_languages`, Plan 4d).** The same ownership map has
a symmetric projection — the languages an engine *does* own:

```
owned_languages(k) = { lang : ownership[lang] == k }
```

carried beside the leave-alone set on the same `ExecutionContext` field pair (and
`TsExecuteOptions.owned_languages` for TS engines). It is **informational, not
enforcement**: `handled_languages` stays the execute-time gate (cede / re-emit),
while `owned_languages` makes the resolution decision legible so an engine can
select the cells it was chosen for directly (`owned_languages` membership) rather
than inferring them as the complement of the leave-alone set — the latter is
ambiguous because "not handled" conflates *owned by me* with *owned by nobody*.
The two projections are disjoint over present languages; a language owned by
**nobody** — present-but-unclaimed, or injected at execute time (§6.1) — is in
neither set, which is exactly how such a cell reads as "not mine, pass through
unexecuted." `owned_languages` does **not** re-resolve or execute injected
languages; §6.1's ratified pass-through is unchanged.

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

**Resolution-driven handoff loss — RATIFIED 2026-07-01 (Gordon; T9).** The
engine sequence is derived **once, from the original parsed AST** — an engine
is in the sequence only if it owns ≥1 language actually present in the source.
This is intended behavior, not a bug. Documented here so it is not re-litigated
(cross-refs: §4.3 fallback gating, §8 file-claim single-engine, §11/bd-r8n4r
nested-handoff splice).

*Scenarios this rules OUT (documented, accepted):*
1. **Injected-cell handoff to an engine absent from the sequence.** Engine A,
   *at execution time*, emits a cell in language L whose only would-be owner is
   engine B — but B was excluded because the *original* source had no L cells.
   The sequence is fixed pre-execution, so B never runs and the injected L cell
   is not executed by B (it passes through as display code — §8/§10
   non-enforcement).
2. **An explicitly-listed engine that owns nothing originally is dropped.**
   `engines: [knitr, customX]` where customX's language never appears in the
   source: customX contributes nothing to the sequence and cannot receive
   runtime-injected cells in its language. The fallback net does **not** save
   this: per §4.3, T4 only adds jupyter for *implicit* sequences, so an
   explicit `[knitr]` with a runtime-injected `{python}` does not auto-add
   jupyter either.

*Scenarios that still WORK (unaffected):* handoff between engines that both own
something in the original AST (knitr re-emits `{python}`, jupyter executes it,
*because the doc already had `{python}` cells*); knitr↔reticulate interop;
jupyter-as-`Fallback(0)` catching the remainder in implicit docs.

*Why acceptable / why not "just fix it":* resolving (1)/(2) would require
**runtime sequence growth** — re-resolving mid-execution as new cells appear —
which the resolution-driven + replay model deliberately avoids (§6.2: replay
drives from recorded captures, not re-resolution; mid-execute re-resolution
would break the determinism guard and the eventual freeze cache-key). Tracked
as a live-preview limitation (bd-r8n4r); the valuable common handoffs are all
in the "still works" set.

**The static escape: `generated-languages` (§4.1).** Scenario 1 above
(injected-cell handoff to an engine absent from the sequence) has a
**declared, static** escape: `meta.generated-languages` widens the
pre-execution language scan to include a language the doc promises to
generate but does not yet contain a literal cell for, so the target engine's
ownership — and its place in the sequence — is decided from metadata before
execution starts, not by re-scanning the AST after it runs. This does **not**
relax the T9 ratification above: the sequence is still derived **once**, from
information known before execution (the scan *plus* the declared list), never
by re-resolving mid-execution as cells appear. An author who does not declare
the generated language remains subject to scenario 1 exactly as ratified.

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

**Resolution is attempted per-document at Pass-1; the stamp is
complete-or-absent, never partial.** `resolve_engines` is a pure function of
`(meta, ast, registry, claimed)` (§9), so `DocumentProfileStage` can call its
Pass-1 counterpart (`resolve_engines_pass1`, §9) directly at the checkpoint:
when the doc satisfies the needs-no-load predicate (§3.3, P1–P4) the call
runs entirely over the no-load claim path and the profile is stamped with a
**complete** `ProfileEngineResolution` (§9); the moment any consultation
would need to load an engine, the attempt aborts and the profile field is
`engine_resolution: None`. There is no partial/pending representation, and
this lift never loads an engine to resolve (only file-claim conversion,
below, may load one, for an unrelated reason). A `None` stamp is not an
error: Pass-2 re-resolves the same doc from scratch via `resolve_engines`,
exactly as it did before this lift existed, so a fall-through doc's render is
unaffected — only Pass-1-only, profile-consuming features (the LSP today;
freeze/pooling later, §12) see the gap.

The lift is per-doc *and* project-grain at once: a doc's P1–P3 status is its
own, but P4 depends on the registry (which engines are static or tabled), so
in practice a project either has every doc resolve load-free or has a shared
minority permanently fall through until its extensions or `engines:` tables
catch up — the condition the index-pass warning (§12) surfaces.

The file-claim half (§8) *is* in Pass 1 (it must, to convert non-QMD input before
parse), but it only spawns an engine when a doc genuinely needs conversion, and
it must be inserted into **both** the full pipeline and the Pass-1 builder
(`pass1_profile_single_file_live`) — otherwise non-QMD docs get a garbage
`DocumentProfile`. File-claim conversion and engine-set resolution are
independent Pass-1 activities: a claimed file's `claimed` argument feeds
`resolve_engines_pass1` exactly as it feeds `resolve_engines` today (§8's
single-engine short-circuit is P1 of the predicate above), but the file
claim's own engine load (to run `markdown_for_file`) is not itself a
resolution load and does not count against the needs-no-load predicate.

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
  passes the rest through, exactly Q1. *(The gating landed as P2-13/P2-13a:
  `engine/jupyter/text_execute.rs` `partition_cells` takes `multi_engine` and
  errors only when it is true; single-engine passes through, with binding
  tests in both directions.)*

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
    pub sequence:  Vec<DetectedEngine>,             // ordered, distinct owners
    pub ownership: LinkedHashMap<String, String>,   // language -> owning engine name, insertion order
    pub notes:     Vec<ResolutionNote>,             // warnings — advisory, resolver stays infallible
}
impl EngineResolution {
    pub fn handled_languages_for(&self, engine: &str) -> Vec<String>;  // §5 (leave-alone)
    pub fn owned_languages_for(&self, engine: &str) -> Vec<String>;    // §5 (positive; Plan 4d)
}
pub fn resolve_engines(
    meta: &ConfigValue, ast: &Pandoc, registry: &EngineRegistry, claimed: Option<&str>,
) -> EngineResolution;   // tiers, presence-gating, fallback ordering all live here

/// Advisory warnings the resolver records instead of failing — the resolver
/// stays pure and infallible (§3's per-doc-layer "warn-and-skip" unknown-name
/// policy), so anything a caller wants to surface travels as returned data,
/// not an error.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionNote {
    UnknownOverrideEngine { engine: String },
    ConflictingDuplicateEngineConfig { engine: String },
}
```

`ownership` is a `LinkedHashMap` (`resolution.rs:286`, insertion-ordered), not
a `HashMap` — `handled_languages_for`'s deterministic output and
`ProfileEngineResolution`'s `Vec<(String, String)>` conversion (§9.1 below)
both rely on iterating it in insertion order. `notes` lives on
`EngineResolution`, **not** on the reduced profile type — warnings are Pass-2
`StageContext` data (drained into `ctx.diagnostics`), and the profile stays a
pure names-only snapshot.

`EngineExecutionStage::run` calls `resolve_engines` once and stashes
`EngineResolution` on `StageContext` (mirroring `project_index` in
`run_pipeline`). The loop reads `ownership` for each engine's
`handled_languages`; the trace records `sequence`; `notes` drains into
`ctx.diagnostics` as warnings. Benefits: the tier/presence/fallback logic is
**unit-testable in isolation** with mock claim tables (no subprocess); the
trace can emit a "resolved engines" entry. It is a function + `StageContext`
artifact, **not** a pipeline stage (it transforms no `PipelineData`).

### 9.1 The Pass-1 lift: `resolve_engines_pass1` and `ProfileEngineResolution`

```rust
// crates/quarto-core/src/engine/resolution.rs
pub fn resolve_engines_pass1(
    meta: &ConfigValue, ast: &Pandoc,
    registry: &EngineRegistry, claimed: Option<&str>,
) -> Option<EngineResolution>; // Some = load-free stamp, None = fall through

// crates/quarto-core/src/document_profile.rs
pub struct ProfileEngineResolution {
    pub sequence:  Vec<String>,            // ordered distinct owners
    pub ownership: Vec<(String, String)>,  // language -> engine, insertion order
}
```

`resolve_engines_pass1` shares `resolve_engines`'s tiers/presence/fallback
core (§4), parameterized to run only over the no-load claim path
(`try_claims_language`, §3.3) — same inputs, same algorithm, different claim
source. It returns `Some(EngineResolution)` when the needs-no-load predicate
(§3.3, P1–P4) holds for the doc — a **complete** resolution, identical in
content to what `resolve_engines` would have produced — and `None` the
moment any consultation would need to load an engine; there is no partial
result (§7). `DocumentProfileStage` calls it at the profile checkpoint and,
on `Some`, projects the full `EngineResolution` down to the reduced
`ProfileEngineResolution` — names only, no `ConfigValue` blobs — for the
`engine_resolution: Option<ProfileEngineResolution>` profile field
(`document-profile-contract.md`): `sequence` becomes engine names,
`ownership` becomes `Vec<(String, String)>` in the `LinkedHashMap`'s
insertion order. A `None` stamp means the field is `None`, not an error —
Pass-2 re-resolves via `resolve_engines` regardless. `EngineResolution::notes`
does not travel to the profile: a Pass-1 stamp is complete precisely because
the no-load path answered every consultation, so it produces no warnings to
carry; `notes` remains a Pass-2 `StageContext`/diagnostics concern.

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
  is what buys deterministic selection.

**Capability is judged from declarations, never from execution results
(ratified 2026-07-02, Gordon).** Every check in this section fires off
*declared* data — `handled_languages`, static claims, `is_available()` — at
resolution/partition time. q2 does **not** verify post-hoc that an engine
actually executed the cells of a language it owns: an engine that runs and
leaves cells unexecuted produces display code blocks, not an error (§8
pass-through). The capture data would support such a post-execution check, but
it is explicitly out of scope — do not add one under the banner of "enforcing"
case-4. **Deliberate divergence from Q1's eager
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
     **A nonsensical claim table follows this same gating.** A metadata claim
     table (§3.3) that names an owner unable to actually execute a language it
     claimed — e.g. `engines: [{jupyter: {claims: {sql: primary}}}]` — is not
     a new failure path: it fails loudly at execute in a multi-engine
     sequence, exactly like any other case-4 owner, and passes through
     silently as display code in a single-engine sequence. Tables are a claim
     *source* (§3.3), not a bypass of this gate.
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

**In scope, delivered by this plan (moved out of "future"):**

- **Pass-1 resolution.** §7/§9 above: once a doc's resolution satisfies the
  needs-no-load predicate (§3.3, P1–P4), `resolve_engines_pass1` runs it in
  Pass 1 and stamps a `ProfileEngineResolution` on the `DocumentProfile`
  (profile-version bump — `document-profile-contract.md`), making the index
  engine-aware (kernel pooling, freeze planning, LSP) for every doc that
  qualifies. **The index-pass warning**: when Pass-1 cannot resolve every
  doc, the orchestrator prints one warning at index-pass completion naming
  each engine that must load to answer language claims (with its
  `_extension.yml` path) and both fixes (author-side `claims:` in
  `_extension.yml`; user-side `engines:` table in `_quarto.yml`) — the
  human-facing surface for the fall-through condition described above.
  **Freeze-key caveat:** a doc whose `engine_resolution` field is `None`
  cannot be frozen until resolution completes at Pass-2 — the profile alone
  does not carry enough to key a freeze cache entry for a fall-through doc.
  This forces **Option A** (load contested engines at Pass-1 to complete the
  set) *for freeze specifically*, whenever freeze is built; the V1
  load-free-only stamp here is sufficient for the LSP, which already
  tolerates `None`, but not sufficient on its own for a freeze cache that
  must cover every doc.
- **Project-level claim overrides.** §3.3 above ("Claim tables from
  metadata") and `engine-and-engines-keys.md` §3: static claims as project
  config, via the `engines:` key's `claims:` entries. "Knitr does not interop
  python here" is exactly `engines: [{knitr: {claims: {r: primary}}}]` — a
  whole-table replacement that omits python, masking it out of knitr's table
  project-wide.

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
- `owned_languages` parity (Plan 4d): the positive projection carried on
  `ExecutionContext`/`TsExecuteOptions.owned_languages` equals
  `{ lang : ownership[lang] == k }` and is disjoint from `handled_languages`
  over present languages (informational, not an enforcement change).
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
