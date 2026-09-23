# P4 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P4-run-machinery.md`](2026-08-20-pandoc-hybrid-P4-run-machinery.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Research companion:** [`../research/2026-07-13-q1-format-typescript.md`](../research/2026-07-13-q1-format-typescript.md)
**Depends on:** (per the epic's graph) **P2** — needs the wire-format schema for Task 9's
serialization step, though Tasks 1-8 can proceed against the *frozen schema decision* before P2's
implementation lands. P4 is scheduled **before P5**, so the `main.lua` splice patch (Task 8) ships
before the shim it splices in; Task 8's seams are bound to *position and shape*, never conversion.
**Status:** Ready for subagent-driven execution. No blockers remain on P4's own deliverable.
**P4's transport smoke depends on P2 Task 7** (`quarto_pandoc_reader_opts`).

This file adds nothing to P4's scope — it converts P4's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P4 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P4 + the design doc; where this file and the plan disagree, the plan wins.

Lua/TS citations are read against `quarto-cli` at tag **`v1.11.3`** (read via
`git show v1.11.3:<path>` against `/Users/gordon/src/quarto-cli`, not the local checkout's current
tip). Rust citations are read against this worktree. Behaviour claims marked **(measured)** were
reproduced by actually running `pandoc 3.8.1` against a materialized `v1.11.3` tree — see the
worked example in Task 4.

---

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **`U`** | Rust unit test | `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **`I`** | Rust integration test, in-process, no external binary | `crates/<crate>/tests/integration/<name>.rs`, registered `pub mod <name>;` in that crate's `tests/integration/main.rs` | `cargo nextest run -p <crate>` |
| **`L`** | Lua/pandoc integration test — a **real** `pandoc` subprocess with `--data-dir` + `-L main.lua` against the materialized Q1 tree | same file layout as `I`; gated (see below) | `cargo nextest run -p quarto-core` |
| **`G`** | Dev-only golden capture needing a real Q1 `quarto` binary and/or `external-sources/` | not in CI (CLAUDE.md External Sources Policy) | local/dev only |
| **`X`** | `cargo xtask` lint / verify gate; the rule's own unit tests are the seam | `crates/xtask/src/lint/<rule>.rs` (per-file) or a repo-level `check(workspace_root)` | `cargo xtask lint` / `cargo xtask verify` |

**Per `.claude/rules/integration-tests.md`, never add a top-level `crates/<crate>/tests/<name>.rs`.**
One `integration` binary per crate. `crates/quarto-core/tests/integration/main.rs` exists today with
96 `pub mod` entries (`:4-99`); new files are appended alphabetically.

### `L`-tier gate policy (explicit, and its skipping is visible)

A silently-skipping test is a vacuous test. The `L` tier uses **two** gates and makes both audible:

1. **Hard gate, not a skip.** A helper `assert_pandoc_available()` — modelled on
   `crates/pampa/tests/integration/test.rs:161` `assert_good_pandoc_version()`, which already
   `.expect()`-panics when pandoc is absent — **panics** rather than returning `false`. Pandoc is
   already a hard dependency of `cargo nextest run --workspace` today (pampa's four oracle tests at
   `test.rs:418,445,623,710` call that helper), so the `L` tier introduces no new environment
   requirement, and a missing pandoc must not read as green.
2. **Version gate is also a hard failure.** Below P4's floor (Task 7), the helper panics naming the
   floor. It must **not** compare against a soft window and skip: that is exactly how
   `preview-renderer` reddened unnoticed on `main` (GH #250).
3. **An `L`-tier census test.** One `U` test asserts the count of `#[test]` functions carrying the
   `L`-tier marker attribute equals a hardcoded constant. If someone converts an `L` test to a
   skip-if-absent shape, or deletes one, the census reddens. This is the "how its skipping is
   itself visible" mechanism.

There is deliberately **no `QUARTO_TEST_PANDOC=1`-style opt-in env var.** An opt-in gate is a skip
by default, and the plan's whole point is that the `L` tier is P4's deliverable.

---

## Which of P4's tasks unblocks the `L` tier for P5/P6/P7

**Task 2** — *Materialize both vendored trees to disk in Q1's required layout + the runtime-environment
contract + the `pandoc_lua_harness` test helper.*

Task 2 delivers the named helper

```rust
// crates/quarto-core/src/pandoc_filters/harness.rs  (cfg(any(test, feature = "test-harness")))
pub fn run_main_lua(ast_json: &str, to_format: &str, params_blob_json: &str, out: &Path)
    -> PandocRunOutcome   // { status: ExitStatus, stderr: String, out_path: PathBuf }
```

It takes the params blob as a **caller-supplied JSON string**, so Task 2's own `L` tests use a
hand-written minimal blob (verified to work — see Task 4's worked example) and do not wait on
Task 4's builder. Task 4 then supplies the *real* builder's output to the same helper.

Successor plans defer to this capability **by name**: "requires P4 Task 2's `pandoc_lua_harness`
(`run_main_lua`)". Concretely:
- **P5** Layer-2 golden renders and the Route-N function-arity probe.
- **P6** figure/theorem/callout-number parity renders.
- **P7** per-format golden captures (the `G` tier's Q2 half; its Q1 half needs a real `quarto`
  binary and stays out of CI).

Task 2 is **not** sufficient for *content* assertions — those need P5's shim. Task 2's own `L`
assertions are limited to "pandoc exited 0, bytes exist, the env contract held".

---

## Task 1: Vendor the two Q1 source subtrees at tag `v1.11.3`, with an ours-vs-pinned README

**Scope.** Create `resources/pandoc-filters/` holding two copies of upstream `v1.11.3` subtrees, plus
a `README.md` mirroring `resources/scss/README.md`'s structure, plus the `include_dir!` statics that
embed them. No materialization, no running — Task 2 owns that.

**Files** (all *ours*, i.e. q2 proper unless marked):
- `resources/pandoc-filters/README.md` — new. Mirror `resources/scss/README.md`'s headings, verified
  today as: `# …` `:1`, `## Contents` `:5`, `## Source` `:16`, `## Updating` `:23`,
  `## Why Local Copy?` `:49`, `## License` `:58`. Add one **new** section the SCSS README has no
  need for: `## Ours vs. pinned` — an explicit file list of everything in the tree that is *not*
  from `v1.11.3`, because a re-vendor is a delete-and-recopy that would otherwise remove it.
  At minimum: the marked `main.lua` patch (Task 8), the placeholder/real shim file (Task 8 / P5),
  and P3's 3-file crossref patch.
- `resources/pandoc-filters/filters/**` — **the vendored copy** of upstream
  `src/resources/filters/` at `v1.11.3`. Root file `main.lua`; its `import()` block spans
  `main.lua:13` (`import("./mainstateinit.lua")`) through `main.lua:203`
  (`import("./quarto-init/metainit.lua")`) — **170 `import(...)` lines.**
- `resources/pandoc-filters/pandoc/datadir/**` — **the vendored copy** of upstream
  `src/resources/pandoc/datadir/` at `v1.11.3`: 27 files, `init.lua` plus `_base64.lua`,
  `_format.lua`, `_json.lua`, `_utils.lua`, `logging.lua`, `lpegfenceddiv.lua`,
  `lpegshortcode.lua`, `profiler.lua`, `readqmd.lua`, and the `luacov/` subtree.
  **Note the directory names are load-bearing, not cosmetic** — see Task 2: `init.lua:257` derives
  the filters path from the data-dir's own location.
- `crates/quarto-core/src/pandoc_filters/mod.rs` — new; the two `include_dir!` statics and the pin
  constants (`QUARTO_CLI_PIN: &str = "v1.11.3"`, `PANDOC_PIN: &str = "3.10"`).
- `crates/xtask/src/lint/vendored_pandoc_filters.rs` — new repo-level rule; registered by appending
  one `all_violations.extend(...)` line in `crates/xtask/src/lint/mod.rs::run_check` at the
  repo-level block (`mod.rs:93-107`, whose own comment reads "Repo-level checks: these reconcile
  whole trees rather than grepping a single Rust file"). **Not** `check_file` (`mod.rs:217`), which
  is the per-file tier.

**Acceptance criterion.** `cargo xtask lint` is green (including `external-sources-in-macro` —
no `include_dir!` may point at `external-sources/`); the README's `## Source` section records
upstream tag `v1.11.3` **and** pandoc `3.10`; the two `include_dir!` statics each resolve every
intra-tree module reference (tests below).

**Prerequisite.** None. Task 1 is P4's entry point.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | `pandoc_filters::FILTERS_DIR` (`include_dir!`) | For every `import("./X")` line parsed out of the embedded `main.lua`, `FILTERS_DIR.get_file(X)` is `Some` → assert all 170 resolve, and assert the parsed count `== 170` | none (bytes are compiled in) | the `include_dir!("$CARGO_MANIFEST_DIR/../../resources/pandoc-filters/filters")` static, or any one copied subdirectory (`layout/`, `crossref/`, …) |
| T1.2 | U | `pandoc_filters::DATADIR_DIR` | For every `require '<mod>'` in the embedded `init.lua` (`_format`, `_base64`, `_json`, `_utils`, `logging` — `init.lua:149-154`), `DATADIR_DIR.get_file("<mod>.lua")` is `Some` | none | the `include_dir!` for the datadir subtree, or deleting `_format.lua` from the copy |
| T1.3 | U | `pandoc_filters::{QUARTO_CLI_PIN, PANDOC_PIN}` | Parse `resources/pandoc-filters/README.md`'s `## Source` section → assert the recorded quarto-cli tag `== QUARTO_CLI_PIN` and the recorded pandoc version `== PANDOC_PIN` | filesystem read of an in-repo file | the README's recorded tag line, or either constant |
| T1.4 | X | `xtask::lint::vendored_pandoc_filters::check(workspace_root)` | Rule run against a synthetic tree in which a file listed under the README's `## Ours vs. pinned` is absent → exactly one `Violation`, anchored at that README line | `tempfile` tree standing in for `workspace_root` (the *rule* is the unit under test and is not mocked) | the `check` fn's "for each ours-listed path, assert it exists" loop |
| T1.5 | X | the same rule, positive direction | Rule run against the real `workspace_root` → zero violations | none | any ours-listed file actually deleted from the tree |

**Revert hunks, stated exactly:**
- T1.1 — Revert the `include_dir!` static for `resources/pandoc-filters/filters` (or delete the
  copied `layout/` subdirectory) → `assert_eq!(unresolved, Vec::<String>::new())` in
  `test_embedded_filters_closure_is_complete` RED.
- T1.2 — Revert the vendoring of `resources/pandoc-filters/pandoc/datadir/_format.lua` →
  `assert!(DATADIR_DIR.get_file("_format.lua").is_some())` in
  `test_embedded_datadir_requires_resolve` RED.
- T1.3 — Revert the `## Source` section's `quarto-cli tag: v1.11.3` line (or change
  `QUARTO_CLI_PIN`) → `assert_eq!(readme_tag, QUARTO_CLI_PIN)` in `test_readme_records_the_pins` RED.
- T1.4 — Revert the ours-listed-path existence loop in
  `vendored_pandoc_filters::check` → `assert_eq!(violations.len(), 1)` in
  `test_missing_ours_file_is_flagged` RED.
- T1.5 — Revert (delete) the real shim file from `resources/pandoc-filters/filters/` →
  `assert!(violations.is_empty())` in `test_real_tree_is_clean` RED. **This is the mechanical
  re-vendor guard**; without T1.4/T1.5 the README is the whole mitigation.

### Refactor-induced vacuity check

- **T1.1's expected value `170`.** The plan carried "~45 `import` lines at `main.lua:13-60`"; the
  measured count today is 170, spanning `:13-203`. Asserting only "all parsed imports resolve" is
  *non-discriminating against a parser bug*: a regex that matches nothing makes `unresolved` empty
  and the test green. The count assertion is the discriminator; it must be a literal, and a future
  re-vendor that changes it is a deliberate edit. Keep both assertions.
- **T1.3 is shape-only, not behaviour.** It cannot detect that the *copied bytes* came from a
  different tag — only that the README and the constants agree. The bytes' provenance is
  `accepted-untested`; see **Missing-test pass**.

---

## Task 2: Materialize both trees in Q1's required layout, fix the runtime-environment contract, and deliver the `pandoc_lua_harness`

**Scope.** Extract the two embedded trees to disk via `ResourceBundle` into **one** root shaped
`<share>/pandoc/datadir/` + `<share>/filters/`, set the env contract Q1's Lua requires, and ship the
`run_main_lua` test helper that creates the `L` tier. Also document the cwd / temp-file /
`--resource-path` contract the plan's checklist asks for.

**Files:**
- `crates/quarto-core/src/pandoc_filters/bundle.rs` — new (ours). Two `ResourceBundle`s, or one
  bundle per tree extracted into a shared parent. `ResourceBundle` is at
  `crates/quarto-core/src/resources.rs:383-392` (struct), `impl` `:394-454`; the disk-materializing
  methods are `ResourceBundle::path` `:421` (lazy, `get_or_init`) and the private
  `ResourceBundle::extract` `:443`.
- `crates/quarto-core/src/pandoc_filters/harness.rs` — new (ours); `run_main_lua`.
- `crates/quarto-core/tests/integration/pandoc_transport.rs` — new (ours); registered as
  `pub mod pandoc_transport;` in `crates/quarto-core/tests/integration/main.rs` (alphabetical: after
  `page_navigation_pipeline` `:68`, before `pass1_engine_resolution_pipeline` `:69`).
- `resources/pandoc-filters/README.md` — extend `## Contents` with the layout requirement.

**The layout and env contract, measured.** `--data-dir` alone is **not** sufficient:
- `init.lua:123-132` reads `os.getenv("QUARTO_SHARE_PATH")` and, only if non-nil, appends
  `<share>/pandoc/datadir/?.lua` to `package.path`. Without it, `init.lua`'s own
  `local format = require '_format'` (`init.lua:149`) fails and pandoc refuses to run any filter:
  `Couldn't load 'init.lua': … module '_format' not found`, **exit 83** (measured, pandoc 3.8.1).
- `init.lua:257` appends `pandoc.path.normalize(PANDOC_STATE.user_data_dir .. '/../../filters/?.lua')`
  — so the filters tree must sit two levels above the data-dir, i.e. the single-root layout above:
  two source subtrees, **one** materialized root with Q1's own relative shape.
- `QUARTO_FILTER_DEPENDENCY_FILE` must point at a writable file. Without it, `init.lua`'s
  dependency-file accessor `fail()`s and pandoc dumps ~35 KB of `init.lua` source to stderr **on an
  otherwise successful (exit 0) render** (measured). The plan's params table already notes
  `initFilterParams`'s env side effect; this is the observable consequence.
- `--resource-path` / cwd: `normalize/astpipeline.lua:68` joins `QUARTO_SHARE_PATH` with
  `scripts/juice.ts` (HTML-only, not reached for docx/pptx) — the only other `QUARTO_SHARE_PATH`
  consumer in the filters tree. Record it; do not vendor `scripts/`.

**Acceptance criterion.** `run_main_lua` with a hand-written minimal params blob and a trivial
Pandoc-JSON AST exits **0**, writes a non-empty `.docx` whose first four bytes are `PK\x03\x04`, and
leaves stderr empty except for genuine document warnings.

**Prerequisite.** Task 1 (the embedded trees). Nothing from P5 — Task 2 runs `main.lua` *without*
the shim, which is exactly what Task 8's prerequisite note bounds.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | U | `pandoc_filters::bundle::share_path()` | Call it → assert `share_path().join("filters/main.lua")` and `share_path().join("pandoc/datadir/init.lua")` both exist, **and share the same parent root** | none (real extraction to a real temp dir) | the `extract_into(<root>/filters)` / `extract_into(<root>/pandoc/datadir)` destination arguments in `bundle.rs` |
| T2.2 | **L** | `harness::run_main_lua` + the real `pandoc` + the real materialized `main.lua` | Feed a 3-block Pandoc-JSON AST and the Task-4 worked-example blob → assert `status.success()`, `out_path` exists, first 4 bytes `== b"PK\x03\x04"` | **nothing is mocked** — real pandoc subprocess, real Lua, real vendored tree | the `cmd.env("QUARTO_SHARE_PATH", share_path())` line in `run_main_lua` |
| T2.3 | **L** | the same, env-contract negative | Invoke with `QUARTO_SHARE_PATH` explicitly removed → assert exit code `== 83` **and** `stderr.contains("module '_format' not found")` | none | the same `cmd.env("QUARTO_SHARE_PATH", …)` line (this test asserts the *failure shape*, so it is the one that documents *why* the line exists) |
| T2.4 | **L** | the same, dependency-file contract | Successful render → assert `stderr` contains no `"Missing expected dependency file environment variable"` and `stderr.len() < 4096` | none | the `cmd.env("QUARTO_FILTER_DEPENDENCY_FILE", …)` line |
| T2.5 | U | `bundle::share_path()` caching | Call twice → assert the same path both times (the `OnceLock<Result<TempDir, String>>` at `resources.rs:391` is honoured, one extraction per process) | none | replacing `get_or_init` with a fresh `extract()` per call |
| T2.6 | U | the `L`-tier census | Count `#[test]` fns carrying the `L` marker across `crates/quarto-core/tests/integration/pandoc_*.rs` → assert `== L_TIER_TEST_COUNT` | filesystem read of the crate's own test sources | any `L` test converted to a `return`-if-absent skip, or deleted |

**Revert hunks, stated exactly:**
- T2.1 — Revert `bundle.rs`'s single-root destination (extract the two trees to two independent
  temp roots) → `assert_eq!(filters_root.parent(), datadir_root.parent().and_then(Path::parent))`
  in `test_materialized_layout_is_single_rooted` RED.
- T2.2 — Revert the `cmd.env("QUARTO_SHARE_PATH", share_path())` line in `run_main_lua` →
  `assert!(outcome.status.success())` in `test_run_main_lua_produces_docx_bytes` RED **with exit
  83** (measured).
- T2.3 — Revert the same line → this test instead goes **GREEN by construction** and must be read
  as its documentation, not its guard; see the vacuity check below.
- T2.4 — Revert the `cmd.env("QUARTO_FILTER_DEPENDENCY_FILE", …)` line →
  `assert!(outcome.stderr.len() < 4096)` in `test_successful_render_stderr_is_quiet` RED (measured:
  ~35 KB of `init.lua` source appears).
- T2.5 — Revert `ResourceBundle::path`'s `get_or_init` to an unconditional `extract()` →
  `assert_eq!(first, second)` in `test_share_path_is_extracted_once` RED.
- T2.6 — Revert any `L` test to a skip-shaped body, or delete one →
  `assert_eq!(found, L_TIER_TEST_COUNT)` in `test_l_tier_census` RED.

### Refactor-induced vacuity check

- **T2.3 survives its own revert.** It asserts the *failure* that occurs when `QUARTO_SHARE_PATH` is
  absent, and it removes the var itself — so it passes whether or not the production code sets it.
  It is therefore **shape/gating only**, not a guard; T2.2 carries the discriminator. Recorded here
  rather than deleted, because the exact stderr string (`module '_format' not found`) is the
  diagnostic a future implementer will actually see and needs a committed record of.
- **T2.2's `PK\x03\x04` prefix.** Non-discriminating against "pandoc ran but the filters did
  nothing" — a docx produced with no `-L` at all also starts `PK`. That is deliberate: Task 2's
  contract is transport, not content. The "the Lua actually ran" assertion lives in Task 4 (T4.7,
  the sentinel) and Task 5 (T5.3, an observable param effect). **Do not strengthen T2.2 into a
  content assertion — it would be asserting P5's behaviour from P4.**
- **T2.4's `4096` bound.** Chosen so the measured ~35 KB failure is caught while the measured
  77-byte success case (`[WARNING] Could not fetch resource img.png…`) passes. If a future fixture
  legitimately emits more warnings, raise the bound *and* re-confirm the 35 KB case still exceeds it.

---

## Task 3: The `QUARTO_FILTER_PARAMS` codec — standard-padded base64 + the platform env-block bound

**Scope.** Two small pure functions: the base64 encoder pinned to the exact variant Q1's decoder
accepts, and the Windows environment-block size predicate. Kept separate from Task 4 because the
*fallback* half of the size bound is `accepted-untested` by decision (no fallback is designed) and
must not block the params builder.

**Files:**
- `crates/quarto-core/src/pandoc_filters/params_codec.rs` — new (ours).
  - `pub fn encode_params_blob(json: &str) -> String` — must use
    `base64::engine::general_purpose::STANDARD` (standard alphabet, **with** padding). Upstream
    writers: `pandoc.ts:310` and `pandoc.ts:335`, both `encodeBase64(JSON.stringify(...))` from
    Deno's `encoding/base64`, which is standard+padded. Upstream decoder: `init.lua:596`
    `base64.decode(os.getenv("QUARTO_FILTER_PARAMS"))`, then `init.lua:599-604`
    `function param(name, default)`.
  - `pub fn params_blob_exceeds_platform_limit(len: usize) -> bool` — the Windows 32,767-character
    environment-block cap as data, a unit-testable pure function because Windows has **no CI test
    leg** (`.github/workflows/test-suite.yml:28`, `os: [ubuntu-latest, macos-latest]`).

**Acceptance criterion.** `encode_params_blob` output round-trips through the real vendored
`_base64.lua`/`_json.lua` (T3.4); `params_blob_exceeds_platform_limit` is exact at the boundary.

**Prerequisite.** Task 2 for T3.4's harness. The **fallback** behaviour past the size limit is
`accepted-untested` by decision — no fallback is designed, and designing one is new functionality
this epic is not taking on. If a real document ever exceeds the platform limit, the symptom is a
`pandoc` exec failure surfaced by Task 10's nonzero-exit diagnostic. Task 3 binds the *predicate*
only.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | U | `encode_params_blob` | Encode a fixed payload whose byte length `% 3 == 2` → assert the exact expected string, which **ends in `=`** | none | the `general_purpose::STANDARD` engine selection |
| T3.2 | U | `encode_params_blob` | Encode a fixed payload whose base64 contains both `+` and `/` → assert the output contains `+` and `/` and contains neither `-` nor `_` | none | the same engine selection (swapping to `URL_SAFE`) |
| T3.3 | U | `params_blob_exceeds_platform_limit` | `(32_766, 32_767, 32_768)` → `(false, false, true)` | none | the `len > 32_767` comparison, or the constant |
| T3.4 | **L** | `encode_params_blob` + the real `init.lua`/`_base64.lua`/`_json.lua` | `run_main_lua` with a blob carrying `"quarto2-sentinel": "<uuid>"` and a probe `-L` filter that writes `param("quarto2-sentinel", "ABSENT")` to stderr → assert stderr contains the uuid | **nothing mocked** — the real Lua decoder is the point | the `general_purpose::STANDARD` engine selection |

**Revert hunks, stated exactly:**
- T3.1 — Revert `general_purpose::STANDARD` to `STANDARD_NO_PAD` →
  `assert_eq!(encode_params_blob(VEC_LEN_MOD3_IS_2), "…=")` in
  `test_encoder_is_standard_with_padding` RED.
- T3.2 — Revert `general_purpose::STANDARD` to `URL_SAFE` →
  `assert!(!out.contains('-') && !out.contains('_'))` in `test_encoder_is_standard_alphabet` RED.
- T3.3 — Revert the `32_767` constant to `32_768` →
  `assert!(params_blob_exceeds_platform_limit(32_768))` / `assert!(!…(32_767))` in
  `test_platform_limit_boundary` RED.
- T3.4 — Revert the engine selection to `URL_SAFE` → `assert!(stderr.contains(&uuid))` in
  `test_params_blob_round_trips_through_real_init_lua` RED.

### Refactor-induced vacuity check

This task is where the discipline's Check 2 bites hardest, and **two measured facts change the
test design**:

- **`STANDARD_NO_PAD` is indistinguishable for most payloads.** Measured: a 27-byte payload
  (`27 % 3 == 0`) encodes identically under `STANDARD` and `STANDARD_NO_PAD`, and the no-pad form
  decoded correctly through the real `init.lua` (exit 0, sentinel intact). **So T3.1's fixture
  length is the discriminator, not the assertion text**: the payload must satisfy
  `len % 3 != 0` so the expected string carries `=`. Any future edit to that fixture must re-check
  this. A test written with a `% 3 == 0` payload survives the `STANDARD` → `STANDARD_NO_PAD` revert.
- **`URL_SAFE` is not reliably a crash, and not reliably silent either.** `_base64.lua:112-121`
  builds a pattern from the decoder's own alphabet and does `b64 = b64:gsub(pattern, '')` — it
  **strips** out-of-alphabet characters rather than rejecting them. Measured outcomes: usually a
  hard failure (`_base64.lua:140` arithmetic-on-nil, or `_json.lua:212` parse error, both pandoc
  exit 83), but for at least one reproduced payload the stripped-and-shifted bytes still parsed as
  JSON and the render **succeeded with every key absent** (exit 0, sentinel `ABSENT`). **So an
  `L`-tier test asserting "wrong variant → nonzero exit" is payload-dependent and flaky.** T3.2
  therefore binds the *encoder's alphabet* at the `U` tier (deterministic), and T3.4 binds the
  *positive* round-trip.
- **T3.4's `uuid`.** A hardcoded sentinel value would still discriminate, but a per-run uuid also
  rules out a stale stderr capture from a previous invocation being read. Cheap; keep it.

---

## Task 4: The `QUARTO_FILTER_PARAMS` builder — structural + core keys, the synthetic project value, and the two structurally-required keys

**Scope.** Build the blob the plan's re-derivation specifies: `quartoFilterParams`'s ~28 core keys,
`extractIncludeParams`'s include plumbing, `layoutFilterParams`, `crossrefFilterParams`'s four
non-project keys, the top-level literals, and the synthetic single-file "project" value. Plus the
two keys that are **structurally required**: `quarto-filters` and `language` (P4 owns both — see
the Upstream anchors below).

**Files:**
- `crates/quarto-core/src/pandoc_filters/params.rs` — new (ours). `FilterParamsBuilder` with a
  `Vec<Box<dyn FilterParamsContributor>>`-style extension point for P7's format-specific extras
  (the plan: "P4 just needs to plumb an extension point for them").
- `crates/quarto-core/src/pandoc_filters/synthetic_project.rs` — new (ours), or a struct in
  `params.rs`. At minimum `is_single_file: bool` and `dir: PathBuf`, mirroring Q1's non-optional
  `isSingleFile: boolean` (`src/project/types.ts:140` and `:178`) which `quartoFilterParams`
  dereferences **unconditionally** at `src/command/render/filters.ts:663`.
- `crates/quarto-core/src/language.rs` — read-only consumer. `LanguageTerms` at `:128`; the API
  already provides `get` `:140`, `iter` `:164`, `to_config_value` `:214`. Q2 already vendors the
  full upstream locale set at `resources/language/_language*.yml` (111 keys in `_language.yml`), so
  the `language` bag needs no new data.

**Upstream anchors** (`v1.11.3`):
- `filterParamsJson` — `src/command/render/filters.ts:128`; the `params` object literal ends at
  `:201`.
- `languageFilterParams` — `filters.ts:465`; the `-prefix`-from-`-title` derivation loop at `:484`.
- `projectFilterParams` — `filters.ts:498-526`.
- `projType.filterParams` signature — `src/project/types/types.ts:62`.
- `crossrefFilterActive` — `src/command/render/crossref.ts:27`.
- `layoutFilterParams` — `src/command/render/layout.ts:23`.
- `citeIndexFilterParams` — `src/project/project-cites.ts:22` (returns `{}` for every non-book
  render; nothing to build).
- **`language`** — `src/command/render/pandoc.ts:488`,
  `formatFilterParams["language"] = options.format.language;`. **P4 owns this key, not P7** — it
  is not format-specific and not optional, so it belongs in P4's required list rather than behind
  P7's caller-supplied `filterParams` extension point.
- **`quarto-filters`** — consumed at `src/resources/filters/ast/emulatedfilter.lua:45`,
  `for _, v in ipairs(param("quarto-filters").entryPoints) do` — **no default**, called
  unconditionally from `main.lua:735` `inject_user_filters_at_entry_points(quarto_filter_list)`.
  Semantically N/A (Q2 runs user filters itself) but structurally required: P4 emits
  `{"entryPoints": []}`.

**The literal worked example the plan asks for** (measured end-to-end: pandoc 3.8.1 exit 0,
10,795-byte docx). This is the *minimal* blob that makes `main.lua` run to completion for
`--to docx`; the required-key set of Tasks 4 and 5 is a superset of it.

```json
{
  "quarto2-sentinel": "<uuid>",
  "enable-crossref": true,
  "output-divs": true,
  "format-identifier": { "base-format": "docx", "target-format": "docx" },
  "active-filters": { "normalization": true, "crossref": true, "jats_subarticle": false },
  "quarto-filters": { "entryPoints": [] },
  "language": { "...": "all 111 keys of resources/language/_language.yml, flat" },
  "crossref-fig-title": "Figure",  "crossref-fig-prefix": "Figure",
  "crossref-thm-title": "Theorem", "crossref-thm-prefix": "Theorem",
  "results-file": "<temp>/results.json",
  "execution-engine": "markdown"
}
```

**The AST side has a requirement too, and it is not a param.** The Pandoc `Meta` must carry
`quarto_pandoc_reader_opts` (an empty `MetaMap` suffices):
`normalize/capturereaderstate.lua:9` does
`readqmd.meta_to_options(meta.quarto_pandoc_reader_opts)` unconditionally, and
`readqmd.lua:280-286` indexes the argument. `active-filters.normalization = false` does **not**
gate it — that lever reaches only the single `normalize` entry (`main.lua:263-273`), while
`normalize-capture-reader-state` is a separate sibling entry (`main.lua:275-278`). Without it:
`readqmd.lua:283: attempt to index a nil value (local 'meta')`, exit 83 (measured). **This is
P2's responsibility, not P4's** — it's part of the Meta contract (design doc §8: "P2 owns the
carriage, P7 owns per-format Meta→template mapping"), landed as **P2 Task 7**, which emits an
empty `MetaMap` for `quarto_pandoc_reader_opts`. P4's transport smoke depends on P2 Task 7.

**Acceptance criterion.** For a fixed fixture, `FilterParamsBuilder::build()` produces a blob whose
key set matches a committed `insta` snapshot; the required set (items 1-5 of the plan's
recommendation, plus `quarto-filters` and `language`) is present; the deferred set (item 6) is
absent; `run_main_lua` with the built blob exits 0.

**Prerequisite.** Tasks 2 and 3. The **format-specific extras** (docx's 5 callout-icon params) are
`seam deferred until P7's per-format tail` — Task 4 owns only the extension point and asserts that
an injected test contributor's keys reach the blob.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | U | `FilterParamsBuilder::build` | Build for a fixed docx fixture → `insta` snapshot of the **sorted key list** (not values) | a fixed `Format`/`RenderContext` fixture | any contributor call removed from `build` |
| T4.2 | U | `FilterParamsBuilder::build` + `SyntheticProject` | Build with `is_single_file: true` → assert `crossref-index-file` is **absent**; build with `is_single_file: false` → assert it is **present** | the synthetic project value (a real struct, constructed directly) | the `if !project.is_single_file { insert("crossref-index-file", …) }` branch |
| T4.3 | U | `FilterParamsBuilder::build` | Assert `blob["quarto-filters"]["entryPoints"]` is an empty JSON array (type-checked, not just non-null) | fixture | the `insert("quarto-filters", json!({"entryPoints": []}))` line |
| T4.4 | **L** | the real `main.lua:735` / `emulatedfilter.lua:45` | `run_main_lua` with `quarto-filters` **removed** from the built blob → assert exit `83` and `stderr.contains("emulatedfilter.lua:45")` | none | the same `insert("quarto-filters", …)` line |
| T4.5 | U | `FilterParamsBuilder::build` + `LanguageTerms` | Assert `blob["language"]` is a JSON object with `>= 100` keys and contains `"source-notebooks-prefix"` and `"title-block-author-single"` | a real `LanguageTerms` loaded from `resources/language/_language.yml` | the `insert("language", terms.to_config_value())` line |
| T4.6 | **L** | the real `layout/manuscript.lua:29-30` (via `main.lua`'s `quarto_layout_filters`, `main.lua:630-634`) | `run_main_lua` with `language` **removed** → assert exit `83` and `stderr.contains("manuscript.lua")` | none | the same `insert("language", …)` line |
| T4.7 | **L** | the full builder + the real `init.lua` `param()` | `run_main_lua` with the built blob and a probe filter reading `param("quarto2-sentinel")` → assert the uuid reaches stderr **and** `status.success()` | none | the `insert("quarto2-sentinel", …)` line, or Task 3's encoder engine |
| T4.8 | U | `FilterParamsBuilder::build` | Assert each of `ipynb-title-block-template`, `jats-subarticle-id`, `notebook-context`, `cites-index-file`, `quarto-custom-format`, `reference-location` is **absent** | fixture | any stub contributor added for a deferred key |
| T4.9 | U | `FilterParamsBuilder::build` | Assert `number-sections`, `number-offset`, `number-depth` are **present** (API completeness per design §11) | fixture | the `crossrefFilterParams`-equivalent contributor's three-key insert |
| T4.10 | U | `FilterParamsBuilder` extension point | Register a test contributor emitting `{"t4-probe": 1}` → assert it appears in the built blob, and that it can **override** a core key (matching Q1's spread order, `filters.ts:186` `...filterParams` after `...quartoFilterParams`) | a hand-written test contributor (the *builder* is the unit under test) | the `for c in &self.contributors { … }` loop, or its position in `build` |
| T4.11 | U | the `results-file` / `quarto-environment` literals | Assert `results-file` is an absolute path and `quarto-environment.paths` is an object with the three expected keys | fixture | the top-level-literals block |

**Revert hunks, stated exactly:**
- T4.1 — Revert any one contributor call in `build` (e.g. the `layoutFilterParams` equivalent) →
  the `insta` snapshot assertion in `test_params_blob_key_set_snapshot` RED.
- T4.2 — Revert the `if !project.is_single_file` guard around `crossref-index-file` →
  `assert!(!blob.contains_key("crossref-index-file"))` in
  `test_single_file_project_omits_crossref_index` RED.
- T4.3 — Revert `insert("quarto-filters", json!({"entryPoints": []}))` →
  `assert_eq!(blob["quarto-filters"]["entryPoints"].as_array().unwrap().len(), 0)` in
  `test_quarto_filters_is_an_empty_entrypoints_bag` RED (key absent → index panics).
- T4.4 — Revert the same line → `assert_eq!(outcome.status.code(), Some(83))` in
  `test_main_lua_requires_quarto_filters_key` RED (measured: without it the run **succeeds**, so the
  assertion flips).
- T4.5 — Revert `insert("language", terms.to_config_value())` →
  `assert!(blob["language"].as_object().unwrap().len() >= 100)` in
  `test_language_bag_is_complete` RED.
- T4.6 — Revert the same line → `assert_eq!(outcome.status.code(), Some(83))` in
  `test_main_lua_requires_language_bag` RED.
- T4.7 — Revert `insert("quarto2-sentinel", …)` → `assert!(stderr.contains(&uuid))` in
  `test_built_blob_decodes_inside_pandoc` RED.
- T4.8 — Revert by *adding* a stub contributor for `ipynb-title-block-template` →
  `assert!(!blob.contains_key("ipynb-title-block-template"))` in
  `test_deferred_keys_are_not_stubbed` RED.
- T4.9 — Revert the three-key insert → `assert!(blob.contains_key("number-depth"))` in
  `test_inert_numbering_params_are_still_emitted` RED.
- T4.10 — Revert the contributor loop in `build` → `assert_eq!(blob["t4-probe"], 1)` in
  `test_format_contributor_extension_point` RED; revert its *position* (move it before the core
  contributors) → the override half of the same test RED.
- T4.11 — Revert the top-level-literals block →
  `assert!(Path::new(blob["results-file"].as_str().unwrap()).is_absolute())` in
  `test_top_level_literals` RED.

### Refactor-induced vacuity check

- **T4.4 and T4.6 are the two seams whose expected value is a *failure*, and both were measured
  rather than reasoned.** Before measurement, the plan implies both keys are unnecessary
  (`quarto-filters` "genuinely N/A"; `language` deferred to P7). Both assertions therefore flip
  polarity relative to the plan's own text — which is precisely why they are written as
  exit-83-plus-stderr-substring rather than "the render fails": a bare `assert!(!success())` would
  also pass for the *unrelated* `quarto_pandoc_reader_opts` failure, or for a missing pandoc. The
  stderr substring (`emulatedfilter.lua:45`, `manuscript.lua`) is the "the path was actually
  exercised" assertion.
- **T4.1's snapshot is a key-set snapshot, not a value snapshot.** Values include absolute temp
  paths and the whole 111-key language bag; a value snapshot would be unstable and would bury the
  signal. The *values* that matter are bound individually (T4.2, T4.9, T4.11, and Task 5's family).
- **T4.9 asserts presence of the inert numbering params — and presence is all it can assert.**
  Design §11 states these three are required for API completeness but inert under external mode,
  because the only Lua that reads them
  (`quarto_crossref_filters`, gated at `main.lua:718` `if enableCrossRef then`) does not run. A test
  that asserted they *change the output* would be asserting a behaviour that by design does not
  exist. Their inertness is `accepted-untested` — see **Missing-test pass**.
- **T4.5's `>= 100` bound.** `_language.yml` has 111 keys today; an exact `== 111` would redden on a
  routine locale addition. `>= 100` plus two named keys discriminates "the bag is real" from "the bag
  is a stub" (the failure mode measured at `manuscript.lua` and again at `authors.lua:854-864`, both
  of which index specific keys) without being brittle.

---

## Task 5: The `crossref-<type>-title` **and** `-prefix` families — Q2's registry becomes authoritative

**Scope.** Emit both key families, one pair per registered ref-type, so Q2 owns the caption label
*and* the reference-text label — the implementation of the "Q2 owns presentation defaults"
standing principle.

**Files:**
- `crates/quarto-core/src/pandoc_filters/params.rs` — the crossref-family contributor (ours).
- `crates/quarto-core/src/crossref/registry.rs` — **needs a new public accessor.**
  `RefTypeRegistry` is at `:37` with a **private** `entries: HashMap<String, RefTypeDef>` field and
  **no iterator** in its public API (`localize_builtin_display_names` `:115`, `builtin` `:131`,
  `contains` `:148`, `len` `:153`, `is_empty` `:159`, `get` `:164`, `classify_cite_id` `:178`,
  `register_custom` `:188`, `extend_from_metadata` `:219`, `extend_from_promised` `:240`). Add
  `pub fn iter(&self) -> impl Iterator<Item = (&str, &RefTypeDef)>`.
- `crates/quarto-core/src/language.rs` — read-only. `crossref_title` `:151`,
  `crossref_prefix` `:158`.

**The derivation rule, and why it is not "from `RefTypeRegistry`" alone.** `RefTypeDef`
(`registry.rs:43-58`) has exactly two string fields: `ref_type` `:46` (*the id prefix*, e.g. `"fig"`)
and `kind` `:50` (*the display name*, e.g. `"Figure"`). **There is no display-*prefix* field.** And
`resources/language/_language.yml` carries `crossref-<t>-prefix` only for `ch` `:96`, `apx` `:97`,
`sec` `:98`, `eq` `:99` — never for `fig`/`tbl`/`lst` or the eight theorem-family types. So the
implementable rule mirrors `filters.ts:484` exactly:

```
for (ref_type, def) in registry.iter():
    title  := def.kind                                                    # already localized by
                                                                          # localize_builtin_display_names
    prefix := terms.crossref_prefix(ref_type).unwrap_or(title)            # derive from title
    emit "crossref-{ref_type}-title"  = title
    emit "crossref-{ref_type}-prefix" = prefix
```

**Why both families, measured.** `title(type, default)` (`crossref/format.lua:4-7`) reads
`crossref-<type>-title` and feeds **captions**. `refPrefix(type, upper)`
(`crossref/format.lua:66-79`) reads `param("crossref-" .. type .. "-prefix")` **first**, then
`crossref.categories.by_ref_type[type].prefix`, then the bare literal `type .. "."` — and feeds
**reference text**. `crossref.categories` (`mainstateinit.lua`, `ref_type` entries at `:40-108`,
`by_ref_type` built at `:125`) carries only `fig, tbl, lst, nte, wrn, cau, tip, imp, prf, rem, sol`
— **no theorem-family entry at all**. Observed output for a document containing `See @thm-p and
@fig-x.` plus `::: {#thm-p .theorem}`, rendered to docx through the real `main.lua`:

| params supplied | reference text | caption |
|---|---|---|
| neither | `thm. 1` | `Theorem 1` |
| `crossref-thm-title` only | `thm. 1` | `THMTITLE 1` |
| `-title` **and** `-prefix` | `THMPREFIX 1` | `THMTITLE 1` |

**Acceptance criterion.** For every type in `RefTypeRegistry`, both keys are present; and in the
`L`-tier render, a `@thm-` reference carries the value supplied via `-prefix`, not `thm.`.

**Prerequisite.** Task 4. The `L` test uses a **hand-written Pandoc-JSON AST in Q1's own raw syntax**
(`::: {#thm-p .theorem}`), not Q2's wire format — Q2's theorem sugar produces a `CustomNode` that
only P5's shim can convert. So T5.3 binds *the param mechanism*, explicitly **not** Q2's production
of theorems: `seam deferred until P5's shim implementation` for the wire-format path.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | U | the crossref-family contributor + `RefTypeRegistry::iter` | Build with the default registry (21 builtins, `registry.rs:78` `BUILTINS`) → for **every** `ref_type` assert **both** `crossref-<t>-title` and `crossref-<t>-prefix` are present; assert the emitted pair count `== 2 * registry.len()` | a real `RefTypeRegistry::builtin()` | the `emit "crossref-{ref_type}-prefix"` line |
| T5.2 | U | the same, value direction | With a `LanguageTerms` whose `crossref-thm-title` is `"Satz"` and which has no `crossref-thm-prefix`, assert `crossref-thm-title == "Satz"` **and** `crossref-thm-prefix == "Satz"` (derived) | a hand-built `LanguageTerms` | the `.unwrap_or(title)` fallback in the prefix derivation |
| T5.3 | **L** | the real `crossref/format.lua:66-79` `refPrefix` | `run_main_lua` on the raw-syntax `thm` fixture with `crossref-thm-prefix = "THMPREFIX"` → convert the docx back with `pandoc -f docx -t plain` and assert the body contains `"THMPREFIX 1"` and **does not** contain `"thm. 1"` | none | the `emit "crossref-{ref_type}-prefix"` line |
| T5.4 | **L** | the real `crossref/format.lua:4-7` `title` | Same fixture with `crossref-thm-title = "THMTITLE"` → assert the caption/environment line contains `"THMTITLE 1"` | none | the `emit "crossref-{ref_type}-title"` line |
| T5.5 | U | `RefTypeRegistry::iter` | Register a custom type via `register_custom` `:188`, then build → assert its pair is emitted too (the principle "generalizes for free to every registered ref-type") | a real registry | the `iter()` accessor, or replacing it with a hardcoded builtin list |

**Revert hunks, stated exactly:**
- T5.1 — Revert the `emit "crossref-{ref_type}-prefix"` line → `assert_eq!(pairs, 2 * len)` in
  `test_both_crossref_families_are_emitted` RED.
- T5.2 — Revert `.unwrap_or(title)` to `.unwrap_or_default()` (or to omitting the key) →
  `assert_eq!(blob["crossref-thm-prefix"], "Satz")` in
  `test_prefix_derives_from_title_when_absent` RED.
- T5.3 — Revert the `-prefix` emit line → `assert!(plain.contains("THMPREFIX 1"))` in
  `test_thm_reference_uses_prefix_param` RED, **and** the companion
  `assert!(!plain.contains("thm. 1"))` RED — measured: the observed fallback is exactly `thm. 1`.
- T5.4 — Revert the `-title` emit line → `assert!(plain.contains("THMTITLE 1"))` in
  `test_thm_caption_uses_title_param` RED (observed fallback: `Theorem 1`).
- T5.5 — Revert `RefTypeRegistry::iter` to a hardcoded builtin list →
  `assert!(blob.contains_key("crossref-mytype-prefix"))` in
  `test_custom_ref_types_get_both_keys` RED.

### Refactor-induced vacuity check

Three separate collapses are live here:

1. **"Some crossref param is set" is not a discriminator.** A test asserting
   `blob.keys().any(|k| k.starts_with("crossref-"))`, or asserting a `-title` key alone, passes
   if `-title` is emitted but `-prefix` is unfed. T5.1's discriminator is
   the **paired** count `2 * registry.len()`; T5.3's is the **reference text**, which is the only
   surface where the two states differ (measured: caption is identical in the `-title`-only and
   both-keys states).
2. **The expected *value* must not equal Q1's own fallback.** Measured non-discriminating cases:
   - `crossref-fig-prefix = "Figure"` — Q1's `by_ref_type["fig"].prefix` is already `"Figure"`, so
     omitting the key produces the identical output. **`fig` cannot be the fixture type.**
   - `crossref-thm-title = "Theorem"` — Q1's static `theorem_types` default is also `"Theorem"`, so
     a `-title` test on `thm` with the English value is likewise non-discriminating.
   - `crossref-thm-prefix` is the **one** pair member whose Q2 English default (`"Theorem"`) differs
     from Q1's fallback (`"thm."`). It is the only naturally-discriminating value in the family.
   Rather than rely on that single coincidence, **T5.3/T5.4 use synthetic values (`THMPREFIX`,
   `THMTITLE`) that differ from every Q1 fallback**, and `thm` as the type because its `-prefix`
   fallback (`thm.`) is unmistakable in the observed output. T5.2 uses `"Satz"` for the same reason.
3. **T5.3 must prove it exercised the Pandoc leg, not Q2's HTML renderer.** Asserting on a
   `pandoc -f docx -t plain` round-trip of a file produced by `run_main_lua` is that proof: Q2's
   native HTML renderer hard-codes English crossref presentation (`crossref_render.rs:28-31`, design
   §12) and cannot produce `THMPREFIX` at all.

---

## Task 6: Reserve the `pandoc` error-catalog subsystem (number 18) with its pages and sidebar section

**Scope.** Add the `pandoc` subsystem and its initial `Q-18-*` code set to the catalog, author one
docs page per code, and add the `- section: "pandoc"` block to the errors sidebar — all in the same
commit, as both lint rules require.

**Files** (all ours):
- `crates/quarto-error-catalog/error_catalog.json` — add `Q-18-*` entries. Verified today: the 15
  existing subsystems occupy `{0 internal, 1 yaml, 2 markdown, 3 writer, 5 project, 7 cli, 9 xml,
  10 template, 11 lua, 12 listing, 13 navigation, 14 theme, 15 crossref, 16 extension, 17 include}`;
  **4, 6, 8 and 18 are all unused**; this task claims **18** for the `pandoc` subsystem.
- `docs/errors/pandoc/Q-18-<n>.qmd` — one page per code; template per `docs/errors/README.md`;
  `docs_url` must be exactly `https://quarto.org/docs/errors/pandoc/Q-18-<n>`.
- `docs/_quarto.yml` — a new `- section: "pandoc"` block in the `- id: errors` sidebar, entries
  **ascending by code number** (the `error-docs-sidebar-unlisted` rule enforces intra-section
  numeric order; section *order* is deliberately unpoliced).

**The initial code set** (P4-owned; P5/P6/P7 extend it rather than each inventing a subsystem):
`pandoc` binary not found; `pandoc` present but below the minimum version; `pandoc` exited nonzero
(stderr verbatim); params-blob exceeds the platform environment-block limit. Task 10 decides whether
the `[WARNING]`-passthrough diagnostic reuses the existing `Q-11-1` "Lua Filter Diagnostic" instead
of a new `Q-18-*`; `Q-11-1` (`error_catalog.json:891`) already has two real emitters at
`crates/pampa/src/lua/diagnostics.rs:379` and `:386`.

**Acceptance criterion.** `cargo xtask lint` green — specifically `error-docs-page-missing`
(`crates/xtask/src/lint/error_docs.rs:73`) and `error-docs-sidebar-unlisted`
(`crates/xtask/src/lint/error_docs_sidebar.rs:81`), both invoked from the repo-level block in
`crates/xtask/src/lint/mod.rs:98` and `:103`.

**Prerequisite.** None within P4; Tasks 7 and 10 consume the codes.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | X | `xtask::lint::error_docs::check(workspace_root)` | Run against the real `workspace_root` → zero violations | none | any `docs/errors/pandoc/Q-18-<n>.qmd` page deleted |
| T6.2 | X | `xtask::lint::error_docs_sidebar::check(workspace_root)` | Run against the real `workspace_root` → zero violations | none | the `- section: "pandoc"` block in `docs/_quarto.yml` |
| T6.3 | U | the catalog data + `quarto-error-catalog`'s own provider | Load the catalog → assert every `Q-18-*` code has `subsystem == "pandoc"` and `docs_url == format!("https://quarto.org/docs/errors/pandoc/{code}")` | none | any `Q-18-*` entry's `subsystem` or `docs_url` field |
| T6.4 | U | the catalog data | Assert no subsystem name maps to more than one number and `"pandoc" -> 18` | none | the subsystem number in the new entries |

**Revert hunks, stated exactly:**
- T6.1 — Revert (delete) `docs/errors/pandoc/Q-18-1.qmd` →
  `assert!(violations.is_empty())` in `test_error_docs_pages_complete` RED.
- T6.2 — Revert the `- section: "pandoc"` block in `docs/_quarto.yml` →
  `assert!(violations.is_empty())` in `test_error_docs_sidebar_complete` RED. **This is exactly the
  "no `- section:` block at all" scenario the rule was created for** (`crossref`/`extension`
  historically).
- T6.3 — Revert a `Q-18-*` entry's `docs_url` to the `lua` subsystem's URL shape →
  `assert_eq!(entry.docs_url, expected)` in `test_pandoc_codes_docs_urls` RED.
- T6.4 — Revert a `Q-18-*` entry's subsystem number to `11` →
  `assert_eq!(number_for("pandoc"), 18)` in `test_pandoc_subsystem_number` RED.

### Refactor-induced vacuity check

- **T6.1/T6.2 are the repo-level rules run in *positive* direction, and that is the right tier.**
  A `U` test asserting "the file `docs/errors/pandoc/Q-18-1.qmd` exists" would duplicate the rule
  and drift from it. The rules already have their own unit tests (`error_docs.rs`,
  `error_docs_sidebar.rs` both carry `#[cfg(test)] mod tests`, and `error_docs_sidebar.rs:535,538`
  already use `Q-11-1` as fixture data); this task adds no new rule, so **the rules' existing unit
  tests are the seam** and Tasks 6's own tests are the two positive-direction runs plus the data
  assertions.
- **T6.4's expected value `18` is frozen, not derived.** It must be a literal, not
  `max(existing) + 1` — a computed expectation would silently follow a future renumbering and stop
  discriminating.

---

## Task 7: Pandoc version matrix — comparison logic, the gate, and the `(quarto tag, pandoc version)` pin

**Scope.** A version-comparison function, a gate that turns "absent or too old" into a `Q-18-*`
diagnostic, the pandoc version recorded as part of the vendoring pin, the CI/dev-tooling bumps, and
a `cargo xtask verify` preflight. Binary **location** is already done —
`BinaryDependencies::discover` at `crates/quarto-core/src/render.rs:150` sets
`pandoc: runtime.find_binary("pandoc", "QUARTO_PANDOC")` at `:154`, returning
`Option<PathBuf>` via `SystemRuntime::find_binary`
(`crates/quarto-system-runtime/src/traits.rs:556`, native impl `native.rs:371`).

**Files:**
- `crates/quarto-core/src/pandoc_filters/version.rs` — new (ours).
- `crates/xtask/src/verify.rs` — add a pandoc preflight. Today `verify.rs` has **zero pandoc
  awareness**: `const TOTAL_STEPS: u32 = 14` (`:30`), steps at `:112` (Node preflight), `:122`
  (lints), `:174` (fmt), `:190` (build), `:235` (tree-sitter), `:266` (Rust tests), `:298`
  (ts-packages), `:356` (hub-client build), `:382` (hub-client tests), `:405`, `:431`, `:467`,
  `:541`, `:594`, `:627`.
- `crates/xtask/src/dev_setup.rs` — `check_pandoc()` at `:283`; the floor is an **inline literal**
  `pandoc_version_at_least(&version_str, 3, 6)` at `:295`, and the check is **warn-only and
  returns `()`**, so it cannot fail (`:286-292`, `:302-306`). Promote to a hard failure — this is
  a signature change (`()` → `Result<(), _>` or equivalent), not a one-liner.
- `.github/workflows/test-suite.yml:18` and `.github/workflows/ts-test-suite.yml:18` — both
  `PANDOC_VERSION: "3.8.3"`.
- `resources/pandoc-filters/README.md` — record pandoc `3.10` alongside tag `v1.11.3` (Task 1's
  T1.3 already binds this).
- **Reconcile with `crates/xtask/src/pandoc_check.rs`** — an existing `cargo xtask pandoc-check`
  subcommand (`pub fn run()` at `:45`, wired at `main.rs:232`/`:417`) that reads
  `PANDOC_ORACLE_MIN_VERSION = (3, 6)` and `PANDOC_ORACLE_MAX_VERSION = (3, 10)` out of
  `crates/pampa/tests/integration/test.rs:117-118`. Task 7's `verify` preflight must reconcile
  with this existing subcommand, not become a third source of truth. Note **3.10 is exactly
  pampa's oracle `MAX_VERSION`**, so this task's bump lands on the last value that keeps the four
  oracle tests running; any later bump also needs that constant raised.

**Acceptance criterion.** `pandoc 3.9.9` compares as **below** `3.10`; a version below the floor and
an absent binary each produce their distinct `Q-18-*` code; the four places recording a pandoc
version agree.

**Prerequisite.** Task 6 (the codes).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | U | `version::parse_pandoc_version` + `at_least` | Table: `("3.10", (3,10)) >= (3,10)` true; `("3.9.9", …) >= (3,10)` **false**; `("3.10.1") >= (3,10)` true; `("pandoc 3.8.1\nFeatures: …")` parses to `(3,8,1)`; `("")` → `Err` | none — version **strings**, not a binary | the numeric comparison (e.g. reverting to a `str` comparison, under which `"3.9" > "3.10"`) |
| T7.2 | U | `version::gate(found: Option<&str>) -> Result<(), Diagnostic>` | `Some("3.8.3")` → `Err` carrying the too-old code; `Some("3.10")` → `Ok`; `Some("3.11")` → `Ok` | the version string is injected; the **gate** is the unit under test and is not mocked | the `if !at_least(found, FLOOR) { return Err(too_old) }` branch |
| T7.3 | U | `version::gate` | `None` → `Err` carrying the **not-found** code, distinct from the too-old code | injected `None`, mirroring `find_binary`'s `Option<PathBuf>` | the `None => Err(not_found)` arm |
| T7.4 | X | a repo-level pin-agreement check (new rule, or a `U` test reading the files) | Parse `PANDOC_VERSION` from both workflows, the floor literal from `dev_setup.rs`, and the README's recorded version → assert all equal `PANDOC_PIN` | filesystem reads of in-repo files | any one of the four values changed independently |
| T7.5 | X | `xtask::verify`'s pandoc preflight | Run the preflight helper with an injected absent/too-old version → non-zero/`Err`; with the floor version → `Ok` | injected version string (not a real subprocess) | the preflight call added to `verify::run` |

**Revert hunks, stated exactly:**
- T7.1 — Revert the tuple comparison to a lexicographic string comparison →
  `assert!(!at_least("3.9.9", (3, 10)))` in `test_version_compare_is_numeric` RED (`"3.9.9"` sorts
  after `"3.10"` as a string).
- T7.2 — Revert the floor check branch in `gate` →
  `assert_eq!(gate(Some("3.8.3")).unwrap_err().code(), TOO_OLD)` in `test_gate_rejects_old` RED.
- T7.3 — Revert the `None` arm in `gate` →
  `assert_eq!(gate(None).unwrap_err().code(), NOT_FOUND)` in `test_gate_rejects_absent` RED.
- T7.4 — Revert `.github/workflows/test-suite.yml:18` to `"3.8.3"` →
  `assert_eq!(ci_version, PANDOC_PIN)` in `test_pandoc_pin_agrees_everywhere` RED. **This is the
  guard the plan asks for against "a routine `PANDOC_VERSION` bump reddening every P7 golden with
  no Q1 or Q2 change."**
- T7.5 — Revert the preflight call in `verify::run` →
  `assert!(preflight(Some("3.6.0")).is_err())` in `test_verify_pandoc_preflight` RED (the helper is
  still tested; the *wiring* is what the revert removes, so the test must assert the helper is
  reachable from `run`'s step list — see the vacuity note).

### Refactor-induced vacuity check

- **An assertion against the locally-installed pandoc is not a discriminator.** The dev box here
  reports `pandoc 3.8.1`, CI reports `3.8.3`, and the pin is `3.10` — a test of the form
  `assert!(gate(installed_version()).is_ok())` would be RED locally, RED in CI, and GREEN only on a
  correctly-provisioned machine, i.e. it measures the environment, not the code. **Every version
  test above injects the version string.** The *environment* is asserted separately and only by
  T7.5's preflight wiring and the `L`-tier gate, where failing loudly is the intent.
- **T7.5's revert is a wiring revert, so the assertion must reach the wiring.** Asserting only
  `preflight(...)` behaviour survives removing the call from `verify::run`. The test must therefore
  also assert the preflight appears in `verify`'s ordered step inventory (or that `TOTAL_STEPS`
  incremented to `15` and the preflight owns one of them). Without that second assertion T7.5 is
  vacuous against exactly the CLAUDE.md failure this repo has already had (the CSS lint reaching CI
  twelve days before `verify`, bd-4bu7vwi5).
- **T7.1's `"3.9.9"` row is the load-bearing one.** With a `3.6` floor, every plausible input
  compares the same way under numeric and string comparison; only a `3.10`-or-higher floor makes the
  two disagree. The floor bump from `3.6` to `3.10` is what makes this row discriminate — if the
  floor is ever lowered below `3.10`, re-derive the row.

---

## Task 8: The marked `main.lua` patch — import the shim, splice its group between init and normalize, and ship a placeholder group

**Scope.** Two lines in two regions of the **vendored** `main.lua`, plus a placeholder shim file that
defines the global the splice references. Nothing about conversion — the shim's behaviour is P5's.

**Files:**
- `resources/pandoc-filters/filters/main.lua` — **the vendored copy, patched** (marked, and listed
  in the README's `## Ours vs. pinned`). Two edits:
  1. An `import("./quarto2-shim.lua")` line in the import block. It must come **after** the imports
     that define what the shim calls — `import("./ast/customnodes.lua")` (`main.lua:18`) and the
     `customnodes/*.lua` block — because `import` is `dofile`-based (`main.lua:8-11`,
     `PANDOC_SCRIPT_FILE:match("(.*[/\\])")`) and executes at load time. The block runs `:13-203`.
  2. `tappend(quarto_filter_list, quarto_pandoc_shim_filters)` **between**
     `tappend(quarto_filter_list, quarto_init_filters)` (`main.lua:712`) and
     `tappend(quarto_filter_list, quarto_normalize_filters)` (`main.lua:713`).
     The marked-patch comment must anchor by **group name**, not line number — `main.lua`'s group
     *contents* have been refactored twice in two years while the top-level group *order* has been
     stable since 2023.
- `resources/pandoc-filters/filters/quarto2-shim.lua` — **ours, inside the vendored tree** (the
  inside-the-tree placement is forced: `import()` resolves relative to `PANDOC_SCRIPT_FILE`'s own
  directory, and no `package.path` setup for an outside placement has been scoped). P4 ships the
  **placeholder**; P5 replaces its body.

**The filter-group literal P5 asks P4 to show concretely** (not just the `tappend` line). The entry
shape is taken from `main.lua`'s own groups, e.g. `quarto_normalize_filters` (`main.lua:257-278`):

```lua
-- QUARTO2-PATCH (q2 pandoc-hybrid, P4): placeholder for P5's wire-format shim.
-- Spliced into quarto_filter_list between quarto_init_filters and
-- quarto_normalize_filters -- see resources/pandoc-filters/README.md.
quarto_pandoc_shim_filters = {
  { name = "quarto2-wire-shim",
    filter = {},          -- P5 fills this in; run_emulated_filter short-circuits
                          -- on an empty filter (ast/customnodes.lua:92-94)
    traverser = 'jog',    -- the walker, matching every main.lua entry
    -- NO `traverse = 'topdown'` on the filter table: the contract is BOTTOM-UP,
    -- so a nested wire node (a FloatRefTarget inside a Callout's content slot)
    -- is converted inner-first and the outer constructor receives a real Q1
    -- scaffold. `traverse` is a property of the *filter* table
    -- (ast/runemulation.lua:132 is the only topdown user in the tree);
    -- `traverser` selects the *walker* (ast/customnodes.lua:76-88).
  }
}
```

**Why the position is load-bearing in both directions** (frozen; restated so the test can bind it):
after `quarto_init_filters` because `crossrefOption()` indexes `crossref.options`, which is `nil`
until `init_crossref_options(meta)` runs inside that group; before `quarto_normalize_filters` because
the wire Div **retains its original semantic classes** and Q1's class-keyed dispatcher
(`normalize/astpipeline.lua`, reached from `quarto_normalize_filters`) would otherwise fire
`Callout.parse()`/`Tabset.parse()`/`ConditionalBlock.parse()` on it. Confirmed: the
`active-filters.normalization` lever cannot substitute — it gates only the single `normalize` entry
(`main.lua:263-273`), while `tappend(quarto_normalize_filters, quarto_ast_pipeline())`
(`main.lua:281`) is unconditional, and `normalize-capture-reader-state` (`main.lua:275-278`) is a
separate ungated sibling.

**Acceptance criterion.** With the patch and the placeholder in place, `main.lua` still loads and
`run_main_lua` still produces bytes; the splice line's index in the file is strictly between the two
named `tappend` lines; the placeholder group does not set `traverse = 'topdown'`.

**Prerequisite.** **`seam deferred until P5's shim implementation`** for anything about
*conversion*. P4 can assert **position**, **import order**, **group shape** (including the bottom-up
field) and **no-regression**. P4 **cannot** assert that a wire Div becomes a Q1 scaffold, nor that
`Callout.parse()` fails to fire — with an empty placeholder filter, Q1's dispatcher **will** fire on
any callout-shaped wire Div, producing a mis-rendered but non-crashing document. Task 11's smoke
fixture must therefore be chosen to contain no wire-format custom nodes; see Task 11.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T8.1 | U | the patched, embedded `main.lua` bytes | Find the **line indices** of `tappend(quarto_filter_list, quarto_init_filters)`, `tappend(quarto_filter_list, quarto_pandoc_shim_filters)`, `tappend(quarto_filter_list, quarto_normalize_filters)` → assert `init_idx < shim_idx < normalize_idx`, and that all three were found exactly once | none (compiled-in bytes) | the placement of the `tappend(… quarto_pandoc_shim_filters)` line |
| T8.2 | U | the same bytes | Assert the `import("./quarto2-shim.lua")` line index is **greater** than the `import("./ast/customnodes.lua")` line index and **less** than the last `import(` line index | none | the placement of the shim's `import` line |
| T8.3 | U | the embedded `quarto2-shim.lua` bytes | Assert it assigns `quarto_pandoc_shim_filters`, that the group has `>= 1` entry with a `name` and a `filter`, and that the file contains **no** `traverse = 'topdown'` | none | adding `traverse = 'topdown'` to the group, or removing the `name`/`filter` field |
| T8.4 | **L** | the real patched `main.lua` + the placeholder | `run_main_lua` on the Task 4 worked-example fixture → assert `status.success()` (no load-time regression from the patch) | none | deleting `resources/pandoc-filters/filters/quarto2-shim.lua` |
| T8.5 | U | the patched bytes | Assert every edited region carries the `QUARTO2-PATCH` marker comment, and that the marker text names both group boundaries (`quarto_init_filters`, `quarto_normalize_filters`) rather than a line number | none | the marker comment |
| T8.6 | X | Task 1's `vendored_pandoc_filters` rule | Real tree → zero violations, with `main.lua` and `quarto2-shim.lua` both listed under the README's `## Ours vs. pinned` | none | either README listing |

**Revert hunks, stated exactly:**
- T8.1 — Revert the splice line's position (move it to after
  `tappend(quarto_filter_list, quarto_normalize_filters)`, `main.lua:713`) →
  `assert!(init_idx < shim_idx && shim_idx < normalize_idx)` in
  `test_shim_splice_position` RED. **Presence alone does not discriminate**; see the vacuity check.
- T8.2 — Revert the `import("./quarto2-shim.lua")` line to the top of the import block (before
  `import("./ast/customnodes.lua")`) → `assert!(shim_import_idx > customnodes_import_idx)` in
  `test_shim_import_order` RED.
- T8.3 — Revert the placeholder group by adding `traverse = 'topdown'` →
  `assert!(!shim_src.contains("traverse = 'topdown'"))` in
  `test_shim_group_is_bottom_up` RED.
- T8.4 — Revert (delete) `quarto2-shim.lua` → `assert!(outcome.status.success())` in
  `test_patched_main_lua_still_runs` RED (`import`'s `dofile` on a missing file, pandoc exit 83).
- T8.5 — Revert the `QUARTO2-PATCH` marker comment →
  `assert!(marker_names_both_boundaries)` in `test_patch_markers_anchor_by_group_name` RED.
- T8.6 — Revert either README `## Ours vs. pinned` entry →
  `assert!(violations.is_empty())` in Task 1's `test_real_tree_is_clean` RED.

### Refactor-induced vacuity check

- **"The shim group is present in `quarto_filter_list`" is the canonical collapsed assertion here.**
  P5's plan is explicit that position is *the entire mitigation* for at least three handler
  collisions (`Callout`, `Tabset`, `ConditionalBlock`) and that a future reader who believes
  "class-keyed handlers can't fire on the wrapper" has no reason not to move the shim later. A
  presence test passes for every possible position. **T8.1 therefore asserts ordering of line
  indices, all three anchors matched exactly once, and nothing about presence per se.**
- **T8.3 binds a *negative* (`no traverse = 'topdown'`), which is weak on its own.** It survives a
  refactor that moves traversal direction to a different spelling. Paired with the positive
  assertions (a `name`, a `filter`, and `traverser = 'jog'`) it discriminates the shape P5 needs. The
  *behavioural* consequence of bottom-up — inner-before-outer conversion of a nested wire node,
  P6 Finding 5's Tabset-containing-subfloat fixture — is
  `seam deferred until P5's shim implementation`: with an empty placeholder filter there is nothing
  to convert, so no assertion here can distinguish the two directions by effect.
- **T8.4's `status.success()` is a no-regression assertion, not a feature assertion.** It is green
  both before and after the splice exists; its discriminator is the *deletion of the placeholder
  file*, which is the failure a future re-vendor would actually cause. Recorded as a regression
  guard, and the re-vendor guard proper is T8.6/T1.4.

---

## Task 9: `PandocWriteStage`, the `render_qmd_to_pandoc` entry point, and the Pandoc-leg stage list

**Scope.** The stage that serializes the wire format, shells out to `pandoc`, and **writes its own
output file** — no binary bytes through `PipelineData`/`RenderedOutput`. Plus the sibling entry point
and the stage-list assembly.

**Files** (all ours):
- `crates/quarto-core/src/stage/pandoc_write.rs` — new; `PandocWriteStage`. `#[async_trait(?Send)]`
  per `.claude/rules/wasm.md`.
- `crates/quarto-core/src/pipeline.rs` — add `render_qmd_to_pandoc`, a sibling to
  `render_qmd_to_html` (`:843-849`, returning `RenderOutput { html, diagnostics, source_context }`
  defined at `:166-173`).
- `crates/quarto-core/src/stage/data.rs` — `RenderedOutput` at `:441-465`; `content: String` at
  `:449`; `output_path: PathBuf` at `:445`. The stage returns an **empty** `content` and a
  populated `output_path`; no type change is needed.
- `crates/pampa/src/writers/json.rs` — read-only consumer. The Pandoc-superset mode is
  `JsonConfig { raw: false }` — `JsonConfig` at `:42-81` (plan cited `40-81`; the `derive` is at
  `:41`), `raw` field at `:80`. Entry points: `write_with_config` `:1881`, `write` `:1892`.
  **Do not use `raw: true`** — explicitly non-Pandoc-compatible.
- `crates/quarto-core/tests/integration/pandoc_transport.rs` — extend.

**Note on `pandoc-api-version`.** Pampa's hardcoded `[1, 23, 1]` is set in two places: at
`json.rs:1869` and again in the *streaming* writer at `json.rs:4248-4252`. `write_with_config`/
`write` both go through the streaming writer, so a conditional bump touching only `:1869` would
not change real output — any future bump must touch both. No bump is required for pandoc
3.8.1/3.10 today.

**Acceptance criterion.** `render_qmd_to_pandoc` on a fixture writes a real `.docx` to the requested
path; the returned `RenderedOutput` has an empty `content` and the correct `output_path`; the
serialized JSON handed to pandoc is Pandoc-superset shape (carries `pandoc-api-version`), not pampa's
`raw` envelope.

**Prerequisite.** **P2's wire-format schema** for the serialization step (the epic's stated
P4-after-P2 edge); Tasks 2, 4, 5 for the invocation. The **transform** exclude-list for the
`Pandoc(fmt)` profile is `seam deferred until P1's PipelineProfile work` — P4 owns only the **stage**
list that plugs `PandocWriteStage` in.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T9.1 | U | `PandocWriteStage`'s serializer call | Serialize a fixture AST → assert the JSON has a top-level `pandoc-api-version` key and **no** `pampa-json-format` envelope key | none | the `JsonConfig { raw: false, .. }` construction |
| T9.2 | U | `PandocWriteStage::run`'s return value | Run against a stub `pandoc` path that writes a known file → assert `RenderedOutput.content.is_empty()` and `output_path` equals the requested path | **the `pandoc` binary path only** (a fixture script standing in for the engine, used *solely* to bind the return-value contract — T9.4 does the real run) | the `content: String::new()` assignment, or threading bytes into it |
| T9.3 | I | the Pandoc-leg stage list builder | Build the list for `Pandoc("docx")` → assert it contains `PandocWriteStage` and contains no HTML-writer stage | none | the `PandocWriteStage` push in the stage-list builder |
| T9.4 | **L** | `render_qmd_to_pandoc` end to end, real `pandoc` | Call it on a minimal fixture with `--to docx` → assert the file exists, first 4 bytes `PK\x03\x04`, and `RenderedOutput.content.is_empty()` | **nothing mocked** | the `Command::new(pandoc).arg("-L").arg(main_lua)` invocation |
| T9.5 | U | the temp-JSON path derivation | Run → assert the serialized JSON was written inside a per-render temp directory, not the output directory or CWD | none | the temp-dir construction |
| T9.6 | U | `PandocWriteStage`'s argument assembly | Assert the assembled argv contains, in order, `-f json`, `-t docx`, `--data-dir <share>/pandoc/datadir`, `-L <share>/filters/main.lua`, `-o <output>` | none | any one argument in the assembly |

**Revert hunks, stated exactly:**
- T9.1 — Revert `JsonConfig { raw: false }` to `raw: true` →
  `assert!(json.get("pandoc-api-version").is_some())` in
  `test_stage_serializes_pandoc_superset` RED (raw mode omits `pandoc-api-version`, `json.rs:67-80`).
- T9.2 — Revert `content: String::new()` to reading the output bytes into `content` →
  `assert!(out.content.is_empty())` in `test_stage_does_not_thread_bytes` RED.
- T9.3 — Revert the `PandocWriteStage` push →
  `assert!(stages.iter().any(|s| s.name() == "pandoc-write"))` in
  `test_pandoc_leg_stage_list` RED.
- T9.4 — Revert the `-L <main.lua>` argument (or the whole invocation) →
  `assert_eq!(&bytes[..4], b"PK\x03\x04")` in `test_render_qmd_to_pandoc_writes_docx` RED.
- T9.5 — Revert the temp-dir construction to writing beside the output →
  `assert!(json_path.starts_with(temp_root))` in `test_temp_json_is_in_temp_dir` RED.
- T9.6 — Revert the `--data-dir` argument →
  `assert!(argv.windows(2).any(|w| w == ["--data-dir", expected]))` in
  `test_pandoc_argv_assembly` RED.

### Refactor-induced vacuity check

- **T9.2's fixture-script stand-in is the one place a mock touches the engine, and it must not be
  read as an engine test.** The discipline's named anti-pattern is "simulate the engine in a mock and
  assert success". T9.2 asserts **only** the shape of the value returned — never that a render
  succeeded — and T9.4 runs the real pandoc for the same code path. If T9.4 is ever deleted or
  weakened, T9.2 alone becomes exactly that anti-pattern.
- **T9.4's `PK\x03\x04` is non-discriminating against "the Lua did nothing"**, for the same reason as
  T2.2. The "the Lua actually ran" assertions live in T4.7 (sentinel) and T5.3 (observable param
  effect). Do not strengthen T9.4 into a content assertion; that is P5/P6/P7 territory.
- **T9.1's discriminator is `pandoc-api-version`, not the absence of `s:` keys.** The plan records
  that the superset's extra `astContext`/`s:` keys are *confirmed harmless* to real pandoc, so
  asserting their absence would assert the wrong thing and would also pass under `raw: true`.

---

## Task 10: The `pandoc` subprocess diagnostic — unconditional stderr capture, `[WARNING]` re-emission, verbatim nonzero-exit passthrough, temp-JSON retention

**Scope.** Capture stderr **unconditionally**, re-emit `[WARNING]`-shaped lines as diagnostics on
a *successful* render, wrap stderr verbatim on a nonzero exit, and retain the temp JSON on
failure.

**Files** (all ours):
- `crates/quarto-core/src/pandoc_filters/diagnostics.rs` — new; `classify_pandoc_stderr(&str) ->
  Vec<Diagnostic>` and the exit-code handling.
- `crates/quarto-core/src/stage/pandoc_write.rs` — wire it in.
- `crates/quarto-core/tests/integration/pandoc_transport.rs` — extend.

**Why unconditional matters, measured.** A **successful** (exit 0) render of a fixture whose image is
missing emits exactly `[WARNING] Could not fetch resource img.png: replacing image with description`
on stderr — 77 bytes, exit 0. That is the natural fixture for the success-case discriminator. It is
the same channel through which P3's documented `order`-missing degradation
(`crossref/tables.lua:229`, `floatreftarget.lua:218`/`:272`, all `warn()`-and-skip inside functions
that have already passed their gate) and P5's unrecognized-`type_name` warning arrive — all of them
on zero-exit renders. The plan explicitly invites routing these through the existing `Q-11-1`
"Lua Filter Diagnostic" code (`error_catalog.json:891`) where the shape matches; note `Q-11-1`
already has real emitters (`crates/pampa/src/lua/diagnostics.rs:379`, `:386`).

**Not all stderr is `[WARNING]`-shaped.** Measured: with `QUARTO_FILTER_DEPENDENCY_FILE` unset, a
zero-exit render dumps ~35 KB of `init.lua` source to stderr with no `[WARNING]` prefix. A
classifier that keeps only `[WARNING]` lines would swallow it entirely. Task 2's T2.4 catches the
specific cause; the general "non-`[WARNING]` stderr on a successful render" case needs a verdict —
see **Missing-test pass**.

**Acceptance criterion.** A zero-exit render emitting `[WARNING] …` produces a corresponding
diagnostic; a nonzero-exit render produces an error whose payload contains the pandoc stderr
verbatim (including a Lua traceback) and leaves the temp JSON on disk; a zero-exit render removes it.

**Prerequisite.** Tasks 6 (codes) and 9 (the stage). P5's `proof-missing-type.qmd` fixture depends
on this diagnostic by name; P4 owns the channel, P5 owns the fixture —
`seam deferred until P5's shim implementation` for the Route-R-constructor-crash *fixture*, but
**not** for the channel, which Task 10 binds with a P4-producible failure (T10.4).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T10.1 | U | `classify_pandoc_stderr` | Feed `"[WARNING] Could not fetch resource img.png…\n"` with **exit status 0** → assert exactly one diagnostic, warning severity, message containing the line verbatim | the stderr text and exit code are injected | the `if !status.success()` guard around the capture/classify call in `pandoc_write.rs` |
| T10.2 | U | the nonzero-exit handler | Feed a multi-line Lua traceback with exit 83 → assert the returned `Err`'s payload `.contains()` **every** line of the input, byte-for-byte | injected text/code | the `.stderr(stderr_text)` field on the error construction |
| T10.3 | U | the temp-JSON retention policy | Exit 83 → assert the temp JSON path still exists and the error message names it; exit 0 → assert it is gone | injected exit code, real temp files | the `if status.success() { remove_file(json) }` branch |
| T10.4 | **L** | the real pandoc + real Lua, failing | `run_main_lua` with `language` removed from the blob (the measured `manuscript.lua` crash, exit 83) → assert the surfaced error contains `"manuscript.lua"` **and** the temp JSON is retained | none | the `.stderr(...)` field, or the retention branch |
| T10.5 | **L** | the real pandoc, **succeeding with a warning** | `run_main_lua` on the missing-image fixture → assert `status.success()` **and** that at least one warning diagnostic was produced carrying `"Could not fetch resource"` | none | the `if !status.success()` guard around the capture |
| T10.6 | U | `classify_pandoc_stderr` | Feed empty stderr with exit 0 → assert zero diagnostics (no spurious noise on a clean render) | injected | a classifier that emits a diagnostic per line unconditionally |

**Revert hunks, stated exactly:**
- T10.1 — Revert the unconditional capture to `if !status.success() { … }` →
  `assert_eq!(diags.len(), 1)` in `test_warning_on_successful_render_is_surfaced` RED.
- T10.2 — Revert the error's `.stderr(stderr_text)` field to a generic "pandoc failed" message →
  `assert!(err.to_string().contains(line))` for each traceback line in
  `test_nonzero_exit_wraps_stderr_verbatim` RED.
- T10.3 — Revert the retention branch to an unconditional `remove_file` →
  `assert!(json_path.exists())` in `test_temp_json_retained_on_failure` RED.
- T10.4 — Revert the `.stderr(...)` field → `assert!(err_text.contains("manuscript.lua"))` in
  `test_real_lua_crash_surfaces_traceback` RED.
- T10.5 — Revert the unconditional capture to failure-only →
  `assert!(diags.iter().any(|d| d.message().contains("Could not fetch resource")))` in
  `test_real_successful_render_surfaces_warning` RED.
- T10.6 — Revert `classify_pandoc_stderr` to emit one diagnostic per input line →
  `assert!(diags.is_empty())` in `test_clean_stderr_produces_no_diagnostics` RED.

### Refactor-induced vacuity check

- **The policy is *unconditional* stderr capture, not *stderr-on-nonzero-exit-only*.** A test
  asserting "stderr appears when pandoc exits nonzero" **passes under both policies** — it cannot
  discriminate between them. **The discriminator is the success case.** T10.1 (`U`) and
  T10.5 (`L`) are the only two tests in this task whose revert is the `if !status.success()` guard;
  T10.2/T10.3/T10.4 guard the older, already-correct half. This must stay visible in the test
  names: `..._on_successful_render_...` for the discriminators.
- **T10.4's fixture is a P4-producible failure, deliberately.** The plan's motivating example is a
  Route-R constructor crash, which does not exist until P5. Substituting "omit the `language` key"
  reaches the same code path (pandoc exit 83 + a Lua traceback on stderr) with a cause P4 owns. When
  P5 lands, `proof-missing-type.qmd` should be **added**, not substituted — T10.4 also serves as the
  regression guard that the channel keeps working if P5's fixture is ever removed.
- **T10.2 asserts every line, not "contains a traceback".** A substring test for `"stack traceback"`
  would pass for a truncated capture; verbatim-ness is the actual contract ("wrap its stderr
  verbatim in the diagnostic").

---

## Task 11: The transport smoke — "run `main.lua`, get bytes", with the blob-decoded check

**Scope.** P4's reviewed deliverable: one fixture, the real params builder, the real materialized
tree, the real `pandoc`, through `render_qmd_to_pandoc`. Plus a cheap check that the blob
actually decoded.

**Files** (all ours):
- `crates/quarto-core/tests/integration/pandoc_transport.rs` — the smoke.
- `crates/quarto-core/tests/fixtures/pandoc_transport/smoke.qmd` — new fixture.

**Fixture constraint, forced by Task 8's prerequisite.** With only the **placeholder** shim in place,
any wire-format custom node reaching `main.lua` keeps its original semantic classes and Q1's
class-keyed dispatcher **will** fire on it in `quarto_normalize_filters`, producing a mis-rendered
but non-crashing document. So `smoke.qmd` must contain **no** construct
that Q2's sugar transforms turn into a `CustomNode` — no callout, no tabset, no theorem/proof, no
`FloatRefTarget`-eligible figure with an id. A heading, prose, a plain code block and a plain image
are safe. A `// P4 CONSTRAINT:` comment in the fixture should say why, so P5 knows it is free to
enrich it once the shim exists.

**Acceptance criterion.** `cargo nextest run -p quarto-core -E 'binary(integration) &
test(pandoc_transport::)'` is green; the produced `.docx` is a valid ZIP; the sentinel param
round-tripped; stderr is quiet.

**Prerequisite.** Tasks 2, 3, 4, 5, 8, 9, 10.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T11.1 | **L** | `render_qmd_to_pandoc` + the real builder + the real tree + the real `pandoc` | Render `smoke.qmd` to docx → assert exit success, file exists, first 4 bytes `PK\x03\x04`, and `word/document.xml` is extractable from the ZIP | **nothing mocked** | the `-L <main.lua>` argument (or `bundle::share_path()`) |
| T11.2 | **L** | the blob-decoded check | Same render, with a standalone probe `-L` filter appended that reads `param("quarto2-sentinel")` and writes it to a path from the env → assert the sentinel param was observed **inside pandoc's Lua** | none | Task 3's `general_purpose::STANDARD` engine selection |
| T11.3 | **L** | stderr quietness on the canonical fixture | Same render → assert no diagnostics of error severity and `stderr` contains no `"stack traceback"` | none | any of Tasks 2/4/5's env or key inserts |
| T11.4 | U | the fixture's own constraint | Parse `smoke.qmd` → assert it contains none of `:::`, `{.callout`, `{#thm-`, `{#fig-` (the constraint Task 8's prerequisite forces) | none | enriching the fixture with a callout before P5's shim exists |

**Revert hunks, stated exactly:**
- T11.1 — Revert the `-L <main.lua>` argument in `PandocWriteStage` →
  `assert_eq!(&bytes[..4], b"PK\x03\x04")` stays GREEN (pandoc alone still makes a docx), so the
  binding assertion is the ZIP-entry one:
  `assert!(zip.by_name("word/document.xml").is_ok())` — also GREEN. **See the vacuity check: T11.1
  cannot bind the Lua leg**; T11.2 is what does.
- T11.2 — Revert `general_purpose::STANDARD` to `URL_SAFE` in `params_codec.rs` → the sentinel
  assertion in `test_transport_smoke_blob_decoded` RED.
- T11.3 — Revert the `cmd.env("QUARTO_SHARE_PATH", …)` line →
  `assert!(!stderr.contains("stack traceback"))` RED.
- T11.4 — Revert (enrich) `smoke.qmd` with a `::: {.callout-note}` block →
  `assert!(!src.contains("{.callout"))` in `test_smoke_fixture_has_no_custom_nodes` RED.

### Refactor-induced vacuity check

- **"Get bytes" is precisely the assertion the measured failure mode satisfies.** A docx is
  produced by `pandoc -f json -t docx` with **no** `-L` at all, and
  (measured) can also be produced with a params blob whose every key silently decoded to absent. So
  T11.1's byte and ZIP assertions are **shape/gating only** — they confirm transport, not that the
  vendored Lua configured anything. **T11.2 carries the whole discriminator for this task**, and it
  must not be dropped as redundant with T3.4: T3.4 binds the codec against a hand-written blob; T11.2
  binds the *built* blob through the *production* entry point.
- **T11.2's mechanism is a standalone probe `-L` filter.** It works because **`init.lua` defines
  `param()` from `--data-dir` for every filter in the chain**, verified by reproduction — so it
  needs no `main.lua` state and, critically, **no dependency on P5's shim existing**. Two things
  the implementer must preserve: the probe is a *test* artifact and must not be added to the
  placeholder shim file Task 8 creates (keeping P5's inherited production file free of assertions
  it did not write); and the probe must write its observation out (a path from the env) rather than
  `assert`ing inside Lua, because a Lua-side `assert` surfaces as a pandoc nonzero exit that
  Task 10's diagnostic would attribute to the wrong cause.
- **The CLI end-to-end verification CLAUDE.md requires is not available to P4.**
  `cargo run --bin q2 -- render smoke.qmd --to docx` is rejected by the format check at
  `crates/quarto/src/commands/render.rs:680-684`; relaxing it is **P7-foundation's** checklist
  item. P4's highest-fidelity entry point is therefore `render_qmd_to_pandoc` in-process. Per
  CLAUDE.md's own instruction, state this explicitly on completion: *"Tests pass, including a
  real `pandoc` subprocess against the vendored Lua; I did not verify through the `q2` binary,
  because the CLI format gate that admits docx is P7-foundation's."*

---

## Missing-test pass

Behaviour with no test above, each given a bound seam or an explicit `accepted-untested`. The
mandated verdicts first.

**1. `pandoc` not found on `PATH`.** **Bound** — T7.3 (`U`), injecting `None` to mirror
`SystemRuntime::find_binary`'s `Option<PathBuf>` (`traits.rs:556`). The *discovery* path itself
(`BinaryDependencies::discover`, `render.rs:150-154`) is pre-existing and already covered by its own
crate's tests; P4 adds only the gate.

**2. `pandoc` present but below the minimum version.** **Bound** — T7.2 (`U`), injected version
strings, with T7.1's `"3.9.9" < "3.10"` row as the numeric-vs-lexicographic discriminator. Asserting
against the installed pandoc is explicitly rejected as environment-measuring.

**3. The `pandoc`-nonzero-exit diagnostic.**
- *Is stderr asserted verbatim?* **Yes, bound** — T10.2 asserts **every input line** appears in the
  error payload, and T10.4 asserts it on a real pandoc exit-83 failure.
- *Is temp-JSON retention asserted?* **Yes, bound** — T10.3 (both polarities) and T10.4 (real).
- *The structural contract P5 relies on.* P5's `proof-missing-type.qmd` depends on exactly this
  channel. T10.4 binds the channel with a P4-producible failure so the contract is guarded **before**
  P5 exists; P5's fixture is added alongside, not in place of it.

**4. The inert `number-sections` / `number-offset` / `number-depth` params.**
- *Presence in the blob:* **bound** — T4.9.
- *Inertness:* **`accepted-untested`: under external mode the only Lua that reads them
  (`quarto_crossref_filters`, gated at `main.lua:718` `if enableCrossRef then`) is not appended to
  `quarto_filter_list` at all, so there is no observable difference between emitting them and not —
  a test asserting inertness would assert the absence of a code path rather than a behaviour, and
  would go green for the wrong reason the moment P3's `crossref-numbering: external` wiring changed
  which group runs.** The guard that actually matters is P3/P6's: that `quarto_crossref_filters` is
  suppressed under external mode. Design §11 also records Gordon's decision *not* to forward these
  to pandoc's own defaults, which is the behaviour a reader might otherwise expect to test.

**5. The synthetic single-file "project" value.**
- *What breaks if it is wrong:* `quartoFilterParams` dereferences `options.project.isSingleFile`
  unconditionally (`filters.ts:663`; the type is non-optional at `src/project/types.ts:140`, `:178`),
  so Q1 TS has no project-less path. A wrong value flips `crossref-index-file` on, pointing Q1's
  crossref pass at a cross-document index file that does not exist.
- **Bound** — T4.2 asserts both polarities, which is what makes it a discriminator rather than a
  presence check.

**6. The re-vendor safety contract (ours vs. pinned).**
- **Bound mechanically, not README-only** — T1.4 (the new repo-level `vendored-pandoc-filters` rule,
  negative direction, in `tempfile`), T1.5 (positive direction against the real tree), T8.6 (the
  `main.lua` patch and the shim file specifically). This is the answer to "is the README the whole
  mitigation": it is not, deliberately, because a delete-and-recopy re-vendor would otherwise
  remove the shim file along with the rest of the `v1.11.3` tree.
- *That the copied bytes came from tag `v1.11.3`:* **`accepted-untested`: verifying byte provenance
  needs a `git` operation against an out-of-tree checkout of quarto-cli, which is exactly the
  `external-sources/`-in-CI dependency CLAUDE.md's External Sources Policy forbids. T1.3 binds the
  recorded pin to the code constant; P5's Layer-1/Layer-2 contract tests are the plan's designated
  drift tripwire.** A `G`-tier check is possible locally and is not worth a task.

**7. `--data-dir` resolution, including the cross-platform angle.**
- *Layout:* **bound** — T2.1 asserts the single-root shape with `Path::join`, never a hardcoded
  separator (`.claude/rules/cross-platform.md`).
- *The env contract:* **bound** — T2.2 (positive, the discriminator), T2.3 (documented failure
  shape), T2.4 (dependency file).
- *Windows specifically:* **`accepted-untested` on Windows, bound as pure logic on the platforms CI
  runs: `test-suite.yml:28`'s matrix is `[ubuntu-latest, macos-latest]` — there is no Windows CI leg
  at all.** Mitigations, all of them required: (a) T3.3 makes the 32,767-char env-block bound a
  `U`-tier pure-function test, exactly as the plan instructs, so it runs on Linux and macOS; (b) all
  path construction in Tasks 2 and 9 must use `Path::join`, and T2.1/T9.6 assert on `PathBuf`s
  rather than on string literals containing `/`; (c) `init.lua:123-131` derives its own separator
  from `package.config:sub(1,1)`, so the Lua side is already platform-aware and needs no patch.
  The residual untested risk is the *materialization* of a 27-file tree plus a 170-file tree into a
  Windows temp directory, and the env-block total once a real `include-in-header` is inlined.

**Further items this pass surfaced (not in the mandated list):**

**8. `main.lua` loads at all after the patch.** **Bound** — T8.4. Worth naming separately because
`import` is `dofile`-based: a typo in the patched path is a load-time failure affecting every render,
not a per-document bug.

**9. Non-`[WARNING]`-shaped stderr on a *successful* render.** Measured: ~35 KB of `init.lua` source
with no `[WARNING]` prefix when `QUARTO_FILTER_DEPENDENCY_FILE` is unset. T2.4 binds that specific
cause; the *general* case — should `classify_pandoc_stderr` surface unrecognized stderr on a zero
exit, or drop it? — is **`accepted-untested` pending a decision, because the plan specifies only the
`[WARNING]` re-emission rule and specifying a catch-all here would be inventing policy.** Recommended
shape if it is decided: emit one `Q-18-*` "unclassified pandoc stderr" diagnostic carrying a truncated
prefix, so the channel is never silent. Flagged so it is not discovered by a user.

**10. Nested wire-node conversion order (the bottom-up contract's *effect*).**
`seam deferred until P5's shim implementation` — with an empty placeholder filter there is nothing to
convert, so no P4 assertion can distinguish bottom-up from topdown by effect. T8.3 binds the *field*;
P6 Finding 5's Tabset-containing-subfloat fixture binds the effect.

**11. The `include-in-header` text-inlining path.** `extractIncludeParams` embeds include-file
**text** into the blob. Task 4 builds the keys, but the size-bound **fallback** is
`accepted-untested` by decision — no fallback is designed, and designing one is new
functionality this epic is not taking on. `accepted-untested: the predicate is bound (T3.3); the
behaviour past the predicate does not exist yet.`

**12. `meta.quarto_pandoc_reader_opts`.** Required in the Pandoc `Meta` (measured crash at
`readqmd.lua:283` via `capturereaderstate.lua:9`). This is **P2 Task 7's** seam, not P4's — see
the AST-side note in Task 4.

**13. `pandoc-api-version` agreement.** `accepted-untested: no bump is required for the pinned pandoc
3.10, and pampa's own oracle tests already exercise the value against a real pandoc. The anchor
correction (two hardcoded sites, `json.rs:1869` and `:4248-4252`) is recorded in Task 9 so a future
bump does not silently miss the live path.`

