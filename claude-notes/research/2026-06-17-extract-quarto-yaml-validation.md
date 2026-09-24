# Extracting `quarto-yaml-validation` into a standalone repository

**Strand:** bd-egcyeym9
**Date:** 2026-06-17
**Status:** Information gathering / current-state architecture (no code changes this session)

## Goal

Move the YAML-schema-validation infrastructure (`quarto-yaml-validation`) out of
`quarto-dev/q2` so that non-Quarto developers can depend on it as a general-purpose
crate. This session maps where the crate lives in the dependency graph, its inbound
and outbound dependencies, and the design problems an extraction must solve — chiefly
the Quarto-specific error-code catalog.

## TL;DR findings

1. **The outbound closure is small and clean.** To use `quarto-yaml-validation` you
   need exactly three other workspace crates: `quarto-yaml`, `quarto-source-map`,
   and `quarto-error-reporting`. `quarto-source-map` is a true leaf (only `serde`,
   `serde_json`, `smallvec`). None of the four pulls in the heavy Quarto engine
   crates (`pampa`, `quarto-core`, etc.).

2. **Inside q2, `quarto-yaml-validation` has essentially no inbound coupling.** The
   only in-repo consumer is the `validate-yaml` binary (a thin CLI/test harness). It
   is **not** wired into the render pipeline, `quarto-config`, or the WASM client.
   This makes extraction low-risk: nothing in the engine breaks if the crate moves.

3. **The three *foundation* crates are heavily used inside q2.** `quarto-source-map`
   (~26 dependents), `quarto-error-reporting` (~19), and `quarto-yaml` (8) are core
   infrastructure. They **cannot simply be relocated** — q2 still needs them. The
   split therefore has to be a *publish-and-consume* arrangement (q2 depends on the
   externalized crates), not a move.

4. **The hard design problem is error-code identity.** `quarto-yaml-validation`
   hard-codes Quarto error codes (`Q-1-10`, `Q-1-11`, …) and leans on
   `quarto-error-reporting`\'s centralized `error_catalog.json` (titles, docs URLs at
   `https://quarto.org/docs/errors/...`). Outside q2 those codes and URLs are
   meaningless. The standalone library needs a *pluggable error-code / catalog
   provider*; the q2-embedded build keeps the existing `Q-1-x` codes. This is both a
   technical change (abstraction boundary) and an org change (two catalogs, two
   "since_version" lineages, a remapping layer).

## Crate dependency graph (relevant slice)

```
quarto-source-map        leaf:  serde, serde_json, smallvec        (NO quarto deps)
        ▲
        │
quarto-yaml              → quarto-source-map, yaml-rust2, serde, thiserror
        ▲
        │
quarto-error-reporting   → quarto-source-map, ariadne, schemars, url, once_cell,
        ▲                   serde, serde_json, thiserror
        │
quarto-yaml-validation   → quarto-yaml, quarto-source-map, quarto-error-reporting,
        ▲                   yaml-rust2, regex, serde, serde_json, thiserror
        │
validate-yaml (bin)      → the only in-repo consumer of quarto-yaml-validation
```

Note: `quarto-error-reporting` does **not** depend on `quarto-yaml-validation`. (A
grep match is a false positive — it appears only in a code comment that warns against
adding `schemars` for user-YAML validation. There is no dependency cycle.)

### Outbound dependencies (what extraction must carry along)

`quarto-yaml-validation` → (workspace) `quarto-yaml`, `quarto-source-map`,
`quarto-error-reporting`; (crates.io) `yaml-rust2`, `regex`, `serde`, `serde_json`,
`thiserror`, `anyhow`.

The **transitive workspace closure** that must be externalized to make
`quarto-yaml-validation` buildable outside q2 is:

| Crate | Outbound (workspace) deps | crates.io deps | Notes |
|---|---|---|---|
| `quarto-source-map` | *(none)* | serde, serde_json, smallvec | Clean leaf — easiest to externalize |
| `quarto-yaml` | source-map | yaml-rust2, serde, thiserror | Annotated YAML parse; re-exports `SourceInfo` |
| `quarto-error-reporting` | source-map | ariadne, schemars, url, once_cell, serde(_json), thiserror | Holds the **error catalog** — the contentious piece |
| `quarto-yaml-validation` | the above three | yaml-rust2, regex, serde(_json), thiserror, anyhow | The crate we want to ship |

All four share the workspace `[workspace.package]` metadata
(`repository = posit-dev/quarto-markdown-syntax`, `license = MIT`,
`edition = 2024`, version `0.3.0`).

### Inbound dependencies (who would be affected by a move)

| Crate | Direct in-repo dependents (Cargo) | Implication for split |
|---|---|---|
| `quarto-yaml-validation` | `validate-yaml` only | **Trivial** — move freely; only the CLI follows |
| `quarto-yaml` | pampa, quarto-core, quarto-config, quarto-error-reporting, quarto-lsp-core, validate-yaml (8) | q2 still needs it → must depend on externalized crate |
| `quarto-error-reporting` | ~19 crates (pampa, quarto-core, quarto-config, quarto-csl, quarto-citeproc, quarto-doctemplate, quarto-lsp-core, quarto-preview, quarto-publish, quarto, wasm-quarto-hub-client, …) | Deeply embedded → cannot move; must be a shared dependency |
| `quarto-source-map` | ~26 crates (most of the workspace, incl. WASM client) | Core infra → cannot move; must be a shared dependency |

**Consequence:** the only crate that *leaves* q2 cleanly is `quarto-yaml-validation`
itself. The three foundation crates have to become shared/published dependencies that
*both* the new external repo and q2 consume. This is the central structural decision
(see "Extraction strategies" below).

## How `quarto-yaml-validation` couples to source-location infra

`quarto-source-map` provides the annotated-parse machinery the user mentioned. The
validation crate uses it pervasively:

- `quarto-yaml` parses YAML into `YamlWithSourceInfo`, carrying `SourceInfo` (byte
  ranges + provenance) for every node. `SourceInfo` is re-exported from
  `quarto-source-map`.
- `ValidationError` (in `error.rs`) stores `yaml_node: Option<YamlWithSourceInfo>`
  and a resolved `SourceLocation { file, line, column }` obtained via
  `SourceInfo::map_offset(0, ctx)` against a `SourceContext`.
- `ValidationDiagnostic` (in `diagnostic.rs`) calls `source_info.map_offset(...)` and
  `source_ctx.get_file(...)` to produce a `SourceRange` (filename + offset +
  line/col) for machine-readable JSON, and hands the `SourceInfo` to
  `quarto-error-reporting`\'s ariadne renderer for human text.

The public `quarto-source-map` surface in play:
`SourceContext`, `SourceFile`, `SourceInfo`, `FileId`, `Location`, `MappedLocation`,
`map_offset`, `get_file`, `start_offset`/`end_offset`. This surface is general (it is
not Quarto-specific in any way) — externalizing `quarto-source-map` is conceptually
clean; the only cost is that ~26 q2 crates must now consume it as an external crate.

## How `quarto-yaml-validation` couples to error reporting / the catalog

Two distinct couplings, both via `quarto-error-reporting`:

### (a) Rendering coupling — `DiagnosticMessage` / `DiagnosticMessageBuilder`

`ValidationDiagnostic::build_diagnostic_message` builds a
`DiagnosticMessageBuilder::error("YAML Validation Failed")` and calls `.with_code(...)`,
`.problem(...)`, `.with_location(SourceInfo)`, `.add_detail/.add_info/.add_hint`,
`.build()`. The resulting `DiagnosticMessage` is what renders to ariadne text
(`to_text`) and JSON. This coupling is purely about *presentation* and is
non-Quarto-specific in shape — it could move to the external library as-is.

### (b) Identity coupling — the centralized error-code catalog (the hard part)

- `ValidationErrorKind::error_code()` (in `error.rs`) hard-codes
  `&'static str` Quarto codes:
  `Q-1-10` (missing required property), `Q-1-11` (type mismatch),
  `Q-1-12` (invalid enum), `Q-1-13`, `Q-1-14`, `Q-1-15`, `Q-1-16`, `Q-1-17`,
  `Q-1-18`, `Q-1-19`, `Q-1-20`, `Q-1-29`, `Q-1-99` (other).
- These strings are looked up at render time in `quarto-error-reporting`'s
  **catalog**: `catalog.rs` loads `error_catalog.json` via `include_str!` into a
  `HashMap<String, ErrorCodeInfo>` where `ErrorCodeInfo { subsystem, title,
  message_template, docs_url, since_version }`. `DiagnosticMessage::docs_url()` does
  `catalog::get_docs_url(code)`.
- The catalog is **global and centralized**: 145 entries spanning many subsystems
  (yaml=`Q-1-*`, internal=`Q-0-*`, listing=`Q-12-*`, navigation=`Q-13-*`, …). Codes
  follow `Q-<subsystem>-<number>`. The yaml subsystem occupies `Q-1-1..Q-1-29` +
  `Q-1-99`. `docs_url`s all point at `https://quarto.org/docs/errors/<subsystem>/<code>`.
- A repo-wide **audit** (`scripts/audit-error-codes.py`, `scripts/quick-error-audit.sh`)
  enforces consistency between codes referenced in source and entries in
  `error_catalog.json`. Deliberate exceptions use the marker
  `// quarto-error-code-audit-ignore` (line) or `...-ignore-file` (file). Any
  extraction must keep this audit green for the q2-embedded build.

**Why this blocks a naive extraction:** the codes `Q-1-x`, the central JSON catalog,
the `quarto.org` docs URLs, and the cross-subsystem numbering are all *Quarto policy*.
A standalone library shipped to non-Quarto users must not bake in `Q-1-x`/`quarto.org`,
yet the q2-embedded build must keep them (codes are a stability contract — see
`error.rs`/`builder.rs` doc comments: "codes don't change even if message wording
improves").

## The error-code remapping design problem (statement, not solution)

We need an abstraction where:

- `quarto-yaml-validation` emits *semantic* error kinds (it already has a clean
  `ValidationErrorKind` enum — machine-readable, no strings). The mapping from kind →
  catalog code becomes a **policy supplied by the embedder**, not hard-coded in the
  library.
- `quarto-error-reporting` becomes a reusable error-reporting library whose catalog is
  **pluggable / remappable**: a host can register its own code namespace, titles, and
  docs-URL scheme. The library ships with no Quarto-specific catalog; q2 supplies the
  `Q-*` catalog + `quarto.org` URLs; an external user supplies their own (or none).
- The same `ValidationErrorKind` therefore renders as `Q-1-11` + a `quarto.org` URL
  inside q2, and as `<their-scheme>` (or code-less) outside q2.

Candidate shapes to evaluate in a follow-up design session (NOT decided here):

- A trait, e.g. `ErrorCodeProvider`/`Catalog`, injected into the diagnostic builder;
  q2 implements it over `error_catalog.json`, the standalone lib provides a no-op or
  caller-supplied default.
- Keep `error_code()` returning a *library-local* stable id (namespaced to the
  validation crate, e.g. `YV-…` or a typed enum discriminant), and let the embedder
  *remap* library ids → its own catalog codes at the reporting boundary. This is the
  "remap error codes when errors are defined in different packages" idea the user
  raised, and it generalizes beyond yaml-validation (any externalized crate that
  defines its own errors).
- Split `quarto-error-reporting` into `error-reporting-core` (catalog-agnostic
  builder/render/JSON) + `quarto-error-catalog` (the q2 `Q-*` policy + audit). q2
  depends on both; external users depend only on the core.

## Extraction strategies (to weigh in design phase)

Because the three foundation crates must serve *both* repos, the split is really a
question of *who owns the source of truth*:

1. **New repo owns the foundation crates; q2 consumes them as published crates.**
   Move `quarto-source-map`, `quarto-yaml`, `quarto-error-reporting`(-core),
   `quarto-yaml-validation` to a new repo; publish to crates.io (or git deps); q2
   replaces its in-tree copies with version deps. Cleanest long-term, highest
   coordination cost (every q2 change to source-map now crosses a repo boundary).

2. **q2 stays the source of truth; new repo vendors via subtree/`git subtree` or a
   read-only mirror + crates.io publish from q2.** Lowest disruption to q2 dev flow;
   external repo is a packaging view. Risk: the external "non-Quarto" identity is
   thin if it is just a mirror.

3. **Cargo workspace inheritance / path-or-version deps.** Publish the foundation
   crates from q2 but keep developing them in q2; the external repo pins versions.
   Requires decoupling the shared `[workspace.package]` metadata
   (`posit-dev/quarto-markdown-syntax` repository field, version `0.3.0`) per-crate.

Each option interacts with the error-code design: option 1/3 force the
catalog-pluggability work (external users get no `Q-*`); option 2 could defer it.

## Open questions for the next session

- Which extraction strategy (own / mirror / publish-from-q2)?
- Does `quarto-error-reporting` split into core + catalog, or gain a provider trait?
- Where do the **JSON wire types** (`json.rs`: `JsonDiagnostic`, shared with
  `wasm-quarto-hub-client` and `quarto-preview`) live after the split? They are
  catalog-adjacent and have their own q2 consumers.
- `schemars` is scoped into `quarto-error-reporting` deliberately (machine-to-machine
  JSON shapes); does the external core keep it?
- Naming/identity of the external project (the YAML validator is "Quarto's dialect"
  per the code comments — is the externalized lib still "Quarto-dialect YAML schema"
  or rebranded general-purpose?).
- How does the `error-code audit` (`scripts/audit-error-codes.py`) adapt — does it
  only police the q2 catalog, with the library carrying its own check?

## Source references

- `crates/quarto-yaml-validation/src/error.rs` — `ValidationErrorKind::error_code()`
  (hard-coded `Q-1-x`), `ValidationError`, `SchemaError`, source-loc usage.
- `crates/quarto-yaml-validation/src/diagnostic.rs` — `ValidationDiagnostic`,
  builder/source-map/catalog coupling, JSON output.
- `crates/quarto-error-reporting/src/catalog.rs` + `error_catalog.json` — centralized
  catalog (145 entries), `ErrorCodeInfo`, `get_docs_url`.
- `crates/quarto-error-reporting/src/builder.rs` / `diagnostic.rs` — `with_code`,
  `docs_url()`, ariadne/JSON rendering.
- `crates/quarto-error-reporting/src/lib.rs` — re-exports incl. `json` module.
- `crates/quarto-source-map/src/lib.rs` — public surface (`SourceContext`,
  `SourceInfo`, `map_offset`, …).
- `crates/quarto-yaml/src/lib.rs` — `YamlWithSourceInfo`, re-export of `SourceInfo`.
- `crates/validate-yaml/Cargo.toml` — the sole in-repo consumer.
- `scripts/audit-error-codes.py`, `scripts/quick-error-audit.sh` — catalog audit.
- Prior related notes: `claude-notes/error-id-system-design.md`,
  `claude-notes/error-reporting-design-research.md`,
  `claude-notes/yaml-validation-rust-design.md`,
  `claude-notes/validate-yaml-error-reporting-integration.md`.
