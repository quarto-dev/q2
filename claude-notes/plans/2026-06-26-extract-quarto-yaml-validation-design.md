# Extracting `quarto-yaml-validation`: design decisions

**Strand:** bd-egcyeym9
**Date:** 2026-06-26
**Status:** Design decisions (chosen direction; not yet implemented)
**Predecessor:** `claude-notes/research/2026-06-17-extract-quarto-yaml-validation.md`
  (current-state architecture)

> **⚠️ Partially superseded (2026-06-27) by
> `claude-notes/plans/2026-06-26-extract-error-reporting-foundation.md`.** This
> doc's YAML substance — origin codes (`yaml-schema/*`), the q2 remap, deleting
> `validate-yaml`, the discipline application — stands. But its assumptions about
> the *error-reporting crate structure* are **out of date**: there is **no
> `error-reporting-core` rename** (the externalized crate keeps the name
> `quarto-error-reporting`), **no q2-side façade**, and **`json.rs` does not move
> q2-side** (it stays in the external crate behind a default-off `json` feature;
> only the `Q-*` catalog *data* carves out, into `quarto-error-catalog`). Where Q1,
> Q4, and Q5 below describe `error-reporting-core`/façade/json-relocation, defer to
> the foundation plan. Step ordering is also foundation-first (source-map →
> error-reporting → *then* this YAML work).
>
> **➡️ For execution, use the handoff note
> `claude-notes/plans/2026-06-29-yaml-stack-extraction-handoff.md`** — it is the
> up-to-date, self-contained plan: the YAML stack ships as **one repo
> `posit-dev/quarto-yaml`, a workspace with two crates** (decided 2026-06-29; the
> foundation crates\' "one repo per crate" rule does *not* apply here), and the
> foundation extraction it depends on is **done** (source-map + error-reporting
> published, PRs #348/#349/#350). This design doc remains the rationale for the
> error-code discipline applied to YAML.

This document moves the extraction from *current-state architecture* to
*chosen design*. It resolves the seven open questions the research doc left
hanging, given two decisions the user has already made:

- **Session goal:** decide the open questions.
- **Extraction strategy lean:** *new repo owns the foundation crates; q2
  consumes them as published crates* (research-doc Option 1).

The hardest question — error-code identity across the package boundary — is
elevated to its own general design note, because it governs *any* error defined
in one package and surfaced by another product, not just YAML:
**`claude-notes/designs/cross-package-error-codes.md`**. Q3/Q6 below are the
YAML-specific application of that philosophy.

### Two clarifications from the user (2026-06-26)

- **`validate-yaml` is to be deleted, not carried along.** It is a demo binary,
  not a supported one. It is the **only** in-repo Cargo dependent of
  `quarto-yaml-validation` (the other two grep hits — in
  `quarto-error-reporting/Cargo.toml` and `quarto-core/.../attribution/mode.rs`
  — are comments, not deps). **Consequence: once `validate-yaml` is removed,
  `quarto-yaml-validation` has ZERO in-repo consumers.** The crate lifts out with
  nothing trailing it, and the error-code remap (Q3) is needed only by *external*
  embedders today — the YAML validator becomes the **proving ground** for the
  general cross-package philosophy rather than a forced q2 integration.
- **"TypeScript" = the language/compiler, not TS Quarto.** The `Q-*` scheme was
  inspired by the TypeScript *compiler*\'s flat numeric `TSxxxx` catalog. That is a
  good template for the *presentation* layer but offers nothing for the
  cross-package case (it is a monolith with a central allocator). The composable
  precedents are Clippy (`clippy::needless_return`) and ESLint plugin namespacing —
  see the design note.

Every section below is **Decision → Rationale**, and flags where a decision is a
judgment call the user may want to override.

---

## 0. The reframing that drives everything

The research doc treated `quarto-error-reporting` as one of three
interchangeable "foundation crates" to externalize alongside `quarto-source-map`
and `quarto-yaml`. Reading the code, it is not interchangeable:

- It is **q2's entire diagnostics substrate**, not just a YAML helper:
  `DiagnosticMessage`, `DiagnosticMessageBuilder`, ariadne/text rendering,
  `coalesce`, `macros`, and the **JSON wire shape** (`json.rs`).
- The JSON wire shape is a **shared cross-crate contract**: consumers include
  `wasm-quarto-hub-client`, `quarto-hub`, `quarto-publish`, `quarto-trace`,
  `quarto-parse-errors`, `quarto-mcp-launcher`, plus the preview SPA and CLI.
- It bakes in **Quarto policy** in two places, not one: the catalog's
  `docs_url` (`https://quarto.org/docs/errors/...`) *and*
  `JsonDiagnostic::SCHEMA_URL` (`https://quarto.org/schemas/v1/...`).
- The catalog is reached as a **global static + free functions**
  (`ERROR_CATALOG`, `get_docs_url`, `get_error_info`, `get_subsystem`) called
  from \~19 crates — not as an injected dependency.

So "externalize `quarto-error-reporting`" is really two separable things welded
together: a **catalog-agnostic reporting core** (reusable, belongs outside) and
a **Quarto error-code policy** (the `Q-*` catalog + quarto.org URLs + the audit,
belongs in q2). The whole design hinges on splitting them.

---

## Q1. Which extraction strategy? — DECIDED: Option 1 (own), but split first

**Decision.** New repo (`quarto-yaml-schema`, name TBD — see Q7) owns four
crates and q2 consumes them as published crates. But `quarto-error-reporting`
is **split before the move**, and only the *core* half leaves:

External repo owns:

| Crate (external) | Was | Role |
|---|---|---|
| `quarto-source-map` | unchanged | clean leaf, location tracking |
| `quarto-yaml` | unchanged | annotated YAML parse |
| `error-reporting-core` | **split** out of `quarto-error-reporting` | catalog-agnostic `DiagnosticMessage` + builder + render + pluggable catalog |
| `quarto-yaml-validation` | unchanged crate, error-code change | schema validation over the above |

q2 keeps / gains:

| Crate (in q2) | Was | Role |
|---|---|---|
| `quarto-error-catalog` | **split** out of `quarto-error-reporting` | the `Q-*` `error_catalog.json`, quarto.org URLs, the audit, the `CatalogProvider` impl |
| `quarto-error-reporting` (façade) | shrinks to a re-export shim | re-exports `error-reporting-core` + installs the q2 catalog, so the \~19 existing `use quarto_error_reporting::…` call sites keep compiling |

**Rationale.** Option 1 is the only strategy that delivers the actual goal — a
crate non-Quarto developers can `cargo add` without a Quarto identity. Options 2
(mirror) and 3 (publish-from-q2 with pins) were rejected because:

- They leave the `Q-*`/quarto.org policy baked into the shipped artifact, so the
  "non-Quarto" identity is cosmetic. The research doc itself notes Option 2's
  external identity "is thin if it is just a mirror."
- The error-code pluggability work (Q2/Q3) is *unavoidable* the moment a real
  external user appears; deferring it (Option 2's only advantage) just moves the
  cost without shrinking it.

**The cost we are accepting (judgment call):** every q2 diagnostic — not just
YAML — now renders through an externally-owned `error-reporting-core`. Cross-repo
coordination on the diagnostic builder/render is the standing tax. We mitigate
it by keeping a thin `quarto-error-reporting` façade in q2 so day-to-day q2 code
does not change its imports, and by making `error-reporting-core`\'s surface
deliberately small and slow-moving.

> **Override point.** If that tax is judged too high right now, the fallback is
> "split, but don't move yet" — do all of §Q2–Q3 (the core/catalog split lands
> *inside q2*), publish nothing, and defer the repo move to a later strand once
> the seam has proven stable. This keeps every decision below valid and de-risks
> the cross-repo step.

---

## Q2. Does `quarto-error-reporting` split into core + catalog, or gain a provider trait? — DECIDED: both (split *and* a provider trait)

**Decision.** Split into `error-reporting-core` + `quarto-error-catalog`, and
the seam between them is a `CatalogProvider` trait installed into a process-global
registry.

```rust
// error-reporting-core (external)
pub struct ErrorCodeInfo {            // moved verbatim from catalog.rs
    pub subsystem: String,
    pub title: String,
    pub message_template: String,
    pub docs_url: Option<String>,
    pub since_version: String,
}

pub trait CatalogProvider: Send + Sync {
    fn lookup(&self, code: &str) -> Option<&ErrorCodeInfo>;
}

// A no-op default so the core is usable standalone with zero config.
struct EmptyCatalog;
impl CatalogProvider for EmptyCatalog { fn lookup(&self, _: &str) -> Option<&ErrorCodeInfo> { None } }

static CATALOG: OnceLock<Box<dyn CatalogProvider>> = OnceLock::new();

/// Embedders call this once at startup. Idempotent-by-first-write.
pub fn install_catalog(p: Box<dyn CatalogProvider>) { let _ = CATALOG.set(p); }

fn catalog() -> &'static dyn CatalogProvider {
    CATALOG.get().map(|b| b.as_ref()).unwrap_or(&EmptyCatalog)
}

// The existing free functions keep their signatures — now delegating:
pub fn get_docs_url(code: &str) -> Option<&'static str> { /* via catalog() */ }
pub fn get_error_info(code: &str) -> Option<&'static ErrorCodeInfo> { /* via catalog() */ }
```

```rust
// quarto-error-catalog (stays in q2)
struct QuartoCatalog(HashMap<String, ErrorCodeInfo>); // from error_catalog.json
impl CatalogProvider for QuartoCatalog { /* HashMap::get */ }

pub fn install() { error_reporting_core::install_catalog(Box::new(QuartoCatalog::load())); }
```

**Rationale — why a global registry rather than threading the provider through
every call site.** Today the catalog is a `Lazy<HashMap>` global reached by free
functions from \~19 crates. Converting all of those to take a `&dyn
CatalogProvider` parameter is a large, invasive churn with no behavioural payoff.
A `OnceLock`-installed global keeps every existing call site (`get_docs_url(code)`)
source-compatible; only the *initialization* changes (q2 calls
`quarto_error_catalog::install()` once at binary/WASM entry). The standalone
library, installing nothing, transparently gets the `EmptyCatalog` (codes render
without titles/URLs — exactly right for a non-Quarto user).

**Trade-off (acknowledged).** This is process-global state with an init-ordering
requirement: q2 must `install()` before the first diagnostic renders. We accept
it because it mirrors the *current* global-static behaviour (no regression) and
because the install point is trivially early (binary `main` / WASM bootstrap). A
test-only `install()` helper covers the test binaries. The `OnceLock::set`
swallow-on-second-write keeps double-install (e.g. test + lib) harmless.

> This is the *"split into core + catalog"* and *"provider trait"* candidates
> from the research doc, combined. They were never mutually exclusive: the split
> is the crate boundary; the trait is the seam across it.

---

## Q3. Where does the `Q-1-x` mapping live, and what does `quarto-yaml-validation` emit? — DECIDED: library-local stable ids, remapped at the q2 boundary

> This is the YAML-specific application of the general philosophy in
> `claude-notes/designs/cross-package-error-codes.md`. "Library-local id" =
> *origin code* (namespaced, package-owned); "Q-1-x" = *presentation code*
> (flat, product-owned); the remap is the product-owned bridge. Read that note
> for the invariants (esp. I1 subsystem≠package, I5 provenance).

**Decision.** `quarto-yaml-validation` stops returning `Q-1-x`. Its
`ValidationErrorKind` is already a clean, string-free enum; we give it a
**library-local stable code namespace** (an *origin-code* namespace) owned by the
library, e.g.:

```rust
impl ValidationErrorKind {
    /// Stable, library-owned identifiers. NOT Quarto codes.
    pub fn code(&self) -> &'static str {
        match self {
            ValidationErrorKind::MissingRequiredProperty { .. } => "yaml-schema/missing-required",
            ValidationErrorKind::TypeMismatch { .. }            => "yaml-schema/type-mismatch",
            ValidationErrorKind::InvalidEnumValue { .. }        => "yaml-schema/invalid-enum",
            // … one per variant …
        }
    }
}
```

q2 owns the **remap** from those origin codes to its `Q-1-x` presentation codes,
at the reporting boundary, in `quarto-error-catalog` (since `validate-yaml` is
being deleted, there is no binary-local place for it to live — it belongs with the
product's catalog policy):

```rust
// q2 side
fn quarto_code(lib_code: &str) -> Option<&'static str> {
    match lib_code {
        "yaml-schema/missing-required" => Some("Q-1-10"),
        "yaml-schema/type-mismatch"    => Some("Q-1-11"),
        // …
        _ => None,
    }
}
```

So the *same* `TypeMismatch` renders as `Q-1-11` + a quarto.org URL inside q2,
and as `yaml-schema/type-mismatch` (or code-less, per the user's catalog)
outside q2.

**Rationale.** This is the research doc's "remap error codes when errors are
defined in different packages" idea, and it is the only option that satisfies
*both* contracts simultaneously:

- The library gets a stability contract it **owns** (its ids never depend on
  Quarto's numbering).
- q2 keeps `Q-1-x` as its public stability contract (the catalog/audit/docs URLs
  are unchanged downstream of the remap).

It also **generalizes** beyond YAML: any future externalized crate that defines
its own errors uses the same pattern (own ids → q2 remap table). That is a
reusable architectural seam, not a one-off.

**Migration safety net.** The existing `error_code()` (returning `Q-1-x`) and its
\~15 unit tests in `error.rs` are the regression oracle, but the check **splits**
across the boundary once the crate leaves:

- *Upstream (library):* a unit test pins `kind.code()` → origin-code string for
  every `ValidationErrorKind` variant (exhaustive match guarantees coverage).
- *q2 side:* a test pins the remap `origin-code → Q-1-x` against a **captured
  snapshot** of today's `error_code()` output (a frozen `[(origin, Q-code)]`
  table), proving the remap reproduces today's presentation codes exactly. The
  snapshot is taken *before* any code moves (TDD: capture first).

The split is necessary because, post-extraction, q2 no longer depends on the enum
(see honesty note), so the q2 oracle keys on origin-code **strings**, not on
`ValidationErrorKind`.

> **Honesty note — the yaml remap is dormant at first.** With `validate-yaml`
> deleted and the validator not wired into q2's config pipeline, **q2 surfaces no
> yaml-validation errors today**, so the `Q-1-x` catalog entries and the remap are
> *forward-looking*: they exist for when q2 actually consumes the external
> validator (presumably front-matter validation — the validator's reason to
> exist). Two honest options: (a) keep the `Q-1-*` catalog entries + remap as a
> dormant, audited contract ready for that integration; or (b) **remove** the
> `Q-1-*` yaml entries from q2's catalog now and re-add them with the integration.
> Recommendation: (a) — the entries are cheap, the docs URLs are already public,
> and keeping them avoids a churn later; the audit's remap-completeness check
> (Q6) is simply scoped to "surfaced" codes, which is currently empty for yaml.
> **The append-only principle largely settles this toward (a):** if any `Q-1-*`
> yaml docs page has been *published*, it is under the cool-URL covenant and must
> not be deleted (it is a "dormant" catalog entry, a first-class state — see the
> design note's lifecycle). Removal is only on the table for entries never
> publicly documented. The fork survives only for those un-published entries.

---

## Q4. Where do the JSON wire types (`json.rs`) live after the split? — DECIDED: stay in q2

**Decision.** `JsonDiagnostic`, `JsonDiagnosticDetail`, `JsonPass1Failure`,
`diagnostic_to_json`, `with_source_file` stay **q2-side** (in the
`quarto-error-reporting` façade or a small `quarto-diagnostic-json` crate). They
are **not** part of the externalized library.

**Rationale.**

- Every consumer of the wire shape is a q2 concern (the preview SPA, the WASM
  bridge, the hub, publish, trace, the CLI). A non-Quarto user of
  `quarto-yaml-validation` has no use for it — they have `ValidationDiagnostic`
  and the text/ariadne renderer.
- The shape **hard-codes quarto.org schema URLs** (`SCHEMA_URL` consts) and is
  versioned under Quarto's `/v1/` scheme. That is Quarto policy, same category as
  the catalog — it belongs with the policy, not in the neutral core.
- Keeping it q2-side means the externalized core need not take a `schemars`
  dependency for *this* shape (see Q5), and the cross-repo surface stays smaller.

`json.rs` depends only on `DiagnosticMessage` + `SourceContext`, both of which
remain available (the former re-exported through the façade from
`error-reporting-core`, the latter an external dep q2 already consumes). So the
move is mechanical.

---

## Q5. Does the external core keep `schemars`? — DECIDED: no

**Decision.** `error-reporting-core` does **not** depend on `schemars`. The only
`schemars`-deriving types are the JSON wire shapes, which stay q2-side (Q4), so
`schemars` stays a q2 dependency.

**Rationale.** `schemars` exists in `quarto-error-reporting` solely to emit JSON
Schema for the machine-to-machine wire types. With those types staying in q2, the
external core's surface (`DiagnosticMessage`, builder, render, `ErrorCodeInfo`,
`CatalogProvider`) needs only `serde`/`serde_json` for `ErrorCodeInfo`
(de)serialization. Smaller external dependency footprint = easier for a
non-Quarto user to adopt, and one fewer version to coordinate across repos.

---

## Q6. How does the error-code audit (`scripts/audit-error-codes.py`) adapt? — DECIDED: it stays q2-only and polices the q2 catalog + remap table

**Decision.** The audit remains a q2 script. Its scope changes from bidirectional
"codes referenced in source ↔ `error_catalog.json`" to three checks — and the
**bidirectionality is deliberately broken** to honour the append-only principle
(see `claude-notes/designs/cross-package-error-codes.md`, "Codes are append-only"):

1. **q2 catalog consistency — forward only.** Every `Q-*` *emitted* in q2 source
   has an `error_catalog.json` entry. The **reverse is dropped**: a catalog entry
   with no emitter is a legitimate *retired* or *dormant* code, not an error. (This
   is the one concrete code-change the append-only principle forces.)
2. **append-only (new).** No `error_catalog.json` entry is ever *removed* or
   *redefined* (diff against git history or a committed snapshot). Enforceable
   because it is q2's own catalog in q2's own repo; the cross-repo analogue is only
   documented expectation.
3. **remap completeness (new).** Every library-local id an externalized crate's
   `code()` returns *that q2 chooses to map* has a `Q-*` target that exists in the
   catalog. Unmapped is allowed (tier-2 passthrough), so this checks the *mapped*
   subset only. (The `quarto_code()`/`old error_code()` equivalence test from Q3 is
   the machine-checkable half; the audit covers the catalog-entry half.)

The externalized library carries its **own**, much simpler check (its `code()`
arms are exhaustive over the enum — guaranteed by the compiler's match
exhaustiveness; a unit test pins the id strings so they are not changed
accidentally).

**Rationale.** The `Q-*` namespace, the `quarto.org` URLs, and the cross-subsystem
numbering are Quarto policy; the audit enforces that policy and has no meaning for
an external user. Keeping it q2-only is the natural consequence of the core/catalog
split. The new remap-completeness check is small and replaces the coupling the old
audit had to the (now externalized) yaml-validation source.

---

## Q7. Naming / identity of the external project — DECIDED (proposal): `quarto-yaml-schema`, kept under the Quarto brand

**Decision (proposal, lowest-confidence — explicitly flagged for the user).**
Ship the external repo as **`quarto-yaml-schema`** (or a `quarto-` family of the
four crate names, unchanged), and keep the Quarto brand rather than rebranding to
a generic name.

**Rationale.** The validator implements *Quarto's* YAML schema dialect (the code
comments call it exactly that), and the crate names are already `quarto-*` with
`[workspace.package] repository = posit-dev/quarto-markdown-syntax`. A rename to a
neutral identity (a) loses the discoverability of the Quarto association, (b)
forces a crate-rename churn, and (c) is not required for a non-Quarto user to
adopt it — the catalog-pluggability (Q2/Q3) is what makes it non-Quarto-*specific*,
not the name. "Quarto-flavoured but catalog-agnostic" is an honest description.

> **This is the decision most likely to be wrong without the user's product
> intent.** If the goal is to court non-Quarto adopters aggressively, a neutral
> name may matter more than I am weighting it. Cheap to revisit before the first
> `cargo publish`; expensive after. Left as a proposal.

---

## Target topology (summary diagram)

```
EXTERNAL REPO (Option 1 — owns these, published to crates.io)
  quarto-source-map         leaf
        ▲
  quarto-yaml               → source-map
        ▲
  error-reporting-core      → source-map   (CatalogProvider trait; EmptyCatalog default;
        ▲                                    NO schemars; NO quarto.org URLs)
  quarto-yaml-validation    → the above three
                                 ValidationErrorKind::code() -> "yaml-schema/*"

Q2 (consumes the four as published deps; adds policy)
  quarto-error-catalog      → error-reporting-core   (error_catalog.json, Q-*,
                                                       quarto.org URLs, install())
  quarto-diagnostic-json    → error-reporting-core + source-map   (JsonDiagnostic, schemars,
        (or kept in façade)                                        quarto.org SCHEMA_URLs)
  quarto-error-reporting    → façade: re-exports core + installs catalog
        (shim)                  (keeps ~19 `use quarto_error_reporting::…` sites compiling)
  (remap origin -> Q-1-x)    lives in quarto-error-catalog; DORMANT until q2 wires
                             the external validator into its config pipeline
  validate-yaml             → DELETED (demo binary; was the only consumer)
```

---

## Sequencing decision (2026-06-26 — supersedes the in-q2-first ordering)

The user chose **extract-first, migrate-q2-last**, and **error-reporting before
yaml-validation**, because (a) there are invisible internal Posit consumers of
`quarto-yaml-validation` that need a real standalone repo, and (b) the error-code
*discipline* (`claude-notes/designs/cross-package-error-codes.md`) is a **host**
contract that must be designed and proven before its first client. Concretely:

1. **Foundation repo first** (`posit-dev/…`): extract `quarto-source-map` (leaf) +
   `error-reporting-core` (the split-out catalog-agnostic half, carrying the
   `CatalogProvider` + remap hook). Publish to crates.io; validate standalone.
   q2 keeps its in-tree copies until the external crates are proven, then switches
   its deps; a thin `quarto-error-reporting` façade + `quarto-error-catalog`
   remain q2-side (Q1, Q4).
2. **YAML repo second**: extract `quarto-yaml` + `quarto-yaml-validation` as the
   **first client** of the discipline (origin namespace `yaml-schema/*`); delete
   `validate-yaml`. Publish; the Posit consumers repoint here.
3. **q2 migration last**: q2 consumes the published crates; adds its (dormant)
   yaml remap.

This **inverts** the "P0–P3 in q2, P4–P5 cross-repo" ordering below — the repo
moves now come *first*, not last. The phase *contents* below remain valid as a
work breakdown; only their order relative to the repo split changes. The
error-reporting extraction (step 1) warrants **its own plan doc** once the
discipline is accepted; this doc remains the yaml-validation-specific plan
(gated behind step 1).

> **Open repo-granularity fork:** one foundation repo + one YAML repo (recommended
> — clean separation of "general diagnostics infra" from "the YAML stack"), vs. a
> single workspace repo holding all four crates (simpler publish/CI, but couples
> the YAML stack's release cadence to the infra's). Needs the user.

## Phased implementation outline (TDD — to become braid sub-strands)

Each phase is independently shippable and leaves the workspace green.

- [ ] **P0 — Capture the equivalence oracle (no code moves).** Snapshot today's
      `ValidationErrorKind → Q-1-x` mapping as a frozen `[(origin, Q-code)]` table,
      and add the (initially failing) remap test that will guard P2. This is the
      regression contract for everything after.
- [ ] **P0b — Delete `validate-yaml`.** Remove the demo binary and its workspace
      member entry. Confirms `quarto-yaml-validation` then has zero in-repo
      consumers (`cargo build --workspace` green). Independent of the rest.
- [ ] **P1 — Split `quarto-error-reporting` *in place* (still one repo).** Carve
      `error-reporting-core` (catalog-agnostic) + `quarto-error-catalog` (Q-*
      policy) + the `CatalogProvider` registry; turn `quarto-error-reporting` into
      the re-export façade that calls `install()`. Move `json.rs` to its q2 home
      (Q4). Workspace stays green; \~19 dependents unchanged. **This is the bulk of
      the work and is valuable even if the repo move never happens.**
- [ ] **P2 — Re-point `quarto-yaml-validation` to library-local ids.** Replace
      `error_code()`’s `Q-1-x` with `code() -> "yaml-schema/*"`; move the
      `Q-1-x` knowledge into the q2 remap table; make P0's oracle pass.
- [ ] **P3 — Adapt the audit.** Implement the two-check audit (Q6).
- [ ] **P4 — Decouple workspace metadata.** Per-crate `repository`/`version` for
      the four externalizing crates so they can publish independently (research
      doc's note on shared `[workspace.package]`).
- [ ] **P5 — Stand up the external repo + publish.** Move the four crates; q2
      switches its `path` deps to version (or git) deps; CI on both sides.
      *(Gated on the Q1 override decision — may be deferred.)*

P0–P3 land entirely inside q2 and deliver the reusable seam; P4–P5 are the
cross-repo commitment. This ordering means we can stop after P3 with a clean,
catalog-pluggable diagnostics stack and decide the repo move on its own merits.

---

## Open items genuinely needing the user (not decided here)

1. **Q1 override:** full repo move now (P5) vs. stop after the in-q2 split (P3)
   and defer the move? (Recommendation: do P0–P3 regardless; treat P4–P5 as a
   separate go/no-go.)
2. **Q7 naming:** keep `quarto-*` / `quarto-yaml-schema`, or rebrand neutral?
3. **Publish channel:** crates.io vs. git deps for q2→external consumption (P5).
4. Whether `quarto-error-reporting` keeps its name as the façade, or the façade is
   removed and the \~19 dependents migrate to `error-reporting-core` directly
   (more churn, cleaner end state).
