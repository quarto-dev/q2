---
title: 'Parallel project render interleaves cells of different documents on one shared Jupyter kernel (bd-i7yz08uh)'
date: 2026-11-11
description: 'Documents of a project share one Jupyter kernel with no exclusivity, so parallel renders interleave cells and even serial renders leak state across documents; decide between a per-document kernel key (Quarto 1 parity) and a per-session document lease.'
status: implemented  # On branch braid/bd-i7yz08uh-parallel-project-render-interleaves; PR pending
braid:
  strand: bd-i7yz08uh
---

**Checkout:** main checkout, branch `main` @ `78b4863c6` (no topic branch yet; set one up before implementing).
**Implementation approved 2026-11-11; see Implementation below.**

## Triage verdict

**Ready to implement** (design questions answered 2026-11-11). Both the reported interleave and a broader sequential state leak reproduce at HEAD, the root cause is a few lines in `daemon.rs`, and the remaining work is one design choice (how documents map to kernels) plus tests.

## Issue context

P1 bug, filed 2026-11-10 by Carlos Scheidegger while landing GH #1131 (bd-8fmuyh4d, PR #1161). Pass 2 of a project render renders documents on parallel rayon workers; the global `JupyterDaemon` keys sessions on `(kernel_name, cwd)`, so all documents of a project that use `python3` (in one directory) share one kernel, and nothing gives a document exclusive use of it. With two documents whose second cell sleeps and then prints a variable set by the first cell, `QUARTO_JOBS=2` prints `doc a sees owner = b` (or the reverse). The strand names two candidate fixes: a per-session document lease, or keying sessions per concurrent document.

## Dependency graph

- **discovered-from** bd-8fmuyh4d (closed, PR #1161): gave code cells the Quarto 1 execute variables (`QUARTO_DOCUMENT_PATH` etc.). Because kernels are shared, those variables are delivered by a silent per-document setup request at the start of each document (`text_execute.rs`, the `jupyter_setup_code` block). Interleaving makes that unsound: document A's later cells can observe document B's `QUARTO_DOCUMENT_PATH`. So this bug undermines a feature that just shipped.
- **related** bd-hxhnnlzs (closed, PR #491): the kernel-leak fix that introduced the `start_lock`, the shared `engine_runtime`, and `KernelScope` teardown. Its plan (`2026-08-10-jupyter-kernel-leak.md`, design notes) states the intent of cross-render reuse: a warm kernel for fast `q2 preview` re-renders. That rationale concerns re-rendering **the same** document; nothing in it argues for sharing state **across** documents.
- Siblings from bd-8fmuyh4d: bd-x3h4zjhl (TS engines' execute env) and bd-08hfh3ut (brand path). Neither interacts with this.
- No incoming `blocks` edges. The pressure comes from correctness: parallel project render is the default.

## What the code looks like today

- `crates/quarto-core/src/engine/jupyter/daemon.rs:90`: `SessionKey::new(kernel_name, working_dir)`. The key has no document component.
- `daemon.rs:295-306`, `execute_in_session_with`: takes the **daemon-wide** `sessions` write lock for the duration of **one** cell. Two consequences:
  1. Between two cells of document A, document B can run cells on the same kernel. This is the interleave.
  2. Cells on **unrelated** kernels (different documents, different directories, different languages) are serialized too, because the lock covers the whole map, not one session. Parallel rendering of Jupyter documents is therefore almost entirely serial at the cell level today.
- `text_execute.rs:548-556`: each document gets or starts its sessions once, then runs its setup request (`execute_env`) and its cells, each a separate daemon call.
- `ExecutionContext.cwd` (`engine/context.rs:75`) is documented as "typically the directory containing the source document". So documents in **different** directories already get different kernels; the sharing affects documents in the same directory (or whatever `execute-dir` resolves to).
- No production code calls `shutdown_session`; kernels live until `KernelScope` teardown (end of `q2 render`, or exit of `q2 preview`).

### Repro at HEAD (both reproduce)

Fixtures: `claude-notes/plans/jupyter-kernel-doc-isolation-investigation/` (`interleave/` copied from `issue-reports/1131/kernel-interleave`, plus a new `sequential-leak/`). Their configs are named `_quarto.yml.example` so the claude-notes site does not treat them as nested projects; copy each one out and rename the config to `_quarto.yml` before running.

```text
$ QUARTO_JOBS=2 q2 render interleave/        ->  doc a sees owner = a / doc b sees owner = a   (wrong)
$ QUARTO_JOBS=1 q2 render interleave/        ->  doc a sees owner = a / doc b sees owner = b
$ QUARTO_JOBS=1 q2 render sequential-leak/   ->  b sees: defined in a.qmd                      (wrong)
$ QUARTO_JOBS=2 q2 render sequential-leak/   ->  b sees: <undefined>
```

The `sequential-leak` case is **not** in the strand. `b.qmd` reads a global that only `a.qmd` defines. A serial render succeeds only because `a` happened to run first, and the result changes with scheduling. A per-document **lease** fixes the interleave but **not** this leak.

### Quarto 1 behavior (`external-sources/quarto-cli`)

Quarto 1 never shares one kernel's state between documents:

- The default for non-interactive and CI renders is `executeKernelOneshot`, which starts a fresh kernel per document (`src/execute/jupyter/jupyter.ts:473-494`).
- The keepalive daemon (interactive sessions and preview) is keyed by `md5(absolute target path)` (`jupyter-kernel.ts:303-324`, `kernelTransportFile`): one warm kernel **per document**, reused across re-renders of that document. On reuse it restarts the kernel only if the Python executable, the supervisor PID, or a kernel dependency changed (`resources/jupyter/notebook.py:162-191, 326-334`).

## Decisions (2026-11-11, with the user)

1. **Isolation model: per-document key.** `SessionKey` gains the document's path, which matches Quarto 1. This fixes both the interleave and the sequential leak.
2. **`q2 render`: shut a document's kernel down as soon as that document finishes.** A live kernel holds everything the document computed (data frames, models) plus a process and five ZeroMQ ports, so keeping one per document until the run ends could cost a lot on large projects. At most `QUARTO_JOBS` kernels are alive at once. Each document pays one kernel startup, the same cost as Quarto 1's one-shot path.
3. **`q2 preview`: one warm kernel per previewed document, kept indefinitely (Quarto 1 keepalive).** A centralized per-process kernel manager is a possible follow-up; see the feasibility note below.
4. **Re-render of the same document: keep state, never reset.** This is Quarto 1's behavior: its keepalive daemon reuses a document's kernel as is, and restarts it only when the Python executable, the supervising process, or a kernel dependency changed (`notebook.py:162-191, 326-334`). No reset policy is planned.
5. **The daemon-wide lock is fixed here.** Per-document sessions need per-session locking anyway.

### Feasibility: a per-process kernel manager

The architecture already has the skeleton of one. `JupyterDaemon` is a process-wide singleton (`daemon()`, a `OnceLock<Arc<JupyterDaemon>>`). It owns every session, runs all kernel I/O on one shared `engine_runtime()`, and is torn down by the refcounted `KernelScope` (`daemon.rs:370-460`). Commands already open a scope for their own lifetime: `q2 render` at `crates/quarto/src/commands/render.rs:1089` and `:1299`, `q2 preview` at `crates/quarto/src/commands/preview.rs:159`, and the test runner at `crates/quarto-test/src/runner.rs:495`.

What it lacks is **policy**. Today every caller gets the same behavior, decided implicitly by the key and the scope refcount. A manager means giving the daemon an explicit policy that the command sets when it opens its scope:

- release mode: shut down when the document finishes (render), or retain warm (preview);
- an optional cap on warm kernels (LRU), for later;
- a reset-on-reuse choice per kernel language (point 4).

This work needs the release/retain distinction anyway (decision 2 versus decision 3), so it can introduce a small policy type on the scope. LRU and reset can come later without reshaping it.

### Findings: resetting state without restarting the kernel

- **Python: yes, and it is cheap.** `%reset -f` clears the user namespace but leaves `sys.modules` intact. Checked locally with IPython: after `import pandas` and `%reset -f`, `x` is gone, `pandas` is still in `sys.modules`, and re-importing pandas takes about 0.1 ms. It does not undo process-level side effects: `os.chdir`, `os.environ`, `sys.path`, monkeypatches, and matplotlib `rcParams` survive. The per-document setup request already re-sets the execute variables on every run.
- **Julia (IJulia): no equivalent.** Bindings cannot be removed from `Main`; `workspace()` was removed before Julia 1.0. Restarting is the only full reset, and it reloads packages, which is the cost you are worried about. A conceivable middle ground is evaluating each run in a fresh anonymous module, so loaded packages stay loaded but user bindings are new. IJulia evaluates into `Main`, though, so this needs prototyping. **Unverified: Julia is not installed on this machine.** Note also that `{julia}` reaches the Jupyter engine only as a fallback; the dedicated Julia engine extension wins when it is installed (`jupyter/mod.rs`, `claims_language`).
- Under the decisions above, reuse happens only when **the same** document is re-rendered in preview. Keeping state there is Quarto 1's behavior. The visible symptom is a stale binding: a deleted cell's variable still exists. That makes reset a preview-only, per-language follow-up, not a requirement for this fix.

## Proposed phases (draft)

- Phase 0: Tests first, all using the fake kernel so they are CI-safe, and verified failing at HEAD:
  - an interleave test: two documents, where cell 1 sets state and cell 2 reads it, under `QUARTO_JOBS=2` with a barrier;
  - a sequential-leak test at `QUARTO_JOBS=1`;
  - `QUARTO_DOCUMENT_PATH` read in a document's **last** cell names that document;
  - in release mode, a document's kernel is gone once its render returns;
  - in retain mode, a second render of the same document reuses its kernel;
  - two documents on different kernels run concurrently (a lock regression test).
- Phase 1: Per-document `SessionKey` (kernel name, working directory, document path). Decide the document identity for single-file renders and for preview (absolute input path).
- Phase 2: Per-session locking. The map holds `Arc<tokio::sync::Mutex<KernelSession>>`. A document acquires its session once and holds it for its whole run, so the map lock is held only for lookup and insert. Keep `start_lock` semantics for spawn (bd-hxhnnlzs).
- Phase 3: Lifecycle policy on `KernelScope` (release for render, retain for preview), with release at the end of `execute_blocks_inner` on the success **and** error paths.
- Phase 4: Docs. The isolation guarantee goes in `docs/guides/authoring/execution-context.qmd` or the Jupyter page. Update the shared-kernel comments from bd-8fmuyh4d in `text_execute.rs` and the `get_or_start_session` doc comment.
- Phase 5: End-to-end through `q2 render` on both fixtures at `QUARTO_JOBS=1` and `2`, a process check that no kernels remain afterwards, and a manual `q2 preview` check that re-rendering one document reuses its warm kernel.
- Possible follow-up: an LRU cap for preview's warm kernels (not filed; Quarto 1 has none either).

## Risks / tradeoffs (draft)

- Option A raises the number of kernels and kernel startups in a project render. That cost was the reason for sharing, so it needs measuring (startup is about 1–2 s per Python kernel). Keeping each document on its own kernel across the run preserves preview warmth.
- Moving from a map-wide `RwLock` to per-session locks changes `start_lock` and eviction reasoning from bd-hxhnnlzs. Re-run `jupyter_kernel_cleanup` and the orphan checks.
- Windows orphan bd-iy2zbos7 is still open; more kernels means more exposure to that leak until it is fixed.

## Incidental findings

- Pre-flight `cargo xtask verify --skip-hub-build` failed on `quarto-core engine::knitr::capture_tests::capture_classifies_pieces` (`ename` `"rlang_error"` instead of `"simpleError"`). It depends on the environment: the locally installed knitr 1.52 reports a plain `stop("boom")` in rlang style (`Error:\n! boom`). Unrelated to Jupyter; filed as bd-3p8y4gdt.

## Implementation (2026-11-11)

- [x] Phase 0: failing tests, each verified red at `78b4863c6`:
  - fake kernel (`text_execute_isolation.rs`): two documents in one directory ran on one kernel (same PID); the kernel outlived its document;
  - real Python (`crates/quarto/tests/integration/jupyter_document_isolation_e2e.rs`): `doc a sees owner = b` under `QUARTO_JOBS=2`, three runs out of three; `b sees a: True` under `QUARTO_JOBS=1`;
  - `render_leaves_no_kernel_behind` now expects two kernels and observed one.
  The first interleave test (two cells: set; sleep and read) passed at HEAD. The old lock was fair and a document has at most one request waiting, so interleaving needed the other document to arrive in the gap between the two cells. Four cells (set, sleep, filler, read) make it deterministic; the test's doc comment explains why.
- [x] Phase 1: `SessionKey` gains `document` (`ExecutionContext.source_path`).
- [x] Phase 2: the map holds one `Arc<tokio::Mutex<Option<KernelSession>>>` slot per session, and `JupyterDaemon::acquire` returns a `SessionLease` (an owned guard) that the document holds for its whole run. The map's `std::sync::Mutex` is never held across an await. A releasing `finish` retires the slot while it still holds it, so a waiter on the same document starts over on a fresh slot (tested). Kernels start without any shared lock. The old `start_lock` also kept `peek_ports` calls from racing, so a `reserved_ports` set now keeps two starting kernels from being handed the same port; a reservation lasts until the kernel is ready.
- [x] Phase 3: `KernelScope` carries a policy. `kernel_scope()` releases a document's kernel when the document finishes; `kernel_scope_retaining()` (used by `q2 preview`) keeps it. `execute_blocks_with_leases` finishes every lease on the success and error paths.
- [x] Phase 4: docs. `guides/authoring/jupyter-kernels.qmd` gains "Each document has its own kernel", and `execution-context.qmd` drops the shared-kernel paragraph. Comments in `execute_info.rs`, `context.rs`, `render.rs`, `preview.rs` and `jupyter/mod.rs` are updated, and the `QUARTO_JOBS=1` workaround in `execute_env_cli_e2e.rs` is removed.
- [x] Phase 5: end-to-end, inspected by hand (below).

### End-to-end evidence

`target/debug/q2 render <fixture>` on the investigation fixtures (copied out, config renamed to `_quarto.yml`):

```text
interleave       QUARTO_JOBS=1  doc a sees owner = a / doc b sees owner = b   (11.4 s)
interleave       QUARTO_JOBS=2  doc a sees owner = a / doc b sees owner = b   (7.2 s)
sequential-leak  QUARTO_JOBS=1  b sees: <undefined>
sequential-leak  QUARTO_JOBS=2  b sees: <undefined>
```

`q2 preview <proj> --no-browser` with `JUPYTER_RUNTIME_DIR` isolated, driven by headless Playwright. A code edit marks the capture stale; clicking "Re-execute" twice ran `a.qmd`\'s cell on the same kernel (PID 79184 both times) with its state kept (`runs 1`, then `runs 2`), while `b.qmd` had its own kernel (PID 79466). After SIGINT to preview, both kernels were gone and their connection files removed. Note: preview does not re-execute on a prose-only edit, so an observation without a forced re-execution proves nothing.

### Discovered

- bd-wjg6sjxj: 105 orphaned `ipykernel` processes on this machine (Nov 2–11, PPID 1), from processes killed by a signal (hypothesis). Present at HEAD.
