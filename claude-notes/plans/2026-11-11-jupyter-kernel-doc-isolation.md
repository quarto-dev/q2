---
title: 'Parallel project render interleaves cells of different documents on one shared Jupyter kernel (bd-i7yz08uh)'
date: 2026-11-11
description: 'Documents of a project share one Jupyter kernel with no exclusivity, so parallel renders interleave cells and even serial renders leak state across documents; decide between a per-document kernel key (Quarto 1 parity) and a per-session document lease.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-i7yz08uh
---

**Checkout:** main checkout, branch `main` @ `78b4863c6` (no topic branch yet; set one up before implementing).
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Both the reported interleave and a broader sequential state leak reproduce at HEAD, the root cause is a few lines in `daemon.rs`, and the remaining work is one design choice (how documents map to kernels) plus tests.

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

## Proposed phases (draft)

- Phase 0: Tests first. A fake-kernel (CI-safe) two-document project test that fails on interleave (cell 1 sets state, cell 2 reads it, run under `QUARTO_JOBS=2` with a barrier or sleep). A sequential-leak test that fails at `QUARTO_JOBS=1`. A test that `QUARTO_DOCUMENT_PATH` read in a document's **last** cell names that document. Verify all fail at HEAD.
- Phase 1: Change how documents map to kernel sessions (shape depends on Q1 below).
- Phase 2: Make locking per session instead of per map (an `Arc<Mutex<KernelSession>>` per entry, or a held lease), so independent kernels run in parallel.
- Phase 3: Lifecycle. If keying per document, bound the number of live kernels: release or shut down a document's kernel when one-shot render finishes with it, and decide preview's policy.
- Phase 4: Docs. `docs/guides/authoring/execution-context.qmd` (or the Jupyter page) should state the isolation guarantee, and the shared-kernel caveat in the bd-8fmuyh4d code comment should be updated.
- Phase 5: End-to-end through `q2 render` on both fixtures at `QUARTO_JOBS=1` and `2`, plus a manual `q2 preview` check that re-rendering one document still reuses its warm kernel.

## Open design questions for the user

1. **Isolation model.** Should documents ever share kernel state? Option A, **per-document key** (`SessionKey` gains the document path, which matches Quarto 1), fixes both the interleave and the sequential leak. Option B, a **per-session document lease** held across a document's whole run, fixes only the interleave: documents still see each other's globals, and documents that share a kernel serialize. My recommendation is A, unless sharing state across documents was an intended q2 feature.
2. **One-shot render lifecycle under option A.** With 50 Python documents and `QUARTO_JOBS=8`, should `q2 render` shut each document's kernel down as soon as that document finishes (at most `jobs` live kernels, each document pays kernel startup, which is Quarto 1's one-shot cost), or keep them all until the end of the invocation (simpler, but 50 kernels)?
3. **Preview lifecycle under option A.** Keep one warm kernel per previewed document indefinitely (Quarto 1 keepalive), or cap it with an LRU limit? Does preview ever render more than a handful of documents per session?
4. **Restart policy on reuse.** When a warm kernel is reused for a re-render of the same document, Quarto 1 keeps its state (it restarts only if the interpreter or kernel dependencies changed). Keep that semantics, or restart on every re-render for reproducibility at the cost of speed?
5. **Scope of the lock fix.** The daemon-wide write lock also serializes unrelated kernels. Fix it in this strand, since the per-document change requires it anyway, or file it separately?

## Risks / tradeoffs (draft)

- Option A raises the number of kernels and kernel startups in a project render. That cost was the reason for sharing, so it needs measuring (startup is about 1–2 s per Python kernel). Keeping each document on its own kernel across the run preserves preview warmth.
- Moving from a map-wide `RwLock` to per-session locks changes `start_lock` and eviction reasoning from bd-hxhnnlzs. Re-run `jupyter_kernel_cleanup` and the orphan checks.
- Windows orphan bd-iy2zbos7 is still open; more kernels means more exposure to that leak until it is fixed.

## Incidental findings

- Pre-flight `cargo xtask verify --skip-hub-build` failed on `quarto-core engine::knitr::capture_tests::capture_classifies_pieces` (`ename` `"rlang_error"` instead of `"simpleError"`). It depends on the environment: the locally installed knitr 1.52 reports a plain `stop("boom")` in rlang style (`Error:\n! boom`). Unrelated to Jupyter; filed as bd-3p8y4gdt.
