# Step 1: extract the diagnostics foundation into two standalone `posit-dev/` repos — `quarto-source-map` first, then `quarto-error-reporting`

> **Naming (decided 2026-06-27):** both externalized crates **keep their current
> names** — `quarto-source-map` and `quarto-error-reporting` (and `quarto-yaml`
> later). No `error-reporting-core` rename. We rename only "if it comes to it."
> Consequence: there is **no q2-side façade** (the name belongs to the external
> crate); q2 depends on the external `quarto-error-reporting` directly. The only
> crate carved out q2-side is `quarto-error-catalog` (the `Q-*` policy). `json.rs` +
> `coalesce.rs` stay in the external crate behind a default-off `json` feature
> (see "The split").

**Strand:** bd-egcyeym9
**Date:** 2026-06-26
**Status:** Plan (not started)
**Design contract:** `claude-notes/designs/cross-package-error-codes.md`
**Sibling plan (gated behind this):**
  `claude-notes/plans/2026-06-26-extract-quarto-yaml-validation-design.md`

This is **step 1 of 3** in the agreed sequence (foundation → YAML stack → q2
migration). It extracts and publishes the diagnostics *host* — the crate that
defines the cross-package error-code discipline — and proves it standalone
*before* any client (yaml-validation) adopts it.

## Scope

**In scope:** extract `quarto-source-map` into its own `posit-dev/` repo and
publish it (leaf-first); make `quarto-error-reporting` catalog-agnostic by carving
its `Q-*` catalog data out into a new q2-side `quarto-error-catalog` policy crate;
extract the (now catalog-agnostic) `quarto-error-reporting` into a **separate**
`posit-dev/` repo (depending on the now-published `quarto-source-map`) and publish
it; cut q2 over to the published crates, incrementally (source-map first).

**Two repos, not one** (decided 2026-06-27 — see Decisions §1). The crates split
naturally: `quarto-source-map` is a general source-location leaf with 14 in-repo
dependents and no relationship to diagnostics; `quarto-error-reporting` is the
diagnostics host. Independent repos = independent release cadences.

**Explicitly NOT in scope:** `quarto-yaml` / `quarto-yaml-validation` (step 2),
deleting `validate-yaml` (step 2), wiring yaml validation into q2 config, the
`Q-1-x` remap (step 2/3). This plan touches **no YAML code**.

## Grounding facts (measured 2026-06-26)

- **The catalog coupling inside `error-reporting` is one line.**
  `crates/quarto-error-reporting/src/diagnostic.rs:290` —
  `DiagnosticMessage::docs_url()` calls `crate::catalog::get_docs_url(code)`. That
  is the *entire* hard seam between the renderer and the Quarto catalog.
- **Only two external q2 crates call the catalog free-functions directly:**
  `quarto-core/src/project_resources.rs` and `quarto-core/src/theme_diagnostic.rs`.
  Everything else goes through `DiagnosticMessage`.
- **Direct Cargo dependents:** `quarto-source-map` 14, `quarto-error-reporting` 9.
  (The research doc's 26/19 counted transitive reach.)
- **Module sizes** (`crates/quarto-error-reporting/src/`): `diagnostic.rs` 1187,
  `builder.rs` 595, `coalesce.rs` 461, `json.rs` 480, `catalog.rs` 317,
  `macros.rs` 98, `lib.rs` 87.
- **`quarto-source-map`** is a true leaf: deps `serde`, `serde_json`, `smallvec`;
  no `description` field (must add one to publish).
- **The dependency between the two is strictly one-directional:**
  `quarto-error-reporting → quarto-source-map`, pervasively (`SourceInfo` is a field
  on `DiagnosticMessage` at `diagnostic.rs:126`; `SourceContext` threads through
  rendering and the builder). `quarto-source-map` depends on *nothing* in
  error-reporting. **This forces leaf-first extraction order:** source-map must be
  published before `quarto-error-reporting` can be (crates.io rejects unpublished
  path deps).
- **Workspace metadata** all crates inherit (`[workspace.package]`): `version =
  "0.7.0"`, `repository = posit-dev/quarto-markdown-syntax`, `license = MIT`,
  `edition = 2024`. The externalized crates must stop inheriting q2's `version`
  and `repository` (they get their own in the new repo).
- Neither crate has ever been published to crates.io (no public version line yet).

## The split: what goes where

Only **one** thing actually leaves `quarto-error-reporting`: the `Q-*` catalog
*data*. Everything else stays in the (now catalog-agnostic) crate, so consumers\'
imports are unchanged.

```
quarto-error-reporting  (EXTERNAL — keeps its name)   quarto-error-catalog (q2 — NEW)
  diagnostic.rs   DiagnosticMessage, render             error_catalog.json (Q-* data)
  builder.rs      DiagnosticMessageBuilder              ERROR_CATALOG static (moved here)
  macros.rs       convenience macros                    QuartoCatalog: CatalogProvider
  ErrorCodeInfo   (struct, moved from catalog.rs)       install() at startup
  CatalogProvider trait + OnceLock registry             (the Q-* policy + audit live here)
  get_docs_url/get_error_info/get_subsystem
    (now DELEGATE to the installed provider)
  json.rs    behind default-off `json` feature
  coalesce.rs (render-summary helper)
  deps: url, ariadne, thiserror, serde(_json);
        schemars ONLY under `json` feature
  NO error_catalog.json / ERROR_CATALOG static

quarto-source-map  (EXTERNAL — unchanged surface)
  leaf; serde/serde_json/smallvec
```

**Rationale for each boundary** (full version in the design note):

- `ErrorCodeInfo` (the *struct*) stays, because the `CatalogProvider` trait returns
  it. Only `ERROR_CATALOG` (the *static* + `error_catalog.json` loader) leaves →
  `quarto-error-catalog`, because the *data* is Quarto policy.
- `get_docs_url`/`get_error_info`/`get_subsystem` stay, reimplemented to delegate to
  the installed provider (was: read the static map). The two `quarto-core` callers
  and the `lib.rs` re-export are therefore **source-unchanged** (they still resolve
  against `quarto-error-reporting`). The one symbol that *does* move is the
  `ERROR_CATALOG` static re-export — audit for direct importers (the free functions
  cover almost all uses).
- `json.rs` + `coalesce.rs` **stay** in `quarto-error-reporting`, with `json` behind
  a **default-off `json` feature** so a non-Quarto consumer pulls neither the
  `schemars` dep nor the wire format. q2 enables `features = ["json"]`. This keeps
  every `use quarto_error_reporting::json::…` / `::coalesce::…` consumer
  source-unchanged. (Reverses the earlier Q4/Q5 "move json q2-side" — the
  feature-gate recovers the clean-build benefit with far less churn. Residual:
  the `quarto.org` `SCHEMA_URL` const ships behind the feature; revisit only if a
  non-Quarto user needs the wire format under a different scheme.)
- `quarto-source-map` moves with **no surface change** — only its manifest changes.

## The host seam (canonical definition — supersedes the sketch in the sibling plan)

```rust
// quarto-error-reporting (external, catalog-agnostic)
pub struct ErrorCodeInfo {                 // moved verbatim from catalog.rs
    pub subsystem: String,
    pub title: String,
    pub message_template: String,
    pub docs_url: Option<String>,
    pub since_version: String,
}

pub trait CatalogProvider: Send + Sync {
    fn lookup(&self, code: &str) -> Option<&ErrorCodeInfo>;
}

struct EmptyCatalog;                        // default: catalog-agnostic, standalone-usable
impl CatalogProvider for EmptyCatalog {
    fn lookup(&self, _: &str) -> Option<&ErrorCodeInfo> { None }
}

static CATALOG: std::sync::OnceLock<Box<dyn CatalogProvider>> = std::sync::OnceLock::new();

/// Embedders call once at startup; first write wins, later writes are no-ops.
pub fn install_catalog(p: Box<dyn CatalogProvider>) { let _ = CATALOG.set(p); }

fn catalog() -> &'static dyn CatalogProvider {
    CATALOG.get().map(|b| b.as_ref()).unwrap_or(&EmptyCatalog)
}

pub fn get_docs_url(code: &str) -> Option<&'static str> { /* via catalog() */ }
// get_error_info / get_subsystem likewise delegate.
```

```rust
// quarto-error-catalog (q2)
struct QuartoCatalog(std::collections::HashMap<String, ErrorCodeInfo>);
impl CatalogProvider for QuartoCatalog { /* HashMap::get */ }
pub fn install() {
    quarto_error_reporting::install_catalog(Box::new(QuartoCatalog::load_embedded()));
}
```

`std::sync::OnceLock` (not `once_cell`) so core needs no extra dep for the
registry. Init point: q2 binary `main` + the WASM bootstrap + a test helper call
`quarto_error_catalog::install()` before the first diagnostic renders. Double
install is harmless (first-write-wins).

The three phases below. **Phase 1** (extract source-map) and **Phase 2** (make
error-reporting catalog-agnostic in place) are independent and may overlap;
**Phase 3** (extract `quarto-error-reporting`) requires both — the carve-out done
*and* source-map published.

## Phase 1 — Extract `quarto-source-map` into its own repo (the leaf-first warmup)

The simplest possible extraction (a clean leaf, no split, no policy). Doing it
first proves the whole pipeline — `posit-dev/` repo setup, crates.io publish, and
the **WASM cutover** — on the easiest crate, before the harder error-reporting work.

- [x] **1a.** Created `posit-dev/quarto-source-map` (public,
      https://github.com/posit-dev/quarto-source-map): copied the 8 sources +
      `LICENSE`, wrote a standalone single-crate manifest (`version = "0.1.0"`,
      `edition = "2024"`, `description`, `repository`, `keywords`, `categories`,
      pinned dep versions serde 1.0.228 +rc / serde_json 1.0.149 / smallvec 1.13
      +serde), added `README.md` + `.gitignore`. Builds on **stable** rustc 1.95
      (no nightly needed).
- [x] **1b.** Local validation green: `cargo build`, `cargo test` (104 unit + 4
      doctests pass), `cargo publish --dry-run` clean. **CI workflow added**
      (`.github/workflows/ci.yml`, commit `ee3780d`): stable Rust, test matrix
      Linux/macOS/**Windows** + a fmt/clippy(`-D warnings`) job. First run green on
      all 4 jobs — Windows build confirmed (first time this crate has built there).
- [x] **1c.** Published `quarto-source-map 0.1.0` to crates.io (Carlos's personal
      account; `cargo owner --add github:posit-dev:<team>` deferred — weekend).
- [x] **1d.** q2 cutover (branch `braid/bd-egcyeym9-source-map-extraction`):
      flipped `[workspace.dependencies.quarto-source-map]` `path` → `version =
      "0.1.0"`; consolidated the 13 *main-workspace* members onto
      `{ workspace = true }`; deleted the in-tree `crates/quarto-source-map`.
      **Gotcha:** `wasm-quarto-hub-client` is *excluded* from the main workspace
      and is its own standalone workspace (refs every q2 crate by `path`), so it
      can't inherit a workspace dep — it gets a **direct** `quarto-source-map =
      "0.1.0"`. (A blanket `crates/*/Cargo.toml` rewrite broke it first; the WASM
      build caught it — and a `| tail` had masked the first verify's real exit
      code. Lesson: don't pipe `cargo xtask verify` through `tail`.) Final state:
      `cargo build --workspace` ✅, `cargo nextest run --workspace` 10238 ✅,
      **full `cargo xtask verify` (all 14 steps, incl. WASM build + hub tests)
      ✅**. Both root and wasm-crate `Cargo.lock` resolve from the crates.io
      registry with matching checksum. *(Not yet committed — awaiting user
      go-ahead per GIT PUSH POLICY.)*

**Phase 1 is functionally complete** (crate published + q2 cut over + full verify
green). Only the new repo's CI workflow (1b gap) and the deferred crates.io
owner-add remain as tidy-ups.

## Phase 2 — Make `quarto-error-reporting` catalog-agnostic, in place (TDD; independent of Phase 1)

Goal: carve the `Q-*` catalog *data* out into `quarto-error-catalog` and route the
renderer through the installed provider, proving it compiles and is
behaviour-preserving *before* any code leaves the repo. This is the bulk of the
engineering and is valuable even if the repo move slipped. Does **not** depend on
Phase 1. There is **no façade** — `quarto-error-reporting` keeps its name and its
public surface (minus the moved catalog *data*).

- [x] **2a (test first).** Behaviour pinned: `empty_catalog_returns_none`
      (direct, global-free) + `installed_catalog_is_used_by_lookups` (the single
      global-mutating test) in `quarto-error-reporting`; the positive
      `get_docs_url("Q-0-1") → quarto.org` case relocated to `quarto-error-catalog`
      integration tests (`install_makes_get_docs_url_resolve`,
      `diagnostic_docs_url_resolves_after_install`).
- [x] **2b.** `quarto-error-reporting` now catalog-agnostic: `CatalogProvider`
      trait + `EmptyCatalog` + `OnceLock` registry + `install_catalog` in
      `catalog.rs`; `get_docs_url`/`get_error_info`/`get_subsystem` keep their
      signatures but delegate to the installed provider (the `&'static` lifetime
      survives via `catalog(): &'static dyn CatalogProvider`). `ERROR_CATALOG`
      static + `include_str!` removed; `lib.rs` re-exports updated. `diagnostic.rs`
      `docs_url()` unchanged (still calls the local delegating fn); its positive
      doctest/test relaxed/relocated.
- [x] **2c.** New `quarto-error-catalog` crate: `error_catalog.json` (git-moved) +
      `ERROR_CATALOG` (Lazy) + `QuartoCatalog: CatalogProvider` + `install()`; the
      example moved here; the 10 data-presence tests ported (direct map access).
      `install()` wired into the `q2` binary `main`. **WASM deliberately does
      NOT install** (see the 2f note): the WASM bridge never surfaces docs URLs
      (`JsonDiagnostic` has no `docs_url` field), and installing would
      `include_str!` the 46 KB catalog into the bundle, pushing the WASM past
      hub-client's 35 MiB PWA precache limit. A legitimate "embedder installs
      nothing → EmptyCatalog" choice. The 2 `quarto-core` data-presence `#[test]`s
      now query
      `quarto_error_catalog::ERROR_CATALOG` directly (dev-dep added). **Audit
      script + ~25 path references updated** to `crates/quarto-error-catalog/…`;
      `scripts/audit-error-codes.py` passes (exit 0). Full workspace nextest:
      **10240 passed**.
- [x] **2d.** `json.rs` now behind a **default-off `json` feature** (`lib.rs`
      `#[cfg(feature = "json")]` on the module + re-export; `tests/schema_drift.rs`
      gated with `#![cfg(feature = "json")]`). Only **4** crates use the wire
      symbols (`quarto`, `quarto-core`, `quarto-preview`, `wasm-quarto-hub-client`);
      each now declares `features = ["json"]`. `to_json` (uses `serde_json::json!`,
      not the module) and `coalesce.rs` stay unconditional. Verified by `cargo
      tree`: `schemars` **absent** by default, present with `--features json`.
- [x] **2e.** `schemars` made `optional = true` + `[features] json =
      ["dep:schemars"]`. `once_cell` **dropped** (registry uses `std::sync::OnceLock`;
      confirmed unused via `cargo tree`). `url` stays. Two clippy fixes in the new
      code (`map().unwrap_or` → `match`; needless doctest `fn main`).
- [x] **2f.** `cargo xtask verify` **GREEN — all 14 steps** (incl. WASM build +
      hub-client tests). Two failures found + fixed en route: (1) two clippy lints
      in the new code (Step 1); (2) the WASM build (Step 7) broke hub-client's vite
      PWA step — wiring `install()` into the WASM bootstrap `include_str!`'d the
      46 KB catalog and forced it past the 35 MiB precache limit (`vite.config.ts`
      `maximumFileSizeToCacheInBytes`). Fixed *soundly* (not by raising the limit):
      removed the WASM `install()` + `quarto-error-catalog` dep, since the WASM
      surfaces no docs URLs — pure dead weight there. WASM now 36,684,365 B
      (15.8 KB **under** the limit). Workspace nextest **10240 passed**.

**Phase 2 is COMPLETE.** `quarto-error-reporting` is now catalog-agnostic (a
`CatalogProvider` seam + `EmptyCatalog` default, no embedded `Q-*` data); the q2
policy lives in the new `quarto-error-catalog`; `json` is a default-off feature.
The crate is ready to carve out (Phase 3) once the Phase-1-style repo/publish
machinery is pointed at it. Uncommitted on `braid/bd-egcyeym9-error-reporting-split`.

> At the end of Phase 2, q2 still builds `quarto-error-reporting` from a path dep;
> it is now catalog-agnostic and cleanly carve-able.

### Phase 2 — implementation notes (investigation 2026-06-27)

**Blast-radius finding (much smaller than feared).** A full workspace audit shows
the catalog is **completely decoupled from the production render path**:
- `DiagnosticMessage` does *not* consult the catalog to build its title/message
  (verified: no `get_error_info`/`message_template` use in `builder.rs`/`diagnostic.rs`).
- `to_text` (ariadne) and the JSON wire shape **never call `docs_url()`**; `json.rs`
  has no `docs_url` field. **Zero `.snap` files contain a `quarto.org/docs/errors`
  URL.** So the carve-out cannot change any rendered output or snapshot.
- `docs_url()` has **zero** consumers anywhere in the workspace. `ErrorCodeInfo` has
  no external users.
- The *only* catalog uses outside `quarto-error-reporting`: two `quarto-core`
  `#[test]`s (`project_resources.rs:1447`, `theme_diagnostic.rs:271`) asserting
  their codes are registered, plus the `examples/with_error_code.rs` example.

**Consequence:** `install()` is **behaviour-neutral today** (nothing reads the
catalog in production); it matters only for the data-verification tests and for
future features that surface docs URLs. This removes the test-fragility risk that
would otherwise come from an uninitialised global.

**Finalised design.**
- *`quarto-error-reporting` (catalog-agnostic):* keep `ErrorCodeInfo`; add
  `CatalogProvider` trait + `OnceLock<Box<dyn CatalogProvider>>` registry +
  `install_catalog()` + `EmptyCatalog` default. `get_docs_url`/`get_error_info`/
  `get_subsystem` keep their **exact signatures** (the `&'static` lifetime survives
  because `catalog()` returns `&'static dyn CatalogProvider`) but delegate to the
  installed provider. `diagnostic.rs:290` is unchanged (still calls the local
  `get_docs_url`). Remove `error_catalog.json` + the `ERROR_CATALOG` static + the
  data tests + the example. Drop the `ERROR_CATALOG` re-export from `lib.rs`.
- *`quarto-error-catalog` (new q2 crate):* `error_catalog.json` + the loader +
  `QuartoCatalog: CatalogProvider` + `install()`. Houses the moved data-presence
  tests, the moved doctests (e.g. `get_subsystem("Q-0-1") == Some("internal")`),
  and the moved example. Deps: `quarto-error-reporting`, `serde`, `serde_json`,
  `once_cell`.
- *Test seams:* test providers/`EmptyCatalog` are exercised **directly** (no
  global) wherever possible; exactly one test asserts the global-empty default and
  one asserts global-install delegation — safe under nextest's process-per-test
  (and per-process doctests). The two `quarto-core` tests gain a
  `quarto-error-catalog` dev-dependency and call `quarto_error_catalog::install()`.
- *Binaries/WASM:* wire `quarto_error_catalog::install()` into the `q2` binary
  `main` and the WASM bootstrap (future-proofing; behaviour-neutral now).

## Phase 3 — Extract `quarto-error-reporting` into a *separate* repo and cut q2 over

Requires Phase 1 (source-map published) **and** Phase 2 (carve-out done).

- [x] **3a.** Created **`posit-dev/quarto-error-reporting`** (public,
      https://github.com/posit-dev/quarto-error-reporting): copied src/ + tests/ +
      examples/ + `schemas/` + LICENSE; standalone single-crate manifest
      (`version = "0.1.0"`, edition 2024, explicit dep versions,
      **`quarto-source-map = "0.1.0"`** published dep, `schemars` optional +
      `[features] json`). Dropped `CONTRIBUTING-ERRORS.md` (Quarto catalog policy —
      belongs with `quarto-error-catalog`) and rewrote `README.md` for the
      catalog-agnostic library. One source fix needed for stable-clippy
      `-D warnings`: `macros.rs` had `items_after_test_module` (q2's pinned-nightly
      clippy tolerated it) — moved the test module below the `#[macro_export]`
      macros + dropped the now-redundant macro imports. (q2 deletes its copy at
      3e, so the standalone becomes the single source; no divergence.)
- [x] **3b.** Standalone CI green locally: builds **default (json off, no
      schemars)** and `--all-features`; `cargo test` 51 / `--all-features` 61 +
      doctests + schema_drift; fmt + clippy (both feature sets) clean.
      `.github/workflows/ci.yml` added (Linux/macOS/Windows, both feature sets).
      CI running on the new repo.
- [x] **3c.** External-consumer smoke test (scratchpad `er-smoke`): a separate
      crate with **default features** builds+renders a `DiagnosticMessage`, gets
      `EmptyCatalog` (no docs URL), then installs a custom `CatalogProvider` and
      resolves a URL — `cargo tree` confirms **no schemars** in its tree. Passes.
      `cargo publish --dry-run` clean (24 files, verify-built).
- [ ] **3d.** Publish `quarto-error-reporting` to crates.io. **(User step — needs
      crates.io credentials; depends on the published `quarto-source-map 0.1.0`,
      which is already live.)**
- [x] **3e.** q2 cutover (branch `braid/bd-egcyeym9-error-reporting-cutover`):
      flipped `[workspace.dependencies.quarto-error-reporting]` `path` →
      `version = "0.1.0"`; consolidated the 7 plain path-deps onto `{ workspace =
      true }`; the WASM crate (excluded standalone workspace) → `{ version =
      "0.1.0", features = ["json"] }`; the json consumers (`quarto`, `quarto-core`,
      `quarto-preview`) kept their `features = ["json"]` (now resolving to the
      external crate); deleted in-tree `crates/quarto-error-reporting`.
      `quarto-error-catalog` now depends on the external crate. Cargo.lock resolves
      `quarto-error-reporting 0.1.0` + transitively `quarto-source-map 0.1.0` from
      the registry. `cargo nextest run --workspace` **10177 passed**; **full
      `cargo xtask verify` GREEN** (all 14 steps incl. WASM — precache succeeded,
      bundle under limit).

**Phase 3 is COMPLETE.** Both foundation crates (`quarto-source-map`,
`quarto-error-reporting`) are now published to crates.io from their own
`posit-dev/` repos and consumed by q2 as version deps. The q2 tree keeps
`quarto-error-catalog` (the `Q-*` policy) + the 4 json consumers. Next up: the
YAML stack (`quarto-yaml` + `quarto-yaml-validation`) per the sibling plan.
- [x] **3f.** Updated `CLAUDE.md` crate layout: `quarto-error-reporting` +
      `quarto-source-map` moved to an "Externalized foundation crates" section
      (published from `posit-dev/`); added `quarto-error-catalog`. (Workspace member
      list needs no edit — the in-tree crate was pulled in via the `crates/*` glob,
      so deleting the directory removes it.)

## Decisions

1. **Repo granularity — DECIDED (2026-06-27): two repos**, one per *foundation*
   crate. The foundation crates split naturally (leaf source-location utility vs.
   diagnostics host) and already have independent dependent sets (14 vs. 9);
   co-locating buys only weak CI cohesion and needlessly couples their release
   cadences. **The YAML stack is different — DECIDED (2026-06-29): one repo,
   `posit-dev/quarto-yaml`, a Rust *workspace* with two crates** (`quarto-yaml` +
   `quarto-yaml-validation`). They are tightly coupled (validation depends on the
   parser) and both Quarto-dialect-specific, so a shared workspace fits. Both still
   publish to crates.io independently (leaf-first: `quarto-yaml`, then
   `quarto-yaml-validation`). Execution handoff:
   `claude-notes/plans/2026-06-29-yaml-stack-extraction-handoff.md`.

2. **Crate names — DECIDED (2026-06-27): keep current names.** Both externalized
   crates stay `quarto-source-map` and `quarto-error-reporting` (and `quarto-yaml`
   later). No rename now ("if it comes to it"). Consequence: no q2-side façade
   crate; the q2-only carve-out is `quarto-error-catalog`; `json`/`coalesce` stay in
   the external crate (`json` feature-gated). See the header note + "The split".

Open forks (settle before 1a / 3a):

3. **Version start** (each repo independently). Fresh `0.1.0` public line (honest —
   never published), vs. continue `0.7.0`. Recommend `0.1.0`.
4. **Distribution channel.** crates.io (recommended — that is the whole point for
   non-Quarto users) vs. git deps as an interim before the first publish.
5. **Repo names** for the two `posit-dev/…` repos (the source-map repo and the
   error-reporting repo — the *crate* names are fixed; the *repo* slugs are not).

## Risks

- **WASM cutover (1d, 3e).** Each external crate must build for
  `wasm32-unknown-unknown` (both should — pure Rust, no `std::fs`), and the
  async-trait/`?Send` rules in `.claude/rules/wasm.md` apply if any trait is
  touched. The `CatalogProvider` registry uses `OnceLock<Box<dyn CatalogProvider>>`;
  the `Send + Sync` bound is fine natively and irrelevant single-threaded in WASM,
  but confirm it compiles for the target. **Verify with the full
  `cargo xtask verify`, not just `cargo build`.** Phase 1d does this on the trivial
  leaf first, so 3e is already de-risked.
- **Install-ordering.** A diagnostic rendered before `install()` would silently use
  `EmptyCatalog` (no titles/URLs). Mitigation: install at the earliest entry
  points; a debug-only assertion or a test that renders a known `Q-*` code and
  checks its URL guards against a missing install.
- **Two copies during extract→cutover.** Each crate exists in both repos between
  publish and the q2 cutover that deletes the in-tree copy (1c→1d, 3d→3e). Keep the
  window short; pin the q2 dep to the exact published version.

## Test plan (TDD gates)

- *Behaviour-preservation (Phase 2):* the 2a tests — installed catalog reproduces
  today's `docs_url`; empty catalog returns `None`.
- *Catalog-agnostic (Phase 3):* `quarto-error-reporting`'s own test suite passes
  with `EmptyCatalog` and **no** dependency on any `Q-*` data.
- *External-consumer smoke (3c):* a throwaway crate builds a `DiagnosticMessage`,
  installs a trivial `CatalogProvider`, and renders — proving the published API is
  usable with zero Quarto context.
- *Full regression (1d, 3e):* `cargo xtask verify` (incl. hub-build) green with q2
  consuming each published crate in turn.
