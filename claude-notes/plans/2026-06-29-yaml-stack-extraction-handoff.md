# Handoff: extract the YAML stack (`quarto-yaml` + `quarto-yaml-validation`)

**Strand:** bd-egcyeym9 (final phase of the diagnostics/YAML extraction epic)
**Date:** 2026-06-29
**Audience:** an agent picking this up cold. You should not need to read the
session transcript — everything needed is here or in the linked docs.

---

## 1. Goal & motivation

Extract `quarto-yaml` and `quarto-yaml-validation` out of the q2 monorepo into a
**single new repository `posit-dev/quarto-yaml`, structured as a Rust workspace
with two crates**, and publish both to crates.io independently. The motivating
need: **invisible internal Posit consumers of `quarto-yaml-validation`** want a
standalone crate; and the error codes must follow the cross-package discipline.

This is the **last phase** of the epic. The diagnostics foundation is already
done and merged:
- `quarto-source-map 0.1.0` → `posit-dev/quarto-source-map` (crates.io) — PR #348.
- `quarto-error-reporting 0.1.0` → `posit-dev/quarto-error-reporting` (crates.io,
  catalog-agnostic, `json` feature-gated) — PRs #349 (carve-out) + #350 (cutover).

## 2. Preconditions (verify before starting)

- **PR #350 merged** (q2 consumes `quarto-error-reporting 0.1.0`). `git switch
  main && git pull`. Confirm `crates/quarto-error-reporting/` is **gone** and the
  workspace builds.
- Both foundation crates are live on crates.io at `0.1.0`.
- You have (or the user grants) the same setup used in Phases 1/3: GitHub `gh`
  auth with `posit-dev` org rights (SSH key SSO-authorized), and the **user**
  performs every `cargo publish` (crates.io credentials are theirs; publishing is
  irreversible).

## 3. Read these first (the mechanics are already proven)

- **`claude-notes/plans/2026-06-26-extract-error-reporting-foundation.md`** — the
  authoritative playbook. Phase 1 (source-map) is the leaf-extraction template you
  will mirror almost exactly; Phase 3 (error-reporting) is the second run with the
  gotchas. Read the **Risks** and the per-step completion notes.
- **`claude-notes/designs/cross-package-error-codes.md`** — the error-code
  discipline. Drives the one real design task (§6 below).
- **`claude-notes/plans/2026-06-26-extract-quarto-yaml-validation-design.md`** —
  the original YAML design doc. Note its top banner: parts about
  `error-reporting-core`/façade are superseded; the YAML-specific substance
  (origin codes, delete `validate-yaml`, the discipline application) still stands.

## 4. Repo structure (DECIDED: one two-crate workspace)

```
posit-dev/quarto-yaml/            (new repo)
  Cargo.toml                      # [workspace] members=["crates/*"]; [workspace.package]; [workspace.dependencies]
  crates/
    quarto-yaml/                  # the parser leaf
    quarto-yaml-validation/       # schema validation; depends on quarto-yaml
  LICENSE  README.md  .gitignore  .gitattributes  .github/workflows/ci.yml
```

- Use a **workspace** (unlike the single-crate foundation repos). Per-crate
  `Cargo.toml`s inherit `version`/`edition`/`license`/`repository` from
  `[workspace.package]` and pull shared deps from `[workspace.dependencies]`.
- **`.gitattributes` with `* text=auto eol=lf` from commit 1** — Phase 3's Windows
  CI caught a CRLF bug in committed JSON-vs-generated comparisons; start with LF
  enforced so you never hit it. (`quarto-yaml-validation` ships
  `test-fixtures/` YAML/JSON — same exposure.)
- **CI** (`.github/workflows/ci.yml`): mirror the foundation repos — matrix
  Linux/macOS/**Windows** on **stable** Rust, `fmt` + `clippy --all-targets -D
  warnings`, `cargo test`. (Both crates need no nightly.) Watch for **stable-clippy
  lints the q2 pinned-nightly tolerates** — Phase 3 hit `items_after_test_module`
  in `macros.rs`; fix in the new repo (q2 deletes its copy at cutover, so the
  standalone becomes the single source — no divergence).

## 5. Dependency & cutover facts (measured 2026-06-29 on main)

**`quarto-yaml`** (the leaf): deps `yaml-rust2`, `serde`, `thiserror`,
`quarto-source-map`. Its **only** quarto dep is the published source-map →
becomes `quarto-source-map = "0.1.0"`. Does **not** depend on
`quarto-error-reporting`. In-tree q2 consumers: **pampa, quarto-config,
quarto-core, quarto-lsp-core** (+ `validate-yaml`, being deleted).

**`quarto-yaml-validation`**: deps `anyhow`, `thiserror`, `serde`, `serde_json`,
`yaml-rust2`, `regex`, `quarto-yaml`, `quarto-source-map`,
`quarto-error-reporting`. The quarto deps become: `quarto-yaml` (intra-workspace),
`quarto-source-map = "0.1.0"`, `quarto-error-reporting = "0.1.0"`. **Only in-tree
consumer is `validate-yaml`** (the demo binary). **After deleting `validate-yaml`,
`quarto-yaml-validation` has ZERO q2 consumers** — q2 does not depend on it at all.

**Consequence for the q2 cutover:** q2 **deletes** `crates/quarto-yaml-validation`
AND `crates/validate-yaml`, keeps consuming `quarto-yaml` (now published), and
gains **no** dependency on the published `quarto-yaml-validation`. The latter is
published purely for the external Posit consumers.

### The WASM gotcha (read carefully — different from Phase 1)

`wasm-quarto-hub-client` is an *excluded standalone workspace*. It does **not**
directly depend on `quarto-yaml`; it gets it transitively via `pampa`/`quarto-core`
(path-included). Those crates use `quarto-yaml = { workspace = true }`, which
resolves against **q2's** `[workspace.dependencies]` (workspace inheritance follows
the crate's filesystem home — q2 root — even inside the WASM build). So:
- Convert the **path**-dep consumers (`pampa`, `quarto-config`) to
  `{ workspace = true }`; the `{ workspace = true }` ones (`quarto-core`,
  `quarto-lsp-core`) stay.
- Set `[workspace.dependencies.quarto-yaml]` → `version = "0.1.0"`.
- The WASM crate likely needs **no direct `quarto-yaml` dep** (it had a direct
  `quarto-source-map = "0.1.0"` only because it uses source-map directly). **Verify
  with the full `cargo xtask verify`** — the WASM build is the proof; if it can't
  resolve `quarto-yaml`, add a direct `quarto-yaml = "0.1.0"` to the wasm crate (as
  was needed for source-map).

## 6. The one design task: error codes (RESOLVED 2026-06-29 → option B)

> **DECISION (2026-06-29, user):** Option **(B)** — ship `Q-1-x` as-is in `0.1.0`,
> defer the origin-code migration (`yaml-schema/*`) to `0.2.0`. Rationale: keep
> `0.1.0` **non-breaking** for the invisible internal Posit consumers that
> currently key on `Q-1-x`; cut over to discipline-conformant origin codes in a
> coordinated `0.2.0`.
>
> **Consequence:** `quarto-yaml-validation/src/error.rs` is shipped **unchanged**
> for `0.1.0` — the 14 `ValidationErrorKind::error_code()` mappings (`Q-1-10` …
> `Q-1-99`) and the ~15 tests asserting them stay exactly as-is and stay green. No
> error-code work in this phase. With no catalog installed in the standalone repo,
> diagnostics render **code-only** (`EmptyCatalog`); tests assert on the
> `error_code()` string, not on rendered catalog text, so they are unaffected.
> A `0.2.0` TODO carries the `Q-1-x` → `yaml-schema/*` migration (see §6-original
> below for the proposed mapping).
>
> ### Original analysis (kept for the deferred 0.2.0 work)

`quarto-yaml-validation/src/error.rs` `ValidationErrorKind::error_code()` currently
returns **Quarto presentation codes** `Q-1-10`, `Q-1-11`, … These do **not** belong
in a standalone library (they are q2's namespace, per the discipline). It has
**no** dependency on an installed catalog (no `get_docs_url`/`install_catalog`
refs), so the change is localized to `error_code()` + the ~15 `error.rs` tests that
assert `"Q-1-x"`.

Per `cross-package-error-codes.md`, change `error_code()` to **own, namespaced
origin codes** — e.g. `yaml-schema/missing-required`, `yaml-schema/type-mismatch`,
`yaml-schema/invalid-enum`, … (one per `ValidationErrorKind` variant). There is
**no q2 remap** to build (q2 doesn't consume the crate); the external consumers get
the origin codes and may remap to their own presentation codes.

> **⚠️ DECISION TO CONFIRM WITH THE USER before implementing.** Changing `Q-1-x` →
> `yaml-schema/*` is **breaking** for the invisible Posit consumers that currently
> see `Q-1-x`. Options:
> - **(A) Origin codes from `0.1.0`** — clean per the discipline; coordinate the
>   break with those consumers. *Recommended* (0.1.0 is a fresh public line; do it
>   right from the start).
> - **(B) Ship `Q-1-x` as-is in `0.1.0`, defer origin codes to `0.2.0`** —
>   non-breaking now, but ships Quarto codes in a "non-Quarto" crate (violates the
>   discipline) until later.
>
> Ask the user which, and (for A) capture the `Q-1-x` → `yaml-schema/*` mapping as
> a frozen table in the commit message so the lineage is recoverable.

## 7. Execution checklist

### Phase A — `quarto-yaml` (the leaf; publish first)
> **Status 2026-06-29:** local scaffolding + verification DONE; committed locally
> on `main` of `posit-dev/quarto-yaml` (commit `06c6dd3`). Stopped at the outward
> gate (`gh repo create` + user `cargo publish`).
- [x] Create `/Users/cscheid/repos/github/posit-dev/quarto-yaml/` as a **workspace**;
      copied `crates/quarto-yaml/` (src + benches) → `crates/quarto-yaml/`; added
      `LICENSE` (from q2 root), `.gitignore` (`/target`), `.gitattributes`
      (`* text=auto eol=lf`), repo `README.md` + crate `README.md`.
- [x] Workspace `Cargo.toml`: `[workspace] resolver="3" members=["crates/*"]`,
      `[workspace.package]` (version `0.1.0`, edition `2024`, license `MIT`,
      `repository`/`homepage` = posit-dev/quarto-yaml, authors), and
      `[workspace.dependencies]` with `quarto-source-map = "0.1.0"` + shared deps
      pinned to q2 (`yaml-rust2 0.11`, `serde 1.0.228`, `thiserror 2.0`,
      `serde_json 1.0.149`, `anyhow 1.0.101`, `regex 1.12`). Added a minimal
      `[workspace.lints.clippy]` (`result_large_err`/`large_enum_variant` = allow,
      to preserve the public `Result`/`Error` API — not q2's full allow-list).
- [x] Build + `cargo test` (44: 39 unit + 5 doctest) + `cargo clippy
      --all-targets -- -D warnings` (clean after the lints block) + `cargo fmt
      --check` + `cargo publish --dry-run -p quarto-yaml` (10 files) — all green on
      stable rustc 1.95.
- [x] External-consumer smoke test (separate crate, path dep, parsed
      `title: My Document`, asserted the value's `source_info.start_offset() == 7`)
      — public API usable standalone. ✅
- [x] `gh repo create posit-dev/quarto-yaml --public --source=. --push` — done
      2026-06-29 (https://github.com/posit-dev/quarto-yaml). **CI green on all 3
      OSes** (run 28377015391: ubuntu 26s, macos 22s, windows 1m4s, fmt+clippy 22s).
- [x] **USER:** `cargo publish -p quarto-yaml` — **DONE** (live on crates.io as
      `quarto-yaml 0.1.0`, 2026-06-29).

### Phase B — `quarto-yaml-validation` (second crate, same repo)
> **Status 2026-06-29:** local work + CI DONE; committed `ac8d72b`, pushed to
> `main`, **CI green on all 3 OSes** (run 28377797754: ubuntu 30s, macos 37s,
> windows 2m44s, fmt+clippy 28s). Awaiting user `cargo publish`.
- [x] Copied `crates/quarto-yaml-validation/` (src + integration tests +
      `test-fixtures/` + the 4 design `.md`s) into the workspace. `quarto-yaml`
      dep = `{ workspace = true }`, where the workspace entry carries both
      `path = "crates/quarto-yaml"` (local dev) and `version = "0.1.0"` (so
      `cargo publish` resolves the registry crate); `quarto-source-map` /
      `quarto-error-reporting` = `"0.1.0"` (default features — no json/coalesce
      use, so json feature not needed).
- [x] **Error codes:** NO change for `0.1.0` (decision **B** — keep `Q-1-x`).
      `error.rs` shipped verbatim; the 14 mappings + ~15 tests stay green.
- [x] No `Q-1-x` test/snapshot edits needed: the render path never consults the
      catalog (title "YAML Validation Failed" is hardcoded; no docs URL surfaced),
      so the diagnostic snapshot reproduced **byte-for-byte** with no catalog
      installed. One stable-clippy fix: moved the test module to end-of-file in
      `schema/parsers/combinators.rs` (`items_after_test_module`).
- [x] Build + test (330 incl. snapshot; 5 pre-existing ignored doctests) + clippy
      `-D warnings` + fmt + `cargo publish --dry-run -p quarto-yaml-validation`
      (39 files) + external smoke test (validated a doc → `Q-1-11` standalone,
      rendered `[Q-1-11] age: Expected number, got string`). All green.
- [x] CI green (3 OSes). **USER (pending):** `cargo publish -p
      quarto-yaml-validation` (`quarto-yaml 0.1.0` is already live, so the dep
      resolves).

### Phase C — q2 cutover (one PR, like #348/#350)
> **Status 2026-06-29:** all local steps DONE on branch
> `braid/bd-egcyeym9-yaml-cutover`; **full `cargo xtask verify` GREEN (all 14
> steps incl. WASM build + hub tests)**. Awaiting commit/push/PR.
- [x] Branch `braid/bd-egcyeym9-yaml-cutover` off updated main (HEAD `df029875`).
- [x] `[workspace.dependencies.quarto-yaml]` `path` → `version = "0.1.0"`.
- [x] Converted `quarto-yaml` path-deps (`pampa`, `quarto-config`) →
      `{ workspace = true }`; left the existing `{ workspace = true }` ones
      (`quarto-core`, `quarto-lsp-core`).
- [x] **Deleted** `crates/quarto-yaml-validation/` and `crates/validate-yaml/`;
      dropped `[workspace.dependencies.quarto-yaml-validation]`. No stray
      `validate-yaml` refs in xtask/CI/docs — only historical `claude-notes/`
      design docs (left as-is); one prose comment in
      `quarto-core/src/attribution/mode.rs` mentions the crate as a hypothetical
      consumer (left — design rationale, not a dep).
- [x] Deleted in-tree `crates/quarto-yaml/`.
- [x] `cargo build --workspace` clean; **`Cargo.lock` resolves `quarto-yaml 0.1.0`
      from the registry** (checksum `c32ab7b3…`); `quarto-yaml-validation` /
      `validate-yaml` absent. `cargo nextest run --workspace` **9855 passed**.
      **Full `cargo xtask verify` GREEN (14/14)** — the WASM crate resolved
      `quarto-yaml` *transitively* (no direct dep needed, per §5); its own
      `Cargo.lock` flipped `0.7.0` path → `0.1.0` registry.
- [x] Updated `CLAUDE.md`: `quarto-yaml` moved to the "Externalized foundation
      crates" section; `quarto-yaml-validation` reframed as published-but-not-a-q2-
      dep; `validate-yaml` binary line removed.
- [ ] Commit, push to `feature/bd-egcyeym9-yaml-cutover`, open PR against `main`,
      watch CI, report. Merge is the user's call.

## 8. Proven gotchas (from Phases 1 & 3 — don't rediscover them)

- **CRLF on Windows** → `.gitattributes` `* text=auto eol=lf` from the first commit.
- **Stable clippy stricter than q2's nightly** → fix lints in the new repo
  (`items_after_test_module` etc.); the standalone repo becomes the single source.
- **WASM workspace resolution** → §5; verify with full `cargo xtask verify`, never
  `--skip-hub-build`.
- **`| tail` masks `cargo xtask verify`'s real exit code** → run it without a tail
  pipe (or check the file), or use `run_in_background`.
- **crates.io / GitHub are user/identity-gated and irreversible** → you prep & dry-
  run; the user publishes and (optionally) `cargo owner --add github:posit-dev:<team>`.

## 9. Open items — RESOLVED 2026-06-29

1. **Error-code policy (§6 A vs B)** — **RESOLVED: option B** (keep `Q-1-x` in
   `0.1.0`, defer origin codes to `0.2.0`). See §6.
2. **Repo = `posit-dev/quarto-yaml` (workspace, two crates)** — **CONFIRMED**.
3. **Process choices** — **CONFIRMED: mirror Phase 1/3.** Public repo; agent preps
   + dry-runs; **user** runs each `cargo publish` (leaf `quarto-yaml` first);
   personal crates.io account now, `cargo owner --add posit-dev` deferred to a
   weekday.
4. **Relocate `CONTRIBUTING-ERRORS.md` / q2 YAML docs** — low priority; default to
   **skip** unless the user asks. (Phase 3 dropped its `CONTRIBUTING-ERRORS.md`;
   the Quarto catalog policy lives with `quarto-error-catalog`.)
