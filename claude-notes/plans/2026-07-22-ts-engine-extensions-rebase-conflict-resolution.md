# ts-engine-extensions → main: Squash-then-Rebase Conflict-Resolution Plan

> **For the engineer executing this:** this is a **rebase runbook**, not a
> feature build. The tests already exist; each task is *resolve a conflict →
> rebuild/retest*. Work top-to-bottom. Do **not** push (see Global Constraints).

**Goal:** Land `feature/ts-engine-extensions` on top of current `main`,
resolving all 14 merge conflicts correctly and leaving `cargo xtask verify`
green.

**Precondition (done separately — NOT part of this plan):** the ~515-commit
branch has already been squashed **chronologically into a reasonable dozen (~12)
commits** on top of the merge-base, using the executor's own technique
(`git revise`). This plan starts from those ~12 commits and does **not** perform
any squash — but it imposes **one hard grouping constraint** on how they are
drawn (see next).

**REQUIRED grouping constraint (from the commit analysis):** the
engine-registry/pipeline conflict cluster is **not** contiguous — it lives in
**two tight clusters** in the replay order, ~116 unrelated commits apart:

- **Cluster A — the 06-24 day** (replay positions 74/78/86/87): the `Arc`
  migration + `TsEngineHost` transport + stage wiring. Of the hard files, this
  touches `pipeline.rs` **once** (the `engine_registry` → `Arc` field change),
  plus `quarto-util/lib.rs` (`quarto_runtime_dir`), `stage/mod.rs`,
  `replay_engine.rs`, and `text_execute.rs` (Phase-3.5 enforcement).
- **Cluster B — the 06-30 day** (positions 204–216): stateless
  `EngineExecutionStage` (registry → `ctx.registry`), `EngineClaimsFileStage`,
  `resolve_engines`, failure-model. This touches `pipeline.rs` **five times**
  (the bulk: builder-signature removal, stage reconstruction, the new stage, the
  length/order assertions), plus `stage/mod.rs`, `text_execute.rs` (cede/claim),
  `replay_engine.rs`, and the day's `dev_setup.rs`/`test-suite.yml`.

Collapsing both into one bucket would mean squashing **143 commits (~28% of the
branch)** into one opaque commit — not worth it. **The constraint is therefore
weaker and easier: keep the 06-24 day as one coherent bucket and the 06-30 day
as one coherent bucket — do NOT fragment either day.** Natural day-granular
chronological squashing already satisfies this. The engine conflict then
resolves at **two stops**, and the split is mostly clean:

- **Stop 06-24 owns the `Arc`/field-type half** — small: the `engine_registry`
  field becomes `Arc` (and must coexist with main's already-applied `captures`
  field), plus the Arc test call-sites and `quarto-util` export.
- **Stop 06-30 owns the structural half** — the big one: builder-signature
  removal, engine-stage reconstruction, `EngineClaimsFileStage` insertion, and
  the pipeline length/order assertions.

These are largely *different hunks*, so it is not "resolve the same thing
twice." The genuinely repeated hunks — the `pipeline.rs`/`stage/mod.rs` `use`
blocks and the `engine_registry` field region — are exactly what **`git rerere`
(enabled in this repo)** auto-replays. The one hunk that legitimately resolves
*differently* at each stop is the pipeline-length assertion (23 at stop 06-24
before `EngineClaimsFileStage` exists; 24 at stop 06-30 after it lands) — that is
correct, not a mistake.

**Mechanic:** the rebase-onto-main replays ~12 commits sequentially and **stops
only at the buckets that conflict.** The engine resolution is split across the
**two stops above** (Task 2 is annotated per-stop); the profile-version conflict
is its own stop; the rest are cheap single-touch stops (see "How conflicts
surface across buckets"). Each individual stop is still a 3-way merge; the union
of the two engine stops reproduces what `git merge-tree main HEAD` simulated (the
basis for this plan). Unique merge-base: `61e2d2276` (`perf(hub-mcp): minify the
esbuild bundle`, 2026-06-19). Resolutions hold for `git rebase main` or
`git merge main`.

**The one idea behind almost every hard conflict:** at the merge-base a
*shared* engine-execution/capture subsystem already existed
(`CaptureSpliceStage`, `capture_splice.rs`, the `quarto-preview` crate,
`RenderConfig.engine_registry: Option<EngineRegistry>` threaded through the
pipeline builders). **Both lines then refactored that same subsystem in
structurally incompatible directions:**

- **main** kept the registry as a by-value builder argument and *added
  alongside it* a `captures` field + `build_html_pipeline_stages_with_captures`
  (bd-uy4uygha), plus `LanguageResolveStage` (i18n) and jupyter cell-options.
- **the branch** changed the registry to `Arc`, **removed it from the builder
  signature** and relocated it to `ctx.registry` (read at run time, Task 8),
  and added `EngineClaimsFileStage` before parse (Task 10).

**The resolution rule for the whole engine cluster:** *keep the branch's
structural shape (`Arc<EngineRegistry>` + `ctx.registry`, no registry arg on
the builders) and re-graft main's feature additions (the `captures` field, the
capture builder, `LanguageResolveStage`) onto it.* Never "take theirs" or "take
ours" wholesale — every hard conflict needs the branch's structure **and**
main's new feature.

---

## Global Constraints

- **NEVER push without explicit user permission** (project GIT PUSH POLICY).
  This plan ends at "green locally + commit prepared," not at push.
- **Verify command (final gate):** `cargo xtask verify` (full — the WASM leg is
  in scope because `quarto-core` and `wasm-quarto-hub-client` both change). For
  fast intermediate loops use `cargo build --workspace` then
  `cargo nextest run -p quarto-core`.
- **`.braid/snapshot.jsonl` is regenerated, never hand-merged** (snapshot
  policy). On conflict, take either side then `braid export > .braid/snapshot.jsonl`.
- **Lockfiles are regenerated, never hand-merged:** `Cargo.lock`,
  `crates/wasm-quarto-hub-client/Cargo.lock`, `package-lock.json`.
- Run from the feature worktree: `/Users/gordon/src/q2/.worktrees/ts-engine-extensions`.
- A safety branch already exists: `ts-engine-extensions-backup-prerebase`. If
  this attempt goes wrong, `git rebase --abort` / `git merge --abort` returns
  you to the pre-op state; the backup branch is the belt-and-suspenders copy.

## The 14 conflicting files (merge-tree simulation)

| # | File | Category | Task |
|---|------|----------|------|
| 1 | `crates/quarto-core/src/pipeline.rs` | **Engine cluster (9 hunks)** | 2 |
| 2 | `crates/quarto-core/src/stage/mod.rs` | Engine cluster (import union) | 2 |
| 3 | `crates/quarto-core/tests/integration/replay_engine.rs` | Arc landmine | 2 |
| 4 | `crates/quarto-core/src/document_profile.rs` | **Double version bump** | 3 |
| 5 | `claude-notes/designs/document-profile-contract.md` | Doc reconcile (follows #4) | 3 |
| 6 | `crates/quarto-core/src/engine/jupyter/text_execute.rs` | Cede/claim vs error-policy | 4 |
| 7 | `crates/quarto-core/tests/integration/main.rs` | Test-module list union | 5 |
| 8 | `crates/quarto-util/src/lib.rs` | Export list union | 5 |
| 9 | `crates/xtask/src/dev_setup.rs` | Additive list union | 5 |
| 10 | `.github/workflows/test-suite.yml` | CI config union | 5 |
| 11 | `hub-client/vite.config.ts` | Config union | 5 |
| 12 | `.braid/snapshot.jsonl` | Regenerate | 1 |
| 13 | `Cargo.lock` (+ `crates/wasm-quarto-hub-client/Cargo.lock`) | Regenerate | 1 |
| 14 | `package-lock.json` | Regenerate | 1 |

> 35 further files touched on both sides auto-merge cleanly and need no action.

## How conflicts surface across the ~12 buckets

Under a multi-commit rebase the 14 conflicts do **not** all appear at once —
each surfaces at the bucket that touches its file. Distribution (from the commit
analysis), with the two engine days kept as coherent buckets (option 2):

| Bucket (theme, date) | Conflicts that fire here | Tasks to apply |
|---|---|---|
| **Engine stop A** (Arc/transport, 06-24) | `pipeline.rs` (Arc field only), `stage/mod.rs`, `replay_engine.rs`, `quarto-util/lib.rs`, `text_execute.rs` (Phase-3.5) | **Task 2 steps 3/5/7**, **Task 5** (quarto-util), + Task 1 (locks) |
| **Engine stop B** (stateless stage/claims, 06-30) | `pipeline.rs` (builder sig, stage reconstruction, `EngineClaimsFileStage`, length→24), `stage/mod.rs`, `replay_engine.rs`, `text_execute.rs` (cede/claim), `dev_setup.rs`, `test-suite.yml` | **Task 2 steps 1/2/4/6**, **Task 4**, **Task 5** (dev_setup, yml), + Task 1 (locks) |
| Jupyter-host port (07-01) | `hub-client/vite.config.ts` + maybe lockfiles | Task 5 (vite), Task 1 (locks) |
| Plan6 profile stamping (07-20) | `document_profile.rs` + `document-profile-contract.md` | **Task 3** |
| Several buckets (06-30, 07-02, 07-03, 07-20, 07-22) | `tests/integration/main.rs` (additive `pub mod` lines) | Task 5 (cheap; see note) |
| Any bucket editing manifests | `Cargo.lock`, `package-lock.json`, `.braid/snapshot.jsonl` | Task 1 (defer regen to end) |

**Consequence for execution:** `git rebase main` will pause multiple times.
At each pause, run only the Task step(s) whose files are in that bucket's
conflict set (`git status` shows which), `git add`, then `git rebase --continue`.
**Stop B (06-30) is the one big stop**; stop A and the rest are quick. Because
Task 2 spans both engine stops, follow its per-step **(stop A)** / **(stop B)**
annotations rather than doing it all at once.

- **`tests/integration/main.rs`** is touched in up to 5 buckets, but every touch
  is a disjoint additive `pub mod X;` line — git's line merge usually handles
  these without a conflict at all, and `rerere` (enabled) will auto-replay the
  rare repeat. Just re-take the union each time it does stop.
- **Lockfiles + `.braid/snapshot.jsonl`** may conflict at *several* buckets.
  Do **not** regenerate at every stop — at each pause just clear the marker
  (`git checkout --ours <lockfile>` — under rebase `--ours` is main), `git add`,
  continue; regenerate **once** at the very end (Task 1 Steps 2–4 run after the
  final `--continue`). This avoids N pointless `npm install`/`cargo build` cycles.

---

## Interaction with in-flight plans (plan1c3, plan1a.6)

**Short version: this rebase does not collide with either plan's code.** Both
live entirely outside the conflict surface (which is the 06-24→06-30 engine
bucket plus plan6's 07-20 `document_profile.rs` version work — all older or
already-committed).

- **plan1c3** (`q2 call build-ts-extension` rename + build-lib extraction,
  executing now, branch tip / bucket 14): touches **0** of the 5 hard
  engine-cluster conflict files. Its own targets — `extension/build.rs`,
  `extension/read.rs`, `ts_engine.rs`, the synth fixtures — are **branch-only**
  (main has none of them), so they cannot conflict. **Caveat:** plan1c3 has
  *uncommitted* changes in the worktree right now (`M extension/read.rs`,
  `M quarto/src/main.rs`, `M tests/…/build_ts_extension_e2e.rs`) and sits at
  HEAD. A rebase rewrites every commit hash. **Reach a clean commit boundary —
  finish/commit the current plan1c3 step — before starting the rebase.** Do not
  rebase mid-step.

- **plan1a.6** (off-stdout → loopback-TCP engine-host transport, "next"): **not
  implemented yet** — the branch has only `docs(plan1a.6)` commits plus a
  Phase-0 spike touching `ts_process.rs` (branch-only). Its future
  implementation targets engine-host transport files main does not have, so it
  will not inherit conflicts. **Ideal sequencing: do this rebase in the gap
  *between* plan1c3 and plan1a.6** — then plan1a.6's new code is authored
  directly on top of main, and its `git revise` test-seam citations reference
  post-rebase line numbers.

- **The `document_profile.rs` conflict (Task 3)** is plan6's `engine_resolution`
  field (committed 07-20), not plan1c3 or plan1a.6 — resolving it does not
  disturb either plan's active work.

---

## Task 0: Verify the squashed precondition, then start the rebase

**Files:** none (git plumbing). **Assumes the branch is already ~12 chronological
commits** (produced separately via `git revise`), **with the 06-24→06-30 engine
window collapsed into one commit** (the REQUIRED grouping constraint).

- [ ] **Step 1: Confirm clean tree, location, and a sane squash**

```bash
cd /Users/gordon/src/q2/.worktrees/ts-engine-extensions
git status --short                                          # working tree clean
git merge-base HEAD main                                    # expect: 61e2d2276...
git rev-list --count "$(git merge-base HEAD main)"..HEAD    # expect: ~12 (a dozen-ish)
git log --oneline --reverse "$(git merge-base HEAD main)"..HEAD   # eyeball the buckets
```
  If this is still ~500 commits, the squash has not been done — **stop**. This
  plan does not squash.

- [ ] **Step 2: VERIFY neither engine day was fragmented** (the load-bearing
  check). Under option 2 each hard file is expected to be touched by **at most
  two** commits — one for the 06-24 bucket, one for the 06-30 bucket. More than
  two means a day got split and you'll resolve that file at *three+* stops:

```bash
mb="$(git merge-base HEAD main)"
for f in crates/quarto-core/src/pipeline.rs crates/quarto-core/src/stage/mod.rs \
         crates/quarto-core/src/engine/jupyter/text_execute.rs \
         crates/quarto-core/tests/integration/replay_engine.rs \
         crates/quarto-util/src/lib.rs; do
  n=$(git rev-list --count "$mb"..HEAD -- "$f")
  echo "$n  $f"
done
# And confirm the touching commits collapse to ≤2 distinct dates (the two engine days):
git log --format='%cs' "$mb"..HEAD -- crates/quarto-core/src/pipeline.rs | sort -u
```
  Expected: `≤ 2` for **all five**, and the pipeline.rs dates print as just the
  two engine-bucket days. If any file shows `3+`, go back and re-coalesce that
  day in `git revise` (keep each of 06-24 and 06-30 as one bucket) before
  rebasing. (Collapsing both days into a *single* bucket also works but squashes
  ~143 commits into one — the analysis advises against it; two stops is cheaper.)

- [ ] **Step 3: Confirm the safety net exists**

```bash
git branch --list ts-engine-extensions-backup-prerebase   # should print the branch
# if absent, snapshot the post-squash / pre-rebase tip:
# git branch ts-engine-extensions-backup-prerebase HEAD
```

- [ ] **Step 4: Start the rebase onto main** (it will stop at each conflicting bucket)

```bash
git rebase main
# expect: "CONFLICT ... could not apply <bucket commit>"
git status --short | grep -E '^(UU|AA|DU|UD)'    # the conflicted set FOR THIS STOP
git log -1 --oneline REBASE_HEAD                 # which bucket you're on
```
  Unlike a single-commit rebase, this pauses **several times**. At each pause,
  consult "How conflicts surface across the ~12 buckets" above, apply only the
  Task(s) for the files in *this* stop's conflict set, `git add`, then
  `git rebase --continue`. Keep going until the rebase completes; **Task 7 is the
  final `--continue` + verification.** (Merge topology alternative: `git merge
  main` surfaces everything in one pass; resolve identically and `git commit`.)

---

## Task 1: Regenerated files (lockfiles + braid snapshot)

**Files:** `Cargo.lock`, `crates/wasm-quarto-hub-client/Cargo.lock`,
`package-lock.json`, `.braid/snapshot.jsonl`.

Do **not** hand-edit conflict markers in any of these. Since all four are
regenerated below, it does not matter which side you check out — pick either to
clear the conflict.

> **Rebase vs. merge `--ours`/`--theirs` inversion:** during `git rebase main`,
> `--ours` = **main** and `--theirs` = **your branch** (the reverse of a merge).
> For these regenerated files the choice is irrelevant, but keep the inversion
> in mind for any hunk you resolve by picking a whole side.

- [ ] **Step 1: Clear the conflict on all four (either side), then regenerate**

```bash
# during a rebase, --ours is main; either side is fine since we regenerate:
git checkout --ours Cargo.lock crates/wasm-quarto-hub-client/Cargo.lock package-lock.json .braid/snapshot.jsonl
```

- [ ] **Step 2: Regenerate Cargo lockfiles from the merged manifests**

```bash
cargo build --workspace 2>/dev/null || true   # rewrites Cargo.lock from Cargo.toml set
# wasm lock is refreshed by the hub build later (Task 6); leaving it as main's is fine for now
```

- [ ] **Step 3: Regenerate the npm lockfile**

```bash
npm install    # from repo root per project rule; rewrites package-lock.json
```

- [ ] **Step 4: Regenerate the braid snapshot from the live skein**

```bash
braid export > .braid/snapshot.jsonl
```

- [ ] **Step 5: Stage them**

```bash
git add Cargo.lock crates/wasm-quarto-hub-client/Cargo.lock package-lock.json .braid/snapshot.jsonl
```

---

## Task 2: The engine cluster — `pipeline.rs`, `stage/mod.rs`, `replay_engine.rs`

This is the core of the merge. Apply the resolution rule: **branch structure
(`Arc` + `ctx.registry`, no registry builder arg) + main's features
(`captures`, capture builder, `LanguageResolveStage`).**

> **This task executes across the TWO engine stops** (see the grouping
> constraint). Do the steps in *stop order*, not numeric order:
> - **At stop A (06-24, the Arc/transport bucket):** Steps 3, 5, 7 — the
>   `Arc`/field-type half. `stage/mod.rs` here is a light transport-wiring
>   union (take both sides' additions); `EngineClaimsFileStage` does not exist
>   yet, so leave the import/length work for stop B.
> - **At stop B (06-30, the stateless-stage/claims bucket):** Steps 1, 2, 4, 6 —
>   the structural half.
> - **Steps 8 (build/test) and 9 (stage)** run at the *end of each* stop, then
>   `git rebase --continue`. The 24-stage length test only goes green after
>   stop B; at stop A expect length 23 (main's `language-resolve`, no
>   `EngineClaimsFileStage` yet).

**Files:**
- Modify: `crates/quarto-core/src/pipeline.rs` (9 conflict hunks)
- Modify: `crates/quarto-core/src/stage/mod.rs` (1 hunk)
- Modify: `crates/quarto-core/tests/integration/replay_engine.rs` (1 hunk)

- [ ] **Step 1 (stop B) — `stage/mod.rs` import union (trivial).** Keep **both**
  `EngineClaimsFileStage` (branch) and `LanguageResolveStage` (main) in the
  `pub use` list. Alphabetize; drop the markers.

- [ ] **Step 2 (stop B) — `pipeline.rs` hunk: import list.** Same union — keep
  `EngineClaimsFileStage` *and* `LanguageResolveStage` in the `use` block.

- [ ] **Step 3 (stop A) — `pipeline.rs` hunk: `RenderConfig.engine_registry` field.**
  Take the branch's **`Arc`** type *and* keep main's new **`captures`** field
  and its doc comment. Both must be present:

```rust
    pub engine_registry: Option<std::sync::Arc<crate::engine::EngineRegistry>>,

    /// Server-recorded engine captures to splice into the HTML render
    /// (bd-uy4uygha). When non-empty, a [`crate::stage::CaptureSpliceStage`]
    /// is inserted before [`EngineExecutionStage`] ...
    pub captures: Vec<quarto_trace::EngineCapture>,
```
  Update the corresponding `Default`/constructor so both fields are initialized
  (`engine_registry: None, captures: Vec::new()`).

- [ ] **Step 4 (stop B) — `pipeline.rs` hunks: builder signatures & engine-stage
  reconstruction.** Take the **branch** side (registry comes from
  `ctx.registry`, not a builder argument):
  - `build_html_pipeline_stages_with_options(apply_config)` — **no** registry arg.
  - Engine stage: `EngineExecutionStage::new().with_spliced_engines(...)` — no
    `with_registry` match on a passed-in registry.
  - Where main branched on `config.captures.is_empty()` to pick
    `build_html_pipeline_stages_with_captures`, **keep that branching** but drop
    the `engine_registry` argument main threaded into it (the branch's
    `with_captures` builder likewise reads the registry from `ctx.registry`):

```rust
    let stages = if config.captures.is_empty() {
        build_html_pipeline_stages_with_options(apply_config)
    } else {
        build_html_pipeline_stages_with_captures(apply_config, config.captures.clone())
    };
```
  If `build_html_pipeline_stages_with_captures` does not yet exist on the branch
  in a registry-free form, port main's function body but delete its
  `engine_registry` parameter and any `with_registry(...)` it performed —
  reconstruct the engine stage with `EngineExecutionStage::new()` exactly as the
  branch's `_with_options` does.

- [ ] **Step 5 (stop A) — `pipeline.rs` hunks: the two test call-sites** (`probe_registry`,
  `replay_registry`). Take the branch's Arc form **and** main's struct-update
  syntax (needed because the struct now has the extra `captures` field):

```rust
                engine_registry: Some(std::sync::Arc::new(probe_registry)),
                ..Default::default()
```

- [ ] **Step 6 (stop B) — `pipeline.rs` hunks: pipeline length/order assertions
  (`test_build_html_pipeline_stages`, `test_build_html_pipeline`).**
  **Both sides bumped 22→23 for different reasons; the merged pipeline has BOTH
  new stages, so the real length is 24.** Set the expected order to reflect
  `EngineClaimsFileStage` at `[0]` (branch) and `LanguageResolveStage` inserted
  immediately after `metadata-merge` (main). The correct head of the list:

```rust
        assert_eq!(stages.len(), 24);
        assert_eq!(stages[0].name(), "engine-claims-file");   // Task 10 (branch)
        assert_eq!(stages[1].name(), "parse-document");
        assert_eq!(stages[2].name(), "metadata-merge");
        assert_eq!(stages[3].name(), "language-resolve");     // bd-llhlzd7p (main)
        assert_eq!(stages[4].name(), "include-expansion");
        // ...every subsequent branch index shifts by +1; end with:
        assert_eq!(stages[23].name(), "apply-template");
```
  And `test_build_html_pipeline`: `assert_eq!(pipeline.len(), 24);`.

  > **Do not hand-count the tail.** After the build compiles, run the ordering
  > test (Step 8); it prints the actual sequence on failure. Update each
  > `stages[n]` line to match the observed order, confirming every entry is a
  > stage you *expect* (both new stages present, no accidental drop). This is
  > the TDD gate for this task.

- [ ] **Step 7 (stop A) — `replay_engine.rs` (integration test): Arc landmine.** Take the
  branch's Arc form + struct-update:

```rust
        engine_registry: Some(std::sync::Arc::new(registry)),
        ..Default::default()
```

- [ ] **Step 8 (each stop) — build + run the pipeline tests, iterate on order values**

```bash
cargo build -p quarto-core 2>&1 | tail -30
cargo nextest run -p quarto-core -- test_build_html_pipeline test_build_transform_pipeline_phase_ordering
```
  Expected after fixes: PASS. If the ordering test fails, read the printed
  sequence and correct the `stages[n]` expectations (Step 6 note). If
  `wasm`/`analysis` pipeline counts (`test_build_wasm_html_pipeline` = 19,
  `test_build_analysis_pipeline` = 5) shift because a new stage joined those
  pipelines too, update them to the observed value — again confirming the delta
  is intentional.

- [ ] **Step 9 (each stop): Stage the files resolved at THIS stop, then continue**

```bash
# stage only what this stop touched (git status shows the set), e.g. at stop A:
git add crates/quarto-core/src/pipeline.rs crates/quarto-core/src/stage/mod.rs \
        crates/quarto-core/tests/integration/replay_engine.rs
git rebase --continue    # proceed to the next bucket
```

---

## Task 3: `document_profile.rs` — the double version bump

**Files:**
- Modify: `crates/quarto-core/src/document_profile.rs`
- Modify: `claude-notes/designs/document-profile-contract.md`

Both sides set `DOCUMENT_PROFILE_VERSION = 7` for **different** additions —
main added `authors_structured: Vec<ProfileAuthor>` (title-block parity,
bd-ez0hiowa); the branch added `engine_resolution: Option<ProfileEngineResolution>`
(Plan 6). A "take one side" resolution silently drops a profile field.

- [ ] **Step 1: Keep BOTH new fields** on the profile struct
  (`authors_structured` from main **and** `engine_resolution` from the branch),
  and keep **both** supporting type definitions (`ProfileAuthor` /
  `ProfileEngineResolution`).

- [ ] **Step 2: Bump the version to 8** and merge both changelog doc-comment
  entries under a single `8:` note (do **not** leave two conflicting `7:`
  entries):

```rust
/// - `7`: (superseded — see 8) title-block parity added `authors_structured`;
///   Plan 6 added `engine_resolution`. Both shipped concurrently on separate
///   branches; the merged profile carries both under version 8.
/// - `8`: merge of the two version-7 lines — `authors_structured`
///   (bd-ez0hiowa) + `engine_resolution` (Plan 6) coexist.
pub const DOCUMENT_PROFILE_VERSION: u32 = 8;
```

- [ ] **Step 3: Reconcile `document-profile-contract.md`** — the design doc's
  version table conflicts for the same reason. Record version 8 with both
  fields; keep both prose paragraphs.

- [ ] **Step 4: Build + test the profile**

```bash
cargo build -p quarto-core 2>&1 | tail -20
cargo nextest run -p quarto-core -- document_profile profile_version
```
  Expected: PASS. If a snapshot encodes the profile version, it will need
  updating to 8 — inspect the diff and confirm only the version + the two new
  fields changed (report per the project's snapshot-change rule).

- [ ] **Step 5: Stage**

```bash
git add crates/quarto-core/src/document_profile.rs claude-notes/designs/document-profile-contract.md
```

---

## Task 4: `text_execute.rs` — cede/claim vs. error-policy

**Files:** Modify `crates/quarto-core/src/engine/jupyter/text_execute.rs` (2 hunks).

The branch added multi-engine **cell cede/claim** (filter to owned/executable
cells, empty-passthrough); main added the **cell-error policy** + canonical
`::: {.cell}` emission and threaded `ctx` (not `&ctx.cwd`) into the async call.
Keep both behaviors.

- [ ] **Step 1 — execute-call hunk.** Keep the branch's early
  cede/passthrough guard and its filtered `executable` slice, but call
  `execute_blocks_async` with **main's** argument shape (whatever main changed
  the final param to — e.g. `ctx` instead of `&ctx.cwd`):

```rust
    if executable.is_empty() {
        // All cells were ceded — passthrough unchanged (branch).
        return Ok(ExecuteResult::new(input));
    }
    let kernel_name = map_language_to_kernel(&executable[0].language);
    // main's threading of ctx (confirm the exact param main introduced):
    let result = execute_blocks_async(input, &executable, &kernel_name, ctx);
```
  Verify against main's version which arg it passes; match main's signature but
  keep the branch's `&executable` (not `&blocks`).

- [ ] **Step 2 — code-block collection hunk.** The branch removed the
  `is_executable_language` gate here (it collects all cells and cedes later);
  main kept the gate. **Take the branch's ungated collection** — cede/claim
  supersedes the old executable-only filter — unless main's error-policy relies
  on seeing non-executable blocks, in which case keep the gate and let
  cede/claim run downstream. Decide by reading both surrounding functions; the
  branch's multi-engine model is authoritative for *which* cells execute.

- [ ] **Step 3 — build + test jupyter engine**

```bash
cargo build -p quarto-core 2>&1 | tail -20
cargo nextest run -p quarto-core -- jupyter text_execute cell_options
```
  Expected: PASS. If the cell-options/error-policy tests (from main) fail,
  the collection-gate decision in Step 2 is likely wrong — revisit it.

- [ ] **Step 4: Stage**

```bash
git add crates/quarto-core/src/engine/jupyter/text_execute.rs
```

---

## Task 5: Additive list/config unions (low-risk)

**Files:**
- `crates/quarto-core/tests/integration/main.rs` — `pub mod` list: keep **both**
  sides' new module declarations, alphabetized.
- `crates/quarto-util/src/lib.rs` — keep main's widened export
  `pub use path::{is_rooted, to_forward_slashes};` (superset of base). Confirm
  the branch did not intentionally remove an export; if unsure, keep the union.
- `crates/xtask/src/dev_setup.rs` — keep **both** sides' added setup steps
  (additive list; take the union).
- `.github/workflows/test-suite.yml` — keep both sides' job/step additions.
- `hub-client/vite.config.ts` — keep both sides' config additions.

- [ ] **Step 1: Resolve each by taking the union of both additions** (drop
  markers; no side is discarded). For `quarto-util`, prefer main's
  `{is_rooted, to_forward_slashes}` form.

- [ ] **Step 2: Sanity-build the affected crates**

```bash
cargo build -p quarto-util -p xtask 2>&1 | tail -15
cd hub-client && npx tsc --noEmit -p . ; cd ..    # quick vite/ts config sanity
```

- [ ] **Step 3: Stage**

```bash
git add crates/quarto-core/tests/integration/main.rs crates/quarto-util/src/lib.rs \
        crates/xtask/src/dev_setup.rs .github/workflows/test-suite.yml hub-client/vite.config.ts
```

---

## Task 6: Compile-landmine sweep (the invisible conflicts)

Merge-tree only flags **textual** conflicts. The branch's
`engine_registry: Option<EngineRegistry>` → `Option<Arc<EngineRegistry>>` change
means every main-side call-site that constructs it **by value** compiles-broken
after merge even though it merged cleanly. Known sites (main added these):

- `crates/quarto-preview/tests/integration/eager_capture.rs`
- `crates/quarto-preview/tests/integration/staleness.rs`
- `crates/quarto-preview/tests/integration/diagnostics_capture_failure.rs`
- (plus anything the full build surfaces)

- [ ] **Step 1: Full workspace build — let the compiler find them**

```bash
cargo build --workspace 2>&1 | tee /tmp/mergebuild.log | tail -40
grep -nE "expected .*Arc.*found|engine_registry" /tmp/mergebuild.log
```

- [ ] **Step 2: Wrap each flagged site** `Some(x)` → `Some(std::sync::Arc::new(x))`.
  These are mechanical; the compiler error points at each line.

- [ ] **Step 3: Rebuild until clean**

```bash
cargo build --workspace 2>&1 | tail -20   # expect: Finished, 0 errors
```

- [ ] **Step 4: Stage any files touched**

```bash
git add -A
```

---

## Task 7: Full verification + prepare the commit

- [ ] **Step 1: Rust tests**

```bash
cargo nextest run --workspace 2>&1 | tail -30
```
  Expected: all pass. Any pipeline-order or profile-version failures point back
  to Tasks 2/3.

- [ ] **Step 2: Full verify (includes WASM leg + hub-client + ts-packages)**

```bash
cargo xtask verify 2>&1 | tee /tmp/verify.log | tail -40
```
  Full (not `--skip-hub-build`) because `quarto-core` and
  `wasm-quarto-hub-client` both changed. Expected: PASS.

- [ ] **Step 3: End-to-end smoke of the merged engine path** (per project's
  end-to-end rule — tests alone are not sufficient for a CLI-visible feature):

```bash
cargo run --bin q2 -- render docs/ 2>&1 | tail -20
# and a single multi-engine fixture if one exists, e.g.:
# cargo run --bin q2 -- render <multi-engine-fixture>.qmd && grep -c 'class="cell"' <out>.html
```
  Inspect the output; confirm engine cells render and captures splice as
  expected. Record the invocation + a snippet of observed output here:

  ```
  (paste the verified invocation + output snippet on completion)
  ```

- [ ] **Step 4: Report snapshot changes** (project rule). List every `.snap`
  added/modified/removed, summarize what changed, and flag anything surprising
  (esp. the profile-version snapshot from Task 3).

- [ ] **Step 5: Finalize the rebase** (do NOT push)

```bash
git rebase --continue    # replays the single resolved commit onto main
git log --oneline -3
```
  (If you took the `git merge main` topology instead, run `git commit` here to
  finalize the merge commit.) The squashed commit message is preserved from the
  `git revise` squash; amend it if the resolution warrants a note.

- [ ] **Step 6: Ask the user for push permission.** Stop here. Do not
  `git push` until explicitly approved (project GIT PUSH POLICY).

---

## Rollback

At any point:

```bash
git rebase --abort         # or: git merge --abort (if using the merge topology)
git reset --hard ts-engine-extensions-backup-prerebase   # nuclear: restore the squashed tip
```

## Post-merge follow-ups to watch (not conflicts, but verify they survived)

Main features that auto-merged cleanly but sit next to the rewired pipeline —
spot-check they still work after the engine-cluster resolution:
mermaid diagrams, raw-json reader/writer, i18n term files, title-block-parity
transforms (`authors_normalize`, `date_normalize`, `title_banner`,
`attribution_viewer`, `theorem`), and the jupyter `error`/`output`/`transform`
module split. `cargo xtask verify` covers most; the docs render (Task 7 Step 3)
exercises mermaid + title-block.
